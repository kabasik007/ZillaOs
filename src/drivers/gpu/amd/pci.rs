//! Pure PCI identity boundary for AMD A0. No ports, MMIO or enumeration.
//! The Platform PCI agent will adapt its shared PciIdentity to this value type.
#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciAddress {
    pub segment: u16,
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl PciAddress {
    pub const fn is_valid(self) -> bool {
        self.device < 32 && self.function < 8
    }

    /// Adapter for the existing storage_pci::PciLocation's three fields.
    /// CF8/CFC accesses only segment 0; this function does not read hardware.
    pub const fn from_legacy_segment_zero(bus: u8, device: u8, function: u8) -> Self {
        Self { segment: 0, bus, device, function }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciIdentity {
    pub address: PciAddress,
    pub vendor_id: u16,
    pub device_id: u16,
    pub revision: u8,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub subsystem_vendor_id: Option<u16>,
    pub subsystem_device_id: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PciIdentityError {
    InvalidAddress,
    InvalidVendor,
    InvalidDevice,
    InvalidClass,
    IncompleteSubsystem,
    InvalidHeader,
    AddressMismatch,
}

impl PciIdentity {
    pub const fn validate(self) -> Result<(), PciIdentityError> {
        if !self.address.is_valid() {
            return Err(PciIdentityError::InvalidAddress);
        }
        if self.vendor_id == 0 || self.vendor_id == 0xffff {
            return Err(PciIdentityError::InvalidVendor);
        }
        if self.device_id == 0xffff {
            return Err(PciIdentityError::InvalidDevice);
        }
        if self.class_code == 0xff {
            return Err(PciIdentityError::InvalidClass);
        }
        match (self.subsystem_vendor_id, self.subsystem_device_id) {
            (Some(v), Some(d)) if v == 0 || v == 0xffff || d == 0xffff => {
                Err(PciIdentityError::IncompleteSubsystem)
            }
            (Some(_), None) | (None, Some(_)) => Err(PciIdentityError::IncompleteSubsystem),
            _ => Ok(()),
        }
    }

    /// Decode a *previously read* PCI configuration snapshot; not a scanner.
    /// DWORDs are from offsets 0x00, 0x08, 0x0c, and optionally 0x2c.
    /// For non-type-0 headers subsystem IDs are not read from 0x2c.
    pub fn from_config_dwords(
        address: PciAddress,
        vendor_device: u32,
        class_revision: u32,
        header: u32,
        subsystem: Option<u32>,
    ) -> Result<Option<Self>, PciIdentityError> {
        if !address.is_valid() {
            return Err(PciIdentityError::InvalidAddress);
        }
        let vendor = vendor_device as u16;
        if vendor == 0xffff {
            return Ok(None);
        }
        if vendor == 0 {
            return Err(PciIdentityError::InvalidVendor);
        }
        if header == u32::MAX {
            return Err(PciIdentityError::InvalidHeader);
        }
        let header_type = ((header >> 16) as u8) & 0x7f;
        if header_type != 0 && subsystem.is_some() {
            return Err(PciIdentityError::InvalidHeader);
        }
        let subsystem_pair = match subsystem {
            Some(dword) if dword != u32::MAX && dword != 0 => {
                (Some(dword as u16), Some((dword >> 16) as u16))
            }
            _ => (None, None),
        };
        let value = Self {
            address,
            vendor_id: vendor,
            device_id: (vendor_device >> 16) as u16,
            revision: class_revision as u8,
            prog_if: (class_revision >> 8) as u8,
            subclass: (class_revision >> 16) as u8,
            class_code: (class_revision >> 24) as u8,
            subsystem_vendor_id: subsystem_pair.0,
            subsystem_device_id: subsystem_pair.1,
        };
        value.validate()?;
        Ok(Some(value))
    }
}

/// An immutable-snapshot consumer contract, NOT a PCI bus enumerator.
/// The eventual Platform DeviceRegistry must provide the source of truth.
pub trait ReadOnlyPciInventory {
    fn identity_at(&self, address: PciAddress) -> Option<PciIdentity>;
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: PciAddress = PciAddress { segment: 2, bus: 3, device: 4, function: 5 };

    #[test]
    fn preserves_full_domain_bdf_revision_and_subsystem() {
        let id = PciIdentity::from_config_dwords(
            A, 0x73bf_1002, 0x0300_00a1, 0, Some(0xe438_1da2),
        ).unwrap().unwrap();
        assert_eq!(id.address, A);
        assert_eq!(id.device_id, 0x73bf);
        assert_eq!(id.revision, 0xa1);
        assert_eq!(id.class_code, 0x03);
        assert_eq!(id.subsystem_vendor_id, Some(0x1da2));
        assert_eq!(id.subsystem_device_id, Some(0xe438));
    }

    #[test]
    fn missing_function_is_not_invalid_and_bad_vendor_is_invalid() {
        assert_eq!(PciIdentity::from_config_dwords(A, u32::MAX, u32::MAX, u32::MAX, None), Ok(None));
        assert_eq!(PciIdentity::from_config_dwords(A, 0, 0, 0, None), Err(PciIdentityError::InvalidVendor));
    }

    #[test]
    fn rejects_bad_slot_or_function_and_bad_header() {
        let invalid = PciAddress { device: 32, ..A };
        assert_eq!(PciIdentity::from_config_dwords(invalid, 0x73bf_1002, 0x0300_0000, 0, None), Err(PciIdentityError::InvalidAddress));
        assert_eq!(PciIdentity::from_config_dwords(A, 0x73bf_1002, 0x0300_0000, u32::MAX, None), Err(PciIdentityError::InvalidHeader));
        assert_eq!(PciIdentity::from_config_dwords(A, 0x73bf_1002, 0x0300_0000, 0x0001_0000, Some(0x1234_5678)), Err(PciIdentityError::InvalidHeader));
    }

    #[test]
    fn bad_subsystem_and_unknown_class_are_rejected() {
        assert_eq!(PciIdentity::from_config_dwords(A, 0x73bf_1002, 0xff00_0000, 0, None), Err(PciIdentityError::InvalidClass));
        assert_eq!(PciIdentity::from_config_dwords(A, 0x73bf_1002, 0x0300_0000, 0, Some(0x1234_0000)), Err(PciIdentityError::IncompleteSubsystem));
    }

    #[test]
    fn legacy_adapter_does_not_claim_nonzero_segment_support() {
        let loc = PciAddress::from_legacy_segment_zero(1, 31, 7);
        assert_eq!(loc, PciAddress { segment: 0, bus: 1, device: 31, function: 7 });
        assert!(loc.is_valid());
        assert!(!PciAddress { function: 8, ..loc }.is_valid());
    }
}
