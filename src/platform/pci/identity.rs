//! Canonical PCI identity, including the full PCI segment group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PciAddress {
    pub segment: u16,
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddressError {
    InvalidDevice,
    InvalidFunction,
}

impl PciAddress {
    pub const fn new(
        segment: u16,
        bus: u8,
        device: u8,
        function: u8,
    ) -> Result<Self, AddressError> {
        if device >= 32 {
            return Err(AddressError::InvalidDevice);
        }
        if function >= 8 {
            return Err(AddressError::InvalidFunction);
        }
        Ok(Self {
            segment,
            bus,
            device,
            function,
        })
    }

    pub const fn is_valid(self) -> bool {
        self.device < 32 && self.function < 8
    }

    /// Legacy CF8/CFC can address only segment group zero.
    pub const fn legacy_location(self) -> Option<crate::storage_pci::PciLocation> {
        if self.segment != 0 || !self.is_valid() {
            return None;
        }
        Some(crate::storage_pci::PciLocation {
            bus: self.bus,
            device: self.device,
            function: self.function,
        })
    }
}

impl From<crate::storage_pci::PciLocation> for PciAddress {
    fn from(location: crate::storage_pci::PciLocation) -> Self {
        Self {
            segment: 0,
            bus: location.bus,
            device: location.device,
            function: location.function,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciClass {
    pub base: u8,
    pub subclass: u8,
    pub programming_interface: u8,
}

impl PciClass {
    pub const fn is_display(self) -> bool {
        self.base == 0x03
    }
    pub const fn is_pci_bridge(self) -> bool {
        self.base == 0x06 && self.subclass == 0x04
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciIdentity {
    pub address: PciAddress,
    pub vendor_id: u16,
    pub device_id: u16,
    pub revision: u8,
    pub class: PciClass,
    pub subsystem_vendor_id: Option<u16>,
    pub subsystem_device_id: Option<u16>,
}

impl PciIdentity {
    pub const fn is_vmware_svga_ii(self) -> bool {
        self.vendor_id == 0x15ad && self.device_id == 0x0405 && self.class.is_display()
    }

    /// This detects AMD's display vendor, NOT supported hardware or usable acceleration.
    /// AMD chipset vendor 0x1022 alone is not a Radeon GPU identity.
    pub const fn is_amd_display_candidate(self) -> bool {
        self.vendor_id == 0x1002 && self.class.is_display()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciBridge {
    pub primary_bus: u8,
    pub secondary_bus: u8,
    pub subordinate_bus: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciDeviceInfo {
    pub identity: PciIdentity,
    /// Header type without the multifunction bit (0 = endpoint, 1 = bridge).
    pub header_type: u8,
    pub multifunction: bool,
    pub bridge: Option<PciBridge>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn segment_and_bdf_are_not_truncated() {
        let a = PciAddress::new(0xabcd, 255, 31, 7).unwrap();
        assert_eq!(a.segment, 0xabcd);
        assert_eq!(a.legacy_location(), None);
        assert_eq!(
            PciAddress::new(0, 0, 32, 0),
            Err(AddressError::InvalidDevice)
        );
        assert_eq!(
            PciAddress::new(0, 0, 0, 8),
            Err(AddressError::InvalidFunction)
        );
        let old = crate::storage_pci::PciLocation {
            bus: 4,
            device: 9,
            function: 1,
        };
        assert_eq!(PciAddress::from(old).legacy_location(), Some(old));
    }
    #[test]
    fn amd_and_vmware_identity_contract_is_strict() {
        let address = PciAddress::new(0, 0, 2, 0).unwrap();
        let mut i = PciIdentity {
            address,
            vendor_id: 0x1002,
            device_id: 0x73bf,
            revision: 0,
            class: PciClass {
                base: 3,
                subclass: 0,
                programming_interface: 0,
            },
            subsystem_vendor_id: None,
            subsystem_device_id: None,
        };
        assert!(i.is_amd_display_candidate());
        assert!(!i.is_vmware_svga_ii());
        i.class.base = 1;
        assert!(!i.is_amd_display_candidate());
        i.vendor_id = 0x1022;
        i.class.base = 3;
        assert!(!i.is_amd_display_candidate());
        i.vendor_id = 0x15ad;
        i.device_id = 0x0405;
        assert!(i.is_vmware_svga_ii());
    }
}
