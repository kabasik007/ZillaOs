//! Pure FIFO queue geometry and SVGA_CMD_UPDATE encoding.
//! No MMIO, no FIFO commands are actually submitted to the device.
//! All offsets are bytes, not DWORD indexes.

pub const SVGA_CMD_UPDATE: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FifoPointers {
    pub min: u32,
    pub max: u32,
    pub next: u32,
    pub stop: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FifoError {
    InvalidAperture,
    InvalidAlignment,
    InvalidBounds,
}

impl FifoPointers {
    pub fn validate(self, aperture: u32) -> Result<(), FifoError> {
        if aperture < 24 || aperture & 3 != 0 {
            return Err(FifoError::InvalidAperture);
        }
        if (self.min | self.max | self.next | self.stop) & 3 != 0 {
            return Err(FifoError::InvalidAlignment);
        }
        if self.min < 16
            || self.max > aperture
            || self.max.checked_sub(self.min).is_none_or(|n| n < 8)
            || !(self.min..self.max).contains(&self.next)
            || !(self.min..self.max).contains(&self.stop)
        {
            return Err(FifoError::InvalidBounds);
        }
        Ok(())
    }

    /// Reserve a DWORD so full/empty states are distinguishable.
    /// This is ONLY arithmetic, not a reservation against a moving GPU STOP.
    pub fn can_enqueue(self, aperture: u32, bytes: u32) -> Result<bool, FifoError> {
        self.validate(aperture)?;
        if bytes == 0 || bytes & 3 != 0 {
            return Err(FifoError::InvalidAlignment);
        }
        let free = if self.next >= self.stop {
            u64::from(self.max - self.next) + u64::from(self.stop - self.min)
        } else {
            u64::from(self.stop - self.next)
        };
        Ok(u64::from(bytes) + 4 <= free)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateError {
    EmptyOrInvalidSurface,
    EmptyRect,
    OutOfBounds,
}

/// Encode an UPDATE rectangle in the five DWORDs specified by SVGA II.
/// The caller must prove the destination framebuffer is the one described
/// by surface_width/surface_height and submit only after FIFO initialization.
pub fn encode_update(
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    surface_width: u32,
    surface_height: u32,
) -> Result<[u32; 5], UpdateError> {
    if surface_width == 0 || surface_height == 0 {
        return Err(UpdateError::EmptyOrInvalidSurface);
    }
    if width == 0 || height == 0 {
        return Err(UpdateError::EmptyRect);
    }
    if x.checked_add(width).is_none_or(|end| end > surface_width)
        || y.checked_add(height).is_none_or(|end| end > surface_height)
    {
        return Err(UpdateError::OutOfBounds);
    }
    Ok([SVGA_CMD_UPDATE, x, y, width, height])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_wrap_and_space_are_bounded() {
        let a = FifoPointers {
            min: 16,
            max: 256,
            next: 248,
            stop: 32,
        };
        assert_eq!(a.validate(256), Ok(()));
        assert_eq!(a.can_enqueue(256, 16), Ok(true));
        assert_eq!(a.can_enqueue(256, 32), Ok(false));
        let full = FifoPointers {
            min: 16,
            max: 256,
            next: 20,
            stop: 24,
        };
        assert_eq!(full.can_enqueue(256, 4), Ok(false));
    }

    #[test]
    fn broken_pointers_and_overflow_fail_closed() {
        for bad in [
            FifoPointers {
                min: 16,
                max: 260,
                next: 16,
                stop: 16,
            },
            FifoPointers {
                min: 16,
                max: 256,
                next: 256,
                stop: 16,
            },
            FifoPointers {
                min: 16,
                max: 256,
                next: 20,
                stop: 256,
            },
            FifoPointers {
                min: 12,
                max: 256,
                next: 12,
                stop: 16,
            },
            FifoPointers {
                min: u32::MAX - 3,
                max: u32::MAX,
                next: 16,
                stop: 16,
            },
        ] {
            assert!(bad.validate(256).is_err());
        }
        let ok = FifoPointers {
            min: 16,
            max: 256,
            next: 16,
            stop: 16,
        };
        assert_eq!(
            ok.can_enqueue(256, u32::MAX),
            Err(FifoError::InvalidAlignment)
        );
        assert_eq!(ok.can_enqueue(256, u32::MAX - 3), Ok(false));
    }

    #[test]
    fn update_is_five_words_with_precise_geometry() {
        assert_eq!(
            encode_update(9, 10, 22, 33, 1920, 1080),
            Ok([1, 9, 10, 22, 33])
        );
        assert_eq!(
            encode_update(0, 0, 1920, 1080, 1920, 1080),
            Ok([1, 0, 0, 1920, 1080])
        );
    }

    #[test]
    fn update_rejects_overflow_zero_and_out_of_frame_rectangles() {
        assert_eq!(
            encode_update(0, 0, 1, 1, 0, 1080),
            Err(UpdateError::EmptyOrInvalidSurface)
        );
        assert_eq!(
            encode_update(0, 0, 0, 1, 1920, 1080),
            Err(UpdateError::EmptyRect)
        );
        assert_eq!(
            encode_update(1900, 0, 30, 1, 1920, 1080),
            Err(UpdateError::OutOfBounds)
        );
        assert_eq!(
            encode_update(u32::MAX, 0, 2, 2, 1920, 1080),
            Err(UpdateError::OutOfBounds)
        );
        assert_eq!(
            encode_update(1, u32::MAX, 2, 2, 1920, 1080),
            Err(UpdateError::OutOfBounds)
        );
    }
}
