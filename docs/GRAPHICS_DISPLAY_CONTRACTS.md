# ZillaOS — Graphics G0/G1/G2/G3 integration contract

**Snapshot:** 2026-11-07. **Status:** source models and host tests; **not** a working
native VMware or AMD modeset implementation. This document supplements
[VMWARE_SVGA_DRIVER.md](VMWARE_SVGA_DRIVER.md) and the architecture-only
[DISPLAY_STACK_PROTOCOL.md](https://github.com/kabasik007/ZillaOs/pull/12).
Do not merge PR #12's docs by copying them into the graphics code branch.

## Ownership and current call graph

```text
Desktop / WindowManager / existing CPU compositor (UNCHANGED)
          | potential future damage producer
          v
src/video/display_contracts.rs: BufferLayout / DamageRect / SurfaceRegistry
          | only checked values; NO address or mapped memory
          v
src/video/display_manager.rs: confirmed / pending / poisoned
          | ModeBackend TEST_ONLY -> prepare -> set -> readback
          +--> future VMware SVGA backend (not wired)
          +--> future AMD DCN/DCE backend (not implemented)
          v
src/drivers/gpu/vmware_svga/update_bridge.rs: optional pure
          SVGA_CMD_UPDATE encoding; NEVER enqueued by this feature
```

The active ZillaOS desktop remains on its existing Limine/UEFI GOP software
framebuffer. No `src/graphics.rs`, `src/shell/desktop.rs`, ProcessManager,
PCI DeviceManager or boot feature flags are changed here.

## G0 — no_std display data plane

- `OutputId {slot:u16, generation:u16}`, `SurfaceId {slot:u32, generation:u32}`
  are generation-tagged *logical* handles. Both registries invalidate an ID
  when an output disconnects or a surface is destroyed. On generation wrap,
  a slot must be retired rather than reused (no ABA resurrection).
- `PixelFormat` currently specifies little-endian `Xrgb8888`,
  `Argb8888` and `Rgb565` data layouts. Enumeration of a type is **not**
  evidence that VMware, AMD, GOP, or a monitor accepts it.
- `BufferLayout::validate()` checks nonzero dimensions, max 8192 each,
  bytes/pixel, min pitch and pitch alignment, required pitched byte length
  (`pitch_bytes * height`), supplied backing length and a 256 MiB
  **per-surface model limit**. This is NOT an allocation or total quota.
  The kernel memory broker must separately validate all backing pages,
  DMA/IOMMU access, physical aperture extents and memory/cache attributes.
- `DamageRect` uses checked addition and bounds. No zero-area or
  out-of-bounds rectangles. This is a rectangle contract, not a CPU or
  GPU damage scheduling implementation.
- `SurfaceOwner::{Kernel,Process(pid)}` is metadata only. PID 0 is rejected
  for process ownership; a real IPC endpoint still needs kernel-authenticated
  capabilities and revocation. Untrusted callers must not directly control
  these registries without authorization.
- `SurfaceRegistry` models exclusive scanout, forbidding CPU damage and
  destruction until the display owner retires the scanout fence.
  It does **not** map, protect, allocate, free or blit any real buffer.
- `OutputRegistry` models disconnect/reconnect invalidation, not EDID or
  physical hotplug delivery.

### Driver-neutral surface/output API for AMD and VMware

`VideoDisplayBackend: ModeBackend` adds `active_output()`,
`enumerate_modes(output, out)` and `test_scanout(output, surface)`.
The supplied output ID is generation-tagged; a backend must reject stale
outputs, invalid/mismatched scanout format, unowned memory and insufficient
apertures **without writes**.

The `enumerate_modes` destination is caller-bounded. A backend must return
at most `out.len()` and fill only validated entries. Unknown refresh is
`None`, not a guessed 60 Hz. Adapter support for multi-output is a **future
contract**; the current `DisplayManager` still tracks one `Mode`.
Do not create a second DisplayManager per GPU vendor.

## G1 — atomic modeset model

Existing `Mode`, `ModeBackend` required methods, `Apply`,
`DisplayManager::apply/confirm/cancel` and constructor remain source-
compatible. A new *defaulted* `ModeBackend::test_mode` makes old
implementations compile, but deliberately returns `Unavailable`
unless a backend overrides it with read-only atomic validation.
This is a safety tightening: legacy implementations cannot perform
native modesets merely because `supports_mode` was true.

Mode flow:

1. Reject disabled backend, invalid/unsupported candidate, poisoned state or
   another pending transaction.
2. Read current hardware mode and compare to manager snapshot.
   Stale/missing output poisons before hardware writes.
3. Call `test_mode(requested)` with no writes and no visible state change.
4. Call existing `prepare_mode` to reserve all resources before commits.
5. Call `set_mode`, then verify `read_mode`. Only *then* mark pending.
6. `confirm` checks a fresh device readback before updating confirmed mode.
7. `cancel` restores the previous mode only when the backend proves it safe,
   and verifies the restored hardware mode.
8. On ambiguous failure, do not report success. Failed rollback poisons
   the manager; a lost output never causes a blind restore attempt.

**Crucial:** readback is a **driver-provided claim** until actual device
hardware is tested. Also, if a real native modeset/reset has occurred,
firmware GOP framebuffer scanout **cannot** be assumed recoverable.
Use tested device-specific recovery and independent serial/headless
diagnostics. No global automatic GOP rollback is promised.

## G2 — VMware separation and hardware gating

Already present:

- `vmware_svga/pci.rs`: read-only PCI matching and BAR descriptors.
- `vmware_svga/capabilities.rs`: register snapshot and dimension limits.
- `vmware_svga/mmio.rs`: pure checks against *externally supplied* BAR
  extents; device register sizes do NOT prove PCI aperture lengths.
- `vmware_svga/queue.rs`: host-test FIFO transaction model; no live
  `FifoBackend` or mapped BAR2 implementation.
- `vmware_svga/fifo.rs`: checked five-word UPDATE packet builder.

New `vmware_svga/update_bridge.rs` translates validated XRGB8888 damage
for a matching verified `Mode` to a five-DWORD UPDATE payload. It cannot
submit that payload: no FIFO mapping, framebuffer acquisition, sync,
publication or hardware writes are added.

**Before live VMware modeset:** obtain authoritative PCI BAR1/BAR2
aperture sizes and unique claims from the Platform/PCI owner; validate a
UC memory mapping with device fences, sole queue writer, negotiated SVGA
version/capabilities, measured resource bounds, readback and device-specific
restore. Add an explicit opt-in experimental ISO only after all gates and
VM snapshots. Normal `BuildRun` must remain unchanged. Do not confuse
mode enumeration from MAX_WIDTH/MAX_HEIGHT with supported physical modes.

## G3 — AMD agent contract

AMD A0 remains a separate **read-only PCI matcher** owned by the AMD agent.
The AMD agent should:

1. Read `src/video/display_contracts.rs` and
   `src/video/display_manager.rs` before any display integration.
2. Reuse `OutputId`, `SurfaceId`, `BufferLayout`, `DamageRect`,
   `VideoDisplayBackend`, and the existing `DisplayManager`; do not
   invent a parallel KMS manager or PCI scanner.
3. For A0/A1 only: identify vendor `0x1002` class `0x03` devices,
   report unknown IDs unsupported; **do not** implement
   `test_mode/set_mode` or pretend AMD scanout works.
4. Later A3/A4: acquire validated resource claims from the shared PCI
   broker; implement device-specific, no-write `test_mode` and
   `test_scanout`; only opt-in after documented rollback, firmware,
   connector, display-engine and page ownership requirements are met.
5. A real display path must report lost output, invalid buffers,
   mismatched readback, and recovery failures distinctly. A host mock
   cannot establish AMD hardware readiness.

## Validation and missing evidence

Implemented tests in the Rust source cover format size/stride, damage
overflow, stale output/surface IDs, scanout lease, owner mismatch,
TEST_ONLY rejection, legacy backend fail-closed, output disappearance
before/after commit, failed prepare, failed set, readback mismatch and
rollback poisoning. **Tests authored do not imply tests executed.**

Suggested validation, on an environment with Rust 1.90:

```bash
cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features vm-demo
cargo +1.90.0 fmt --all -- --check
```

For UI regression, use the existing QEMU desktop smoke workflow, then
capture COM1, desktop screenshots, keyboard/mouse and device ID probe
in VMware Workstation. Native VMware resolution switching and AMD
hardware operation remain **not verified / not implemented**. Do not
cite QEMU software framebuffer output as evidence for physical modeset.

### Maintainer merge gates

- No changes to `ProcessManager`, `src/graphics.rs`, Desktop, `src/main.rs`,
  `Cargo.toml`, `src/lib.rs`, boot ISO scripts, MMIO or PCI resource ownership.
- Review `AGENTS.md` and PR #12 for any newly agreed types; prefer this
  typed interface as one shared source of truth after coordination.
- Merge only after host build/tests pass and no shared file conflict;
  VMware/AMD hardware promises remain separate post-merge milestones.
