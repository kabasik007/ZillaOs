# ZillaOS — integration audit / cross-workstream rendezvous (2026-11-07)

> **State:** integration research + staging + actual GitHub Actions evidence. Not a release, not a production microkernel. Do not merge this integration branch without explicit owner approval. Tests below were performed at the precise code SHA recorded per run, **not** automatically at later docs-only commits.

## 1. Git refs and provenance

- Repository: `kabasik007/ZillaOs`
- Pinned integration baseline: `main@fe93f09a5013f90d7e0051e1a25c037e723e4c74`
- Integration branch: `integration/20261107-cross-workstreams`
- Staging snapshot: `4969f8c1b73e163a217fe088056806ca5a30bbf0` (54 unique paths). This is a **combined file tree**, not merges/cherry-picks of upstream commit histories.
- PCI bridge + tests: `c64179adb515f16ce58951fe8e26b71666e99e35`, exported at `3d0e23e082c46a8ceb583e9cd2a7bbc58f6b6e4e`.
- Adapter counter and scoped formatting fixes: `d0c6c9df972890fb7740cc42b9c918e862bd16f9`, `7386d121e6b724029b807dde8209d9056c1cbae2`.
- Integration validation workflow added in `9e67b023a328104aaaae3a0cc1a9bd9c0bf470e6`.
- Draft integration PR: [#19](https://github.com/kabasik007/ZillaOs/pull/19).

| Original PR | Stream | Source head staged | Reuse status |
| --- | --- | --- | --- |
| [#12](https://github.com/kabasik007/ZillaOs/pull/12) | Architecture (10 documentation files) | `36fc690dd163bb88791b30b4e7bce94757c1a100` | Included as docs, main untouched |
| [#13](https://github.com/kabasik007/ZillaOs/pull/13) | AMD A0/A1 | `d052e6a72528dc8f0ca75874945d3413caf7530a` | Exact blobs; read-only PCI bridge added separately |
| [#14](https://github.com/kabasik007/ZillaOs/pull/14) | ServiceManager S0–S3 | `d8dfbdeddf7cbba89e920e177803dd117f72824a` | Exact blobs; still standalone model, not a kernel-linked daemon |
| [#15](https://github.com/kabasik007/ZillaOs/pull/15) | Display G0–G3 | `3d1c728b34cba35825adca7bf92f9da6676d3010` | Exact blobs; no active modeset |
| [#16](https://github.com/kabasik007/ZillaOs/pull/16) | Capabilities and IPC I0–I3 | `9e2640cf97c2514adfeba2b2a7966cc343c4d62e` | Exact blobs except additive `src/lib.rs` reconciliation |
| [#17](https://github.com/kabasik007/ZillaOs/pull/17) | Platform PCI P0–P2 | `4937ab131a3ee515756ae92d057f53781eccbfd1` | Exact blobs except additive `src/lib.rs` reconciliation and `src/platform/mod.rs` bridge export |
| [#18](https://github.com/kabasik007/ZillaOs/pull/18) | Process and CPU-local R0 | `ff71d239197cab33f6407376b7b6bed4f6f955fc` | Exact blobs; no live SMP |

**Commit safety:** upstream PRs remain independent and unmerged. The integration snapshot reuses blob content, not the individual PR parent histories. Integrators must reconcile the original histories deliberately at final merge time; do not re-apply overlapping `src/lib.rs` edits without reviewing the resulting tree.

## 2. One owner per resource — observed truth

| Resource / authority | Canonical source | Current level in this branch | Missing production boundary |
| --- | --- | --- | --- |
| PCI identity, resource inventory | `src/platform/pci/**`, `src/platform/device/registry.rs` | Bounded host-testable discovery and DeviceRegistry **model**; generation-safe IDs | Existing boot PCI I/O and VMware/storage consumers still scan independently; no kernel-owned single runtime registry |
| PID/thread state | `src/kernel/process/**`, `src/services/process_manager.rs` | One gateway/model, CPU-local dispatch and guarded stack metadata | No generic user process runtime, kernel stack activation or SMP scheduler |
| Display mode and output | `src/video/display_manager.rs`, `src/video/display_contracts.rs` | One fail-closed DisplayManager model; output/surface identity registries | No native runtime modeset backend or compositor IPC |
| Application launch | `src/services/app_manager.rs` | One built-in application routing gateway | No arbitrary ELF/PE executable launch |
| Window focus/geometry/z-order | `src/shell/window_manager.rs` | Single WindowManager in embedded desktop | No separate user-space WindowManager |
| Service lifecycle | `src/services/service_manager.rs` | Bounded isolated host/no_std model; execute returns Unsupported | No zillad, authenticated PID, real spawn, revocation/cleanup |
| Object rights and IPC | `src/kernel/capabilities/**`, `src/kernel/ipc/**` | Host-testable bounded handle/queue models, no hardware | No trusted kernel syscall/CPU task identity enforcement; no end-to-end runtime isolation |

**No new singleton global device inventory is created by the bridge.** `src/platform/amd_bridge.rs` only converts canonical Platform identity to AMD's A0 read-only consumer type; it preserves segment, BDF, class, revision, subsystem IDs and denies malformed input. VMware's identity remains distinct in the common Platform contract. AMD's local value type is **transitional** and should be retired after its owner migrates the API.

## 3. Known compatibility conflicts and dispositions

| ID | Conflict | Resolution now | Remaining gate |
| --- | --- | --- | --- |
| C1 | #16 and #17 both append exports to `src/lib.rs` | One additive integration file exports `capabilities`, `ipc`, `platform`; host and bare-metal compile verified | Confirm future rebases against moving `main` |
| C2 | #13 uses AMD-local `PciAddress/PciIdentity`; #17 supplies canonical Platform types | Read-only `AmdInventoryAdapter` + four new tests; no extra scan or MMIO | Migrate production AMD API to canonical identity once owners agree |
| C3 | Existing `src/pci_io.rs` invokes separate storage/video/VMware scans | Left untouched to preserve known GUI and VMware behavior | [#20](https://github.com/kabasik007/ZillaOs/issues/20) replace with one shared snapshot and compatibility adapters |
| C4 | #14 ServiceManager has no real ProcessManager PID or capability grant source | Model intentionally isolated and standalone-tested; no fake daemon | [#21](https://github.com/kabasik007/ZillaOs/issues/21), [#22](https://github.com/kabasik007/ZillaOs/issues/22) |
| C5 | #15 ModeBackend can represent safe transactions but hardware implementation absent | Default-deny TEST_ONLY preserved; no GPU modeset/FIFO activation | Verified per-device backend, leases, rollback; GOP not guaranteed after native reprogramming |
| C6 | IPC model objects are not tied to live process memory/real syscalls | No premature syscalls or user-space drivers | Authenticate PID, user buffer copying, revoke/teardown and atomic transfer |
| C7 | Repository-wide formatting drift | Owned bridge passes scoped rustfmt | [#23](https://github.com/kabasik007/ZillaOs/issues/23) owner-scoped formatting; avoid blanket formatting other agents' source |

## 4. Verified tests (exact SHA)

**Code tested:** `7386d121e6b724029b807dde8209d9056c1cbae2`  
**Run:** [Integration gate 42332189295](https://github.com/kabasik007/ZillaOs/actions/runs/42332189295), Ubuntu 24.04 / Rust 1.90.0.

| Test | Result | Evidence and limits |
| --- | --- | --- |
| Host Rust `cargo test --lib` | **PASS** | 340 tests, including four Platform–AMD adapter tests; 0 failed |
| Standalone ServiceManager Rust tests | **PASS** | 20 tests, 0 failed; model, not a service process |
| ServiceManager `no_std` metadata compile | **PASS** | Bare-metal typecheck only |
| Python `unittest discover` | **PASS** | 127 tests |
| Bare-metal default, `vm-demo`, `native-ring3-threads`, `native-ring3-preempt` | **PASS** | All four compile; not proof of SMP |
| QEMU experimental native Ring3 fixture | **PASS** | Real serial smoke on disposable CI runner |
| QEMU user guard-page #PF negative fixture | **PASS** | User page fault contained; no kernel fatal marker |
| QEMU supervisor-boundary negative fixture | **PASS** | User read denied and control returned |
| Scoped rustfmt of new AMD bridge | **PASS** | `rustfmt +1.90.0 --edition 2024 --check src/platform/amd_bridge.rs` |
| Repository-wide `cargo fmt --all -- --check` | **FAIL** | 942 rustfmt hunks over 79 paths, including both pre-existing/untouched files and imported agent code; no blanket reformat applied |
| VMware workstation GUI ISO after integration | **NOT RUN** | Requires Windows/WSL/VMware evidence from owner |
| Physical AMD card, native GPU MMIO/DMA | **NOT RUN** | No physical environment or explicit hardware approval |
| Live SMP, user-space service runtime, real IPC syscalls | **NOT RUN / NOT IMPLEMENTED** | Host tests and QEMU fixtures do not imply these exist |

Negative coverage **in models**: malformed PCI, bridges, resource claims, disabled MMIO/IRQ/DMA, stale capability handles, IPC lengths and revocation, stack leases, output handle generations. No claim is made that these are all tested against real native PCI hardware or live Ring3 services.

## 5. Dependency matrix and integration boundaries

```text
[Platform PCI P0/P1: read-only identity and DeviceRegistry]
      |              \
      |               +---> AMD A0 matcher (no hardware activation)
      +---> VMware / Storage adapters (still blocked: #20)
      |
      +---> resource lease -> capability handle (blocked: #21/#22)

[Process metadata / R0 CPU scheduler] ---> trusted PID ownership (blocked: #21)
             |                                      |
             +-----------> IPC/cap revoke ----------+
                                                    |
                              ServiceManager S0-S3 -> zillad (future)

[WindowManager (existing)] ---> SurfaceId/OutputId --> DisplayManager
                                                            |
                                                VMware mode backend / AMD
                                                hardware backends (future)
```

Kernel must remain the authority for PCI/frames/resource rights and task identity. Future services must be separately authenticated, quota-bounded and cancellable. User shell must only consume snapshots, never make itself the security authority.

## 6. Blocking issue register

- [#20 — Canonical PCI handoff](https://github.com/kabasik007/ZillaOs/issues/20): unify boot PCI discovery without breaking storage/VMware.
- [#21 — Kernel PID/cap lifecycle](https://github.com/kabasik007/ZillaOs/issues/21): handle authorization, owner provenance, exit cleanup.
- [#22 — Typed service/device/display interface](https://github.com/kabasik007/ZillaOs/issues/22): no fake daemon, no hardware activation.
- [#23 — Formatting debt](https://github.com/kabasik007/ZillaOs/issues/23): owner-scoped fix and repository-wide gate.

## 7. Recommended next milestone: I-M1 — One Inventory, One Identity, One Lifecycle

**Ship a host-testable resource and lifecycle handoff, not GPU acceleration.**

1. Route all **read-only** boot PCI consumers through one Platform-owned snapshot with strict segment-zero adapter for CF8/CFC, no direct second scan, no config writes.
2. Make DeviceId and kernel ObjectId relationships explicit and test generation/revoke semantics across process/service simulated exit. No active DMA/MMIO/IRQ lease yet.
3. Define typed service/device/display messages and authentic kernel caller identity, but retain real daemon spawn disabled until an isolated Ring3 runtime exists.
4. Keep old Desktop/VMware behavior, all host regressions and QEMU Ring3/fault negatives passing; make rustfmt scope green before promoting to broad gate.
5. Independently log VMware Workstation ISO boot/screenshots if Windows runner is available. Never treat QEMU or host models as proof for physical AMD.

**Acceptance:** exactly one canonical registry/state owner per resource, zero duplicated hardware PCI scans, reproducible CI evidence by SHA, no change to main until explicit approval. Next milestone after I-M1 is I-M2 (real capability checked IPC + protected service lifetime), not AMD native display modesetting.

## 8. Safety rules for the next integrator

- No force push, no hard reset, no bulk overwrite of another stream, no unreviewed merge to `main`.
- Re-read `main` and each PR head before applying more changes: parallel agents can advance both.
- Keep PR #19 **draft**; source PRs are still separate. Treat it as a live compatibility benchmark, not a competing permanent feature branch.
- Do not turn on physical AMD MMIO/DMA, PCI config writes, bus mastering or disk writes.
- Do not claim GPU acceleration, true isolated services or SMP until runtime evidence on actual implementation.
