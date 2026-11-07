//! Opt-in, no-switch experiment: stage an independent user PML4 in
//! exclusively reserved USABLE RAM. Does NOT perform CR3 or Ring 3 entry.
//! The active Limine page tables are read-only and never reclaimed.
use core::ptr;

use zillaos_kernel::memory::{FrameAllocator, MemoryRegion, PAGE_SIZE, PhysicalFrame};
use zillaos_kernel::page_table_walk::{WalkError, translate};
use zillaos_kernel::paging::{NO_EXECUTE, PRESENT, USER, WRITABLE, is_canonical};
use zillaos_kernel::process::address_space::{Access, AddressSpacePlan, Permissions, UserRegion};

use crate::page_table_observer::Snapshot;

const HHDM_USABLE: u64 = 0;
const HHDM_BOOTLOADER: u64 = 5;
const CODE_VA: u64 = 0x0040_0000;
const DATA_VA: u64 = 0x0040_1000;
const GUARD_VA: u64 = 0x007f_e000;
const STACK_VA: u64 = 0x007f_f000;
const STACK_TOP: u64 = 0x0080_0000;
/// Supervisor-only MMIO window in the optional preemption test PML4. It is
/// adjacent to the second user stack guard but never mapped user-accessible.
#[cfg(feature = "native-ring3-preempt")]
pub const LAPIC_MMIO_VA: u64 = 0x007f_b000;

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
    #[cfg(feature = "native-ring3-threads")]
    pub stack2_frame: u64,
}

fn mapped_table_frame(regions: &[MemoryRegion], physical: u64) -> bool {
    let Some(end) = physical.checked_add(PAGE_SIZE) else {
        return false;
    };
    physical % PAGE_SIZE == 0
        && regions.iter().any(|region| {
            matches!(region.kind, HHDM_USABLE | HHDM_BOOTLOADER)
                && region.base <= physical
                && region
                    .base
                    .checked_add(region.length)
                    .is_some_and(|last| end <= last)
        })
}

fn frame_address(frame: u64, hhdm: u64) -> Result<*mut u64, StageError> {
    let start = hhdm.checked_add(frame).ok_or(StageError::InvalidHhdm)?;
    let end = start
        .checked_add(PAGE_SIZE - 1)
        .ok_or(StageError::InvalidHhdm)?;
    if !is_canonical(start, 48) || !is_canonical(end, 48) {
        return Err(StageError::InvalidHhdm);
    }
    Ok(start as *mut u64)
}

fn reserve_clean(allocator: &mut FrameAllocator, hhdm: u64) -> Result<PhysicalFrame, StageError> {
    let frame = allocator
        .reserve_frame()
        .ok_or(StageError::PhysicalMemoryUnavailable)?;
    let ptr = frame_address(frame.address, hhdm)?;
    // SAFETY: FrameAllocator grants exclusive ownership of this USABLE frame.
    // Limine base revision 6 maps USABLE pages via the HHDM, and no CR3
    // change has been performed. This zeroes exactly one owned page.
    unsafe { ptr::write_bytes(ptr, 0, 512) };
    Ok(frame)
}

fn put(frame: u64, index: usize, value: u64, hhdm: u64) -> Result<(), StageError> {
    if index >= 512 {
        return Err(StageError::InvalidPageTables);
    }
    let ptr = frame_address(frame, hhdm)?;
    // SAFETY: all destination frames were exclusively allocated and zeroed.
    unsafe { ptr::write_volatile(ptr.add(index), value) };
    Ok(())
}

fn read_verified(
    regions: &[MemoryRegion],
    frame: u64,
    index: usize,
    hhdm: u64,
) -> Result<u64, StageError> {
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

    let mut plan =
        AddressSpacePlan::with_stack(STACK_TOP, 1).map_err(|_| StageError::InvalidUserPlan)?;
    plan.map(
        UserRegion::checked(CODE_VA, PAGE_SIZE, Permissions::READ_EXECUTE)
            .map_err(|_| StageError::InvalidUserPlan)?,
    )
    .map_err(|_| StageError::InvalidUserPlan)?;
    plan.map(
        UserRegion::checked(DATA_VA, PAGE_SIZE, Permissions::READ_WRITE)
            .map_err(|_| StageError::InvalidUserPlan)?,
    )
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
    #[cfg(feature = "native-ring3-threads")]
    let stack2 = reserve_clean(allocator, hhdm)?;

    // Mirror the upper half *only*. Clear USER at the new root, even when
    // Limine has USER on an ancestor but supervisor-only effective access
    // through lower-level entries. Never alter the bootloader's own PML4.
    for index in 256..512 {
        let entry = read_verified(regions, snapshot.cr3, index, hhdm)?;
        let supervisor_entry = entry & !USER;
        put(root.address, index, supervisor_entry, hhdm)?;
        if read_verified(regions, root.address, index, hhdm)? & USER != 0 {
            return Err(StageError::UnsafeBootMapping);
        }
    }

    let user_table = PRESENT | USER | WRITABLE;
    put(root.address, 0, pdpt.address | user_table, hhdm)?;
    put(pdpt.address, 0, pd.address | user_table, hhdm)?;
    put(pd.address, 2, pt_code.address | user_table, hhdm)?;
    put(pd.address, 3, pt_stack.address | user_table, hhdm)?;
    put(pt_code.address, 0, code.address | PRESENT | USER, hhdm)?;
    put(
        pt_code.address,
        1,
        data.address | PRESENT | USER | WRITABLE | NO_EXECUTE,
        hhdm,
    )?;
    put(
        pt_stack.address,
        511,
        stack.address | PRESENT | USER | WRITABLE | NO_EXECUTE,
        hhdm,
    )?;
    // PT[510] is intentionally absent: the first stack guard page.
    #[cfg(feature = "native-ring3-threads")]
    put(
        pt_stack.address,
        509,
        stack2.address | PRESENT | USER | WRITABLE | NO_EXECUTE,
        hhdm,
    )?;
    // Second stack at 0x7fd000, distinct from first 0x7ff000; PT[508]
    // remains absent as its independent lower guard page.

    #[cfg(feature = "native-ring3-preempt")]
    let lapic_physical: u64 = {
        let low: u32;
        let high: u32;
        // SAFETY: QEMU exposes the xAPIC IA32_APIC_BASE MSR on CPU0. No
        // MMIO dereference occurs here; the PTE is installed before use.
        unsafe {
            core::arch::asm!("rdmsr", in("ecx") 0x1b_u32,
                out("eax") low, out("edx") high, options(nomem, nostack));
        }
        let base_msr = ((high as u64) << 32) | low as u64;
        let physical = base_msr & 0x000f_ffff_ffff_f000;
        if base_msr & (1 << 11) == 0
            || base_msr & (1 << 10) != 0
            || physical == 0
            || physical >= (1u64 << physical_bits)
        {
            return Err(StageError::UnsupportedPaging);
        }
        // Index 507 of this existing stack PT is unused, supervisor-only,
        // write-through + cache-disable, NX. Do not install in Limine CR3.
        put(
            pt_stack.address,
            507,
            physical | PRESENT | WRITABLE | NO_EXECUTE | (1 << 3) | (1 << 4),
            hhdm,
        )?;
        physical
    };
    let verify = |va| {
        translate(root.address, va, 48, physical_bits, |frame, index| {
            read_verified(regions, frame, index, hhdm).map_err(|_| WalkError::TableReadDenied)
        })
    };
    let text = verify(CODE_VA).map_err(|_| StageError::InvalidPageTables)?;
    let writable_data = verify(DATA_VA).map_err(|_| StageError::InvalidPageTables)?;
    let writable_stack = verify(STACK_VA).map_err(|_| StageError::InvalidPageTables)?;
    #[cfg(feature = "native-ring3-threads")]
    let writable_stack2 = verify(0x007f_d000).map_err(|_| StageError::InvalidPageTables)?;
    if text.physical_address != code.address
        || !text.user
        || text.writable
        || !text.executable
        || writable_data.physical_address != data.address
        || !writable_data.user
        || !writable_data.writable
        || writable_data.executable
        || writable_stack.physical_address != stack.address
        || !writable_stack.user
        || !writable_stack.writable
        || writable_stack.executable
        || verify(GUARD_VA) != Err(WalkError::Unmapped)
    {
        return Err(StageError::InvalidPageTables);
    }
    #[cfg(feature = "native-ring3-threads")]
    if writable_stack2.physical_address != stack2.address
        || !writable_stack2.user
        || !writable_stack2.writable
        || writable_stack2.executable
        || verify(0x007f_c000) != Err(WalkError::Unmapped)
    {
        return Err(StageError::InvalidPageTables);
    }

    // Verifies inherited supervisor kernel mapping is still present.
    #[cfg(feature = "native-ring3-preempt")]
    {
        let lapic = verify(LAPIC_MMIO_VA).map_err(|_| StageError::InvalidPageTables)?;
        if lapic.physical_address != lapic_physical
            || lapic.user
            || !lapic.writable
            || lapic.executable
            || verify(0x007f_c000) != Err(WalkError::Unmapped)
        {
            return Err(StageError::InvalidPageTables);
        }
    }

    let kernel_text =
        verify(crate::_start as usize as u64).map_err(|_| StageError::InvalidPageTables)?;
    if kernel_text.user || !kernel_text.executable {
        return Err(StageError::InvalidPageTables);
    }
    Ok(StagedSpace {
        root: root.address,
        code_frame: code.address,
        data_frame: data.address,
        stack_frame: stack.address,
        #[cfg(feature = "native-ring3-threads")]
        stack2_frame: stack2.address,
    })
}
