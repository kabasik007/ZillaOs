# ZillaOS ServiceManager Foundation (S0–S3 model)

**Status:** pure Rust no_std model on branch feat/service-manager-foundation. It is not wired into the kernel, cannot execute any native program, and does not create or retain PIDs. See architecture PR #12 for the target design and security gates.

## Scope and ownership

- Owns: src/services/service_manager.rs, src/services/service_manifests.rs, tests/service_manager_host.rs and its own CI workflow.
- Unchanged: ProcessManager, ApplicationManager, TaskSnapshot, src/services/mod.rs, src/lib.rs, Cargo.toml, scheduler, Desktop, GPU and core IPC.
- Host module is compiled directly from the test harness via #[path]. It does **not** automatically enter the kernel build until an integration owner adds one public module declaration in src/services/mod.rs. This avoids concurrent edits to shared source files.

## Boundaries

1. No real PID appears in ServiceSnapshot or ServiceManager state. PID/TID identity, execution and exit reasons belong exclusively to kernel ProcessManager.
2. A ServiceId is a registry-local token, **not** a process handle. It must be re-resolved by name when crossing registry instances; never persist it as durable machine ID.
3. This is a deterministic simulation API with explicit model_* methods. Calling execute_service always returns Unsupported and cannot mutate the runtime.
4. model_started / model_stopped / model_crash are mock acknowledgements; production code must only accept their equivalents from authenticated kernel process or trusted launcher events.
5. Explicit control boolean is **test scaffolding**, not authorization; real authorization must use kernel-validated capability handles at the syscall/IPC boundary.
6. Requested capabilities are validated labels only, **not** automatically granted. The allowlist is provisional and excludes arbitrary MMIO/DMA and unrestricted process control.
7. Executable identities must match /System/Services/{id} exactly, but this check is only syntactic. Real native binaries require a trusted signer, image hash, loader and per-process memory isolation.
8. All arrays are bounded, no alloc/unsafe/IO; external JSON/TOML parser, signature validation and generated service configuration are not implemented.

## API (model, not stable ABI)

| Area | API | Rule |
| --- | --- | --- |
| Manifest | ServiceRegistry::validate(&[ServiceManifest]) | rejects duplicates, invalid IDs/schema/paths, bad quotas/caps, missing hard deps, cycles |
| Lookup | ServiceRegistry::find(name) | registry-local ServiceId; no PID |
| Graph | startup_order(), shutdown_order() | deterministic lexical Kahn order with reverse stop order |
| State | ServiceManager::model_start / model_started | start request and separate acknowledgement; required dependencies must be Running |
| Stop | model_stop / model_stopped | denies stopping providers with active hard dependents; ack awaited |
| Restart | model_restart | stop must be acknowledged before new start request |
| Faults | model_crash / model_tick | exponential, capped retry backoff; no timers or busy waiting |
| Health | model_health | Unknown/Healthy/Degraded/Unhealthy; retry budget resets only after 60 seconds of stable running |
| Events | subscribe / poll | local bounded 32-event ring; lagging consumers receive EventsLost and resync cursor |
| State view | snapshot | owned, detached immutable copy; zero fabricated PID fields |
| Execution | execute_service | always Unsupported |

Constants: maximum 16 services, 8 required and 8 optional-after dependencies per service, 8 requested capabilities, 32 queued events, 1 GiB memory request per service, 32 threads. Manifest schema_version must be 1 and the service ID must be ASCII lower-case alphanumeric, hyphen or underscore, starting with a letter, at most 32 bytes.

### Dependencies and shutdown

- required: hard edge. Registration rejects unknown targets; model_start waits until required providers are Running.
- after: ordering edge only. Unknown optional targets are ignored. Existing targets affect topological order and are included in cycle checks; they are not auto-started.
- shutdown_order is reverse topological; stop calls return ActiveDependent if a direct required consumer is Starting/Running/Stopping.
- On provider crash, transitive dependent mock instances receive DependencyLost/StopRequested in reverse order. This is **not** a real process teardown; production integration must obtain kernel exit/revocation and stop acknowledgements.
- No automatic start of on-boot services occurs in the model. External supervisor may inspect StartMode to decide which to request, only after full platform security gates.

### Retry example

Start request at t=0 -> mock started at t=1 -> crash at t=10.
With OnFailure { max_retries: 2, base_ms: 100, cap_ms: 1000 }:
- First crash -> Backoff until t=110 (retry count 1).
- Second crash at t=120 -> Backoff until t=320 (retry count 2).
- Third crash at t=330 -> Failed terminal, no further automatic retries.
- A healthy report resets the retry budget **only** after at least 60,000 ms since the latest Started acknowledgement.
- Overflowing the monotonic deadline returns ClockOverflow without applying the crash transition.

## Future system-service manifests

The actual typed examples are in src/services/service_manifests.rs. The public human-readable *proposal* below is not parsed by the kernel and contains no executable or capability grant:

```toml
schema_version = 1
id = "displayd"
executable = "/System/Services/displayd"
start = "on-boot"
required = ["deviced"]
after = ["logd"]
requested_capabilities = ["gpu.surface.create", "gpu.surface.present", "ipc.connect"]
memory_bytes = 67108864
max_threads = 4
restart = "on-failure"
max_retries = 3
base_ms = 250
cap_ms = 30000
```

| Service | Startup | Hard service deps | Ordering only | Proposed role |
| --- | --- | --- | --- | --- |
| logd | boot | none | none | consume kernel events |
| securityd | boot | none | logd | policy front-end; kernel is security authority |
| deviced | boot | none | logd | device observation/binding policy; kernel claims resources |
| inputd | boot | none | logd | input-event routing |
| displayd | boot | deviced | logd | compositor/surfaces, not raw MMIO |
| storaged | demand | none | logd | VFS/block orchestration |
| netd | demand | deviced | logd | NIC/network policy |
| audiod | demand | deviced | logd | mixing/output |
| powerd | boot | none | deviced | ACPI/power observation |
| updated | manual | storaged, securityd | logd | verified staged updates with rollback later |

The ServiceManager itself does not spawn zillad. zillad will become the supervisor in an isolated user process only once the kernel provides protected native user execution, trusted launch and versioned IPC.

## Required APIs from Process/IPC/Security workstreams (NOT provided by this PR)

1. **ProcessManager:** authenticated spawn of signed, allowlisted native image returning kernel-owned generation-safe process handle and PID snapshot; explicit Started/Exit/Fault/StopAcknowledged events with monotonic timestamps; bounded async termination, wait and resource teardown.
2. **Kernel capabilities:** per-process handle table with generation and revocation; intersection on handle delegation; rights checked at operation time. The model's control boolean must not be exposed through the user ABI.
3. **IPC:** versioned bounded channel, authenticated caller identity from kernel, back-pressure, cursor/sequence-based event loss handling, deadlines and ServerGone on crash. Do not map model event array directly to user memory.
4. **Monotonic timer:** trusted u64 milliseconds/ticks and deadline wakeups, no polling on busy loops, clock generation/overflow rules.
5. **System trust:** executable allowlist + signer verification, immutable manifests or signed boot policy; dependency failure and resource revocation ordering (DMA/IRQ quiescence before claim reassignment).
6. **TaskSnapshot:** join service ID to real process identity only by kernel-assigned handle; never invent service-owned PID counters.

## Test and evidence matrix

```bash
cargo +1.90.0 test --test service_manager_host --target x86_64-unknown-linux-gnu --offline
rustc +1.90.0 --crate-type lib --edition 2024 \
  --target x86_64-unknown-linux-gnu \
  src/services/service_manager.rs --emit metadata -o /tmp/service_manager.rmeta
```

The dedicated service-manager-model GitHub Actions workflow runs both commands for the PR. A host PASS proves only deterministic model behavior and compilation as a no_std library; it does not prove a bare-metal kernel integration, VM boot, native daemon, system isolation or correct hardware crash recovery.

Negative cases include duplicate service IDs, unknown schemas, invalid paths, unrecognized capabilities, excessive quotas, hard missing dependencies, required/after cycles, authorization refusal, control-state transitions, dependent stop ordering, crash/backoff/retry exhaustion, deadline overflow, event overflow and detached snapshot semantics.

## Integration checklist (future, coordinated)

- Confirm service-framework API identifiers with Process/IPC agents and re-read latest main HEAD.
- Add one minimal src/services/mod.rs declaration *only when shared-file owner agrees*, then run lib/host/kernel build and QEMU smoke.
- Replace mock lifecycle acknowledgements with authenticated kernel ProcessManager notifications.
- Implement real capability-checking and process launch in user mode. Do not replace execute_service with arbitrary Ring 0 function dispatch.
- Review service table, security policy and signed manifests with OS integration owner before enabling boot autostart.

**Never** infer from this document that deviced, displayd or zillad are executing daemons.
