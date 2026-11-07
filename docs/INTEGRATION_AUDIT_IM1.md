# ZillaOS I-M1 Cross-Agent Integration Audit — 2026-11-07

> **HISTORICAL AUDIT:** STAGED/NOT APPROVED below describes PR #19 *before* it merged. The consolidated code and Rustfmt PR #26 were merged later. Current [STATUS](STATUS.md); preserve SHA/decisions as audit evidence.

Status: **STAGED FOR INDEPENDENT INTEGRATION CI**, not approved for main.

## Immutable input heads

- main: `fe93f09a5013f90d7e0051e1a25c037e723e4c74`
- Architecture #12: `36fc690dd163bb88791b30b4e7bce94757c1a100`
- Graphics #15: `3d1c728b34cba35825adca7bf92f9da6676d3010`
- Platform foundation #17: `4937ab131a3ee515756ae92d057f53781eccbfd1`
- Platform I-M1 #24: `be082dd9582b3bc3a3f40ebe4d76407896ebc781`
- AMD #13: `ef35ecac312161f674dc7b04eb6d7989cef80908`
- ServiceManager #14: `20895ecf372b4020a696aca2b9a3029f507873b5`
- IPC #16: `ad9ad72d3abeae16ab6ad64490db2063e4a2cc7a`
- Process #18: `2279405b7f695771e07ac9bca89cf6ff957580f8`
- Baseline integration PR #19: `ef88436c8e54bea28be816701e01a9b928c93a08`

## Integration strategy and conflict disposition

This staged tree starts from Platform I-M1 #24, which itself builds on PR #19. It overlays **only agent-owned changed files** from AMD, ServiceManager, IPC and Process. Shared `src/lib.rs`, Platform `src/platform/mod.rs`, and Platform boot PCI ownership are preserved.

The collision in `src/platform/amd_bridge.rs` was explicitly resolved by retaining both:
1. Platform's registry-backed `AmdRegistryAdapter` and stale-handle test, used by the kernel's single PCI boot inventory.
2. AMD's registry-scoped `diagnose_registry_device`, bounded diagnostics, generation-safe revalidation and negative tests.

`src/services/mod.rs` exports the service-manager model (not a daemon or launcher). The integration gate also runs both typed ServiceSupervisor host test suites, including canonical kernel `Pid` fixtures.

PR #19 is the staging / evidence PR; this is **not** an authorization to merge to `main`.

## Existing evidence on individual branches

- Platform I-M1 workflow #42333801643: PASS, host builds and disposable QEMU/OVMF; physical VMware/AMD NOT RUN.
- AMD workflow #42333782901: PASS; full integration formatting job on that branch FAILED.
- Services workflow #42334008705: PASS; actual user-space daemon NOT RUN.
- IPC workflow #42333996272: PASS; real kernel IPC syscalls NOT RUN.
- Process workflow #42334062338: PASS, host and QEMU Ring3 feature fixtures; genuine SMP NOT RUN.
- Graphics workflow #42331333899: PASS for earlier G0/G1 branch.
- Previous integration gate #42332189295: host/bare-metal/QEMU PASS, repo-wide Rust formatting FAIL.

## Mandatory new CI review

- Host `cargo test --lib`, ServiceManager host tests, Supervisor host/PID tests, Python regressions.
- Bare-metal build matrix: default, vm-demo, Ring3 threads and preemption.
- QEMU Ring3, guard-page #PF and kernel boundary checks.
- Scoped formatting and **repository-wide rustfmt** independently recorded. The existing widespread formatting drift (Issue #23) must not be silently hidden.
- If CI rejects an integration API, fix the integration branch with minimal owner-respecting changes and re-run.

## Security / runtime boundary

Single PCI discovery remains read-only: no BAR sizing/config writes, MMIO/DMA/IRQ enabling, disk writes or AMD hardware initialization. ServiceManager and capability cleanup hooks remain model-based until proven in production kernel runtime. CPU0 scheduler state is **not SMP**. A CI host/mock pass is **not physical hardware evidence**.

## Open issues

- #20 Platform: verify single discovery, read-only consumer parity in QEMU and VMware.
- #21 Process/IPC: real trusted task binding and ordered backend cleanup still require an implementation proof.
- #22 Services/Device/Display: typed authorization and actual lifecycle backends are incomplete.
- #23 Formatting: repo-wide drift is a separate mandatory quality blocker.

**Decision:** HOLD merge to main pending integrated CI and explicit approval.
