//! Early ZillaOS Notepad text editor. ASCII/PS2 input only, no heap.
//! Documents save to the bounded volatile RamFs overlay, never to a disk.
use crate::fs::ramfs::{RamFs, StorageError, TEXT_LIMIT};

pub const EDITOR_LIMIT: usize = TEXT_LIMIT;
pub const NAME_LIMIT: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorStatus {
    Ready,
    Modified,
    Saved,
    UnsavedChanges,
    StorageError(StorageError),
    BufferFull,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Notepad {
    data: [u8; EDITOR_LIMIT],
    len: usize,
    cursor: usize,
    filename: [u8; NAME_LIMIT],
    filename_len: usize,
    pub dirty: bool,
    pub status: EditorStatus,
    next_number: u8,
    pub revision: u32,
}

impl Notepad {
    pub const fn new() -> Self {
        let mut name = [0u8; NAME_LIMIT];
        name[0] = b'N';
        name[1] = b'O';
        name[2] = b'T';
        name[3] = b'E';
        name[4] = b'0';
        name[5] = b'1';
        name[6] = b'.';
        name[7] = b'T';
        name[8] = b'X';
        name[9] = b'T';
        Self {
            data: [0; EDITOR_LIMIT],
            len: 0,
            cursor: 0,
            filename: name,
            filename_len: 10,
            dirty: false,
            status: EditorStatus::Ready,
            next_number: 2,
            revision: 0,
        }
    }

    pub fn filename(&self) -> &str {
        core::str::from_utf8(&self.filename[..self.filename_len]).unwrap_or("")
    }
    pub fn bytes(&self) -> &[u8] {
        &self.data[..self.len]
    }
    pub fn content(&self) -> &str {
        core::str::from_utf8(self.bytes()).unwrap_or("")
    }
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn insert(&mut self, ch: u8) -> bool {
        if ch != b'\n' && !(32..=126).contains(&ch) {
            return false;
        }
        if self.len >= EDITOR_LIMIT {
            self.status = EditorStatus::BufferFull;
            self.revision = self.revision.wrapping_add(1);
            return false;
        }
        self.data
            .copy_within(self.cursor..self.len, self.cursor + 1);
        self.data[self.cursor] = ch;
        self.cursor += 1;
        self.len += 1;
        self.dirty = true;
        self.status = EditorStatus::Modified;
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.data
            .copy_within(self.cursor..self.len, self.cursor - 1);
        self.cursor -= 1;
        self.len -= 1;
        self.data[self.len] = 0;
        self.dirty = true;
        self.status = EditorStatus::Modified;
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub fn move_left(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        self.revision = self.revision.wrapping_add(1);
        true
    }
    pub fn move_right(&mut self) -> bool {
        if self.cursor >= self.len {
            return false;
        }
        self.cursor += 1;
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub fn save(&mut self, fs: &mut RamFs) -> Result<(), StorageError> {
        let result = fs.save(self.filename(), self.bytes());
        match result {
            Ok(_) => {
                self.dirty = false;
                self.status = EditorStatus::Saved;
                self.revision = self.revision.wrapping_add(1);
                Ok(())
            }
            Err(error) => {
                self.status = EditorStatus::StorageError(error);
                self.revision = self.revision.wrapping_add(1);
                Err(error)
            }
        }
    }

    pub fn new_document(&mut self) -> bool {
        if self.dirty {
            self.status = EditorStatus::UnsavedChanges;
            self.revision = self.revision.wrapping_add(1);
            return false;
        }
        let n = self.next_number;
        if n > 99 {
            return false;
        }
        self.next_number = n + 1;
        self.data = [0; EDITOR_LIMIT];
        self.len = 0;
        self.cursor = 0;
        self.set_filename_number(n);
        self.dirty = false;
        self.status = EditorStatus::Ready;
        self.revision = self.revision.wrapping_add(1);
        true
    }

    fn set_filename_number(&mut self, n: u8) {
        self.filename = [0; NAME_LIMIT];
        self.filename[..4].copy_from_slice(b"NOTE");
        self.filename[4] = b'0' + n / 10;
        self.filename[5] = b'0' + n % 10;
        self.filename[6..10].copy_from_slice(b".TXT");
        self.filename_len = 10;
    }

    pub fn open(&mut self, name: &str, content: &[u8]) -> bool {
        if self.dirty {
            self.status = EditorStatus::UnsavedChanges;
            self.revision = self.revision.wrapping_add(1);
            return false;
        }
        if !crate::fs::ramfs::valid_filename(name)
            || content.len() > EDITOR_LIMIT
            || !content
                .iter()
                .all(|b| *b == b'\n' || (32..=126).contains(b))
        {
            return false;
        }
        self.data = [0; EDITOR_LIMIT];
        self.data[..content.len()].copy_from_slice(content);
        self.len = content.len();
        self.cursor = self.len;
        self.filename = [0; NAME_LIMIT];
        self.filename[..name.len()].copy_from_slice(name.as_bytes());
        self.filename_len = name.len();
        self.dirty = false;
        self.status = EditorStatus::Ready;
        self.revision = self.revision.wrapping_add(1);
        true
    }

    /// Screen line wrapping helpers used by the renderer, never by storage.
    pub fn visual_line_count(&self, columns: usize) -> usize {
        let width = columns.max(1);
        let mut lines = 1;
        let mut col = 0;
        for &byte in self.bytes() {
            if byte == b'\n' {
                lines += 1;
                col = 0;
            } else {
                if col >= width {
                    lines += 1;
                    col = 0;
                }
                col += 1;
            }
        }
        lines
    }
    pub fn cursor_visual(&self, columns: usize) -> (usize, usize) {
        let width = columns.max(1);
        let mut row = 0;
        let mut col = 0;
        for &byte in &self.data[..self.cursor] {
            if byte == b'\n' {
                row += 1;
                col = 0;
            } else {
                if col >= width {
                    row += 1;
                    col = 0;
                }
                col += 1;
            }
        }
        (row, col)
    }
}
impl Default for Notepad {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_with_movable_cursor_and_saves_to_volatile_documents() {
        let mut editor = Notepad::new();
        let mut fs = RamFs::new();
        for byte in b"Helo" {
            assert!(editor.insert(*byte));
        }
        assert!(editor.move_left());
        assert!(editor.insert(b'l'));
        assert_eq!(editor.content(), "Hello");
        assert!(editor.save(&mut fs).is_ok());
        assert_eq!(editor.filename(), "NOTE01.TXT");
        assert!(!editor.dirty);
        let id = fs.find("NOTE01.TXT").unwrap();
        assert_eq!(fs.get(id).unwrap().content(), b"Hello");
        assert!(editor.new_document());
        assert_eq!(editor.filename(), "NOTE02.TXT");
        assert!(editor.open("NOTE01.TXT", fs.get(id).unwrap().content()));
        assert_eq!(editor.content(), "Hello");
    }
    #[test]
    fn rejects_unsaved_document_loss_and_invalid_binary_input() {
        let mut editor = Notepad::new();
        editor.insert(b'a');
        assert!(!editor.new_document());
        assert_eq!(editor.status, EditorStatus::UnsavedChanges);
        assert!(!editor.open("OTHER.TXT", b"replace"));
        assert_eq!(editor.content(), "a");
        assert!(!editor.insert(0x80));
        assert!(editor.backspace());
        assert!(!editor.backspace());
        editor.save(&mut RamFs::new()).unwrap();
        assert!(editor.new_document());
        assert!(!editor.open("../BAD.TXT", b"bad"));
        assert!(!editor.open("GOOD.TXT", &[0; 3]));
    }
    #[test]
    fn bounded_text_cannot_overflow_or_clobber_ramfs_data() {
        let mut editor = Notepad::new();
        for _ in 0..EDITOR_LIMIT {
            assert!(editor.insert(b'X'));
        }
        assert!(!editor.insert(b'Y'));
        assert_eq!(editor.status, EditorStatus::BufferFull);
        assert_eq!(editor.content().len(), EDITOR_LIMIT);
        let mut fs = RamFs::new();
        editor.save(&mut fs).unwrap();
        assert_eq!(fs.nth(0).unwrap().1.content().len(), EDITOR_LIMIT);
    }
    #[test]
    fn text_cursor_wraps_without_dropping_newlines() {
        let mut editor = Notepad::new();
        for b in b"ABCD\nEF" {
            editor.insert(*b);
        }
        assert_eq!(editor.visual_line_count(3), 3);
        assert_eq!(editor.cursor_visual(3), (2, 2));
    }
}
