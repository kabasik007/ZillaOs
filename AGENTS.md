# AGENTS.md — ZillaOS development contract (I-M2)

**Last coordinated reset:** 2026-11-07.  
**Canonical repository:** https://github.com/kabasik007/ZillaOs  
**Canonical code branch:** `main`. Always fetch the **current main HEAD**, not a SHA remembered from a previous chat.  
**Handoff for fresh chats:** [docs/NEW_CHAT_START_I_M2.md](docs/NEW_CHAT_START_I_M2.md).  
**Owner matrix and next milestones:** [docs/NEXT_WAVE_I_M2.md](docs/NEXT_WAVE_I_M2.md).

## Spec-first behavior contracts

Before changing a kernel/user ABI, process/IPC/device lifecycle, rights, stateful feature or user-visible behavior, create or update a source-backed [specification](docs/specs/README.md). Record facts, non-goals, invariants, negative/failure cases and target-specific evidence. Chief Architect reviews shared contracts before agent code changes. Small no-behavior formatting changes need no spec. Documentation routing: [docs/README](docs/README.md); adoption analysis: [BOOTSTRAP_ADOPTION](docs/BOOTSTRAP_ADOPTION.md).

## Non-negotiable workflow

1. Read current `main`, `README.md`, this file, `docs/AGENT_COORDINATION.md`, `docs/ARCHITECTURE_CURRENT.md`, `docs/SECURITY_ARCHITECTURE.md` and your subsystem docs **before** modifying code.
2. Each engineering agent creates its **own new** `agent/<domain>-im2-<task>` branch from current `main`, opens a draft PR and confines writes to its owned source. No direct agent commits to `main`.
3. Chief Architect alone owns cross-module reconciliation, shared kernel exports and integration PRs. Changes to shared files require an explicit contract, latest-blob read and minimal diff.
4. Use normal, non-force updates; read back returned SHA. No force push, losing reset/rebase, blind overwrite, copy of stale full file, or deletion of other agents' work.
5. For a substantial tranche, provide code, negative/adversarial tests, documentation and CI evidence. Do not stop at a plan if safe implementation is possible.
6. Never claim host unit tests, compile success or QEMU mock fixtures prove a real hardware driver, running daemon, general process isolation or SMP.

## Single-writer source ownership

| Agent | Exclusive primary code | Must consume | Do not duplicate |
|---|---|---|---|
| A — Platform / PCI | `src/platform/**`, `src/pci_io.rs` (except shared `src/platform/amd_bridge.rs`) | canonical PCI/DeviceId state, bounded inventory, read-only device snapshot | second PCI enumerator/DeviceManager |
| B — Process / Scheduler | `src/kernel/process/**`, `src/services/process_manager.rs` | trusted task identity, process/thread exit lifecycle | capability or service registry |
| C — IPC / Capabilities | `src/kernel/ipc/**`, `src/kernel/capabilities/**` | PID/TID identity exclusively from ProcessManager, rights and revocation | own PID table; user message as trusted PID |
| D — ServiceManager | `src/services/service_manager.rs`, `src/services/service_supervisor.rs`, `src/services/service_manifests.rs` | authenticated process events; capability-cleanup proof | launcher or synthetic working zillad |
| E — Graphics / Display / VMware | `src/video/**`, `src/drivers/gpu/vmware_svga/**` | Platform DeviceId, output/surface ownership | WindowManager, PCI scan |
| F — AMD Radeon | `src/drivers/gpu/amd/**` | Platform PCI registry read-only identity | MMIO/DMA/ASIC enablement without lab proof |

**Chief Architect owns:** `src/main.rs`, `src/lib.rs`, `Cargo.toml`, `src/services/mod.rs`, `src/platform/amd_bridge.rs`, `src/shell/**`, `src/services/app_manager.rs`, `src/graphics.rs`, shared workflows, build and architecture documentation. Shared ownership may be explicitly delegated for one task, never silently assumed.

## Architectural invariants

- `ApplicationManager` routes application launches; `ProcessManager` owns PID/TID; `WindowManager` owns windows; `DeviceRegistry` owns device handles; `DisplayManager` owns display output contracts. Exactly one owner of each.
- Current Rust `no_std` x86-64 kernel is not Linux. Limine / UEFI and Windows/WSL builder are infrastructure, not dependencies in the kernel runtime.
- Ring 3 QEMU fixtures demonstrate specific isolated experimental paths, **not** arbitrary ELF/PE execution or production user processes.
- ServiceManager supervisor and IPC/capability lifetime models exist; actual user-space `zillad`, syscall IPC kernel binding, safe hardware resource grants and SMP runtime require future evidence.
- AMD A0 read-only candidate diagnostics do **not** authorize GPU initialization or acceleration.
- Never execute untrusted PE/ELF/APK in Ring 0. Fail-closed memory, length/pointer validation, quotas, W^X/NX, rights checking at every access.
- No physical PCI device writes, BAR sizing, GPU MMIO, DMA, IRQ or firmware loading; no writes to physical disks, host firmware or NVRAM without separate reviewed permission. Hardware experiments only under explicit documented lab safeguards.

## Required checks and evidence

```bash
cargo +1.90.0 fmt --all -- --check
cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features vm-demo
python3 -m unittest discover -s tests -v
```

Depending on owner: feature-specific bare-metal builds, ServiceManager host-model tests, QEMU Ring3/guard/kernel-fault and PCI/desktop smoke. Link to real CI run **on the exact tested commit**. Record PASS, FAIL, NOT RUN separately.

Development baseline I-M1 PR [#19](https://github.com/kabasik007/ZillaOs/pull/19) and repository Rustfmt repair PR [#26](https://github.com/kabasik007/ZillaOs/pull/26) are merged. All nine specialized/combined PR workflows were green on the PR #26 formatting head; new commits require their own validation. Ten prior I-M1 remote branches were pruned with commit ancestry preserved.

Windows/WSL end-user check:
```powershell
cd M:\projects\ZillaOs
git switch main
git pull --ff-only origin main
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows.ps1 -Action BuildRun
```

Do not claim VMware GUI boot or physical AMD validation without direct user/CI evidence.

## Completion report (mandatory)

Provide: (1) base/head SHA and PR link; (2) files changed; (3) implementation and API contracts; (4) exact tests and CI URLs; (5) negative tests/security claims; (6) known blockers and next wave handoff. Do not merge into main without Chief Architect integration/explicit owner approval.
