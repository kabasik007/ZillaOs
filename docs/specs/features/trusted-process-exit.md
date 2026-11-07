# SPEC: trusted process exit → capability + IPC cleanup

**ID:** I-M2-LIFECYCLE-001 · **Status:** Draft / Blocked for production runtime  
**Owner:** B — Process/Scheduler, C — IPC/Capabilities, D — Services as consumer  
**As of:** 2026-11-07  
**Evidence:** [Process model](../../PROCESS_LIFECYCLE_IM1.md), [IPC model](../../IPC_CAPABILITIES_FOUNDATION.md), [Service bridge](../../SERVICE_MANAGER_PROCESS_BRIDGE.md), [Issue #21](https://github.com/kabasik007/ZillaOs/issues/21).

## Goal

Коли kernel-authenticated процес завершується або падає, його права, pending IPC і service dependencies мають бути послідовно відв'язані без використання PID з недовіреного payload і без повторного застосування stale handles.

## Boundary and non-goals

- **Current**: є `no_std` моделі PID/TID, cleanup tickets, rights/queues, host-negative tests **і прямий model-only ProcessManager → IPC cleanup bridge** (`drive_exit_ipc_cleanup_model`). Він використовує одну канонічну таблицю PID, викликає існуючий IPC `process_exit_cleanup()` та підтверджує обидві стадії лише після успіху; це все ще не production syscall/runtime.
- **Future**: kernel trap entry встановлює trusted identity, ProcessManager випускає exit ticket; process-bound capabilities та IPC cleanup підтверджуються до PID reuse і restart.
- **NOT in scope now**: довільний user-space syscall ABI, справжній SMP scheduling, physical memory free, IRQ/DMA teardown і працюючий `zillad` без окремих доказів.

## Invariants

1. `current_pid` отримано тільки від перевіреного kernel/CPU task state, ніколи від `sender_pid` з користувацького повідомлення.
2. Handle має owner PID + generation + rights; stale/foreign/revoked handle відхиляється на кожному доступі.
3. Cleanup послідовний і idempotent: capabilities revoke → IPC queue/call cancellation → Services notice → memory/stack resource gate; retries не подвоюють release.
4. Service restart не видає новий binding, доки kernel не підтвердив teardown старого; ServiceId ≠ PID.
5. Невдале cleanup підтвердження залишає процес у безпечному pending-state, не вільним для передчасного reuse.

## Adversarial cases

| Scenario | Expected |
| --- | --- |
| Forged IPC sender identity | Reject; privileged PID not accepted from payload |
| Duplicate ticket / reply / close | Idempotent or explicit stale rejection |
| Server crash with pending calls | Bound queues drained/cancelled; callers unblocked deterministically |
| Generation wrap/stale PID or TID | Reuse denied until safe lifetime proof |
| Cleanup step fails | Continue only via defined retry/rollback, never fake success |

## Реалізований model bridge у main (2026-11-07)

Власник B додав у `src/services/process_manager.rs`:

- `process_table_model() -> &ProcessTable`: read-only канонічні PID-записи для kernel-only моделі IPC; не приймає sender PID з мережі.
- `drive_exit_ipc_cleanup_model(ticket, book, registry, calls, queues)`: перша перевірка `ExitCoordinator` має бути на стадії `Capabilities`. Викликає **існуючий** `crate::ipc::lifecycle::process_exit_cleanup()`; після його успіху разом підтверджує `Capabilities` та `Ipc` і залишає стадію `Services`. IPC-власник, а не ProcessManager, виконує закриття handle book, відкликання об'єктів, зупинку endpoint, `ServerGone` та очищення клієнтських викликів.
- Невдалий preflight (`NotBound`, невірний ticket/phase) не змінює cleanup journal, queue чи реєстр. Друга спроба після успіху не звільняє ресурси повторно.
- `reap_native_model()` більше не може успішно звільнити PID без cleanup ticket: для unjournaled Exited повертає `CleanupRequired`. Єдиний model gateway для остаточного PID reap — `finish_exit_model(ticket)` після всіх callback-ACK та reaping TID.
- Негативні тести з'єднують ProcessManager з реальними **модельними** ObjectRegistry / ProcessHandleBook / IpcQueue / CallTracker і перевіряють `ServerGone`, revoked handles та заборону раннього reap.

**Безпекова межа:** IPC adapter сьогодні використовує перевірку стану `ProcessTable::Running` у host-model, а не апаратний trap-current PID/TID. `process_table_model()` не можна давати userland; `CleanupBackend` залишається моделлю. Відсутні kernel IRQ-safe locking, SMP atomicity, TSS/CR3 user tasks, фактичне release physical pages, живий IPC syscall і підписаний ServiceManager cleanup proof. Issue #21 лишається відкритим до цих незалежних доказів.

## Verification gates (not yet completed in production)

- Host-model tests for forged sender, stale handles, callbacks and double reap: existing tests **PASS**.
- End-to-end production Ring3 caller → syscall → IPC → exit revoke: **NOT RUN / NOT IMPLEMENTED**.
- SMP concurrent races, physical IRQ/DMA quiescence: **NOT RUN**.

**Ready decision:** blocked until B+C agree a concrete trusted trap/capability adapter and demonstrate its controlled runtime with negative tests. Review before writing kernel entrypoint code.
