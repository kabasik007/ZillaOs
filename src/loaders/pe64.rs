//! Checked, allocation-free PE32+ image layout into a caller-owned buffer.
//!
//! No page tables are changed. No instructions are executed. Imported
//! DLLs are rejected until an isolated resolver/runtime exists. All section
//! permissions below are metadata only, not hardware-enforced PTEs.
use crate::compat::windows_pe::{PeError, inspect_pe64};

const MAX_SECTIONS: usize = 96;
const MAX_IMAGE: usize = 16 * 1024 * 1024;
const PAGE: u32 = 4096;
const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
const IMAGE_SCN_MEM_READ: u32 = 0x4000_0000;
const IMAGE_SCN_MEM_WRITE: u32 = 0x8000_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    Pe(PeError),
    ImageTooLarge,
    OutputTooSmall,
    InvalidHeaders,
    UnsupportedAlignment,
    UnsupportedSectionPermissions,
    OverlappingSections,
    InvalidEntryPoint,
    ImportsUnsupported,
    MissingRelocations,
    InvalidRelocation,
    UnsupportedRelocation,
    RelocationOverflow,
}

impl From<PeError> for LoadError {
    fn from(error: PeError) -> Self {
        Self::Pe(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Section {
    pub rva: u32,
    pub mapped_size: u32,
    pub initialized_size: u32,
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
}

impl Section {
    const EMPTY: Self = Self {
        rva: 0,
        mapped_size: 0,
        initialized_size: 0,
        readable: false,
        writable: false,
        executable: false,
    };
    fn end(self) -> u32 {
        self.rva + self.mapped_size
    }
    fn contains(self, rva: u32, length: u32) -> bool {
        rva >= self.rva && rva.checked_add(length).is_some_and(|end| end <= self.end())
    }
}

#[derive(Clone, Debug)]
pub struct MappedImage {
    pub preferred_base: u64,
    pub loaded_base: u64,
    pub image_size: u32,
    pub entry_va: u64,
    pub header_size: u32,
    pub section_count: usize,
    sections: [Section; MAX_SECTIONS],
}

impl MappedImage {
    pub fn sections(&self) -> &[Section] {
        &self.sections[..self.section_count]
    }

    pub fn section_at(&self, rva: u32, len: u32) -> Option<Section> {
        self.sections()
            .iter()
            .copied()
            .find(|s| s.contains(rva, len))
    }
}

fn u16_at(b: &[u8], offset: usize) -> Option<u16> {
    let x = b.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([x[0], x[1]]))
}
fn u32_at(b: &[u8], offset: usize) -> Option<u32> {
    let x = b.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}
fn u64_at(b: &[u8], offset: usize) -> Option<u64> {
    let x = b.get(offset..offset.checked_add(8)?)?;
    Some(u64::from_le_bytes([
        x[0], x[1], x[2], x[3], x[4], x[5], x[6], x[7],
    ]))
}
fn round_page(value: u32) -> Option<u32> {
    value.checked_add(PAGE - 1).map(|n| n & !(PAGE - 1))
}
fn bounds(offset: u32, length: u32, limit: u32) -> bool {
    offset.checked_add(length).is_some_and(|end| end <= limit)
}

fn relocate(
    image: &mut [u8],
    sections: &[Section],
    directory_rva: u32,
    directory_size: u32,
    delta: i128,
) -> Result<(), LoadError> {
    if directory_size == 0
        || directory_size > MAX_IMAGE as u32
        || !bounds(directory_rva, directory_size, image.len() as u32)
        || !sections
            .iter()
            .any(|s| s.contains(directory_rva, directory_size))
    {
        return Err(LoadError::InvalidRelocation);
    }
    let start = directory_rva as usize;
    let end = start + directory_size as usize;
    let mut cursor = start;
    while cursor < end {
        let page = u32_at(image, cursor).ok_or(LoadError::InvalidRelocation)?;
        let size = u32_at(image, cursor + 4).ok_or(LoadError::InvalidRelocation)? as usize;
        if page % PAGE != 0
            || size < 8
            || size % 2 != 0
            || cursor.checked_add(size).is_none_or(|next| next > end)
        {
            return Err(LoadError::InvalidRelocation);
        }
        let mut p = cursor + 8;
        while p < cursor + size {
            let descriptor = u16_at(image, p).ok_or(LoadError::InvalidRelocation)?;
            p += 2;
            let kind = descriptor >> 12;
            if kind == 0 {
                continue;
            } // IMAGE_REL_BASED_ABSOLUTE
            if kind != 10 {
                return Err(LoadError::UnsupportedRelocation);
            } // DIR64
            let target = page
                .checked_add((descriptor & 0xfff) as u32)
                .ok_or(LoadError::InvalidRelocation)?;
            if target % 8 != 0
                || !bounds(target, 8, image.len() as u32)
                || !sections.iter().any(|s| s.contains(target, 8))
            {
                return Err(LoadError::InvalidRelocation);
            }
            let original = u64_at(image, target as usize).ok_or(LoadError::InvalidRelocation)?;
            let moved = (original as i128)
                .checked_add(delta)
                .ok_or(LoadError::RelocationOverflow)?;
            if !(0..=u64::MAX as i128).contains(&moved) {
                return Err(LoadError::RelocationOverflow);
            }
            image[target as usize..target as usize + 8]
                .copy_from_slice(&(moved as u64).to_le_bytes());
        }
        cursor += size;
    }
    Ok(())
}

/// Map a checked x64 PE into an independent zero-initialized byte buffer.
///
/// The caller must treat the returned memory as DATA until a separate, tested
/// user process mapper enforces the section permissions. No executable bytes
/// are mapped into Ring 0 by this function. On error, output may be modified;
/// discard it rather than attempting to run the partial image.
pub fn map_into(file: &[u8], image: &mut [u8], load_base: u64) -> Result<MappedImage, LoadError> {
    let pe = inspect_pe64(file)?;
    let image_size = pe.image_size as usize;
    if image_size > MAX_IMAGE {
        return Err(LoadError::ImageTooLarge);
    }
    if image.len() < image_size {
        return Err(LoadError::OutputTooSmall);
    }
    if pe.has_imports {
        return Err(LoadError::ImportsUnsupported);
    }
    if load_base & 0xffff != 0 || load_base.checked_add(pe.image_size as u64).is_none() {
        return Err(LoadError::InvalidHeaders);
    }
    let pe_offset = u32_at(file, 0x3c).ok_or(LoadError::InvalidHeaders)? as usize;
    let optional_offset = pe_offset.checked_add(24).ok_or(LoadError::InvalidHeaders)?;
    let optional_size = u16_at(file, pe_offset + 20).ok_or(LoadError::InvalidHeaders)? as usize;
    let section_headers = optional_offset
        .checked_add(optional_size)
        .ok_or(LoadError::InvalidHeaders)?;
    let alignment = u32_at(file, optional_offset + 32).ok_or(LoadError::InvalidHeaders)?;
    let file_alignment = u32_at(file, optional_offset + 36).ok_or(LoadError::InvalidHeaders)?;
    if alignment != PAGE || file_alignment < 512 || file_alignment > PAGE {
        return Err(LoadError::UnsupportedAlignment);
    }
    if pe.header_size as usize > file.len() || pe.header_size % file_alignment != 0 {
        return Err(LoadError::InvalidHeaders);
    }
    let headers_page_end = round_page(pe.header_size).ok_or(LoadError::InvalidHeaders)?;
    let mut sections = [Section::EMPTY; MAX_SECTIONS];
    let mut raw_ranges = [(0u32, 0u32); MAX_SECTIONS];
    for index in 0..pe.sections as usize {
        let h = section_headers
            .checked_add(index * 40)
            .ok_or(LoadError::InvalidHeaders)?;
        let virtual_size = u32_at(file, h + 8).ok_or(LoadError::InvalidHeaders)?;
        let rva = u32_at(file, h + 12).ok_or(LoadError::InvalidHeaders)?;
        let raw_size = u32_at(file, h + 16).ok_or(LoadError::InvalidHeaders)?;
        let raw = u32_at(file, h + 20).ok_or(LoadError::InvalidHeaders)?;
        let flags = u32_at(file, h + 36).ok_or(LoadError::InvalidHeaders)?;
        let readable = flags & IMAGE_SCN_MEM_READ != 0;
        let writable = flags & IMAGE_SCN_MEM_WRITE != 0;
        let executable = flags & IMAGE_SCN_MEM_EXECUTE != 0;
        if !readable || (writable && executable) {
            return Err(LoadError::UnsupportedSectionPermissions);
        }
        if rva % PAGE != 0 || rva < headers_page_end {
            return Err(LoadError::UnsupportedAlignment);
        }
        if raw_size > 0 && (raw % file_alignment != 0 || raw_size % file_alignment != 0) {
            return Err(LoadError::UnsupportedAlignment);
        }
        let span = virtual_size.max(raw_size);
        let mapped_size = round_page(span).ok_or(LoadError::InvalidHeaders)?;
        if mapped_size == 0 || !bounds(rva, mapped_size, pe.image_size) {
            return Err(LoadError::InvalidHeaders);
        }
        for old in &sections[..index] {
            if rva < old.end() && old.rva < rva + mapped_size {
                return Err(LoadError::OverlappingSections);
            }
        }
        sections[index] = Section {
            rva,
            mapped_size,
            initialized_size: raw_size,
            readable,
            writable,
            executable,
        };
        raw_ranges[index] = (raw, raw_size);
    }
    let actual = &sections[..pe.sections as usize];
    if !actual.iter().any(|s| {
        s.executable && s.contains(pe.entry_rva, 1) && pe.entry_rva - s.rva < s.initialized_size
    }) {
        return Err(LoadError::InvalidEntryPoint);
    }

    let delta = (load_base as i128) - (pe.image_base as i128);
    if delta != 0 && !pe.has_relocations {
        return Err(LoadError::MissingRelocations);
    }

    let destination = &mut image[..image_size];
    destination.fill(0);
    destination[..pe.header_size as usize].copy_from_slice(&file[..pe.header_size as usize]);
    for (section, (raw, raw_size)) in actual.iter().zip(raw_ranges) {
        if raw_size != 0 {
            let start = section.rva as usize;
            let source_start = raw as usize;
            destination[start..start + raw_size as usize]
                .copy_from_slice(&file[source_start..source_start + raw_size as usize]);
        }
    }
    if delta != 0 {
        let directory_rva =
            u32_at(file, optional_offset + 112 + 5 * 8).ok_or(LoadError::InvalidRelocation)?;
        let directory_size =
            u32_at(file, optional_offset + 116 + 5 * 8).ok_or(LoadError::InvalidRelocation)?;
        relocate(destination, actual, directory_rva, directory_size, delta)?;
    }
    let entry_va = load_base
        .checked_add(pe.entry_rva as u64)
        .ok_or(LoadError::InvalidEntryPoint)?;
    Ok(MappedImage {
        preferred_base: pe.image_base,
        loaded_base: load_base,
        image_size: pe.image_size,
        entry_va,
        header_size: pe.header_size,
        section_count: pe.sections as usize,
        sections,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> [u8; 1536] {
        let mut b = [0u8; 1536];
        b[..2].copy_from_slice(b"MZ");
        b[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        b[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        b[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
        b[0x94..0x96].copy_from_slice(&0xf0u16.to_le_bytes());
        let opt = 0x98;
        b[opt..opt + 2].copy_from_slice(&0x20bu16.to_le_bytes());
        b[opt + 16..opt + 20].copy_from_slice(&0x1000u32.to_le_bytes());
        b[opt + 24..opt + 32].copy_from_slice(&0x140000000u64.to_le_bytes());
        b[opt + 32..opt + 36].copy_from_slice(&0x1000u32.to_le_bytes());
        b[opt + 36..opt + 40].copy_from_slice(&0x200u32.to_le_bytes());
        b[opt + 56..opt + 60].copy_from_slice(&0x2000u32.to_le_bytes());
        b[opt + 60..opt + 64].copy_from_slice(&0x200u32.to_le_bytes());
        b[opt + 68..opt + 70].copy_from_slice(&3u16.to_le_bytes());
        b[opt + 108..opt + 112].copy_from_slice(&16u32.to_le_bytes());
        let sh = 0x188;
        b[sh..sh + 5].copy_from_slice(b".text");
        b[sh + 8..sh + 12].copy_from_slice(&0x300u32.to_le_bytes());
        b[sh + 12..sh + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        b[sh + 16..sh + 20].copy_from_slice(&0x200u32.to_le_bytes());
        b[sh + 20..sh + 24].copy_from_slice(&0x200u32.to_le_bytes());
        b[sh + 36..sh + 40].copy_from_slice(&0x60000020u32.to_le_bytes());
        b[0x200..0x400].fill(0x90);
        b
    }

    fn relocate_fixture() -> [u8; 1536] {
        let mut b = fixture();
        b[0x86..0x88].copy_from_slice(&2u16.to_le_bytes());
        b[0x98 + 56..0x98 + 60].copy_from_slice(&0x3000u32.to_le_bytes());
        let sh = 0x188 + 40;
        b[sh..sh + 6].copy_from_slice(b".reloc");
        b[sh + 8..sh + 12].copy_from_slice(&0x200u32.to_le_bytes());
        b[sh + 12..sh + 16].copy_from_slice(&0x2000u32.to_le_bytes());
        b[sh + 16..sh + 20].copy_from_slice(&0x200u32.to_le_bytes());
        b[sh + 20..sh + 24].copy_from_slice(&0x400u32.to_le_bytes());
        b[sh + 36..sh + 40].copy_from_slice(&0x42000040u32.to_le_bytes());
        b[0x98 + 152..0x98 + 156].copy_from_slice(&0x2000u32.to_le_bytes());
        b[0x98 + 156..0x98 + 160].copy_from_slice(&12u32.to_le_bytes());
        b[0x400..0x404].copy_from_slice(&0x1000u32.to_le_bytes());
        b[0x404..0x408].copy_from_slice(&12u32.to_le_bytes());
        b[0x408..0x40a].copy_from_slice(&0xa008u16.to_le_bytes());
        b[0x40a..0x40c].copy_from_slice(&0u16.to_le_bytes());
        b[0x208..0x210].copy_from_slice(&0x140001234u64.to_le_bytes());
        b
    }

    #[test]
    fn maps_rx_section_copies_headers_and_zero_fills_tail() {
        let file = fixture();
        let mut image = [0xffu8; 8192];
        let plan = map_into(&file, &mut image, 0x140000000).unwrap();
        assert_eq!(plan.entry_va, 0x140001000);
        assert_eq!(plan.section_count, 1);
        assert_eq!(plan.sections()[0].mapped_size, 4096);
        assert!(plan.sections()[0].executable);
        assert!(!plan.sections()[0].writable);
        assert_eq!(image[0x1000], 0x90);
        assert_eq!(image[0x11ff], 0x90);
        assert_eq!(image[0x1200], 0);
        assert_eq!(image[0], b'M');
        assert_eq!(image[0x1fff], 0);
    }

    #[test]
    fn rejects_executable_writable_section_and_unmapped_entry() {
        let mut file = fixture();
        file[0x188 + 36..0x188 + 40].copy_from_slice(&0xe0000020u32.to_le_bytes());
        let mut dest = [0u8; 8192];
        assert_eq!(
            map_into(&file, &mut dest, 0x140000000).unwrap_err(),
            LoadError::UnsupportedSectionPermissions
        );
        file = fixture();
        file[0x98 + 16..0x98 + 20].copy_from_slice(&0x1800u32.to_le_bytes());
        assert_eq!(
            map_into(&file, &mut dest, 0x140000000).unwrap_err(),
            LoadError::InvalidEntryPoint
        );
    }

    #[test]
    fn rejects_unsafe_imports_missing_relocations_and_small_output() {
        let mut file = fixture();
        let mut dest = [0u8; 8192];
        file[0x98 + 120..0x98 + 124].copy_from_slice(&0x1100u32.to_le_bytes());
        file[0x98 + 124..0x98 + 128].copy_from_slice(&0x20u32.to_le_bytes());
        assert_eq!(
            map_into(&file, &mut dest, 0x140000000).unwrap_err(),
            LoadError::ImportsUnsupported
        );
        let file = fixture();
        assert_eq!(
            map_into(&file, &mut dest, 0x140010000).unwrap_err(),
            LoadError::MissingRelocations
        );
        assert_eq!(
            map_into(&file, &mut dest[..4096], 0x140000000).unwrap_err(),
            LoadError::OutputTooSmall
        );
    }

    #[test]
    fn supports_checked_dir64_relocation_with_changed_load_base() {
        let file = relocate_fixture();
        let mut dest = [0u8; 12288];
        let plan = map_into(&file, &mut dest, 0x140010000).unwrap();
        assert_eq!(plan.entry_va, 0x140011000);
        assert_eq!(u64_at(&dest, 0x1008), Some(0x140011234));
    }

    #[test]
    fn rejects_invalid_or_unsupported_reloc_blocks() {
        let mut file = relocate_fixture();
        let mut dest = [0u8; 12288];
        file[0x404..0x408].copy_from_slice(&7u32.to_le_bytes());
        assert_eq!(
            map_into(&file, &mut dest, 0x140010000).unwrap_err(),
            LoadError::InvalidRelocation
        );
        file = relocate_fixture();
        file[0x408..0x40a].copy_from_slice(&0x3008u16.to_le_bytes());
        assert_eq!(
            map_into(&file, &mut dest, 0x140010000).unwrap_err(),
            LoadError::UnsupportedRelocation
        );
    }

    #[test]
    fn rejects_overlapping_virtual_sections() {
        let mut file = relocate_fixture();
        // Second section uses the same page as .text and would create
        // conflicting PTE permissions.
        file[0x188 + 40 + 12..0x188 + 40 + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        let mut dest = [0u8; 12288];
        assert_eq!(
            map_into(&file, &mut dest, 0x140000000).unwrap_err(),
            LoadError::OverlappingSections
        );
    }
}
