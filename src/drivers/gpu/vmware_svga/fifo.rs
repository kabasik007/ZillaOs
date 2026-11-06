//! Pure FIFO ring arithmetic: NO FIFO BAR2 mapping, commands or GPU writes.
//! Offsets are bytes, not DWORD indexes.
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
        if self.min < 16 || self.max > aperture
            || self.max.checked_sub(self.min).is_none_or(|n| n < 8)
            || !(self.min..self.max).contains(&self.next)
            || !(self.min..self.max).contains(&self.stop)
        {
            return Err(FifoError::InvalidBounds);
        }
        Ok(())
    }
    /// Reserve one DWORD to distinguish full from empty; no host sync claim.
    pub fn can_enqueue(self, aperture: u32, bytes: u32) -> Result<bool, FifoError> {
        self.validate(aperture)?;
        if bytes == 0 || bytes & 3 != 0 { return Err(FifoError::InvalidAlignment); }
        let free = if self.next >= self.stop {
            u64::from(self.max - self.next) + u64::from(self.stop - self.min)
        } else {
            u64::from(self.stop - self.next)
        };
        Ok(u64::from(bytes) + 4 <= free)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ring_wrap_and_space_are_bounded() {
        let a=FifoPointers{min:16,max:256,next:248,stop:32};
        assert_eq!(a.validate(256),Ok(()));
        assert_eq!(a.can_enqueue(256,16),Ok(true));
        assert_eq!(a.can_enqueue(256,32),Ok(false));
        let full=FifoPointers{min:16,max:256,next:20,stop:24};
        assert_eq!(full.can_enqueue(256,4),Ok(false));
    }
    #[test]
    fn broken_pointers_and_overflow_fail_closed() {
        for bad in [
            FifoPointers{min:16,max:260,next:16,stop:16},
            FifoPointers{min:16,max:256,next:256,stop:16},
            FifoPointers{min:16,max:256,next:20,stop:256},
            FifoPointers{min:12,max:256,next:12,stop:16},
            FifoPointers{min:u32::MAX-3,max:u32::MAX,next:16,stop:16},
        ] { assert!(bad.validate(256).is_err()); }
        let ok=FifoPointers{min:16,max:256,next:16,stop:16};
        assert_eq!(ok.can_enqueue(256,u32::MAX),Err(FifoError::InvalidAlignment));
        assert_eq!(ok.can_enqueue(256,u32::MAX-3),Ok(false));
    }
}
