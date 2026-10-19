//! Pure PS/2 three-byte mouse packet decoding for the Zilla Desktop preview.
//! No port I/O, allocation or privileged instructions: covered by host tests.
//! Protocol: first byte bit 3 set, bits 6/7 report motion overflow.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MousePacket {
    pub dx: i16,
    pub dy: i16,
    pub left: bool,
    pub right: bool,
    pub middle: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseDecoder {
    bytes: [u8; 3],
    length: u8,
}

impl Default for MouseDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl MouseDecoder {
    pub const fn new() -> Self {
        Self {
            bytes: [0; 3],
            length: 0,
        }
    }

    pub fn reset(&mut self) {
        self.length = 0;
    }

    /// Consume one byte received from AUX port. Reject overflow packets,
    /// preserve 9-bit signed motion and invert device Y (screen Y grows down).
    pub fn feed(&mut self, byte: u8) -> Option<MousePacket> {
        if self.length == 0 {
            if byte & 0x08 == 0 {
                return None;
            }
            self.bytes[0] = byte;
            self.length = 1;
            return None;
        }
        self.bytes[self.length as usize] = byte;
        self.length += 1;
        if self.length < 3 {
            return None;
        }
        self.length = 0;
        let flags = self.bytes[0];
        if flags & 0xc0 != 0 {
            return None;
        }
        let dx = (self.bytes[1] as i16) - if flags & 0x10 != 0 { 256 } else { 0 };
        let device_y = (self.bytes[2] as i16) - if flags & 0x20 != 0 { 256 } else { 0 };
        Some(MousePacket {
            dx,
            dy: -device_y,
            left: flags & 1 != 0,
            right: flags & 2 != 0,
            middle: flags & 4 != 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(bytes: [u8; 3]) -> MousePacket {
        let mut decoder = MouseDecoder::new();
        assert_eq!(decoder.feed(bytes[0]), None);
        assert_eq!(decoder.feed(bytes[1]), None);
        decoder.feed(bytes[2]).expect("complete valid packet")
    }

    #[test]
    fn positive_x_and_device_y_up_becomes_screen_y_down() {
        assert_eq!(packet([0x08, 15, 8]), MousePacket {
            dx: 15, dy: -8, left: false, right: false, middle: false,
        });
    }

    #[test]
    fn signed_nine_bit_deltas_and_buttons_are_respected() {
        assert_eq!(packet([0x3b, 0xfe, 0xfd]), MousePacket {
            dx: -2, dy: 3, left: true, right: true, middle: false,
        });
    }

    #[test]
    fn desync_and_overflow_are_rejected() {
        let mut decoder = MouseDecoder::new();
        assert_eq!(decoder.feed(0), None);
        assert_eq!(decoder.feed(0x48), None);
        assert_eq!(decoder.feed(0), None);
        assert_eq!(decoder.feed(0), None);
        assert_eq!(decoder.feed(0x08), None);
        assert_eq!(decoder.feed(0), None);
        assert_eq!(decoder.feed(0), Some(MousePacket {
            dx: 0, dy: 0, left: false, right: false, middle: false
        }));
    }

    #[test]
    fn button_down_and_release_have_distinct_packets() {
        assert!(packet([0x09, 0, 0]).left);
        assert!(!packet([0x08, 0, 0]).left);
    }

    #[test]
    fn reset_discards_incomplete_packet() {
        let mut decoder = MouseDecoder::new();
        decoder.feed(0x08);
        decoder.feed(0x11);
        decoder.reset();
        assert_eq!(decoder.feed(0x08), None);
        assert_eq!(decoder.feed(0), None);
        assert_eq!(decoder.feed(0), Some(MousePacket {
            dx: 0, dy: 0, left: false, right: false, middle: false
        }));
    }
}
