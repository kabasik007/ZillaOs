//! PCIe ECAM address planning only. Hardware accesses are disabled.
//! Actual ECAM readers must acquire a verified UC mapping from kernel MMIO
//! authority and serialize access to devices removed/reset concurrently.
use super::config::{ConfigError, PciConfigRead, check_config_request};
use super::identity::PciAddress;
use super::mcfg::{McfgError, McfgTable};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EcamReadPlan {
    pub address: PciAddress,
    pub register_offset: u16,
    pub physical_address: u64,
}

pub fn plan_read<const CAP: usize>(
    table: &McfgTable<CAP>, address: PciAddress, offset: u16
) -> Result<EcamReadPlan, McfgError> {
    let physical_address = table.ecam_address(address, offset)?;
    Ok(EcamReadPlan { address, register_offset: offset, physical_address })
}

/// This adapter validates offsets, but NEVER dereferences an ECAM address.
/// Having a parsed MCFG is insufficient to claim a trusted MMIO mapping.
pub struct DisabledEcam<'a, const CAP: usize> {
    pub mcfg: &'a McfgTable<CAP>,
}

impl<const CAP: usize> PciConfigRead for DisabledEcam<'_, CAP> {
    fn read32(&mut self, address: PciAddress, offset: u16) -> Result<u32, ConfigError> {
        check_config_request(address, offset, 4096)?;
        plan_read(self.mcfg, address, offset).map_err(|_| ConfigError::Unavailable)?;
        Err(ConfigError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unmapped_ecam_is_denied_even_for_valid_address() {
        let table = McfgTable::<0>::empty();
        let mut backend = DisabledEcam { mcfg: &table };
        assert_eq!(backend.read32(PciAddress::new(0,0,0,0).unwrap(),0), Err(ConfigError::Unavailable));
        assert_eq!(backend.read32(PciAddress::new(0,0,0,0).unwrap(),1), Err(ConfigError::UnalignedOffset));
    }
}
