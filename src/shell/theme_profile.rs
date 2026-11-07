//! Versioned, UTF-8/ASCII appearance profile for Documents RAMFS.
//! This codec performs no I/O. THEME.TXT is volatile until a real persistent
//! storage backend exists; the on-disk file format is only prepared here.

use super::personalization::{Wallpaper, Accent};

pub const PROFILE_FILE: &str = "THEME.TXT";
pub const ENCODED_CAPACITY: usize = 80;
const HEADER: &str = "ZILLAOS-THEME/1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeProfile {
    pub wallpaper: Wallpaper,
    pub accent: Accent,
    pub dark: bool,
}

impl Default for ThemeProfile {
    fn default() -> Self {
        Self {
            wallpaper: Wallpaper::Ocean,
            accent: Accent::Teal,
            dark: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    InvalidUtf8,
    WrongVersion,
    InvalidContents,
}

impl ThemeProfile {
    /// Serialize into a bounded fixed array, without heap allocation.
    /// Returned length is the only initialized prefix the caller should save.
    pub fn encode(self) -> ([u8; ENCODED_CAPACITY], usize) {
        let wallpaper = match self.wallpaper {
            Wallpaper::Ocean => "ocean",
            Wallpaper::Sunset => "sunset",
            Wallpaper::Forest => "forest",
        };
        let accent = match self.accent {
            Accent::Teal => "teal",
            Accent::Violet => "violet",
            Accent::Amber => "amber",
        };
        let theme = if self.dark { "dark" } else { "light" };
        let mut data = [0u8; ENCODED_CAPACITY];
        let mut cursor = 0;
        for piece in [
            HEADER, "\nwallpaper=", wallpaper, "\naccent=", accent,
            "\ntheme=", theme, "\n",
        ] {
            let end = cursor + piece.len();
            data[cursor..end].copy_from_slice(piece.as_bytes());
            cursor = end;
        }
        (data, cursor)
    }

    /// Strict, atomic parse: wrong version, unknown options, extra lines
    /// and malformed UTF-8 are errors. Never partially apply a profile.
    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        let source = core::str::from_utf8(data).map_err(|_| DecodeError::InvalidUtf8)?;
        let mut lines = source.lines();
        if lines.next() != Some(HEADER) {
            return Err(DecodeError::WrongVersion);
        }
        let wallpaper = match lines.next() {
            Some("wallpaper=ocean") => Wallpaper::Ocean,
            Some("wallpaper=sunset") => Wallpaper::Sunset,
            Some("wallpaper=forest") => Wallpaper::Forest,
            _ => return Err(DecodeError::InvalidContents),
        };
        let accent = match lines.next() {
            Some("accent=teal") => Accent::Teal,
            Some("accent=violet") => Accent::Violet,
            Some("accent=amber") => Accent::Amber,
            _ => return Err(DecodeError::InvalidContents),
        };
        let dark = match lines.next() {
            Some("theme=dark") => true,
            Some("theme=light") => false,
            _ => return Err(DecodeError::InvalidContents),
        };
        if lines.next().is_some() {
            return Err(DecodeError::InvalidContents);
        }
        Ok(Self { wallpaper, accent, dark })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileStatus {
    SessionOnly,
    Modified,
    Saved,
    Loaded,
    NotFound,
    InvalidFile,
    StorageFull,
}

impl ProfileStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::SessionOnly => "RAM SESSION ONLY",
            Self::Modified => "UNSAVED APPEARANCE",
            Self::Saved => "SAVED TO DOCUMENTS RAMFS",
            Self::Loaded => "LOADED FROM DOCUMENTS RAMFS",
            Self::NotFound => "NO THEME.TXT IN DOCUMENTS",
            Self::InvalidFile => "INVALID THEME.TXT FORMAT",
            Self::StorageFull => "RAMFS FULL - NOT SAVED",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_eighteen_profiles_round_trip() {
        for wallpaper in [Wallpaper::Ocean, Wallpaper::Sunset, Wallpaper::Forest] {
            for accent in [Accent::Teal, Accent::Violet, Accent::Amber] {
                for dark in [true, false] {
                    let theme = ThemeProfile { wallpaper, accent, dark };
                    let (data, len) = theme.encode();
                    assert!(len <= ENCODED_CAPACITY);
                    assert_eq!(ThemeProfile::decode(&data[..len]), Ok(theme));
                    assert!(data[len..].iter().all(|b| *b == 0));
                }
            }
        }
    }

    #[test]
    fn invalid_files_are_rejected_without_partial_defaulting() {
        let (raw, length) = ThemeProfile::default().encode();
        assert_eq!(ThemeProfile::decode(&raw[..length]), Ok(ThemeProfile::default()));
        for text in [
            "ZILLAOS-THEME/2\nwallpaper=ocean\naccent=teal\ntheme=dark\n",
            "ZILLAOS-THEME/1\nwallpaper=gif\naccent=teal\ntheme=dark\n",
            "ZILLAOS-THEME/1\nwallpaper=ocean\naccent=unsafe\ntheme=dark\n",
            "ZILLAOS-THEME/1\nwallpaper=ocean\naccent=teal\ntheme=unknown\n",
            "ZILLAOS-THEME/1\nwallpaper=ocean\naccent=teal\ntheme=dark\nextra=1\n",
            "ZILLAOS-THEME/1\nwallpaper=ocean\naccent=teal\n",
        ] {
            assert!(ThemeProfile::decode(text.as_bytes()).is_err());
        }
        assert_eq!(ThemeProfile::decode(&[0xff, 0xfe]), Err(DecodeError::InvalidUtf8));
    }

    #[test]
    fn ramfs_round_trip_is_only_volatile() {
        let mut fs = crate::fs::ramfs::RamFs::new();
        let expected = ThemeProfile {
            wallpaper: Wallpaper::Forest,
            accent: Accent::Amber,
            dark: false,
        };
        let (data, len) = expected.encode();
        fs.save(PROFILE_FILE, &data[..len]).unwrap();
        let file = fs.find(PROFILE_FILE).and_then(|i| fs.get(i)).unwrap();
        assert_eq!(ThemeProfile::decode(file.content()), Ok(expected));
        assert_eq!(crate::fs::ramfs::RamFs::new().find(PROFILE_FILE), None);
    }
}
