# ZillaOS — Clean Baseline & Next Work Wave (I-M2)

**Date:** 2026-11-07  
**Canonical development branch:** `main` (after merge of integration PR #19).  
**Previous gate:** [Integration PR #19](https://github.com/kabasik007/ZillaOs/pull/19) + `docs/INTEGRATION_AUDIT_IM1.md`.

## What is actually integrated

- Canonical Platform PCI identity, bounded single-boot inventory/registry, read-only Storage/VMware/AMD handoff.
- AMD read-only diagnostics + registry generation revalidation; **no GPU initialization**.
- IPC queues, rights/capability lifecycle models and ProcessManager-owned trusted PID/exit hooks; **no production user syscall binding**.
- ServiceManager supervised lifecycle/typed PID model; **no launched zillad or real user-space service**.
- DisplayManager and display contracts; **no native accelerated GPU modeset**.
- CPU0 Process/Scheduler model and independent native Ring3 QEMU fixtures; **no general multi-core SMP runtime**.
- Existing software Desktop, Notepad and VMware-oriented builder retained.

## I-M1 measured evidence

Run [42334822081](https://github.com/kabasik007/ZillaOs/actions/runs/42334822081) on integrated source `abc0d09c2adcd7df48adc1202604eb2d6b9f1f99`:
**385 Rust lib + 20 Services + 14 ServiceSupervisor + 34 canonical PID + 127 Python tests PASS (580 total)**; 4 bare-metal feature builds PASS; Ring3, guard-fault, supervisor-access QEMU smoke PASS. Dedicated process, platform (including QEMU), PCI, AMD, IPC, Graphics, Services and Windows Builder workflows PASS on that code revision.

**Known blocking quality debt:** repository-wide `cargo +1.90.0 fmt --all -- --check` FAILS due to formatting differences across existing and new files. Narrow Platform rustfmt issues were repaired. This merge is a development **baseline**, not a stable release or security claim. Track #23 and do not suppress the failing check.

## Single-source ownership for I-M2

| Stream | Exclusive primary ownership | Contract with others | Explicit non-goals |
|---|---|---|---|
| **A — Platform / PCI** | `src/platform/**`, `src/pci_io.rs` | immutable DeviceId/PCI/DeviceRegistry, read-only Storage/VMware/AMD snapshots | no new PCI scanners, no DMA/MMIO bus mastering |
| **B — Process / Scheduler** | `src/kernel/process/**`, `src/services/process_manager.rs` | trusted PID/TID and typed authenticated teardown events | no fake SMP, no services registry |
| **C — IPC / Capabilities** | `src/kernel/ipc/**`, `src/kernel/capabilities/**` | rights, revocation and PID-owned handle cleanup | no caller PID in untrusted message |
| **D — ServiceManager** | `src/services/service_manager.rs`, `src/services/service_supervisor.rs`, `src/services/service_manifests.rs` | consume B/C lifecycle events; require cleanup confirmation before restart | no fabricated process launcher/zillad |
| **E — Graphics / Display / VMware** | `src/video/**`, `src/drivers/gpu/vmware_svga/**` | output/surface lifetime, capability and Platform DeviceId | no second WindowManager, no unsafe FIFO/MMIO activation |
| **F — AMD Radeon** | `src/drivers/gpu/amd/**` | consume Platform registry / diagnostics via agreed adapter | no GPU acceleration, ASIC-specific hardware access without lab validation |

**Chief Architect exclusively coordinates** changes in `src/main.rs`, `src/lib.rs`, `Cargo.toml`, `src/services/mod.rs`, `src/platform/amd_bridge.rs`, `src/shell/**` and cross-stream CI. A specific shared file can be delegated through an explicit documented contract; no duplicate ownership or blind overwrites.

## I-M2 tasks in priority order

**M2-0 / CI quality (blocking):** close issue #23 by committing owner-reviewed, mechanics-only rustfmt fixes in small sections and making `cargo +1.90.0 fmt --all -- --check` green. No mixing semantic refactors with format changes. Retain equivalent host/bare-metal/QEMU evidence.

**M2-1 / Trusted lifecycle (B+C first):** bind one kernel-authoritative current task identity to process-scoped capability handles and IPC queues. Hook genuine exit/fault events into idempotent revoke → IPC cleanup → service notice. Reject forged sender/stale handles/double reap. Keep user-facing syscalls disabled until independently vetted paging and trap boundaries are in place (#21).

**M2-2 / Service supervision (D):** consume B/C typed contracts with real authenticated metadata events; improve restart and dependency cleanup ordering. No synthetic launch success, no daemon until a safely isolated execution backend exists (#22).

**M2-3 / Device and graphics integration (A+E+F):** verify canonical DeviceId across startup, VMware and AMD, avoid a second PCI scan; expose view-only device diagnostics; ensure DisplayManager mode transactions fail closed and software Desktop fallback is preserved. Physical VMware workstation/AMD labs are separately labeled NOT RUN if unavailable (#20/#22).

**M2-4 / Kernel execution correctness (B):** extend negative Ring3 and scheduler stress tests, quiescent kernel stacks, bounded CPU-local ordering. Design SMP contracts but do not claim running AP cores.

## New agent workflow (the reset rule)

1. Read latest `main`, `AGENTS.md`, this document, and relevant owner module/docs. **Discard all stale feature-chat base SHAs.**
2. Each agent creates a **new** branch `agent/<domain>-im2-<task>` based on the **current** `main` SHA and opens a draft PR. Old I-M1 branches are retired; do not reuse them.
3. Make a substantial, bounded implementation with negative tests. Commit normally (non-force). No direct push to main. Shared APIs require an approved minimal typed contract before code changes.
4. Report: base/head SHA, exact changed files, tests with URLs, unverified runtime claims, blocking dependencies and next milestone. Mark PASS, FAIL or NOT RUN precisely.
5. Integration owner reads all PRs anew, validates cross-stream diffs, merges into staging candidate, runs full CI/boot/fault tests, and merges to main only when explicitly authorized.
6. **After merge:** start the following wave from new main; close completed PRs and remove branches only after confirming their tip commits are ancestors of main.

## Cleanup of legacy branches

The complete commit graph was preserved through the multi-parent integration staging history. The old `fix/windows-wsl-path-build` branch included seven unique historical commits that are retained as a separate **ancestry parent**, but its old file versions intentionally do not replace newer Windows/WSL implementation on main. This is a history-preserving merge, not a fresh cherry-pick of obsolete code.

`scripts/cleanup-i-m1-branches.ps1` provides an explicitly gated, **dry-run by default** deletion of the ten I-M1 branches after their tips are proven ancestors of merged main. No force push or `git reset` is used.

## Security gates before production claims

- Never run unsigned/untrusted executables in ring 0.
- Do not enable GPU/PCI MMIO/DMA/IRQ, PCI config device writes, firmware loading or disk writes without explicit sandbox/lab and reviewed invariants.
- Model and mock host checks do **not** demonstrate safe production syscall binding, physical GPU support or working SMP.
- Use disposable QEMU UEFI images, preserve GOP/software framebuffer fallback, and verify Windows/WSL builder separately.
