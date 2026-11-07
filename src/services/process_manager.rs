//! Single service-facing gateway for future standalone processes.
//! Unlike the existing built-in AppManager, this does NOT execute a CPU task.
use crate::process::address_space::UserRegion;
use crate::process::scheduler::RoundRobin;
use crate::process::cpu_local::{CpuLocalState, CpuSchedulerModel, ScheduleError};
use crate::process::{ExitReason, Pid, Process, ProcessError, ProcessState, ProcessTable};
use crate::process::thread::{Thread, ThreadError, ThreadTable, Tid};
use super::task_snapshot::TaskSnapshot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionError {
    IsolationUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReapModelError {
    ThreadsRemain,
    Process(ProcessError),
}

/// Owns metadata and queue selection. It never marks a plan as an executed app.
pub struct ProcessManager {
    processes: ProcessTable,
    scheduler: RoundRobin,
    threads: ThreadTable,
    cpu_scheduler: CpuSchedulerModel,
}

impl Default for ProcessManager {
    fn default() -> Self { Self::new() }
}

impl ProcessManager {
    pub fn new() -> Self {
        Self { processes: ProcessTable::new(), scheduler: RoundRobin::new(), threads: ThreadTable::new(), cpu_scheduler: CpuSchedulerModel::new() }
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
        if self.threads.iter().any(|thread| thread.pid == pid) {
            return Err(ReapModelError::ThreadsRemain);
        }
        self.processes.reap(pid).map_err(ReapModelError::Process)
    }

    pub fn reap_thread_model(&mut self, tid: Tid) -> Result<(), ThreadError> {
        self.threads.reap(tid)
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
        assert_eq!(service.process(pid), None);
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
