//! Single service-facing gateway for future standalone processes.
//! Unlike the existing built-in AppManager, this does NOT execute a CPU task.
use crate::process::address_space::UserRegion;
use crate::process::scheduler::RoundRobin;
use crate::process::{Pid, Process, ProcessError, ProcessTable};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionError {
    IsolationUnavailable,
}

/// Owns metadata and queue selection. It never marks a plan as an executed app.
pub struct ProcessManager {
    processes: ProcessTable,
    scheduler: RoundRobin,
}

impl Default for ProcessManager {
    fn default() -> Self { Self::new() }
}

impl ProcessManager {
    pub fn new() -> Self {
        Self { processes: ProcessTable::new(), scheduler: RoundRobin::new() }
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
    }
}
