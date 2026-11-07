//! Standalone host-only model tests, intentionally not wired into kernel bootstrap.
//! Run: cargo test --test service_manager_host --target x86_64-unknown-linux-gnu
#[path = "../src/services/service_manager.rs"]
mod service_manager;
use service_manager::*;

const DEFAULT_QUOTAS: Quotas = Quotas { memory_bytes: 1024 * 1024, threads: 2 };
fn m(id: &'static str, executable: &'static str) -> ServiceManifest {
    ServiceManifest {
        schema_version: MANIFEST_SCHEMA_V1,
        version: 1,
        id, executable, start: StartMode::OnBoot,
        required: &[], after: &[],
        requested_capabilities: &[],
        quotas: DEFAULT_QUOTAS, restart: RestartPolicy::Never,
    }
}
fn dep(id: &'static str, path: &'static str,
       required: &'static [&'static str],
       after: &'static [&'static str]) -> ServiceManifest {
    ServiceManifest { required, after, ..m(id, path) }
}
fn err(manifests: &[ServiceManifest]) -> Option<ValidationError> {
    ServiceRegistry::validate(manifests).err()
}
fn pair() -> ServiceManager {
    ServiceManager::new(&[
        dep("displayd", "/System/Services/displayd", &["deviced"], &[]),
        m("deviced", "/System/Services/deviced"),
    ]).unwrap()
}

#[test]
fn builtins_are_bounded_and_validate() {
    let registry = ServiceRegistry::validate(&manifests::SYSTEM_MANIFESTS).unwrap();
    assert_eq!(registry.len(), 10);
    let ids: std::vec::Vec<_> = registry.startup_order()
        .map(|id| registry.manifest(id).id).collect();
    assert!(ids.iter().position(|x| *x == "deviced").unwrap()
        < ids.iter().position(|x| *x == "displayd").unwrap());
    assert!(ids.iter().position(|x| *x == "storaged").unwrap()
        < ids.iter().position(|x| *x == "updated").unwrap());
}

#[test]
fn stable_topological_order_is_independent_of_input_order() {
    let a = m("alpha", "/System/Services/alpha");
    let b = m("beta", "/System/Services/beta");
    let c = dep("charlie", "/System/Services/charlie", &["beta"], &["alpha"]);
    let one = ServiceRegistry::validate(&[c, b, a]).unwrap();
    let two = ServiceRegistry::validate(&[a, b, c]).unwrap();
    let names = |g: &ServiceRegistry| -> std::vec::Vec<&'static str> {
        g.startup_order().map(|id| g.manifest(id).id).collect()
    };
    assert_eq!(names(&one), vec!["alpha", "beta", "charlie"]);
    assert_eq!(names(&one), names(&two));
    assert_eq!(one.shutdown_order().map(|id| one.manifest(id).id).collect::<std::vec::Vec<_>>(),
        vec!["charlie", "beta", "alpha"]);
}

#[test]
fn missing_optional_is_not_required() {
    let r = ServiceRegistry::validate(&[dep(
        "inputd", "/System/Services/inputd", &[], &["not_available"],
    )]).unwrap();
    assert_eq!(r.len(), 1);
}
#[test]
fn hard_missing_and_self_dependency_fail_closed() {
    assert_eq!(err(&[dep("alpha", "/System/Services/alpha", &["beta"], &[])]),
        Some(ValidationError::MissingRequired));
    assert_eq!(err(&[dep("alpha", "/System/Services/alpha", &["alpha"], &[])]),
        Some(ValidationError::SelfDependency));
}
#[test]
fn cycles_in_requires_and_after_are_rejected() {
    let a = dep("alpha", "/System/Services/alpha", &["beta"], &[]);
    let b = dep("beta", "/System/Services/beta", &["alpha"], &[]);
    assert_eq!(err(&[a, b]), Some(ValidationError::DependencyCycle));
    let a = dep("alpha", "/System/Services/alpha", &[], &["beta"]);
    let b = dep("beta", "/System/Services/beta", &[], &["alpha"]);
    assert_eq!(err(&[a, b]), Some(ValidationError::DependencyCycle));
}
#[test]
fn duplicates_and_capacity_fail_closed() {
    let a = m("alpha", "/System/Services/alpha");
    assert_eq!(err(&[a, a]), Some(ValidationError::DuplicateId));
    assert_eq!(err(&[]), Some(ValidationError::EmptyRegistry));
    assert_eq!(err(&[a; MAX_SERVICES + 1]), Some(ValidationError::TooManyServices));
}
#[test]
fn invalid_ids_schema_and_executable_are_denied() {
    let base = m("good", "/System/Services/good");
    for bad in ["../bad", "Uppercase", "a.b", "é", ""] {
        assert_eq!(err(&[ServiceManifest { id: bad, ..base }]),
            Some(ValidationError::InvalidId));
    }
    assert_eq!(err(&[ServiceManifest { schema_version: 2, ..base }]),
        Some(ValidationError::InvalidSchema));
    assert_eq!(err(&[ServiceManifest { version: 0, ..base }]),
        Some(ValidationError::InvalidVersion));
    for path in ["/tmp/good", "/System/Services/../good", "/System/Services/bad"] {
        assert_eq!(err(&[ServiceManifest { executable: path, ..base }]),
            Some(ValidationError::InvalidExecutable));
    }
}
#[test]
fn dependencies_duplicates_and_bounds_are_checked() {
    let base = m("alpha", "/System/Services/alpha");
    assert_eq!(err(&[ServiceManifest { required: &["beta", "beta"], ..base }]),
        Some(ValidationError::DuplicateDependency));
    assert_eq!(err(&[ServiceManifest { required: &["beta"], after: &["beta"], ..base }]),
        Some(ValidationError::DuplicateDependency));
    assert_eq!(err(&[ServiceManifest {
        required: &["b", "c", "d", "e", "f", "g", "h", "i", "j"], ..base
    }]), Some(ValidationError::TooManyDependencies));
}
#[test]
fn capability_allowlist_is_not_a_grant() {
    let base = m("alpha", "/System/Services/alpha");
    assert_eq!(err(&[ServiceManifest {
        requested_capabilities: &["device.mmio"], ..base
    }]), Some(ValidationError::UnknownCapability));
    assert_eq!(err(&[ServiceManifest {
        requested_capabilities: &["ipc.send", "ipc.send"], ..base
    }]), Some(ValidationError::DuplicateCapability));
    assert_eq!(err(&[ServiceManifest {
        requested_capabilities: &[
            "ipc.send", "ipc.receive", "ipc.connect", "log.read",
            "fs.read", "fs.write", "service.observe", "device.observe", "device.claim"
        ], ..base
    }]), Some(ValidationError::TooManyCapabilities));
}
#[test]
fn quotas_and_restart_policy_are_checked() {
    let base = m("alpha", "/System/Services/alpha");
    for quotas in [
        Quotas { memory_bytes: 0, threads: 1 },
        Quotas { memory_bytes: MAX_MEMORY_BYTES + 1, threads: 1 },
        Quotas { memory_bytes: 1024 * 1024, threads: 0 },
        Quotas { memory_bytes: 1024 * 1024, threads: MAX_THREADS + 1 },
    ] {
        assert_eq!(err(&[ServiceManifest { quotas, ..base }]),
            Some(ValidationError::InvalidQuota));
    }
    for restart in [
        RestartPolicy::OnFailure { max_retries: 0, base_ms: 100, cap_ms: 1000 },
        RestartPolicy::OnFailure { max_retries: 2, base_ms: 0, cap_ms: 1000 },
        RestartPolicy::OnFailure { max_retries: 2, base_ms: 2000, cap_ms: 1000 },
    ] {
        assert_eq!(err(&[ServiceManifest { restart, ..base }]),
            Some(ValidationError::InvalidRestartPolicy));
    }
}
#[test]
fn dependency_waits_for_mock_ack_not_fake_pid() {
    let mut manager = pair();
    let provider = manager.lookup("deviced").unwrap();
    let consumer = manager.lookup("displayd").unwrap();
    assert_eq!(manager.model_start(consumer, true, 0), Ok(()));
    assert_eq!(manager.snapshot().iter().find(|x| x.id == consumer).unwrap().state,
        ServiceState::WaitingDeps);
    assert_eq!(manager.model_started(consumer, 1), Err(ServiceError::InvalidTransition));
    manager.model_start(provider, true, 2).unwrap();
    manager.model_tick(3);
    assert_eq!(manager.model_started(consumer, 3), Err(ServiceError::InvalidTransition));
    manager.model_started(provider, 4).unwrap();
    manager.model_tick(5);
    manager.model_started(consumer, 6).unwrap();
    assert_eq!(manager.snapshot().iter().find(|x| x.id == consumer).unwrap().state,
        ServiceState::Running);
    assert_eq!(manager.execute_service(consumer), Err(ServiceError::Unsupported));
}
#[test]
fn unauthorized_control_is_side_effect_free() {
    let mut manager = pair();
    let id = manager.lookup("deviced").unwrap();
    let before = manager.snapshot().iter().collect::<std::vec::Vec<_>>();
    assert_eq!(manager.model_start(id, false, 0), Err(ServiceError::AccessDenied));
    assert_eq!(manager.model_stop(id, false, 0), Err(ServiceError::AccessDenied));
    assert_eq!(manager.model_restart(id, false, 0), Err(ServiceError::AccessDenied));
    assert_eq!(manager.snapshot().iter().collect::<std::vec::Vec<_>>(), before);
}
#[test]
fn stops_consumers_before_providers() {
    let mut manager = pair();
    let a = manager.lookup("deviced").unwrap();
    let b = manager.lookup("displayd").unwrap();
    manager.model_start(a, true, 0).unwrap();
    manager.model_started(a, 1).unwrap();
    manager.model_start(b, true, 2).unwrap();
    manager.model_started(b, 3).unwrap();
    assert_eq!(manager.model_stop(a, true, 4), Err(ServiceError::ActiveDependent));
    manager.model_stop(b, true, 4).unwrap();
    manager.model_stopped(b, 5).unwrap();
    manager.model_stop(a, true, 6).unwrap();
    manager.model_stopped(a, 7).unwrap();
    assert!(manager.snapshot().iter().all(|x| x.state == ServiceState::Stopped));
}
#[test]
fn restart_is_stop_ack_then_start_request() {
    let mut manager = pair();
    let id = manager.lookup("deviced").unwrap();
    manager.model_start(id, true, 0).unwrap();
    manager.model_started(id, 1).unwrap();
    manager.model_restart(id, true, 2).unwrap();
    assert_eq!(manager.snapshot().iter().find(|x| x.id == id).unwrap().state,
        ServiceState::Stopping);
    manager.model_stopped(id, 3).unwrap();
    assert_eq!(manager.snapshot().iter().find(|x| x.id == id).unwrap().state,
        ServiceState::Starting);
}
#[test]
fn bounded_backoff_and_retry_limit() {
    let base = ServiceManifest {
        restart: RestartPolicy::OnFailure {
            max_retries: 2, base_ms: 100, cap_ms: 1000,
        },
        ..m("logd", "/System/Services/logd")
    };
    let mut manager = ServiceManager::new(&[base]).unwrap();
    let id = manager.lookup("logd").unwrap();
    manager.model_start(id, true, 0).unwrap();
    manager.model_started(id, 1).unwrap();
    manager.model_crash(id, 10).unwrap();
    let info = manager.snapshot().iter().next().unwrap();
    assert_eq!(info.state, ServiceState::Backoff);
    assert_eq!(info.next_retry_ms, Some(110));
    assert_eq!(info.retries_used, 1);
    manager.model_tick(109);
    assert_eq!(manager.snapshot().iter().next().unwrap().state, ServiceState::Backoff);
    manager.model_tick(110);
    manager.model_started(id, 111).unwrap();
    manager.model_crash(id, 120).unwrap();
    assert_eq!(manager.snapshot().iter().next().unwrap().next_retry_ms, Some(320));
    manager.model_tick(320);
    manager.model_started(id, 321).unwrap();
    manager.model_crash(id, 330).unwrap();
    assert_eq!(manager.snapshot().iter().next().unwrap().state, ServiceState::Failed);
    assert_eq!(manager.model_tick(999_999), ());
    assert_eq!(manager.snapshot().iter().next().unwrap().state, ServiceState::Failed);
}
#[test]
fn clock_overflow_refuses_crash_transition() {
    let base = ServiceManifest {
        restart: RestartPolicy::OnFailure {
            max_retries: 2, base_ms: 100, cap_ms: 1000,
        },
        ..m("logd", "/System/Services/logd")
    };
    let mut manager = ServiceManager::new(&[base]).unwrap();
    let id = manager.lookup("logd").unwrap();
    manager.model_start(id, true, 0).unwrap();
    manager.model_started(id, 1).unwrap();
    assert_eq!(manager.model_crash(id, u64::MAX), Err(ServiceError::ClockOverflow));
    assert_eq!(manager.snapshot().iter().next().unwrap().state, ServiceState::Running);
}
#[test]
fn healthy_service_resets_retry_budget_only_after_stable_window() {
    let base = ServiceManifest {
        restart: RestartPolicy::OnFailure {
            max_retries: 1, base_ms: 100, cap_ms: 200,
        },
        ..m("logd", "/System/Services/logd")
    };
    let mut manager = ServiceManager::new(&[base]).unwrap();
    let id = manager.lookup("logd").unwrap();
    manager.model_start(id, true, 0).unwrap();
    manager.model_started(id, 0).unwrap();
    manager.model_crash(id, 1).unwrap();
    manager.model_tick(101);
    manager.model_started(id, 102).unwrap();
    manager.model_health(id, Health::Healthy, 103).unwrap();
    assert_eq!(manager.snapshot().iter().next().unwrap().retries_used, 1);
    manager.model_health(id, Health::Healthy, 60_102).unwrap();
    assert_eq!(manager.snapshot().iter().next().unwrap().retries_used, 0);
}
#[test]
fn crashing_provider_requests_stops_of_dependent_mock_services() {
    let mut manager = pair();
    let a = manager.lookup("deviced").unwrap();
    let b = manager.lookup("displayd").unwrap();
    manager.model_start(a, true, 0).unwrap();
    manager.model_started(a, 1).unwrap();
    manager.model_start(b, true, 2).unwrap();
    manager.model_started(b, 3).unwrap();
    manager.model_crash(a, 4).unwrap();
    assert_eq!(manager.snapshot().iter().find(|x| x.id == b).unwrap().state,
        ServiceState::Stopping);
    manager.model_stopped(b, 5).unwrap();
    assert_eq!(manager.snapshot().iter().find(|x| x.id == a).unwrap().state,
        ServiceState::Failed);
}
#[test]
fn read_only_snapshot_is_detached_and_events_are_bounded() {
    let mut manager = ServiceManager::new(&[
        m("logd", "/System/Services/logd")
    ]).unwrap();
    let id = manager.lookup("logd").unwrap();
    let before = manager.snapshot();
    let mut subscription = manager.subscribe();
    manager.model_start(id, true, 1).unwrap();
    manager.model_started(id, 2).unwrap();
    for i in 0..(MAX_EVENTS + 3) {
        manager.model_health(id, Health::Healthy, i as u64 + 10).unwrap();
    }
    assert_eq!(before.iter().next().unwrap().state, ServiceState::Validated);
    assert!(matches!(manager.poll(&mut subscription), Err(ServiceError::EventsLost(n)) if n > 0));
    let mut count = 0;
    while let Some(event) = manager.poll(&mut subscription).unwrap() {
        assert_eq!(event.id, id);
        count += 1;
    }
    assert_eq!(count, MAX_EVENTS);
}
#[test]
fn unknown_id_never_resolves_to_a_pid() {
    let manager = pair();
    assert_eq!(manager.lookup("absent"), Err(ServiceError::UnknownService));
}
