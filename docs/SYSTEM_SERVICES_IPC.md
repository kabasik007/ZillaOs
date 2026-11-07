# ZillaOS — System Services, IPC ABI та Capability Security

> **Status 2026-11-07:** proposed contracts only. No real user-space `zillad`, `deviced`, `displayd` or general IPC capability broker currently exists. Experimental `native-ring3-preempt` is a separate one-core QEMU-only fixture, **not** production service execution.

## 1. Current kernel baseline and actual blockers

At time of analysis:
- `src/kernel/process/mod.rs` has `ProcessTable`/PID, lifecycle and guarded memory models.
- `src/kernel/process/thread.rs` has TID, ready/running/wait states and early preemption-oriented model.
- `src/services/process_manager.rs` exposes reserved/mapped/prepare model but cannot execute arbitrary isolated binaries.
- `src/services/task_snapshot.rs` collects read-only process/thread states; avoid separate service-owned PID list.
- `src/native_ring3.rs`, `src/native_ring3_threads.rs`, `src/native_ring3_preempt.rs` provide **opt-in** tests with fixed embedded program, not native executable loader.
- Existing `src/services/app_manager.rs` currently launches embedded WindowManager views, not processes.
- No general, versioned secure IPC, handle transfer, user buffer pinning, monotonic scheduler-driven blocking or independent daemon supervisor has been verified.

### Boot gates before genuine services

```text
Ring 3 execution and memory isolation
        ↓
per-process handle table with generation + rights
        ↓
versioned syscall ABI + checked copy_from_user/copy_to_user
        ↓
bounded message queues, sender identity and deadlines
        ↓
server spawn/exit/fault and teardown
        ↓
ServiceManager -> real PID, dependences, restart
        ↓
deviced / logd / displayd / securityd
```

Do not bypass unfinished gates by declaring ordinary kernel modules to be "services".

## 2. Separating Kernel, DeviceManager, and ServiceManager

**Kernel mechanisms**: real PID/TID creation, address spaces, scheduler, IRQ, MMIO, rights enforcement, DMA domain, timer, IPC object. **User-service policy**: discover service manifests, decide startup order, request approved driver bind, manage retries and UI. **Device authority** remains in kernel even if `deviced` crashes.

| Operation | Authoritative owner | Consumer |
| --- | --- | --- |
| Allocate PID, TID, CR3; handle faults | Kernel ProcessManager | `zillad` |
| Grant/revoke device/capability handles | Kernel object table | policy broker |
| PCI BAR/IRQ/DMA ownership | Kernel DeviceManager | `deviced`/driver |
| Surface handle/physical mode validation | Kernel DisplayManager | `displayd` |
| Window state/z-order/focus | WindowManager | `displayd` |
| Application routing | ApplicationManager | launcher / `zillad` |
| Audit event emission | Kernel EventLog | `logd`/Task Manager |
| Service restart/health policy | `zillad` after user-space ready | UI/administration |

ServiceManager should **not** become a privileged monolith with every device permission.

## 3. Capability object model

Suggested data model (provisional, no numeric ABI assigned):

```rust
// NOT implemented. Exact handle packing/layout needs an ADR.
struct Handle { slot: u32, generation: u32 }
struct Rights(u64);
enum ObjectKind { Endpoint, Process, Thread, Device, Irq, Mmio, Dma, File, Surface, Timer }
struct HandleEntry {
    generation: u32,
    kind: ObjectKind,
    rights: Rights,
    // kernel-owned object ref and owner identity, not a pointer from userland
}
```

Properties:
- No process may provide a raw `Pid` as a substitute for a handle to another process/device.
- `slot` can be reused only after `generation` is incremented; wraparound strategy specified and tested (retire slot or use sufficiently wide generation, never silent reuse).
- Explicit **rights intersection** when duplicating/delegating a handle. Recipient can never gain rights absent in sender/issuer.
- Revocation affects existing and queued requests; kernel checks handle at **use time**, not only at connection setup.
- MMIO and DMA permissions are stronger than ordinary `device.observe`; never expose them to untrusted user apps at bootstrap.
- Kernel-owned audit records sender PID/TID, target object, operation, decision and reason, with bounded message sizes; do not log full secrets/file contents.

### Suggested capability rights

`ipc.connect`, `ipc.send`, `ipc.receive`, `service.observe`, `service.control`, `device.observe`, `device.claim`, `display.mode.test`, `display.mode.commit`, `gpu.surface.create`, `gpu.surface.present`, `log.read`, `process.observe`, `process.kill`, `fs.read`, `fs.write`.

Labels are **proposed** rights namespaces, not present syscalls. `securityd` may configure policy but enforcement must remain in kernel.

## 4. Versioned IPC envelope

The wire format must be explicitly versioned, little-endian on x86-64, and independent of Rust `repr(Rust)` layout:

```text
MessageHeaderV1 (conceptual):
  magic                 u32  (fixed protocol marker)
  version_major         u16
  version_minor         u16
  header_bytes          u16
  flags                 u16
  opcode                u32
  request_id            u64
  payload_length        u32
  handle_count          u16
  reserved              u16
  payload               bytes (checked)
  transferred_handles   kernel-validated descriptors (NOT raw pointers)
```

Detailed ABI decisions (byte offsets, alignment, max payload, flag values, error codes) require an ADR **before code**. Never serialize a `repr(Rust)` struct by unsafely casting pointers into an untrusted guest buffer.

Suggested error classes: `UnsupportedVersion`, `InvalidHeader`, `Oversized`, `BadHandle`, `AccessDenied`, `StaleGeneration`, `QueueFull`, `TimedOut`, `ServerGone`, `Canceled`, `Busy`.

## 5. IPC runtime and scheduling

- Per-endpoint bounded queue; reject overflow cleanly and increment a dropped-message counter.
- Sender identity comes from kernel current task, never from user-supplied `sender_pid`.
- Kernel copies only after validated user virtual buffer, page access permissions and length; no unchecked foreign pointers.
- For larger transfers, offer pinned/shared-memory handles only after copy-based IPC is trustworthy; ownership and timeouts must prevent use-after-free.
- `send`, `recv`, `call`, `reply`, `cancel` are **proposed** semantic operations. Avoid stuck synchronous calls to crashed server.
- Waiting service thread transitions to Blocked using actual scheduler queues; no spin loops disguised as `receive()`.
- Timer expiration is based on a monotonic clock, not wall-clock RTC or a fixed number of "busy" poll iterations.
- SMP later needs IRQ-safe synchronization, per-CPU queues, TLB shootdowns and correct memory ordering for capability table reclamation.

## 6. Service boot graph and failure domains

Recommended startup order after user-mode prerequisites:

```text
Kernel provides: IPC + device registry + EventLog + ProcessManager
  └─ init / zillad
       ├─ logd (bounded subscriber)
       ├─ deviced (policy / observation; kernel owns resource authority)
       ├─ securityd (policy control; kernel enforces rights)
       ├─ inputd (event routing)
       ├─ displayd (if display backend can be safely obtained)
       ├─ storaged / netd / audiod / powerd (when implemented)
       └─ application launcher / desktop
```

On service crash:
1. Kernel records exit/fault and forcibly revokes service-owned handles/capabilities.
2. Quiesce device DMA/IRQ and release ownership **before** reassigning driver claim; invalid handles trigger `StaleGeneration`.
3. `zillad` resolves dependency failures and applies bounded restart policy.
4. `logd` can consume events but cannot tamper with kernel event records.
5. If display service dies, keep serial diagnostics and fallback to software presentation **only where display hardware is still in a valid accessible state**.
6. Task Manager reads authoritative PID/TID snapshot + services mapped by their real PID. No manually invented CPU/memory counters.

## 7. API acceptance tests (pure first; no VM claims)

| Test | Expected |
| --- | --- |
| malformed header / unknown major version | no queue mutation, reject |
| oversized payload or handle array | refuse before allocation / overflow |
| forged sender PID inside payload | ignored; kernel identity wins |
| transfer handle with too many rights | intersection only; no elevation |
| revoke while message queued | target access denied at use time |
| server dies before reply | client receives `ServerGone` and unblocks |
| duplicate service ID, cyclic dependencies | graph rejects startup |
| restart releases BAR/DMA claims | stale generation invalid, no overlap |
| UI requests `device.mmio` without grant | denied |
| 2 CPUs concurrently use endpoint | serialized correctness after SMP exists |
| no Ring 3 general execution available | `Unsupported`, no kernel-ring arbitrary execution |

## 8. Architecture Decision Records required

- **ADR-0007**: syscall convention and ABI versioning, currently OPEN in `docs/DECISIONS.md`.
- **ADR-0008**: capability handle layout, generation and revocation semantics, OPEN.
- **ADR-0009**: bounded IPC message sizing/fairness/timeouts, OPEN.
- **ADR-0011**: DMA/IOMMU and native driver trust, OPEN.
- Decide these before shipping an ABI, not by copying the example struct here.

## 9. Codex milestone proposal

**IPC-0:** pure Rust `MessageHeaderV1` decoder/validator and error enum with host property/negative tests; no syscall integration.

**CAP-0:** pure generation-handle object table, rights attenuation and revoke, bounded memory; no raw unsafe pointers.

**SUP-0:** service dependency graph and deterministic mock scheduler for health/restart, using `docs/SERVICE_FRAMEWORK.md` specification.

**IPC-1:** only after Process agent provides verified user-mode syscall/copy/user stacks, integrate a minimal endpoint to exchange 2 messages between two fixed isolated processes in QEMU.

**IPC-2:** real user-space `zillad` with explicit allowed manifests and exit/restart events.

Do not touch `src/kernel/process/**` while another agent owns it without a negotiated API change.

### References
- [Existing Process plan](PROCESSES.md)
- [Service framework](SERVICE_FRAMEWORK.md)
- [Security architecture](SECURITY_ARCHITECTURE.md)
- [PCI resource ownership](PCI_ACPI_RESOURCE_MODEL.md)
- [Current architecture](ARCHITECTURE_CURRENT.md)
