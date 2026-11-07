//! Integration-ready, **model-only** ProcessManager/ServiceManager handoff.
//!
//! This file is a child of service_manager; it owns **one binding per service**,
//! never a process registry or an allocator of PIDs. The concrete Pid type is
//! supplied by the kernel ProcessManager owner through ProcessBoundary::Pid.
//! All launch/terminate/cleanup actions are *requests*, not execution.
//! There is no Ring 3 runtime, syscall entry, IPC or capability grant here.
use super::{
    MAX_SERVICES, Health, Quotas, ServiceError, ServiceId, ServiceInfo,
    ServiceManager, ServiceManifest, ServiceState, ValidationError,
};

pub const START_TIMEOUT_MS: u64 = 5_000;
pub const STOP_TIMEOUT_MS: u64 = 5_000;

/// Names are stable policy operations, *not* ambient permissions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlOperation { Start, Stop, Restart }

/// A process identity is owned by the kernel's ProcessManager. Production
/// adapters must set Pid = crate::process::Pid, never a service-local number.
pub trait ProcessBoundary {
    type Pid: Copy + Eq;
    /// Kernel-checked control handle and current caller identity; false denies.
    fn allow_control(&self, operation: ControlOperation, service: &ServiceManifest) -> bool;
    /// Authenticate a launch-result event from ProcessManager AND verify the
    /// exact executable image, rights/quotas and kernel-issued grants.
    fn verify_started(
        &self, ticket: LaunchTicket, pid: Self::Pid, manifest: &ServiceManifest,
    ) -> bool;
    /// Check authenticated kernel exit notification, not an IPC-supplied pid.
    fn verify_exited(
        &self, ticket: LaunchTicket, pid: Self::Pid, reason: ExitReason,
    ) -> bool;
    /// Must verify stopped task, revoked handles/IPC, quiesced IRQ/DMA and
    /// process-owned resource teardown. For None, cancel by launch ticket.
    fn verify_cleaned(&self, ticket: LaunchTicket, pid: Option<Self::Pid>) -> bool;
    /// Read-only authoritative kernel liveness query for TaskManager display.
    fn is_live(&self, pid: Self::Pid) -> bool;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitReason { Normal, Fault, Killed }

/// A supervisor-instance epoch must come from the trusted integration owner.
/// Tickets are one-use, never equivalent to PIDs or authorization handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaunchTicket {
    id: ServiceId,
    epoch: u64,
    generation: u64,
}
impl LaunchTicket {
    pub const fn service(self) -> ServiceId { self.id }
    pub const fn epoch(self) -> u64 { self.epoch }
    pub const fn generation(self) -> u64 { self.generation }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeError {
    UnknownService, AccessDenied, InvalidState, InvalidTicket, TicketExpired,
    DuplicatePid, KernelRejected, DeadlineOverflow, GenerationExhausted,
    CleanupPending, ActiveDependent, InconsistentModel, Service(ServiceError),
}
impl From<ServiceError> for BridgeError {
    fn from(error: ServiceError) -> Self { Self::Service(error) }
}

/// Read-only request for the future verified image launcher. It cannot spawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaunchIntent {
    pub ticket: LaunchTicket,
    pub executable: &'static str,
    pub requested_capabilities: &'static [&'static str],
    pub quotas: Quotas,
    pub expires_at_ms: u64,
}

/// Supervisory actions are declarative only; no kernel work is performed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupervisorAction<P: Copy + Eq> {
    Launch(LaunchIntent),
    Stop { ticket: LaunchTicket, pid: P, force: bool },
    Cleanup { ticket: LaunchTicket, pid: Option<P> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BoundProcess<P: Copy + Eq> { pid: P, ticket: LaunchTicket }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingLaunch { intent: LaunchIntent, emitted: bool }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingStop {
    deadline_ms: u64,
    emitted: bool,
    force_requested: bool,
    force_emitted: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingCleanup<P: Copy + Eq> {
    ticket: LaunchTicket,
    pid: Option<P>,
    emitted: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SupervisorInfo<P: Copy + Eq> {
    pub service: ServiceInfo,
    /// Only verified ProcessManager identities, never synthesized PIDs.
    pub verified_pid: Option<P>,
    pub launch_ticket: Option<LaunchTicket>,
    pub teardown_pending: bool,
}
pub struct SupervisorSnapshot<P: Copy + Eq> {
    entries: [Option<SupervisorInfo<P>>; MAX_SERVICES],
    len: usize,
}
impl<P: Copy + Eq> SupervisorSnapshot<P> {
    pub const fn len(&self) -> usize { self.len }
    pub fn iter(&self) -> impl Iterator<Item = SupervisorInfo<P>> + '_ {
        self.entries[..self.len].iter().flatten().copied()
    }
}

/// No process table: only one authenticated ProcessManager binding for each
/// service. All real process accounting remains inside ProcessManager.
pub struct ServiceSupervisor<P: Copy + Eq> {
    model: ServiceManager,
    epoch: u64,
    next_generation: u64,
    bound: [Option<BoundProcess<P>>; MAX_SERVICES],
    launching: [Option<PendingLaunch>; MAX_SERVICES],
    stopping: [Option<PendingStop>; MAX_SERVICES],
    cleaning: [Option<PendingCleanup<P>>; MAX_SERVICES],
}

impl<P: Copy + Eq> ServiceSupervisor<P> {
    /// Epoch must be nonzero, unique per supervisor instance across restarts.
    /// Production creator must obtain it from a trusted kernel monotonic ID.
    pub fn new(
        manifests: &[ServiceManifest], epoch: u64,
    ) -> Result<Self, ValidationError> {
        if epoch == 0 { return Err(ValidationError::InvalidVersion); }
        Ok(Self {
            model: ServiceManager::new(manifests)?,
            epoch, next_generation: 1,
            bound: [None; MAX_SERVICES],
            launching: [None; MAX_SERVICES],
            stopping: [None; MAX_SERVICES],
            cleaning: [None; MAX_SERVICES],
        })
    }
    pub const fn model(&self) -> &ServiceManager { &self.model }
    pub fn lookup(&self, name: &str) -> Result<ServiceId, BridgeError> {
        self.model.lookup(name).map_err(|_| BridgeError::UnknownService)
    }
    fn checked_index(&self, id: ServiceId) -> Result<usize, BridgeError> {
        if id.index() < self.model.registry().len() { Ok(id.index()) }
        else { Err(BridgeError::UnknownService) }
    }
    fn state(&self, id: ServiceId) -> Result<ServiceState, BridgeError> {
        let n = self.checked_index(id)?;
        Ok(self.model.snapshot().iter().nth(n)
            .ok_or(BridgeError::InconsistentModel)?.state)
    }
    fn permit<B: ProcessBoundary<Pid = P>>(
        &self, boundary: &B, id: ServiceId, op: ControlOperation,
    ) -> Result<(), BridgeError> {
        self.checked_index(id)?;
        if boundary.allow_control(op, self.model.registry().manifest(id)) {
            Ok(())
        } else { Err(BridgeError::AccessDenied) }
    }
    fn reserve_ticket(&mut self, id: ServiceId, now: u64)
        -> Result<(), BridgeError>
    {
        let n = self.checked_index(id)?;
        if self.state(id)? != ServiceState::Starting { return Err(BridgeError::InvalidState); }
        if self.launching[n].is_some() { return Ok(()); }
        if self.bound[n].is_some() || self.cleaning[n].is_some() {
            return Err(BridgeError::CleanupPending);
        }
        let deadline = now.checked_add(START_TIMEOUT_MS)
            .ok_or(BridgeError::DeadlineOverflow)?;
        let next = self.next_generation.checked_add(1)
            .ok_or(BridgeError::GenerationExhausted)?;
        let ticket = LaunchTicket {
            id, epoch: self.epoch, generation: self.next_generation,
        };
        self.next_generation = next;
        let m = self.model.registry().manifest(id);
        self.launching[n] = Some(PendingLaunch {
            intent: LaunchIntent {
                ticket, executable: m.executable,
                requested_capabilities: m.requested_capabilities,
                quotas: m.quotas, expires_at_ms: deadline,
            },
            emitted: false,
        });
        Ok(())
    }
    /// Caller must be authenticated at the *kernel* boundary. No PID issued.
    pub fn request_start<B: ProcessBoundary<Pid = P>>(
        &mut self, boundary: &B, id: ServiceId, now: u64,
    ) -> Result<(), BridgeError> {
        self.permit(boundary, id, ControlOperation::Start)?;
        if now.checked_add(START_TIMEOUT_MS).is_none() {
            return Err(BridgeError::DeadlineOverflow);
        }
        self.model.model_start(id, true, now)?;
        if self.state(id)? == ServiceState::Starting {
            self.reserve_ticket(id, now)?;
        }
        Ok(())
    }
    /// Boot policy is deliberately NOT automatic. Only explicit per-service
    /// requests may select OnBoot manifests; dependency waits are still valid.
    pub fn request_stop<B: ProcessBoundary<Pid = P>>(
        &mut self, boundary: &B, id: ServiceId, now: u64,
    ) -> Result<(), BridgeError> {
        self.permit(boundary, id, ControlOperation::Stop)?;
        self.checked_index(id)?;
        if now.checked_add(STOP_TIMEOUT_MS).is_none() {
            return Err(BridgeError::DeadlineOverflow);
        }
        self.model.model_stop(id, true, now)?;
        self.mark_stopping_or_cancel(id, now)
    }
    pub fn request_restart<B: ProcessBoundary<Pid = P>>(
        &mut self, boundary: &B, id: ServiceId, now: u64,
    ) -> Result<(), BridgeError> {
        self.permit(boundary, id, ControlOperation::Restart)?;
        if now.checked_add(START_TIMEOUT_MS.max(STOP_TIMEOUT_MS)).is_none() {
            return Err(BridgeError::DeadlineOverflow);
        }
        if matches!(self.state(id)?, ServiceState::Failed | ServiceState::Backoff) {
            // Explicit authorized repair re-arms the circuit breaker.
            if self.cleaning[id.index()].is_some() { return Err(BridgeError::CleanupPending); }
            self.model.model_stop(id, true, now)?;
            self.model.model_start(id, true, now)?;
        } else {
            self.model.model_restart(id, true, now)?;
        }
        self.mark_stopping_or_cancel(id, now)?;
        if self.state(id)? == ServiceState::Starting {
            self.reserve_ticket(id, now)?;
        }
        Ok(())
    }
    fn mark_stopping_or_cancel(
        &mut self, id: ServiceId, now: u64,
    ) -> Result<(), BridgeError> {
        let n = self.checked_index(id)?;
        if self.state(id)? != ServiceState::Stopping { return Ok(()); }
        if self.stopping[n].is_some() || self.cleaning[n].is_some() { return Ok(()); }
        if let Some(launching) = self.launching[n].take() {
            // A launch intent might already have crossed the kernel boundary.
            // Require a trusted cancellation+revoke ACK before reusing it.
            self.cleaning[n] = Some(PendingCleanup {
                ticket: launching.intent.ticket, pid: None, emitted: false,
            });
        } else if self.bound[n].is_some() {
            self.stopping[n] = Some(PendingStop {
                deadline_ms: now.checked_add(STOP_TIMEOUT_MS)
                    .ok_or(BridgeError::DeadlineOverflow)?,
                emitted: false, force_requested: false, force_emitted: false,
            });
        } else {
            return Err(BridgeError::InconsistentModel);
        }
        Ok(())
    }

    /// ProcessManager response must be kernel-authenticated. A caller cannot
    /// merely assert that requested capabilities were granted.
    pub fn confirm_started<B: ProcessBoundary<Pid = P>>(
        &mut self, boundary: &B, ticket: LaunchTicket, pid: P, now: u64,
    ) -> Result<(), BridgeError> {
        let n = self.checked_index(ticket.id)?;
        if ticket.epoch != self.epoch { return Err(BridgeError::InvalidTicket); }
        let pending = self.launching[n].ok_or(BridgeError::InvalidTicket)?;
        if pending.intent.ticket != ticket { return Err(BridgeError::InvalidTicket); }
        if now > pending.intent.expires_at_ms { return Err(BridgeError::TicketExpired); }
        if self.state(ticket.id)? != ServiceState::Starting || self.cleaning[n].is_some() {
            return Err(BridgeError::InvalidState);
        }
        if self.bound.iter().flatten().any(|b| b.pid == pid) {
            return Err(BridgeError::DuplicatePid);
        }
        if !boundary.verify_started(
            ticket, pid, self.model.registry().manifest(ticket.id)
        ) || !boundary.is_live(pid) {
            return Err(BridgeError::KernelRejected);
        }
        self.model.model_started(ticket.id, now)?;
        self.bound[n] = Some(BoundProcess {pid, ticket});
        self.launching[n] = None;
        Ok(())
    }
    /// Authenticated exit. Stale or cross-service PID/ticket events are inert.
    /// A crash blocks all restart actions until teardown is acknowledged.
    pub fn notify_exit<B: ProcessBoundary<Pid = P>>(
        &mut self, boundary: &B, ticket: LaunchTicket, pid: P,
        reason: ExitReason, now: u64,
    ) -> Result<(), BridgeError> {
        let n = self.checked_index(ticket.id)?;
        let binding = self.bound[n].ok_or(BridgeError::InvalidTicket)?;
        if ticket.epoch != self.epoch || binding.ticket != ticket || binding.pid != pid {
            return Err(BridgeError::InvalidTicket);
        }
        if self.cleaning[n].is_some() { return Err(BridgeError::CleanupPending); }
        if !boundary.verify_exited(ticket, pid, reason) {
            return Err(BridgeError::KernelRejected);
        }
        match self.state(ticket.id)? {
            ServiceState::Stopping => {}
            ServiceState::Running | ServiceState::Starting => {
                self.model.model_crash(ticket.id, now)?;
            }
            _ => return Err(BridgeError::InvalidState),
        }
        self.stopping[n] = None;
        self.cleaning[n] = Some(PendingCleanup {
            ticket, pid: Some(pid), emitted: false,
        });
        Ok(())
    }
    /// A failed provider must not release its last supervised binding while
    /// transitive consumers still own unacknowledged processes or launches.
    fn active_required_dependents(&self, id: ServiceId) -> bool {
        let len = self.model.registry().len();
        let mut transitively_dependent = [false; MAX_SERVICES];
        transitively_dependent[id.index()] = true;
        for _ in 0..len {
            for child in 0..len {
                for parent in 0..len {
                    if transitively_dependent[parent] && self.model.registry().requires(
                        ServiceId(child as u8), ServiceId(parent as u8)
                    ) {
                        transitively_dependent[child] = true;
                    }
                }
            }
        }
        (0..len).any(|n| n != id.index() && transitively_dependent[n]
            && (self.bound[n].is_some()
                || self.launching[n].is_some()
                || self.stopping[n].is_some()
                || self.cleaning[n].is_some()))
    }
    /// A cleanup ACK is trusted only if the kernel confirms exit, handle
    /// revocation, queue closure, IRQ/DMA quiescence and resource reclaim.
    pub fn confirm_cleaned<B: ProcessBoundary<Pid = P>>(
        &mut self, boundary: &B, ticket: LaunchTicket, pid: Option<P>,
        now: u64,
    ) -> Result<(), BridgeError> {
        let n = self.checked_index(ticket.id)?;
        let cleanup = self.cleaning[n].ok_or(BridgeError::InvalidTicket)?;
        if ticket.epoch != self.epoch || cleanup.ticket != ticket || cleanup.pid != pid {
            return Err(BridgeError::InvalidTicket);
        }
        if self.active_required_dependents(ticket.id) {
            return Err(BridgeError::ActiveDependent);
        }
        if !boundary.verify_cleaned(ticket, pid) {
            return Err(BridgeError::KernelRejected);
        }
        if let Some(process) = pid {
            if boundary.is_live(process) { return Err(BridgeError::KernelRejected); }
        }
        if self.state(ticket.id)? == ServiceState::Stopping {
            self.model.model_stopped(ticket.id, now)?;
        }
        self.bound[n] = None;
        self.stopping[n] = None;
        self.cleaning[n] = None;
        Ok(())
    }
    /// Monotonic mock time only. This cannot issue process syscalls.
    pub fn tick(&mut self, now: u64) -> Result<(), BridgeError> {
        if now.checked_add(START_TIMEOUT_MS.max(STOP_TIMEOUT_MS)).is_none() {
            return Err(BridgeError::DeadlineOverflow);
        }
        // First expire unacknowledged launch intents. Their possible
        // in-flight launches MUST be canceled by kernel before retry.
        for n in 0..self.model.registry().len() {
            if let Some(pending) = self.launching[n] {
                if now >= pending.intent.expires_at_ms {
                    self.launching[n] = None;
                    let id = pending.intent.ticket.id;
                    match self.state(id)? {
                        ServiceState::Starting => self.model.model_crash(id, now)?,
                        ServiceState::Stopping => {}
                        _ => return Err(BridgeError::InconsistentModel),
                    }
                    self.cleaning[n] = Some(PendingCleanup {
                        ticket: pending.intent.ticket, pid: None, emitted: false,
                    });
                }
            }
            if let Some(stop) = &mut self.stopping[n] {
                if now >= stop.deadline_ms { stop.force_requested = true; }
            }
        }
        // Conservative global barrier: do not retry/start anything until all
        // failed process/launch resource reclamation ACKs are verified.
        if self.cleaning.iter().any(Option::is_some) { return Ok(()); }
        self.model.model_tick(now);
        let mut ordered = [ServiceId(0); MAX_SERVICES];
        for (i, id) in self.model.registry().startup_order().enumerate() {
            ordered[i] = id;
        }
        for id in ordered[..self.model.registry().len()].iter().copied() {
            if self.state(id)? == ServiceState::Starting
                && self.launching[id.index()].is_none()
                && self.bound[id.index()].is_none()
            {
                self.reserve_ticket(id, now)?;
            }
        }
        Ok(())
    }
    /// Deterministic action draining. Shutdown/cleanup first, reversed graph.
    /// A real kernel adapter must never consume these as arbitrary syscalls.
    pub fn next_action(&mut self, now: u64)
        -> Result<Option<SupervisorAction<P>>, BridgeError>
    {
        self.tick(now)?;
        let mut reversed = [ServiceId(0); MAX_SERVICES];
        for (i, id) in self.model.registry().shutdown_order().enumerate() {
            reversed[i] = id;
        }
        for id in reversed[..self.model.registry().len()].iter().copied() {
            let n = id.index();
            if self.state(id)? == ServiceState::Stopping
                && self.stopping[n].is_none()
                && self.cleaning[n].is_none()
            {
                self.mark_stopping_or_cancel(id, now)?;
            }
            if let Some(cleanup) = &mut self.cleaning[n] {
                if !cleanup.emitted && !self.active_required_dependents(id) {
                    cleanup.emitted = true;
                    return Ok(Some(SupervisorAction::Cleanup {
                        ticket: cleanup.ticket, pid: cleanup.pid,
                    }));
                }
            }
            if let Some(stop) = &mut self.stopping[n] {
                if let Some(binding) = self.bound[n] {
                    if !stop.emitted {
                        stop.emitted = true;
                        return Ok(Some(SupervisorAction::Stop {
                            ticket: binding.ticket, pid: binding.pid, force: false,
                        }));
                    }
                    if stop.force_requested && !stop.force_emitted {
                        stop.force_emitted = true;
                        return Ok(Some(SupervisorAction::Stop {
                            ticket: binding.ticket, pid: binding.pid, force: true,
                        }));
                    }
                }
            }
        }
        for id in self.model.registry().startup_order() {
            let n = id.index();
            if let Some(pending) = &mut self.launching[n] {
                if !pending.emitted {
                    pending.emitted = true;
                    return Ok(Some(SupervisorAction::Launch(pending.intent)));
                }
            }
        }
        Ok(None)
    }
    pub fn snapshot<B: ProcessBoundary<Pid = P>>(
        &self, boundary: &B,
    ) -> SupervisorSnapshot<P> {
        let mut entries = [None; MAX_SERVICES];
        let raw = self.model.snapshot();
        for info in raw.iter() {
            let n = info.id.index();
            // Do not present a dead/faulting PID as a running service PID.
            let bound = self.bound[n].filter(|b| {
                self.cleaning[n].is_none() && boundary.is_live(b.pid)
            });
            entries[n] = Some(SupervisorInfo {
                service: info,
                verified_pid: bound.map(|b| b.pid),
                launch_ticket: bound.map(|b| b.ticket),
                teardown_pending: self.cleaning[n].is_some(),
            });
        }
        SupervisorSnapshot { entries, len: raw.len() }
    }
    pub fn model_health(&mut self, id: ServiceId, health: Health, now: u64)
        -> Result<(), BridgeError>
    {
        self.checked_index(id)?;
        if self.cleaning[id.index()].is_some() {
            return Err(BridgeError::CleanupPending);
        }
        self.model.model_health(id, health, now).map_err(BridgeError::from)
    }
}
