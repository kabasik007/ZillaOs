# ZillaOS Process I-M1 — process ownership, cleanup, IPC/service integration

> **HISTORICAL PR REFERENCE:** PR #18 branch is merged and retired. Model-only cleanup still differs from production syscalls. See [STATUS](STATUS.md), [trusted-exit spec](specs/features/trusted-process-exit.md), Issue #21.

Status (2026-11-07): bounded Rust no_std **MODELS**, not production process isolation, real SMP, Ring3 service runtime, or actual hardware memory reclamation.

Coordination: Process PR #18, IPC PR #16, Integration PR #19 and Issue #21. Each remains owned by its respective agent. Before merging, recheck the latest upstream HEAD and PR diff.

## What the process agent implemented

- CPU-local current PID/TID derivation: CurrentTaskIdentity has private fields and is derived by verifying CPU online state, saved Running TID, assigned CPU, its actual parent PID in ProcessTable, and the parent's live state. It never accepts an IPC sender-claimed PID. The argument cpu must come from trusted Ring0 hardware, not from a user packet.
- SavedTrapContext::confirmed_user_frame: a trusted unsafe admission contract that validates CPL3 selectors, user RIP/RSP bounds, RFLAGS fixed bit, IOPL/NT/VM denial and CPU/TID range. The architecture trap handler still must verify actual GDT, page rights and all saved CPU/FPU state. The older unsafe confirmed(tid) remains model-test-only.
- ExitCoordinator: bounded MAX_PROCESSES journal of monotonic exit tickets and stable ExitNotification{pid,reason,ticket}. A notification is private until parent and thread state transitions commit, and no ticket is ever reused.
- CleanupBackend: typed, trusted, idempotent callbacks for Capabilities, IPC, Services and Memory. A backend failure leaves its phase pending; a backend success advances exactly one stage. No IPC table or ServiceManager was duplicated.
- ProcessManager lifecycle: request_exit_model requires quiescent owned TIDs and no matching CPU-local Running reference, marks process Exited, transitions quiescent threads Exited and publishes the event. Remaining cleanup blocks PID reaping. After all external stages ACK, thread records can be reaped, then finish_exit_model reaps the actual PID and retires the ticket.
- Kernel-stack record lifecycle: one high-half guard-page lease per TID, metadata-only activate and unsafe retire-after-quiescence, no stale generation reuse. A TID with an attached stack lease cannot be reaped. The Memory cleanup stage refuses to advance while any owned stack lease remains.
- Bulk ThreadTable::terminate_owned_quiescent rejects the whole state change if any matching TID is still Running, before touching other TIDs.
- CPU-local model retains nested IRQ and disable-preemption depth, deferred reschedule, checked saved-context transitions. Only BSP CPU0 is actually online.

## APIs for integration

ProcessManager exposes:

    current_task_identity_model(cpu) -> Result<CurrentTaskIdentity, ScheduleError>
    reserve_kernel_stack_model(tid, guard_va, pages) -> Result<u64, StackModelError>
    kernel_stack_model(tid) -> Option<KernelStackLease>
    activate_kernel_stack_model(id) -> Result<(), StackError>
    unsafe retire_kernel_stack_model(id) -> Result<(), StackError>
    release_kernel_stack_record_model(id) -> Result<(), StackError>
    yield_thread_model(cpu, saved) -> Result<Tid, ScheduleError>
    block_thread_model(cpu, saved) -> Result<Tid, ScheduleError>
    exit_thread_model(cpu, saved) -> Result<Tid, ScheduleError>
    wake_thread_model(tid) -> Result<(), ScheduleError>
    request_exit_model(pid, reason) -> Result<ExitNotification, ExitModelError>
    exit_cleanup_work(ticket) -> Result<CleanupWork, CleanupError>
    exit_cleanup_for_pid(pid) -> Option<CleanupWork>
    drive_exit_cleanup_model(ticket, backend) -> Result<CleanupPhase, ExitModelError>
    finish_exit_model(ticket) -> Result<ExitReason, ExitModelError>

Pure kernel interfaces: ProcessTable::terminate(pid,reason); ThreadTable::terminate_owned_quiescent(pid); ExitCoordinator::reserve/publish/work/acknowledge/retire; CurrentTaskIdentity and validated UserTrapFrame.

## Exit and fault cleanup ordering

    Kernel fault/exit/kill identified by canonical ProcessTable
      -> reject unknown/exited PID, Running TID or CPU-owned stack
      -> reserve unpublished monotonic exit ticket
      -> mark ProcessTable Exited + all quiescent owned TIDs Exited
      -> publish ExitNotification {pid, reason, ticket}
      -> Capabilities: IPC owner closes all process handles/revokes objects
      -> IPC: IPC owner shuts endpoints, cancels calls, wakes waiters
      -> Services: ServiceManager consumes real PID/exit reason for restart policy
      -> Memory: VMM quiesces stacks, releases page tables, flushes TLB
      -> Complete: reap TIDs, reap PID, retire ticket

Only the actual owner of an external resource may acknowledge its release. A mock backend that returns Ok in unit tests does not mean handles or physical pages were reclaimed in a real kernel. Backends must be idempotent per (pid,ticket): if a side effect succeeds and its acknowledgment is lost, retry must not double-free. Failed callbacks never advance the phase. Cross-core atomicity and IRQ-safe locking are not implemented.

## IPC PR #16 handoff

- The IPC/capability agent continues to own HandleTable::close_all, ObjectRegistry revoke and generation rules, rights attenuation, endpoint queues, pending calls, ServerGone wakeup.
- The process agent provides a single trusted origin for currently running PID/TID in the model, and an exit cleanup ticket. Do NOT pass untrusted sender_pid into these kernel operations.
- A real adapter must attach kernel-owned handle tables to actual PID lifecycle, close all handles/calls when exit cleanup reaches the proper stages, ensure transactional rollback, prevent stale handle reuse and protect against server crash. No such physical IPC/capability integration is claimed here.

## ServiceManager handoff

- Services agent owns ServiceId and the graph/restart policy. It maps ServiceId to a real PID only after an authorized native launch. It should consume ExitNotification at the Services phase and never allocate a process ID itself.
- ProcessManager does not create zillad, deviced or displayd and does not install new syscalls.

## Evidence and negative test gates

New tests cover parent exit with outstanding TIDs, fault reason propagation, duplicate/pending exits, failed IPC cleanup retry, no early PID reap, stale double thread reap, running-thread refusal, stack overlap/lease lifecycle, active-stack retirement rejection, CPU0-only validation, fake saved frame/IOPL/selector checks, nested IRQ deferral and CPU-local current identity consistency.

Run on Rust 1.90.0 (host target, then bare metal):

    cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline
    cargo +1.90.0 build --release --target x86_64-unknown-none --offline
    cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features vm-demo
    cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features native-ring3
    cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features native-ring3-threads
    cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features native-ring3-preempt
    cargo +1.90.0 fmt --all -- --check

Disposable QEMU evidence is still required for guard fault, kernel access fault, cooperative Ring3 and timer preemption. The existing Ring3 fixtures and feature flags were NOT changed. No GUI or GPU source was modified.

## Open R1 and SMP gates

1. Real per-process owned CR3/page tables and guarded distinct user stacks; kernel-owned memory mapping/reclamation.
2. Full saved register/FPU context, TSS.RSP0/IST, syscall/copy_from_user and fault isolation on independent PID address spaces.
3. IPC handler ownership actually connected to ProcessManager; per-PID HandleTable and true server death wakeups, with rollback and negative tests.
4. ACPI MADT/APIC/x2APIC discovery, AP bringup, per-core descriptor/stacks, synchronization, TLB shootdowns and QEMU -smp 2/-smp 4 stress before enabling multi-core execution.
5. Evidence of actual QEMU/VMware process isolation; compilation and host-only state tests do not prove this.

No force push or merge of PR #18 is authorized or performed.
