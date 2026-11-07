//! AMD A1 -> future DisplayManager handoff. Diagnostic metadata only.
//!
//! This module intentionally DOES NOT implement ModeBackend or
//! VideoDisplayBackend. No output is connected and no modes are advertised.
#![forbid(unsafe_code)]

use crate::platform::device::DeviceId;
use crate::video::display_contracts::OutputId;

use super::AmdGpuIdentity;
use super::ip_blocks::{AsicIpInventory, IpContractError, IpDiscovery, TypedIpContracts};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmdDisplayBlocker {
    InvalidIpContracts(IpContractError),
    UnverifiedDisplayEngine,
    /// Even verified IP metadata is not firmware, clock, link training,
    /// buffer ownership, MMIO authorization or a tested modeset driver.
    NoNativeDriver,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmdDisplayHandoff {
    pub device: DeviceId,
    pub identity: AmdGpuIdentity,
    pub blocker: AmdDisplayBlocker,
    /// None until a real display driver establishes a generation-safe
    /// connector/output and an actual readback.
    pub output: Option<OutputId>,
    /// No mode is enumerated from vendor ID or theoretical GPU maxima.
    pub advertised_modes: usize,
}

impl AmdDisplayHandoff {
    pub const fn can_modeset(self) -> bool {
        false
    }

    pub const fn can_submit_commands(self) -> bool {
        false
    }
}

/// Gate a future DeviceRegistry-backed AMD display adapter. This function
/// is pure, does not bind to the DeviceRegistry, and does not touch hardware.
pub fn describe_display_handoff(
    device: DeviceId,
    identity: AmdGpuIdentity,
    ip: AsicIpInventory,
) -> AmdDisplayHandoff {
    let blocker = match ip.typed_contracts() {
        Err(error) => AmdDisplayBlocker::InvalidIpContracts(error),
        Ok(TypedIpContracts { display, .. })
            if !matches!(display.dce, IpDiscovery::PresentVerified(_))
                && !matches!(display.dcn, IpDiscovery::PresentVerified(_)) =>
        {
            AmdDisplayBlocker::UnverifiedDisplayEngine
        }
        Ok(_) => AmdDisplayBlocker::NoNativeDriver,
    };
    AmdDisplayHandoff {
        device,
        identity,
        blocker,
        output: None,
        advertised_modes: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::gpu::amd::ip_blocks::{IpKind, IpVersion};
    use crate::video::gpu::amd::pci::{PciAddress, PciIdentity};
    use crate::video::gpu::amd::{AmdGpuProbeStatus, AsicFamily, match_amd_display};

    fn gpu() -> AmdGpuIdentity {
        let pci = PciIdentity {
            address: PciAddress {
                segment: 12,
                bus: 3,
                device: 1,
                function: 0,
            },
            vendor_id: 0x1002,
            device_id: 0x73bf,
            revision: 0x20,
            class_code: 0x03,
            subclass: 0,
            prog_if: 0,
            subsystem_vendor_id: Some(0x1da2),
            subsystem_device_id: Some(0xe438),
        };
        let AmdGpuProbeStatus::DetectedUnsupported(gpu) = match_amd_display(pci) else {
            panic!("must be unsupported");
        };
        gpu
    }

    #[test]
    fn unknown_ip_does_not_create_output_or_modeset() {
        let gpu = gpu();
        let report = describe_display_handoff(
            DeviceId {
                slot: 2,
                generation: 1,
            },
            gpu,
            gpu.ip_inventory(),
        );
        assert_eq!(report.blocker, AmdDisplayBlocker::UnverifiedDisplayEngine);
        assert_eq!(report.output, None);
        assert_eq!(report.advertised_modes, 0);
        assert!(!report.can_modeset());
        assert!(!report.can_submit_commands());
    }

    #[test]
    fn synthetic_verified_engine_is_still_not_an_active_driver() {
        let gpu = gpu();
        let mut ip = AsicIpInventory::unknown(AsicFamily::Navi21);
        assert_eq!(ip.blocks[1].kind, IpKind::DisplayDcn);
        ip.blocks[1].discovery = IpDiscovery::PresentVerified(IpVersion {
            major: 3,
            minor: 2,
            revision: 0,
        });
        let report = describe_display_handoff(
            DeviceId {
                slot: 2,
                generation: 9,
            },
            gpu,
            ip,
        );
        assert_eq!(report.blocker, AmdDisplayBlocker::NoNativeDriver);
        assert_eq!(report.output, None);
        assert_eq!(report.advertised_modes, 0);
        assert!(!report.can_modeset());
    }

    #[test]
    fn inconsistent_ip_metadata_is_not_accepted() {
        let gpu = gpu();
        let mut ip = gpu.ip_inventory();
        let version = IpDiscovery::PresentVerified(IpVersion {
            major: 3,
            minor: 0,
            revision: 0,
        });
        ip.blocks[0].discovery = version;
        ip.blocks[1].discovery = version;
        assert_eq!(
            describe_display_handoff(
                DeviceId {
                    slot: 0,
                    generation: 1
                },
                gpu,
                ip
            )
            .blocker,
            AmdDisplayBlocker::InvalidIpContracts(IpContractError::ConflictingDisplayEngines,)
        );
    }
}
