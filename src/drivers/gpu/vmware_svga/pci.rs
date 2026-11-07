//! Validate the VMware SVGA PCI header and BARs before port access.
//! BAR1 and BAR2 remain physical addresses, not virtual memory mappings.
use crate::storage_pci::PciLocation;

pub const VENDOR_ID: u16 = 0x15ad;
pub const DEVICE_ID: u16 = 0x0405;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciDevice {
    pub location: PciLocation,
    pub io_base: u16,
    pub framebuffer_bar: u32,
    pub fifo_bar: u32,
    pub memory_decode_enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PciError {
    IoDecodeDisabled,
    InvalidIoBar,
    InvalidFramebufferBar,
    InvalidFifoBar,
    OverlappingMemoryBars,
}
impl PciError {
    pub const fn label(self) -> &'static str {
        match self {
            Self::IoDecodeDisabled => "IO_DECODE_DISABLED",
            Self::InvalidIoBar => "INVALID_IO_BAR0",
            Self::InvalidFramebufferBar => "INVALID_FB_BAR1",
            Self::InvalidFifoBar => "INVALID_FIFO_BAR2",
            Self::OverlappingMemoryBars => "OVERLAPPING_GPU_BARS",
        }
    }
}
const fn overlaps(base: u16, start: u16, end: u16) -> bool {
    base <= end && base + 1 >= start
}
fn io_base(raw: u32) -> Result<u16, PciError> {
    if raw == u32::MAX || raw & 1 == 0 || raw & 0xffff_0000 != 0 {
        return Err(PciError::InvalidIoBar);
    }
    let base = (raw & !3) as u16;
    // Deny overflow and overlapping reserved serial, VGA and PCI config ports.
    if base < 0x100
        || base > 0xfffd
        || overlaps(base, 0x02f8, 0x02ff)
        || overlaps(base, 0x03b0, 0x03ff)
        || overlaps(base, 0x0cf8, 0x0cff)
    {
        return Err(PciError::InvalidIoBar);
    }
    Ok(base)
}
fn memory_bar(raw: u32) -> Option<u32> {
    // Only 32-bit memory BARs; no 64-bit decoding or mapping at this stage.
    if raw == u32::MAX || raw & 1 != 0 || raw & 0x6 != 0 {
        return None;
    }
    let address = raw & !0xf;
    if address == 0 || address == 0xffff_fff0 {
        None
    } else {
        Some(address)
    }
}
pub fn locate(mut read: impl FnMut(PciLocation, u8) -> u32) -> Result<Option<PciDevice>, PciError> {
    for bus in 0..=u8::MAX {
        for device in 0..32u8 {
            let first = PciLocation {
                bus,
                device,
                function: 0,
            };
            let first_id = read(first, 0x00);
            if first_id as u16 == 0 || first_id as u16 == 0xffff {
                continue;
            }
            let multifunction = (read(first, 0x0c) >> 23) & 1 != 0;
            let functions = if multifunction { 8 } else { 1 };
            for function in 0..functions {
                let loc = PciLocation {
                    bus,
                    device,
                    function,
                };
                let id = if function == 0 {
                    first_id
                } else {
                    read(loc, 0x00)
                };
                if id as u16 != VENDOR_ID || (id >> 16) as u16 != DEVICE_ID {
                    continue;
                }
                if (read(loc, 0x08) >> 24) as u8 != 0x03 {
                    continue;
                }
                let command = read(loc, 0x04) as u16;
                if command & 1 == 0 {
                    return Err(PciError::IoDecodeDisabled);
                }
                let io_base = io_base(read(loc, 0x10))?;
                let framebuffer_bar =
                    memory_bar(read(loc, 0x14)).ok_or(PciError::InvalidFramebufferBar)?;
                let fifo_bar = memory_bar(read(loc, 0x18)).ok_or(PciError::InvalidFifoBar)?;
                if framebuffer_bar == fifo_bar {
                    return Err(PciError::OverlappingMemoryBars);
                }
                return Ok(Some(PciDevice {
                    location: loc,
                    io_base,
                    framebuffer_bar,
                    fifo_bar,
                    memory_decode_enabled: command & 2 != 0,
                }));
            }
        }
    }
    Ok(None)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn valid(loc: PciLocation, offset: u8) -> u32 {
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
    fn validates_io_and_memory_bar_descriptors() {
        let d = locate(valid).unwrap().unwrap();
        assert_eq!(d.io_base, 0x1070);
        assert_eq!(d.framebuffer_bar, 0xe000_0000);
        assert_eq!(d.fifo_bar, 0xe800_0000);
        assert!(d.memory_decode_enabled);
    }
    #[test]
    fn rejects_disabled_decode_and_bad_bars() {
        assert_eq!(
            locate(|l, o| if o == 4 && l.device == 15 {
                2
            } else {
                valid(l, o)
            }),
            Err(PciError::IoDecodeDisabled)
        );
        for raw in [0, u32::MAX, 0xe000_0000, 0x03f9, 0x0cf9, 0x1_0001] {
            assert_eq!(
                locate(|l, o| if o == 16 && l.device == 15 {
                    raw
                } else {
                    valid(l, o)
                }),
                Err(PciError::InvalidIoBar)
            );
        }
        assert_eq!(
            locate(|l, o| if o == 20 && l.device == 15 {
                0xe000_0004
            } else {
                valid(l, o)
            }),
            Err(PciError::InvalidFramebufferBar)
        );
        assert_eq!(
            locate(|l, o| if o == 24 && l.device == 15 {
                0xe000_0008
            } else {
                valid(l, o)
            }),
            Err(PciError::OverlappingMemoryBars)
        );
    }
    #[test]
    fn ignores_wrong_pci_class_and_scans_multifunction_devices() {
        assert_eq!(
            locate(|l, o| if o == 8 && l.device == 15 {
                0x0106_0000
            } else {
                valid(l, o)
            }),
            Ok(None)
        );
        let d = locate(|l, o| {
            if l.bus != 0 || l.device != 15 {
                return u32::MAX;
            }
            match (l.function, o) {
                (0, 0) => 0x0001_15ad,
                (0, 12) => 0x0080_0000,
                (1, 0) => 0x0405_15ad,
                (1, 4) => 3,
                (1, 8) => 0x0300_0000,
                (1, 16) => 0x1071,
                (1, 20) => 0xe000_0000,
                (1, 24) => 0xe800_0000,
                _ => u32::MAX,
            }
        })
        .unwrap()
        .unwrap();
        assert_eq!(d.location.function, 1);
    }
}
