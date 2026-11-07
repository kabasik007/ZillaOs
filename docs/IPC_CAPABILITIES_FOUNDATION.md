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
