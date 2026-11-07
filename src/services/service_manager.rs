//! Pure, bounded service supervisor MODEL. No process creation, PID ownership,
//! privileged execution, kernel clock, IPC or capability grants.
//!
//! Standalone host tests import this file by path until an integration owner
//! enables it in src/services/mod.rs. Safe for no_std (no heap or unsafe).
#![allow(dead_code)]

pub const MAX_SERVICES: usize = 16;
pub const MAX_DEPENDENCIES: usize = 8;
pub const MAX_CAPABILITIES: usize = 8;
pub const MAX_EVENTS: usize = 32;
pub const MANIFEST_SCHEMA_V1: u16 = 1;
pub const MIN_MEMORY_BYTES: u64 = 4096;
pub const MAX_MEMORY_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_THREADS: u8 = 32;
const STABLE_RESET_MS: u64 = 60_000;
const EXEC_PREFIX: &str = "/System/Services/";

/// Labels are requests only, never capability grants.
pub const KNOWN_CAPABILITIES: &[&str] = &[
    "device.observe", "device.claim", "display.mode.test",
    "gpu.surface.create", "gpu.surface.present", "ipc.connect",
    "ipc.send", "ipc.receive", "log.read", "process.observe",
    "fs.read", "fs.write", "network.configure", "audio.output",
    "power.observe", "update.stage", "service.observe",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceId(u8);
impl ServiceId {
    pub const fn index(self) -> usize { self.0 as usize }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartMode { OnBoot, OnDemand, Manual }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartPolicy {
    Never,
    OnFailure { max_retries: u8, base_ms: u64, cap_ms: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quotas { pub memory_bytes: u64, pub threads: u8 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceManifest {
    pub schema_version: u16,
    pub version: u32,
    pub id: &'static str,
    /// A future *allowlisted* native executable identity, not a runnable path.
    pub executable: &'static str,
    pub start: StartMode,
    pub required: &'static [&'static str],
    /// Soft ordering edge if present. Does not start or require that service.
    pub after: &'static [&'static str],
    pub requested_capabilities: &'static [&'static str],
    pub quotas: Quotas,
    pub restart: RestartPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationError {
    EmptyRegistry, TooManyServices, InvalidSchema, InvalidVersion, InvalidId,
    InvalidExecutable, DuplicateId, TooManyDependencies,
    DuplicateDependency, SelfDependency, MissingRequired,
    DependencyCycle, TooManyCapabilities, UnknownCapability,
    DuplicateCapability, InvalidQuota, InvalidRestartPolicy,
}

fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty() && bytes.len() <= 32
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|b| b.is_ascii_lowercase()
            || b.is_ascii_digit() || *b == b'_' || *b == b'-')
}

fn validate_manifest(m: ServiceManifest) -> Result<(), ValidationError> {
    if m.schema_version != MANIFEST_SCHEMA_V1 {
        return Err(ValidationError::InvalidSchema);
    }
    if m.version == 0 { return Err(ValidationError::InvalidVersion); }
    if !valid_id(m.id) { return Err(ValidationError::InvalidId); }
    if m.executable.strip_prefix(EXEC_PREFIX) != Some(m.id) {
        return Err(ValidationError::InvalidExecutable);
    }
    if m.required.len() > MAX_DEPENDENCIES || m.after.len() > MAX_DEPENDENCIES {
        return Err(ValidationError::TooManyDependencies);
    }
    for (group_index, group) in [m.required, m.after].iter().enumerate() {
        for (i, dep) in group.iter().enumerate() {
            if !valid_id(dep) { return Err(ValidationError::InvalidId); }
            if *dep == m.id { return Err(ValidationError::SelfDependency); }
            if group[..i].contains(dep) {
                return Err(ValidationError::DuplicateDependency);
            }
            if group_index == 1 && m.required.contains(dep) {
                return Err(ValidationError::DuplicateDependency);
            }
        }
    }
    if m.requested_capabilities.len() > MAX_CAPABILITIES {
        return Err(ValidationError::TooManyCapabilities);
    }
    for (i, cap) in m.requested_capabilities.iter().enumerate() {
        if !KNOWN_CAPABILITIES.contains(cap) {
            return Err(ValidationError::UnknownCapability);
        }
        if m.requested_capabilities[..i].contains(cap) {
            return Err(ValidationError::DuplicateCapability);
        }
    }
    if m.quotas.memory_bytes < MIN_MEMORY_BYTES
        || m.quotas.memory_bytes > MAX_MEMORY_BYTES
        || m.quotas.threads == 0 || m.quotas.threads > MAX_THREADS
    { return Err(ValidationError::InvalidQuota); }
    if let RestartPolicy::OnFailure { max_retries, base_ms, cap_ms } = m.restart {
        if max_retries == 0 || max_retries > 16
            || base_ms == 0 || cap_ms < base_ms || cap_ms > 3_600_000
        { return Err(ValidationError::InvalidRestartPolicy); }
    }
    Ok(())
}

/// Fully validated, stable-ordered graph. IDs are local indices, NOT PIDs.
pub struct ServiceRegistry {
    manifests: [Option<ServiceManifest>; MAX_SERVICES],
    count: usize,
    order: [ServiceId; MAX_SERVICES],
}

impl ServiceRegistry {
    pub fn validate(manifests: &[ServiceManifest]) -> Result<Self, ValidationError> {
        if manifests.is_empty() { return Err(ValidationError::EmptyRegistry); }
        if manifests.len() > MAX_SERVICES {
            return Err(ValidationError::TooManyServices);
        }
        let mut registry = Self {
            manifests: [None; MAX_SERVICES],
            count: manifests.len(),
            order: [ServiceId(0); MAX_SERVICES],
        };
        for (i, m) in manifests.iter().copied().enumerate() {
            validate_manifest(m)?;
            if manifests[..i].iter().any(|p| p.id == m.id) {
                return Err(ValidationError::DuplicateId);
            }
            registry.manifests[i] = Some(m);
        }
        for m in manifests {
            for dep in m.required {
                if registry.find(dep).is_none() {
                    return Err(ValidationError::MissingRequired);
                }
            }
        }
        // Kahn's algorithm with lexical tie-breaking, independent of input order.
        let mut chosen = [false; MAX_SERVICES];
        for position in 0..registry.count {
            let mut next: Option<usize> = None;
            for candidate in 0..registry.count {
                if chosen[candidate] { continue; }
                let has_pending_predecessor = (0..registry.count).any(|parent| {
                    !chosen[parent] && registry.edge(candidate, parent)
                });
                if has_pending_predecessor { continue; }
                if next.is_none_or(|old| {
                    registry.manifest(ServiceId(candidate as u8)).id
                        < registry.manifest(ServiceId(old as u8)).id
                }) { next = Some(candidate); }
            }
            let idx = next.ok_or(ValidationError::DependencyCycle)?;
            registry.order[position] = ServiceId(idx as u8);
            chosen[idx] = true;
        }
        Ok(registry)
    }

    fn edge(&self, child: usize, parent: usize) -> bool {
        let m = self.manifest(ServiceId(child as u8));
        let name = self.manifest(ServiceId(parent as u8)).id;
        m.required.contains(&name) || m.after.contains(&name)
    }

    pub const fn len(&self) -> usize { self.count }
    pub const fn is_empty(&self) -> bool { self.count == 0 }
    pub fn find(&self, name: &str) -> Option<ServiceId> {
        (0..self.count).find(|&i| {
            self.manifests[i].is_some_and(|m| m.id == name)
        }).map(|i| ServiceId(i as u8))
    }
    pub fn manifest(&self, id: ServiceId) -> &ServiceManifest {
        // Only IDs returned by this registry should be passed.
        self.manifests[id.index()].as_ref().expect("valid registry ID")
    }
    pub fn startup_order(&self) -> impl DoubleEndedIterator<Item = ServiceId> + '_ {
        self.order[..self.count].iter().copied()
    }
    pub fn shutdown_order(&self) -> impl Iterator<Item = ServiceId> + '_ {
        self.startup_order().rev()
    }
    pub fn requires(&self, child: ServiceId, parent: ServiceId) -> bool {
        self.manifest(child).required.contains(&self.manifest(parent).id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceState {
    Validated, WaitingDeps, Starting, Running, Backoff,
    Stopping, Stopped, Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health { Unknown, Healthy, Degraded, Unhealthy }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceError {
    UnknownService, AccessDenied, InvalidTransition, DependencyUnavailable,
    ActiveDependent, ClockOverflow, Unsupported, EventsLost(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    StartRequested, Started, StopRequested, Stopped, Crashed,
    BackoffScheduled, RetriesExhausted, DependencyLost, HealthChanged,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceEvent {
    pub sequence: u64,
    pub at_ms: u64,
    pub id: ServiceId,
    pub kind: EventKind,
}
/// Local model cursor; NOT an authenticated IPC subscription.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventCursor { next_sequence: u64 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Runtime {
    state: ServiceState,
    health: Health,
    retry_count: u8,
    retry_at_ms: Option<u64>,
    running_since_ms: Option<u64>,
    restart_after_stop: bool,
}
impl Runtime {
    const fn initial() -> Self {
        Self {
            state: ServiceState::Validated, health: Health::Unknown,
            retry_count: 0, retry_at_ms: None,
            running_since_ms: None, restart_after_stop: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceInfo {
    pub id: ServiceId,
    pub name: &'static str,
    pub state: ServiceState,
    pub health: Health,
    pub retries_used: u8,
    pub next_retry_ms: Option<u64>,
    // Deliberately NO PID: only ProcessManager can publish a real one.
}
/// Immutable owned copy, not a writable reference into ServiceManager.
pub struct ServiceSnapshot {
    entries: [Option<ServiceInfo>; MAX_SERVICES],
    len: usize,
}
impl ServiceSnapshot {
    pub const fn len(&self) -> usize { self.len }
    pub fn iter(&self) -> impl Iterator<Item = ServiceInfo> + '_ {
        self.entries[..self.len].iter().flatten().copied()
    }
}

pub struct ServiceManager {
    registry: ServiceRegistry,
    runtime: [Runtime; MAX_SERVICES],
    events: [Option<ServiceEvent>; MAX_EVENTS],
    event_sequence: u64,
    event_count: usize,
}

impl ServiceManager {
    pub fn new(manifests: &[ServiceManifest]) -> Result<Self, ValidationError> {
        Ok(Self {
            registry: ServiceRegistry::validate(manifests)?,
            runtime: [Runtime::initial(); MAX_SERVICES],
            events: [None; MAX_EVENTS], event_sequence: 0, event_count: 0,
        })
    }
    pub const fn registry(&self) -> &ServiceRegistry { &self.registry }
    pub fn lookup(&self, name: &str) -> Result<ServiceId, ServiceError> {
        self.registry.find(name).ok_or(ServiceError::UnknownService)
    }
    pub fn snapshot(&self) -> ServiceSnapshot {
        let mut entries = [None; MAX_SERVICES];
        for (i, slot) in entries.iter_mut().enumerate().take(self.registry.len()) {
            let id = ServiceId(i as u8);
            let runtime = self.runtime[i];
            *slot = Some(ServiceInfo {
                id, name: self.registry.manifest(id).id,
                state: runtime.state, health: runtime.health,
                retries_used: runtime.retry_count,
                next_retry_ms: runtime.retry_at_ms,
            });
        }
        ServiceSnapshot { entries, len: self.registry.len() }
    }
    pub fn subscribe(&self) -> EventCursor {
        EventCursor { next_sequence: self.event_sequence }
    }
    /// A lagging reader receives EventsLost and a resynced cursor.
    pub fn poll(&self, cursor: &mut EventCursor)
        -> Result<Option<ServiceEvent>, ServiceError>
    {
        let oldest = self.event_sequence - self.event_count as u64;
        if cursor.next_sequence < oldest {
            let missed = oldest - cursor.next_sequence;
            cursor.next_sequence = oldest;
            return Err(ServiceError::EventsLost(missed));
        }
        if cursor.next_sequence >= self.event_sequence { return Ok(None); }
        let position = (cursor.next_sequence as usize) % MAX_EVENTS;
        cursor.next_sequence += 1;
        Ok(self.events[position])
    }
    fn emit(&mut self, id: ServiceId, kind: EventKind, at_ms: u64) {
        // Exhausting u64 event IDs is fail-closed: do not silently wrap.
        if self.event_sequence == u64::MAX { return; }
        let seq = self.event_sequence;
        self.events[(seq as usize) % MAX_EVENTS] = Some(ServiceEvent {
            sequence: seq, at_ms, id, kind,
        });
        self.event_sequence += 1;
        self.event_count = (self.event_count + 1).min(MAX_EVENTS);
    }
    fn dependencies_running(&self, id: ServiceId) -> bool {
        (0..self.registry.len()).all(|n| {
            let parent = ServiceId(n as u8);
            !self.registry.requires(id, parent)
                || self.runtime[n].state == ServiceState::Running
        })
    }
    /// This is intentionally never a real launcher, including for builtins.
    pub fn execute_service(&self, _id: ServiceId) -> Result<(), ServiceError> {
        Err(ServiceError::Unsupported)
    }
    fn authorize(control: bool) -> Result<(), ServiceError> {
        if control { Ok(()) } else { Err(ServiceError::AccessDenied) }
    }
    /// Simulate a supervisor start request; ONLY a trusted mock may ack started.
    pub fn model_start(&mut self, id: ServiceId, control: bool, now: u64)
        -> Result<(), ServiceError>
    {
        Self::authorize(control)?;
        let rt = &mut self.runtime[id.index()];
        if !matches!(rt.state, ServiceState::Validated | ServiceState::Stopped) {
            return Err(ServiceError::InvalidTransition);
        }
        if self.dependencies_running(id) {
            self.runtime[id.index()].state = ServiceState::Starting;
            self.emit(id, EventKind::StartRequested, now);
        } else {
            self.runtime[id.index()].state = ServiceState::WaitingDeps;
        }
        Ok(())
    }
    pub fn model_started(&mut self, id: ServiceId, now: u64)
        -> Result<(), ServiceError>
    {
        if self.runtime[id.index()].state != ServiceState::Starting {
            return Err(ServiceError::InvalidTransition);
        }
        if !self.dependencies_running(id) {
            return Err(ServiceError::DependencyUnavailable);
        }
        let rt = &mut self.runtime[id.index()];
        rt.state = ServiceState::Running;
        rt.health = Health::Unknown;
        rt.retry_at_ms = None;
        rt.running_since_ms = Some(now);
        self.emit(id, EventKind::Started, now);
        Ok(())
    }
    pub fn model_stop(&mut self, id: ServiceId, control: bool, now: u64)
        -> Result<(), ServiceError>
    {
        Self::authorize(control)?;
        for n in 0..self.registry.len() {
            if self.registry.requires(ServiceId(n as u8), id)
                && matches!(self.runtime[n].state,
                    ServiceState::Running | ServiceState::Starting
                    | ServiceState::Stopping)
            { return Err(ServiceError::ActiveDependent); }
        }
        match self.runtime[id.index()].state {
            ServiceState::Running | ServiceState::Starting => {
                self.runtime[id.index()].state = ServiceState::Stopping;
                self.emit(id, EventKind::StopRequested, now);
            }
            ServiceState::Validated | ServiceState::WaitingDeps
            | ServiceState::Backoff | ServiceState::Failed => {
                self.runtime[id.index()] = Runtime {
                    state: ServiceState::Stopped, ..Runtime::initial()
                };
                self.emit(id, EventKind::Stopped, now);
            }
            _ => return Err(ServiceError::InvalidTransition),
        }
        Ok(())
    }
    pub fn model_stopped(&mut self, id: ServiceId, now: u64)
        -> Result<(), ServiceError>
    {
        if self.runtime[id.index()].state != ServiceState::Stopping {
            return Err(ServiceError::InvalidTransition);
        }
        let restart = self.runtime[id.index()].restart_after_stop;
        self.runtime[id.index()] = Runtime {
            state: ServiceState::Stopped, ..Runtime::initial()
        };
        self.emit(id, EventKind::Stopped, now);
        if restart { self.model_start(id, true, now)?; }
        Ok(())
    }
    pub fn model_restart(&mut self, id: ServiceId, control: bool, now: u64)
        -> Result<(), ServiceError>
    {
        Self::authorize(control)?;
        if matches!(self.runtime[id.index()].state,
            ServiceState::Running | ServiceState::Starting)
        {
            // Stop then start, never two overlapping mock instances.
            for n in 0..self.registry.len() {
                if self.registry.requires(ServiceId(n as u8), id)
                    && matches!(self.runtime[n].state,
                        ServiceState::Running | ServiceState::Starting
                        | ServiceState::Stopping)
                { return Err(ServiceError::ActiveDependent); }
            }
            self.runtime[id.index()].restart_after_stop = true;
            self.runtime[id.index()].state = ServiceState::Stopping;
            self.emit(id, EventKind::StopRequested, now);
            Ok(())
        } else {
            self.model_start(id, true, now)
        }
    }
    pub fn model_health(&mut self, id: ServiceId, health: Health, now: u64)
        -> Result<(), ServiceError>
    {
        if self.runtime[id.index()].state != ServiceState::Running {
            return Err(ServiceError::InvalidTransition);
        }
        let rt = &mut self.runtime[id.index()];
        rt.health = health;
        if health == Health::Healthy
            && rt.running_since_ms.is_some_and(|start| {
                now.checked_sub(start).is_some_and(|elapsed| elapsed >= STABLE_RESET_MS)
            })
        { rt.retry_count = 0; }
        self.emit(id, EventKind::HealthChanged, now);
        Ok(())
    }

    /// A mock process-failure event. Callers MUST NOT supply/forge a PID.
    pub fn model_crash(&mut self, id: ServiceId, now: u64)
        -> Result<(), ServiceError>
    {
        let index = id.index();
        if !matches!(self.runtime[index].state,
            ServiceState::Running | ServiceState::Starting)
        { return Err(ServiceError::InvalidTransition); }
        let manifest = self.registry.manifest(id);
        let retries = self.runtime[index].retry_count;
        let (state, deadline, new_retries) = match manifest.restart {
            RestartPolicy::Never => (ServiceState::Failed, None, retries),
            RestartPolicy::OnFailure { max_retries, base_ms, cap_ms } => {
                if retries >= max_retries {
                    (ServiceState::Failed, None, retries)
                } else {
                    let mut delay = base_ms;
                    for _ in 0..retries { delay = delay.saturating_mul(2).min(cap_ms); }
                    let deadline = now.checked_add(delay.min(cap_ms))
                        .ok_or(ServiceError::ClockOverflow)?;
                    (ServiceState::Backoff, Some(deadline), retries + 1)
                }
            }
        };
        self.runtime[index].state = state;
        self.runtime[index].health = Health::Unhealthy;
        self.runtime[index].retry_at_ms = deadline;
        self.runtime[index].running_since_ms = None;
        self.runtime[index].restart_after_stop = false;
        self.runtime[index].retry_count = new_retries;
        self.emit(id, EventKind::Crashed, now);
        self.emit(id, if state == ServiceState::Backoff {
            EventKind::BackoffScheduled
        } else { EventKind::RetriesExhausted }, now);

        // Dependency failure propagates transitively, consumers first.
        let mut lost = [false; MAX_SERVICES];
        lost[index] = true;
        for _ in 0..self.registry.len() {
            for n in 0..self.registry.len() {
                for p in 0..self.registry.len() {
                    if lost[p] && self.registry.requires(
                        ServiceId(n as u8), ServiceId(p as u8)
                    ) { lost[n] = true; }
                }
            }
        }
        let order: [ServiceId; MAX_SERVICES] = {
            let mut a = [ServiceId(0); MAX_SERVICES];
            for (i, s) in self.registry.shutdown_order().enumerate() { a[i] = s; }
            a
        };
        for child in order[..self.registry.len()].iter().copied() {
            if child == id || !lost[child.index()] { continue; }
            let state = self.runtime[child.index()].state;
            match state {
                ServiceState::Running | ServiceState::Starting => {
                    self.runtime[child.index()].state = ServiceState::Stopping;
                    self.runtime[child.index()].restart_after_stop = false;
                    self.emit(child, EventKind::DependencyLost, now);
                    self.emit(child, EventKind::StopRequested, now);
                }
                ServiceState::Backoff => {
                    self.runtime[child.index()].state = ServiceState::WaitingDeps;
                    self.runtime[child.index()].retry_at_ms = None;
                    self.emit(child, EventKind::DependencyLost, now);
                }
                _ => {}
            }
        }
        Ok(())
    }
    /// Deterministic monotonic-clock driver for tests, no busy waiting.
    pub fn model_tick(&mut self, now: u64) {
        let mut order = [ServiceId(0); MAX_SERVICES];
        for (i, id) in self.registry.startup_order().enumerate() { order[i] = id; }
        for id in order[..self.registry.len()].iter().copied() {
            let rt = self.runtime[id.index()];
            let due = rt.state == ServiceState::Backoff
                && rt.retry_at_ms.is_some_and(|d| now >= d);
            let unblocked = rt.state == ServiceState::WaitingDeps;
            if (due || unblocked) && self.dependencies_running(id) {
                self.runtime[id.index()].state = ServiceState::Starting;
                self.runtime[id.index()].retry_at_ms = None;
                self.emit(id, EventKind::StartRequested, now);
            } else if due {
                self.runtime[id.index()].state = ServiceState::WaitingDeps;
                self.runtime[id.index()].retry_at_ms = None;
            }
        }
    }
}

#[path = "service_manifests.rs"]
pub mod manifests;

#[path = "service_supervisor.rs"]
pub mod supervisor;
