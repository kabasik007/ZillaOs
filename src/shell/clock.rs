//! Early read-only clock value for the graphical desktop.
//! The CMOS driver supplies these fields; this type never reads a device.
//! CMOS can be local or UTC depending on VM firmware configuration,
//! so the UI must label it RTC until timezone rules are implemented.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockTime {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl ClockTime {
    pub const fn new(hour: u8, minute: u8, second: u8) -> Option<Self> {
        if hour > 23 || minute > 59 || second > 59 { return None; }
        Some(Self { hour, minute, second })
    }

    pub fn from_cmos(hour_raw: u8, minute_raw: u8, second_raw: u8,
                     status_b: u8) -> Option<Self> {
        let binary = status_b & 0x04 != 0;
        let is_24h = status_b & 0x02 != 0;
        let pm = hour_raw & 0x80 != 0;
        let hour_val = hour_raw & 0x7f;
        let mut hour = if binary { hour_val } else { from_bcd(hour_val)? };
        let minute = if binary { minute_raw } else { from_bcd(minute_raw)? };
        let second = if binary { second_raw } else { from_bcd(second_raw)? };
        if !is_24h {
            if !(1..=12).contains(&hour) { return None; }
            hour = (hour % 12) + if pm { 12 } else { 0 };
        }
        Self::new(hour, minute, second)
    }

    pub const fn ascii(self) -> [u8; 8] {
        [b'0' + self.hour / 10, b'0' + self.hour % 10, b':',
         b'0' + self.minute / 10, b'0' + self.minute % 10, b':',
         b'0' + self.second / 10, b'0' + self.second % 10]
    }
}

fn from_bcd(value: u8) -> Option<u8> {
    if value & 0x0f > 9 || value >> 4 > 9 { return None; }
    Some((value >> 4) * 10 + (value & 0x0f))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bcd_and_binary_24h_clocks() {
        assert_eq!(ClockTime::from_cmos(0x23, 0x59, 0x58, 0x02),
                   ClockTime::new(23, 59, 58));
        assert_eq!(ClockTime::from_cmos(23, 59, 58, 0x06),
                   ClockTime::new(23, 59, 58));
        assert_eq!(ClockTime::new(1, 2, 3).unwrap().ascii(), *b"01:02:03");
    }
    #[test]
    fn twelve_hour_pm_am_and_midnight() {
        assert_eq!(ClockTime::from_cmos(0x92, 0x01, 0x03, 0x00),
                   ClockTime::new(12, 1, 3));
        assert_eq!(ClockTime::from_cmos(0x12, 0x00, 0x00, 0x00),
                   ClockTime::new(0, 0, 0));
        assert_eq!(ClockTime::from_cmos(0x81, 0x00, 0x00, 0x00),
                   ClockTime::new(13, 0, 0));
    }
    #[test]
    fn invalid_values_are_rejected() {
        for (h, m, s, status) in [(24, 0, 0, 0x06), (0, 60, 0, 0x06),
                                  (0, 0, 60, 0x06), (0x3a, 0, 0, 0x02),
                                  (0x12, 0x6a, 0, 0x00), (0, 0, 0, 0)] {
            assert!(ClockTime::from_cmos(h,m,s,status).is_none());
        }
    }
}
