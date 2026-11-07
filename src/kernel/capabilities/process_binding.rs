//! Adapter to the ONE authoritative ProcessTable; no PID allocator or scheduler.
//! Only model state is available today. A future trap path must supply current
//! PID/TID from trusted CPU-local context, never from user message bytes.
use crate::process::{Pid, Process, ProcessState, ProcessTable, MAX_PROCESSES};
use super::{CapError, Handle, HandleTable, ObjectId, ObjectKey, ObjectKind, ObjectRegistry, Rights};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingError {
    MissingProcess, ProcessExited, NoRunningProcess, AmbiguousRunningProcess,
    AlreadyBound, NotBound, Full, Capability(CapError),
}
impl From<CapError> for BindingError {
    fn from(error: CapError) -> Self { Self::Capability(error) }
}

/// Provenance comes from an actual ProcessTable record, NOT from a wire PID.
/// It does NOT prove real CPL3 execution or provide a syscall entry boundary.
pub struct RunningIdentity<'a> {
    process: &'a Process,
}
impl RunningIdentity<'_> {
    pub const fn pid(&self) -> Pid { self.process.pid }
}
pub fn current_running(processes: &ProcessTable) -> Result<RunningIdentity<'_>, BindingError> {
    let mut found = None;
    for process in processes.iter() {
        if process.state == ProcessState::Running {
            if found.is_some() { return Err(BindingError::AmbiguousRunningProcess); }
            found = Some(process);
        }
    }
    found.map(|process| RunningIdentity { process }).ok_or(BindingError::NoRunningProcess)
}

/// Handle storage for the existing ProcessTable's PIDs; does not store any
/// Process or Thread records and cannot assign/reuse a PID.
pub struct ProcessHandleBook {
    tables: [Option<HandleTable>; MAX_PROCESSES],
}
impl Default for ProcessHandleBook { fn default() -> Self { Self::new() } }
impl ProcessHandleBook {
    pub fn new() -> Self {
        Self { tables: core::array::from_fn(|_| None) }
    }
    pub fn count(&self) -> usize { self.tables.iter().flatten().count() }

    /// Called by trusted ProcessManager creation/prepare lifecycle, never by an
    /// untrusted IPC argument. Checks the record still exists in ProcessTable.
    pub fn bind(&mut self, processes: &ProcessTable, pid: Pid) -> Result<(), BindingError> {
        let process = processes.process(pid).ok_or(BindingError::MissingProcess)?;
        if pid.0 == 0 || process.state == ProcessState::Exited {
            return Err(BindingError::ProcessExited);
        }
        if self.tables.iter().flatten().any(|table| table.owner() == pid.0) {
            return Err(BindingError::AlreadyBound);
        }
        let slot = self.tables.iter_mut().find(|slot| slot.is_none())
            .ok_or(BindingError::Full)?;
        *slot = Some(HandleTable::new(pid.0));
        Ok(())
    }
    /// Kernel-only explicit grants: the model never grants declared labels
    /// automatically. The caller must have already made a policy decision.
    pub fn kernel_grant(
        &mut self, processes: &ProcessTable, pid: Pid,
        objects: &ObjectRegistry, key: ObjectKey, rights: Rights,
    ) -> Result<Handle, BindingError> {
        let process = processes.process(pid).ok_or(BindingError::MissingProcess)?;
        if process.state == ProcessState::Exited { return Err(BindingError::ProcessExited); }
        let table = self.by_pid_mut(pid)?;
        Ok(table.grant(pid.0, objects, key, rights)?)
    }
    /// Derive active identity by scanning the authoritative model's Running
    /// state, never trusting a sender PID from payload or syscall register.
    pub fn running_table<'a>(
        &'a self, processes: &ProcessTable,
    ) -> Result<(&'a HandleTable, Pid), BindingError> {
        let pid = current_running(processes)?.pid();
        Ok((self.by_pid(pid)?, pid))
    }
    pub fn authorize_running(
        &self, processes: &ProcessTable, objects: &ObjectRegistry,
        handle: Handle, kind: ObjectKind, required: Rights,
    ) -> Result<ObjectKey, BindingError> {
        let (table, pid) = self.running_table(processes)?;
        Ok(table.authorize_key(pid.0, objects, handle, kind, required)?)
    }
    pub fn by_pid(&self, pid: Pid) -> Result<&HandleTable, BindingError> {
        self.tables.iter().flatten().find(|table| table.owner() == pid.0)
            .ok_or(BindingError::NotBound)
    }
    pub(crate) fn by_pid_mut(&mut self, pid: Pid) -> Result<&mut HandleTable, BindingError> {
        self.tables.iter_mut().flatten().find(|table| table.owner() == pid.0)
            .ok_or(BindingError::NotBound)
    }
    /// Exit callback MUST run before ProcessTable::reap. Closes all local handles,
    /// including references to resources owned by surviving processes.
    pub fn cleanup_exited(&mut self, processes: &ProcessTable, pid: Pid)
        -> Result<usize, BindingError>
    {
        let process = processes.process(pid).ok_or(BindingError::MissingProcess)?;
        if process.state != ProcessState::Exited { return Err(BindingError::ProcessExited); }
        let index = self.tables.iter()
            .position(|slot| slot.as_ref().is_some_and(|table| table.owner() == pid.0))
            .ok_or(BindingError::NotBound)?;
        let mut table = self.tables[index].take().ok_or(BindingError::NotBound)?;
        let closed = table.count();
        table.close_all();
        Ok(closed)
    }
    /// The bridge consumes generation-safe ObjectKey, not raw ObjectId.
    pub fn authorize_object_running(
        &self, processes: &ProcessTable, objects: &ObjectRegistry,
        handle: Handle, kind: ObjectKind, required: Rights,
    ) -> Result<ObjectId, BindingError> {
        Ok(self.authorize_running(processes, objects, handle, kind, required)?.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::address_space::{Permissions, UserRegion, PAGE_SIZE};
    use crate::process::ExitReason;
    fn ready(table: &mut ProcessTable) -> Pid {
        let pid = table.create("bound", 0x400000, 0x800000, 8).unwrap();
        table.map_region(pid, UserRegion::checked(0x400000, PAGE_SIZE, Permissions::READ_EXECUTE).unwrap()).unwrap();
        table.prepare(pid).unwrap();
        pid
    }
    #[test]
    fn current_pid_is_derived_from_real_process_state_not_wire_bytes() {
        let mut processes = ProcessTable::new();
        let actual = ready(&mut processes);
        let other = ready(&mut processes);
        let mut bound = ProcessHandleBook::new();
        bound.bind(&processes, actual).unwrap();
        bound.bind(&processes, other).unwrap();
        let mut objects = ObjectRegistry::new();
        let key = objects.create(ObjectKind::Device).unwrap();
        let authorized = bound.kernel_grant(&processes, actual, &objects, key, Rights::OBSERVE).unwrap();
        assert_eq!(current_running(&processes).err(), Some(BindingError::NoRunningProcess));
        processes.mark_running(actual).unwrap();
        assert_eq!(current_running(&processes).unwrap().pid(), actual);
        let attacker_payload_pid = other.0;
        assert_ne!(attacker_payload_pid, actual.0);
        assert_eq!(bound.authorize_running(&processes, &objects, authorized,
            ObjectKind::Device, Rights::OBSERVE).unwrap().id(), key.id());
        assert_eq!(bound.authorize_running(&processes, &objects, authorized,
            ObjectKind::Device, Rights::CONTROL), Err(BindingError::Capability(CapError::AccessDenied)));
        processes.mark_exited(actual, ExitReason::Normal(0)).unwrap();
        assert_eq!(bound.authorize_running(&processes, &objects, authorized,
            ObjectKind::Device, Rights::OBSERVE), Err(BindingError::NoRunningProcess));
        assert_eq!(bound.cleanup_exited(&processes, actual), Ok(1));
        assert_eq!(bound.count(), 1);
    }
    #[test]
    fn rejects_unknown_exited_and_duplicate_bindings() {
        let mut processes = ProcessTable::new();
        let pid = ready(&mut processes);
        let mut bound = ProcessHandleBook::new();
        assert_eq!(bound.bind(&processes, Pid(999)), Err(BindingError::MissingProcess));
        bound.bind(&processes, pid).unwrap();
        assert_eq!(bound.bind(&processes, pid), Err(BindingError::AlreadyBound));
        assert_eq!(bound.cleanup_exited(&processes, pid), Err(BindingError::ProcessExited));
        processes.kill(pid).unwrap();
        assert_eq!(bound.bind(&processes, pid), Err(BindingError::ProcessExited));
        assert_eq!(bound.cleanup_exited(&processes, pid), Ok(0));
        assert_eq!(bound.cleanup_exited(&processes, pid), Err(BindingError::NotBound));
    }
}
