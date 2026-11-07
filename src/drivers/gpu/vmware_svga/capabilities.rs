//! Read a firmware-negotiated SVGA ID and bounded, read-only device metadata.
//! Unknown interface IDs fall back to Limine/GOP; never guess register layout.
use super::registers;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub interface_id: u32,
    pub max_width: u32,
    pub max_height: u32,
    pub vram_bytes: u32,
    pub framebuffer_bytes: u32,
    pub framebuffer_offset: u32,
    pub fifo_bytes: Option<u32>,
    pub capability_bits: Option<u32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterError {
    VersionNotNegotiated,
    InvalidDimensions,
    InvalidVram,
    InvalidFramebuffer,
    InvalidFifo,
}
impl RegisterError {
    pub const fn label(self) -> &'static str {
        match self {
            Self::VersionNotNegotiated => "ID_NEGOTIATION_REQUIRED",
            Self::InvalidDimensions => "INVALID_MAX_DIMENSIONS",
            Self::InvalidVram => "INVALID_VRAM_SIZE",
            Self::InvalidFramebuffer => "INVALID_FRAMEBUFFER_SIZE",
            Self::InvalidFifo => "INVALID_FIFO_SIZE",
        }
    }
}
pub fn inspect(
    io_base: u16,
    mut read: impl FnMut(u16, u32) -> u32,
) -> Result<Snapshot, RegisterError> {
    let interface_id = read(io_base, registers::ID);
    if !registers::known_id(interface_id) {
        return Err(RegisterError::VersionNotNegotiated);
    }
    let max_width = read(io_base, registers::MAX_WIDTH);
    let max_height = read(io_base, registers::MAX_HEIGHT);
    if !(320..=16384).contains(&max_width) || !(200..=16384).contains(&max_height) {
        return Err(RegisterError::InvalidDimensions);
    }
    let vram_bytes = read(io_base, registers::VRAM_SIZE);
    if !(1024 * 1024..=1024 * 1024 * 1024).contains(&vram_bytes) {
        return Err(RegisterError::InvalidVram);
    }
    let framebuffer_bytes = read(io_base, registers::FB_SIZE);
    let framebuffer_offset = read(io_base, registers::FB_OFFSET);
    if framebuffer_bytes < 1024 * 1024
        || framebuffer_bytes > vram_bytes
        || framebuffer_offset >= framebuffer_bytes
    {
        return Err(RegisterError::InvalidFramebuffer);
    }
    let (fifo_bytes, capability_bits) = if interface_id >= registers::SVGA_ID_1 {
        let bytes = read(io_base, registers::MEM_SIZE);
        if !(4096..=64 * 1024 * 1024).contains(&bytes) || bytes & 3 != 0 {
            return Err(RegisterError::InvalidFifo);
        }
        (Some(bytes), Some(read(io_base, registers::CAPABILITIES)))
    } else {
        (None, None)
    };
    // Readback is not proof of MMIO BAR sizes or runtime mode availability.
    Ok(Snapshot {
        interface_id,
        max_width,
        max_height,
        vram_bytes,
        framebuffer_bytes,
        framebuffer_offset,
        fifo_bytes,
        capability_bits,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn reg(_: u16, index: u32) -> u32 {
        match index {
            registers::ID => registers::SVGA_ID_2,
            registers::MAX_WIDTH => 3840,
            registers::MAX_HEIGHT => 2160,
            registers::VRAM_SIZE => 128 * 1024 * 1024,
            registers::FB_SIZE => 64 * 1024 * 1024,
            registers::FB_OFFSET => 0,
            registers::MEM_SIZE => 1024 * 1024,
            registers::CAPABILITIES => 0x1234,
            _ => panic!("unknown register"),
        }
    }
    #[test]
    fn reads_bounded_capability_snapshot() {
        let s = inspect(0x1070, reg).unwrap();
        assert_eq!(s.interface_id, registers::SVGA_ID_2);
        assert_eq!(s.fifo_bytes, Some(1024 * 1024));
        assert_eq!(s.capability_bits, Some(0x1234));
    }
    #[test]
    fn unnegotiated_id_stops_reads() {
        let mut calls = 0;
        let r = inspect(0x1070, |_, index| {
            calls += 1;
            assert_eq!(index, registers::ID);
            0
        });
        assert_eq!(r, Err(RegisterError::VersionNotNegotiated));
        assert_eq!(calls, 1);
    }
    #[test]
    fn rejects_corrupt_sizes_and_geometry() {
        assert_eq!(
            inspect(0x1070, |p, i| if i == registers::MAX_WIDTH {
                0
            } else {
                reg(p, i)
            }),
            Err(RegisterError::InvalidDimensions)
        );
        assert_eq!(
            inspect(0x1070, |p, i| if i == registers::VRAM_SIZE {
                u32::MAX
            } else {
                reg(p, i)
            }),
            Err(RegisterError::InvalidVram)
        );
        assert_eq!(
            inspect(0x1070, |p, i| if i == registers::FB_OFFSET {
                128 * 1024 * 1024
            } else {
                reg(p, i)
            }),
            Err(RegisterError::InvalidFramebuffer)
        );
        assert_eq!(
            inspect(0x1070, |p, i| if i == registers::MEM_SIZE {
                0
            } else {
                reg(p, i)
            }),
            Err(RegisterError::InvalidFifo)
        );
    }
    #[test]
    fn oldest_interface_never_touches_fifo_registers() {
        let s = inspect(0x1070, |p, i| {
            assert!(i != registers::MEM_SIZE && i != registers::CAPABILITIES);
            if i == registers::ID {
                registers::SVGA_ID_0
            } else {
                reg(p, i)
            }
        })
        .unwrap();
        assert_eq!(s.fifo_bytes, None);
    }
}
