//! Pure x86-64 virtual-address and paging metadata primitives.
//!
//! No CR3 changes or page-table edits are made here. Host-side tests can
//! exercise page arithmetic before installing any kernel-owned page tables.

pub const PAGE_SHIFT: u64 = 12;
pub const PAGE_BYTES: u64 = 1 << PAGE_SHIFT;
pub const ENTRY_ADDRESS_MASK: u64 = 0x000f_ffff_ffff_f000;

pub const PRESENT: u64 = 1 << 0;
pub const WRITABLE: u64 = 1 << 1;
pub const USER: u64 = 1 << 2;
pub const WRITE_THROUGH: u64 = 1 << 3;
pub const CACHE_DISABLE: u64 = 1 << 4;
pub const ACCESSED: u64 = 1 << 5;
pub const DIRTY: u64 = 1 << 6;
pub const HUGE: u64 = 1 << 7;
pub const GLOBAL: u64 = 1 << 8;
pub const NO_EXECUTE: u64 = 1 << 63;

const ALLOWED_TABLE_FLAGS: u64 =
    PRESENT | WRITABLE | USER | WRITE_THROUGH | CACHE_DISABLE | ACCESSED | NO_EXECUTE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagingError {
    InvalidVirtualWidth,
    NonCanonicalAddress,
    UnalignedPhysicalAddress,
    InvalidPhysicalWidth,
    PhysicalAddressOutOfRange,
    UnsupportedFlags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageIndices {
    pub pml5: Option<usize>,
    pub pml4: usize,
    pub pdpt: usize,
    pub pd: usize,
    pub pt: usize,
    pub offset: usize,
}

pub fn is_canonical(address: u64, virtual_bits: u8) -> bool {
    if virtual_bits != 48 && virtual_bits != 57 {
        return false;
    }
    let shift = 64 - virtual_bits;
    (((address << shift) as i64) >> shift) as u64 == address
}

pub fn indices(address: u64, virtual_bits: u8) -> Result<PageIndices, PagingError> {
    if virtual_bits != 48 && virtual_bits != 57 {
        return Err(PagingError::InvalidVirtualWidth);
    }
    if !is_canonical(address, virtual_bits) {
        return Err(PagingError::NonCanonicalAddress);
    }
    Ok(PageIndices {
        pml5: if virtual_bits == 57 {
            Some(((address >> 48) & 0x1ff) as usize)
        } else {
            None
        },
        pml4: ((address >> 39) & 0x1ff) as usize,
        pdpt: ((address >> 30) & 0x1ff) as usize,
        pd: ((address >> 21) & 0x1ff) as usize,
        pt: ((address >> 12) & 0x1ff) as usize,
        offset: (address & 0xfff) as usize,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    /// Create a pointer to a next-level page table (not a leaf mapping).
    /// NX permission is permitted in the bit representation only; the caller
    /// must check CPUID NX capability and enable EFER.NXE before installing it.
    pub fn table_pointer(
        physical_address: u64,
        physical_bits: u8,
        flags: u64,
    ) -> Result<Self, PagingError> {
        if !(32..=52).contains(&physical_bits) {
            return Err(PagingError::InvalidPhysicalWidth);
        }
        if physical_address & (PAGE_BYTES - 1) != 0 {
            return Err(PagingError::UnalignedPhysicalAddress);
        }
        if physical_address >= 1u64 << physical_bits {
            return Err(PagingError::PhysicalAddressOutOfRange);
        }
        if flags & !ALLOWED_TABLE_FLAGS != 0 {
            return Err(PagingError::UnsupportedFlags);
        }
        Ok(Self(physical_address | flags))
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    pub const fn physical_address(self) -> u64 {
        self.0 & ENTRY_ADDRESS_MASK
    }

    pub const fn is_present(self) -> bool {
        self.0 & PRESENT != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_48_bit_boundaries() {
        assert!(is_canonical(0x0000_7fff_ffff_ffff, 48));
        assert!(!is_canonical(0x0000_8000_0000_0000, 48));
        assert!(is_canonical(0xffff_8000_0000_0000, 48));
        assert!(!is_canonical(0xffff_7fff_ffff_ffff, 48));
    }

    #[test]
    fn canonical_57_bit_boundaries() {
        assert!(is_canonical(0x00ff_ffff_ffff_ffff, 57));
        assert!(!is_canonical(0x0100_0000_0000_0000, 57));
        assert!(is_canonical(0xff00_0000_0000_0000, 57));
    }

    #[test]
    fn indices_for_regular_4k_address() {
        let i = indices(0xffff_8000_0000_2003, 48).unwrap();
        assert_eq!(i.pml5, None);
        assert_eq!(i.pml4, 256);
        assert_eq!(i.pdpt, 0);
        assert_eq!(i.pd, 0);
        assert_eq!(i.pt, 2);
        assert_eq!(i.offset, 3);
    }

    #[test]
    fn indices_for_five_level_address() {
        let i = indices(0xff00_0000_0000_1000, 57).unwrap();
        assert_eq!(i.pml5, Some(256));
        assert_eq!(i.pt, 1);
    }

    #[test]
    fn rejects_holes_and_invalid_widths() {
        assert_eq!(
            indices(0x0000_8000_0000_0000, 48),
            Err(PagingError::NonCanonicalAddress)
        );
        assert_eq!(
            indices(0, 32),
            Err(PagingError::InvalidVirtualWidth)
        );
    }

    #[test]
    fn safe_table_entry_building() {
        let entry = PageTableEntry::table_pointer(
            0x1234_5000, 36, PRESENT | WRITABLE
        ).unwrap();
        assert_eq!(entry.physical_address(), 0x1234_5000);
        assert!(entry.is_present());
    }

    #[test]
    fn disallow_unaligned_reserved_and_out_of_range_entries() {
        assert_eq!(
            PageTableEntry::table_pointer(0x1234, 36, PRESENT),
            Err(PagingError::UnalignedPhysicalAddress)
        );
        assert_eq!(
            PageTableEntry::table_pointer(0x2000, 36, PRESENT | HUGE),
            Err(PagingError::UnsupportedFlags)
        );
        assert_eq!(
            PageTableEntry::table_pointer(1u64 << 36, 36, PRESENT),
            Err(PagingError::PhysicalAddressOutOfRange)
        );
    }
}
