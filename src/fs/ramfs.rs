//! Volatile, bounded text-file overlay for the built-in ZillaOS Documents.
//! Never writes to block devices, to the host, or to immutable boot assets.
pub const DOCUMENTS_DIR: usize = 11;
pub const FILE_LIMIT: usize = 8;
pub const NAME_LIMIT: usize = 24;
pub const TEXT_LIMIT: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageError {
    InvalidName,
    TooLarge,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryFile {
    pub used: bool,
    name: [u8; NAME_LIMIT],
    name_len: usize,
    data: [u8; TEXT_LIMIT],
    len: usize,
}
impl MemoryFile {
    pub const fn empty() -> Self {
        Self {
            used: false,
            name: [0; NAME_LIMIT],
            name_len: 0,
            data: [0; TEXT_LIMIT],
            len: 0,
        }
    }
    pub fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }
    pub fn content(&self) -> &[u8] {
        &self.data[..self.len]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RamFs {
    files: [MemoryFile; FILE_LIMIT],
    pub revision: u32,
}
impl RamFs {
    pub const fn new() -> Self {
        Self {
            files: [MemoryFile::empty(); FILE_LIMIT],
            revision: 0,
        }
    }
    pub fn len(&self) -> usize {
        self.files.iter().filter(|file| file.used).count()
    }
    pub fn nth(&self, row: usize) -> Option<(usize, &MemoryFile)> {
        self.files
            .iter()
            .enumerate()
            .filter(|(_, file)| file.used)
            .nth(row)
    }
    pub fn get(&self, index: usize) -> Option<&MemoryFile> {
        self.files.get(index).filter(|file| file.used)
    }
    pub fn find(&self, name: &str) -> Option<usize> {
        self.files.iter().position(|f| f.used && f.name() == name)
    }
    /// Atomic bounded write. Failed requests do not modify existing content.
    pub fn save(&mut self, name: &str, data: &[u8]) -> Result<usize, StorageError> {
        if !valid_filename(name) {
            return Err(StorageError::InvalidName);
        }
        if data.len() > TEXT_LIMIT {
            return Err(StorageError::TooLarge);
        }
        let slot = self
            .find(name)
            .or_else(|| self.files.iter().position(|f| !f.used))
            .ok_or(StorageError::Full)?;
        let file = &mut self.files[slot];
        let mut next = MemoryFile::empty();
        next.used = true;
        next.name_len = name.len();
        next.name[..name.len()].copy_from_slice(name.as_bytes());
        next.len = data.len();
        next.data[..data.len()].copy_from_slice(data);
        *file = next;
        self.revision = self.revision.wrapping_add(1);
        Ok(slot)
    }
}
impl Default for RamFs {
    fn default() -> Self {
        Self::new()
    }
}

pub fn valid_filename(name: &str) -> bool {
    if name.len() < 5 || name.len() > NAME_LIMIT || !name.ends_with(".TXT") {
        return false;
    }
    let prefix = &name.as_bytes()[..name.len() - 4];
    !prefix.is_empty()
        && prefix
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_' || *b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn documents_only_volatile_files_support_overwrite_and_listing() {
        let mut fs = RamFs::new();
        assert_eq!(fs.len(), 0);
        let id = fs.save("NOTE01.TXT", b"hello").unwrap();
        assert_eq!(fs.get(id).unwrap().content(), b"hello");
        assert_eq!(fs.nth(0).unwrap().1.name(), "NOTE01.TXT");
        assert_eq!(fs.save("NOTE01.TXT", b"hello again").unwrap(), id);
        assert_eq!(fs.len(), 1);
        assert_eq!(fs.get(id).unwrap().content(), b"hello again");
        assert_eq!(fs.revision, 2);
        assert_eq!(RamFs::new().len(), 0); // all files vanish on reboot
    }
    #[test]
    fn invalid_names_and_oversized_writes_are_non_mutating() {
        let mut fs = RamFs::new();
        let id = fs.save("GOOD.TXT", b"good").unwrap();
        let initial = fs;
        for name in [
            "../BAD.TXT",
            "/BAD.TXT",
            "C:\\BAD.TXT",
            "bad.txt",
            "BAD.EXE",
            "..TXT",
            "A B.TXT",
            "a.TXT",
        ] {
            assert_eq!(fs.save(name, b"evil"), Err(StorageError::InvalidName));
        }
        assert_eq!(
            fs.save("GOOD.TXT", &[b'x'; TEXT_LIMIT + 1]),
            Err(StorageError::TooLarge)
        );
        assert_eq!(fs, initial);
        assert_eq!(fs.get(id).unwrap().content(), b"good");
    }
    #[test]
    fn full_storage_refuses_ninth_file_without_corrupting_existing_data() {
        let mut fs = RamFs::new();
        for i in 0..FILE_LIMIT {
            let name = match i {
                0 => "A.TXT",
                1 => "B.TXT",
                2 => "C.TXT",
                3 => "D.TXT",
                4 => "E.TXT",
                5 => "F.TXT",
                6 => "G.TXT",
                _ => "H.TXT",
            };
            fs.save(name, b"data").unwrap();
        }
        let initial = fs;
        assert_eq!(fs.save("NINTH.TXT", b"no"), Err(StorageError::Full));
        assert_eq!(fs, initial);
    }
}
