//! Opt-in read-only observation of Limine-owned active x86-64 page tables.
//! Never rewrite bootloader PTEs, enable USER on them, or load a different CR3.
use core::arch::asm;
use core::ptr;

use zillaos_kernel::memory::{MemoryRegion, PAGE_SIZE};
use zillaos_kernel::page_table_walk::{translate, Translation, WalkError};
use zillaos_kernel::paging::is_canonical;

const BOOTLOADER_RECLAIMABLE: u64 = 5;
const USABLE: u64 = 0;
const CR3_ADDRESS_MASK: u64 = 0x000f_ffff_ffff_f000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObserverError {
    InvalidHhdm,
    InvalidPhysicalMap,
    Walk(WalkError),
    UnsafeSupervisorMapping,
    KernelCodeNotExecutable,
    StackNotWritable,
    HhdmNotWritable,
}

impl From<WalkError> for ObserverError {
    fn from(err: WalkError) -> Self { Self::Walk(err) }
}

pub struct Snapshot {
    pub cr3: u64,
    pub virtual_bits: u8,
    pub kernel_text: Translation,
    pub kernel_stack: Translation,
    pub hhdm_page: Translation,
}

/// Validate a complete physical table frame is in the bootloader's
/// explicitly HHDM-mapped regions. No speculative dereferences.
fn mapped_table_frame(regions: &[MemoryRegion], start: u64) -> bool {
    let Some(end) = start.checked_add(PAGE_SIZE) else { return false };
    if start % PAGE_SIZE != 0 { return false }
    regions.iter().any(|region| {
        (region.kind == BOOTLOADER_RECLAIMABLE || region.kind == USABLE)
            && region.base <= start
            && region.base.checked_add(region.length).is_some_and(|region_end| end <= region_end)
    })
}

struct Reader<'a> {
    regions: &'a [MemoryRegion],
    hhdm_offset: u64,
    virtual_bits: u8,
}

impl Reader<'_> {
    fn read(&self, table: u64, index: usize) -> Result<u64, WalkError> {
        if index >= 512 || !mapped_table_frame(self.regions, table) {
            return Err(WalkError::TableReadDenied);
        }
        let address = self.hhdm_offset.checked_add(table)
            .and_then(|address| address.checked_add(index as u64 * 8))
            .ok_or(WalkError::TableReadDenied)?;
        if !is_canonical(address, self.virtual_bits) {
            return Err(WalkError::TableReadDenied);
        }
        // SAFETY: Limine revision 6 HHDM maps entire usable and bootloader-
        // reclaimable memory regions. The physical page has been checked
        // against the still-live memory map. Only one CPU is running.
        Ok(unsafe { ptr::read_volatile(address as *const u64) })
    }
}

/// # Safety
///
/// Only call before reclaiming any Limine bootloader pages and while
/// interrupts remain disabled. The caller must pass Limine's validated
/// base-revision-6 memory map and HHDM offset, not untrusted input.
pub unsafe fn observe(
    regions: &[MemoryRegion],
    hhdm_offset: u64,
    physical_bits: u8,
    owned_probe_frame: u64,
) -> Result<Snapshot, ObserverError> {
    let cr3: u64;
    let cr4: u64;
    let stack_pointer: u64;
    // SAFETY: read-only privileged CPU register queries from Ring 0.
    unsafe {
        asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
        asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
        asm!("mov {}, rsp", out(reg) stack_pointer, options(nomem, nostack, preserves_flags));
    }
    let virtual_bits = if cr4 & (1 << 12) != 0 { 57 } else { 48 };
    let root = cr3 & CR3_ADDRESS_MASK;
    if hhdm_offset & 0xfff != 0
        || !is_canonical(hhdm_offset, virtual_bits)
        || !mapped_table_frame(regions, root)
    {
        return Err(ObserverError::InvalidHhdm);
    }
    if !mapped_table_frame(regions, owned_probe_frame) {
        return Err(ObserverError::InvalidPhysicalMap);
    }
    let hhdm_virtual = hhdm_offset.checked_add(owned_probe_frame)
        .ok_or(ObserverError::InvalidHhdm)?;
    let reader = Reader { regions, hhdm_offset, virtual_bits };
    let map = |address| {
        translate(root, address, virtual_bits, physical_bits, |phys, index| {
            reader.read(phys, index)
        }).map_err(ObserverError::from)
    };
    let kernel_text = map(crate::_start as usize as u64)?;
    let kernel_stack = map(stack_pointer)?;
    let hhdm_page = map(hhdm_virtual)?;

    if kernel_text.user || kernel_stack.user || hhdm_page.user {
        return Err(ObserverError::UnsafeSupervisorMapping);
    }
    if !kernel_text.executable {
        return Err(ObserverError::KernelCodeNotExecutable);
    }
    if !kernel_stack.writable {
        return Err(ObserverError::StackNotWritable);
    }
    if !hhdm_page.writable || hhdm_page.physical_address != owned_probe_frame {
        return Err(ObserverError::HhdmNotWritable);
    }
    Ok(Snapshot { cr3: root, virtual_bits, kernel_text, kernel_stack, hhdm_page })
}
