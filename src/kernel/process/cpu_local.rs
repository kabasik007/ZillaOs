//! CPU-local scheduling and preemption bookkeeping (R0).
//!
//! This is a pure no_std STATE MODEL, not a running scheduler, IRQ handler,
//! SMP implementation, CR3 switch or privilege boundary.
//! The authoritative TID lifecycle remains in ThreadTable. A trusted trap
//! backend must save the FULL CPU context before descheduling a running TID.

use super::thread::{MAX_CPUS, ThreadError, ThreadState, ThreadTable, Tid};
use super::address_space::{USER_MIN, USER_TOP};
use super::{Pid, ProcessState, ProcessTable};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleError {
    CpuOffline,
    CpuBusy,
    NoCurrent,
    Desynchronized,
    ParentNotRunnable,
    WrongContext,
    NotInInterrupt,
    InterruptUnderflow,
    InterruptOverflow,
    PreemptUnderflow,
    PreemptOverflow,
    SwitchUnsafe,
    NoReschedulePending,
    Thread(ThreadError),
}

impl From<ThreadError> for ScheduleError {
    fn from(value: ThreadError) -> Self { Self::Thread(value) }
}

/// An acknowledgement from a TRUSTED, architecture-specific trap path.
///
/// # Safety
/// Only the Ring0 trap/context-switch backend may call confirmed(), after
/// saving ALL registers required by the ABI, including flags, selectors,
/// interrupt frame and FPU/SIMD state where applicable, on a valid kernel
/// stack. This marker does not itself save registers, mask IRQs or lock memory.
/// It must never be constructed from an untrusted syscall argument.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrapFrameError {
    InvalidCpu,
    InvalidTid,
    InvalidPrivilege,
    InvalidInstruction,
    InvalidStack,
    InvalidFlags,
}

/// Diagnostic policy validation of a saved CPL3 interrupt return frame.
/// This does NOT validate actual GDT selectors, PTEs or ownership of RIP/RSP.
/// The trusted trap backend must additionally check mappings and save ALL
/// registers/FPU state before acknowledging a frame as reusable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserTrapFrame {
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
    pub cs: u64,
    pub ss: u64,
}

impl UserTrapFrame {
    pub fn validate(self) -> Result<(), TrapFrameError> {
        if self.cs == 0 || self.ss == 0 || (self.cs & 3) != 3 || (self.ss & 3) != 3 {
            return Err(TrapFrameError::InvalidPrivilege);
        }
        if !(USER_MIN..USER_TOP).contains(&self.rip) {
            return Err(TrapFrameError::InvalidInstruction);
        }
        if self.rsp <= USER_MIN || self.rsp > USER_TOP {
            return Err(TrapFrameError::InvalidStack);
        }
        // RFLAGS bit1 must be set. Reject IOPL, NT and VM on return to CPL3.
        const UNSAFE_FLAGS: u64 = (3 << 12) | (1 << 14) | (1 << 17);
        if self.rflags & 0x2 == 0 || self.rflags & UNSAFE_FLAGS != 0 {
            return Err(TrapFrameError::InvalidFlags);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavedTrapContext {
    tid: Tid,
    /// The legacy opt-in host-test marker lacks an architectural CPU claim.
    /// Production trap entry must use confirmed_user_frame with an actual CPU.
    cpu: Option<usize>,
}

impl SavedTrapContext {
    /// Legacy model-only acknowledgment. Never wire to untrusted syscalls.
    pub unsafe fn confirmed(tid: Tid) -> Self { Self { tid, cpu: None } }

    /// # Safety
    /// Only a trusted Ring0 trap backend may call this, *after* verifying
    /// actual CS/SS/GDT, user PTE rights, saved machine/FPU state, the
    /// interrupted TID and current CPU ID. Passing arbitrary user data or
    /// omitting context save violates this API's safety preconditions.
    pub unsafe fn confirmed_user_frame(
        tid: Tid, cpu: usize, frame: UserTrapFrame,
    ) -> Result<Self, TrapFrameError> {
        if cpu >= MAX_CPUS { return Err(TrapFrameError::InvalidCpu); }
        if tid.0 == 0 { return Err(TrapFrameError::InvalidTid); }
        frame.validate()?;
        Ok(Self { tid, cpu: Some(cpu) })
    }
}

/// Trusted scheduler-derived identity for the CURRENT running thread.
/// This struct is returned only after verifying CPU-local state, TID owner,
/// CPU assignment, and parent PID state. No user buffer is consulted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurrentTaskIdentity {
    cpu: usize,
    pid: Pid,
    tid: Tid,
}
impl CurrentTaskIdentity {
    pub const fn cpu(self) -> usize { self.cpu }
    pub const fn pid(self) -> Pid { self.pid }
    pub const fn tid(self) -> Tid { self.tid }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preemption {
    Continue,
    Deferred,
    SwitchRequested(Tid),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuLocalState {
    current: Option<Tid>,
    irq_depth: u16,
    preempt_disabled: u16,
    need_resched: bool,
    timer_ticks: u64,
}

impl CpuLocalState {
    pub const fn new() -> Self {
        Self {
            current: None,
            irq_depth: 0,
            preempt_disabled: 0,
            need_resched: false,
            timer_ticks: 0,
        }
    }

    pub const fn current(&self) -> Option<Tid> { self.current }
    pub const fn irq_depth(&self) -> u16 { self.irq_depth }
    pub const fn preempt_disabled(&self) -> u16 { self.preempt_disabled }
    pub const fn need_resched(&self) -> bool { self.need_resched }
    pub const fn timer_ticks(&self) -> u64 { self.timer_ticks }

    fn decision(&self) -> Preemption {
        if !self.need_resched { return Preemption::Continue; }
        if self.irq_depth != 0 || self.preempt_disabled != 0 {
            return Preemption::Deferred;
        }
        match self.current {
            Some(tid) => Preemption::SwitchRequested(tid),
            None => Preemption::Continue,
        }
    }
}

/// One slot per logical CPU; ONLY CPU0 is online in ThreadTable::new().
/// Not a lock, and not safe for concurrent AP use. A future SMP backend must
/// provide actual CPU-local storage plus IRQ-safe synchronization.
pub struct CpuSchedulerModel {
    cpus: [CpuLocalState; MAX_CPUS],
}

impl Default for CpuSchedulerModel {
    fn default() -> Self { Self::new() }
}

impl CpuSchedulerModel {
    pub const fn new() -> Self {
        Self { cpus: [CpuLocalState::new(); MAX_CPUS] }
    }

    pub fn snapshot(&self, cpu: usize) -> Option<CpuLocalState> {
        self.cpus.get(cpu).copied()
    }

    /// Future IPC sender identity must originate here (or an equivalent
    /// authenticated trap-current accessor), not from a claimed sender PID.
    /// The CPU index must come from a trusted arch-specific current-CPU
    /// source, never from userspace. This is still a host-only model.
    pub fn current_identity(
        &self, threads: &ThreadTable, processes: &ProcessTable, cpu: usize,
    ) -> Result<CurrentTaskIdentity, ScheduleError> {
        if cpu >= MAX_CPUS || (threads.online_cpu_mask() & (1u64 << cpu)) == 0 {
            return Err(ScheduleError::CpuOffline);
        }
        let state = &self.cpus[cpu];
        let tid = Self::verify_current(state, threads, cpu)?;
        let thread = threads.get(tid).ok_or(ScheduleError::Desynchronized)?;
        if !processes.process(thread.pid).is_some_and(|parent|
            matches!(parent.state, ProcessState::Ready | ProcessState::Running))
        {
            return Err(ScheduleError::ParentNotRunnable);
        }
        Ok(CurrentTaskIdentity { cpu, pid: thread.pid, tid })
    }

    fn checked<'a>(
        &'a mut self, threads: &ThreadTable, cpu: usize,
    ) -> Result<&'a mut CpuLocalState, ScheduleError> {
        if cpu >= MAX_CPUS || (threads.online_cpu_mask() & (1u64 << cpu)) == 0 {
            return Err(ScheduleError::CpuOffline);
        }
        Ok(&mut self.cpus[cpu])
    }

    fn verify_current(
        state: &CpuLocalState, threads: &ThreadTable, cpu: usize,
    ) -> Result<Tid, ScheduleError> {
        let tid = state.current.ok_or(ScheduleError::NoCurrent)?;
        let t = threads.get(tid).ok_or(ScheduleError::Desynchronized)?;
        if t.state != ThreadState::Running || t.assigned_cpu != Some(cpu) {
            return Err(ScheduleError::Desynchronized);
        }
        Ok(tid)
    }

    fn verify_saved(
        state: &CpuLocalState, threads: &ThreadTable, cpu: usize,
        saved: SavedTrapContext,
    ) -> Result<Tid, ScheduleError> {
        let current = Self::verify_current(state, threads, cpu)?;
        if current != saved.tid { return Err(ScheduleError::WrongContext); }
        if saved.cpu.is_some_and(|owner| owner != cpu) {
            return Err(ScheduleError::WrongContext);
        }
        if state.irq_depth != 0 || state.preempt_disabled != 0 {
            return Err(ScheduleError::SwitchUnsafe);
        }
        Ok(current)
    }

    /// Select a Ready TID using ThreadTable's deterministic Round Robin.
    /// Metadata only: the caller must install the real context separately.
    pub fn dispatch_next(
        &mut self, threads: &mut ThreadTable, cpu: usize,
    ) -> Result<Option<Tid>, ScheduleError> {
        let state = self.checked(threads, cpu)?;
        if state.current.is_some() { return Err(ScheduleError::CpuBusy); }
        if state.irq_depth != 0 || state.preempt_disabled != 0 {
            return Err(ScheduleError::SwitchUnsafe);
        }
        if threads.iter().any(|t| {
            t.state == ThreadState::Running && t.assigned_cpu == Some(cpu)
        }) {
            return Err(ScheduleError::Desynchronized);
        }
        let Some(tid) = threads.next_ready(cpu) else { return Ok(None); };
        threads.mark_running(cpu, tid)?;
        state.current = Some(tid);
        state.need_resched = false;
        Ok(Some(tid))
    }

    pub fn enter_interrupt(
        &mut self, threads: &ThreadTable, cpu: usize,
    ) -> Result<(), ScheduleError> {
        let state = self.checked(threads, cpu)?;
        state.irq_depth = state.irq_depth.checked_add(1)
            .ok_or(ScheduleError::InterruptOverflow)?;
        Ok(())
    }

    /// A timer IRQ only requests work; it never switches stacks/registers.
    pub fn timer_tick(
        &mut self, threads: &ThreadTable, cpu: usize,
    ) -> Result<(), ScheduleError> {
        let state = self.checked(threads, cpu)?;
        if state.irq_depth == 0 { return Err(ScheduleError::NotInInterrupt); }
        state.timer_ticks = state.timer_ticks.saturating_add(1);
        if state.current.is_some() { state.need_resched = true; }
        Ok(())
    }

    pub fn leave_interrupt(
        &mut self, threads: &ThreadTable, cpu: usize,
    ) -> Result<Preemption, ScheduleError> {
        let state = self.checked(threads, cpu)?;
        if state.irq_depth == 0 { return Err(ScheduleError::InterruptUnderflow); }
        state.irq_depth -= 1;
        Ok(state.decision())
    }

    pub fn disable_preemption(
        &mut self, threads: &ThreadTable, cpu: usize,
    ) -> Result<(), ScheduleError> {
        let state = self.checked(threads, cpu)?;
        state.preempt_disabled = state.preempt_disabled.checked_add(1)
            .ok_or(ScheduleError::PreemptOverflow)?;
        Ok(())
    }

    pub fn enable_preemption(
        &mut self, threads: &ThreadTable, cpu: usize,
    ) -> Result<Preemption, ScheduleError> {
        let state = self.checked(threads, cpu)?;
        if state.preempt_disabled == 0 { return Err(ScheduleError::PreemptUnderflow); }
        state.preempt_disabled -= 1;
        Ok(state.decision())
    }

    /// Requires a trusted saved trap context for the CURRENT TID. This is
    /// NOT invoked from the default Desktop or timer fixture.
    pub fn commit_saved_preemption(
        &mut self, threads: &mut ThreadTable, cpu: usize,
        saved: SavedTrapContext,
    ) -> Result<Tid, ScheduleError> {
        let state = self.checked(threads, cpu)?;
        let tid = Self::verify_saved(state, threads, cpu, saved)?;
        if !state.need_resched { return Err(ScheduleError::NoReschedulePending); }
        threads.preempt_running(cpu, tid)?;
        state.current = None;
        state.need_resched = false;
        Ok(tid)
    }

    pub fn yield_saved(
        &mut self, threads: &mut ThreadTable, cpu: usize,
        saved: SavedTrapContext,
    ) -> Result<Tid, ScheduleError> {
        let state = self.checked(threads, cpu)?;
        let tid = Self::verify_saved(state, threads, cpu, saved)?;
        threads.yield_running(tid)?;
        state.current = None;
        state.need_resched = false;
        Ok(tid)
    }

    pub fn block_saved(
        &mut self, threads: &mut ThreadTable, cpu: usize,
        saved: SavedTrapContext,
    ) -> Result<Tid, ScheduleError> {
        let state = self.checked(threads, cpu)?;
        let tid = Self::verify_saved(state, threads, cpu, saved)?;
        threads.block(tid)?;
        state.current = None;
        state.need_resched = false;
        Ok(tid)
    }

    pub fn exit_saved(
        &mut self, threads: &mut ThreadTable, cpu: usize,
        saved: SavedTrapContext,
    ) -> Result<Tid, ScheduleError> {
        let state = self.checked(threads, cpu)?;
        let tid = Self::verify_saved(state, threads, cpu, saved)?;
        threads.exit(tid)?;
        state.current = None;
        state.need_resched = false;
        Ok(tid)
    }

    pub fn wake_blocked(
        &mut self, threads: &mut ThreadTable, tid: Tid,
    ) -> Result<(), ScheduleError> {
        threads.wake(tid)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::address_space::{Permissions, UserRegion, PAGE_SIZE};
    use crate::process::{ProcessTable, DEFAULT_STACK_PAGES};

    fn fixture() -> (ThreadTable, [Tid; 3]) {
        let mut processes = ProcessTable::new();
        let pid = processes.create(
            "scheduler-fixture", 0x400000, 0x800000, DEFAULT_STACK_PAGES,
        ).unwrap();
        processes.map_region(pid, UserRegion::checked(
            0x400000, PAGE_SIZE, Permissions::READ_EXECUTE,
        ).unwrap()).unwrap();
        processes.prepare(pid).unwrap();

        let mut threads = ThreadTable::new();
        let ids: [Tid; 3] = core::array::from_fn(|_| {
            let tid = threads.create(&processes, pid, 0x400000, 0x800000, 1).unwrap();
            threads.prepare(&processes, tid).unwrap();
            tid
        });
        (threads, ids)
    }

    // Host tests do not execute CPU instructions. They merely simulate
    // an already-saved trap frame to verify metadata transitions.
    fn saved(tid: Tid) -> SavedTrapContext {
        unsafe { SavedTrapContext::confirmed(tid) }
    }

    #[test]
    fn deterministic_fair_round_robin_after_saved_timer_preemption() {
        let (mut threads, ids) = fixture();
        let mut scheduler = CpuSchedulerModel::new();
        for id in ids.iter().copied().cycle().take(9) {
            assert_eq!(scheduler.dispatch_next(&mut threads, 0), Ok(Some(id)));
            scheduler.enter_interrupt(&threads, 0).unwrap();
            scheduler.timer_tick(&threads, 0).unwrap();
            assert_eq!(scheduler.leave_interrupt(&threads, 0),
                Ok(Preemption::SwitchRequested(id)));
            assert_eq!(scheduler.commit_saved_preemption(&mut threads, 0, saved(id)), Ok(id));
        }
        assert_eq!(scheduler.snapshot(0).unwrap().timer_ticks(), 9);
        assert_eq!(scheduler.snapshot(0).unwrap().current(), None);
    }

    #[test]
    fn nested_irq_and_preemption_disable_defer_without_context_mutation() {
        let (mut threads, ids) = fixture();
        let mut scheduler = CpuSchedulerModel::new();
        scheduler.dispatch_next(&mut threads, 0).unwrap();
        scheduler.disable_preemption(&threads, 0).unwrap();
        scheduler.enter_interrupt(&threads, 0).unwrap();
        scheduler.enter_interrupt(&threads, 0).unwrap();
        scheduler.timer_tick(&threads, 0).unwrap();
        assert_eq!(scheduler.leave_interrupt(&threads, 0), Ok(Preemption::Deferred));
        assert_eq!(scheduler.leave_interrupt(&threads, 0), Ok(Preemption::Deferred));
        assert_eq!(scheduler.commit_saved_preemption(&mut threads, 0, saved(ids[0])),
            Err(ScheduleError::SwitchUnsafe));
        assert_eq!(threads.get(ids[0]).unwrap().state, ThreadState::Running);
        assert_eq!(scheduler.enable_preemption(&threads, 0),
            Ok(Preemption::SwitchRequested(ids[0])));
        scheduler.commit_saved_preemption(&mut threads, 0, saved(ids[0])).unwrap();
        assert_eq!(threads.get(ids[0]).unwrap().state, ThreadState::Ready);
    }

    #[test]
    fn wrong_or_unacknowledged_context_never_deschedules_thread() {
        let (mut threads, ids) = fixture();
        let mut scheduler = CpuSchedulerModel::new();
        scheduler.dispatch_next(&mut threads, 0).unwrap();
        assert_eq!(scheduler.commit_saved_preemption(&mut threads, 0, saved(ids[0])),
            Err(ScheduleError::NoReschedulePending));
        scheduler.enter_interrupt(&threads, 0).unwrap();
        scheduler.timer_tick(&threads, 0).unwrap();
        scheduler.leave_interrupt(&threads, 0).unwrap();
        assert_eq!(scheduler.commit_saved_preemption(&mut threads, 0, saved(ids[1])),
            Err(ScheduleError::WrongContext));
        assert_eq!(threads.get(ids[0]).unwrap().state, ThreadState::Running);
        assert_eq!(scheduler.snapshot(0).unwrap().current(), Some(ids[0]));
    }

    #[test]
    fn block_wake_exit_and_idle_preserve_thread_states() {
        let (mut threads, ids) = fixture();
        let mut scheduler = CpuSchedulerModel::new();
        scheduler.dispatch_next(&mut threads, 0).unwrap();
        assert_eq!(scheduler.block_saved(&mut threads, 0, saved(ids[0])), Ok(ids[0]));
        assert_eq!(threads.get(ids[0]).unwrap().state, ThreadState::Blocked);
        assert_eq!(scheduler.wake_blocked(&mut threads, ids[0]), Ok(()));
        assert_eq!(scheduler.wake_blocked(&mut threads, ids[0]),
            Err(ScheduleError::Thread(ThreadError::InvalidState)));
        assert_eq!(scheduler.dispatch_next(&mut threads, 0), Ok(Some(ids[1])));
        assert_eq!(scheduler.exit_saved(&mut threads, 0, saved(ids[1])), Ok(ids[1]));
        assert_eq!(threads.get(ids[1]).unwrap().state, ThreadState::Exited);
        assert_eq!(scheduler.dispatch_next(&mut threads, 0), Ok(Some(ids[2])));
        scheduler.yield_saved(&mut threads, 0, saved(ids[2])).unwrap();
        assert_eq!(scheduler.dispatch_next(&mut threads, 0), Ok(Some(ids[0])));
    }

    #[test]
    fn offline_cpu_invalid_irq_and_desync_fail_closed() {
        let (mut threads, ids) = fixture();
        let mut scheduler = CpuSchedulerModel::new();
        assert_eq!(scheduler.dispatch_next(&mut threads, 1), Err(ScheduleError::CpuOffline));
        assert_eq!(scheduler.leave_interrupt(&threads, 0),
            Err(ScheduleError::InterruptUnderflow));
        assert_eq!(scheduler.enable_preemption(&threads, 0),
            Err(ScheduleError::PreemptUnderflow));
        assert_eq!(scheduler.timer_tick(&threads, 0), Err(ScheduleError::NotInInterrupt));
        threads.mark_running(0, ids[0]).unwrap(); // external mutation, no CPU state
        assert_eq!(scheduler.dispatch_next(&mut threads, 0),
            Err(ScheduleError::Desynchronized));
    }

    #[test]
    fn pending_tick_never_switches_while_irq_is_nested() {
        let (mut threads, ids) = fixture();
        let mut scheduler = CpuSchedulerModel::new();
        scheduler.dispatch_next(&mut threads, 0).unwrap();
        scheduler.enter_interrupt(&threads, 0).unwrap();
        scheduler.timer_tick(&threads, 0).unwrap();
        assert_eq!(scheduler.commit_saved_preemption(&mut threads, 0, saved(ids[0])),
            Err(ScheduleError::SwitchUnsafe));
        assert_eq!(scheduler.leave_interrupt(&threads, 0),
            Ok(Preemption::SwitchRequested(ids[0])));
    }
    #[test]
    fn invalid_saved_user_frame_rejected_before_descheduling() {
        let good = UserTrapFrame {
            rip: 0x400000, rsp: 0x800000,
            rflags: 0x202, cs: 0x23, ss: 0x1b,
        };
        assert_eq!(good.validate(), Ok(()));
        assert_eq!(UserTrapFrame { cs: 0x8, ..good }.validate(),
            Err(TrapFrameError::InvalidPrivilege));
        assert_eq!(UserTrapFrame { rip: USER_TOP, ..good }.validate(),
            Err(TrapFrameError::InvalidInstruction));
        assert_eq!(UserTrapFrame { rsp: 0, ..good }.validate(),
            Err(TrapFrameError::InvalidStack));
        assert_eq!(UserTrapFrame { rflags: 0x3202, ..good }.validate(),
            Err(TrapFrameError::InvalidFlags));
        assert_eq!(unsafe { SavedTrapContext::confirmed_user_frame(Tid(0), 0, good) },
            Err(TrapFrameError::InvalidTid));
        assert_eq!(unsafe { SavedTrapContext::confirmed_user_frame(Tid(1), MAX_CPUS, good) },
            Err(TrapFrameError::InvalidCpu));

        let (mut threads, ids) = fixture();
        let mut scheduler = CpuSchedulerModel::new();
        scheduler.dispatch_next(&mut threads, 0).unwrap();
        let legitimate = unsafe { SavedTrapContext::confirmed_user_frame(
            ids[0], 0, good,
        ).unwrap() };
        assert_eq!(scheduler.yield_saved(&mut threads, 1, legitimate),
            Err(ScheduleError::CpuOffline));
        assert_eq!(scheduler.yield_saved(&mut threads, 0, legitimate), Ok(ids[0]));
    }

    #[test]
    fn current_identity_is_derived_from_authenticated_cpu_thread_parent() {
        use crate::process::address_space::{Permissions, UserRegion, PAGE_SIZE};
        let mut parents = ProcessTable::new();
        let pid = parents.create("identity", 0x400000, 0x800000, 4).unwrap();
        parents.map_region(pid, UserRegion::checked(
            0x400000, PAGE_SIZE, Permissions::READ_EXECUTE,
        ).unwrap()).unwrap();
        parents.prepare(pid).unwrap();
        let mut threads = ThreadTable::new();
        let tid = threads.create(&parents, pid, 0x400000, 0x800000, 1).unwrap();
        threads.prepare(&parents, tid).unwrap();
        let mut cpu = CpuSchedulerModel::new();
        assert_eq!(cpu.current_identity(&threads, &parents, 0), Err(ScheduleError::NoCurrent));
        assert_eq!(cpu.current_identity(&threads, &parents, 1), Err(ScheduleError::CpuOffline));
        cpu.dispatch_next(&mut threads, 0).unwrap();
        let trusted = cpu.current_identity(&threads, &parents, 0).unwrap();
        assert_eq!((trusted.pid(), trusted.tid(), trusted.cpu()), (pid, tid, 0));
        parents.kill(pid).unwrap();
        assert_eq!(cpu.current_identity(&threads, &parents, 0),
            Err(ScheduleError::ParentNotRunnable));
        parents.reap(pid).unwrap();
        assert_eq!(cpu.current_identity(&threads, &parents, 0),
            Err(ScheduleError::ParentNotRunnable));
        // Raw ThreadTable mutation cannot trick the CPU-local identity.
        threads.yield_running(tid).unwrap();
        assert_eq!(cpu.current_identity(&threads, &parents, 0),
            Err(ScheduleError::Desynchronized));
    }

}
