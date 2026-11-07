# ZillaOS — AMD Radeon GPU Driver: bring-up roadmap (Rust `no_std`)

> **Status 2026-11-07: DESIGN ONLY.** ZillaOS does **not** contain an AMD GPU driver. This is an implementation plan for read-only detection first, not a claim of framebuffer switching, acceleration, or device initialization.

Repository: `kabasik007/ZillaOs`. Existing graphics: `src/video/mod.rs`, `src/video/display_manager.rs`, `src/drivers/gpu/vmware_svga/**`, `src/graphics.rs`, `src/pci_io.rs`. Review [DRIVER_FRAMEWORK.md](DRIVER_FRAMEWORK.md) first.

## 1. Narrow first target: AMD GPU, not "all AMD hardware"

"AMD driver" is not one device:
- **AMD Radeon GPU/APU graphics:** PCI display controller, display engines DCE/DCN, GMC/VRAM, PSP, SMU, graphics/compute GFX, SDMA; these IP blocks vary by generation.
- **AMD CPU/platform:** x86-64 feature detection, APIC, topology, ACPI, AMD-Vi/IOMMU, timers, power controls. This belongs to CPU/Platform workstream, not `gpu/amd`.
- **AMD chipset USB/SATA/NVMe/audio:** separate standardized bus-class drivers, not the GPU driver.

This plan addresses Radeon/AMD GPU discovery first. Do not advertise RDNA 2/3/4 or any particular Radeon board as supported without actual matching PCI ID/IP revisions and hardware evidence.

## 2. Reality check: VMware

The current VMware Workstation guest presents virtual **VMware SVGA II** rather than automatically exposing the host AMD Radeon. Therefore:
- `BuildRun` inside a normal VMware guest can test GOP/VMware regression and **mock AMD PCI config data**.
- It **cannot** prove AMD MMIO, DCN modesetting, VRAM, firmware, display output or GPU acceleration.
- Active AMD GPU testing needs a safe dedicated physical test machine or an environment with **verified GPU passthrough** and compatible firmware/IOMMU setup. Do not assume VMware Workstation gives working PCIe GPU passthrough.
- Use safe boot media, serial capture, external recovery and no writes to the user's system disk.

## 3. Minimum PCI matching for stage A0

- AMD Radeon graphics typically exposes PCI vendor ID `0x1002`. Do **not** equate AMD chipset `0x1022` with a Radeon graphics function.
- PCI `class=0x03` (display controller) with relevant subclass is necessary for this initial matcher, but **insufficient** for declaring driver support.
- Distinguish `DetectedKnownId`, `DetectedUnsupportedId`, `Absent`, `Malformed`; no blind "supported" for every `0x1002`.
- Report full BDF and `vendor:device`, `subsystem vendor:device`, revision, class/subclass, BAR descriptor types (safe reads only), with bounded logging.
- Handle multifunction/bridges, missing config space, id `0xffff`, and future nonzero PCI segments gracefully.

```text
PCI core: read-only snapshot
    │
    ├── VMware [15ad:0405] -> existing SVGA probe
    └── AMD [1002:xxxx, class 03] -> AMDDeviceInfo (diagnostics ONLY)
                                     |
                               Known/unknown ID
                                     |
                              no writes/no DMA/no MMIO
                                     |
                            GOP desktop unaffected
```

Proposed locations (not yet created):

```text
src/drivers/gpu/amd/
    mod.rs             # pure public probe result types
    pci.rs             # matcher on shared PciIdentity
    device_ids.rs      # explicit supported/tested list, not blanket vendor match
    ip_blocks.rs       # later, per-ASIC block versions
    firmware.rs        # later, read-only validation and gated load
    display/           # later, DCE/DCN only with known registers and handoff
    memory/            # later, VRAM/GMC, only with DMA/resource broker
    gfx/               # later, command processor/scheduling
    interrupts.rs      # later, IRQ handler
```

**A0 should add only `mod.rs`, `pci.rs`, `device_ids.rs` and tests**, ideally using the future generic PCI inventory. If generic PCI core is not yet integrated, implement a pure matcher accepting the existing `PciLocation`-based snapshot, with adapter later; never add a third hardware scanner.

## 4. AMD milestones and acceptance gates

| Phase | Work | Evidence | Explicit non-goals |
| --- | --- | --- | --- |
| **A0: pure discovery** | AMD vendor/class matcher, stable descriptive result, fake PCI fixtures | host unit tests; no I/O writes | no BAR mapping |
| **A1: integration** | common device registry, driver binding status, serial marker `ZILLAOS_AMD_GPU=DETECTED_UNSUPPORTED` on detected AMD | bare-metal build + VM mock path + GOP regression | no claim of physical AMD |
| **A2: hardware inventory** | get exact board/device/revision/ASIC, read-only PCI and safe resource inventory on dedicated hardware | serial snapshot from *actual AMD* host | no MMIO writes |
| **A3: resource groundwork** | authoritative BAR lengths, validated UC MMIO mapping, PCI ownership, IRQ/DMA/IOMMU contracts | negative tests and gated register *reads* only | no GPU reset, no display mode changes |
| **A4: display** | device-specific firmware/ASIC analysis, controlled DCE/DCN bring-up, EDID, atomic mode set with rollback | external capture confirms mode, failure triggers hardware-specific recovery or serial/headless diagnostics; GOP restoration is not guaranteed after GPU reprogramming | no 3D |
| **A5: memory/queues** | verified firmware handling, VRAM/GMC mapping, fences, IRQ, DMA, rings, bounded reset | real GPU test; fault injection | no untrusted shader execution |
| **A6: acceleration** | explicit GFX family command API, 2D then graphics/compute userspace, synchronization | measured drawing + test scenes + isolation | no universal Radeon support claim |

Each phase has its **own evidence**. Host mock A0 passing does not imply A4 or A6.

## 5. Device-specific details to gather before any active AMD work

```text
GPU model / board (e.g. exact RX model):  UNKNOWN
PCI vendor_id:                         0x1002 if AMD Radeon, VERIFY
PCI device_id:                         UNKNOWN
PCI subsystem_vendor / subsystem_id:   UNKNOWN
PCI revision:                          UNKNOWN
Graphics generation / ASIC codename:   UNKNOWN
DCE/DCN + GMC + PSP/SMU + GFX versions: UNKNOWN
Is integrated APU or discrete GPU:     UNKNOWN
Test environment:                      UNKNOWN
Serial logging / recovery method:      UNKNOWN
```

Never pick an arbitrary RX 580/6700/7900 register table as a generic Radeon implementation. AMDGPU itself has per-ASIC IP block dependencies and firmware, so select **one actual test board** when phase A2 begins.

## 6. AMD architecture layers (roadmap only)

```text
apps/desktop compositor
      │ GPU surface handle, display request (future IPC)
      ▼
DisplayManager  ←→ GOP fallback / rollback
      │
AMDGPU DriverFacade
      ├── PCI device ownership
      ├── ASIC family and IP-block table
      ├── Firmware policy / PSP
      ├── SMU power/thermal and reset
      ├── GMC memory and VM
      ├── IH interrupts
      ├── DCN / DCE display controller
      ├── SDMA copy / page operations
      └── GFX/CP rings, scheduling
                 │
      kernel resource broker (MMIO/IRQ/DMA/IOMMU)
                 │
         target AMD GPU
```

Separate display and rendering: **seeing a screen via GOP does not mean the AMD driver owns it**; displaying pixels is not proof that shader/3D GPU acceleration is active. Native AMD userspace 3D drivers (e.g. Mesa architecture) are distinct from a kernel display/device driver.

## 7. Firmware, IP blocks, and licensing gates

AMD GPUs often require hardware-specific microcode, power management and/or trusted processor interactions for full initialization. GPUOpen ISA documents help understand instructions but **do not substitute** for driver register descriptions, firmware, scheduling and IP dependencies. Before A4/A5:
1. Identify exact ASIC/IP revisions; inspect Linux AMDGPU driver initialization sequences **as reference material**, not as drop-in Rust source.
2. Determine required firmware set, signatures, license/distribution rights and integrity validation. Never flash VBIOS from ZillaOS at this stage.
3. Implement strict parsing length/version/checksum sanity and safe failure. Never trust GPU or firmware metadata for out-of-bounds mappings.
4. Explicitly order init/resume/teardown per block. On any error, stop and return to safe display fallback; no secret global GPU state.
5. Treat PSP, SMU, GMC, IH, DCN, SDMA and GFX as separate IP units with known revisions.

Relevant open references:
- AMDGPU driver IP blocks: https://docs.kernel.org/gpu/amdgpu/driver-core.html
- Linux AMDGPU overview: https://docs.kernel.org/gpu/amdgpu/index.html
- AMD GPU architecture PDFs and firmware overview: https://gpuopen.com/amd-gpu-architecture-programming-documentation/
- AMD RDNA ISA XML specs: https://gpuopen.com/machine-readable-isa/
- DMA mapping / bus addresses: https://docs.kernel.org/core-api/dma-api-howto.html
- Kernel Mode Setting design: https://docs.kernel.org/gpu/

These documents are **background and design references**; AMD Radeon Linux packages are not a binary driver ZillaOS can simply install.

## 8. First development batch for Codex / AMD agent (A0 only)

**Own files**: new `src/drivers/gpu/amd/**`, corresponding tests. **Do not modify** ProcessManager, Desktop, current VMware SVGA code, `graphics.rs`, or boot/ISO scripts. If module exports are needed, ask GPU/Integration owner for one tiny patch after reading newest HEAD.

Deliverables:
1. Define `AmdGpuIdentity`, `AmdGpuProbeStatus` (Absent / DetectedUnsupported / SupportedByExplicitTable / Invalid), `AMD_VENDOR_ID=0x1002`.
2. Implement `match_amd_display(PciIdentity)` as a pure function; keep tested ID allowlist empty or only IDs backed by verified reference, with no advertised support for unknown IDs.
3. Host tests: correct AMD display class, AMD non-display function ignored, `0x1022` ignored, VMware `15ad` ignored, invalid vendor ID, various revisions/subsystem IDs, deterministic snapshots.
4. Ensure **zero MMIO, config writes, DMA, firmware access or framebuffer edits** in A0.
5. Publish a short A0 report listing source paths, test commands/results and what is not implemented.
6. Only after generic DeviceRegistry interface is agreed: add A1 read-only boot diagnostic, behind opt-in if the integration changes normal boot path.

**Definition of Done A0:** a deterministic Rust `no_std` matcher with host unit tests and a *clear `Unsupported` default*. No "hardware acceleration enabled" log.

## 9. Tests / validation / rollback

- Pure test first; `cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline`.
- `cargo +1.90.0 fmt --all -- --check` and bare-metal `cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features vm-demo` after module integration.
- Simulated PCI bus with known/unknown AMD device, other vendors, bridged topology, duplicate function, truncated capacity and malformed config.
- Default desktop with no AMD keeps all VMware SVGA and GOP paths functional. Unavailable AMD never panics/hangs.
- At A2+ use test hardware with **logs and recovery**; never write disk, update firmware, enable GPU bus mastering or reset display controller without explicit approval and safe resource policy.
- **Rollback:** software renderer, GOP display state and VGA/VMware integration remain available. A failed AMD bind must not corrupt `DisplayManager` mode tracking.

## 10. Workstream coordination

- **AMD/GPU agent** owns only `src/drivers/gpu/amd/**` in A0, later works with Graphics/SVGA owner on shared `src/video/**`.
- **Platform/PCI agent** owns proposed `src/bus/pci/**` and device resource model; reads/coordinates `src/pci_io.rs`.
- **Process/Security agent** owns process and capability isolation; shared DMA/IOMMU requires joint review.
- **Services agent** owns ServiceManager models, future `deviced`.
- **Integration maintainer** alone merges changes to `src/main.rs`, `src/lib.rs` and `Cargo.toml` after current HEAD check.

Following `AGENTS.md`: no force pushes, no rewriting other stream's modules, report `source / host test / build / VM / physical GPU` independently.

See [Driver Framework](DRIVER_FRAMEWORK.md), [Service Framework](SERVICE_FRAMEWORK.md), [Agent coordination](AGENT_COORDINATION.md), [VMware SVGA](VMWARE_SVGA_DRIVER.md).

## 11. Additional research links and implementation scope

- [AMD hardware lab](AMD_HARDWARE_LAB.md): collect exact PCI `1002:xxxx`, ASIC/IP revisions, subsystem, evidence; VMware guest typically does not expose host Radeon.
- [PCI/ACPI resource model](PCI_ACPI_RESOURCE_MODEL.md): one BDF/segment inventory, ECAM/MCFG, BAR ownership, typed MMIO, IRQ, DMA/IOMMU.
- [Display stack contract](DISPLAY_STACK_PROTOCOL.md): surface vs scanout vs 3D acceleration, atomic validation before hardware commit, output generation and poisoning.
- [Parallel workstreams](PLATFORM_WORKSTREAMS.md): AMD A0 only on first pass, avoid modifying GPU/VMware shared files.

**Correction:** The GOP fallback is reliable for the initial software framebuffer *while native driver has not reprogrammed display hardware*. Once a Radeon DCN/DCE is reset or modeset, a previously valid firmware framebuffer may cease to scan out. Any post-commit recovery must be verified per ASIC; default rescue diagnostics are independent serial/log capture rather than a blanket GOP promise. See [UEFI GOP protocol](https://uefi.org/specs/UEFI/2.10/12_Protocols_Console_Support.html) and [DRM atomic modeset](https://docs.kernel.org/gpu/drm-kms.html).
