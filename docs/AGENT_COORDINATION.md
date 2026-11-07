# ZillaOS — Six-Agent Coordination, I-M2

**Status 2026-11-07:** integration PR #19 merged, Rustfmt cleanup PR #26 merged and tested, 10 old I-M1 branches deleted after ancestry verification. Canonical live code = current `main`; six agents begin only with fresh feature branches.

**Before any changes:** [AGENTS.md](../AGENTS.md) · [I-M2 owner matrix](NEXT_WAVE_I_M2.md) · [new-chat handoff](NEW_CHAT_START_I_M2.md) · [security](SECURITY_ARCHITECTURE.md).

## Ownership and API handshake

| Stream | Single writer | Consumer/contract |
|---|---|---|
| A — Platform/PCI | `src/platform/**` and `src/pci_io.rs` excluding integration-owned AMD bridge | Immutable canonical DeviceId + registry for AMD, VMware, Storage; no secondary PCI discovery |
| B — Process/Scheduler | `src/kernel/process/**`, `src/services/process_manager.rs` | Trusted PID/TID, exit and fault lifecycle, cleanup sequencing |
| C — IPC/Capabilities | `src/kernel/ipc/**`, `src/kernel/capabilities/**` | Consume **B-trusted** current identity, enforce rights and deterministic revocation |
| D — Services | `src/services/service_manager.rs`, `src/services/service_supervisor.rs`, `src/services/service_manifests.rs` | Consume B/C typed events; never fabricate successful service launches |
| E — Graphics/VMware | `src/video/**`, `src/drivers/gpu/vmware_svga/**` | DeviceId from A, output and surface capability owner, software fallback |
| F — AMD | `src/drivers/gpu/amd/**` | Read-only canonical A registry diagnostic, no active hardware |

**Chief Architect** owns `src/main.rs`, `src/lib.rs`, `src/services/mod.rs`, `src/platform/amd_bridge.rs`, `Cargo.toml`, `src/shell/**`, shared CI/workflows, `ApplicationManager` gateway and cross-module exports. If a feature needs a change outside its ownership, raise a narrow typed interface proposal in its PR before touching the shared file.

## Unique canonical owners

- `WindowManager`: focus, geometry, z-order; no second GUI window registry.
- `ApplicationManager`: routes Start/Dock/Files/App Center, not a second process table.
- `ProcessManager`: PID/TID/process lifecycle. A User GUI built-in view is not automatically a Ring3 task.
- `DeviceRegistry`: generation-safe DeviceId and ownership, not raw BAR/physical pointers.
- `DisplayManager`: bounded mode-test/prepare/commit/rollback contracts. Native SVGA/AMD hardware paths remain disabled until explicit lab evidence.
- `IPC/Capabilities`: every access mediated by rights, owner and generation; request metadata does not grant privileges.
- `ServiceManager`: ServiceId != PID; launches and cleanup are authenticated model ports until runtime verified.

## Dependency order

1. **B + C** agree authoritative current-task identity, handle ownership, exit ticket and cleanup ACK schema. They may code in parallel; Chief Architect merges interface changes.
2. **D** consumes B/C typed lifecycle and teardown contract. Its request to start a service is **not proof** that a process ran.
3. **A + E + F** harden snapshot and read-only visual device diagnostics, generation-safe surface/output claims.
4. Chief Architect integrates all PRs, runs full CI, actual disposable QEMU Ring3/fault/PCI desktop regression, and records per-agent SHA. No automatic main merge.

## PR protocol

1. Fetch live `main` and record its SHA. Create new `agent/<domain>-im2-<topic>` branch.
2. Touch owner files only; commit with no force and with expected-head/blob recheck.
3. Test host `no_std` model, Rustfmt, bare-metal, Python and relevant QEMU; negative malformed/fault/security cases mandatory.
4. Draft PR includes exact code SHA, checks, NOT RUN claims, API dependencies and rollback/fallback notes.
5. Integration owner resolves any cross-stream overlap and performs staging verification. Only then owner-authorized merge to `main`.
6. After merge, confirm source SHA ancestor of `main` before pruning old branches. No deletion of unmerged work.

## Proven evidence and remaining limitations

PR #26 integration run [42336227902](https://github.com/kabasik007/ZillaOs/actions/runs/42336227902) passed 385 Rust library + 20 Services + 14 Supervisor + 34 canonical PID + 127 Python tests (580 total), full Rustfmt, four bare-metal builds and QEMU Ring3/guard/supervisor checks. Separate Platform/QEMU, Process/QEMU and dedicated workflows all passed on its exact head. This is a **development baseline**, not a physical GPU or security certification.

Open integration priorities: trusted runtime PID→IPC capability cleanup (#21), ServiceManager/Device/Display authorization model (#22), [I-M2 mission control #25](https://github.com/kabasik007/ZillaOs/issues/25).

For Windows user testing: update `main`, then `scripts/windows.ps1 -Action BuildRun`. Keep VMware/real AMD NOT RUN until actually tested.
