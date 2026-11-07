//! Host-only negative and integration-model tests for typed ProcessManager handoff.
#[path = "../src/services/service_manager.rs"]
mod service_manager;
use service_manager::{supervisor::*, *};
use std::cell::{Cell, RefCell};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct KernelPid(u64);

struct FakeKernel {
    controller: Cell<bool>,
    grants: Cell<bool>,
    exit_auth: Cell<bool>,
    cleanup_auth: Cell<bool>,
    live: RefCell<std::vec::Vec<KernelPid>>,
}
impl FakeKernel {
    fn new() -> Self {
        Self {
            controller: Cell::new(true), grants: Cell::new(true),
            exit_auth: Cell::new(true), cleanup_auth: Cell::new(true),
            live: RefCell::new(vec![]),
        }
    }
    fn spawn(&self, pid: KernelPid) { self.live.borrow_mut().push(pid); }
    fn exited(&self, pid: KernelPid) {
        self.live.borrow_mut().retain(|x| *x != pid);
    }
}
impl ProcessBoundary for FakeKernel {
    type Pid = KernelPid;
    fn allow_control(&self, _: ControlOperation, _: &ServiceManifest) -> bool {
        self.controller.get()
    }
    fn verify_started(
        &self, _: LaunchTicket, pid: Self::Pid, manifest: &ServiceManifest
    ) -> bool {
        self.grants.get() && self.is_live(pid)
            && !manifest.requested_capabilities.iter().any(|right| *right == "device.claim")
    }
    fn verify_exited(
        &self, _: LaunchTicket, pid: Self::Pid, _: ExitReason
    ) -> bool {
        self.exit_auth.get() && !self.is_live(pid)
    }
    fn verify_cleaned(&self, _: LaunchTicket, pid: Option<Self::Pid>) -> bool {
        self.cleanup_auth.get() && pid.is_none_or(|p| !self.is_live(p))
    }
    fn is_live(&self, pid: Self::Pid) -> bool {
        self.live.borrow().contains(&pid)
    }
}
fn m(id: &'static str, path: &'static str) -> ServiceManifest {
    ServiceManifest {
        schema_version: MANIFEST_SCHEMA_V1, version: 1,
        id, executable: path, start: StartMode::OnBoot,
        required: &[], after: &[],
        requested_capabilities: &["ipc.send"],
        quotas: Quotas { memory_bytes: 4096, threads: 1 },
        restart: RestartPolicy::OnFailure {
            max_retries: 2, base_ms: 100, cap_ms: 1000,
        },
    }
}
fn one() -> ServiceSupervisor<KernelPid> {
    ServiceSupervisor::new(&[m("logd", "/System/Services/logd")], 77).unwrap()
}
fn launch(s: &mut ServiceSupervisor<KernelPid>, k: &FakeKernel, name: &str, now: u64)
    -> LaunchIntent
{
    let id = s.lookup(name).unwrap();
    s.request_start(k, id, now).unwrap();
    match s.next_action(now).unwrap().unwrap() {
        SupervisorAction::Launch(intent) => intent,
        _ => panic!("unexpected supervisor action"),
    }
}
fn started(s: &mut ServiceSupervisor<KernelPid>, k: &FakeKernel,
           name: &str, pid: KernelPid, now: u64) -> LaunchTicket {
    let intent = launch(s, k, name, now);
    k.spawn(pid);
    s.confirm_started(k, intent.ticket, pid, now+1).unwrap();
    intent.ticket
}

#[test]
fn typed_pid_is_only_kernel_verified_not_service_generated() {
    let k = FakeKernel::new();
    let mut s = one();
    let before = s.snapshot(&k);
    assert_eq!(before.len(), 1);
    assert_eq!(before.iter().next().unwrap().verified_pid, None);
    let intent = launch(&mut s, &k, "logd", 0);
    assert_eq!(intent.executable, "/System/Services/logd");
    assert_eq!(intent.requested_capabilities, ["ipc.send"]);
    assert_eq!(s.snapshot(&k).iter().next().unwrap().verified_pid, None);
    assert_eq!(s.model().execute_service(intent.ticket.service()),
        Err(ServiceError::Unsupported));
    k.spawn(KernelPid(100));
    s.confirm_started(&k, intent.ticket, KernelPid(100), 1).unwrap();
    assert_eq!(s.snapshot(&k).iter().next().unwrap().verified_pid, Some(KernelPid(100)));
}
#[test]
fn launch_permission_must_be_approved_by_kernel_grant_authority() {
    let k = FakeKernel::new();
    let mut s = one();
    k.controller.set(false);
    assert_eq!(s.request_start(&k, s.lookup("logd").unwrap(), 0),
        Err(BridgeError::AccessDenied));
    assert_eq!(s.model().snapshot().iter().next().unwrap().state, ServiceState::Validated);
    k.controller.set(true);
    let intent = launch(&mut s, &k, "logd", 1);
    k.spawn(KernelPid(1));
    k.grants.set(false);
    assert_eq!(s.confirm_started(&k, intent.ticket, KernelPid(1), 2),
        Err(BridgeError::KernelRejected));
    assert_eq!(s.snapshot(&k).iter().next().unwrap().verified_pid, None);
    k.grants.set(true);
    s.confirm_started(&k, intent.ticket, KernelPid(1), 3).unwrap();
}
#[test]
fn declared_but_unapproved_device_claim_cannot_launch() {
    let k = FakeKernel::new();
    let mut m = m("deviced", "/System/Services/deviced");
    m.requested_capabilities = &["device.claim"];
    let mut s = ServiceSupervisor::<KernelPid>::new(&[m], 1).unwrap();
    let intent = launch(&mut s, &k, "deviced", 0);
    k.spawn(KernelPid(4));
    assert_eq!(s.confirm_started(&k, intent.ticket, KernelPid(4), 1),
        Err(BridgeError::KernelRejected));
}
#[test]
fn forged_exit_and_stale_pid_tokens_have_no_effect() {
    let k = FakeKernel::new();
    let mut s = one();
    let ticket = started(&mut s, &k, "logd", KernelPid(15), 0);
    assert_eq!(s.notify_exit(&k, ticket, KernelPid(16), ExitReason::Fault, 2),
        Err(BridgeError::InvalidTicket));
    assert_eq!(s.notify_exit(&k, ticket, KernelPid(15), ExitReason::Fault, 2),
        Err(BridgeError::KernelRejected)); // still live, not an exit
    k.exited(KernelPid(15));
    k.exit_auth.set(false);
    assert_eq!(s.notify_exit(&k, ticket, KernelPid(15), ExitReason::Fault, 2),
        Err(BridgeError::KernelRejected));
    k.exit_auth.set(true);
    s.notify_exit(&k, ticket, KernelPid(15), ExitReason::Fault, 2).unwrap();
    assert_eq!(s.snapshot(&k).iter().next().unwrap().verified_pid, None);
    assert!(s.snapshot(&k).iter().next().unwrap().teardown_pending);
    assert_eq!(s.confirm_started(&k, ticket, KernelPid(15), 3),
        Err(BridgeError::InvalidTicket));
}
#[test]
fn cleanup_must_confirm_revocation_before_restart() {
    let k = FakeKernel::new();
    let mut s = one();
    let t = started(&mut s, &k, "logd", KernelPid(15), 0);
    k.exited(KernelPid(15));
    s.notify_exit(&k, t, KernelPid(15), ExitReason::Fault, 1).unwrap();
    assert_eq!(s.next_action(101).unwrap(), Some(SupervisorAction::Cleanup {
        ticket:t, pid:Some(KernelPid(15))
    }));
    assert_eq!(s.next_action(999).unwrap(), None); // no retry without cleanup
    k.cleanup_auth.set(false);
    assert_eq!(s.confirm_cleaned(&k, t, Some(KernelPid(15)), 1000),
        Err(BridgeError::KernelRejected));
    k.cleanup_auth.set(true);
    assert_eq!(s.confirm_cleaned(&k, t, Some(KernelPid(16)), 1000),
        Err(BridgeError::InvalidTicket));
    s.confirm_cleaned(&k, t, Some(KernelPid(15)), 1000).unwrap();
    assert!(matches!(s.next_action(1001).unwrap(), Some(SupervisorAction::Launch(_))));
}
#[test]
fn stopped_service_waits_for_kernel_exit_and_cleanup_ack() {
    let k = FakeKernel::new();
    let mut s = one();
    let ticket = started(&mut s, &k, "logd", KernelPid(27), 0);
    let id = s.lookup("logd").unwrap();
    s.request_stop(&k, id, 10).unwrap();
    assert_eq!(s.next_action(10).unwrap(), Some(SupervisorAction::Stop {
        ticket, pid:KernelPid(27), force:false
    }));
    assert_eq!(s.model().snapshot().iter().next().unwrap().state, ServiceState::Stopping);
    k.exited(KernelPid(27));
    s.notify_exit(&k, ticket, KernelPid(27), ExitReason::Killed, 11).unwrap();
    assert_eq!(s.model().snapshot().iter().next().unwrap().state, ServiceState::Stopping);
    assert_eq!(s.next_action(12).unwrap(), Some(SupervisorAction::Cleanup {
        ticket, pid:Some(KernelPid(27))
    }));
    s.confirm_cleaned(&k, ticket, Some(KernelPid(27)), 13).unwrap();
    assert_eq!(s.model().snapshot().iter().next().unwrap().state, ServiceState::Stopped);
    assert!(s.next_action(50).unwrap().is_none());
}
#[test]
fn stop_timeout_requests_force_termination_once() {
    let k = FakeKernel::new();
    let mut s = one();
    let ticket = started(&mut s, &k, "logd", KernelPid(10), 0);
    s.request_stop(&k, s.lookup("logd").unwrap(), 3).unwrap();
    assert_eq!(s.next_action(3).unwrap(), Some(SupervisorAction::Stop {
        ticket, pid:KernelPid(10), force:false
    }));
    assert_eq!(s.next_action(5_002).unwrap(), None);
    assert_eq!(s.next_action(5_003).unwrap(), Some(SupervisorAction::Stop {
        ticket, pid:KernelPid(10), force:true
    }));
    assert_eq!(s.next_action(9_999).unwrap(), None);
    assert_eq!(s.model().snapshot().iter().next().unwrap().state, ServiceState::Stopping);
}
#[test]
fn unacknowledged_start_times_out_and_requires_abort_before_retry() {
    let k = FakeKernel::new();
    let mut s = one();
    let intent = launch(&mut s, &k, "logd", 0);
    assert_eq!(s.next_action(4_999).unwrap(), None);
    assert_eq!(s.next_action(5_000).unwrap(), Some(SupervisorAction::Cleanup {
        ticket: intent.ticket, pid:None,
    }));
    k.spawn(KernelPid(123)); // late kernel work cannot be accepted
    assert_eq!(s.confirm_started(&k, intent.ticket, KernelPid(123), 5_001),
        Err(BridgeError::InvalidTicket));
    // kernel must kill and revoke even an unreported late-start task.
    k.exited(KernelPid(123));
    assert_eq!(s.next_action(20_000).unwrap(), None);
    s.confirm_cleaned(&k, intent.ticket, None, 20_001).unwrap();
    assert!(matches!(s.next_action(20_002).unwrap(), Some(SupervisorAction::Launch(_))));
}
#[test]
fn restart_requires_old_process_stopped_before_reuse() {
    let k = FakeKernel::new();
    let mut s = one();
    let old = started(&mut s, &k, "logd", KernelPid(5), 0);
    let id = s.lookup("logd").unwrap();
    s.request_restart(&k, id, 2).unwrap();
    assert_eq!(s.next_action(2).unwrap(), Some(SupervisorAction::Stop {
        ticket:old, pid:KernelPid(5), force:false
    }));
    assert_eq!(s.next_action(2).unwrap(), None);
    k.exited(KernelPid(5));
    s.notify_exit(&k, old, KernelPid(5), ExitReason::Killed, 3).unwrap();
    assert!(matches!(s.next_action(3).unwrap(), Some(SupervisorAction::Cleanup {..})));
    s.confirm_cleaned(&k, old, Some(KernelPid(5)), 4).unwrap();
    let new = match s.next_action(4).unwrap() {
        Some(SupervisorAction::Launch(intent)) => intent.ticket,
        x => panic!("expected fresh launch, got {x:?}"),
    };
    assert_ne!(new.generation(), old.generation());
    assert_eq!(s.confirm_started(&k, old, KernelPid(5), 5), Err(BridgeError::InvalidTicket));
    k.spawn(KernelPid(5)); // even PID reuse cannot replay an old ticket
    s.confirm_started(&k, new, KernelPid(5), 5).unwrap();
}
#[test]
fn restart_storm_reaches_terminal_failure_not_infinite_retry() {
    let k = FakeKernel::new();
    let mut s = one();
    let mut ticket = started(&mut s, &k, "logd", KernelPid(1), 0);
    for iteration in 0..3 {
        let now = 10 + iteration * 1000;
        k.exited(KernelPid(1));
        s.notify_exit(&k, ticket, KernelPid(1), ExitReason::Fault, now).unwrap();
        assert!(matches!(s.next_action(now).unwrap(), Some(SupervisorAction::Cleanup {..})));
        s.confirm_cleaned(&k, ticket, Some(KernelPid(1)), now+1).unwrap();
        if iteration < 2 {
            let next = match s.next_action(now+500).unwrap() {
                Some(SupervisorAction::Launch(i)) => i.ticket,
                other => panic!("expected retry {iteration}: {other:?}"),
            };
            k.spawn(KernelPid(1));
            s.confirm_started(&k, next, KernelPid(1), now+501).unwrap();
            ticket = next;
        }
    }
    assert_eq!(s.model().snapshot().iter().next().unwrap().state, ServiceState::Failed);
    assert_eq!(s.next_action(100_000).unwrap(), None);
}
#[test]
fn dependency_failure_stops_consumers_before_restarting_provider() {
    let k = FakeKernel::new();
    let provider = m("deviced", "/System/Services/deviced");
    let mut consumer = m("displayd", "/System/Services/displayd");
    consumer.required = &["deviced"];
    let mut s = ServiceSupervisor::<KernelPid>::new(&[consumer, provider], 7).unwrap();
    s.request_start(&k, s.lookup("displayd").unwrap(), 0).unwrap();
    assert_eq!(s.next_action(0).unwrap(), None);
    let provider_t = started(&mut s, &k, "deviced", KernelPid(1), 1);
    // model_tick promotes already requested consumer
    let display_intent = match s.next_action(3).unwrap() {
        Some(SupervisorAction::Launch(i)) => i, x => panic!("expected display launch: {x:?}")
    };
    k.spawn(KernelPid(2));
    s.confirm_started(&k, display_intent.ticket, KernelPid(2), 4).unwrap();
    k.exited(KernelPid(1));
    s.notify_exit(&k, provider_t, KernelPid(1), ExitReason::Fault, 5).unwrap();
    assert_eq!(s.next_action(6).unwrap(), Some(SupervisorAction::Stop {
        ticket:display_intent.ticket, pid:KernelPid(2), force:false
    }));
    assert_eq!(s.next_action(6).unwrap(), Some(SupervisorAction::Cleanup {
        ticket:provider_t, pid:Some(KernelPid(1))
    }));
    assert_eq!(s.next_action(1000).unwrap(), None);
    k.exited(KernelPid(2));
    s.notify_exit(&k, display_intent.ticket, KernelPid(2), ExitReason::Killed, 1001).unwrap();
    assert!(matches!(s.next_action(1002).unwrap(), Some(SupervisorAction::Cleanup { .. })));
    s.confirm_cleaned(&k, display_intent.ticket, Some(KernelPid(2)), 1003).unwrap();
    s.confirm_cleaned(&k, provider_t, Some(KernelPid(1)), 1003).unwrap();
    assert!(matches!(s.next_action(1004).unwrap(), Some(SupervisorAction::Launch(_))));
}
#[test]
fn rejected_duplicate_pid_and_foreign_supervisor_epoch() {
    let k = FakeKernel::new();
    let a = m("alpha", "/System/Services/alpha");
    let b = m("beta", "/System/Services/beta");
    let mut s = ServiceSupervisor::<KernelPid>::new(&[a,b], 10).unwrap();
    let ta = started(&mut s, &k, "alpha", KernelPid(1), 0);
    let ib = launch(&mut s, &k, "beta", 3);
    assert_eq!(s.confirm_started(&k, ib.ticket, KernelPid(1), 4),
        Err(BridgeError::DuplicatePid));
    let mut another = ServiceSupervisor::<KernelPid>::new(&[a,b], 11).unwrap();
    another.request_start(&k, another.lookup("alpha").unwrap(), 0).unwrap();
    assert_eq!(another.confirm_started(&k, ta, KernelPid(1), 1),
        Err(BridgeError::InvalidTicket));
}
#[test]
fn missing_dependency_and_cycle_are_rejected_even_with_bridge() {
    let mut m1 = m("alpha", "/System/Services/alpha");
    m1.required = &["beta"];
    assert!(matches!(ServiceSupervisor::<KernelPid>::new(&[m1], 1),
        Err(ValidationError::MissingRequired)));
    let mut m2 = m("beta", "/System/Services/beta");
    m2.required = &["alpha"];
    assert!(matches!(ServiceSupervisor::<KernelPid>::new(&[m1,m2], 1),
        Err(ValidationError::DependencyCycle)));
}
#[test]
fn bad_epoch_and_clock_overflow_fail_closed() {
    let manifests = [m("logd","/System/Services/logd")];
    assert!(matches!(ServiceSupervisor::<KernelPid>::new(&manifests, 0),
        Err(ValidationError::InvalidVersion)));
    let k=FakeKernel::new();
    let mut s=one();
    assert_eq!(s.request_start(&k,s.lookup("logd").unwrap(),u64::MAX),
        Err(BridgeError::DeadlineOverflow));
    assert_eq!(s.model().snapshot().iter().next().unwrap().state, ServiceState::Validated);
}
