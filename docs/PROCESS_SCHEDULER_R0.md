# ZillaOS Process/Scheduler R0 — ownership, preemption and future gates

**Branch:** agent/process-scheduler-r0-20261107 (isolated from main)
**Status (2026-11-07):** R0 pure Rust no_std models implemented, runtime NOT integrated. Nothing here starts a second CPU, installs an IRQ handler, changes page tables, or executes a user-space service.

Read [AGENTS.md](../AGENTS.md), [PROCESSES.md](PROCESSES.md), [ARCHITECTURE_TASKS.md](ARCHITECTURE_TASKS.md) and PR #12's proposed [SYSTEM_SERVICES_IPC.md](https://github.com/kabasik007/ZillaOs/pull/12) before integration. The mainline code and active feature flags are authoritative.

## Scope of this R0 change

- New src/kernel/process/cpu_local.rs: per-logical-CPU *model* for current TID, nested IRQ bookkeeping, preemption-disable nesting, pending reschedule and observed timer-tick count. ThreadTable still owns TID lifecycle, affinity and deterministic round-robin policy; no second runqueue is introduced.
- New src/kernel/process/kernel_stack.rs: bounded high-half virtual kernel-stack *reservation records* with one guard page, no overlapping leases, monotonic lease IDs, explicit reserve/activate/retire/release lifecycle. VMM is responsible for actual physical allocation, NX/RW PTEs, unmapped guards and TLB flushing; this registry never frees a frame.
- Existing src/kernel/process/mod.rs: exports only the two new modules.
- Existing src/services/process_manager.rs: model-only CPU dispatch/snapshot, and reap guard rejecting process PID reaping while ANY associated TID remains, even if the thread has exited. The gateway also rejects selecting Ready threads whose owning process is missing, still Created, or Exited. No untrusted execution enabled.

These additions do not modify src/main.rs, Cargo.toml, src/native_ring3*.rs, graphics, Desktop, PCI, APIC, existing QEMU feature flags or default VMware boot.

## Non-negotiable ownership and state rules

1. PID identifies a process record allocated monotonically by ProcessTable. A process is NOT a thread. Only the canonical ProcessManager owns process lifecycle; the GUI does not fabricate PID or CPU metrics.
2. TID identifies a thread record allocated monotonically by ThreadTable. TID is associated with exactly one PID; TID can be reaped only after Exited. The ProcessManager-facing PID reaper fails with ThreadsRemain if any TID record survives.
3. A running TID has at most one CPU assignment and each CPU may have at most one Running TID. R0 additionally mirrors current TID in CpuLocalState and rejects desynchronized direct ThreadTable mutations. Direct legacy Table access still exists and must not be used concurrently with the new model.
4. Scheduling order stays deterministic: choose a Ready TID matching affinity; rotate by TID after each successful dispatch. Blocked/Exited TIDs are never selected. Only BSP CPU0 is online by default.
5. An interrupt tick requests rescheduling, but may NEVER directly change a thread's state. Nested IRQ or preemption-disabled region defers switching. The host model reports SwitchRequested only after both depths become zero.
6. Descheduling requires SavedTrapContext created with an explicitly unsafe trusted acknowledgment. Only the architecture trap path may create this marker after saving all required CPU/FPU state. An arbitrary syscall cannot assert a context has been saved. This is a *precondition* to model transitions, not a verified trap implementation.
7. Kernel stack reservation is metadata, not memory ownership. A guard stays unmapped; the mapped stack must be supervisor-only, writable, NX, with validated TSS.RSP0 and no in-use frame reclamation. A stack may retire only after CPU/interrupt/TSS quiescence. The unsafe retirement API documents this precondition.
8. A process may not launch a real service until its per-process CR3/PML4, user stacks, fault path and kernel-owned capability table have been installed and validated.

**Important existing limitation:** ThreadTable::create()/prepare() currently permit multiple TIDs to refer to the same *planned* user stack address. This is acceptable for host-only metadata tests, NOT for actual concurrent execution. R1 must allocate distinct guarded user stacks (or reject overlap) before admitting additional threads. The existing isolated QEMU two-thread fixture creates separate real stacks by its own opt-in path.

## R0 state graph

```text
ThreadTable: Created -> Ready -> Running -> Ready (yield/preempt)
                                    |        |
                                    v        v
                                 Blocked -> Ready (wake)
                                    |
                                    v
                                  Exited -> reaped

CPU-local:   Idle --dispatch--> Running
             Running --timer IRQ--> Pending (no switch)
             Pending --nested IRQ / preempt disabled--> Deferred
             Pending --IRQ unwind + saved trap--> Ready + Idle
             Running --saved cooperative yield/block/exit--> Idle
```

The process state model remains Created -> Ready -> Running -> Exited -> reaped. A process's bookkeeping Running state is not proof of Ring3 execution.

## Test and evidence matrix

New host unit/negative tests cover:
- Deterministic fairness over 9 preemption/dispatch cycles for 3 ready TIDs.
- Nested interrupts and preemption guards defer switching; pending request survives.
- Wrong saved TID, no pending request, offline CPU and unsaved/nested context rejection.
- Block -> wake -> dispatch, exit and idle transitions, invalid double wake.
- Desynchronization when external code directly mutates ThreadTable.
- Kernel stack guard geometry; overflow, bad alignment, user VA and oversized stack denial.
- Duplicate TID, overlapping stacks, full registry, double activation/release and stale lease ID.
- Process reaping rejected while a TID remains; reaping succeeds only after thread record cleanup and process Exited.

Commands to run on Rust 1.90.0:

```bash
cargo +1.90.0 fmt --all -- --check
cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features vm-demo
cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features native-ring3-threads
cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features native-ring3-preempt
```

Runtime regression commands, only on a disposable QEMU environment with OVMF/xorriso:
```bash
bash scripts/build.sh ring3-threads
python3 scripts/smoke_test.py --iso dist/zillaos-0.0.2-dev-ring3-threads-uefi.iso --timeout 90
bash scripts/build.sh ring3-preempt
python3 scripts/smoke_test.py --iso dist/zillaos-0.0.2-dev-ring3-preempt-uefi.iso --timeout 45
```

**Evidence status in this authored branch:** Host Rust toolchain and QEMU are unavailable in the current editing environment; tests, compilation, fmt and runtime have NOT been executed here. Existing upstream docs describe prior QEMU evidence for the opt-in fixtures, which must not be misreported as regression evidence for THIS commit. Before merge, record actual command output and COM1 serial markers in the PR.

## R1 — real protected process runtime (NOT IMPLEMENTED)

**Memory owner API** (coordinate with the current memory/paging workstream):
- AddressSpaceId + generation, physical root page-table frame, owned mapping records with RX/RW/NX/U/S, reference counts and per-process frame quotas.
- Commit requires atomic rollback on partial PTE/frame allocation, cannot reuse a stale address-space identifier.
- Distinct guarded *user* stacks per TID; kernel stacks mapped supervisor RW/NX with separate IST and TSS.RSP0 lifecycle. No active stack can be freed.
- On every context switch: trusted trap frame, CR3 transition, TSS.RSP0 update, XSAVE/FPU handling and IRETQ; user-triggered faults terminate only the owned task, kernel faults fail closed.
- Terminate: revoke handles and IPC access through their owning subsystem, detach waiters, stop running TIDs, wait for quiescence, switch to safe kernel stack, unmap and free pages, record ExitReason, then reap PID.
- No public arbitrary ELF/PE execution until separate hardware negative tests verify kernel text denied, guard page trapped, per-process cross-address-space read denied, and two different PIDs execute and clean up safely.

## R2 — SMP design gates (NOT IMPLEMENTED)

1. Parse ACPI RSDP/XSDT/MADT boundedly, validate checksums and entry lengths, enumerate enabled LAPIC/x2APIC IDs without assuming contiguous IDs. Keep CPU0-only fallback.
2. Make CPU-local state physically CPU-local, not a shared mutable array; allocate guarded per-CPU kernel/interrupt stacks, per-CPU GDT/TSS/IST and local APIC timer before AP rendezvous.
3. Establish atomic runqueue ownership transfer, IRQ-safe locks (preempt disabling is not itself a SMP lock), memory ordering, real scheduler idle states, AP startup/shutdown barriers and monotonic clock calibration.
4. Implement inter-processor TLB shootdown with address-space generation/ack deadline, safe frame reclamation after all CPUs acknowledge; global kernel mapping changes need appropriate all-CPU invalidation.
5. Test QEMU -smp 2/-smp 4 with competing wake/exit, IPI stress, inter-CPU migration and fault injection. Never auto-enable in default desktop without these proofs.

## R3 — contracts for Services and IPC agents (NOT IMPLEMENTED)

- Process agent owns the true PID/TID identity and future kernel-owned address-space/handle-table attachment. A raw user-supplied PID is not a capability to access another process.
- IPC agent owns handle object type/rights, generation, revocation, checked copy_from_user, bounded queues, wakeup and sender identity. It should request a kernel scheduler block/wake API rather than mutating ThreadTable directly.
- Services agent owns manifest verification, dependency graph, restart policy and ServiceId -> *real* PID association. It must request an authorized launch via the canonical ProcessManager; never allocate artificial PIDs or run unknown service binaries in Ring0.
- Proposed future interface, subject to ADR and integration: spawn_verified_native(image, verified_capability_bundle, quotas) -> Result<Pid, SpawnError>; wait(pid, authorized_handle) -> ExitReason; kill(handle, reason), not kill arbitrary PID supplied by a client.
- Existing execute_native() must continue to return IsolationUnavailable until R1 and capability/IPC gates are met. Avoid publishing numeric syscall ABI or capability layout before the IPC agent completes ADR-0007/0008.

## Open gates / handoff

- Host tests and fmt pending actual Rust toolchain; bare-metal and QEMU regressions pending CI/hardware.
- Existing ProcessTable exposes direct reap(), and ThreadTable can be manipulated without CpuSchedulerModel. The new ProcessManager guard applies only to callers using its public gateway; a future integration task must centralize ALL lifecycle mutations.
- No actual kernel stack allocator, per-process CR3, scheduler IRQ integration, fault isolation between distinct PIDs, user-space zillad, MADT parsing, APIC IPI, AP boot, TLB shootdown or verified SMP exists in this R0 patch.
- Do not merge with other agent branch edits to ProcessManager without reviewing shared API and the newest main HEAD.
