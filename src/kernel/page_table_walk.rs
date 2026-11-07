//! Read-only x86-64 page-table walker, independent of hardware access.
//!
//! It cannot install mappings, modify page tables, or provide Ring 3 isolation.
//! Callers must check that every physical page-table frame is actually mapped,
//! owned by the active bootloader or kernel, and safe to read.
use crate::paging::{HUGE, NO_EXECUTE, PRESENT, USER, WRITABLE, is_canonical};

const FRAME_MASK: u64 = 0x000f_ffff_ffff_f000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalkError {
    UnsupportedWidth,
    NonCanonical,
    InvalidRoot,
    Unmapped,
    InvalidTable,
    InvalidLargePage,
    PhysicalOutOfRange,
    TableReadDenied,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Translation {
    pub physical_address: u64,
    pub page_size: u64,
    pub writable: bool,
    pub user: bool,
    pub executable: bool,
    pub levels: u8,
}

fn physical_ok(physical: u64, len: u64, bits: u8) -> bool {
    physical
        .checked_add(len)
        .is_some_and(|end| end <= (1u64 << bits))
}

/// Walk 4- or 5-level page tables without allocating.
///
/// `read_entry` must only read an entry from an already mapped, verified
/// physical page-table frame. This function deliberately has no pointer casts.
pub fn translate<F>(
    root_physical: u64,
    virtual_address: u64,
    virtual_bits: u8,
    physical_bits: u8,
    mut read_entry: F,
) -> Result<Translation, WalkError>
where
    F: FnMut(u64, usize) -> Result<u64, WalkError>,
{
    if !matches!(virtual_bits, 48 | 57) || !(32..=52).contains(&physical_bits) {
        return Err(WalkError::UnsupportedWidth);
    }
    if !is_canonical(virtual_address, virtual_bits) {
        return Err(WalkError::NonCanonical);
    }
    if root_physical == 0
        || root_physical & 0xfff != 0
        || !physical_ok(root_physical, 4096, physical_bits)
    {
        return Err(WalkError::InvalidRoot);
    }

    let mut table = root_physical;
    let mut writable = true;
    let mut user = true;
    let mut executable = true;
    let levels = if virtual_bits == 57 { 5 } else { 4 };

    for level in (1..=levels).rev() {
        let shift = 12 + 9 * (level - 1);
        let index = ((virtual_address >> shift) & 0x1ff) as usize;
        let raw = read_entry(table, index)?;
        if raw & PRESENT == 0 {
            return Err(WalkError::Unmapped);
        }
        writable &= raw & WRITABLE != 0;
        user &= raw & USER != 0;
        executable &= raw & NO_EXECUTE == 0;

        let large = raw & HUGE != 0;
        if large && level != 3 && level != 2 {
            return Err(WalkError::InvalidLargePage);
        }

        let phys = raw & FRAME_MASK;
        if phys >= 1u64 << physical_bits {
            return Err(WalkError::PhysicalOutOfRange);
        }

        if level == 1 || large {
            let page_size = 1u64 << shift;
            if phys & (page_size - 1) != 0 {
                return Err(WalkError::InvalidLargePage);
            }
            let base = phys
                .checked_add(virtual_address & (page_size - 1))
                .ok_or(WalkError::PhysicalOutOfRange)?;
            if !physical_ok(phys, page_size, physical_bits) {
                return Err(WalkError::PhysicalOutOfRange);
            }
            return Ok(Translation {
                physical_address: base,
                page_size,
                writable,
                user,
                executable,
                levels: levels as u8,
            });
        }
        if phys == 0 || !physical_ok(phys, 4096, physical_bits) {
            return Err(WalkError::InvalidTable);
        }
        table = phys;
    }
    Err(WalkError::InvalidTable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn four_level(virtual_address: u64, leaf: u64) -> Result<Translation, WalkError> {
        translate(0x1000, virtual_address, 48, 36, |table, index| {
            Ok(match (table, index) {
                (0x1000, 0) => 0x2000 | PRESENT | WRITABLE, // supervisor root
                (0x2000, 0) => 0x3000 | PRESENT | WRITABLE | USER,
                (0x3000, 2) => 0x4000 | PRESENT | WRITABLE | USER,
                (0x4000, 0) => leaf,
                _ => 0,
            })
        })
    }

    #[test]
    fn walks_regular_pages_and_intersects_permissions() {
        let translation = four_level(0x400000, 0x5000 | PRESENT | USER | WRITABLE).unwrap();
        assert_eq!(translation.physical_address, 0x5000);
        assert_eq!(translation.page_size, 4096);
        assert!(translation.writable);
        assert!(!translation.user); // parent supervisor restriction wins
        assert!(translation.executable);
        assert_eq!(translation.levels, 4);
    }

    #[test]
    fn detects_unmapped_and_inherited_nx_permissions() {
        assert_eq!(four_level(0x400000, 0), Err(WalkError::Unmapped));
        let got = four_level(0x400000, 0x5000 | PRESENT | NO_EXECUTE).unwrap();
        assert!(!got.executable);
        assert!(!got.writable);
    }

    #[test]
    fn resolves_2mb_huge_leaf() {
        let got = translate(0x1000, 0x202345, 48, 36, |table, index| {
            Ok(match (table, index) {
                (0x1000, 0) => 0x2000 | PRESENT,
                (0x2000, 0) => 0x3000 | PRESENT,
                (0x3000, 1) => 0x800000 | PRESENT | HUGE,
                _ => 0,
            })
        })
        .unwrap();
        assert_eq!(got.physical_address, 0x802345);
        assert_eq!(got.page_size, 2 * 1024 * 1024);
    }

    #[test]
    fn resolves_1gb_huge_leaf() {
        let got = translate(0x1000, 0x40345678, 48, 36, |table, index| {
            Ok(match (table, index) {
                (0x1000, 0) => 0x2000 | PRESENT,
                (0x2000, 1) => 0x40000000 | PRESENT | HUGE,
                _ => 0,
            })
        })
        .unwrap();
        assert_eq!(got.physical_address, 0x40345678);
        assert_eq!(got.page_size, 1 << 30);
    }

    #[test]
    fn supports_five_level_walk_and_rejects_noncanonical() {
        let got = translate(0x1000, 0x1234, 57, 36, |table, index| {
            Ok(match (table, index) {
                (0x1000, 0) => 0x2000 | PRESENT,
                (0x2000, 0) => 0x3000 | PRESENT,
                (0x3000, 0) => 0x4000 | PRESENT,
                (0x4000, 0) => 0x5000 | PRESENT,
                (0x5000, 1) => 0x6000 | PRESENT,
                _ => 0,
            })
        })
        .unwrap();
        assert_eq!(got.physical_address, 0x6234);
        assert_eq!(got.levels, 5);
        assert_eq!(
            translate(0x1000, 0x0080_0000_0000_0000, 48, 36, |_, _| Ok(0)),
            Err(WalkError::NonCanonical)
        );
    }

    #[test]
    fn refuses_invalid_roots_large_page_alignment_and_inaccessible_tables() {
        assert_eq!(
            translate(0x1010, 0, 48, 36, |_, _| Ok(0)),
            Err(WalkError::InvalidRoot)
        );
        assert_eq!(
            translate(0x1000, 0, 48, 36, |_, _| Err(WalkError::TableReadDenied)),
            Err(WalkError::TableReadDenied)
        );
        assert_eq!(
            translate(0x1000, 0, 48, 36, |table, _| Ok(match table {
                0x1000 => 0x2000 | PRESENT,
                0x2000 => 0x3000 | PRESENT,
                _ => 0x201000 | PRESENT | HUGE,
            })),
            Err(WalkError::InvalidLargePage)
        );
    }

    #[test]
    fn rejects_out_of_range_frame_and_parent_large_page() {
        assert_eq!(
            translate(0x1000, 0, 48, 36, |_, _| Ok((1u64 << 36) | PRESENT)),
            Err(WalkError::PhysicalOutOfRange)
        );
        assert_eq!(
            translate(0x1000, 0, 48, 36, |_, _| Ok(0x2000 | PRESENT | HUGE)),
            Err(WalkError::InvalidLargePage)
        );
    }
}
