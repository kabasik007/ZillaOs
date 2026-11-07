# ZillaOS — Parallel Engineering Workstreams (architecture → code gates)

**Read before delegating to Codex agents.** This is a recommended task plan, not a command to run all dangerous hardware experiments at once. Repository `kabasik007/ZillaOs`, snapshot 2026-11-07. Recheck current HEAD and `AGENTS.md` before every edit.

## 1. Coordination: one owner per shared module

| Workstream | Files it can own | Blocked/coordination needed |
| --- | --- | --- |
| **P — Platform/PCI** | new `src/bus/pci/**`, new pure DeviceRegistry modules | `src/pci_io.rs`, `src/lib.rs`, `main.rs` shared |
| **A — AMD GPU** | new `src/drivers/gpu/amd/**` | `src/drivers/gpu/mod.rs` owned by existing Graphics/SVGA |
| **G — Graphics/SVGA** | existing `src/drivers/gpu/vmware_svga/**`, `src/video/**` | owns display integration, coordinate with A and P |
| **S — Services** | new `src/services/service_manager.rs` and pure manifest/graph tests | `src/services/mod.rs` shared; no real spawn |
| **I — IPC/Security** | proposed `src/kernel/ipc/**`, `src/kernel/capabilities/**` | coordinated with existing Process agent |
| **R — Process/Runtime** | `src/kernel/process/**`, `src/services/process_manager.rs`, Ring3 fixtures | no concurrent changes by S/I to same files |
| **D — Docs/Quality** | `docs/OS_*.md`, `docs/*FRAMEWORK*.md`, architecture and CI docs with approval | `README.md`, `AGENTS.md` and other agent documentation shared |

**Shared touchpoints:** `src/main.rs`, `src/lib.rs`, `Cargo.toml`, `src/pci_io.rs`, `src/services/mod.rs`, `src/drivers/gpu/mod.rs`, CI workflows. Integration maintainer applies minimal diff after reading latest HEAD and preserving other agents' changes.

## 2. Current capability ladder / evidence standard

As of latest checked code, there are opt-in fixtures for single Ring 3, guard faults, cooperative two-thread switching, and **single-core PIT IRQ0 timer-preempted switching** (`native-ring3-preempt`), documented in `docs/PROCESSES.md`. They do **not** imply production interruptible scheduler, SMP or real services. The normal VMware desktop remains a kernel-embedded demo.

For every workstream report:
- `SOURCE`: files, commit SHA, exact branch/PR.
- `HOST_TEST`: command, PASS/FAIL, output.
- `BARE_METAL_BUILD`: target + feature, PASS/FAIL.
- `QEMU_RUNTIME`: ISO, feature, COM1 capture, expected and actual markers.
- `VMWARE_RUNTIME`: BuildRun, serial/screenshot, PASS/FAIL or NOT_RUN.
- `PHYSICAL_AMD`: PCI ID, experimental phase, evidence, or NOT_RUN.
- `BLOCKERS`: specific dependency, no capability inflation.

Never call an unsigned/untested new doc "working GPU driver".

## 3. Parallel batches

### Batch 0: no common-file changes (agents can start simultaneously)

**P0.1** — pure PCI identity and read-only parser
- Input: byte-aligned config fake, segment/BDF, vendor/device, class/subclass/prog-if.
- Output: immutable bounded `PciDeviceInfo`.
- Tests: absent devices, multifunction, bridge cycles, segment unsupported, endian/alignment, capacity overflow.
- No access to ports/BAR writes. Source: [PCI_ACPI_RESOURCE_MODEL.md](PCI_ACPI_RESOURCE_MODEL.md).

**A0.1** — pure AMD matcher
- Input: `PciIdentity` data from shared descriptor contract (or temporary adapter, no own scanner).
- Output: `DetectedUnsupported` for unknown Radeon display; `NoMatch` for AMD platform `0x1022` and VMware `0x15ad`.
- Tests: vendor/class/ID/revision negatives; no register writes.
- No `src/video/mod.rs` edit. Source: [AMD_GPU_BRINGUP.md](AMD_GPU_BRINGUP.md).

**S0.1** — pure manifest/dependency graph
- Input: bounded static test manifests.
- Output: startup order, `Rejected` on cycles, duplicates, excessive rights/invalid dependencies.
- Tests: deterministic topological sort, restart limits with mock clock, no fabricated PID.
- No `execute_native` changes. Source: [SERVICE_FRAMEWORK.md](SERVICE_FRAMEWORK.md).

**I0.1** — pure IPC envelope/capability model
- Validate version/header/payload bounds, generation+rights intersection/revoke, build with `no_std`.
- No syscall/Process changes in this batch.
- Source: [SYSTEM_SERVICES_IPC.md](SYSTEM_SERVICES_IPC.md).

**G0.1** — existing Graphics agent continues verified VMware SVGA; contract work for Output/Surface/Mode with mock only. Source: [DISPLAY_STACK_PROTOCOL.md](DISPLAY_STACK_PROTOCOL.md).

### Batch 1: integration rendezvous

**INT1** — agree on shared `PciAddress/PciIdentity` and `DeviceSnapshot` type (P + A + G). Test that VMware & AMD consume same fake PCI bus and that storage remains unaffected.

**INT2** — one integration owner adds minimal modules into `src/lib.rs`/GPU mod and tests pure no_std import compatibility. No change to default device behavior.

**INT3** — ensure current `cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline`, bare-metal build, format, Python tests and QEMU/VMware smoke remain intact. Do not claim running test on unavailable environment.

### Batch 2: resource broker with mock hardware

**P2** — DeviceRegistry: device generation, exclusive BAR/IRQ claims, stale-handle rejection, read-only backend, deterministic release.

**G2** — DisplayManager: mock atomic validation and mode generation, `TEST_ONLY` before commit; keep current API until migration.

**S2** — Process-owned service registry *model*, `TaskSnapshot` as only PID source; manifest launch remains Unsupported pending real user services.

### Batch 3: hardware & protected service milestones (separate gates)

**P3** — ACPI MCFG parser, verified MMIO map and interrupt broker; DMA/IOMMU threat model approved before bus mastering.

**R3/I3** — production-ready user process launch, multi-task scheduler, secure IPC and cap tables with fault recovery.

**S3** — first `zillad` true Ring 3 process, dependency/restart/health, then `deviced`, `logd`, `displayd` under explicit security policy.

**A3** — use [AMD_HARDWARE_LAB.md](AMD_HARDWARE_LAB.md) on one actual AMD board; read-only hardware evidence. Move from A0 match to mode setting only after all PCI resource/power/firmware gates.

## 4. Dependencies (must not skip)

```text
 P0 PCI identities ─┬─ A0 AMD matcher
                    ├─ G VMware binding adapter
                    └─ P2 DeviceRegistry ─ P3 MMIO/IRQ/DMA/IOMMU ─ A3 native AMD
 R Process isolation ─ I IPC+capabilities ─ S real zillad + deviced
 G DisplayManager ── surface/mode protocol ── displayd integration
```

Avoid task fragmentation that forces repeated "Enter" between obvious dependent steps: within **one ownership boundary**, an agent may finish code + negative tests + docs + CI in one cohesive PR. But don't ask it to concurrently modify another agent's files or skip hardware evidence.

## 5. Agent handoff prompts (copy directly)

### Platform / PCI engineer

```text
You are the ZillaOS Platform/PCI engineer. Study AGENTS.md,
docs/AGENT_COORDINATION.md, docs/PCI_ACPI_RESOURCE_MODEL.md and
docs/DRIVER_FRAMEWORK.md, then re-read current HEAD.

Implement P0.1 fully with host unit/negative tests: pure no_std
PCI address/identity/config parsing model and deterministic bounded
read-only inventory. Keep existing storage_pci and VMware paths intact.
No config writes, MMIO, DMA, firmware changes or broad refactor.
Do not modify shared main.rs/lib.rs/Cargo.toml without an explicit
minimal integration boundary. Report commit and actual test evidence.
```

### AMD GPU engineer

```text
You are the ZillaOS AMD GPU research engineer. Read AGENTS.md,
docs/AMD_GPU_BRINGUP.md, docs/AMD_HARDWARE_LAB.md,
docs/DRIVER_FRAMEWORK.md and current GPU code at HEAD.

Implement A0.1 only: pure AMD PCI display matcher on shared PCI
identity, untrusted/unknown ASIC -> DetectedUnsupported, no active
hardware init. Write exhaustive host negative tests and document
tested IDs; never claim native GPU support. You own only new
src/drivers/gpu/amd/** and matching standalone tests. Do not modify
VMware SVGA, Desktop, ProcessManager, GPU BAR/firmware, DMA, main.
Report file paths, SHA, test commands and explicit non-goals.
```

### Services engineer

```text
You are the ZillaOS Services engineer. Read AGENTS.md,
docs/SERVICE_FRAMEWORK.md and docs/SYSTEM_SERVICES_IPC.md at HEAD.

Create a pure bounded no_std ServiceManifest/ServiceGraph/restart
state model and tests. Reject cycles, duplicate IDs, bad caps,
missing dependencies and unlimited restart. No user process
execution, no fabricated PID or background daemon claim. Do not
touch kernel process files, shared main/lib/Cargo, or GUI code.
Report commit and host/bare-metal test evidence separately.
```

### Integration/Quality engineer

```text
You are the ZillaOS Integration engineer. Read all active workstreams
and current HEAD. Merge only compatible P/A/S pure models after each
owner finishes and shared API is agreed. Use no force push, no reset
over concurrent commits. Run host tests, bare-metal feature builds,
and QEMU/VMware smoke when available. Do not mark hardware features
verified without actual physical device evidence. Report exact
test logs, newest HEAD and any conflict/blockers.
```

## 6. Incremental Definition of Done

- No new executable paths to untrusted kernel Ring 0.
- Every new parser validates lengths, overflow and unknown versions.
- Every resource claim has clear owner, release and generation invalidation.
- Real counters only; no fake PIDs/CPU/memory/temperature in Settings/Task Manager.
- The default ISO continues to show software desktop in VMware; PCI/AMD work cannot black-screen it.
- A hardware experiment never silently changes real disks, GPU firmware, IRQ/DMA policy or host BIOS.
- Multi-core/SMP marked future until proven with QEMU SMP + hardware fault tests.
- No claims of graceful GOP return after unverified active AMD mode writes.

Docs index: [OS_SUBSYSTEMS.md](OS_SUBSYSTEMS.md) · [DRIVER_FRAMEWORK.md](DRIVER_FRAMEWORK.md) · [SERVICE_FRAMEWORK.md](SERVICE_FRAMEWORK.md) · [SYSTEM_SERVICES_IPC.md](SYSTEM_SERVICES_IPC.md) · [DISPLAY_STACK_PROTOCOL.md](DISPLAY_STACK_PROTOCOL.md).
