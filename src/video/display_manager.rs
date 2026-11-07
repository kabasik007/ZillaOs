//! Pure no_std transaction logic for a future runtime DisplayManager.
//!
//! Not connected to the current GOP framebuffer, Settings or WindowManager.
//! Real backends must implement atomic surface allocation and viewport
//! preparation as part of prepare_mode before issuing physical mode writes.
//! The user must confirm the mode before the new mode is considered stable.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    pub bpp: u16,
}

impl Mode {
    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            bpp: 32,
        }
    }

    pub fn is_sane(self) -> bool {
        self.bpp == 32
            && (320..=8192).contains(&self.width)
            && (200..=8192).contains(&self.height)
            && u64::from(self.width) * u64::from(self.height) * 4 <= 256 * 1024 * 1024
    }
}

pub const PROPOSED_MODES: [Mode; 5] = [
    Mode::new(800, 600),
    Mode::new(1024, 768),
    Mode::new(1280, 800),
    Mode::new(1600, 900),
    Mode::new(1920, 1080),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendFailure {
    Unavailable,
    Rejected,
    ReadbackFailed,
    /// The previously connected output vanished or cannot be trusted.
    LostOutput,
}

/// Backend is responsible for proving the mode is actually supported; merely
/// checking SVGA MAX_WIDTH/MAX_HEIGHT is NOT proof of mode availability.
/// A future backend must guarantee prepare_mode changes no visible mode,
/// and preserve resources for safe rollback until user confirmation.
pub trait ModeBackend {
    fn runtime_switch_available(&self) -> bool;
    fn supports_mode(&self, mode: Mode) -> bool;
    fn prepare_mode(&mut self, mode: Mode) -> Result<(), BackendFailure>;
    fn set_mode(&mut self, mode: Mode) -> Result<(), BackendFailure>;
    fn read_mode(&mut self) -> Result<Mode, BackendFailure>;
    fn can_restore(&self, previous: Mode) -> bool;

    /// TEST_ONLY phase with no hardware writes. Legacy implementors retain
    /// trait/source compatibility but fail closed until they explicitly
    /// implement a safe device-specific atomic validator. supports_mode()
    /// alone is NOT sufficient proof of usable hardware resources.
    fn test_mode(&mut self, _mode: Mode) -> Result<(), BackendFailure> {
        Err(BackendFailure::Unavailable)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayError {
    NotAvailable,
    InvalidMode,
    Unsupported,
    AlreadyPending,
    NoPendingMode,
    Poisoned,
    PreparationFailed,
    AtomicTestFailed,
    OutputLost,
    ApplyFailed,
    ReadbackMismatch,
    RollbackFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Apply {
    AlreadyCurrent,
    AwaitingConfirmation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayManager {
    confirmed: Mode,
    current: Mode,
    pending_previous: Option<Mode>,
    poisoned: bool,
}

impl DisplayManager {
    /// Current must be obtained from a validated boot framebuffer or a real
    /// device readback; this model does not inspect hardware on creation.
    pub fn new(boot_mode: Mode) -> Option<Self> {
        if !boot_mode.is_sane() {
            return None;
        }
        Some(Self {
            confirmed: boot_mode,
            current: boot_mode,
            pending_previous: None,
            poisoned: false,
        })
    }

    pub const fn confirmed_mode(&self) -> Mode {
        self.confirmed
    }

    pub const fn current_mode(&self) -> Mode {
        self.current
    }

    pub const fn awaiting_confirmation(&self) -> bool {
        self.pending_previous.is_some()
    }

    pub const fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    /// This is a list of *candidate* modes filtered by backend evidence;
    /// a device maximum width/height alone cannot validate these modes.
    pub fn supported_presets(&self, backend: &impl ModeBackend) -> [Option<Mode>; 5] {
        let mut modes = [None; 5];
        if backend.runtime_switch_available() && !self.poisoned {
            for (index, mode) in PROPOSED_MODES.iter().copied().enumerate() {
                if backend.supports_mode(mode) {
                    modes[index] = Some(mode);
                }
            }
        }
        modes
    }

    fn verify_current(&mut self, backend: &mut impl ModeBackend) -> Result<(), DisplayError> {
        match backend.read_mode() {
            Ok(observed) if observed == self.current => Ok(()),
            Err(BackendFailure::LostOutput) => {
                self.poisoned = true;
                Err(DisplayError::OutputLost)
            }
            _ => {
                // A stale manager snapshot must never authorize a modeset.
                self.poisoned = true;
                Err(DisplayError::ReadbackMismatch)
            }
        }
    }

    fn rollback(
        &mut self,
        backend: &mut impl ModeBackend,
        previous: Mode,
    ) -> Result<(), DisplayError> {
        // A disconnected output cannot be restored by blindly writing its
        // previous mode. Do not attempt hardware writes after a lost readback.
        if backend.read_mode() == Err(BackendFailure::LostOutput) {
            self.poisoned = true;
            self.pending_previous = None;
            return Err(DisplayError::OutputLost);
        }
        // If restoration cannot be proved safe, fail closed rather than
        // guessing that GOP or the prior SVGA mode survived a mode write.
        if !backend.can_restore(previous)
            || backend.prepare_mode(previous).is_err()
            || backend.set_mode(previous).is_err()
            || backend.read_mode() != Ok(previous)
        {
            self.poisoned = true;
            self.pending_previous = None;
            return Err(DisplayError::RollbackFailed);
        }
        self.current = previous;
        self.pending_previous = None;
        Ok(())
    }

    /// Apply is provisional. The DisplayManager must not notify the UI of a
    /// confirmed mode until readback succeeds and the user confirms.
    pub fn apply(
        &mut self,
        backend: &mut impl ModeBackend,
        requested: Mode,
    ) -> Result<Apply, DisplayError> {
        if self.poisoned {
            return Err(DisplayError::Poisoned);
        }
        if self.awaiting_confirmation() {
            return Err(DisplayError::AlreadyPending);
        }
        if !backend.runtime_switch_available() {
            return Err(DisplayError::NotAvailable);
        }
        if !requested.is_sane() {
            return Err(DisplayError::InvalidMode);
        }
        if !backend.supports_mode(requested) {
            return Err(DisplayError::Unsupported);
        }
        self.verify_current(backend)?;
        if requested == self.current {
            return Ok(Apply::AlreadyCurrent);
        }
        // Order is invariant: current hardware readback -> TEST_ONLY
        // -> prepare resources -> commit hardware -> final readback.
        // No mode or FIFO writes are permitted during test_mode.
        match backend.test_mode(requested) {
            Ok(()) => {}
            Err(BackendFailure::LostOutput) => {
                self.poisoned = true;
                return Err(DisplayError::OutputLost);
            }
            Err(_) => return Err(DisplayError::AtomicTestFailed),
        }
        match backend.prepare_mode(requested) {
            Ok(()) => {}
            Err(BackendFailure::LostOutput) => {
                self.poisoned = true;
                return Err(DisplayError::OutputLost);
            }
            Err(_) => return Err(DisplayError::PreparationFailed),
        }
        let prior = self.current;
        match backend.set_mode(requested) {
            Ok(()) => {}
            Err(BackendFailure::LostOutput) => {
                self.poisoned = true;
                return Err(DisplayError::OutputLost);
            }
            Err(_) => {
                self.rollback(backend, prior)?;
                return Err(DisplayError::ApplyFailed);
            }
        }
        match backend.read_mode() {
            Ok(observed) if observed == requested => {}
            Err(BackendFailure::LostOutput) => {
                self.poisoned = true;
                return Err(DisplayError::OutputLost);
            }
            _ => {
                self.rollback(backend, prior)?;
                return Err(DisplayError::ReadbackMismatch);
            }
        }
        self.current = requested;
        self.pending_previous = Some(prior);
        Ok(Apply::AwaitingConfirmation)
    }

    /// Confirm after explicit user approval, with a final hardware readback.
    pub fn confirm(&mut self, backend: &mut impl ModeBackend) -> Result<(), DisplayError> {
        let Some(prior) = self.pending_previous else {
            return Err(DisplayError::NoPendingMode);
        };
        if self.poisoned {
            return Err(DisplayError::Poisoned);
        }
        match backend.read_mode() {
            Ok(mode) if mode == self.current => {}
            Err(BackendFailure::LostOutput) => {
                self.poisoned = true;
                self.pending_previous = None;
                return Err(DisplayError::OutputLost);
            }
            _ => {
                self.rollback(backend, prior)?;
                return Err(DisplayError::ReadbackMismatch);
            }
        }
        self.confirmed = self.current;
        self.pending_previous = None;
        Ok(())
    }

    /// Cancel or external confirmation deadline: restore and verify old mode.
    pub fn cancel(&mut self, backend: &mut impl ModeBackend) -> Result<(), DisplayError> {
        let Some(prior) = self.pending_previous else {
            return Err(DisplayError::NoPendingMode);
        };
        if self.poisoned {
            return Err(DisplayError::Poisoned);
        }
        self.rollback(backend, prior)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockDisplay {
        current: Mode,
        available: bool,
        failed_apply: bool,
        wrong_readback: bool,
        restorable: bool,
        prepared: usize,
        writes: usize,
        fail_prepare: bool,
        fail_atomic_test: bool,
        lost_output: bool,
        lose_after_set: bool,
        tested: usize,
    }

    impl MockDisplay {
        fn new() -> Self {
            Self {
                current: Mode::new(1024, 768),
                available: true,
                failed_apply: false,
                wrong_readback: false,
                restorable: true,
                prepared: 0,
                writes: 0,
                fail_prepare: false,
                fail_atomic_test: false,
                lost_output: false,
                lose_after_set: false,
                tested: 0,
            }
        }
    }

    impl ModeBackend for MockDisplay {
        fn runtime_switch_available(&self) -> bool {
            self.available
        }
        fn supports_mode(&self, m: Mode) -> bool {
            matches!(
                (m.width, m.height, m.bpp),
                (800, 600, 32) | (1024, 768, 32) | (1280, 800, 32)
            )
        }
        fn test_mode(&mut self, mode: Mode) -> Result<(), BackendFailure> {
            self.tested += 1;
            if self.lost_output {
                return Err(BackendFailure::LostOutput);
            }
            if self.fail_atomic_test {
                return Err(BackendFailure::Rejected);
            }
            if self.supports_mode(mode) {
                Ok(())
            } else {
                Err(BackendFailure::Rejected)
            }
        }
        fn prepare_mode(&mut self, _: Mode) -> Result<(), BackendFailure> {
            self.prepared += 1;
            if self.fail_prepare {
                Err(BackendFailure::Rejected)
            } else {
                Ok(())
            }
        }
        fn set_mode(&mut self, m: Mode) -> Result<(), BackendFailure> {
            self.writes += 1;
            if self.failed_apply && m.width == 1280 {
                return Err(BackendFailure::Rejected);
            }
            self.current = m;
            if self.lose_after_set && m.width == 1280 {
                self.lost_output = true;
            }
            Ok(())
        }
        fn read_mode(&mut self) -> Result<Mode, BackendFailure> {
            if self.lost_output {
                return Err(BackendFailure::LostOutput);
            }
            if self.wrong_readback && self.current.width == 1280 {
                Ok(Mode::new(800, 600))
            } else {
                Ok(self.current)
            }
        }
        fn can_restore(&self, _: Mode) -> bool {
            self.restorable
        }
    }

    #[test]
    fn no_backend_means_no_fake_runtime_modes() {
        let mut gpu = MockDisplay::new();
        gpu.available = false;
        let mut mgr = DisplayManager::new(gpu.current).unwrap();
        assert_eq!(mgr.supported_presets(&gpu), [None; 5]);
        assert_eq!(
            mgr.apply(&mut gpu, Mode::new(1280, 800)),
            Err(DisplayError::NotAvailable)
        );
        assert_eq!(gpu.writes, 0);
    }

    #[test]
    fn only_backend_confirmed_presets_are_listed() {
        let gpu = MockDisplay::new();
        let mgr = DisplayManager::new(gpu.current).unwrap();
        let modes = mgr.supported_presets(&gpu);
        assert_eq!(modes[0], Some(Mode::new(800, 600)));
        assert_eq!(modes[1], Some(Mode::new(1024, 768)));
        assert_eq!(modes[2], Some(Mode::new(1280, 800)));
        assert_eq!(modes[3], None);
        assert_eq!(modes[4], None);
    }

    #[test]
    fn apply_confirm_and_cancel_preserve_confirmed_state() {
        let mut gpu = MockDisplay::new();
        let mut mgr = DisplayManager::new(gpu.current).unwrap();
        assert_eq!(
            mgr.apply(&mut gpu, Mode::new(1280, 800)),
            Ok(Apply::AwaitingConfirmation)
        );
        assert!(mgr.awaiting_confirmation());
        assert_eq!(mgr.confirmed_mode(), Mode::new(1024, 768));
        assert_eq!(
            mgr.apply(&mut gpu, Mode::new(800, 600)),
            Err(DisplayError::AlreadyPending)
        );
        assert_eq!(mgr.cancel(&mut gpu), Ok(()));
        assert_eq!(mgr.current_mode(), Mode::new(1024, 768));
        assert_eq!(
            mgr.apply(&mut gpu, Mode::new(1280, 800)),
            Ok(Apply::AwaitingConfirmation)
        );
        assert_eq!(mgr.confirm(&mut gpu), Ok(()));
        assert_eq!(mgr.confirmed_mode(), Mode::new(1280, 800));
    }

    #[test]
    fn failed_set_mode_rolls_back_and_reports_failure() {
        let mut gpu = MockDisplay::new();
        gpu.failed_apply = true;
        let mut mgr = DisplayManager::new(gpu.current).unwrap();
        assert_eq!(
            mgr.apply(&mut gpu, Mode::new(1280, 800)),
            Err(DisplayError::ApplyFailed)
        );
        assert_eq!(mgr.current_mode(), Mode::new(1024, 768));
        assert!(!mgr.awaiting_confirmation());
        assert!(!mgr.is_poisoned());
    }

    #[test]
    fn wrong_readback_rolls_back_and_never_confirms() {
        let mut gpu = MockDisplay::new();
        gpu.wrong_readback = true;
        let mut mgr = DisplayManager::new(gpu.current).unwrap();
        assert_eq!(
            mgr.apply(&mut gpu, Mode::new(1280, 800)),
            Err(DisplayError::ReadbackMismatch)
        );
        assert_eq!(gpu.current, Mode::new(1024, 768));
        assert_eq!(mgr.confirmed_mode(), Mode::new(1024, 768));
    }

    #[test]
    fn unsafe_rollback_poison_prevents_all_future_changes() {
        let mut gpu = MockDisplay::new();
        gpu.failed_apply = true;
        gpu.restorable = false;
        let mut mgr = DisplayManager::new(gpu.current).unwrap();
        assert_eq!(
            mgr.apply(&mut gpu, Mode::new(1280, 800)),
            Err(DisplayError::RollbackFailed)
        );
        assert!(mgr.is_poisoned());
        assert_eq!(
            mgr.apply(&mut gpu, Mode::new(800, 600)),
            Err(DisplayError::Poisoned)
        );
    }

    #[test]
    fn unsupported_and_invalid_modes_are_rejected_without_writes() {
        let mut gpu = MockDisplay::new();
        let mut mgr = DisplayManager::new(gpu.current).unwrap();
        assert_eq!(
            mgr.apply(&mut gpu, Mode::new(1920, 1080)),
            Err(DisplayError::Unsupported)
        );
        assert_eq!(
            mgr.apply(&mut gpu, Mode::new(0, 0)),
            Err(DisplayError::InvalidMode)
        );
        assert_eq!(gpu.writes, 0);
        assert_eq!(gpu.prepared, 0);
    }

    #[test]
    fn prepare_error_does_not_change_visible_mode() {
        let mut gpu = MockDisplay::new();
        gpu.fail_prepare = true;
        let mut mgr = DisplayManager::new(gpu.current).unwrap();
        assert_eq!(
            mgr.apply(&mut gpu, Mode::new(1280, 800)),
            Err(DisplayError::PreparationFailed)
        );
        assert_eq!(gpu.writes, 0);
        assert_eq!(mgr.current_mode(), Mode::new(1024, 768));
    }

    #[test]
    fn same_mode_still_requires_device_readback() {
        let mut gpu = MockDisplay::new();
        let mut mgr = DisplayManager::new(gpu.current).unwrap();
        gpu.current = Mode::new(800, 600);
        assert_eq!(
            mgr.apply(&mut gpu, Mode::new(1024, 768)),
            Err(DisplayError::ReadbackMismatch)
        );
        assert_eq!(gpu.writes, 0);
    }

    #[test]
    fn legacy_backend_without_atomic_test_fails_closed() {
        struct Legacy(MockDisplay);
        impl ModeBackend for Legacy {
            fn runtime_switch_available(&self) -> bool {
                self.0.runtime_switch_available()
            }
            fn supports_mode(&self, mode: Mode) -> bool {
                self.0.supports_mode(mode)
            }
            fn prepare_mode(&mut self, mode: Mode) -> Result<(), BackendFailure> {
                self.0.prepare_mode(mode)
            }
            fn set_mode(&mut self, mode: Mode) -> Result<(), BackendFailure> {
                self.0.set_mode(mode)
            }
            fn read_mode(&mut self) -> Result<Mode, BackendFailure> {
                self.0.read_mode()
            }
            fn can_restore(&self, mode: Mode) -> bool {
                self.0.can_restore(mode)
            }
        }
        let mut legacy = Legacy(MockDisplay::new());
        let mut mgr = DisplayManager::new(legacy.0.current).unwrap();
        assert_eq!(
            mgr.apply(&mut legacy, Mode::new(1280, 800)),
            Err(DisplayError::AtomicTestFailed)
        );
        assert_eq!(legacy.0.writes, 0);
        assert_eq!(legacy.0.prepared, 0);
    }

    #[test]
    fn atomic_test_denial_never_prepares_or_writes() {
        let mut gpu = MockDisplay::new();
        gpu.fail_atomic_test = true;
        let mut mgr = DisplayManager::new(gpu.current).unwrap();
        assert_eq!(
            mgr.apply(&mut gpu, Mode::new(1280, 800)),
            Err(DisplayError::AtomicTestFailed)
        );
        assert_eq!(gpu.tested, 1);
        assert_eq!(gpu.prepared, 0);
        assert_eq!(gpu.writes, 0);
        assert_eq!(mgr.current_mode(), Mode::new(1024, 768));
        assert!(!mgr.is_poisoned());
    }

    #[test]
    fn disappearing_output_before_commit_poison_without_writes() {
        let mut gpu = MockDisplay::new();
        gpu.lost_output = true;
        let mut mgr = DisplayManager::new(Mode::new(1024, 768)).unwrap();
        assert_eq!(
            mgr.apply(&mut gpu, Mode::new(1280, 800)),
            Err(DisplayError::OutputLost)
        );
        assert!(mgr.is_poisoned());
        assert_eq!(gpu.tested, 0);
        assert_eq!(gpu.prepared, 0);
        assert_eq!(gpu.writes, 0);
        assert_eq!(
            mgr.apply(&mut gpu, Mode::new(800, 600)),
            Err(DisplayError::Poisoned)
        );
    }

    #[test]
    fn disappearing_output_after_commit_never_attempts_blind_rollback() {
        let mut gpu = MockDisplay::new();
        gpu.lose_after_set = true;
        let mut mgr = DisplayManager::new(gpu.current).unwrap();
        assert_eq!(
            mgr.apply(&mut gpu, Mode::new(1280, 800)),
            Err(DisplayError::OutputLost)
        );
        assert!(mgr.is_poisoned());
        assert_eq!(gpu.writes, 1);
        assert_eq!(mgr.confirmed_mode(), Mode::new(1024, 768));
    }

    #[test]
    fn stale_initial_readback_poison_before_atomic_test() {
        let mut gpu = MockDisplay::new();
        let mut mgr = DisplayManager::new(gpu.current).unwrap();
        gpu.current = Mode::new(800, 600);
        assert_eq!(
            mgr.apply(&mut gpu, Mode::new(1280, 800)),
            Err(DisplayError::ReadbackMismatch)
        );
        assert!(mgr.is_poisoned());
        assert_eq!(gpu.tested, 0);
        assert_eq!(gpu.writes, 0);
    }

    #[test]
    fn cannot_confirm_without_a_pending_device_mode() {
        let mut gpu = MockDisplay::new();
        let mut mgr = DisplayManager::new(gpu.current).unwrap();
        assert_eq!(mgr.confirm(&mut gpu), Err(DisplayError::NoPendingMode));
        assert_eq!(mgr.cancel(&mut gpu), Err(DisplayError::NoPendingMode));
        assert_eq!(
            mgr.apply(&mut gpu, Mode::new(1024, 768)),
            Ok(Apply::AlreadyCurrent)
        );
    }
}
