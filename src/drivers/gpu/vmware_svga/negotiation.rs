//! Opt-in SVGA interface ID negotiation, separate from mode setting.
//! VMware svga_interface.txt requires writing a candidate to REG_ID and
//! accepting it only when the readback matches. No other register is written.

use super::registers;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NegotiationError {
    NoSupportedInterface,
    UnstableInterfaceId,
}

impl NegotiationError {
    pub const fn label(self) -> &'static str {
        match self {
            Self::NoSupportedInterface => "NO_SUPPORTED_INTERFACE",
            Self::UnstableInterfaceId => "UNSTABLE_INTERFACE_ID",
        }
    }
}

/// Bounded handshake: at most three ID writes and three readbacks.
/// The caller MUST use a port adapter which permits writes to REG_ID only.
/// A successful handshake does not authorize MMIO, FIFO, or mode switching.
pub fn negotiate(
    mut read: impl FnMut(u32) -> u32,
    mut write: impl FnMut(u32, u32),
) -> Result<u32, NegotiationError> {
    for candidate in [
        registers::SVGA_ID_2,
        registers::SVGA_ID_1,
        registers::SVGA_ID_0,
    ] {
        write(registers::ID, candidate);
        if read(registers::ID) == candidate {
            return Ok(candidate);
        }
    }
    Err(NegotiationError::NoSupportedInterface)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;

    #[test]
    fn negotiates_highest_compatible_interface_in_one_attempt() {
        let active = Cell::new(0);
        let mut writes = 0;
        let result = negotiate(
            |index| {
                assert_eq!(index, registers::ID);
                active.get()
            },
            |index, value| {
                assert_eq!(index, registers::ID);
                assert!(registers::known_id(value));
                writes += 1;
                active.set(value);
            },
        );
        assert_eq!(result, Ok(registers::SVGA_ID_2));
        assert_eq!(writes, 1);
    }

    #[test]
    fn falls_back_from_v2_to_v1_without_other_register_writes() {
        let active = Cell::new(0);
        let mut attempts = [0u32; 3];
        let mut count = 0;
        let result = negotiate(
            |index| {
                assert_eq!(index, registers::ID);
                active.get()
            },
            |index, value| {
                assert_eq!(index, registers::ID);
                attempts[count] = value;
                count += 1;
                active.set(if value == registers::SVGA_ID_2 {
                    registers::SVGA_ID_1
                } else {
                    value
                });
            },
        );
        assert_eq!(result, Ok(registers::SVGA_ID_1));
        assert_eq!(count, 2);
        assert_eq!(attempts[..count], [
            registers::SVGA_ID_2,
            registers::SVGA_ID_1,
        ]);
    }

    #[test]
    fn unsupported_device_fails_after_three_attempts_without_retry_loop() {
        let mut writes = 0;
        let result = negotiate(
            |_| u32::MAX,
            |index, value| {
                assert_eq!(index, registers::ID);
                assert!(registers::known_id(value));
                writes += 1;
            },
        );
        assert_eq!(result, Err(NegotiationError::NoSupportedInterface));
        assert_eq!(writes, 3);
    }

    #[test]
    fn accepts_oldest_interface_after_two_failed_candidates() {
        let active = Cell::new(0);
        let mut count = 0;
        let result = negotiate(
            |_| active.get(),
            |_, value| {
                count += 1;
                active.set(if value == registers::SVGA_ID_0 {
                    value
                } else {
                    u32::MAX
                });
            },
        );
        assert_eq!(result, Ok(registers::SVGA_ID_0));
        assert_eq!(count, 3);
    }
}
