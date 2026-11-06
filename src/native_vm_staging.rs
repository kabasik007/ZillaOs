//! Opt-in, no-switch experiment: stage an independent user PML4 in
//! exclusively reserved USABLE RAM. Does NOT perform CR3 or Ring 3 entry.
//! The active Limine page tables are read-only and never reclaimed.
use core::ptr;

use zillaos_kernel::memory::{FrameAllocator, MemoryRegion, PhysicalFrame, PAGE_SIZE};
use zillaos_kernel::page_table_walk::{translate, WalkError};
use zillaos_kernel::paging::{
    is_canonical, NO_EXECUTE, PRESENT, USER, WRITABLE,
};
use zillaos_kernel::process::address_space::{
    Access, AddressSpacePlan, Permissions, UserRegion,
};

use crate::page_table_observer::Snapshot;

const HHDM_USABLE: u64 = 0;
const HHDM_BOOTLOADER: u64 = 5;
const CODE_VA: u64 = 0x0040_0000;
const DATA_VA: u64 = 0x0040_1000;
const GUARD_VA: u64 = 0x007f_e000;
const STACK_VA: u64 = 0x007f_f000;
const STACK_TOP: u64 = 0x0080_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageError {
    UnsupportedPaging,
    PhysicalMemoryUnavailable,
    InvalidHhdm,
    UnsafeBootMapping,
    InvalidUserPlan,
    InvalidPageTables,
}

pub struct StagedSpace {
    pub root: u64,
    pub code_frame: u64,
    pub data_frame: u64,
    pub stack_frame: u64,
}

fn mapped_table_frame(regions: &[MemoryRegion], physical: u64) -> bool {
    let Some(end) = physical.checked_add(PAGE_SIZE) else { return false };
    physical % PAGE_SIZE == 0 && regions.iter().any(|region| {
        matches!(region.kind, HHDM_USABLE | HHDM_BOOTLOADER)
            && region.base <= physical
            && region.base.checked_add(region.length).is_some_and(|last| end <= last)
    })
}

fn frame_address(frame: u64, hhdm: u64) -> Result<*mut u64, StageError> {
    let start = hhdm.checked_add(frame).ok_or(StageError::InvalidHhdm)?;
    let end = start.checked_add(PAGE_SIZE - 1).ok_or(StageError::InvalidHhdm)?;
    if !is_canonical(start, 48) || !is_canonical(end, 48) {
        return Err(StageError::InvalidHhdm);
    }
    Ok(start as *mut u64)
}

fn reserve_clean(allocator: &mut FrameAllocator, hhdm: u64) -> Result<PhysicalFrame, StageError> {
    let frame = allocator.reserve_frame().ok_or(StageError::PhysicalMemoryUnavailable)?;
    let ptr = frame_address(frame.address, hhdm)?;
    // SAFETY: FrameAllocator grants exclusive ownership of this USABLE frame.
    // Limine base revision 6 maps USABLE pages via the HHDM, and no CR3
    // change has been performed. This zeroes exactly one owned page.
    unsafe { ptr::write_bytes(ptr, 0, 512) };
    Ok(frame)
}

fn put(frame: u64, index: usize, value: u64, hhdm: u64) -> Result<(), StageError> {
    if index >= 512 { return Err(StageError::InvalidPageTables) }
    let ptr = frame_address(frame, hhdm)?;
    // SAFETY: all destination frames were exclusively allocated and zeroed.
    unsafe { ptr::write_volatile(ptr.add(index), value) };
    Ok(())
}

fn read_verified(regions: &[MemoryRegion], frame: u64, index: usize, hhdm: u64)
    -> Result<u64, StageError>
{
    if index >= 512 || !mapped_table_frame(regions, frame) {
        return Err(StageError::UnsafeBootMapping);
    }
    let ptr = frame_address(frame, hhdm)?;
    // SAFETY: verified HHDM-mapped frame, read-only access.
    Ok(unsafe { ptr::read_volatile(ptr.add(index)) })
}

/// # Safety
///
/// Single-core boot only; caller owns the *same* bootstrap FrameAllocator
/// used for earlier probes, and guarantees that Limine page tables still
/// exist and that `snapshot` was validated by the read-only observer.
/// This routine never loads the returned root into CR3.
pub unsafe fn stage(
    allocator: &mut FrameAllocator,
    regions: &[MemoryRegion],
    hhdm: u64,
    snapshot: &Snapshot,
    physical_bits: u8,
    nx_supported: bool,
) -> Result<StagedSpace, StageError> {
    if snapshot.virtual_bits != 48 || !nx_supported {
        return Err(StageError::UnsupportedPaging);
    }
    if !mapped_table_frame(regions, snapshot.cr3) {
        return Err(StageError::UnsafeBootMapping);
    }

    let mut plan = AddressSpacePlan::with_stack(STACK_TOP, 1)
        .map_err(|_| StageError::InvalidUserPlan)?;
    plan.map(UserRegion::checked(CODE_VA, PAGE_SIZE, Permissions::READ_EXECUTE)
        .map_err(|_| StageError::InvalidUserPlan)?)
        .map_err(|_| StageError::InvalidUserPlan)?;
    plan.map(UserRegion::checked(DATA_VA, PAGE_SIZE, Permissions::READ_WRITE)
        .map_err(|_| StageError::InvalidUserPlan)?)
        .map_err(|_| StageError::InvalidUserPlan)?;
    if !plan.allows(CODE_VA, 1, Access::Execute)
        || !plan.allows(DATA_VA, 1, Access::Write)
        || !plan.allows(STACK_VA, 1, Access::Write)
        || plan.allows(GUARD_VA, 1, Access::Read)
    {
        return Err(StageError::InvalidUserPlan);
    }

    let root = reserve_clean(allocator, hhdm)?;
    let pdpt = reserve_clean(allocator, hhdm)?;
    let pd = reserve_clean(allocator, hhdm)?;
    let pt_code = reserve_clean(allocator, hhdm)?;
    let pt_stack = reserve_clean(allocator, hhdm)?;
    let code = reserve_clean(allocator, hhdm)?;
    let data = reserve_clean(allocator, hhdm)?;
    let stack = reserve_clean(allocator, hhdm)?;

    // Mirror the upper half *only*. Every imported present PML4 entry must
    // be supervisor-only. Do not copy lower half or make boot tables USER.
    for index in 256..512 {
        let entry = read_verified(regions, snapshot.cr3, index, hhdm)?;
        if entry & (PRESENT | USER) == (PRESENT | USER) {
            return Err(StageError::UnsafeBootMapping);
        }
        put(root.address, index, entry, hhdm)?;
    }

    let user_table = PRESENT | USER | WRITABLE;
    put(root.address, 0, pdpt.address | user_table, hhdm)?;
    put(pdpt.address, 0, pd.address | user_table, hhdm)?;
    put(pd.address, 2, pt_code.address | user_table, hhdm)?;
    put(pd.address, 3, pt_stack.address | user_table, hhdm)?;
    put(pt_code.address, 0, code.address | PRESENT | USER, hhdm)?;
    put(pt_code.address, 1, data.address | PRESENT | USER | WRITABLE | NO_EXECUTE, hhdm)?;
    put(pt_stack.address, 511, stack.address | PRESENT | USER | WRITABLE | NO_EXECUTE, hhdm)?;
    // PT[510] is intentionally absent: stack guard page.
    let verify = |va| {
        translate(root.address, va, 48, physical_bits, |frame, index| {
            read_verified(regions, frame, index, hhdm)
                .map_err(|_| WalkError::TableReadDenied)
        })
    };
    let text = verify(CODE_VA).map_err(|_| StageError::InvalidPageTables)?;
    let writable_data = verify(DATA_VA).map_err(|_| StageError::InvalidPageTables)?;
    let writable_stack = verify(STACK_VA).map_err(|_| StageError::InvalidPageTables)?;
    if text.physical_address != code.address || !text.user || text.writable || !text.executable
        || writable_data.physical_address != data.address
        || !writable_data.user || !writable_data.writable || writable_data.executable
        || writable_stack.physical_address != stack.address
        || !writable_stack.user || !writable_stack.writable || writable_stack.executable
        || verify(GUARD_VA) != Err(WalkError::Unmapped)
    {
        return Err(StageError::InvalidPageTables);
    }

    // Verifies inherited supervisor kernel mapping is still present.
    let kernel_text = verify(crate::_start as usize as u64)
        .map_err(|_| StageError::InvalidPageTables)?;
    if kernel_text.user || !kernel_text.executable {
        return Err(StageError::InvalidPageTables);
    }
    Ok(StagedSpace {
        root: root.address,
        code_frame: code.address,
        data_frame: data.address,
        stack_frame: stack.address,
    })
}
