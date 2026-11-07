//! Read-only, injected PCI config access. No write operation exists in this interface.
use super::identity::PciAddress;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    InvalidAddress,
    UnalignedOffset,
    OffsetOutOfRange,
    UnsupportedSegment,
    Unavailable,
}

pub trait PciConfigRead {
    fn read32(&mut self, address: PciAddress, offset: u16) -> Result<u32, ConfigError>;
}

/// A mock injection backend; the closure can simulate holes, malformed devices
/// and access faults without any privileged instructions.
pub struct MockConfig<F> {
    read: F,
}

impl<F> MockConfig<F> {
    pub const fn new(read: F) -> Self {
        Self { read }
    }
}

impl<F> PciConfigRead for MockConfig<F>
where
    F: FnMut(PciAddress, u16) -> Result<u32, ConfigError>,
{
    fn read32(&mut self, address: PciAddress, offset: u16) -> Result<u32, ConfigError> {
        check_config_request(address, offset, 4096)?;
        (self.read)(address, offset)
    }
}

/// Adapter for the existing PCI CF8/CFC reader; caller must serialize the
/// selector port (0xCF8) across CPUs/interrupts before sharing it.
pub struct LegacyConfig<F> {
    read: F,
}

impl<F> LegacyConfig<F> {
    pub const fn new(read: F) -> Self {
        Self { read }
    }
}

impl<F> PciConfigRead for LegacyConfig<F>
where
    F: FnMut(crate::storage_pci::PciLocation, u8) -> u32,
{
    fn read32(&mut self, address: PciAddress, offset: u16) -> Result<u32, ConfigError> {
        check_config_request(address, offset, 256)?;
        let location = address
            .legacy_location()
            .ok_or(ConfigError::UnsupportedSegment)?;
        Ok((self.read)(location, offset as u8))
    }
}

pub fn check_config_request(
    address: PciAddress,
    offset: u16,
    length: u16,
) -> Result<(), ConfigError> {
    if !address.is_valid() {
        return Err(ConfigError::InvalidAddress);
    }
    if offset & 3 != 0 {
        return Err(ConfigError::UnalignedOffset);
    }
    if offset.checked_add(4).is_none_or(|end| end > length) {
        return Err(ConfigError::OffsetOutOfRange);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_rejects_other_segment_before_port_read() {
        let calls = core::cell::Cell::new(0);
        let mut config = LegacyConfig::new(|_, _| {
            calls.set(calls.get() + 1);
            0x0405_15ad
        });
        assert_eq!(
            config.read32(PciAddress::new(1, 0, 1, 0).unwrap(), 0),
            Err(ConfigError::UnsupportedSegment)
        );
        assert_eq!(
            config.read32(PciAddress::new(0, 0, 1, 0).unwrap(), 256),
            Err(ConfigError::OffsetOutOfRange)
        );
        assert_eq!(
            config.read32(PciAddress::new(0, 0, 1, 0).unwrap(), 3),
            Err(ConfigError::UnalignedOffset)
        );
        assert_eq!(calls.get(), 0);
        assert_eq!(
            config.read32(PciAddress::new(0, 0, 1, 0).unwrap(), 0),
            Ok(0x0405_15ad)
        );
        assert_eq!(calls.get(), 1);
    }
    #[test]
    fn mock_checks_invalid_address_and_extended_offset() {
        let mut mock = MockConfig::new(|_, _| Ok(0));
        assert_eq!(
            mock.read32(
                PciAddress {
                    segment: 0,
                    bus: 0,
                    device: 35,
                    function: 0
                },
                0
            ),
            Err(ConfigError::InvalidAddress)
        );
        assert_eq!(
            mock.read32(PciAddress::new(0, 0, 0, 0).unwrap(), 4096),
            Err(ConfigError::OffsetOutOfRange)
        );
    }
}
