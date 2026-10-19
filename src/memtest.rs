//! A bounded HHDM write/read/zero probe of exactly two owned usable frames.
//!
//! This is a QEMU integration experiment, not yet a page-table allocator.
//! The Limine base-revision-6 direct map covers usable RAM as supervisor RW.
//! We only touch frames that were uniquely reserved from type-0 regions.

use core::ptr;
use zillaos_kernel::memory::{PhysicalFrame, PAGE_SIZE};
use zillaos_kernel::paging::is_canonical;

#[derive(Debug, PartialEq, Eq)]
pub enum ProbeError {
    InvalidHhdmOffset,
    NonCanonicalMapping,
    AddressOverflow,
    UnalignedPhysicalFrame,
    AliasedFrames,
    PatternMismatch,
}

fn mapped_frame(
    frame: PhysicalFrame,
    hhdm_offset: u64,
    virtual_bits: u8,
) -> Result<*mut u64, ProbeError> {
    if frame.address % PAGE_SIZE != 0 {
        return Err(ProbeError::UnalignedPhysicalFrame);
    }
    let virtual_address = hhdm_offset
        .checked_add(frame.address)
        .ok_or(ProbeError::AddressOverflow)?;
    let virtual_end = virtual_address
        .checked_add(PAGE_SIZE - 1)
        .ok_or(ProbeError::AddressOverflow)?;
    // Trusted bootloader returns an address in the upper canonical half.
    // No fixed offset is assumed; the 4 KiB buffer must be contiguous.
    if hhdm_offset == 0 || virtual_address < hhdm_offset || virtual_end < virtual_address {
        return Err(ProbeError::InvalidHhdmOffset);
    }
    if !is_canonical(virtual_address, virtual_bits) || !is_canonical(virtual_end, virtual_bits) {
        return Err(ProbeError::NonCanonicalMapping);
    }
    if virtual_address & 7 != 0 {
        return Err(ProbeError::UnalignedPhysicalFrame);
    }
    Ok(virtual_address as *mut u64)
}

/// # Safety
///
/// Caller guarantees both `frame` values are distinct, exclusively owned,
/// type-0 USABLE physical pages, and that the trusted Limine HHDM maps those
/// pages as writable. Must run before reclaiming/changing boot page tables.
pub unsafe fn two_frame_probe(
    first: PhysicalFrame,
    second: PhysicalFrame,
    hhdm_offset: u64,
    virtual_bits: u8,
) -> Result<(), ProbeError> {
    if first.address == second.address {
        return Err(ProbeError::AliasedFrames);
    }
    let a = mapped_frame(first, hhdm_offset, virtual_bits)?;
    let b = mapped_frame(second, hhdm_offset, virtual_bits)?;
    const WORDS: usize = PAGE_SIZE as usize / core::mem::size_of::<u64>();

    // SAFETY: caller owns both aligned mapped pages. All pointer arithmetic
    // remains within 4096 bytes of each distinct HHDM-backed frame.
    unsafe {
        for index in 0..WORDS {
            ptr::write_volatile(a.add(index), 0x5a11_a110_0101_0000u64 ^ index as u64);
            ptr::write_volatile(b.add(index), 0xa55a_5aa5_2020_0000u64 ^ index as u64);
        }

        let mut mismatch = false;
        for index in 0..WORDS {
            let a_expected = 0x5a11_a110_0101_0000u64 ^ index as u64;
            let b_expected = 0xa55a_5aa5_2020_0000u64 ^ index as u64;
            if ptr::read_volatile(a.add(index)) != a_expected
                || ptr::read_volatile(b.add(index)) != b_expected
            {
                mismatch = true;
            }
        }

        // Always sanitize pages after testing, including mismatch cases.
        for index in 0..WORDS {
            ptr::write_volatile(a.add(index), 0);
            ptr::write_volatile(b.add(index), 0);
        }

        if mismatch {
            return Err(ProbeError::PatternMismatch);
        }
    }
    Ok(())
}
