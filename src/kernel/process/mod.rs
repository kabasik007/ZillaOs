//! Process lifecycle and memory *plans* for ZillaOS.
//!
//! This module is safe, allocation-free groundwork. It does not provide
//! hardware isolation, CR3 switching, CPU context switching or syscall
//! entry. A separate opt-in QEMU Ring3 fixture exists, but production
//! ProcessManager cannot execute arbitrary tasks yet.
pub mod address_space;
pub mod cpu_local;
pub mod kernel_stack;
pub mod context;
pub mod scheduler;
pub mod syscall;
pub mod thread;

use address_space::{Access, AddressSpacePlan, MapError, UserRegion, USER_MIN, USER_TOP};
use context::InitialUserContext;

pub const MAX_PROCESSES: usize = 16;
pub const DEFAULT_STACK_PAGES: u64 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pid(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    Created,
    Ready,
    Running,
    Exited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserFault {
    InvalidOpcode,
    GeneralProtection,
    PageFault { address: u64, error: u64 },
    Other(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitReason {
    Normal(i32),
    Fault(UserFault),
    Killed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessError {
    NoFreeSlot,
    PidExhausted,
    InvalidEntry,
    InvalidState,
    NotExecutable,
    CpuBusy,
    UnknownPid,
    AddressSpace(MapError),
}

impl From<MapError> for ProcessError {
    fn from(error: MapError) -> Self {
        Self::AddressSpace(error)
    }
}

#[derive(Clone, Debug)]
pub struct Process {
    pub pid: Pid,
    pub name: &'static str,
    pub state: ProcessState,
    pub context: InitialUserContext,
    pub address_space: AddressSpacePlan,
    pub exit_reason: Option<ExitReason>,
}

pub struct ProcessTable {
    slots: [Option<Process>; MAX_PROCESSES],
    next_pid: u64,
}

impl Default for ProcessTable {
    fn default() -> Self { Self::new() }
}

impl ProcessTable {
    pub fn new() -> Self {
        Self { slots: core::array::from_fn(|_| None), next_pid: 1 }
    }

    /// Reserve a process record only. No executable bytes are loaded or run.
    pub fn create(
        &mut self,
        name: &'static str,
        entry: u64,
        stack_top: u64,
        stack_pages: u64,
    ) -> Result<Pid, ProcessError> {
        if !(USER_MIN..USER_TOP).contains(&entry) {
            return Err(ProcessError::InvalidEntry);
        }
        let index = self.slots.iter().position(Option::is_none).ok_or(ProcessError::NoFreeSlot)?;
        let address_space = AddressSpacePlan::with_stack(stack_top, stack_pages)?;
        let next = self.next_pid.checked_add(1).ok_or(ProcessError::PidExhausted)?;
        let pid = Pid(self.next_pid);
        self.next_pid = next;
        self.slots[index] = Some(Process {
            pid,
            name,
            state: ProcessState::Created,
            context: InitialUserContext::new(entry, stack_top),
            address_space,
            exit_reason: None,
        });
        Ok(pid)
    }

    pub fn process(&self, pid: Pid) -> Option<&Process> {
        self.iter().find(|process| process.pid == pid)
    }

    fn process_mut(&mut self, pid: Pid) -> Result<&mut Process, ProcessError> {
        self.slots.iter_mut().flatten()
            .find(|process| process.pid == pid).ok_or(ProcessError::UnknownPid)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Process> + '_ {
        self.slots.iter().flatten()
    }

    pub fn count(&self) -> usize {
        self.iter().count()
    }

    pub fn map_region(&mut self, pid: Pid, region: UserRegion) -> Result<(), ProcessError> {
        let process = self.process_mut(pid)?;
        if process.state != ProcessState::Created {
            return Err(ProcessError::InvalidState);
        }
        process.address_space.map(region)?;
        Ok(())
    }

    /// Transition to Ready after checking that the entry is RX and stack RW.
    /// This still does not commit hardware mappings.
    pub fn prepare(&mut self, pid: Pid) -> Result<(), ProcessError> {
        let process = self.process_mut(pid)?;
        if process.state != ProcessState::Created {
            return Err(ProcessError::InvalidState);
        }
        if !process.address_space.allows(process.context.rip, 1, Access::Execute) {
            return Err(ProcessError::NotExecutable);
        }
        if !process.address_space.allows(process.context.rsp - 1, 1, Access::Write) {
            return Err(ProcessError::InvalidState);
        }
        process.state = ProcessState::Ready;
        Ok(())
    }

    /// Bookkeeping only: caller must provide real Ring 3 switching later.
    pub fn mark_running(&mut self, pid: Pid) -> Result<(), ProcessError> {
        if self.iter().any(|process| process.state == ProcessState::Running) {
            return Err(ProcessError::CpuBusy);
        }
        let process = self.process_mut(pid)?;
        if process.state != ProcessState::Ready {
            return Err(ProcessError::InvalidState);
        }
        process.state = ProcessState::Running;
        Ok(())
    }

    pub fn mark_exited(&mut self, pid: Pid, reason: ExitReason) -> Result<(), ProcessError> {
        let process = self.process_mut(pid)?;
        if process.state != ProcessState::Running {
            return Err(ProcessError::InvalidState);
        }
        process.exit_reason = Some(reason);
        process.state = ProcessState::Exited;
        Ok(())
    }

    pub fn kill(&mut self, pid: Pid) -> Result<(), ProcessError> {
        let process = self.process_mut(pid)?;
        if process.state == ProcessState::Exited {
            return Err(ProcessError::InvalidState);
        }
        process.exit_reason = Some(ExitReason::Killed);
        process.state = ProcessState::Exited;
        Ok(())
    }

    pub fn reap(&mut self, pid: Pid) -> Result<ExitReason, ProcessError> {
        let index = self.slots.iter()
            .position(|slot| slot.as_ref().is_some_and(|process| process.pid == pid))
            .ok_or(ProcessError::UnknownPid)?;
        let process = self.slots[index].as_ref().ok_or(ProcessError::UnknownPid)?;
        if process.state != ProcessState::Exited {
            return Err(ProcessError::InvalidState);
        }
        let reason = process.exit_reason.ok_or(ProcessError::InvalidState)?;
        self.slots[index] = None;
        Ok(reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use address_space::{Permissions, PAGE_SIZE};
    use scheduler::RoundRobin;
    use syscall::{Action, Error, Request, SYS_EXIT, SYS_WRITE, SYS_YIELD};

    fn code_region(base: u64) -> UserRegion {
        UserRegion::checked(base, PAGE_SIZE, Permissions::READ_EXECUTE).unwrap()
    }

    fn ready(table: &mut ProcessTable, name: &'static str) -> Pid {
        let pid = table.create(name, 0x400000, 0x800000, DEFAULT_STACK_PAGES).unwrap();
        table.map_region(pid, code_region(0x400000)).unwrap();
        table.prepare(pid).unwrap();
        pid
    }

    #[test]
    fn create_prepare_exit_and_reap_preserve_identity() {
        let mut table = ProcessTable::new();
        let pid = table.create("native-hello", 0x400000, 0x800000, 8).unwrap();
        assert_eq!(pid, Pid(1));
        assert_eq!(table.prepare(pid), Err(ProcessError::NotExecutable));
        table.map_region(pid, code_region(0x400000)).unwrap();
        table.prepare(pid).unwrap();
        assert_eq!(table.process(pid).unwrap().state, ProcessState::Ready);
        table.mark_running(pid).unwrap();
        table.mark_exited(pid, ExitReason::Normal(0)).unwrap();
        assert_eq!(table.reap(pid), Ok(ExitReason::Normal(0)));
        assert_eq!(table.process(pid).map(|p| p.state), None);
        assert_eq!(ready(&mut table, "next"), Pid(2));
    }

    #[test]
    fn only_one_running_process_and_valid_transitions() {
        let mut table = ProcessTable::new();
        let p1 = ready(&mut table, "one");
        let p2 = ready(&mut table, "two");
        assert_eq!(table.mark_exited(p1, ExitReason::Normal(0)), Err(ProcessError::InvalidState));
        table.mark_running(p1).unwrap();
        assert_eq!(table.mark_running(p2), Err(ProcessError::CpuBusy));
        table.mark_exited(p1, ExitReason::Normal(4)).unwrap();
        table.mark_running(p2).unwrap();
        assert_eq!(table.reap(p2), Err(ProcessError::InvalidState));
    }

    #[test]
    fn user_fault_is_recorded_not_treated_as_kernel_panic_in_model() {
        let mut table = ProcessTable::new();
        let pid = ready(&mut table, "faulting");
        table.mark_running(pid).unwrap();
        let fault = UserFault::PageFault { address: USER_TOP, error: 5 };
        table.mark_exited(pid, ExitReason::Fault(fault)).unwrap();
        assert_eq!(table.reap(pid), Ok(ExitReason::Fault(fault)));
    }

    #[test]
    fn scheduler_round_robins_ready_pids() {
        let mut table = ProcessTable::new();
        let a = ready(&mut table, "a");
        let b = ready(&mut table, "b");
        let c = ready(&mut table, "c");
        let mut scheduler = RoundRobin::new();
        assert_eq!(scheduler.next(&table), Some(a));
        assert_eq!(scheduler.next(&table), Some(b));
        table.kill(b).unwrap();
        assert_eq!(scheduler.next(&table), Some(c));
        assert_eq!(scheduler.next(&table), Some(a));
    }

    #[test]
    fn syscall_contract_rejects_kernel_and_invalid_buffers() {
        let mut table = ProcessTable::new();
        let pid = ready(&mut table, "writer");
        let p = table.process(pid).unwrap();
        let good = Request { number: SYS_WRITE, arg0: 1, arg1: 0x400000, arg2: 5 };
        assert_eq!(syscall::validate(p, good), Ok(Action::Write { address: 0x400000, len: 5 }));
        assert_eq!(syscall::validate(p, Request { arg1: USER_TOP, ..good }), Err(Error::InvalidUserBuffer));
        assert_eq!(syscall::validate(p, Request { arg1: u64::MAX, ..good }), Err(Error::InvalidUserBuffer));
        assert_eq!(syscall::validate(p, Request { arg0: 2, ..good }), Err(Error::InvalidHandle));
        assert_eq!(syscall::validate(p, Request { arg2: 0, ..good }), Err(Error::InvalidLength));
        assert_eq!(syscall::validate(p, Request { number: 0xffff, ..good }), Err(Error::Unsupported));
        assert_eq!(syscall::validate(p, Request { number: SYS_EXIT, arg0: 7, ..good }), Ok(Action::Exit { code: 7 }));
        assert_eq!(syscall::validate(p, Request { number: SYS_YIELD, ..good }), Ok(Action::Yield));
    }

    #[test]
    fn resource_and_entry_guards() {
        let mut table = ProcessTable::new();
        assert_eq!(table.create("kernel", USER_TOP, 0x800000, 8), Err(ProcessError::InvalidEntry));
        assert!(table.create("bad-stack", 0x400000, 0x800000, 0).is_err());
        let pid = table.create("only-data", 0x400000, 0x800000, 8).unwrap();
        table.map_region(
            pid,
            UserRegion::checked(0x400000, PAGE_SIZE, Permissions::READ_WRITE).unwrap(),
        ).unwrap();
        assert_eq!(table.prepare(pid), Err(ProcessError::NotExecutable));
        for _ in 1..MAX_PROCESSES {
            table.create("reserved", 0x400000, 0x800000, 8).unwrap();
        }
        assert_eq!(table.count(), MAX_PROCESSES);
        assert_eq!(table.create("full", 0x400000, 0x800000, 8), Err(ProcessError::NoFreeSlot));
    }
}
