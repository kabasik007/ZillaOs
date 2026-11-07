//! AMD Radeon A0 read-only PCI identification + A1 inert IP model.
//! Does not scan a bus, access a BAR, write any configuration, start DMA,
//! load firmware, manage outputs, or enable hardware acceleration.
#![forbid(unsafe_code)]

pub mod device_ids;
pub mod ip_blocks;
pub mod pci;

pub use device_ids::AsicFamily;
pub use pci::{PciAddress, PciIdentity, PciIdentityError, ReadOnlyPciInventory};

pub const AMD_GPU_VENDOR_ID: u16 = 0x1002;
pub const PCI_DISPLAY_CLASS: u8 = 0x03;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmdGpuIdentity {
    /// Complete function identity and BDF, including segment and revision.
    pub pci: PciIdentity,
    /// Diagnostic hint only. NOT a verified ASIC/IP/board identification.
    pub family_hint: AsicFamily,
}

impl AmdGpuIdentity {
    /// A1 inventory intentionally has no guessed hardware IP revisions.
    pub const fn ip_inventory(self) -> ip_blocks::AsicIpInventory {
        ip_blocks::AsicIpInventory::unknown(self.family_hint)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmdGpuProbeStatus {
    Absent,
    NoMatch,
    InvalidPciIdentity(PciIdentityError),
    DetectedUnsupported(AmdGpuIdentity),
    /// Reserved for board-specific validated enablement; table empty in A0.
    SupportedByExplicitTable(AmdGpuIdentity),
}

impl AmdGpuProbeStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Absent => "ABSENT",
            Self::NoMatch => "NO_MATCH",
            Self::InvalidPciIdentity(_) => "INVALID_PCI_IDENTITY",
            Self::DetectedUnsupported(_) => "DETECTED_UNSUPPORTED",
            Self::SupportedByExplicitTable(_) => "SUPPORTED_EXPLICIT_TABLE",
        }
    }

    /// No A0/A1 status authorizes hardware initialization, even if a
    /// caller constructs a status variant by hand.
    pub const fn may_initialize_hardware(self) -> bool {
        false
    }
}

/// The AMD function matcher: completely pure, no PCI read or write.
/// Class 0x03 and subclasses VGA (00), 3D (02) and other display (80)
/// indicate *candidates*, never operational support.
pub fn match_amd_display(pci: PciIdentity) -> AmdGpuProbeStatus {
    if !pci.address.is_valid() {
        return AmdGpuProbeStatus::InvalidPciIdentity(PciIdentityError::InvalidAddress);
    }
    if pci.vendor_id == 0xffff {
        return AmdGpuProbeStatus::Absent;
    }
    if let Err(error) = pci.validate() {
        return AmdGpuProbeStatus::InvalidPciIdentity(error);
    }
    if pci.vendor_id != AMD_GPU_VENDOR_ID
        || pci.class_code != PCI_DISPLAY_CLASS
        || !matches!(pci.subclass, 0x00 | 0x02 | 0x80)
    {
        return AmdGpuProbeStatus::NoMatch;
    }
    let identity = AmdGpuIdentity {
        pci,
        family_hint: device_ids::diagnostic_family(pci.device_id),
    };
    if device_ids::explicitly_supported(&pci) {
        AmdGpuProbeStatus::SupportedByExplicitTable(identity)
    } else {
        AmdGpuProbeStatus::DetectedUnsupported(identity)
    }
}

/// Query exactly one address in a Platform-provided immutable inventory.
/// DOES NOT enumerate buses or access PCI ports: one read-only mock or
/// DeviceRegistry lookup is the entire side effect of this function.
pub fn probe_address(
    inventory: &impl ReadOnlyPciInventory,
    address: PciAddress,
) -> AmdGpuProbeStatus {
    if !address.is_valid() {
        return AmdGpuProbeStatus::InvalidPciIdentity(PciIdentityError::InvalidAddress);
    }
    match inventory.identity_at(address) {
        None => AmdGpuProbeStatus::Absent,
        Some(id) if id.address != address =>
            AmdGpuProbeStatus::InvalidPciIdentity(PciIdentityError::AddressMismatch),
        Some(id) => match_amd_display(id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;

    const ADDRESS: PciAddress = PciAddress { segment: 0, bus: 12, device: 3, function: 1 };

    fn make(vendor_id: u16, device_id: u16, class_code: u8, subclass: u8) -> PciIdentity {
        PciIdentity {
            address: ADDRESS,
            vendor_id,
            device_id,
            class_code,
            subclass,
            prog_if: 0,
            revision: 0xa3,
            subsystem_vendor_id: Some(0x1da2),
            subsystem_device_id: Some(0xe438),
        }
    }

    fn unsupported(id: PciIdentity) -> AmdGpuIdentity {
        match match_amd_display(id) {
            AmdGpuProbeStatus::DetectedUnsupported(info) => {
                assert!(!AmdGpuProbeStatus::DetectedUnsupported(info).may_initialize_hardware());
                info
            }
            other => panic!("expected unsupported Radeon candidate: {other:?}"),
        }
    }

    #[test]
    fn gpu_candidates_preserve_identity_but_never_gain_access() {
        for device in [0x67df, 0x731f, 0x73bf, 0x9999] {
            let id = make(AMD_GPU_VENDOR_ID, device, 0x03, 0x00);
            let info = unsupported(id);
            assert_eq!(info.pci, id);
            assert_eq!(info.ip_inventory().family_hint, info.family_hint);
            assert!(!info.ip_inventory().all_blocks_verified());
        }
    }

    #[test]
    fn rejects_chipset_cpu_audio_and_non_amd_vga() {
        assert_eq!(match_amd_display(make(0x1022, 0x1234, 3, 0)), AmdGpuProbeStatus::NoMatch);
        assert_eq!(match_amd_display(make(0x1002, 0x1234, 4, 3)), AmdGpuProbeStatus::NoMatch);
        assert_eq!(match_amd_display(make(0x1002, 0x1234, 1, 6)), AmdGpuProbeStatus::NoMatch);
        assert_eq!(match_amd_display(make(0x15ad, 0x0405, 3, 0)), AmdGpuProbeStatus::NoMatch);
        assert_eq!(match_amd_display(make(0x8086, 0x1234, 3, 0)), AmdGpuProbeStatus::NoMatch);
    }

    #[test]
    fn accepts_3d_and_other_display_only_as_unsupported_candidates() {
        unsupported(make(0x1002, 0x731f, 3, 2));
        unsupported(make(0x1002, 0x731f, 3, 0x80));
        assert_eq!(match_amd_display(make(0x1002, 0x731f, 3, 1)), AmdGpuProbeStatus::NoMatch);
    }

    #[test]
    fn rejects_absent_and_malformed_pci_data() {
        assert_eq!(match_amd_display(make(0xffff, 0xffff, 0xff, 0xff)), AmdGpuProbeStatus::Absent);
        assert_eq!(match_amd_display(make(0, 0, 3, 0)), AmdGpuProbeStatus::InvalidPciIdentity(PciIdentityError::InvalidVendor));
        assert_eq!(match_amd_display(make(0x1002, 0xffff, 3, 0)), AmdGpuProbeStatus::InvalidPciIdentity(PciIdentityError::InvalidDevice));
        assert_eq!(match_amd_display(make(0x1002, 0x73bf, 0xff, 0)), AmdGpuProbeStatus::InvalidPciIdentity(PciIdentityError::InvalidClass));
        let mut invalid = make(0x1002, 0x73bf, 3, 0);
        invalid.address.device = 32;
        assert_eq!(match_amd_display(invalid), AmdGpuProbeStatus::InvalidPciIdentity(PciIdentityError::InvalidAddress));
        invalid.address = ADDRESS;
        invalid.subsystem_device_id = None;
        assert_eq!(match_amd_display(invalid), AmdGpuProbeStatus::InvalidPciIdentity(PciIdentityError::IncompleteSubsystem));
    }

    struct MockPci {
        entries: [Option<PciIdentity>; 2],
        calls: Cell<usize>,
    }

    impl ReadOnlyPciInventory for MockPci {
        fn identity_at(&self, address: PciAddress) -> Option<PciIdentity> {
            self.calls.set(self.calls.get() + 1);
            self.entries.iter().flatten().copied().find(|id| id.address == address)
        }
    }

    #[test]
    fn fake_backend_queries_only_one_address_and_no_scanner() {
        let other = PciAddress { function: 0, ..ADDRESS };
        let mock = MockPci {
            entries: [Some(make(0x1002, 0x73bf, 3, 0)), Some(PciIdentity {
                address: other, ..make(0x15ad, 0x0405, 3, 0)
            })],
            calls: Cell::new(0),
        };
        assert!(matches!(probe_address(&mock, ADDRESS), AmdGpuProbeStatus::DetectedUnsupported(_)));
        assert_eq!(mock.calls.get(), 1);
        assert_eq!(probe_address(&mock, other), AmdGpuProbeStatus::NoMatch);
        assert_eq!(mock.calls.get(), 2);
        assert_eq!(probe_address(&mock, PciAddress { bus: 99, ..ADDRESS }), AmdGpuProbeStatus::Absent);
        assert_eq!(mock.calls.get(), 3);
        assert_eq!(probe_address(&mock, PciAddress { function: 8, ..ADDRESS }), AmdGpuProbeStatus::InvalidPciIdentity(PciIdentityError::InvalidAddress));
        assert_eq!(mock.calls.get(), 3);
    }

    #[test]
    fn even_forged_supported_status_is_not_an_init_permission() {
        let info = unsupported(make(0x1002, 0x73bf, 3, 0));
        let forged = AmdGpuProbeStatus::SupportedByExplicitTable(info);
        assert!(!forged.may_initialize_hardware());
    }

    #[test]
    fn mismatched_registry_identity_fails_closed() {
        struct WrongBdf(PciIdentity);
        impl ReadOnlyPciInventory for WrongBdf {
            fn identity_at(&self, _: PciAddress) -> Option<PciIdentity> { Some(self.0) }
        }
        let requested = PciAddress { segment: 1, ..ADDRESS };
        assert_eq!(
            probe_address(&WrongBdf(make(0x1002, 0x73bf, 3, 0)), requested),
            AmdGpuProbeStatus::InvalidPciIdentity(PciIdentityError::AddressMismatch)
        );
    }

    #[test]
    fn every_catalogued_id_remains_explicitly_unsupported() {
        for entry in device_ids::DIAGNOSTIC_IDS {
            let id = make(0x1002, entry.device_id, 3, 0);
            assert_eq!(unsupported(id).family_hint, entry.family);
            assert!(!device_ids::explicitly_supported(&id));
        }
    }
}
