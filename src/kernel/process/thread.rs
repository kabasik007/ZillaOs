//! Thread lifecycle, affinity and per-CPU fair-selection *model*.
//!
//! This module is pure no_std metadata. It performs no IRETQ, CR3 switch,
//! preemption, task execution, AP startup or hardware interrupt delivery.
//! Only the BSP CPU0 is marked online in a new ThreadTable.
use super::{
    Pid, ProcessState, ProcessTable,
    address_space::{Access, USER_MIN, USER_TOP},
};

pub const MAX_THREADS: usize = 64;
pub const MAX_CPUS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tid(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadState {
    Created,
    Ready,
    Running,
    Blocked,
    Exited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadError {
    ParentMissing,
    InvalidContext,
    InvalidAffinity,
    NoFreeSlot,
    IdExhausted,
    UnknownThread,
    InvalidState,
    CpuOffline,
    CpuBusy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Thread {
    pub tid: Tid,
    pub pid: Pid,
    pub state: ThreadState,
    pub instruction: u64,
    pub stack_top: u64,
    pub affinity_mask: u64,
    /// Bookkeeping only. No actual CPU ownership or context switch.
    pub assigned_cpu: Option<usize>,
}

pub struct ThreadTable {
    slots: [Option<Thread>; MAX_THREADS],
    next_tid: u64,
    online_cpu_mask: u64,
    last_scheduled: [u64; MAX_CPUS],
}

impl Default for ThreadTable {
    fn default() -> Self { Self::new() }
}

impl ThreadTable {
    pub const fn new() -> Self {
        Self {
            slots: [None; MAX_THREADS],
            next_tid: 1,
            online_cpu_mask: 1, // BSP-only until real AP bring-up
            last_scheduled: [0; MAX_CPUS],
        }
    }

    pub const fn online_cpu_mask(&self) -> u64 { self.online_cpu_mask }

    pub fn iter(&self) -> impl Iterator<Item = &Thread> + '_ {
        self.slots.iter().flatten()
    }

    pub fn get(&self, tid: Tid) -> Option<&Thread> {
        self.iter().find(|t| t.tid == tid)
    }

    fn get_mut(&mut self, tid: Tid) -> Result<&mut Thread, ThreadError> {
        self.slots.iter_mut().flatten()
            .find(|t| t.tid == tid).ok_or(ThreadError::UnknownThread)
    }

    fn cpu_allowed(&self, cpu: usize, mask: u64) -> bool {
        cpu < MAX_CPUS
            && (self.online_cpu_mask & (1u64 << cpu)) != 0
            && (mask & (1u64 << cpu)) != 0
    }

    pub fn create(
        &mut self,
        processes: &ProcessTable,
        pid: Pid,
        entry: u64,
        stack_top: u64,
        affinity_mask: u64,
    ) -> Result<Tid, ThreadError> {
        let Some(parent) = processes.process(pid) else {
            return Err(ThreadError::ParentMissing);
        };
        if parent.state == ProcessState::Exited { return Err(ThreadError::ParentMissing); }
        if !(USER_MIN..USER_TOP).contains(&entry)
            || !(USER_MIN..USER_TOP).contains(&stack_top)
        {
            return Err(ThreadError::InvalidContext);
        }
        if affinity_mask == 0 || affinity_mask & self.online_cpu_mask == 0
            || affinity_mask >> MAX_CPUS != 0
        {
            return Err(ThreadError::InvalidAffinity);
        }
        let index = self.slots.iter().position(Option::is_none)
            .ok_or(ThreadError::NoFreeSlot)?;
        let new_next = self.next_tid.checked_add(1).ok_or(ThreadError::IdExhausted)?;
        let tid = Tid(self.next_tid);
        self.next_tid = new_next;
        self.slots[index] = Some(Thread {
            tid, pid, state: ThreadState::Created,
            instruction: entry, stack_top, affinity_mask,
            assigned_cpu: None,
        });
        Ok(tid)
    }

    /// Validate the parent's planned RX entry and RW user stack.
    /// Does NOT map pages or initialize the CPU register context.
    pub fn prepare(&mut self, processes: &ProcessTable, tid: Tid)
        -> Result<(), ThreadError>
    {
        let record = *self.get(tid).ok_or(ThreadError::UnknownThread)?;
        if record.state != ThreadState::Created { return Err(ThreadError::InvalidState) }
        let parent = processes.process(record.pid).ok_or(ThreadError::ParentMissing)?;
        if parent.state == ProcessState::Exited
            || !parent.address_space.allows(record.instruction, 1, Access::Execute)
            || !parent.address_space.allows(record.stack_top - 1, 1, Access::Write)
        {
            return Err(ThreadError::InvalidContext);
        }
        self.get_mut(tid)?.state = ThreadState::Ready;
        Ok(())
    }

    /// Non-mutating priority-free Round Robin selection for one *online*
    /// logical CPU. Mutates only the selection cursor; it does not run tasks.
    pub fn next_ready(&mut self, cpu: usize) -> Option<Tid> {
        if !self.cpu_allowed(cpu, 1u64 << cpu.min(63)) { return None; }
        let last = self.last_scheduled[cpu];
        let mut first: Option<Tid> = None;
        let mut after: Option<Tid> = None;
        for thread in self.iter().filter(|t|
            t.state == ThreadState::Ready && t.affinity_mask & (1 << cpu) != 0
        ) {
            let id = thread.tid;
            if first.is_none_or(|prev| id < prev) { first = Some(id); }
            if id.0 > last && after.is_none_or(|prev| id < prev) { after = Some(id); }
        }
        let chosen = after.or(first)?;
        self.last_scheduled[cpu] = chosen.0;
        Some(chosen)
    }

    pub fn mark_running(&mut self, cpu: usize, tid: Tid) -> Result<(), ThreadError> {
        let record = *self.get(tid).ok_or(ThreadError::UnknownThread)?;
        if !self.cpu_allowed(cpu, record.affinity_mask) {
            return Err(ThreadError::CpuOffline);
        }
        if record.state != ThreadState::Ready { return Err(ThreadError::InvalidState) }
        if self.iter().any(|t| t.state == ThreadState::Running && t.assigned_cpu == Some(cpu)) {
            return Err(ThreadError::CpuBusy);
        }
        let t = self.get_mut(tid)?;
        t.state = ThreadState::Running;
        t.assigned_cpu = Some(cpu);
        Ok(())
    }

    /// Mark a CPU0 timer-preempted task ready after saving its full trap
    /// frame. This only changes metadata; the IRQ backend owns the register
    /// switch and interrupt acknowledgement.
    pub fn preempt_running(&mut self, cpu: usize, tid: Tid) -> Result<(), ThreadError> {
        let thread = *self.get(tid).ok_or(ThreadError::UnknownThread)?;
        if cpu >= MAX_CPUS || thread.state != ThreadState::Running
            || thread.assigned_cpu != Some(cpu)
        {
            return Err(ThreadError::InvalidState);
        }
        let thread = self.get_mut(tid)?;
        thread.state = ThreadState::Ready;
        thread.assigned_cpu = None;
        Ok(())
    }

    pub fn yield_running(&mut self, tid: Tid) -> Result<(), ThreadError> {
        let t = self.get_mut(tid)?;
        if t.state != ThreadState::Running { return Err(ThreadError::InvalidState); }
        t.state = ThreadState::Ready;
        t.assigned_cpu = None;
        Ok(())
    }

    pub fn block(&mut self, tid: Tid) -> Result<(), ThreadError> {
        let t = self.get_mut(tid)?;
        if !matches!(t.state, ThreadState::Ready | ThreadState::Running) {
            return Err(ThreadError::InvalidState);
        }
        t.state = ThreadState::Blocked;
        t.assigned_cpu = None;
        Ok(())
    }

    pub fn wake(&mut self, tid: Tid) -> Result<(), ThreadError> {
        let t = self.get_mut(tid)?;
        if t.state != ThreadState::Blocked { return Err(ThreadError::InvalidState); }
        t.state = ThreadState::Ready;
        Ok(())
    }

    pub fn exit(&mut self, tid: Tid) -> Result<(), ThreadError> {
        let t = self.get_mut(tid)?;
        if t.state == ThreadState::Exited { return Err(ThreadError::InvalidState); }
        t.state = ThreadState::Exited;
        t.assigned_cpu = None;
        Ok(())
    }

    pub fn reap(&mut self, tid: Tid) -> Result<(), ThreadError> {
        let slot = self.slots.iter_mut()
            .find(|slot| slot.as_ref().is_some_and(|t| t.tid == tid))
            .ok_or(ThreadError::UnknownThread)?;
        if slot.as_ref().is_none_or(|t| t.state != ThreadState::Exited) {
            return Err(ThreadError::InvalidState);
        }
        *slot = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::address_space::{Permissions, UserRegion, PAGE_SIZE};

    fn parent() -> (ProcessTable, Pid) {
        let mut processes=ProcessTable::new();
        let pid=processes.create("thread-fixture", 0x400000, 0x800000, 4).unwrap();
        processes.map_region(pid, UserRegion::checked(
            0x400000, PAGE_SIZE, Permissions::READ_EXECUTE,
        ).unwrap()).unwrap();
        processes.prepare(pid).unwrap();
        (processes,pid)
    }

    #[test]
    fn lifecycle_and_parent_validation_are_explicit() {
        let (processes,pid)=parent();
        let mut table=ThreadTable::new();
        assert_eq!(table.online_cpu_mask(),1);
        assert_eq!(table.create(&processes,Pid(999),0x400000,0x800000,1),
                   Err(ThreadError::ParentMissing));
        assert_eq!(table.create(&processes,pid,0x400000,0x800000,2),
                   Err(ThreadError::InvalidAffinity));
        let tid=table.create(&processes,pid,0x400000,0x800000,1).unwrap();
        assert_eq!(table.prepare(&processes,tid),Ok(()));
        assert_eq!(table.prepare(&processes,tid),Err(ThreadError::InvalidState));
        assert_eq!(table.next_ready(1),None);
        assert_eq!(table.next_ready(0),Some(tid));
        assert_eq!(table.mark_running(1,tid),Err(ThreadError::CpuOffline));
        table.mark_running(0,tid).unwrap();
        assert_eq!(table.get(tid).unwrap().assigned_cpu,Some(0));
        table.block(tid).unwrap();
        assert_eq!(table.next_ready(0),None);
        table.wake(tid).unwrap();
        table.mark_running(0,tid).unwrap();
        table.yield_running(tid).unwrap();
        table.exit(tid).unwrap();
        assert_eq!(table.reap(tid),Ok(()));
        assert_eq!(table.get(tid),None);
    }

    #[test]
    fn preemption_requires_current_cpu_ownership() {
        let (processes, pid) = parent();
        let mut threads = ThreadTable::new();
        let tid = threads.create(&processes,pid,0x400000,0x800000,1).unwrap();
        threads.prepare(&processes,tid).unwrap();
        assert_eq!(threads.preempt_running(0,tid),Err(ThreadError::InvalidState));
        threads.mark_running(0,tid).unwrap();
        assert_eq!(threads.preempt_running(1,tid),Err(ThreadError::InvalidState));
        assert_eq!(threads.preempt_running(0,tid),Ok(()));
        assert_eq!(threads.get(tid).unwrap().state,ThreadState::Ready);
        assert_eq!(threads.get(tid).unwrap().assigned_cpu,None);
        threads.mark_running(0,tid).unwrap();
        assert_eq!(threads.get(tid).unwrap().assigned_cpu,Some(0));
    }

    #[test]
    fn round_robin_never_schedules_blocked_or_exited_threads() {
        let (processes,pid)=parent();
        let mut t=ThreadTable::new();
        let ids: [_;3]=core::array::from_fn(|_| {
            let tid=t.create(&processes,pid,0x400000,0x800000,1).unwrap();
            t.prepare(&processes,tid).unwrap();
            tid
        });
        assert_eq!(t.next_ready(0),Some(ids[0]));
        assert_eq!(t.next_ready(0),Some(ids[1]));
        t.block(ids[2]).unwrap();
        assert_eq!(t.next_ready(0),Some(ids[0]));
        t.mark_running(0,ids[0]).unwrap();
        assert_eq!(t.mark_running(0,ids[1]),Err(ThreadError::CpuBusy));
        t.exit(ids[1]).unwrap();
        assert_eq!(t.next_ready(0),None);
    }

    #[test]
    fn cannot_prepare_unmapped_or_kernel_context() {
        let (processes,pid)=parent();
        let mut t=ThreadTable::new();
        assert_eq!(t.create(&processes,pid,u64::MAX,0x800000,1),
                   Err(ThreadError::InvalidContext));
        let tid=t.create(&processes,pid,0x500000,0x800000,1).unwrap();
        assert_eq!(t.prepare(&processes,tid),Err(ThreadError::InvalidContext));
    }

    #[test]
    fn bounded_capacity_does_not_reuse_running_tids() {
        let (processes,pid)=parent();
        let mut t=ThreadTable::new();
        let mut first=None;
        for _ in 0..MAX_THREADS {
            let tid=t.create(&processes,pid,0x400000,0x800000,1).unwrap();
            if first.is_none() { first=Some(tid); }
        }
        assert_eq!(t.create(&processes,pid,0x400000,0x800000,1),
                   Err(ThreadError::NoFreeSlot));
        t.exit(first.unwrap()).unwrap();
        t.reap(first.unwrap()).unwrap();
        let new_tid=t.create(&processes,pid,0x400000,0x800000,1).unwrap();
        assert!(new_tid > first.unwrap());
    }
}
