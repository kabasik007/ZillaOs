//! Pure, allocation-free handoff from the single Platform PCI inventory to AMD A0.
//! This module has no PCI reads, MMIO, DMA, device claims, or init side effects.
#![forbid(unsafe_code)]

use crate::platform::pci::{PciIdentity as PlatformIdentity, PciInventory};
use crate::video::gpu::amd::pci::{
    PciAddress as AmdAddress, PciIdentity as AmdIdentity, PciIdentityError, ReadOnlyPciInventory,
};

/// Lossless field mapping: Platform owns PCI enumeration and immutable snapshots.
/// AMD's A0 value type is retained for source compatibility until its owner
/// can migrate consumers to Platform's canonical identity.
pub fn to_amd_identity(source: PlatformIdentity) -> Result<AmdIdentity, PciIdentityError> {
    let identity = AmdIdentity {
        address: AmdAddress {
            segment: source.address.segment,
            bus: source.address.bus,
            device: source.address.device,
            function: source.address.function,
        },
        vendor_id: source.vendor_id,
        device_id: source.device_id,
        revision: source.revision,
        class_code: source.class.base,
        subclass: source.class.subclass,
        prog_if: source.class.programming_interface,
        subsystem_vendor_id: source.subsystem_vendor_id,
        subsystem_device_id: source.subsystem_device_id,
    };
    identity.validate()?;
    Ok(identity)
}

/// Read-only AMD view over the **existing** Platform inventory.
/// This is not a second PCI scanner or a second DeviceManager.
pub struct AmdInventoryAdapter<'a, const CAP: usize> {
    inventory: &'a PciInventory<CAP>,
}

impl<'a, const CAP: usize> AmdInventoryAdapter<'a, CAP> {
    pub const fn new(inventory: &'a PciInventory<CAP>) -> Self {
        Self { inventory }
    }
}

impl<const CAP: usize> ReadOnlyPciInventory for AmdInventoryAdapter<'_, CAP> {
    fn identity_at(&self, address: AmdAddress) -> Option<AmdIdentity> {
        if !address.is_valid() {
            return None;
        }
        self.inventory
            .iter()
            .find(|entry| {
                let a = entry.identity.address;
                a.segment == address.segment
                    && a.bus == address.bus
                    && a.device == address.device
                    && a.function == address.function
            })
            .and_then(|entry| to_amd_identity(entry.identity).ok())
    }
}


/// Single registry view for AMD diagnostics. This does not own the registry,
/// perform PCI configuration reads, match a driver or request resource claims.
use crate::platform::device::{
    DeviceId, DeviceRegistry, DeviceState, RegistryError,
};
use crate::video::gpu::amd::{
    AmdGpuIdentity, AmdGpuProbeStatus, ip_blocks::AsicIpInventory, match_amd_display,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmdRegistryError {
    Registry(RegistryError),
    InvalidHeader,
    InvalidIdentity(PciIdentityError),
    AlreadyOwned(DeviceState),
    UnsupportedDiagnosticState,
    SnapshotCapacity,
    ChangedIdentity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmdReadOnlyDiagnostic {
    /// Generation-safe canonical registry handle (not a PCI address).
    pub device: DeviceId,
    /// Historical copy; validate against DeviceRegistry before reuse.
    pub identity: AmdGpuIdentity,
    /// Exactly Unknown unless a separate ASIC evidence collection is added.
    pub ip: AsicIpInventory,
}

impl AmdReadOnlyDiagnostic {
    pub const fn label(self) -> &'static str {
        "AMD_GPU_DETECTED_UNSUPPORTED"
    }

    /// A diagnostic is NEVER a driver binding or access token.
    pub const fn may_touch_hardware(self) -> bool {
        false
    }
}

/// Inspect one live, generation-safe DeviceRegistry identity. Only the
/// Platform registry is authoritative; no additional PCI scanner exists.
/// Non-Radeon functions are skipped. Invalid AMD endpoint claims fail closed.
pub fn diagnose_registry_device<const DEVICES: usize, const CLAIMS: usize>(
    registry: &DeviceRegistry<DEVICES, CLAIMS>,
    device: DeviceId,
) -> Result<Option<AmdReadOnlyDiagnostic>, AmdRegistryError> {
    let view = registry.get(device).map_err(AmdRegistryError::Registry)?;
    let canonical = view.info.identity;
    if canonical.vendor_id != 0x1002 {
        return Ok(None);
    }
    // Malformed configuration is an error rather than a fabricated Radeon.
    if canonical.class.base == 0xff {
        return Err(AmdRegistryError::InvalidIdentity(PciIdentityError::InvalidClass));
    }
    if canonical.class.base != 0x03 {
        return Ok(None);
    }
    if view.info.header_type != 0 || view.info.bridge.is_some() {
        return Err(AmdRegistryError::InvalidHeader);
    }
    // A0 is diagnostics only and must not attach to an already claimed
    // function, even if the claiming driver used an AMD vendor match.
    if view.state != DeviceState::Discovered || view.driver.is_some() {
        return Err(AmdRegistryError::AlreadyOwned(view.state));
    }
    let identity = to_amd_identity(canonical).map_err(AmdRegistryError::InvalidIdentity)?;
    let info = match match_amd_display(identity) {
        AmdGpuProbeStatus::DetectedUnsupported(info) => info,
        AmdGpuProbeStatus::InvalidPciIdentity(error) => {
            return Err(AmdRegistryError::InvalidIdentity(error));
        }
        AmdGpuProbeStatus::Absent | AmdGpuProbeStatus::NoMatch => return Ok(None),
        AmdGpuProbeStatus::SupportedByExplicitTable(_) => {
            // This A0 API deliberately cannot advertise a supported board,
            // even if a future table gets populated by another workstream.
            return Err(AmdRegistryError::UnsupportedDiagnosticState);
        }
    };
    Ok(Some(AmdReadOnlyDiagnostic {
        device,
        identity: info,
        ip: info.ip_inventory(),
    }))
}

/// Snapshot enumeration is registry-only. Two passes enforce an all-or-none
/// bounded result: malformed claims or too-small output change no caller
/// array. Does not create new DriverId/DeviceId, rescan PCI, or mutate state.
pub fn diagnose_registry<const DEVICES: usize, const CLAIMS: usize>(
    registry: &DeviceRegistry<DEVICES, CLAIMS>,
    output: &mut [Option<AmdReadOnlyDiagnostic>],
) -> Result<usize, AmdRegistryError> {
    let snapshot = registry.snapshot();
    let mut count = 0usize;
    for view in snapshot.iter() {
        if diagnose_registry_device(registry, view.id)?.is_some() {
            count = count.checked_add(1).ok_or(AmdRegistryError::SnapshotCapacity)?;
            if count > output.len() {
                return Err(AmdRegistryError::SnapshotCapacity);
            }
        }
    }
    output.fill(None);
    let mut used = 0usize;
    for view in snapshot.iter() {
        if let Some(diagnostic) = diagnose_registry_device(registry, view.id)? {
            output[used] = Some(diagnostic);
            used += 1;
        }
    }
    Ok(used)
}

/// Prevent a persisted diagnostic from being reused after registry removal,
/// generation advancement or a changed PCI identity. Still read-only.
pub fn revalidate_diagnostic<const DEVICES: usize, const CLAIMS: usize>(
    registry: &DeviceRegistry<DEVICES, CLAIMS>,
    prior: AmdReadOnlyDiagnostic,
) -> Result<(), AmdRegistryError> {
    let current = diagnose_registry_device(registry, prior.device)?
        .ok_or(AmdRegistryError::ChangedIdentity)?;
    if current.identity != prior.identity {
        return Err(AmdRegistryError::ChangedIdentity);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::pci::{MockConfig, PciAddress, PciClass, PciIdentity, scan_topology};
    use crate::video::gpu::amd::{self, AmdGpuProbeStatus};
    use core::cell::Cell;

    fn identity(vendor_id: u16, device_id: u16) -> PlatformIdentity {
        PlatformIdentity {
            address: PciAddress {
                segment: 0x1234,
                bus: 2,
                device: 7,
                function: 3,
            },
            vendor_id,
            device_id,
            revision: 0xa1,
            class: PciClass {
                base: 3,
                subclass: 0,
                programming_interface: 1,
            },
            subsystem_vendor_id: Some(0x1da2),
            subsystem_device_id: Some(0xe438),
        }
    }

    #[test]
    fn conversion_preserves_segment_bdf_class_revision_and_subsystem() {
        let p = identity(0x1002, 0x73bf);
        let a = to_amd_identity(p).unwrap();
        assert_eq!(a.address.segment, 0x1234);
        assert_eq!(
            (a.address.bus, a.address.device, a.address.function),
            (2, 7, 3)
        );
        assert_eq!(a.vendor_id, 0x1002);
        assert_eq!(a.device_id, 0x73bf);
        assert_eq!(a.revision, 0xa1);
        assert_eq!((a.class_code, a.subclass, a.prog_if), (3, 0, 1));
        assert_eq!(
            (a.subsystem_vendor_id, a.subsystem_device_id),
            (Some(0x1da2), Some(0xe438))
        );
    }

    #[test]
    fn malformed_platform_identifiers_fail_closed_at_amd_boundary() {
        let mut p = identity(0xffff, 0x73bf);
        assert_eq!(to_amd_identity(p), Err(PciIdentityError::InvalidVendor));
        p = identity(0x1002, 0x73bf);
        p.address.function = 8;
        assert_eq!(to_amd_identity(p), Err(PciIdentityError::InvalidAddress));
        p = identity(0x1002, 0x73bf);
        p.subsystem_device_id = None;
        assert_eq!(
            to_amd_identity(p),
            Err(PciIdentityError::IncompleteSubsystem)
        );
    }

    #[test]
    fn vmware_identity_remains_distinct_from_amd_vendor_match() {
        let p = identity(0x15ad, 0x0405);
        assert!(p.is_vmware_svga_ii());
        assert!(!p.is_amd_display_candidate());
        let a = to_amd_identity(p).unwrap();
        assert!(!amd::match_amd_display(a).may_initialize_hardware());
    }

    #[test]
    fn amd_consumes_canonical_bounded_inventory_without_rescanning() {
        let reads = Cell::new(0usize);
        let mut config = MockConfig::new(|addr: PciAddress, offset: u16| {
            reads.set(reads.get() + 1);
            if addr.segment == 0 && addr.bus == 0 && addr.device == 2 && addr.function == 0 {
                return Ok(match offset {
                    0x00 => 0x73bf_1002,
                    0x08 => 0x0300_00a1,
                    0x0c => 0,
                    0x2c => 0xe438_1da2,
                    _ => u32::MAX,
                });
            }
            Ok(u32::MAX)
        });
        let inventory = scan_topology::<4>(&mut config, 0, 0, 0).unwrap();
        assert_eq!(inventory.count, 1);
        let reads_after_scan = reads.get();
        let adapter = AmdInventoryAdapter::new(&inventory);
        let address = AmdAddress::from_legacy_segment_zero(0, 2, 0);
        let status = amd::probe_address(&adapter, address);
        assert!(matches!(status, AmdGpuProbeStatus::DetectedUnsupported(_)));
        assert_eq!(reads.get(), reads_after_scan);
        assert!(
            adapter
                .identity_at(AmdAddress {
                    function: 8,
                    ..address
                })
                .is_none()
        );
        assert!(
            adapter
                .identity_at(AmdAddress {
                    segment: 1,
                    ..address
                })
                .is_none()
        );
        assert_eq!(reads.get(), reads_after_scan);
    }

    fn endpoint(identity: PlatformIdentity) -> crate::platform::pci::PciDeviceInfo {
        crate::platform::pci::PciDeviceInfo {
            identity,
            header_type: 0,
            multifunction: false,
            bridge: None,
        }
    }

    #[test]
    fn canonical_registry_gives_generation_safe_read_only_diagnostics() {
        let mut registry = DeviceRegistry::<4, 0>::new();
        let first = registry.insert(endpoint(identity(0x1002, 0x73bf))).unwrap();
        let mut secondary = identity(0x1002, 0x67df);
        secondary.address.device = 8;
        let second = registry.insert(endpoint(secondary)).unwrap();
        let mut vmware = identity(0x15ad, 0x0405);
        vmware.address.device = 9;
        let third = registry.insert(endpoint(vmware)).unwrap();

        let mut records = [None; 2];
        assert_eq!(diagnose_registry(&registry, &mut records), Ok(2));
        assert_eq!(records[0].unwrap().device, first);
        assert_eq!(records[1].unwrap().device, second);
        assert_eq!(records[0].unwrap().identity.pci.address.segment, 0x1234);
        assert_eq!(records[0].unwrap().identity.pci.revision, 0xa1);
        assert_eq!(records[0].unwrap().identity.pci.subsystem_vendor_id, Some(0x1da2));
        assert!(!records[0].unwrap().ip.all_blocks_verified());
        assert!(!records[0].unwrap().may_touch_hardware());
        assert_eq!(diagnose_registry_device(&registry, third), Ok(None));
        assert_eq!(revalidate_diagnostic(&registry, records[0].unwrap()), Ok(()));

        registry.remove(first).unwrap();
        assert_eq!(
            revalidate_diagnostic(&registry, records[0].unwrap()),
            Err(AmdRegistryError::Registry(RegistryError::StaleHandle))
        );
        let recreated = registry.insert(endpoint(identity(0x1002, 0x73bf))).unwrap();
        assert_ne!(recreated.generation, first.generation);
        assert_eq!(
            diagnose_registry_device(&registry, first),
            Err(AmdRegistryError::Registry(RegistryError::StaleHandle))
        );
    }

    #[test]
    fn registry_claims_and_foreign_drivers_do_not_grant_amd_discovery_rights() {
        use crate::platform::device::{DriverId, DriverMatch};
        let mut registry = DeviceRegistry::<2, 0>::new();
        let id = registry.insert(endpoint(identity(0x1002, 0x73bf))).unwrap();
        let other_driver = DriverId(100);
        registry.match_driver(
            id,
            other_driver,
            DriverMatch::Exact { vendor_id: 0x1002, device_id: 0x73bf, class: None },
        ).unwrap();
        assert_eq!(
            diagnose_registry_device(&registry, id),
            Err(AmdRegistryError::AlreadyOwned(DeviceState::Matched))
        );
        // Metadata-only claim from Platform. The AMD probe must not take over
        // an already bound device or smuggle hardware claims of its own.
        registry.claim(id, other_driver, &[]).unwrap();
        assert_eq!(
            diagnose_registry_device(&registry, id),
            Err(AmdRegistryError::AlreadyOwned(DeviceState::Claimed))
        );
    }

    #[test]
    fn malformed_amd_config_endpoint_and_subsystem_are_rejected() {
        let mut r = DeviceRegistry::<2, 0>::new();
        let mut bad_header = endpoint(identity(0x1002, 0x73bf));
        bad_header.header_type = 1; // bridge configuration is not a GPU endpoint
        let id = r.insert(bad_header).unwrap();
        assert_eq!(diagnose_registry_device(&r, id), Err(AmdRegistryError::InvalidHeader));

        let mut r = DeviceRegistry::<1, 0>::new();
        let mut incomplete = identity(0x1002, 0x73bf);
        incomplete.subsystem_device_id = None;
        let id = r.insert(endpoint(incomplete)).unwrap();
        assert_eq!(
            diagnose_registry_device(&r, id),
            Err(AmdRegistryError::InvalidIdentity(PciIdentityError::IncompleteSubsystem))
        );

        let mut r = DeviceRegistry::<1, 0>::new();
        let mut invalid = identity(0x1002, 0xffff);
        invalid.class.base = 0x03;
        let id = r.insert(endpoint(invalid)).unwrap();
        assert_eq!(
            diagnose_registry_device(&r, id),
            Err(AmdRegistryError::InvalidIdentity(PciIdentityError::InvalidDevice))
        );

        let mut r = DeviceRegistry::<1, 0>::new();
        let mut bad_class = identity(0x1002, 0x73bf);
        bad_class.class.base = 0xff;
        let id = r.insert(endpoint(bad_class)).unwrap();
        assert_eq!(
            diagnose_registry_device(&r, id),
            Err(AmdRegistryError::InvalidIdentity(PciIdentityError::InvalidClass))
        );
    }

    #[test]
    fn incomplete_report_is_transactional_and_never_invents_support() {
        let mut r = DeviceRegistry::<2, 0>::new();
        let id = r.insert(endpoint(identity(0x1002, 0x73bf))).unwrap();
        let original = diagnose_registry_device(&r, id).unwrap().unwrap();
        let mut records = [Some(original)];
        // A truncated result is refused before overwriting the output.
        let mut second = identity(0x1002, 0x731f);
        second.address.device = 8;
        r.insert(endpoint(second)).unwrap();
        assert_eq!(
            diagnose_registry(&r, &mut records),
            Err(AmdRegistryError::SnapshotCapacity)
        );
        assert_eq!(records, [Some(original)]);

        // Even a perfectly formatted AMD candidate does not authorize
        // initialization, a display backend, firmware or DMA.
        assert_eq!(original.label(), "AMD_GPU_DETECTED_UNSUPPORTED");
        assert!(!original.may_touch_hardware());
    }

    #[test]
    fn registry_consumption_has_no_second_pci_read_or_scan() {
        let reads = Cell::new(0usize);
        let mut mock = MockConfig::new(|address: PciAddress, offset: u16| {
            reads.set(reads.get() + 1);
            if address.segment == 0 && address.bus == 0
                && address.device == 2 && address.function == 0
            {
                return Ok(match offset {
                    0x00 => 0x73bf_1002,
                    0x08 => 0x0300_00a1,
                    0x0c => 0,
                    0x2c => 0xe438_1da2,
                    _ => u32::MAX,
                });
            }
            Ok(u32::MAX)
        });
        let inventory = scan_topology::<4>(&mut mock, 0, 0, 0).unwrap();
        let mut registry = DeviceRegistry::<4, 0>::new();
        registry.import_inventory(&inventory).unwrap();
        let reads_after_scan = reads.get();
        let mut found = [None; 1];
        assert_eq!(diagnose_registry(&registry, &mut found), Ok(1));
        assert_eq!(reads.get(), reads_after_scan);
        assert_eq!(found[0].unwrap().identity.pci.device_id, 0x73bf);
    }

    #[test]
    fn stale_and_wrong_segment_handles_are_not_interchangeable() {
        let mut r = DeviceRegistry::<1, 0>::new();
        let id = r.insert(endpoint(identity(0x1002, 0x73bf))).unwrap();
        assert_eq!(
            diagnose_registry_device(&r, DeviceId { generation: id.generation + 1, ..id }),
            Err(AmdRegistryError::Registry(RegistryError::StaleHandle))
        );
        let mut invalid_address = identity(0x1002, 0x73bf);
        invalid_address.address.device = 32;
        let converted = to_amd_identity(invalid_address);
        assert_eq!(converted, Err(PciIdentityError::InvalidAddress));
        let good = diagnose_registry_device(&r, id).unwrap().unwrap();
        assert_eq!(good.identity.pci.address.segment, 0x1234);
        assert_eq!(good.identity.pci.address.function, 3);
    }

}
