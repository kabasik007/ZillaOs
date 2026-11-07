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

/// AMD's pure A0 matcher consuming the authoritative DeviceRegistry directly.
/// No secondary PciInventory or PCI config reader is stored by this adapter.
pub struct AmdRegistryAdapter<'a, const DEVICES: usize, const CLAIMS: usize> {
    registry: &'a crate::platform::device::DeviceRegistry<DEVICES, CLAIMS>,
}

impl<'a, const DEVICES: usize, const CLAIMS: usize> AmdRegistryAdapter<'a, DEVICES, CLAIMS> {
    pub const fn new(
        registry: &'a crate::platform::device::DeviceRegistry<DEVICES, CLAIMS>,
    ) -> Self {
        Self { registry }
    }
}

impl<const DEVICES: usize, const CLAIMS: usize> ReadOnlyPciInventory
    for AmdRegistryAdapter<'_, DEVICES, CLAIMS>
{
    fn identity_at(&self, address: AmdAddress) -> Option<AmdIdentity> {
        if !address.is_valid() {
            return None;
        }
        let platform_address = crate::platform::pci::PciAddress {
            segment: address.segment,
            bus: address.bus,
            device: address.device,
            function: address.function,
        };
        let id = self.registry.find(platform_address)?;
        let view = self.registry.get(id).ok()?;
        to_amd_identity(view.info.identity).ok()
    }
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
    #[test]
    fn registry_adapter_preserves_segment_bdf_and_never_reads_config() {
        use crate::platform::device::DeviceRegistry;
        use crate::platform::pci::PciDeviceInfo;
        let mut registry = DeviceRegistry::<2, 0>::new();
        let platform = identity(0x1002, 0x73bf);
        let handle = registry
            .insert(PciDeviceInfo {
                identity: platform,
                header_type: 0,
                multifunction: false,
                bridge: None,
            })
            .unwrap();
        let amd = AmdAddress {
            segment: 0x1234,
            bus: 2,
            device: 7,
            function: 3,
        };
        let adapter = AmdRegistryAdapter::new(&registry);
        assert_eq!(
            adapter.identity_at(amd),
            Some(to_amd_identity(platform).unwrap())
        );
        assert!(matches!(
            amd::probe_address(&adapter, amd),
            AmdGpuProbeStatus::DetectedUnsupported(_)
        ));
        assert_eq!(adapter.identity_at(AmdAddress { segment: 0, ..amd }), None);
        registry.remove(handle).unwrap();
        let adapter = AmdRegistryAdapter::new(&registry);
        assert_eq!(adapter.identity_at(amd), None);
    }
}
