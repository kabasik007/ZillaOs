# ZillaOS — Driver Framework: PCI, DeviceManager, MMIO, IRQ, DMA

> **Architecture proposal — NOT an implemented driver manager.** Repository snapshot: `main` 2026-11-07. This document defines a shared substrate for AMD/VMware/Intel/NVIDIA/Storage without stealing ownership from existing agents.

## 1. Current audit

Already available:
- `src/drivers/storage_pci.rs`: `PciLocation`, read-only storage inventory, class/subclass tests.
- `src/video/mod.rs`: read-only PCI video adapter identification.
- `src/drivers/gpu/vmware_svga/pci.rs`: VMware-specific PCI/BAR validation.
- `src/pci_io.rs`: low-level x86 port I/O backed PCI access, called from early kernel.
- `src/video/display_manager.rs`: mockable `ModeBackend`, rollback and mode confirmation.
- `src/drivers/gpu/vmware_svga/mmio.rs`: conservative aperture preflight, but not a general PCI resource allocator/mapper.

Missing platform-level features: global stable DeviceId/DeviceRegistry, atomic BAR ownership/claiming, generic MMIO mapper with memory attributes, shared IRQ abstraction, DMA API/IOMMU, ACPI MCFG-based PCIe ECAM, safe driver bind/unbind/hotplug and user-mode driver IPC. **Do not declare any of these implemented.**

## 2. Separation of concerns

```text
firmware/CPU ── PCI config access (read-only bootstrap)
                        │
                        ▼
                  PCI Enumerator
             class/vendor/device/BDF/capabilities
                        │
                        ▼
                 DeviceRegistry
              ownership / generation / resources
                        │
                        ▼
           DriverManager (match, claim, bind)
                /                     \
       VMware SVGA probe            AMD GPU probe
                \                     /
                    DisplayManager
                           │
                GOP always-safe fallback
```

- **Bus layer**: знайти BDF/IDs/class, discover PCI bridges та ресурси, не вгадувати виробника за класом.
- **DeviceRegistry**: source of truth for device identity/state and claims; change generation on removal/rebind.
- **DriverManager**: match a registered driver to device, controlled claims, probing, initialization, removal.
- **Resource broker**: reserve BAR/IRQ and map only validated addresses; no driver-specific private page allocator.
- **DisplayManager**: owns physical display state; driver supplies mode backend but does not own Desktop/WindowManager.
- **User-mode `deviced` (later)**: manages policy and broadcasts events; the kernel remains hardware authority.

## 3. Generic PCI contract v0 — read-only first

Suggest a *pure* descriptor model:

```rust
// API SKETCH, not code present in ZillaOS. Prefer no_std, bounded arrays.
struct PciAddress { segment: u16, bus: u8, device: u8, function: u8 }
struct PciIdentity {
    address: PciAddress,
    vendor_id: u16,
    device_id: u16,
    revision: u8,
    class: u8,
    subclass: u8,
    prog_if: u8,
    subsystem_vendor: Option<u16>,
    subsystem_device: Option<u16>,
}
enum ProbeState { NotPresent, Unsupported, MatchReadOnly, Error }
enum DriverState { Discovered, Matched, Claimed, Probed, Active, Faulted, Removed }
```

Read-only discovery inputs are injected as `FnMut(PciAddress, offset) -> u32` (or equivalent trait) so host tests use fake config space. PciAddress must not be truncated to a 16-bit BDF when segment groups become supported. In early CF8/CFC bootstrap, **only segment 0 is accessible**; do not claim all PCIe segments are scanned.

v0 behavior:
1. Read vendor/device safely; `0xffff` and invalid function -> absent.
2. Read `class/subclass/prog_if/revision`, and multifunction flag from header type before scanning functions 1..7.
3. Enumerate bridges where supported rather than assuming all buses/devices exposed.
4. Return bounded `DeviceSnapshot`; overflow -> explicit `truncated` flag, no fabricated devices.
5. Do not write PCI configuration, enable bus mastering, size BAR by all-ones writes, or map MMIO.
6. Duplicate BDF IDs or conflicting claims -> reject.

**Important:** actual BAR aperture *length* cannot be determined from the BAR value alone. A trusted firmware/PCI resource allocator or safely scheduled PCI sizing transaction is required *later* before mapping, and never during early passive probe.

## 4. Driver descriptor / claim lifecycle (proposed)

```text
Discovered → Matched → ClaimRequested → Claimed → ProbedReadOnly
                    └→ Denied                    ├→ SupportedInactive
                                                 └→ Initialized → Active
                                                               └→ Failed/Removed
```

Driver record: `name`, `driver_version`, `match_table`, `required_resources`, `priority`, `capability_set`, `safe_probe`, `active_initialize`, `suspend`, `resume`, `shutdown`. Multiple candidate drivers may match, but **only one active owner** of mutually-exclusive BAR/IRQ/resource set. Driver priority must not bypass security policy.

Separately track a physical `DeviceId` with generation: stale handles, removed devices, remapped BARs and post-reset interrupt handlers must be invalidated. Do not expose raw physical BAR addresses through an untrusted public API.

Recommended Rust split (to create incrementally, avoiding mass refactor):

```text
src/bus/pci/{mod.rs,config.rs,enumerate.rs,identity.rs}  # proposal
src/kernel/device/{registry.rs,claim.rs,resources.rs}    # proposal
src/kernel/hal/{mmio.rs,interrupt.rs}                   # proposal
src/kernel/dma/{mod.rs,iommu.rs}                         # proposal
src/drivers/gpu/amd/{mod.rs,pci.rs,device_ids.rs}        # proposal
```

Start with `identity.rs`, `enumerate.rs`, `registry.rs` as **host-testable pure models**. Do not move existing `storage_pci.rs`/VMware module until adapters can preserve all tests/boot behavior.

## 5. MMIO / BAR / resource invariants

- BAR descriptor `base` != verified `resource_length`; reject zero, overflow, overlap, unaligned or unsupported 64-bit BAR handling.
- Central `MMIOMapper` must validate physical extent against authoritative PCI allocation, memory map restrictions, and page permissions.
- Device register mapping needs device-appropriate **uncached** memory attributes; framebuffers may require **write combining** where correctly supported. Never silently use normal cacheable RAM mapping for device registers.
- Volatile reads/writes with explicit ordering. Avoid unsynchronized accesses from separate CPUs; SMP requires barriers and locks.
- Early GOP framebuffer is a separate firmware-acquired display resource; no AMD ownership transfer without an explicit handoff protocol and rollback.
- Hot remove/reset requires poisoning mapped handles, disabling IRQ, stopping DMA, freeing mappings, then releasing claims.

## 6. DMA / IOMMU is a hard gate for active PCI GPU

CPU virtual address → CPU physical address → **DMA/bus address** are different concepts. A generic DMA API must own mapping/unmapping, addressability masks, coherent vs streaming buffers, cache synchronization, scatter-gather, fences and lifetime. Bus mastering is disabled by default until the driver is trusted, memory ranges validated, and the DMA threat model/IOMMU policy settled.

Design targets:
- `allocate_dma_buffer(owner, len, alignment, direction)`
- `map_dma(owner, pinned_pages) -> DmaHandle`
- `sync_for_device(handle)`, `sync_for_cpu(handle)`, `unmap_dma(handle)`
- `bind_iommu_domain(device, process/capability)`
- `revoke_dma(device)` on driver fault/unbind

All API shapes are **proposals**; not yet valid calls. If hardware lacks/does not expose a usable IOMMU, either keep the driver in a trusted explicitly privileged mode with a documented risk or **disable untrusted DMA**. Do not fake isolation based on Rust memory safety.

## 7. IRQ, timer and service interaction

- One vector allocator + IRQ registration/token ownership; eventually MSI/MSI-X after validated PCI capability parsing.
- IRQ handler reads only known registers, acknowledges within device-specific spec, queues bounded work; no arbitrary allocations/long blocking.
- Timeouts require a verified monotonic timer. Mock budgets are not physical timing guarantees.
- Device failure reports a kernel-owned event for TaskManager and later `deviced`/supervisor.
- `deviced` uses handles, not global MMIO; userspace app requests surface/resource through IPC/caps.

## 8. AMD integration without breaking VMware

- Match AMD GPU **PCI vendor 0x1002** with display class and a deliberate supported device-ID list. AMD CPUs/platform/chipsets may use vendor `0x1022`; `0x1022` alone does **not** identify an AMD Radeon GPU.
- Vendor match alone -> `DetectedUnsupported` until device/revision support is explicitly declared.
- `AMD` and `VMware` both consume the same immutable PCI inventory; neither owns the global scanner.
- The display selection policy is explicit: valid active backend -> verified modes; otherwise GOP software fallback. Do not modify framebuffer, Desktop or app process logic inside the AMD probe.
- First AMD milestone is **diagnostics only**. Do not load GPU firmware, touch ASIC registers, reset/clock/power blocks, or issue commands during that phase.
- Separate display generation (DCE/DCN), memory controller (GMC), IRQ handler (IH), PSP, SMU, SDMA and GFX; these have independent IP revisions and dependencies.

## 9. Review, CI, and fault injection matrix

| Test | Expected |
| --- | --- |
| Empty/invalid PCI vendor ID | no device, no I/O writes |
| Single-function vs multifunction, max bus/device | correct bounded discovery |
| Same physical BDF claimed by two drivers | second claim denied |
| AMD `0x1002` non-display PCI function | not treated as Radeon display GPU |
| Unknown AMD Device ID | detected, never initialized |
| Bad 64-bit BAR or unknown extent | no mapping, no register access |
| Resource overlap/overflow | fail closed |
| DMA request without gate/IOMMU decision | denied |
| Driver fault/restart | stale handle invalid, resource claim released |
| SVGA unavailable / AMD not supported | GOP desktop still boots |
| Host unit tests / bare-metal compile / actual GPU | three separately reported evidence levels |

Commands, when toolchain exists:

```bash
cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features vm-demo
cargo +1.90.0 fmt --all -- --check
python3 -m unittest discover -s tests -v
```

No manual VMware test is a physical AMD proof. Start real-hardware testing only on disposable boot media with external recovery, known PCI ID and serial logs; never on a valuable system disk.

## 10. Reference specifications

- Linux kernel, AMDGPU IP blocks/driver core: https://docs.kernel.org/gpu/amdgpu/driver-core.html
- Linux kernel, PCI driver model: https://docs.kernel.org/PCI/pci.html
- Linux kernel, dynamic DMA mapping: https://docs.kernel.org/core-api/dma-api-howto.html
- Linux DRM KMS/gpu memory abstractions (design inspiration, **not** kernel dependency): https://docs.kernel.org/gpu/
- AMD GPUOpen low-level docs: https://gpuopen.com/amd-gpu-architecture-programming-documentation/

**Licensing:** these sources inform architecture; do not copy Linux C driver code or firmware blobs into ZillaOS without a license/compatibility review. ZillaOS does not link against Linux kernel runtime.

Related: [OS subsystems](OS_SUBSYSTEMS.md), [Service Framework](SERVICE_FRAMEWORK.md), [AMD bring-up](AMD_GPU_BRINGUP.md), [VMware SVGA](VMWARE_SVGA_DRIVER.md).
