//! Process exit cleanup orchestration, not IPC, ServiceManager or a VMM.
//!
//! A single kernel-owned ProcessManager holds this bounded journal. It records
//! one trusted exit/fault notification per PID, with a monotonically allocated
//! ticket; it NEVER invents PIDs or executes service code. A phase is advanced
//! only by a trusted backend acknowledging successfully completed work.
//!
//! Reservation is private staging: no consumer sees an exit notification until
//! ProcessTable and ThreadTable mutations have committed and publish() runs.
//! The model has NO cross-core synchronization; a real SMP implementation
//! must serialize lifecycle/handles/IPC and pin task objects before teardown.
use super::{ExitReason, Pid, MAX_PROCESSES};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupPhase {
    /// IPC agent: close every process-owned handle, revoke object refs.
    Capabilities,
    /// IPC agent: cancel pending calls, drain queues, wake waiters.
    Ipc,
    /// Services agent: publish service exit/fault with the real PID.
    Services,
    /// Memory owner: quiesce kernel stacks, unmap CR3/PTEs, flush TLB.
    Memory,
    /// ProcessManager may reap TIDs then finally reap PID.
    Complete,
}

impl CleanupPhase {
    const fn next(self) -> Self {
        match self {
            Self::Capabilities => Self::Ipc,
            Self::Ipc => Self::Services,
            Self::Services => Self::Memory,
            Self::Memory | Self::Complete => Self::Complete,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitNotification {
    /// Identified by the trusted kernel ProcessTable, never a wire sender.
    pub pid: Pid,
    pub reason: ExitReason,
    /// Stable transaction ID, never reused even if the PID slot is reused.
    pub ticket: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CleanupWork {
    pub event: ExitNotification,
    pub phase: CleanupPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupHookError {
    /// Dependency not yet available; retry same ticket without state advance.
    Deferred,
    /// Injected or observed backend failure; retain current stage.
    Failed,
}

/// Trusted kernel-side integration port. The ProcessManager calls each
/// method at most once PER SUCCESSFUL ACK, but may retry on any failure.
/// Implementations MUST be idempotent by (pid, ticket). In production they
/// must use the IPC agent's actual handle close/call cancellation and the
/// memory owner's quiescent VMM release, not merely return Ok(()).
///
/// ServiceManager must never mint a PID or grant capabilities through this
/// interface. A fault/exit report is kernel-issued data only.
pub trait CleanupBackend {
    fn revoke_capabilities(&mut self, exit: ExitNotification)
        -> Result<(), CleanupHookError>;
    fn cancel_ipc_and_wake(&mut self, exit: ExitNotification)
        -> Result<(), CleanupHookError>;
    fn notify_services(&mut self, exit: ExitNotification)
        -> Result<(), CleanupHookError>;
    fn release_address_space(&mut self, exit: ExitNotification)
        -> Result<(), CleanupHookError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupError {
    Duplicate,
    Capacity,
    Exhausted,
    UnknownTicket,
    NotPublished,
    AlreadyPublished,
    PhaseMismatch,
    Incomplete,
}

#[derive(Clone, Copy, Debug)]
struct CleanupRecord {
    event: ExitNotification,
    phase: CleanupPhase,
    published: bool,
}

/// Journal, not an alternative ProcessTable or an event-loop daemon.
/// A failed external cleanup preserves its current phase for retry.
pub struct ExitCoordinator {
    records: [Option<CleanupRecord>; MAX_PROCESSES],
    next_ticket: u64,
}

impl Default for ExitCoordinator {
    fn default() -> Self { Self::new() }
}

impl ExitCoordinator {
    pub const fn new() -> Self {
        Self { records: [None; MAX_PROCESSES], next_ticket: 1 }
    }

    pub fn pending_for(&self, pid: Pid) -> Option<CleanupWork> {
        self.records.iter().flatten()
            .find(|r| r.published && r.event.pid == pid)
            .map(|r| CleanupWork { event: r.event, phase: r.phase })
    }

    pub fn work(&self, ticket: u64) -> Result<CleanupWork, CleanupError> {
        let r = self.records.iter().flatten()
            .find(|r| r.event.ticket == ticket)
            .ok_or(CleanupError::UnknownTicket)?;
        if !r.published { return Err(CleanupError::NotPublished); }
        Ok(CleanupWork { event: r.event, phase: r.phase })
    }

    /// First half of a two-phase publication. Exclusive ProcessManager access
    /// is required around reserve -> table transitions -> publish.
    pub fn reserve(&mut self, pid: Pid, reason: ExitReason) -> Result<u64, CleanupError> {
        if self.records.iter().flatten().any(|r| r.event.pid == pid) {
            return Err(CleanupError::Duplicate);
        }
        let slot = self.records.iter().position(Option::is_none)
            .ok_or(CleanupError::Capacity)?;
        let next = self.next_ticket.checked_add(1).ok_or(CleanupError::Exhausted)?;
        let ticket = self.next_ticket;
        self.next_ticket = next;
        self.records[slot] = Some(CleanupRecord {
            event: ExitNotification { pid, reason, ticket },
            phase: CleanupPhase::Capabilities,
            published: false,
        });
        Ok(ticket)
    }

    pub fn publish(&mut self, ticket: u64) -> Result<ExitNotification, CleanupError> {
        let r = self.records.iter_mut().flatten()
            .find(|r| r.event.ticket == ticket)
            .ok_or(CleanupError::UnknownTicket)?;
        if r.published { return Err(CleanupError::AlreadyPublished); }
        r.published = true;
        Ok(r.event)
    }

    /// Rollback ONLY an unpublished staging reservation. Ticket stays burned.
    pub fn rollback_unpublished(&mut self, ticket: u64) -> Result<(), CleanupError> {
        let slot = self.records.iter_mut()
            .find(|s| s.as_ref().is_some_and(|r| r.event.ticket == ticket))
            .ok_or(CleanupError::UnknownTicket)?;
        if slot.as_ref().is_some_and(|r| r.published) {
            return Err(CleanupError::AlreadyPublished);
        }
        *slot = None;
        Ok(())
    }

    /// Strictly ordered ACK. A failed backend simply does NOT ack, retaining
    /// the current work for retry. ACK is privileged; no untrusted process may
    /// assert handle/queue/memory cleanup occurred.
    pub fn acknowledge(
        &mut self, ticket: u64, completed: CleanupPhase,
    ) -> Result<CleanupWork, CleanupError> {
        let r = self.records.iter_mut().flatten()
            .find(|r| r.event.ticket == ticket)
            .ok_or(CleanupError::UnknownTicket)?;
        if !r.published { return Err(CleanupError::NotPublished); }
        if r.phase != completed || completed == CleanupPhase::Complete {
            return Err(CleanupError::PhaseMismatch);
        }
        r.phase = r.phase.next();
        Ok(CleanupWork { event: r.event, phase: r.phase })
    }

    /// Called by ProcessManager after ALL owned TIDs and kernel stack leases
    /// have been reaped and ProcessTable::reap has succeeded.
    pub fn retire(&mut self, ticket: u64) -> Result<ExitNotification, CleanupError> {
        let slot = self.records.iter_mut()
            .find(|s| s.as_ref().is_some_and(|r| r.event.ticket == ticket))
            .ok_or(CleanupError::UnknownTicket)?;
        let r = slot.as_ref().ok_or(CleanupError::UnknownTicket)?;
        if !r.published { return Err(CleanupError::NotPublished); }
        if r.phase != CleanupPhase::Complete { return Err(CleanupError::Incomplete); }
        let event = r.event;
        *slot = None;
        Ok(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_stays_private_until_publish_and_cleanup_is_ordered() {
        let mut x = ExitCoordinator::new();
        let pid = Pid(7);
        let ticket = x.reserve(pid, ExitReason::Normal(0)).unwrap();
        assert_eq!(x.work(ticket), Err(CleanupError::NotPublished));
        assert_eq!(x.pending_for(pid), None);
        assert_eq!(x.acknowledge(ticket, CleanupPhase::Capabilities),
            Err(CleanupError::NotPublished));
        let event = x.publish(ticket).unwrap();
        assert_eq!((event.pid, event.ticket), (pid, ticket));
        assert_eq!(x.publish(ticket), Err(CleanupError::AlreadyPublished));
        for (completed, following) in [
            (CleanupPhase::Capabilities, CleanupPhase::Ipc),
            (CleanupPhase::Ipc, CleanupPhase::Services),
            (CleanupPhase::Services, CleanupPhase::Memory),
            (CleanupPhase::Memory, CleanupPhase::Complete),
        ] {
            assert_eq!(x.work(ticket).unwrap().phase, completed);
            assert_eq!(x.acknowledge(ticket, completed).unwrap().phase, following);
        }
        assert_eq!(x.acknowledge(ticket, CleanupPhase::Memory),
            Err(CleanupError::PhaseMismatch));
        assert_eq!(x.retire(ticket), Ok(event));
        assert_eq!(x.work(ticket), Err(CleanupError::UnknownTicket));
        let later = x.reserve(pid, ExitReason::Killed).unwrap();
        assert!(later > ticket);
    }

    #[test]
    fn failed_backend_retry_duplicate_pid_and_out_of_order_ack() {
        let mut x = ExitCoordinator::new();
        let id = x.reserve(Pid(3), ExitReason::Killed).unwrap();
        x.publish(id).unwrap();
        assert_eq!(x.reserve(Pid(3), ExitReason::Killed), Err(CleanupError::Duplicate));
        assert_eq!(x.acknowledge(id, CleanupPhase::Ipc),
            Err(CleanupError::PhaseMismatch));
        assert_eq!(x.retire(id), Err(CleanupError::Incomplete));
        // Mock backend fails: no acknowledgment. Same action remains available.
        assert_eq!(x.work(id).unwrap().phase, CleanupPhase::Capabilities);
        assert_eq!(x.work(id).unwrap().phase, CleanupPhase::Capabilities);
        x.acknowledge(id, CleanupPhase::Capabilities).unwrap();
        assert_eq!(x.acknowledge(id, CleanupPhase::Capabilities),
            Err(CleanupError::PhaseMismatch));
    }

    #[test]
    fn rollback_is_private_and_generation_is_monotonic() {
        let mut x = ExitCoordinator::new();
        let a = x.reserve(Pid(1), ExitReason::Killed).unwrap();
        x.rollback_unpublished(a).unwrap();
        assert_eq!(x.rollback_unpublished(a), Err(CleanupError::UnknownTicket));
        let b = x.reserve(Pid(1), ExitReason::Killed).unwrap();
        assert!(b > a);
        x.publish(b).unwrap();
        assert_eq!(x.rollback_unpublished(b), Err(CleanupError::AlreadyPublished));
        assert_eq!(x.work(a), Err(CleanupError::UnknownTicket));
    }

    #[test]
    fn bounded_pending_exit_journal_never_evicts_live_notifications() {
        let mut x = ExitCoordinator::new();
        for i in 0..MAX_PROCESSES {
            let id = x.reserve(Pid(i as u64 + 1), ExitReason::Killed).unwrap();
            x.publish(id).unwrap();
        }
        assert_eq!(x.reserve(Pid(100), ExitReason::Killed), Err(CleanupError::Capacity));
        assert_eq!(x.pending_for(Pid(1)).unwrap().phase, CleanupPhase::Capabilities);
    }
}
