# ZillaOS — AMD GPU Hardware Lab: identification, safe experiments, recovery

> **Status:** runbook for a FUTURE AMD test environment, not a claim that an AMD GPU is available in current VMware. Date 2026-11-07. All active hardware experiments require a dedicated/disposable setup and explicit user approval.

## 1. Separate three environments

| Environment | What you can verify | Cannot conclude |
| --- | --- | --- |
| Host unit tests with mock PCI | class/vendor matching, resource arithmetic, error state | real GPU PCI/firmware/MMIO |
| QEMU/VMware without PCI passthrough | boot, GOP, UI, simulated PCI, VMware SVGA if available | real AMD native initialization |
| Dedicated AMD physical test host (or verified passthrough) | actual device IDs, BARs, native mode/firmware/IP, real scanout | generalized support for every AMD generation |

Default VMware Workstation guest usually sees VMware SVGA rather than a passed-through host Radeon. Merely seeing AMD graphics in Windows host Device Manager **does not** make it accessible to ZillaOS guest.

## 2. Collect hardware identity (no writes)

Record:

| Field | Value / how to obtain |
| --- | --- |
| GPU marketing model | unknown until user reports |
| Board manufacturer and SKU | unknown |
| PCI vendor/device | full `VEN_1002&DEV_XXXX` or `1002:xxxx`, verify |
| PCI subsystem vendor/device | `SUBSYS`, verify |
| Revision and class code | verify |
| PCI domain/bus/device/function | verify |
| APU vs discrete | verify |
| Firmware/BIOS, VBIOS version | read-only observation |
| Display connectors/cables/monitor | verify |
| VRAM/GTT/UMA topology | verify, no guessed MB |
| Target x86-64 platform/UEFI/Secure Boot | verify |
| IOMMU state and kernel PCI topology | verify |
| Crash recovery method / serial output | identify before active work |

**Windows, read-only identification**:

```powershell
Get-PnpDevice -Class Display
Get-CimInstance Win32_VideoController |
  Select-Object Name,PNPDeviceID,DriverVersion,AdapterRAM
```

Caution: `AdapterRAM` is often unreliable for large VRAM or UMA. Use it only as an indicative source, not authoritative GPU memory sizing. Device Manager → Display adapters → Properties → Details → Hardware IDs is another read-only route.

**Linux live OS, read-only**:

```bash
lspci -nn -D | grep -iE 'VGA|3D|Display'
lspci -nn -D -v -s <BDF>
```

Use a real BDF in place of `<BDF>`; do not execute a placeholder literally. Collect PCI IDs and driver-in-use, not personal serial numbers. Avoid modifying PCI configuration with `setpci` at this stage.

## 3. Pick one narrow initial target

After receiving exact PCI ID and board:
1. Confirm AMD Radeon graphics function (`0x1002`, display class) rather than CPU/chipset.
2. Look up verified ASIC family + IP revisions from board/driver source and official documents.
3. Document which of DCE/DCN, GMC, IH, PSP, SMU, SDMA, GFX exist.
4. Record firmware requirements and licenses. No binary copied into ZillaOS without rights review; no VBIOS flashing.
5. Make an explicit `SupportedForReadOnlyDiscovery` table entry; **do not** mark `Operational` before physical test.
6. Choose first active research goal: read-only verified mapped register (after MMIO broker) OR passive connector inventory, never all components at once.

## 4. Safety and recovery setup

- Use non-production GPU and machine, backup data, disposable boot media, avoid host system disk mount/write.
- Retain a known-booting OS and independent boot path. Have second display/remote console if possible.
- Log serial output independently (COM/UART when present), keep CPU serial markers even when graphical output disappears.
- Physical monitor/video capture for display tests; a working serial log alone does not prove video scanout.
- **Do not** enable DMA/bus mastering without confirmed resource and isolation policy.
- Keep thermal/power management untouched until SMU/PSP versions and safe sequencing are understood.
- Avoid blind register writes, unbounded loops, reset storms, firmware flashing, experimental overclocking.
- Understand that after native modeset GPU may not retain firmware GOP scanout. Headless diagnostics is the safety fallback, not guaranteed pixels.
- Include precise fail-closed flags in test configuration (e.g. `amd_probe_readonly` enabled explicitly), and retain default boot unaffected.

## 5. Experiment levels and evidence

### H0 — mock detection (safe now)
- Pure `PciIdentity` matcher, supported/unsupported split and tests.
- Capture `cargo test` logs, no physical GPU claims.

### H1 — physical read-only PCI inventory
- No register writes, no BAR probing writes, no GPU resets.
- Record `segment:BDF`, vendor/device, subsystem, revision, class, BAR descriptor types where safe.
- Log `ZILLAOS_AMD_GPU_PCI=DETECTED_UNSUPPORTED` until device-specific support proven.
- Compare independent read-only inventory from another OS.

### H2 — resource proof
- Firmware/PCI resource map validation, BAR extent, UC mapping, IRQ identity, MMIO read permitlist.
- Failures: unverified size, BAR overlaps, stale claim, invalid revision, absent IOMMU.
- No GPU mode switch yet.

### H3 — first display hardware interaction
- Requires device-specific spec, safe power/firmware dependencies, output topology, validated EDID, documented rollback.
- Test one monitor / one mode, record actual external image and confirmed mode readback.
- On failure, report device fault; do not claim GOP is guaranteed after partial programming.

### H4 — memory, queues, 2D/3D
- Requires DMA mappings, IOMMU policy, memory fences, interrupt handling, queue validation, timeout/watchdog and recovery.
- Test only on whitelisted ASIC/IP revision; expand after measurable evidence.

## 6. Evidence log template

```yaml
experiment_id: "amd-h1-0001"
date_utc: "<ISO timestamp>"
zillaos_commit: "<commit SHA>"
build_feature: "<opt-in feature name>"
environment: "mock|VMware|QEMU|physical|passthrough"
machine_class: "<no personal identifiers>"
gpu_pci_id: "1002:xxxx"
gpu_revision: "<hex>"
gpu_subsystem: "<vendor:device>"
driver_phase: "H1_READONLY_PCI"
serial_capture_file: "<path in test artifacts>"
video_capture_file: "<path if applicable>"
result: "PASS|FAIL|BLOCKED"
verified_claims: []
not_verified: ["MMIO", "mode switch", "GPU acceleration"]
rollback_result: "NOT_NEEDED|PASSED|FAILED|NOT_TESTED"
```

Test artifacts must not contain passwords, user files, private firmware blobs or unnecessary hardware IDs. Every `PASS` needs explicit evidence.

## 7. Two-dimensional acceptance (hardware vs feature)

A test is only "AMD supported" when both axes are satisfied:
- **Hardware axis:** exact PCI device/revision/subsystem/ASIC tested.
- **Feature axis:** PCI detect, BAR/MMIO, display mode, GPU memory, DMA, 2D, 3D separately.

A board passing `H1` is `Detected`, not `DisplaySupported`. A one-monitor mode test does not establish DP/HDMI, multi-monitor or suspend/resume.

## 8. Research notebook / sources

- AMDGPU IP blocks: https://docs.kernel.org/gpu/amdgpu/driver-core.html
- AMD display validation and firmware: https://docs.kernel.org/gpu/amdgpu/display/index.html
- AMD display firmware debug: https://docs.kernel.org/gpu/amdgpu/display/dc-debug.html
- PCI resource reservations: https://kernel.org/doc/html/latest/driver-api/pci/pci.html
- DMA mapping and IOMMU: https://docs.kernel.org/core-api/dma-api-howto.html
- UEFI specifications: https://uefi.org/specifications

Related: [AMD_GPU_BRINGUP.md](AMD_GPU_BRINGUP.md), [DRIVER_FRAMEWORK.md](DRIVER_FRAMEWORK.md), [PCI_ACPI_RESOURCE_MODEL.md](PCI_ACPI_RESOURCE_MODEL.md), [DISPLAY_STACK_PROTOCOL.md](DISPLAY_STACK_PROTOCOL.md).
