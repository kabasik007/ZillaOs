//! Compile and exercise the typed ServiceSupervisor with the canonical
//! ZillaOS kernel process::Pid, without linking real kernel bootstrap.
#[path = "../src/kernel/process/mod.rs"]
mod process;
#[path = "../src/services/service_manager.rs"]
mod service_manager;

use process::{Pid, ProcessTable, ProcessState};
use service_manager::{supervisor::*, *};
use std::cell::RefCell;

struct CanonicalProcessGateway {
    // This fixture instantiates the EXISTING ProcessTable type solely as
    // a test mock. No second production process registry is introduced.
    kernel_table: RefCell<ProcessTable>,
}
impl CanonicalProcessGateway {
    fn new() -> Self { Self { kernel_table: RefCell::new(ProcessTable::new()) } }
    fn reserve_only(&self, name: &'static str) -> Pid {
        self.kernel_table.borrow_mut().create(name, 0x400000, 0x800000, 8).unwrap()
    }
    fn exit_metadata(&self, pid: Pid) {
        self.kernel_table.borrow_mut().kill(pid).unwrap();
    }
    fn reap_metadata(&self, pid: Pid) {
        self.kernel_table.borrow_mut().reap(pid).unwrap();
    }
}
impl ProcessBoundary for CanonicalProcessGateway {
    type Pid = Pid;
    fn allow_control(&self, _: ControlOperation, _: &ServiceManifest) -> bool {
        true // test-owned kernel mock, not a real capability check
    }
    fn verify_started(&self, _: LaunchTicket, pid: Pid, m: &ServiceManifest) -> bool {
        self.kernel_table.borrow().process(pid).is_some_and(|p|
            p.name == m.id && p.state != ProcessState::Exited)
    }
    fn verify_exited(&self, _: LaunchTicket, pid: Pid, _: ExitReason) -> bool {
        self.kernel_table.borrow().process(pid).is_some_and(|p|
            p.state == ProcessState::Exited)
    }
    fn verify_cleaned(&self, _: LaunchTicket, pid: Option<Pid>) -> bool {
        pid.is_none_or(|p| self.kernel_table.borrow().process(p).is_none())
    }
    fn is_live(&self, pid: Pid) -> bool {
        self.kernel_table.borrow().process(pid).is_some_and(|p|
            p.state != ProcessState::Exited)
    }
}
fn manifest() -> ServiceManifest {
    ServiceManifest {
        schema_version: MANIFEST_SCHEMA_V1, version: 1,
        id: "logd", executable: "/System/Services/logd",
        start: StartMode::Manual, required: &[], after: &[],
        requested_capabilities: &["log.read"],
        quotas: Quotas { memory_bytes: 1024 * 1024, threads: 1 },
        restart: RestartPolicy::Never,
    }
}
#[test]
fn canonical_process_pid_is_accepted_only_with_authoritative_process_record() {
    let kernel = CanonicalProcessGateway::new();
    let mut supervisor = ServiceSupervisor::<Pid>::new(&[manifest()], 456).unwrap();
    let id = supervisor.lookup("logd").unwrap();
    supervisor.request_start(&kernel, id, 0).unwrap();
    let launch = match supervisor.next_action(0).unwrap() {
        Some(SupervisorAction::Launch(intent)) => intent,
        other => panic!("expected launch intent, got {other:?}"),
    };
    assert_eq!(supervisor.snapshot(&kernel).iter().next().unwrap().verified_pid, None);
    let forged = Pid(555);
    assert_eq!(supervisor.confirm_started(&kernel, launch.ticket, forged, 1),
        Err(BridgeError::KernelRejected));
    let canonical = kernel.reserve_only("logd");
    supervisor.confirm_started(&kernel, launch.ticket, canonical, 2).unwrap();
    assert_eq!(supervisor.snapshot(&kernel).iter().next().unwrap().verified_pid,
        Some(canonical));
    // Existing process table owns the actual exit metadata.
    kernel.exit_metadata(canonical);
    supervisor.notify_exit(&kernel, launch.ticket, canonical, ExitReason::Killed, 3).unwrap();
    assert_eq!(supervisor.snapshot(&kernel).iter().next().unwrap().verified_pid, None);
    assert!(matches!(supervisor.next_action(4).unwrap(),
        Some(SupervisorAction::Cleanup { pid: Some(p), .. }) if p == canonical));
    // A killed but *unreaped* kernel PID is not a completed cleanup.
    assert_eq!(supervisor.confirm_cleaned(&kernel, launch.ticket, Some(canonical), 5),
        Err(BridgeError::KernelRejected));
    kernel.reap_metadata(canonical);
    supervisor.confirm_cleaned(&kernel, launch.ticket, Some(canonical), 6).unwrap();
    assert_eq!(supervisor.model().snapshot().iter().next().unwrap().state,
        ServiceState::Failed);
}
