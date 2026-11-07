# ZillaOS ServiceManager / ProcessManager Bridge — I-M1

**Status:** Pure host-testable Rust no_std model; NOT a user-space daemon, NOT a syscall backend.
**PR:** #14; **integration:** #19; **architecture:** #12; **blocking issue:** #22.

## Scope and ownership

- ServiceManager (src/services/service_manager.rs) owns manifests, lifecycle states, dependency graph and backoff policy.
- ServiceSupervisor (src/services/service_supervisor.rs) is a typed ProcessManager integration **adapter**. It holds at most one verified ProcessManager binding **per service**. It is not a PID table, process factory or executable loader.
- ProcessManager remains the only owner of PIDs, task states, faults, threads and memory mappings. A future kernel integration must select ProcessBoundary::Pid = crate::process::Pid; the tests use KernelPid(u64) and nothing allocates real kernel PIDs.
- Capability security is owned by the kernel HandleTable/ObjectRegistry. ServiceManager cannot issue or authorize any capability. Requested labels in manifests are never grants.
- src/services/mod.rs, src/lib.rs, Cargo.toml, src/services/process_manager.rs, src/kernel/process/**, src/kernel/capabilities/**, src/kernel/ipc/**, scheduler, GPU and desktop remain unchanged.

## Typed ProcessBoundary contract

The new ProcessBoundary trait defines an associated Pid type and these kernel-trusted methods:

| API | Required kernel proof |
| --- | --- |
| allow_control(operation,manifest) | actual caller identity plus live service-control right scoped to target |
| verify_started(ticket,pid,manifest) | actual ProcessManager PID, signed native image, exact manifest quotas, all requested and issued rights approved |
| verify_exited(ticket,pid,reason) | authenticated kernel exit/fault notification for this process generation |
| verify_cleaned(ticket,optional pid) | process termination, IPC closure, revoked handles, memory/TID release and DMA/IRQ quiescence |
| is_live(pid) | authoritative kernel ProcessManager liveness/snapshot |

**These are trusted adapter calls, not a user IPC API.** Arbitrary user implementations or user-supplied booleans must never reach the real supervisor. The model's FakeKernel is for tests, not a security boundary.

## Identity and replay prevention

ServiceId is local to the bounded ServiceRegistry; its index is not a PID, kernel handle or durable service ID. LaunchTicket contains ServiceId, a trusted supervisor epoch, and a strictly increasing generation. A new epoch must be provided by the integration owner for every supervisor instance. The next-generation counter rejects overflow rather than silently wrapping.

- A successful start binds exactly one ProcessManager PID to its ticket after verify_started and is_live.
- A duplicate PID, obsolete launch ticket, foreign epoch, inconsistent service state or unverified capability grant fails closed.
- The supervisor never invents a PID or creates a second ProcessTable. TaskManager joins its service view with canonical TaskSnapshot, not a service-maintained CPU/memory counter.

## Lifecycle protocol

1. request_start authenticates service control, checks a bounded 5-second start deadline and asks the core model to transition to Starting (or WaitingDeps).
2. next_action produces an inert LaunchIntent with executable identity, capability *requests*, quotas, expiry and unique ticket. Real spawning requires the future trusted ProcessManager backend.
3. confirm_started only accepts a live kernel PID with matching ticket and verified image/grants. After this acknowledgement the model enters Running.
4. request_stop produces a Stop action, waits for an authenticated exit event, then a Cleanup action. A five-second stop timeout emits at most one force-stop escalation; it never fabricates a completed stop.
5. notify_exit checks the verified PID and exact ticket. Unexpected exit invokes restart policy and transitive dependency failure propagation; planned stop does not consume a retry.
6. Cleanup for an exited PID requires kernel-confirmed revocation and teardown. Cleanup with no PID is a **cancel-by-ticket fence** after an unacknowledged or timed-out launch, since a late process may still exist.
7. confirm_cleaned must reject live PIDs and unverified revocation. Only then can the supervisor restart, release its binding or complete an explicit stop.
8. restart while running is Stop → verified exit → verified cleanup → new launch ticket. A manually authorized restart after Failed/Backoff may rearm the circuit breaker only once cleanup is resolved.

**Global conservative cleanup barrier:** while any cleanup confirmation is pending, new starts and retries are paused (other existing services are not killed). This avoids a race between PID reuse and resource reclamation. Parallel per-service barriers require a proven synchronization policy.

## Dependencies and timeouts

- Required dependencies must exist, are part of deterministic topological startup and prevent early child startup. A provider cannot be stopped while a running hard dependent requires it.
- Optional after dependencies affect ordering if present but do not force availability.
- Cycle detection rejects both required and optional ordering loops before any state changes.
- Unexpected provider exit marks downstream consumers Stopping in reverse graph order; stop commands are generated before restarting failed providers.
- Start timeout: 5000 mock milliseconds, then abort/cleanup barrier. Stop timeout: 5000 mock milliseconds and one force-stop request. No timers run in this model; callers advance trusted monotonic time explicitly.
- OnFailure restart policies cap retry count, exponential delay and maximum backoff; retry storms end in Failed, not infinite loops.

## Capability, Device and Display contract

The current IPC/capability workstream (#16) has generation-safe ObjectRegistry, per-process HandleTable, ObjectKind and Rights, but no production syscall-enforced channel. Future kernel implementation must bind every grant to exact process identity, limit to declared/approved rights and check at each resource use. Never allow a manifest to grant itself service.control, device.claim, MMIO or DMA.

Platform (#17) and Display (#15) own hardware identities, resource leases, outputs and surfaces respectively. DeviceId/OutputId/SurfaceId must be opaque generation-safe resource handles, revoked on service exit by the kernel. ServiceManager does not parse raw PCI BARs or own DisplayManager/WindowManager. Device IRQ and DMA must be quiesced before claim transfer.

## Snapshot

ServiceSupervisor::snapshot(boundary) yields bounded per-service view: service name/ID, state, health, retries, deadline, optional authenticated live PID, optional ticket, pending cleanup. It contains no independent process registry and no invented CPU or memory metrics. It suppresses dead PIDs and any PID pending cleanup. TaskManager must validate service.observe through kernel capabilities before exposing this data over IPC.

## Tests (Rust 1.90)

The standalone CI workflow executes:

    rustc +1.90.0 --edition 2024 --test tests/service_manager_host.rs -o /tmp/service_manager_tests
    /tmp/service_manager_tests
    rustc +1.90.0 --edition 2024 --test tests/service_supervisor_host.rs -o /tmp/service_supervisor_tests
    /tmp/service_supervisor_tests
    rustc +1.90.0 --edition 2024 --crate-type lib --target x86_64-unknown-none tests/service_manager_no_std.rs --emit metadata -o /tmp/service_manager.rmeta

Test coverage: invalid schema/IDs/quotas/capabilities, cycles, hard missing deps; refused control, forged grants, forged exits, duplicate PIDs, supervisor epoch replay, late started ACK, start/stop timeouts, retry exhaustion, cleanup barrier, dependency failure ordering, verified snapshot liveness and no real execution. A host or bare-metal metadata PASS does **not** verify a linked kernel binary or actual QEMU/VMware service runtime.

## Process / IPC agent handoff

Process agent: provide authenticated kernel-owned spawn result carrying exact ticket, immutable identity and quota proof; authoritative PID with generation strategy, verified exit/kill, process-state snapshot, teardown including all TIDs/resources and monotonic timer. ProcessManager::execute_native currently refuses arbitrary isolated execution.

IPC and capabilities agent: provide authenticated service.control and service.observe rights, versioned bounded process-event channel, grant/revoke proofs, cancellation of queued IPC, wakeups with ServerGone and synchronization/locking guarantees. Untrusted userland must not be able to implement ProcessBoundary.

Integration agent: compare this isolated PR against current main and staging PR #19; coordinate one additive src/services/mod.rs export only when ready; run lib/host/kernel/QEMU smoke, do not infer userland daemon from no_std compilation. Merge only after owner approval, never auto-merge.

**Not implemented:** zillad process, real spawn/kill, signed file parser, IPC calls, capability grants, hardware DMA cleanup, process scheduler integration, QEMU or VMware service runtime.
