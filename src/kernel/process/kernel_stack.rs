//! Kernel stack reservation lifecycle CONTRACT for future Ring0 scheduling.
//!
//! No pages are allocated, mapped, guarded or freed here. The kernel VMM
//! remains the sole frame/page-table owner. This registry records only
//! non-overlapping virtual reservations and their phase, with monotonically
//! increasing lease IDs (no ABA reuse). It is NOT a TSS.RSP0 implementation.

use super::address_space::PAGE_SIZE;
use super::thread::{MAX_THREADS, Tid};

pub const MAX_KERNEL_STACK_PAGES: u64 = 64;
pub const KERNEL_HIGH_HALF_START: u64 = 0xffff_8000_0000_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackPhase {
    Reserved,
    Active,
    Retired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackError {
    InvalidRange,
    Capacity,
    IdExhausted,
    DuplicateOwner,
    Overlap,
    UnknownLease,
    InvalidPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KernelStackLease {
    pub id: u64,
    pub tid: Tid,
    pub guard_base: u64,
    pub mapped_base: u64,
    pub mapped_pages: u64,
    pub phase: StackPhase,
}

impl KernelStackLease {
    pub fn end(&self) -> u64 {
        // Reserve() checked all arithmetic and bounds before insertion.
        self.mapped_base + self.mapped_pages * PAGE_SIZE
    }
}

pub struct KernelStackRegistry {
    slots: [Option<KernelStackLease>; MAX_THREADS],
    next_id: u64,
}

impl Default for KernelStackRegistry {
    fn default() -> Self { Self::new() }
}

impl KernelStackRegistry {
    pub const fn new() -> Self {
        Self { slots: [None; MAX_THREADS], next_id: 1 }
    }

    pub fn iter(&self) -> impl Iterator<Item = KernelStackLease> + '_ {
        self.slots.iter().flatten().copied()
    }

    pub fn get(&self, id: u64) -> Option<KernelStackLease> {
        self.iter().find(|lease| lease.id == id)
    }

    pub fn lease_for_thread(&self, tid: Tid) -> Option<KernelStackLease> {
        self.iter().find(|lease| lease.tid == tid)
    }

    /// Reserve exactly one unmapped guard page plus bounded RW/NX kernel
    /// stack pages. The VM subsystem MUST install permissions separately.
    pub fn reserve(
        &mut self, tid: Tid, guard_base: u64, mapped_pages: u64,
    ) -> Result<u64, StackError> {
        if tid.0 == 0 || guard_base < KERNEL_HIGH_HALF_START
            || guard_base % PAGE_SIZE != 0
            || mapped_pages == 0 || mapped_pages > MAX_KERNEL_STACK_PAGES
        {
            return Err(StackError::InvalidRange);
        }
        let mapped_base = guard_base.checked_add(PAGE_SIZE)
            .ok_or(StackError::InvalidRange)?;
        let bytes = mapped_pages.checked_mul(PAGE_SIZE)
            .ok_or(StackError::InvalidRange)?;
        let end = mapped_base.checked_add(bytes).ok_or(StackError::InvalidRange)?;
        if self.iter().any(|lease| lease.tid == tid) {
            return Err(StackError::DuplicateOwner);
        }
        if self.iter().any(|lease| guard_base < lease.end() && lease.guard_base < end) {
            return Err(StackError::Overlap);
        }
        let slot = self.slots.iter().position(Option::is_none).ok_or(StackError::Capacity)?;
        let next = self.next_id.checked_add(1).ok_or(StackError::IdExhausted)?;
        let id = self.next_id;
        self.next_id = next;
        self.slots[slot] = Some(KernelStackLease {
            id, tid, guard_base, mapped_base, mapped_pages,
            phase: StackPhase::Reserved,
        });
        Ok(id)
    }

    pub fn activate(&mut self, id: u64) -> Result<(), StackError> {
        let lease = self.slots.iter_mut().flatten()
            .find(|lease| lease.id == id).ok_or(StackError::UnknownLease)?;
        if lease.phase != StackPhase::Reserved { return Err(StackError::InvalidPhase); }
        lease.phase = StackPhase::Active;
        Ok(())
    }

    /// # Safety
    /// Caller must ensure this TID is no longer running on ANY CPU, no
    /// interrupt/IST return references this stack, and TSS.RSP0 no longer
    /// points into it. An IRQ-safe, architecture-specific quiescence protocol
    /// is required before any real VMM reclamation. Pure host tests only
    /// simulate this obligation.
    pub unsafe fn retire_after_quiescence(&mut self, id: u64) -> Result<(), StackError> {
        let lease = self.slots.iter_mut().flatten()
            .find(|lease| lease.id == id).ok_or(StackError::UnknownLease)?;
        if lease.phase == StackPhase::Retired { return Err(StackError::InvalidPhase); }
        lease.phase = StackPhase::Retired;
        Ok(())
    }

    /// Remove the bookkeeping entry ONLY after the memory owner has actually
    /// unmapped, invalidated and released the pages / guard reservation.
    /// This method does not perform those operations.
    pub fn release_record(&mut self, id: u64) -> Result<(), StackError> {
        let slot = self.slots.iter_mut()
            .find(|item| item.as_ref().is_some_and(|entry| entry.id == id))
            .ok_or(StackError::UnknownLease)?;
        if slot.as_ref().is_none_or(|entry| entry.phase != StackPhase::Retired) {
            return Err(StackError::InvalidPhase);
        }
        *slot = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u64 = KERNEL_HIGH_HALF_START + 0x100000;

    #[test]
    fn stack_guard_layout_and_lifecycle_never_reuse_lease_ids() {
        let mut registry = KernelStackRegistry::new();
        let id = registry.reserve(Tid(1), BASE, 8).unwrap();
        let lease = registry.get(id).unwrap();
        assert_eq!(lease.guard_base, BASE);
        assert_eq!(lease.mapped_base, BASE + PAGE_SIZE);
        assert_eq!(lease.end(), BASE + 9 * PAGE_SIZE);
        assert_eq!(lease.phase, StackPhase::Reserved);
        assert_eq!(registry.release_record(id), Err(StackError::InvalidPhase));
        registry.activate(id).unwrap();
        assert_eq!(registry.activate(id), Err(StackError::InvalidPhase));
        assert_eq!(registry.release_record(id), Err(StackError::InvalidPhase));
        // Simulated host-only quiescence; no machine context is running.
        unsafe { registry.retire_after_quiescence(id).unwrap() };
        registry.release_record(id).unwrap();
        assert_eq!(registry.get(id), None);
        let id2 = registry.reserve(Tid(2), BASE, 8).unwrap();
        assert!(id2 > id);
        assert_eq!(registry.activate(id), Err(StackError::UnknownLease));
    }

    #[test]
    fn overlapping_reservations_and_duplicate_tid_fail_closed() {
        let mut registry = KernelStackRegistry::new();
        registry.reserve(Tid(1), BASE, 8).unwrap();
        assert_eq!(registry.reserve(Tid(1), BASE + 100 * PAGE_SIZE, 2),
            Err(StackError::DuplicateOwner));
        assert_eq!(registry.reserve(Tid(2), BASE + 8 * PAGE_SIZE, 1),
            Err(StackError::Overlap));
        assert_eq!(registry.reserve(Tid(2), BASE + 9 * PAGE_SIZE, 1).is_ok(), true);
    }

    #[test]
    fn reject_kernel_stack_overflow_and_user_addresses() {
        let mut registry = KernelStackRegistry::new();
        assert_eq!(registry.reserve(Tid(0), BASE, 1), Err(StackError::InvalidRange));
        assert_eq!(registry.reserve(Tid(1), 0x800000, 4), Err(StackError::InvalidRange));
        assert_eq!(registry.reserve(Tid(1), BASE + 1, 1), Err(StackError::InvalidRange));
        assert_eq!(registry.reserve(Tid(1), BASE, 0), Err(StackError::InvalidRange));
        assert_eq!(registry.reserve(Tid(1), BASE, MAX_KERNEL_STACK_PAGES + 1),
            Err(StackError::InvalidRange));
        assert_eq!(registry.reserve(Tid(1), u64::MAX - (PAGE_SIZE - 1), 2),
            Err(StackError::InvalidRange));
        assert_eq!(registry.iter().count(), 0);
    }

    #[test]
    fn exhausted_registry_does_not_silently_evict_active_stack() {
        let mut registry = KernelStackRegistry::new();
        for i in 0..MAX_THREADS {
            let id = registry.reserve(Tid(i as u64 + 1), BASE + (i as u64) * 10 * PAGE_SIZE, 8).unwrap();
            registry.activate(id).unwrap();
        }
        assert_eq!(registry.iter().count(), MAX_THREADS);
        assert_eq!(registry.reserve(Tid(100), BASE + 1000 * PAGE_SIZE, 8),
            Err(StackError::Capacity));
    }
}
