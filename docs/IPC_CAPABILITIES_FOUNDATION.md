# ZillaOS — IPC + Capability Foundation (I0–I3)

> **Status: host-testable, kernel-only models; not a production syscall/security boundary.**
> Branch `feat/ipc-capabilities-foundation`. Target is independent process/service isolation, not an alternative ProcessManager. Architecture context: [PR #12](https://github.com/kabasik007/ZillaOs/pull/12).

## I0: object and handle lifetime

- `ObjectRegistry`: at most 128 registered objects; monotonic `ObjectId(u64)`, slot-generation pair. `revoke` invalidates all references to that object. An exhausted generation **retires**, not silently wraps, a slot.
- `HandleTable`: 64 local handles per process. Kernel must instantiate one table per authoritative ProcessManager PID. Local 64-bit handle is `generation:u32 | slot_plus_one:u32`. A raw value has no authority outside its owner's table. Close advances generation; close_all disposes a process's local handles; global revoke must be done separately for process-owned endpoints/devices.
- Rights: READ, WRITE, SEND, RECEIVE, TRANSFER, CONTROL, OBSERVE; unknown bits rejected in wire descriptors. Kernel-only `grant`/registry creation are **privileged** trusted APIs, not user syscalls.
- `prepare_transfer`: requires source TRANSFER; rights are intersected with requested mask. Receiver `accept` revalidates live object. No privilege elevation. Kernel must serialize transfer/accept/revoke atomically before production.
- `authorize(caller_pid, registry, handle, expected_kind, required_rights)` checks caller, generation, global revocation, type and rights **on each use**. Never use raw PID or unvalidated `ObjectId` as resource authorization.
- Stale slots return `StaleHandle`; globally revoked references return `Revoked`. Generation overflow is fail-closed: permanent retirement.

## I1: explicit LE wire envelope

`src/kernel/ipc/protocol.rs` rejects noncanonical envelopes and returns borrowed slices, never `unsafe` casts.

| Offset | Field | Width |
| --- | --- | --- |
| 0 | magic `ZIPC` | u32 |
| 4 | major / minor (1/0) | u16 / u16 |
| 8 | header length (32) | u16 |
| 10 | kind (event=0, request=1, reply=2) | u16 |
| 12 | opcode (nonzero) | u32 |
| 16 | request ID (event=0, otherwise nonzero) | u64 |
| 24 | payload length (<=4096) | u32 |
| 28 | number of handle descriptors (<=4) | u16 |
| 30 | reserved (zero) | u16 |
| 32 | payload, then handle descriptors | variable |

Each handle descriptor is 16 LE bytes: untrusted process-local handle `u64`, requested-rights `u64`. The descriptor is **not a capability** until source table verification. Exact length required; unknown versions/kinds/rights, invalid lengths, unknown flags or nonzero reserved bytes are rejected. No sender PID is present in the wire format: **kernel derives caller PID/TID from current task**. The parser only handles already safely copied bytes.

## I2: bounded model (not a blocking scheduler)

- `IpcQueue`: FIFO of at most 8 envelopes; up to 4096 payload bytes each plus 4 kernel-owned delegations. `QueueFull` and saturating rejection counter, no hidden allocation or drop-oldest behavior.
- `Envelope::stage` checks the sender's handle table and snapshots attenuated delegations. A global revoke after queueing is checked by `validate_transfers`, `delegation`, and receiver's `accept` at actual use.
- `CallTracker`: 16 request correlations per instance; client PID + request ID unique; immutable server object identity; wrong server denied; reply, cancel, deadline and `ServerGone` terminal states. Completed entries are explicitly collected with `take_complete`.
- `FakeClock` accepts explicit milliseconds for deterministic tests; **not** a real kernel monotonic timer. Expired queued messages are skipped and counted.
- `shutdown` destroys queued buffers and sets matching pending calls to `ServerGone`. Production must atomically synchronize queue close, registry revoke, pending wakeups and scheduler transitions; this model does not do so.
- Full FIFO currently copies fixed-size inline buffers: a bounded-memory tradeoff for first correctness tests, not a throughput-optimized design.

## I3: intended permission checks (not implemented as syscalls)

| Interface owner | Future entry | Required authority / behavior |
| --- | --- | --- |
| ProcessManager | process snapshot, kill, spawn | `Process` handle + OBSERVE/CONTROL; kernel PID truth and exit event |
| ServiceManager / zillad | stop, start, health, bind | Service policy + authorized endpoint, no ambient superuser |
| DeviceManager | observe, claim, map, IRQ, DMA | typed Device handle, OBSERVE/CONTROL; MMIO/DMA need **separate** higher-trust capabilities and IOMMU/quiescence |
| IPC core | send, receive, call, reply, cancel | Endpoint handle SEND/RECEIVE and live object identity; revalidate at every operation |
| Kernel audit | denied authorization | bounded structured `DeniedEvent` with authenticated caller, object ID, reason; **not** a live EventLog yet |

**Deny by default** on unknown handle/type/rights/version. The caller identity, request deadlines and object ownership must be sourced from protected kernel state, never arbitrary user data. Invalid source pointers must never be directly dereferenced; verify canonical range, mapped U/S permissions and page lifetime, use checked copy-from/to-user and guard faults. Never trust raw MMIO/DMA from applications. No execution of foreign binaries in Ring 0.

## Proposed ADRs (review before freezing ABI)

- **ADR-0007 (proposed):** Keep current experimental `INT 0x80` write/exit/yield ABI unchanged. For future v1 IPC syscalls, propose x86-64 `RAX` syscall number, `RDI RSI RDX R10 R8 R9` arguments, negative errno in `RAX`, caller identity from trap frame. No new syscall is installed and register convention is **not accepted**.
- **ADR-0008 (proposed):** u64 local handles with high 32-bit generation and low 32-bit 1-based index. Per-PID tables; registry holds monotonic object IDs and global revocation generation; wrap -> retire. Explicitly reject deriving handles from a different process. Future SMP requires lock ordering and RCU/epoch lifetime discipline.
- **ADR-0009 (proposed):** 32-byte LE v1 header, <=4096 bytes payload, <=4 descriptors of 16 bytes, FIFO 8, pending correlations 16; explicit monotonic ms deadline, bounded overflow, terminal shutdown status. CPU and resource quotas should be revised with real load tests, and fairness must be proven under scheduler integration.

## Host tests and negative cases

`cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline` should cover forged handles, wrong owner/type/right, generation staleness, global revocation across copies, bounded table, invalid headers, lengths and descriptor masks, wrong reply peer, duplicate correlations, queue full, cancellation, shutdown and simulated timeouts. Bare-metal compile/VM tests are separate evidence. **Do not report a test as passing until CI or the local toolchain executes it.**

## Missing gates before production

1. Ring 3 multi-process isolation, independent CR3/ASIDs and recoverable copy_from_user/copy_to_user, guarded supervisor stacks.
2. CPU-authenticated PID/TID, process-owned handle table lifecycle, teardown/reap and resource ownership from existing ProcessManager; no competing PID registry.
3. Actual syscall dispatch, ABI handshake, safe user buffer copy/pinning and budgeted memory accounting.
4. IRQ-safe locks, SMP ordering, no blocking under locks, scheduler-backed wait/wakeup, real monotonic timer and cancellation/reply atomicity.
5. Bounded kernel audit ring with drop counters, no secret content; proven denied operation paths.
6. Hardware device revoke must disable DMA/IRQ, invalidate IOMMU mappings and reclaim BAR before reassignment; the registry's in-memory revoke is **not** physical isolation.
7. Integration tests with independent processes in QEMU (one denied, one authorized), fuzz corpus for parser, crash/restart with leaked-handle and race injection.

**Non-goals in this branch:** no `main.rs` changes, no changes to `src/kernel/process/**`, no fake syscall backend, no driver MMIO, no scheduler changes, no invented daemon PIDs, no production sandbox claim.


## I-M1 — Process lifecycle and security integration bridge (PR #16, issue #21)

**Implementation status:** models and negative tests only. The canonical `src/kernel/process/mod.rs` owns `Pid`, `ProcessTable`, `ProcessState`; PR #18 extends CPU-local scheduling and TID models. This branch does **not** import PR #18, invent an alternative PID allocator, install syscalls, or alter its scheduler. PR #19 must reconcile the exports and run combined tests before merge.

### Trusted task identity

`capabilities::process_binding::current_running(&ProcessTable)` obtains exactly one process marked `Running`. It never accepts a PID supplied in ZIPC bytes, a manifest, an application, or a syscall register. Zero/multiple active model entries fail closed. `ProcessHandleBook` associates one `HandleTable` with an existing canonical `Pid`, checks the record is not Exited when binding, and uses the current canonical process for `authorize_running`. It stores **only** bounded capability tables, not process metadata or runqueues. `kernel_grant()` is an issuer-only policy hook; it grants nothing automatically from declared labels.

**Important:** `ProcessTable::Running` is still host-test metadata, **not** an authenticated current-CPU execution context. PR #18's `CpuSchedulerModel::snapshot(cpu).current()` identifies a modeled TID, with `ThreadTable::get(tid).pid` and parent state checks. At integration the Process agent must deliver a kernel-private `CurrentTask { cpu, tid, pid, generation }` from the verified trap/CPU-local state, and refuse desynchronized TID/PID or reaped tasks. Do not use `ProcessTable::Running` as a live multi-core syscall identity backend.

### Lifecycle hook order

1. ProcessManager records verified terminal Exited / Killed / Fault state and quiesces running CPU contexts.
2. Under future kernel lock(s), call `ipc::lifecycle::process_exit_cleanup(processes, exiting_pid, book, registry, calls, queues)` **before** PID reap.
3. For hosted endpoints, close the queue (pending calls -> `ServerGone`); for other queues, purge queued envelopes from the dead sender without disturbing FIFO.
4. Remove exiting client correlations; revoke all process-owned registry objects (global generation invalidation); close/remove process-local handles.
5. Driver/DeviceManager must independently quiesce IRQ/DMA/IOMMU/BAR claims **before device reuse**. This Rust model only invalidates references, not hardware.
6. Reap the PID only after ThreadTable TIDs, stacks and other resources have been safely retired by the canonical ProcessManager (PR #18 model already has a TID-reap guard).

`ExitCleanup` returns counts for host observability; it is not the live kernel audit log. A failed/not-Exited caller is rejected before changes. Callbacks are synchronous *models*: SMP/IRQ locking, in-flight leases, wakeups and fault handling remain P0 gates.

### IPC model API

| Entrypoint | Security condition | Notes |
| --- | --- | --- |
| `send_from_running` | current ProcessTable Running record + sender's typed Endpoint/SEND handle | source PID is not serialized or supplied |
| `receive_for_running` | Endpoint/RECEIVE and endpoint ownership | skips messages from exited or reaped senders |
| `deliver_to_running` | current receiver PID, live sender and globally valid grants | `accept_many` is all-or-none; no concurrent table mutation |
| `reply_from_running` | current owner + Endpoint/RECEIVE + matching kernel-issued `CallToken` | nonce blocks duplicate/replayed token after request-ID reuse |
| `register_owned_object` | canonical live PID with an existing handle binding | trusted kernel issuer only, no implicit user policy grant |
| `process_exit_cleanup` | canonical Exited record and existing bound table | closes owner queues, drops sender messages, revokes owned objects |

`CallTracker::reply(server_id, client_id, request_id)` is legacy test-only model convenience and MUST NOT be exposed as a syscall; use the authenticated `reply_from_running` token path. `CallToken` is a monotonically allocated generation **not a secret or cryptographic MAC**. The kernel must prevent foreign construction/injection via ABI, and authenticate endpoint ownership on each reply. Future sender identity must come from verified CPU/TID state.

### Typed resource adapters and rights

`capabilities::typed::TypedHandle<K>` is only a typed *view* over a raw handle. Its `authorize()` always validates table owner, current object generation/type and required rights. Available marker kinds: `EndpointResource`, `DeviceResource`, `ServiceResource`, `SurfaceResource`, `ProcessResource`. A wrong tag is rejected; tags and string permissions cannot mint any rights.

- **DeviceRegistry / Platform**: canonical generation-safe DeviceId must be mapped by trusted kernel policy to an `ObjectKey` (`ObjectKind::Device`). Device observe/claim requires a live typed capability at every operation; BAR/IRQ/DMA require distinct object kinds, resource-owner synchronization, IOMMU and physical teardown. Never expose raw MMIO/DMA from ordinary apps.
- **ServiceManager**: `ServiceId` != `Pid` != `ObjectId`. The ServiceManager requests authorized service-control actions; ProcessManager alone owns PID creation and exit notifications. Service restart must receive a fresh PID, a fresh Endpoint ObjectId, new explicit grants, and no old handles.
- **DisplayManager**: `SurfaceResource`/typed surface handles authorize surface operations, but the DisplayManager alone validates mode/scanout/VRAM resource ownership. A surface capability cannot claim a GPU BAR or commit a physical mode.
- **Rights matrix**: File/Surface, Device, Service, Process and Endpoint kinds reject nonsensical rights at grant time; a delegated grant can only attenuate rights using intersection. Types do not override issuer policy.

### Model verification, adversarial cases and known limitations

New host tests exercise forged sender PID in a binary payload, wrong/empty/unknown rights, typed-kind mismatch, reused object slot, revoked sender-owned resources, server crash with `ServerGone`, stale handle after service restart, message overflow with pending-call rollback, FIFO wrap-and-purge, duplicate reply, stale token after `request_id` reuse, and exit with unreaped child TID. Separate CI also compiles existing Ring3-thread/preempt feature binaries **without running them**.

Missing security gates: production authenticated CPU-local PID/TID; checked user copies and pinning; real scheduling and unblocking; SMP/IRQ serialization/RCU; ObjectRegistry object destruction and memory lifetime; token confidentiality at kernel boundary; runtime audit/logging; actual DMA shutdown; fault injection and QEMU multi-process tests. `&mut` Rust guarantees only exclusive access within the model, not real kernel synchronization.

**Important scope gap for issue #21:** ProcessTable does not currently track PPID/child-process trees. Tests exercise parent PID with dependent TID records, not real child processes. The Process agent must add authoritative PPID/orphan/exit ordering before a true parent-with-child-process teardown test can exist.

**Memory note:** Fixed-size queue envelopes (4 KiB x depth 8) and a book of 16 x 64 handles are intentionally bounded but sizable. A future kernel must own static/heap allocations with memory quotas; do not instantiate these wholesale on a small kernel interrupt stack.
