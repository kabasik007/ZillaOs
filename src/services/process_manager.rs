//! Single service-facing gateway for future standalone processes.
//! Unlike the existing built-in AppManager, this does NOT execute a CPU task.
use crate::process::address_space::UserRegion;
use crate::process::scheduler::RoundRobin;
use crate::process::cpu_local::{CpuLocalState, CpuSchedulerModel, SavedTrapContext, ScheduleError};
use crate::process::kernel_stack::{KernelStackLease, KernelStackRegistry, StackError};
use crate::process::lifecycle::{
    CleanupBackend, CleanupError, CleanupHookError, CleanupPhase, CleanupWork,
    ExitCoordinator, ExitNotification,
};
use crate::process::{ExitReason, Pid, Process, ProcessError, ProcessState, ProcessTable};
use crate::process::thread::{Thread, ThreadError, ThreadState, ThreadTable, Tid, MAX_CPUS};
use super::task_snapshot::TaskSnapshot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionError {
    IsolationUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReapModelError {
    ThreadsRemain,
    CleanupPending,
    Process(ProcessError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitModelError {
    UnknownPid,
    AlreadyExited,
    RunningThread,
    CpuStillActive,
    ThreadsRemain,
    StacksRemain,
    CleanupIncomplete,
    Process(ProcessError),
    Thread(ThreadError),
    Journal(CleanupError),
    Hook(CleanupHookError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackModelError {
    UnknownTid,
    ThreadExited,
    Stack(StackError),
}

/// Owns metadata and queue selection. It never marks a plan as an executed app.
pub struct ProcessManager {
    processes: ProcessTable,
    scheduler: RoundRobin,
    threads: ThreadTable,
    cpu_scheduler: CpuSchedulerModel,
    kernel_stacks: KernelStackRegistry,
    exit_journal: ExitCoordinator,
}

impl Default for ProcessManager {
    fn default() -> Self { Self::new() }
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            processes: ProcessTable::new(),
            scheduler: RoundRobin::new(),
            threads: ThreadTable::new(),
            cpu_scheduler: CpuSchedulerModel::new(),
            kernel_stacks: KernelStackRegistry::new(),
            exit_journal: ExitCoordinator::new(),
        }
    }

    pub const fn isolated_execution_available() -> bool {
        false
    }

    pub fn reserve_native(
        &mut self,
        name: &'static str,
        entry: u64,
        stack_top: u64,
        stack_pages: u64,
    ) -> Result<Pid, ProcessError> {
        self.processes.create(name, entry, stack_top, stack_pages)
    }

    pub fn map_native_region(
        &mut self,
        pid: Pid,
        region: UserRegion,
    ) -> Result<(), ProcessError> {
        self.processes.map_region(pid, region)
    }

    pub fn prepare_native(&mut self, pid: Pid) -> Result<(), ProcessError> {
        self.processes.prepare(pid)
    }

    pub fn next_ready(&mut self) -> Option<Pid> {
        self.scheduler.next(&self.processes)
    }

    pub fn process(&self, pid: Pid) -> Option<&Process> {
        self.processes.process(pid)
    }

    /// Reserve metadata for one thread owned by a previously created native
    /// process. This does not spawn a CPU thread.
    pub fn reserve_thread(
        &mut self, pid: Pid, entry: u64, stack_top: u64, affinity_mask: u64,
    ) -> Result<Tid, ThreadError> {
        self.threads.create(&self.processes, pid, entry, stack_top, affinity_mask)
    }

    pub fn prepare_thread(&mut self, tid: Tid) -> Result<(), ThreadError> {
        self.threads.prepare(&self.processes, tid)
    }

    pub fn next_ready_thread(&mut self, cpu: usize) -> Option<Tid> {
        self.threads.next_ready(cpu)
    }

    /// Model-only CPU dispatch: does NOT enter Ring 3 or switch register state.
    /// Future trap backend must use the saved-context contract in cpu_local.
    pub fn dispatch_thread_model(&mut self, cpu: usize) -> Result<Option<Tid>, ScheduleError> {
        // Fail closed: selecting any Ready TID whose parent has not been
        // prepared, was killed or was reaped would schedule an orphan.
        // One canonical gateway owns this check before CPU-local selection.
        if self.threads.iter().any(|thread| {
            thread.state == crate::process::thread::ThreadState::Ready
                && !self.processes.process(thread.pid).is_some_and(|process|
                    matches!(process.state, ProcessState::Ready | ProcessState::Running))
        }) {
            return Err(ScheduleError::ParentNotRunnable);
        }
        self.cpu_scheduler.dispatch_next(&mut self.threads, cpu)
    }

    pub fn cpu_local_model_snapshot(&self, cpu: usize) -> Option<CpuLocalState> {
        self.cpu_scheduler.snapshot(cpu)
    }

    /// Do not release a process PID until EVERY owned TID has exited and
    /// been reaped. This check is metadata-only, not a frame/handle teardown.
    pub fn reap_native_model(&mut self, pid: Pid) -> Result<ExitReason, ReapModelError> {
        if self.exit_journal.pending_for(pid).is_some() {
            return Err(ReapModelError::CleanupPending);
        }
        if self.threads.iter().any(|thread| thread.pid == pid) {
            return Err(ReapModelError::ThreadsRemain);
        }
        self.processes.reap(pid).map_err(ReapModelError::Process)
    }

    pub fn reap_thread_model(&mut self, tid: Tid) -> Result<(), ThreadError> {
        let thread = self.threads.get(tid).ok_or(ThreadError::UnknownThread)?;
        if self.kernel_stacks.lease_for_thread(tid).is_some() {
            return Err(ThreadError::StackAttached);
        }
        if let Some(work) = self.exit_journal.pending_for(thread.pid) {
            if work.phase != CleanupPhase::Complete {
                return Err(ThreadError::InvalidState);
            }
        }
        self.threads.reap(tid)
    }

    /// Reserve a kernel-stack VA lease ONLY. The VMM owns actual guarded
    /// supervisor mapping and page reclamation. A TID may have one lease.
    pub fn reserve_kernel_stack_model(
        &mut self, tid: Tid, guard_base: u64, pages: u64,
    ) -> Result<u64, StackModelError> {
        let thread = self.threads.get(tid).ok_or(StackModelError::UnknownTid)?;
        if thread.state == ThreadState::Exited {
            return Err(StackModelError::ThreadExited);
        }
        self.kernel_stacks.reserve(tid, guard_base, pages)
            .map_err(StackModelError::Stack)
    }

    pub fn kernel_stack_model(&self, tid: Tid) -> Option<KernelStackLease> {
        self.kernel_stacks.lease_for_thread(tid)
    }

    pub fn activate_kernel_stack_model(&mut self, id: u64) -> Result<(), StackError> {
        self.kernel_stacks.activate(id)
    }

    /// # Safety
    /// Caller MUST establish no CPU or IRQ is using the stack, update TSS
    /// references and obtain architecture-specific quiescence. Does not unmap
    /// or free pages; no production memory reclamation is implemented.
    pub unsafe fn retire_kernel_stack_model(&mut self, id: u64) -> Result<(), StackError> {
        let lease = self.kernel_stacks.get(id).ok_or(StackError::UnknownLease)?;
        if self.threads.get(lease.tid).is_some_and(|t| t.state == ThreadState::Running)
            || (0..MAX_CPUS).any(|cpu| self.cpu_scheduler.snapshot(cpu)
                .is_some_and(|state| state.current() == Some(lease.tid)))
        {
            return Err(StackError::InvalidPhase);
        }
        // SAFETY: machine quiescence must be proven by this method's caller.
        unsafe { self.kernel_stacks.retire_after_quiescence(id) }
    }

    /// Kernel-owned VMM must have performed the actual unmap/TLB/frame free.
    /// This only drops a retired *metadata* record after such confirmation.
    pub fn release_kernel_stack_record_model(&mut self, id: u64) -> Result<(), StackError> {
        self.kernel_stacks.release_record(id)
    }

    /// Cooperative model transitions need a trusted full saved trap frame.
    /// Caller may not construct a SavedTrapContext from a user-provided TID.
    pub fn yield_thread_model(
        &mut self, cpu: usize, saved: SavedTrapContext,
    ) -> Result<Tid, ScheduleError> {
        self.cpu_scheduler.yield_saved(&mut self.threads, cpu, saved)
    }

    pub fn block_thread_model(
        &mut self, cpu: usize, saved: SavedTrapContext,
    ) -> Result<Tid, ScheduleError> {
        self.cpu_scheduler.block_saved(&mut self.threads, cpu, saved)
    }

    pub fn exit_thread_model(
        &mut self, cpu: usize, saved: SavedTrapContext,
    ) -> Result<Tid, ScheduleError> {
        self.cpu_scheduler.exit_saved(&mut self.threads, cpu, saved)
    }

    pub fn wake_thread_model(&mut self, tid: Tid) -> Result<(), ScheduleError> {
        let thread = self.threads.get(tid)
            .ok_or(ScheduleError::Thread(ThreadError::UnknownThread))?;
        if !self.processes.process(thread.pid).is_some_and(|p|
            matches!(p.state, ProcessState::Ready | ProcessState::Running))
        {
            return Err(ScheduleError::ParentNotRunnable);
        }
        self.cpu_scheduler.wake_blocked(&mut self.threads, tid)
    }

    /// Kernel-owned exit/fault/kill entry. Does not bypass an active CPU:
    /// ALL matching TIDs must be quiescent before changing ProcessTable and
    /// ThreadTable. Pending handles/IPC/memory prevent PID reap until ACKed.
    pub fn request_exit_model(
        &mut self, pid: Pid, reason: ExitReason,
    ) -> Result<ExitNotification, ExitModelError> {
        let process = self.processes.process(pid).ok_or(ExitModelError::UnknownPid)?;
        if process.state == ProcessState::Exited {
            return Err(ExitModelError::AlreadyExited);
        }
        if self.threads.iter().any(|t|
            t.pid == pid && t.state == ThreadState::Running)
        {
            return Err(ExitModelError::RunningThread);
        }
        // Reject stale scheduler references as well; one CPU0 alone does
        // not imply this model has safe multicore execution.
        for cpu in 0..MAX_CPUS {
            if let Some(current) = self.cpu_scheduler.snapshot(cpu)
                .and_then(|state| state.current())
            {
                let owner = self.threads.get(current)
                    .ok_or(ExitModelError::CpuStillActive)?.pid;
                if owner == pid { return Err(ExitModelError::CpuStillActive); }
            }
        }
        let ticket = self.exit_journal.reserve(pid, reason)
            .map_err(ExitModelError::Journal)?;
        if let Err(error) = self.processes.terminate(pid, reason) {
            // Only possible after unexpected change to the preflight contract.
            let _ = self.exit_journal.rollback_unpublished(ticket);
            return Err(ExitModelError::Process(error));
        }
        // ThreadTable preflight above ensures no running thread. There are no
        // panics or partial state changes in this bounded transition.
        self.threads.terminate_owned_quiescent(pid)
            .map_err(ExitModelError::Thread)?;
        self.exit_journal.publish(ticket).map_err(ExitModelError::Journal)
    }

    pub fn exit_cleanup_work(&self, ticket: u64) -> Result<CleanupWork, CleanupError> {
        self.exit_journal.work(ticket)
    }

    pub fn exit_cleanup_for_pid(&self, pid: Pid) -> Option<CleanupWork> {
        self.exit_journal.pending_for(pid)
    }

    /// Execute ONE stage of kernel cleanup; ACK only on backend success.
    /// The backend receives the trusted PID and original exit reason.
    /// It MUST make retries idempotent by ticket, and must NOT synthesize
    /// fake handles/PIDs or claim that mock acknowledgements free memory.
    pub fn drive_exit_cleanup_model<B: CleanupBackend>(
        &mut self, ticket: u64, backend: &mut B,
    ) -> Result<CleanupPhase, ExitModelError> {
        let work = self.exit_journal.work(ticket).map_err(ExitModelError::Journal)?;
        if work.phase == CleanupPhase::Complete { return Ok(CleanupPhase::Complete); }
        if work.phase == CleanupPhase::Memory {
            if self.threads.iter().any(|t| t.pid == work.event.pid
                && t.state == ThreadState::Running)
            {
                return Err(ExitModelError::RunningThread);
            }
            if self.kernel_stacks.iter().any(|lease|
                self.threads.get(lease.tid)
                    .is_some_and(|t| t.pid == work.event.pid))
            {
                return Err(ExitModelError::StacksRemain);
            }
        }
        let result = match work.phase {
            CleanupPhase::Capabilities => backend.revoke_capabilities(work.event),
            CleanupPhase::Ipc => backend.cancel_ipc_and_wake(work.event),
            CleanupPhase::Services => backend.notify_services(work.event),
            CleanupPhase::Memory => backend.release_address_space(work.event),
            CleanupPhase::Complete => Ok(()),
        };
        result.map_err(ExitModelError::Hook)?;
        let next = self.exit_journal.acknowledge(ticket, work.phase)
            .map_err(ExitModelError::Journal)?;
        Ok(next.phase)
    }

    /// Finalize only after ACKs from ALL resource owners AND reaping of all
    /// TIDs. Returning the exit reason does not mean actual user runtime
    /// exists; hardware isolation and memory ownership remain separate gates.
    pub fn finish_exit_model(&mut self, ticket: u64) -> Result<ExitReason, ExitModelError> {
        let work = self.exit_journal.work(ticket).map_err(ExitModelError::Journal)?;
        if work.phase != CleanupPhase::Complete {
            return Err(ExitModelError::CleanupIncomplete);
        }
        if self.threads.iter().any(|t| t.pid == work.event.pid) {
            return Err(ExitModelError::ThreadsRemain);
        }
        // Threads cannot be reaped while a stack lease is attached.
        let reason = self.processes.reap(work.event.pid)
            .map_err(ExitModelError::Process)?;
        self.exit_journal.retire(ticket).map_err(ExitModelError::Journal)?;
        Ok(reason)
    }

    /// Consistent read-only metadata snapshot for a future Task Manager IPC.
    /// The regular Desktop currently has no live user process runtime.
    pub fn snapshot(&self) -> TaskSnapshot {
        TaskSnapshot::collect(&self.processes, &self.threads)
    }

    pub fn thread(&self, tid: Tid) -> Option<&Thread> {
        self.threads.get(tid)
    }

    pub const fn cpu_mask(&self) -> u64 {
        self.threads.online_cpu_mask()
    }

    /// Refuse execution until independent user page tables, GDT/TSS,
    /// Ring 3 entry/return and recoverable faults are all verified.
    pub fn execute_native(&mut self, _pid: Pid) -> Result<(), ExecutionError> {
        Err(ExecutionError::IsolationUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::address_space::{Permissions, UserRegion, PAGE_SIZE};

    #[test]
    fn service_can_prepare_but_cannot_execute_unisolated_code() {
        let mut service = ProcessManager::new();
        let pid = service.reserve_native("hello", 0x400000, 0x800000, 8).unwrap();
        service.map_native_region(
            pid,
            UserRegion::checked(0x400000, PAGE_SIZE, Permissions::READ_EXECUTE).unwrap(),
        ).unwrap();
        service.prepare_native(pid).unwrap();
        assert_eq!(service.next_ready(), Some(pid));
        assert_eq!(service.execute_native(pid), Err(ExecutionError::IsolationUnavailable));
        assert!(!ProcessManager::isolated_execution_available());
        assert_eq!(service.cpu_mask(), 1);
        let tid=service.reserve_thread(pid, 0x400000, 0x800000, 1).unwrap();
        service.prepare_thread(tid).unwrap();
        assert_eq!(service.next_ready_thread(0), Some(tid));
        assert_eq!(service.next_ready_thread(1), None);
        assert_eq!(service.thread(tid).unwrap().pid, pid);
    }
    #[test]
    fn model_reap_refuses_orphan_tids_and_cpu0_is_only_online_cpu() {
        let mut service = ProcessManager::new();
        let pid = service.reserve_native("owned", 0x400000, 0x800000, 4).unwrap();
        service.map_native_region(
            pid, UserRegion::checked(0x400000, PAGE_SIZE, Permissions::READ_EXECUTE).unwrap()
        ).unwrap();
        service.prepare_native(pid).unwrap();
        let tid = service.reserve_thread(pid, 0x400000, 0x800000, 1).unwrap();
        service.prepare_thread(tid).unwrap();
        assert_eq!(service.dispatch_thread_model(1), Err(ScheduleError::CpuOffline));
        assert_eq!(service.cpu_local_model_snapshot(0).unwrap().current(), None);
        assert_eq!(service.reap_native_model(pid), Err(ReapModelError::ThreadsRemain));
        service.threads.exit(tid).unwrap();
        service.reap_thread_model(tid).unwrap();
        service.processes.mark_running(pid).unwrap();
        service.processes.mark_exited(pid, ExitReason::Normal(0)).unwrap();
        assert_eq!(service.reap_native_model(pid), Ok(ExitReason::Normal(0)));
        assert!(service.process(pid).is_none());
    }

    #[test]
    fn model_dispatch_rejects_ready_threads_of_exited_parent() {
        let mut service = ProcessManager::new();
        let pid = service.reserve_native("terminated", 0x400000, 0x800000, 4).unwrap();
        service.map_native_region(
            pid, UserRegion::checked(0x400000, PAGE_SIZE, Permissions::READ_EXECUTE).unwrap()
        ).unwrap();
        service.prepare_native(pid).unwrap();
        let tid = service.reserve_thread(pid, 0x400000, 0x800000, 1).unwrap();
        service.prepare_thread(tid).unwrap();
        service.processes.kill(pid).unwrap();
        assert_eq!(service.dispatch_thread_model(0), Err(ScheduleError::ParentNotRunnable));
        assert_eq!(service.thread(tid).unwrap().state,
            crate::process::thread::ThreadState::Ready);
    }

}
