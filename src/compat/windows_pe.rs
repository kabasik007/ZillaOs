//! Strict, allocation-free PE32+ x86-64 image inspector.
//!
//! PE validation is NOT Windows compatibility and must never execute bytes
//! from a file in ring 0. This parser is preparation for a sandboxed loader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeError {
    TooShort,
    BadDosSignature,
    BadPeOffset,
    BadPeSignature,
    UnsupportedMachine,
    UnsupportedOptionalHeader,
    BadSectionCount,
    InvalidAlignment,
    InvalidImage,
    InvalidSections,
    OutOfBounds,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeInfo {
    pub entry_rva: u32,
    pub image_base: u64,
    pub image_size: u32,
    pub header_size: u32,
    pub sections: u16,
    pub subsystem: u16,
    pub is_dll: bool,
    pub has_imports: bool,
    pub has_relocations: bool,
}

fn u16_at(bytes: &[u8], start: usize) -> Option<u16> {
    let part = bytes.get(start..start.checked_add(2)?)?;
    Some(u16::from_le_bytes([part[0], part[1]]))
}
fn u32_at(bytes: &[u8], start: usize) -> Option<u32> {
    let part = bytes.get(start..start.checked_add(4)?)?;
    Some(u32::from_le_bytes([part[0], part[1], part[2], part[3]]))
}
fn u64_at(bytes: &[u8], start: usize) -> Option<u64> {
    let part = bytes.get(start..start.checked_add(8)?)?;
    Some(u64::from_le_bytes([part[0], part[1], part[2], part[3],
                             part[4], part[5], part[6], part[7]]))
}
fn pow2(v: u32) -> bool { v != 0 && v.is_power_of_two() }

pub fn inspect_pe64(bytes: &[u8]) -> Result<PeInfo, PeError> {
    if bytes.len() < 0x40 { return Err(PeError::TooShort); }
    if &bytes[..2] != b"MZ" { return Err(PeError::BadDosSignature); }
    let pe = u32_at(bytes, 0x3c).ok_or(PeError::BadPeOffset)? as usize;
    if pe < 0x40 || pe > bytes.len().saturating_sub(24) {
        return Err(PeError::BadPeOffset);
    }
    if bytes.get(pe..pe + 4) != Some(b"PE\0\0".as_slice()) {
        return Err(PeError::BadPeSignature);
    }
    if u16_at(bytes, pe + 4) != Some(0x8664) {
        return Err(PeError::UnsupportedMachine);
    }
    let count = u16_at(bytes, pe + 6).ok_or(PeError::OutOfBounds)?;
    if count == 0 || count > 96 { return Err(PeError::BadSectionCount); }
    let opt_size = u16_at(bytes, pe + 20).ok_or(PeError::OutOfBounds)? as usize;
    if opt_size < 112 { return Err(PeError::UnsupportedOptionalHeader); }
    let opt = pe.checked_add(24).ok_or(PeError::OutOfBounds)?;
    let end = opt.checked_add(opt_size).ok_or(PeError::OutOfBounds)?;
    if end > bytes.len() { return Err(PeError::OutOfBounds); }
    if u16_at(bytes, opt) != Some(0x020b) {
        return Err(PeError::UnsupportedOptionalHeader);
    }
    let entry_rva = u32_at(bytes, opt + 16).ok_or(PeError::OutOfBounds)?;
    let image_base = u64_at(bytes, opt + 24).ok_or(PeError::OutOfBounds)?;
    let section_alignment = u32_at(bytes, opt + 32).ok_or(PeError::OutOfBounds)?;
    let file_alignment = u32_at(bytes, opt + 36).ok_or(PeError::OutOfBounds)?;
    let image_size = u32_at(bytes, opt + 56).ok_or(PeError::OutOfBounds)?;
    let header_size = u32_at(bytes, opt + 60).ok_or(PeError::OutOfBounds)?;
    let subsystem = u16_at(bytes, opt + 68).ok_or(PeError::OutOfBounds)?;
    let flags = u16_at(bytes, pe + 22).ok_or(PeError::OutOfBounds)?;
    if !pow2(section_alignment) || !pow2(file_alignment)
        || image_base & 0xffff != 0 {
        return Err(PeError::InvalidAlignment);
    }
    if section_alignment < file_alignment && section_alignment != file_alignment {
        return Err(PeError::InvalidAlignment);
    }
    if image_size == 0 || image_size > 512 * 1024 * 1024
        || header_size == 0 || header_size > image_size
        || entry_rva >= image_size {
        return Err(PeError::InvalidImage);
    }
    let section_end = end.checked_add((count as usize) * 40)
        .ok_or(PeError::OutOfBounds)?;
    if section_end > bytes.len() || section_end > header_size as usize {
        return Err(PeError::InvalidSections);
    }
    // Section entries cannot reference virtual memory beyond SizeOfImage or
    // file bytes beyond EOF. Never trust raw offsets supplied by a PE file.
    for i in 0..count as usize {
        let sh = end + i * 40;
        let virtual_size = u32_at(bytes, sh + 8).ok_or(PeError::InvalidSections)?;
        let virtual_address = u32_at(bytes, sh + 12).ok_or(PeError::InvalidSections)?;
        let raw_size = u32_at(bytes, sh + 16).ok_or(PeError::InvalidSections)?;
        let raw_offset = u32_at(bytes, sh + 20).ok_or(PeError::InvalidSections)?;
        let span = virtual_size.max(raw_size);
        if virtual_address.checked_add(span).is_none_or(|end| end > image_size) {
            return Err(PeError::InvalidSections);
        }
        if raw_size > 0 && (raw_offset < header_size ||
            (raw_offset as usize).checked_add(raw_size as usize)
                .is_none_or(|end| end > bytes.len())) {
            return Err(PeError::InvalidSections);
        }
    }
    let directory_count = u32_at(bytes, opt + 108).ok_or(PeError::OutOfBounds)? as usize;
    let mut has_imports = false;
    let mut has_relocations = false;
    if directory_count >= 2 {
        if opt_size < 112 + 2 * 8 {
            return Err(PeError::UnsupportedOptionalHeader);
        }
        let import_rva = u32_at(bytes, opt + 120).ok_or(PeError::OutOfBounds)?;
        let import_size = u32_at(bytes, opt + 124).ok_or(PeError::OutOfBounds)?;
        if import_rva != 0 || import_size != 0 {
            if import_rva == 0 || import_size == 0 ||
                import_rva.checked_add(import_size).is_none_or(|end| end > image_size) {
                return Err(PeError::InvalidImage);
            }
            has_imports = true;
        }
    }
    if directory_count >= 6 {
        if opt_size < 112 + 6 * 8 { return Err(PeError::UnsupportedOptionalHeader); }
        let rva = u32_at(bytes, opt + 112 + 5 * 8).ok_or(PeError::OutOfBounds)?;
        let size = u32_at(bytes, opt + 116 + 5 * 8).ok_or(PeError::OutOfBounds)?;
        if rva != 0 || size != 0 {
            if rva == 0 || size == 0 ||
                rva.checked_add(size).is_none_or(|end| end > image_size) {
                return Err(PeError::InvalidImage);
            }
            has_relocations = true;
        }
    }
    Ok(PeInfo {
        entry_rva, image_base, image_size, header_size, sections: count,
        subsystem, is_dll: flags & 0x2000 != 0,
        has_imports, has_relocations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> [u8; 1024] {
        let mut b = [0u8; 1024];
        b[0..2].copy_from_slice(b"MZ");
        b[0x3c..0x40].copy_from_slice(&(0x80u32).to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        b[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        b[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
        b[0x94..0x96].copy_from_slice(&0xf0u16.to_le_bytes());
        let o = 0x98usize;
        b[o..o+2].copy_from_slice(&0x20bu16.to_le_bytes());
        b[o+16..o+20].copy_from_slice(&0x1000u32.to_le_bytes());
        b[o+24..o+32].copy_from_slice(&0x140000000u64.to_le_bytes());
        b[o+32..o+36].copy_from_slice(&0x1000u32.to_le_bytes());
        b[o+36..o+40].copy_from_slice(&0x200u32.to_le_bytes());
        b[o+56..o+60].copy_from_slice(&0x2000u32.to_le_bytes());
        b[o+60..o+64].copy_from_slice(&0x200u32.to_le_bytes());
        b[o+68..o+70].copy_from_slice(&3u16.to_le_bytes());
        b[o+108..o+112].copy_from_slice(&16u32.to_le_bytes());
        let sh = 0x188usize;
        b[sh..sh+5].copy_from_slice(b".text");
        b[sh+8..sh+12].copy_from_slice(&0x100u32.to_le_bytes());
        b[sh+12..sh+16].copy_from_slice(&0x1000u32.to_le_bytes());
        b[sh+16..sh+20].copy_from_slice(&0x200u32.to_le_bytes());
        b[sh+20..sh+24].copy_from_slice(&0x200u32.to_le_bytes());
        b
    }
    #[test]
    fn accepts_structural_pe64_without_executing() {
        let b = sample();
        let info = inspect_pe64(&b).unwrap();
        assert_eq!(info.entry_rva, 0x1000);
        assert_eq!(info.image_base, 0x140000000);
        assert_eq!(info.sections, 1);
        assert_eq!(info.subsystem, 3);
        assert!(!info.has_imports);
    }
    #[test]
    fn denies_wrong_architecture_bad_headers_or_truncated_input() {
        let b = sample();
        assert_eq!(inspect_pe64(&b[..60]), Err(PeError::TooShort));
        let mut v = b;
        v[0] = 0;
        assert_eq!(inspect_pe64(&v), Err(PeError::BadDosSignature));
        v = b; v[0x84] = 0x4c;
        assert_eq!(inspect_pe64(&v), Err(PeError::UnsupportedMachine));
        v = b; v[0x3c..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(inspect_pe64(&v), Err(PeError::BadPeOffset));
        v = b; v[0x98] = 0x0b; v[0x99] = 0x01;
        assert_eq!(inspect_pe64(&v), Err(PeError::UnsupportedOptionalHeader));
    }
    #[test]
    fn refuses_out_of_range_sections_imports_and_image_sizes() {
        let b = sample();
        let mut v = b;
        v[0x188+20..0x188+24].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(inspect_pe64(&v), Err(PeError::InvalidSections));
        v = b; v[0x98+120..0x98+124].copy_from_slice(&0x1ff0u32.to_le_bytes());
        v[0x98+124..0x98+128].copy_from_slice(&0x100u32.to_le_bytes());
        assert_eq!(inspect_pe64(&v), Err(PeError::InvalidImage));
        v = b; v[0x98+56..0x98+60].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(inspect_pe64(&v), Err(PeError::InvalidImage));
        v = b; v[0x86..0x88].copy_from_slice(&100u16.to_le_bytes());
        assert_eq!(inspect_pe64(&v), Err(PeError::BadSectionCount));
    }
}
