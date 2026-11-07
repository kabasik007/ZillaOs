# Platform I-M1 — one runtime PCI authority

**Branch:** `feat/platform-im1-single-inventory` (branched from integration PR #19).
**Review chain:** #17 Platform foundation, #12 architecture, #19 cross-workstream integration, #20 acceptance issue.
**Implementation state:** boot-time PCI model with a **single** segment-0 hardware inventory read pass
and one kernel-owned `DeviceRegistry` instance. No device resource activation.

## Live read-only flow

```text
src/main.rs (vm-demo, single-core)
    |
    +--- pci_io::discover_boot() once
             |
             | legacy CF8/CFC read32 (existing unsafe port adapter)
             v
       LegacyConfig / scan_legacy_segment0 (0..=255)
             |
             +--> temporary bounded PciInventory<64>, malformed/overflow checks
             +--> DeviceRegistry<64, 0> (authoritative after import)
             +--> cache validated VMware PCI config dwords (read once)
                        |
       BootPci owns ----+----> StorageInventory snapshot (no bus I/O)
                        +----> VideoAdapter snapshot (no bus I/O)
                        +----> VMware legacy_read32 for unchanged PCI matcher
                        |      (pure in-memory; existing SVGA register reader still opt-in)
                        +----> AMD A0 AmdRegistryAdapter (no bus I/O)
```

The temporary `PciInventory` is dropped when `BootPci::discover` returns.
The registry is **not cloned into another global inventory**. `DeviceSnapshot`
copies are immutable observations, not owners or driver-binding authority.
`src/pci_io.rs` performs the physical scan **once** in vm-demo. Existing
`storage_pci::enumerate_storage`, `video::probe_pci_video`, and
`vmware_svga::pci::locate` remain present as stable testable APIs, but the
first two are no longer invoked at boot; `locate` sees only cached read32 data,
never the physical port reader.

## Public API

- `platform::pci::scan_legacy_segment0::<CAP>(&mut config, last_bus)`: one
  comprehensive bounded segment-0 scan, preserving old all-bus compatibility.
  It uses the **same parser** as bridge-aware `scan_topology`, not an independent
  second parser; bridges and multifunction are handled in either mode.
  Hardware adapter `LegacyConfig` only permits segment zero, offsets 0..255.
- `platform::boot::BootPci::<CAP>::discover(config, last_bus)`:
  bounded **single-writer** boot gateway with fail-closed `BootScanStatus`,
  `report`, `registry()`, `snapshot()`, `storage_inventory()`,
  `video_adapter()`, `legacy_read32()`. Boot uses `CAP=64`.
- `platform::amd_bridge::AmdRegistryAdapter::<DEVICES, CLAIMS>`:
  implements existing AMD `ReadOnlyPciInventory` against the same registry.
  It preserves segment, full BDF, class, revision and optional subsystem IDs;
  no AMD-specific PCI discovery and no active driver initialization.
- `DeviceRegistry` remains the **sole** owner of DeviceIds, states, matches
  and metadata resource claims. Boot imports entries into `Discovered` state
  only, with zero resource claims. No driver is activated or bound on boot.

## Safety and bounded behavior

- A malformed header/bridge, config read error, scan overflow, failed registry
  import or VMware header cache overflow means `ready=false`: no clients
  can see a partial, deceptively complete PCI registry, and GOP fallback remains.
- The legacy full-bus mode checks all 256 bus numbers for compatibility with
  old VMware/QEMU behavior, even when bridge topology is unknown. It is NOT an
  ACPI-proven PCI host bridge topology, PCIe ECAM or multi-segment inventory.
  `scan_topology` remains available for known segment/root topology scans.
- Runtime capacity: **64 PCI functions** and **4 VMware config header caches**.
  Overflow is explicitly rejected, not silently overwritten.
- VMware header readback stores raw command and BAR dwords, **not BAR sizes**.
  The values may only drive existing read-only VMware validation: resource
  ownership and MMIO mapping are NOT granted.
- Device Registry uses generation-checked handles; old handles after remove or
  fault cannot be reused. Active hotplug/teardown and SMP locking are not ready.
- Existing `svga-id-negotiate` opt-in register handshake belongs to the older
  VMware driver. This PR neither enables the feature by default nor adds
  register writes; standard vm-demo uses only read-only SVGA register inspection.
- Hardware config reads: CF8 selector port write followed by CFC read, which
  **does not write PCI configuration registers**. No writes to PCI command,
  BAR sizing, bus mastering, MSI/IRQ, MMIO, DMA, disk sectors or partitions.

## Tests / acceptance

Host Rust unit tests cover: one scan powering three adapters without additional
config reads; far bus/device31/function7, multifunction, malformed header/bridge,
negative config backend, overflow fail closed, strict AMD identity, registry
generation and exclusive resource claims. VMware legacy PCI probe uses mock
cached dwords with unmodified validation logic.

Dedicated `.github/workflows/platform-im1.yml` runs:
- Rust 1.90 host library tests + bare-metal default, vm-demo and Ring3 build
- pinned rustfmt for Platform modules, and Python regression tests
- QEMU+OVMF CLI smoke with disposable ISO, plus QEMU desktop smoke.

**Hardware evidence:** QEMU smoke is virtual guest proof only, not VMware
Workstation testing and not physical AMD or PCIe ECAM validation. Remains
unsupported: ACPI host bridge ranges, multi-segment ECAM, genuine resource
allocator, verified BAR lengths, IRQ allocation, DMA/IOMMU, hotplug and
real GPU display bring-up.

## Merge ordering

Because PR #19 contains staged AMD code and `amd_bridge.rs`, this work is
proposed as a **child PR targeting the PR #19 integration branch**. Do not
merge it into `main` or attempt to replace the original #17. Integrator
should verify concurrent changes and reconcile this patch when #19 is ready.
