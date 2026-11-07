# ZillaOS — Platform Engineering Documentation Index

**Дата:** 2026-11-07. **Status:** architecture/research proposals, not a release announcement.

This index is the navigation entrypoint for platform, driver, service and AMD GPU development. Sources reflect a GitHub repository review plus published AMDGPU/PCI/ACPI/DRM design references. The principal rule: **separate implemented code, validated source, emulated runtime, and native hardware evidence**.

## 1. Read in this order

| # | Document | Focus | Implementation status |
| --- | --- | --- | --- |
| 1 | [Current architecture](ARCHITECTURE_CURRENT.md) | actual repository baseline and known limits | existing audit; may lag new HEAD |
| 2 | [OS subsystems](OS_SUBSYSTEMS.md) | responsibilities, current → target layering | design |
| 3 | [Platform workstreams](PLATFORM_WORKSTREAMS.md) | Codex tasks, file ownership, dependencies | design |
| 4 | [Driver framework](DRIVER_FRAMEWORK.md) | PCI/DeviceManager, MMIO, IRQ, DMA, binding lifecycle | design |
| 5 | [PCI/ACPI resource model](PCI_ACPI_RESOURCE_MODEL.md) | CF8/ECAM, ACPI MCFG, BAR claims, IOMMU | research + design |
| 6 | [Display stack](DISPLAY_STACK_PROTOCOL.md) | compositor, DisplayManager, surfaces, atomic modeset | research + design |
| 7 | [AMD GPU bring-up](AMD_GPU_BRINGUP.md) | Radeon discovery/IP block phases A0–A6 | design |
| 8 | [AMD hardware lab](AMD_HARDWARE_LAB.md) | identify exact board, safe test gates, evidence | runbook |
| 9 | [Service framework](SERVICE_FRAMEWORK.md) | `zillad`, manifests, lifecycle, restart | design |
| 10 | [System services and IPC](SYSTEM_SERVICES_IPC.md) | versioned IPC, handles, authorization | design |

Established in repository:
- [Process tests](PROCESSES.md): Ring3, multi-thread cooperative and opt-in PIT IRQ0 preemption experimental paths.
- [VMware SVGA](VMWARE_SVGA_DRIVER.md): read-only probe, opt-in negotiation, FIFO models.
- [Graphics](GRAPHICS.md): software backbuffer/compositor, not GPU acceleration.
- [Security](SECURITY_ARCHITECTURE.md): target threat model and isolation gates.
- [Agent coordination](AGENT_COORDINATION.md) + [AGENTS.md](../AGENTS.md): mandatory collaboration rules.

## 2. Important distinctions

| Phrase | Only say it when |
| --- | --- |
| AMD detected | Read-only PCI vendor/device/class confirmed |
| AMD supported | Exact PCI ID/IP revision and specified feature physically tested |
| GPU acceleration enabled | Real validated command submission/fences/measurement exists |
| Dynamic resolution | Driver performs actual mode change and readback |
| User-space service running | Separate Ring3 PID created with isolation and IPC |
| Scheduler preemption | Hardware timer actually switches tasks; currently an **opt-in fixture** |
| SMP | More than one CPU executing tasks with safe scheduler/locks/shootdowns |
| Device security | Kernel validates capabilities and DMA/IOMMU policy at every access |
| Rollback to GOP | Verified *for this device state*; not blanket promise after native modeset |

## 3. Dependencies / critical path

```text
  [A] Current AMD unknown hardware
               |
               v
  PCI identity/parser (host tests)
               |
        DeviceRegistry/claims
               |
      MMIO + IRQ + DMA/IOMMU
               |
     selected ASIC/IP revision
               |
      firmware/power requirements
               |
    display scanout + safe handoff
               |
   GPU memory and command queues
               |
       real GPU acceleration

  Parallel:
  Process isolation → IPC/caps → ServiceManager/zillad → deviced/displayd
```

## 4. Priority for next one or two Codex passes

**Batch A (safe, parallel):** P0 pure PCI snapshot, A0 pure AMD matcher, S0 pure ServiceManager dependency model, I0 pure IPC envelope/capability checker, G0 mock display transaction tests. Follow [PLATFORM_WORKSTREAMS.md](PLATFORM_WORKSTREAMS.md).

**Batch B (integration):** one shared PciIdentity consumed by VMware/AMD, DeviceRegistry model, service snapshot, build/unit/QEMU checks. No GPU MMIO or user daemon execution without prerequisites.

**Batch C (hardware):** after specific GPU hardware and safe lab are identified, read-only diagnostics first; actual AMD modeset is a separate guarded milestone.

## 5. Research references (design inspiration, not ZillaOS dependencies)

- AMDGPU block model: https://docs.kernel.org/gpu/amdgpu/driver-core.html
- AMD Display Core: https://docs.kernel.org/gpu/amdgpu/display/index.html
- Linux DRM/KMS atomic display model: https://docs.kernel.org/gpu/drm-kms.html
- Linux PCI resource reservations: https://kernel.org/doc/html/latest/driver-api/pci/pci.html
- CPU/DMA/bus/IOMMU addresses: https://docs.kernel.org/core-api/dma-api-howto.html
- UEFI/ACPI: https://uefi.org/specifications
- PCI-SIG: https://pcisig.com/specifications

**Licence note:** Linux reference code is GPL-licensed; reimplementing architecture concepts is different from copying/porting source. Before importing substantial source, review software licensing and relevant firmware redistribution conditions.

## 6. Maintenance protocol

At the start of each new agent session:
1. Read `AGENTS.md`, `docs/AGENT_COORDINATION.md`, current HEAD and relevant existing modules.
2. Compare these design documents to code. Mark stale statements instead of assuming the spec has already been implemented.
3. Implement inside one owned folder, with negative tests and feature flags for risky experiments.
4. Include commit SHA, test commands and hardware evidence status in the report.
5. Update this index only if new major document families are added.

**Docs-only PR check:** Verify each path exists and all cross-links resolve; no compiled code or native GPU hardware validation is implied by documentation commits.
