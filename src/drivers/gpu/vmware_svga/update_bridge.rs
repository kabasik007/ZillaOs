//! Pure bridge from a validated scanout damage rectangle to the SVGA II
//! five-DWORD UPDATE command. This does not touch PCI BAR, FIFO or GPU.
//!
//! A real adapter must independently prove that this surface is the current
//! hardware scanout and that the selected device mode/format was read back.

use crate::video::display_contracts::{DamageRect, PixelFormat, SurfaceDescriptor, SurfaceError};
use crate::video::display_manager::Mode;

use super::fifo::{self, UpdateError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeError {
    Surface(SurfaceError),
    UnsupportedFormat,
    ScanoutModeMismatch,
    Update(UpdateError),
}

/// Produce a command payload only. No queue submission, sync, MMIO or
/// hardware claim is implied by this successful arithmetic conversion.
pub fn encode_scanout_damage(
    surface: SurfaceDescriptor,
    damage: DamageRect,
    readback_mode: Mode,
) -> Result<[u32; 5], BridgeError> {
    surface.validate().map_err(BridgeError::Surface)?;
    if surface.layout.format != PixelFormat::Xrgb8888 {
        return Err(BridgeError::UnsupportedFormat);
    }
    if readback_mode.bpp != 32
        || readback_mode.width != surface.layout.width
        || readback_mode.height != surface.layout.height
    {
        return Err(BridgeError::ScanoutModeMismatch);
    }
    damage.validate(surface.layout).map_err(BridgeError::Surface)?;
    fifo::encode_update(
        damage.x, damage.y, damage.width, damage.height,
        surface.layout.width, surface.layout.height,
    ).map_err(BridgeError::Update)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::display_contracts::{BufferLayout, SurfaceOwner};

    fn source() -> SurfaceDescriptor {
        SurfaceDescriptor {
            owner: SurfaceOwner::Kernel,
            layout: BufferLayout {
                width: 1024, height: 768, pitch_bytes: 4096,
                format: PixelFormat::Xrgb8888,
                byte_length: 4096 * 768,
            },
        }
    }

    #[test]
    fn valid_damage_generates_exactly_five_words() {
        assert_eq!(encode_scanout_damage(
            source(),
            DamageRect { x: 900, y: 700, width: 124, height: 68 },
            Mode::new(1024, 768),
        ), Ok([1, 900, 700, 124, 68]));
    }

    #[test]
    fn rejects_unverified_mode_format_and_bounds() {
        let damage = DamageRect { x: 0, y: 0, width: 50, height: 50 };
        assert_eq!(encode_scanout_damage(source(), damage, Mode::new(800, 600)),
            Err(BridgeError::ScanoutModeMismatch));
        let mut surface = source();
        surface.layout.format = PixelFormat::Argb8888;
        assert_eq!(encode_scanout_damage(surface, damage, Mode::new(1024, 768)),
            Err(BridgeError::UnsupportedFormat));
        surface = source();
        surface.layout.pitch_bytes = 4;
        assert_eq!(encode_scanout_damage(surface, damage, Mode::new(1024, 768)),
            Err(BridgeError::Surface(SurfaceError::InvalidStride)));
        assert_eq!(encode_scanout_damage(
            source(),
            DamageRect { x: u32::MAX, y: 0, width: 2, height: 1 },
            Mode::new(1024, 768),
        ), Err(BridgeError::Surface(SurfaceError::InvalidDamage)));
    }
}
