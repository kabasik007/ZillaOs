# ZillaOS — Display Stack: GOP → DisplayManager → KMS-like atomic contract

**Status:** architectural specification (2026-11-07), no active AMD/VMware native hardware implementation implied.

## 1. Audit of existing code

- `src/graphics.rs`: current kernel-embedded RGB32 CPU software compositor/backbuffer. At 2026-11-06, updated toward 1920×1080 fixed backing support; **not** hardware flip or userspace compositor.
- `src/video/display_manager.rs`: `Mode{width,height}`, `ModeBackend` (`runtime_switch_available, supports_mode, prepare_mode, set_mode, read_mode, can_restore`), manager `apply/confirm/cancel`, confirmed/pending/poisoned state. Mock unit tests cover rollback.
- `src/video/mod.rs`: `VideoAdapter`, read-only VMware probe, `hardware_acceleration=false`, `runtime_mode_setting=false`.
- `src/shell/window_manager.rs`: windows/focus/geometry; should **never** hold PCI registers, output connectors or DP link-training state.
- `src/drivers/gpu/vmware_svga/**`: read-only probe + pure FIFO models, not live output.
- `src/drivers/gpu/amd/**`: **not present** as of initial research snapshot.

Existing `DisplayManager` is useful; **extend it**, do not create another global modeset manager.

## 2. Separate five independent concepts

1. **Firmware framebuffer**: Limine/UEFI supplies a ready-to-draw GOP framebuffer and pixel layout.
2. **Surface/composition**: window buffers and software compositing — independent of which GPU displays output.
3. **Scanout/display controller**: chooses a framebuffer/plane, timings and connector.
4. **GPU rendering**: 2D/3D/compute command queues, shaders and acceleration.
5. **Display policy/UI**: Settings requests modes, orientation, scaling, multi-monitor; not hardware authority.

A screen being visible via GOP does **not** indicate an AMD driver loaded, a working GPU queue or an accelerated compositor.

## 3. Target display pipeline

```text
Native app (Ring 3 future)
      │ surface handle, damage + fence
      ▼
WindowManager  ─────────→ compositor/displayd (later)
      │ windows/focus            │ output composition
      │                          ▼
      │                   DisplayManager
      │                    atomic test/commit
      │                       /      \
      │            GOP bootstrap   GPU display backends
      │                              /           \
      └──────────────────       VMware SVGA       AMD DCN/DCE
                                             └→ GPU rendering (separate)
```

`WindowManager` owns geometry/z-order; `displayd` schedules frame composition; `DisplayManager` owns physical output state. Drivers own hardware-specific registers and validated resource tokens.

## 4. Learn from DRM/KMS without depending on Linux

Linux KMS uses Framebuffer → Plane → CRTC → Encoder/Bridge → Connector as a display pipeline; atomic state has a **validate without hardware writes** phase before commit. This is a design reference, not a demand to clone Linux DRM ABI.

Suggested ZillaOS v1 types:

```rust
// Sketch, not ZillaOS code yet.
struct OutputId { slot: u16, generation: u16 }
struct SurfaceId { slot: u32, generation: u32 }
struct Mode { width: u32, height: u32, refresh_millihz: u32 }
enum PixelFormat { Xrgb8888, Argb8888 }
struct Rect { x: i32, y: i32, width: u32, height: u32 }
struct BufferLayout { width: u32, height: u32, pitch_bytes: u32, format: PixelFormat }
struct DisplayRequest { output: OutputId, mode: Mode, primary: SurfaceId }
enum CommitResult { Applied, Unsupported, Busy, LostOutput, DeviceFault }
```

**Compatibility warning:** current `Mode` only width/height; do not replace it until `ModeBackend` users and tests are migrated. `refresh_millihz` must be verified from real connector modes, not assumed from width and height.

## 5. Atomic display transactions — model first

Atomic pipeline concept:

```text
snapshot current hardware/software state
     ↓
build prospective Output/Mode/Surface routing
     ↓
validate geometry, pixel format, connector link, pitch/stride, memory, bandwidth
     ↓
TEST_ONLY (NO HARDWARE WRITES)
     ↓
reserve resources + fences
     ↓
COMMIT hardware state
     ↓
readback/verify and signal completion
     ↓
mark DisplayManager state confirmed OR report fault
```

Requirements:
- Reject user-controlled dimensions that overflow `pitch * height`, resource length, or supported formats. Use checked arithmetic.
- Include `OutputId` generation in requests; stale output must return `LostOutput`.
- Framebuffers/surfaces are read-only to scanout while in use; lifetime and fences must be respected.
- Do not change Settings-visible confirmed mode until hardware confirms; return errors without silently inventing a working resolution.
- Bounded commit timeout based on monotonic timer; no unlimited busy loop in kernel.
- On failed validation, no hardware write; on failed actual commit, attempt a **documented supported rollback**, mark state poisoned if unrecoverable, provide serial/headless diagnostics.
- **Critical correction:** a firmware GOP framebuffer is a guaranteed fallback only **while the device has not been reprogrammed**. After native GPU reset or modeset, Limine's old GOP framebuffer could be invalid or no longer scan out. ZillaOS must **not** promise automatic recovery to GOP after partial AMD hardware changes. Keep a rescue console/serial and a verified per-device recovery path.
- Do not present `Apply::Success` as proof of hardware success when backed only by a mock.

Reference: https://docs.kernel.org/gpu/drm-kms.html

## 6. Surfaces and rendering contract

Before actual user-space compositor, pure host-testable code can provide:
- `SurfaceDescriptor {owner_pid_or_kernel, generation, width, height, pitch, pixel_format, byte_length}`.
- `create_surface` checks cap + quota + alignment + total byte length.
- `map_for_cpu` / `submit_damage` need checked rectangles; avoid raw pointers across IPC.
- `acquire_scanout` requires owner permission and prevents premature deallocation; fence retirement signals when safe.
- Do not report GPU memory usage from software backing memory; track CPU and GPU heaps separately.
- Wait/fence should use kernel monotonic clock when available. In v0 host mocks, use injected deterministic fake time.
- Fallback compositor draws into kernel-approved software buffer; no GPU pipeline is inferred.

Suggested future protocol:

```text
DISPLAY/1 HELLO(version, capability_handle)
DISPLAY/1 LIST_OUTPUTS -> bounded snapshot
DISPLAY/1 GET_MODES(output) -> bounded modes
DISPLAY/1 TEST_CONFIGURATION(config) -> validated | error
DISPLAY/1 COMMIT_CONFIGURATION(config) -> request_id
DISPLAY/1 WAIT(request_id, deadline) -> confirmed | failed
SURFACE/1 CREATE(format, extent, owner) -> handle
SURFACE/1 DAMAGE(handle, bounded_rects)
SURFACE/1 PRESENT(handle, fence) -> completion
```

These names are *conceptual*, not syscalls or ABI implemented now. Kernel must authenticate sender PID/rights, verify every handle length, prevent confused-deputy delegation and stale generations.

## 7. AMD DCN/DCE hardware bring-up dependencies

AMDGPU's display controller is one IP block among GMC, IH, PSP, SMU, SDMA and GFX. Display Core (DC) often depends on firmware and board topology; even a display-only driver may need power and firmware cooperation. Treat board's display controller revision as the unit of compatibility, not marketing RX name alone.

Proposed read-only object discovery:
- PCI GPU match → classify ASIC family/IP versions from verified, versioned hardware data.
- Connector topology, EDID parsing (strict length/checksum), port/encoder paths (when supported).
- Select preferred mode from validated monitor data with safe minimum fallback; do not pick refresh/clock blindly.
- Never call DCE/DCN register programming before BAR, mapping, IRQ, firmware and ownership prerequisites.
- No unsupported link-training/SMU power calls as experiments on a production system.

Reference: https://docs.kernel.org/gpu/amdgpu/display/index.html

## 8. GPU acceleration is a separate interface

A future `GpuCommandBackend` and GPU memory manager are distinct from `ModeBackend`. Command submission includes checked command buffers, GPU VM isolation, firmware, ring ownership, fences/interrupts, timeouts and reset recovery. A VMware FIFO update is not evidence that AMD graphics shader acceleration works.

Do not expose arbitrary DMA/VRAM/MMIO to desktop processes. Before untrusted client commands, implement kernel-enforced device address spaces/IOMMU/resource quotas and queue validation.

## 9. Testing matrix

| Level | Test | What it proves |
| --- | --- | --- |
| Pure | format/pitch/area checked arithmetic, rect clipping | memory bounds for descriptors |
| Mock | two output modes, transaction validate/commit, failure | DisplayManager state machine |
| Mock | lost output during pending commit | stale-handle rejection |
| Mock | unsupported refresh and EDID checksum failure | correct reject |
| QEMU | GOP bootstrap, CPU compositor, cursor regression | no early desktop regression |
| VMware | SVGA opt-in experiment, confirmed mode + serial | only explicitly verified VMware support |
| Native AMD | correct connector scanout/readback/external display | one exact ASIC/board/display path |
| Native AMD advanced | fences, GPU memory, queue isolation | only tested accelerated paths |

## 10. Implementation batches / code ownership

**D0:** static `DisplayRequest`/surface layouts and mock atomic transaction tests, no PCI writes. Preserve existing `ModeBackend` API.

**D1:** `OutputId`, discovery/status & EDID parser with validation. Keep GOP path.

**D2:** add actual VMware mode backend only when SVGA owner confirms verified MMIO/FIFO and rollback.

**D3:** with AMD hardware identified and DMA/resource gates passed, per-ASIC DCN modesetting; no cross-family broad support claim.

**D4:** user-space `displayd` after Ring 3 + IPC + caps; TaskManager sees true display service PID.

Coordinate with Graphics agent for `src/video/**`, desktop owner for `src/shell/**`, Process agent for user service IPC; never make these shared changes inside AMD A0.

Related: [DRIVER_FRAMEWORK.md](DRIVER_FRAMEWORK.md), [PCI_ACPI_RESOURCE_MODEL.md](PCI_ACPI_RESOURCE_MODEL.md), [AMD_GPU_BRINGUP.md](AMD_GPU_BRINGUP.md).
