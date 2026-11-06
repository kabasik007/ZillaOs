//! VMware SVGA II stage 1: read-only guest inspection, GOP always fallback.
//! Selecting the INDEX I/O port does not write SVGA register VALUE.
//! No VRAM/FIFO mapping, runtime mode switching or GPU acceleration.
pub mod capabilities;
pub mod fifo;
pub mod pci;
pub mod registers;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProbeInfo {
    pub pci: pci::PciDevice,
    pub registers: capabilities::Snapshot,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeError {
    Pci(pci::PciError),
    Registers(capabilities::RegisterError),
}
impl ProbeError {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pci(e) => e.label(),
            Self::Registers(e) => e.label(),
        }
    }
}
pub fn probe(
    read_config: impl FnMut(crate::storage_pci::PciLocation, u8) -> u32,
    read_register: impl FnMut(u16, u32) -> u32,
) -> Result<Option<ProbeInfo>, ProbeError> {
    let Some(pci) = pci::locate(read_config).map_err(ProbeError::Pci)? else {
        return Ok(None);
    };
    let registers = capabilities::inspect(pci.io_base, read_register)
        .map_err(ProbeError::Registers)?;
    Ok(Some(ProbeInfo { pci, registers }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_bar_rejects_all_device_port_reads() {
        let mut reads=0;
        let r=probe(|l,o|{
            if l.bus!=0 || l.device!=1 { return u32::MAX; }
            match o {
                0=>0x0405_15ad, 4=>3, 8=>0x0300_0000, 12=>0, 16=>0,
                _=>u32::MAX,
            }
        }, |_,_| {reads+=1;u32::MAX});
        assert_eq!(r,Err(ProbeError::Pci(pci::PciError::InvalidIoBar)));
        assert_eq!(reads,0);
    }
    #[test]
    fn missing_device_is_not_an_error() {
        let mut reads=0;
        assert_eq!(probe(|_,_|u32::MAX,|_,_|{reads+=1;0}),Ok(None));
        assert_eq!(reads,0);
    }
}
