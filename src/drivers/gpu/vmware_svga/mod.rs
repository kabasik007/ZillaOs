//! VMware SVGA II stage 1: read-only guest inspection, GOP always fallback.
//! Selecting the INDEX I/O port does not write SVGA register VALUE.
//! No VRAM/FIFO mapping, runtime mode switching or GPU acceleration.
pub mod capabilities;
pub mod fifo;
pub mod negotiation;
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
    Negotiation(negotiation::NegotiationError),
}
impl ProbeError {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pci(e) => e.label(),
            Self::Registers(e) => e.label(),
            Self::Negotiation(e) => e.label(),
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


/// Opt-in path only; the normal boot probe above never writes device values.
/// The PCI descriptor is checked before any handshake I/O occurs.
pub fn probe_with_negotiation(
    read_config: impl FnMut(crate::storage_pci::PciLocation, u8) -> u32,
    mut read_register: impl FnMut(u16, u32) -> u32,
    mut write_register: impl FnMut(u16, u32, u32),
) -> Result<Option<ProbeInfo>, ProbeError> {
    let Some(pci) = pci::locate(read_config).map_err(ProbeError::Pci)? else {
        return Ok(None);
    };
    let selected = negotiation::negotiate(
        |index| read_register(pci.io_base, index),
        |index, value| write_register(pci.io_base, index, value),
    )
    .map_err(ProbeError::Negotiation)?;
    let registers = capabilities::inspect(pci.io_base, read_register)
        .map_err(ProbeError::Registers)?;
    if registers.interface_id != selected {
        return Err(ProbeError::Negotiation(
            negotiation::NegotiationError::UnstableInterfaceId,
        ));
    }
    Ok(Some(ProbeInfo { pci, registers }))
}

#[cfg(test)]
mod negotiation_tests {
    use super::*;
    use core::cell::Cell;

    fn config(loc: crate::storage_pci::PciLocation, offset: u8) -> u32 {
        if loc.bus != 0 || loc.device != 15 || loc.function != 0 {
            return u32::MAX;
        }
        match offset {
            0 => 0x0405_15ad,
            4 => 3,
            8 => 0x0300_0000,
            12 => 0,
            16 => 0x1071,
            20 => 0xe000_0008,
            24 => 0xe800_0000,
            _ => u32::MAX,
        }
    }

    #[test]
    fn handshake_happens_only_after_pci_validation() {
        let active = Cell::new(0);
        let mut writes = 0;
        let info = probe_with_negotiation(
            config,
            |_, index| match index {
                registers::ID => active.get(),
                registers::MAX_WIDTH => 1920,
                registers::MAX_HEIGHT => 1080,
                registers::VRAM_SIZE => 128 * 1024 * 1024,
                registers::FB_SIZE => 64 * 1024 * 1024,
                registers::FB_OFFSET => 0,
                registers::MEM_SIZE => 1024 * 1024,
                registers::CAPABILITIES => 0,
                _ => panic!("unexpected register read"),
            },
            |base, index, value| {
                assert_eq!(base, 0x1070);
                assert_eq!(index, registers::ID);
                assert!(registers::known_id(value));
                active.set(value);
                writes += 1;
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(writes, 1);
        assert_eq!(info.registers.interface_id, registers::SVGA_ID_2);
    }

    #[test]
    fn invalid_pci_bar_cannot_trigger_version_write() {
        let mut writes = 0;
        let result = probe_with_negotiation(
            |loc, off| {
                if off == 16 && loc.bus == 0 && loc.device == 15 {
                    0
                } else {
                    config(loc, off)
                }
            },
            |_, _| panic!("should not read SVGA registers"),
            |_, _, _| writes += 1,
        );
        assert_eq!(result, Err(ProbeError::Pci(pci::PciError::InvalidIoBar)));
        assert_eq!(writes, 0);
    }

    #[test]
    fn no_display_card_never_attempts_id_negotiation() {
        let mut writes = 0;
        assert_eq!(
            probe_with_negotiation(
                |_, _| u32::MAX,
                |_, _| panic!("should not read SVGA registers"),
                |_, _, _| writes += 1,
            ),
            Ok(None)
        );
        assert_eq!(writes, 0);
    }
}
