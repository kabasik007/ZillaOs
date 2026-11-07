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
        if hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        Some(Self {
            hour,
            minute,
            second,
        })
    }

    pub fn from_cmos(hour_raw: u8, minute_raw: u8, second_raw: u8, status_b: u8) -> Option<Self> {
        let binary = status_b & 0x04 != 0;
        let is_24h = status_b & 0x02 != 0;
        let pm = hour_raw & 0x80 != 0;
        let hour_val = hour_raw & 0x7f;
        let mut hour = if binary {
            hour_val
        } else {
            from_bcd(hour_val)?
        };
        let minute = if binary {
            minute_raw
        } else {
            from_bcd(minute_raw)?
        };
        let second = if binary {
            second_raw
        } else {
            from_bcd(second_raw)?
        };
        if !is_24h {
            if !(1..=12).contains(&hour) {
                return None;
            }
            hour = (hour % 12) + if pm { 12 } else { 0 };
        }
        Self::new(hour, minute, second)
    }

    pub const fn ascii(self) -> [u8; 8] {
        [
            b'0' + self.hour / 10,
            b'0' + self.hour % 10,
            b':',
            b'0' + self.minute / 10,
            b'0' + self.minute % 10,
            b':',
            b'0' + self.second / 10,
            b'0' + self.second % 10,
        ]
    }
}

/// Validated calendar date read from the CMOS RTC. It is firmware time,
/// not timezone-adjusted wall clock, NTP or a host-provided date.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl ClockDate {
    pub const fn new(year: u16, month: u8, day: u8) -> Option<Self> {
        if year < 1970 || year > 2199 || month < 1 || month > 12 {
            return None;
        }
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let max_day = match month {
            2 => {
                if leap {
                    29
                } else {
                    28
                }
            }
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
        if day == 0 || day > max_day {
            return None;
        }
        Some(Self { year, month, day })
    }

    pub fn from_cmos(
        day_raw: u8,
        month_raw: u8,
        year_raw: u8,
        century_raw: u8,
        status_b: u8,
    ) -> Option<Self> {
        let binary = status_b & 0x04 != 0;
        let decode = |v| if binary { Some(v) } else { from_bcd(v) };
        let day = decode(day_raw)?;
        let month = decode(month_raw)?;
        let year = decode(year_raw)? as u16;
        // No century register means no trustworthy Gregorian year.
        // Never silently guess 20xx from a 2-digit firmware value.
        let century = decode(century_raw)? as u16;
        Self::new(century * 100 + year, month, day)
    }

    pub const fn ascii(self) -> [u8; 10] {
        let y = self.year;
        [
            b'0' + (y / 1000) as u8,
            b'0' + ((y / 100) % 10) as u8,
            b'0' + ((y / 10) % 10) as u8,
            b'0' + (y % 10) as u8,
            b'-',
            b'0' + self.month / 10,
            b'0' + self.month % 10,
            b'-',
            b'0' + self.day / 10,
            b'0' + self.day % 10,
        ]
    }
}

fn from_bcd(value: u8) -> Option<u8> {
    if value & 0x0f > 9 || value >> 4 > 9 {
        return None;
    }
    Some((value >> 4) * 10 + (value & 0x0f))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bcd_and_binary_24h_clocks() {
        assert_eq!(
            ClockTime::from_cmos(0x23, 0x59, 0x58, 0x02),
            ClockTime::new(23, 59, 58)
        );
        assert_eq!(
            ClockTime::from_cmos(23, 59, 58, 0x06),
            ClockTime::new(23, 59, 58)
        );
        assert_eq!(ClockTime::new(1, 2, 3).unwrap().ascii(), *b"01:02:03");
    }
    #[test]
    fn twelve_hour_pm_am_and_midnight() {
        assert_eq!(
            ClockTime::from_cmos(0x92, 0x01, 0x03, 0x00),
            ClockTime::new(12, 1, 3)
        );
        assert_eq!(
            ClockTime::from_cmos(0x12, 0x00, 0x00, 0x00),
            ClockTime::new(0, 0, 0)
        );
        assert_eq!(
            ClockTime::from_cmos(0x81, 0x00, 0x00, 0x00),
            ClockTime::new(13, 0, 0)
        );
    }
    #[test]
    fn calendar_date_validation_bcd_binary_and_leap_years() {
        let date = ClockDate::new(2026, 11, 6).unwrap();
        assert_eq!(date.ascii(), *b"2026-11-06");
        assert_eq!(ClockDate::from_cmos(0x06, 0x11, 0x26, 0x20, 0), Some(date));
        assert_eq!(ClockDate::from_cmos(6, 11, 26, 20, 0x04), Some(date));
        assert_eq!(ClockDate::new(2024, 2, 29).is_some(), true);
        assert_eq!(ClockDate::new(2026, 2, 29), None);
        assert_eq!(ClockDate::new(2100, 2, 29), None);
        assert_eq!(ClockDate::new(2000, 2, 29).is_some(), true);
        assert_eq!(ClockDate::from_cmos(0x06, 0x11, 0x26, 0, 0), None);
        assert_eq!(ClockDate::new(2026, 13, 1), None);
    }

    #[test]
    fn invalid_values_are_rejected() {
        for (h, m, s, status) in [
            (24, 0, 0, 0x06),
            (0, 60, 0, 0x06),
            (0, 0, 60, 0x06),
            (0x3a, 0, 0, 0x02),
            (0x12, 0x6a, 0, 0x00),
            (0, 0, 0, 0),
        ] {
            assert!(ClockTime::from_cmos(h, m, s, status).is_none());
        }
    }
}
