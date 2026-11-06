//! Stage 3: testable, fail-closed SVGA FIFO software writer.
//!
//! No physical MMIO implementation is supplied. The backend contract
//! deliberately separates payload writes from publication of NEXT_CMD:
//! a future UC-mapped driver must implement ordering and exclusive access.
//! Creating this object does NOT enable device FIFO or display hardware.

use core::sync::atomic::{compiler_fence, Ordering};

use super::fifo::{FifoError, FifoPointers};

pub const MIN_FIFO_DATA_BYTES: u32 = 10 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendError {
    Unavailable,
    Fault,
}

/// A prevalidated and exclusively owned FIFO memory window.
/// This trait is implemented only by tests until a kernel MMIO mapper exists.
/// Implementors MUST provide non-aliased, bounds-checked DWORD accesses.
/// publish_next must make preceding payload writes visible to the device
/// before updating FIFO_NEXT_CMD. A compiler fence alone does not provide
/// the hardware MMIO ordering required by arbitrary architectures/cache types.
pub trait FifoBackend {
    fn word_count(&self) -> usize;
    fn read_word(&mut self, index: usize) -> Result<u32, BackendError>;
    fn write_word(&mut self, index: usize, value: u32) -> Result<(), BackendError>;
    fn publish_next(&mut self, next: u32) -> Result<(), BackendError>;
}

/// Separate register-based synchronization for a future device adapter.
/// The caller must guarantee SYNC and BUSY indices refer to negotiated SVGA.
pub trait SyncBackend {
    fn request_sync(&mut self) -> Result<(), BackendError>;
    fn read_busy(&mut self) -> Result<bool, BackendError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueError {
    Faulted,
    InvalidAperture,
    InvalidCommand,
    BadMetadata(FifoError),
    InsufficientSpace,
    Backend(BackendError),
    SyncTimeout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FifoQueue {
    aperture_bytes: u32,
    faulted: bool,
}

impl FifoQueue {
    /// This represents a logical FIFO, not proof of a physically mapped BAR.
    pub const fn new(aperture_bytes: u32) -> Self {
        Self {
            aperture_bytes,
            faulted: false,
        }
    }

    pub const fn is_faulted(&self) -> bool {
        self.faulted
    }

    fn backend_failure(&mut self, error: BackendError) -> QueueError {
        self.faulted = true;
        QueueError::Backend(error)
    }

    /// Attempts to enqueue a complete DWORD command, without blocking.
    /// Returns InsufficientSpace without changing any FIFO word.
    /// On write/publication failure the queue is poisoned: the last
    /// hardware-visible NEXT_CMD may be indeterminate and must not be reused.
    pub fn try_enqueue(
        &mut self,
        backend: &mut impl FifoBackend,
        command: &[u32],
    ) -> Result<u32, QueueError> {
        if self.faulted {
            return Err(QueueError::Faulted);
        }
        if self.aperture_bytes < MIN_FIFO_DATA_BYTES + 16
            || self.aperture_bytes & 3 != 0
            || backend.word_count() != (self.aperture_bytes / 4) as usize
        {
            self.faulted = true;
            return Err(QueueError::InvalidAperture);
        }
        let bytes = u32::try_from(command.len())
            .ok()
            .and_then(|count| count.checked_mul(4))
            .filter(|count| *count != 0)
            .ok_or(QueueError::InvalidCommand)?;
        let mut raw = [0u32; 4];
        for (index, value) in raw.iter_mut().enumerate() {
            *value = backend.read_word(index).map_err(|e| self.backend_failure(e))?;
        }
        let pointers = FifoPointers {
            min: raw[0],
            max: raw[1],
            next: raw[2],
            stop: raw[3],
        };
        pointers.validate(self.aperture_bytes).map_err(|e| {
            self.faulted = true;
            QueueError::BadMetadata(e)
        })?;
        if (pointers.max - pointers.min) < MIN_FIFO_DATA_BYTES {
            self.faulted = true;
            return Err(QueueError::InvalidAperture);
        }
        if !pointers.can_enqueue(self.aperture_bytes, bytes)
            .map_err(QueueError::BadMetadata)?
        {
            return Err(QueueError::InsufficientSpace);
        }
        let mut next = pointers.next;
        for word in command {
            // Validated pointers are DWORD-aligned and strictly inside the
            // region; every index is < backend.word_count.
            backend
                .write_word((next / 4) as usize, *word)
                .map_err(|e| self.backend_failure(e))?;
            next += 4;
            if next == pointers.max {
                next = pointers.min;
            }
        }
        // Publication is the commit point; no other writer may modify
        // NEXT_CMD or the yet-unpublished payload concurrently.
        compiler_fence(Ordering::Release);
        backend
            .publish_next(next)
            .map_err(|e| self.backend_failure(e))?;
        Ok(next)
    }

    /// Bound the number of BUSY register polls. This is an iteration
    /// budget, NOT a millisecond wall-clock timeout (timer unavailable).
    /// Any failure poisons the queue; do not assume recovery succeeded.
    pub fn synchronize(
        &mut self,
        backend: &mut impl SyncBackend,
        max_polls: u32,
    ) -> Result<(), QueueError> {
        if self.faulted {
            return Err(QueueError::Faulted);
        }
        if max_polls == 0 || max_polls > 1_000_000 {
            return Err(QueueError::InvalidCommand);
        }
        backend
            .request_sync()
            .map_err(|e| self.backend_failure(e))?;
        for _ in 0..max_polls {
            if !backend.read_busy().map_err(|e| self.backend_failure(e))? {
                return Ok(());
            }
        }
        self.faulted = true;
        Err(QueueError::SyncTimeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockFifo {
        words: [u32; 4096],
        publishes: usize,
        writes: usize,
        fail_write_at: Option<usize>,
    }

    impl MockFifo {
        fn new(next: u32, stop: u32) -> Self {
            let mut words = [0u32; 4096];
            words[0] = 16;
            words[1] = 4096 * 4;
            words[2] = next;
            words[3] = stop;
            Self {
                words,
                publishes: 0,
                writes: 0,
                fail_write_at: None,
            }
        }
    }

    impl FifoBackend for MockFifo {
        fn word_count(&self) -> usize {
            self.words.len()
        }
        fn read_word(&mut self, index: usize) -> Result<u32, BackendError> {
            self.words.get(index).copied().ok_or(BackendError::Fault)
        }
        fn write_word(&mut self, index: usize, value: u32) -> Result<(), BackendError> {
            self.writes += 1;
            if self.fail_write_at == Some(self.writes) {
                return Err(BackendError::Fault);
            }
            let slot = self.words.get_mut(index).ok_or(BackendError::Fault)?;
            *slot = value;
            Ok(())
        }
        fn publish_next(&mut self, next: u32) -> Result<(), BackendError> {
            self.publishes += 1;
            self.words[2] = next;
            Ok(())
        }
    }

    struct MockSync {
        polls: u32,
        ready_after: u32,
        requests: u32,
        broken: bool,
    }

    impl SyncBackend for MockSync {
        fn request_sync(&mut self) -> Result<(), BackendError> {
            self.requests += 1;
            Ok(())
        }
        fn read_busy(&mut self) -> Result<bool, BackendError> {
            if self.broken {
                return Err(BackendError::Fault);
            }
            self.polls += 1;
            Ok(self.polls < self.ready_after)
        }
    }

    #[test]
    fn command_is_published_once_after_all_payload_words() {
        let mut memory = MockFifo::new(4096 * 4 - 8, 64);
        let mut queue = FifoQueue::new(4096 * 4);
        let cmd = super::super::fifo::encode_update(
            0, 0, 40, 30, 1920, 1080
        ).unwrap();
        assert_eq!(queue.try_enqueue(&mut memory, &cmd), Ok(28));
        assert_eq!(memory.words[4094], 1);
        assert_eq!(memory.words[4095], 0);
        assert_eq!(&memory.words[4..7], &[0, 40, 30]);
        assert_eq!(memory.words[2], 28);
        assert_eq!(memory.words[3], 64);
        assert_eq!(memory.publishes, 1);
        assert_eq!(memory.writes, 5);
    }

    #[test]
    fn insufficient_space_does_not_write_or_poison() {
        let mut memory = MockFifo::new(20, 24);
        let mut queue = FifoQueue::new(4096 * 4);
        assert_eq!(
            queue.try_enqueue(&mut memory, &[1, 2, 3, 4, 5]),
            Err(QueueError::InsufficientSpace)
        );
        assert!(!queue.is_faulted());
        assert_eq!(memory.publishes, 0);
        assert_eq!(memory.writes, 0);
    }

    #[test]
    fn corrupt_header_fails_closed_before_payload() {
        let mut memory = MockFifo::new(4096 * 4, 16);
        let mut queue = FifoQueue::new(4096 * 4);
        assert_eq!(
            queue.try_enqueue(&mut memory, &[1, 2, 3]),
            Err(QueueError::BadMetadata(FifoError::InvalidBounds))
        );
        assert!(queue.is_faulted());
        assert_eq!(memory.writes, 0);
        assert_eq!(memory.publishes, 0);
    }

    #[test]
    fn partial_backend_write_failure_never_publishes_next() {
        let mut memory = MockFifo::new(16, 16);
        memory.fail_write_at = Some(2);
        let mut queue = FifoQueue::new(4096 * 4);
        assert_eq!(
            queue.try_enqueue(&mut memory, &[1, 2, 3, 4, 5]),
            Err(QueueError::Backend(BackendError::Fault))
        );
        assert_eq!(memory.words[2], 16);
        assert_eq!(memory.publishes, 0);
        assert!(queue.is_faulted());
        assert_eq!(
            queue.try_enqueue(&mut memory, &[10]),
            Err(QueueError::Faulted)
        );
    }

    #[test]
    fn sync_completes_within_budget() {
        let mut queue = FifoQueue::new(4096 * 4);
        let mut sync = MockSync {
            polls: 0,
            ready_after: 3,
            requests: 0,
            broken: false,
        };
        assert_eq!(queue.synchronize(&mut sync, 3), Ok(()));
        assert_eq!(sync.requests, 1);
        assert_eq!(sync.polls, 3);
    }

    #[test]
    fn sync_timeout_poison_is_sticky() {
        let mut queue = FifoQueue::new(4096 * 4);
        let mut sync = MockSync {
            polls: 0,
            ready_after: u32::MAX,
            requests: 0,
            broken: false,
        };
        assert_eq!(queue.synchronize(&mut sync, 4), Err(QueueError::SyncTimeout));
        assert!(queue.is_faulted());
        assert_eq!(sync.polls, 4);
        assert_eq!(queue.synchronize(&mut sync, 4), Err(QueueError::Faulted));
    }

    #[test]
    fn sync_io_error_is_not_mistaken_for_idle() {
        let mut queue = FifoQueue::new(4096 * 4);
        let mut sync = MockSync {
            polls: 0,
            ready_after: 3,
            requests: 0,
            broken: true,
        };
        assert_eq!(
            queue.synchronize(&mut sync, 4),
            Err(QueueError::Backend(BackendError::Fault))
        );
        assert!(queue.is_faulted());
    }

    #[test]
    fn unverified_aperture_length_is_rejected() {
        let mut queue = FifoQueue::new(16 * 1024 - 4);
        let mut memory = MockFifo::new(16, 16);
        assert_eq!(
            queue.try_enqueue(&mut memory, &[1]),
            Err(QueueError::InvalidAperture)
        );
        assert_eq!(memory.writes, 0);
    }
}
