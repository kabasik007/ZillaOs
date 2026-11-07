# ZillaOS PCI/DeviceManager Foundation — implementation contract v0

Status: **read-only, pure no_std models**. Branch: `feat/platform-pci-foundation`.
Architecture background: [DRIVER_FRAMEWORK.md in PR #12](https://github.com/kabasik007/ZillaOs/pull/12) and [PCI_ACPI_RESOURCE_MODEL.md in PR #12](https://github.com/kabasik007/ZillaOs/pull/12).

## Verified code vs planned integrations

| Layer | Implemented in this branch | Deliberately NOT implemented |
| --- | --- | --- |
| PCI address/config | Segment-aware `PciAddress`; `PciConfigRead`, `MockConfig`, `LegacyConfig` | Runtime replacement of `src/pci_io.rs`; cross-CPU CF8 lock |
| Discovery | Caller-owned bounded `PciInventory<N>`, multifunction, bridge BFS, visited-bus bitmap, diagnostics | ACPI host bridge discovery, hotplug, PCI extended caps |
| Device ownership | `DeviceRegistry<D,R>`, generation IDs, matching, atomic metadata-only resource claims, snapshots | Live kernel-global registry; driver binding, unbinding of running hardware |
| ACPI MCFG | SDT signature/length/checksum, entry validation, overlap/overflow guards | RSDP/XSDT memory walking, firmware address trust |
| PCIe ECAM | Checked physical address plan; disabled reader | ECAM MMIO mapping, physical config reads or writes |
| Resource HAL | Type contracts and `DisabledHardware` for MMIO/IRQ/DMA | Actual page mapping, cache-policy programming, MSI, IOMMU |

**Important integration rule:** This branch introduces the *types*, not a second global inventory.
The existing `src/pci_io.rs` remains the only live early CF8/CFC adapter. Storage/video/VMware
continue their current bounded read-only probes. Later integration should scan *once* at boot
via one platform owner and pass immutable views to all drivers. Do not let GPU agents create
a parallel static `PCI_DEVICES` list.

## API summary

```rust
// import from zillaos_kernel::platform
let mut mock = pci::MockConfig::new(|addr, offset| -> Result<u32, pci::ConfigError> {
    // caller supplies mock dword readings; never writes
    Ok(u32::MAX)
});
let inventory = pci::scan_topology::<64>(&mut mock, 0, 0, 255)?;
let mut devices = device::DeviceRegistry::<64, 128>::new();
devices.import_inventory(&inventory)?;  // atomic preflight, rejects truncation
let snapshot = devices.snapshot();        // copy, no raw BARs
```

The sketch uses `?` only inside a caller that maps both error types.

- `PciAddress { segment: u16, bus: u8, device: u8, function: u8 }`
  validates device 0–31 and function 0–7.
- `PciIdentity` contains vendor/device/revision, `PciClass`, optional subsystem IDs.
- `LegacyConfig<F>` adapts the old `storage_pci::PciLocation`, rejecting nonzero
  segments, unaligned config offsets and offsets outside 0–255 **before any port read**.
  The adapter performs no writes, but the calling kernel is responsible for synchronizing
  CF8/CFC across CPUs and interrupt contexts; it is not plugged into boot here.
- `scan_topology::<N>(config, segment, root_bus, last_bus)` follows validated
  primary/secondary/subordinate bridges, only visits each bus once, and retains up to
  `N` functions. It reports `truncated` and error counts. It scans **one reachable root**
  rather than silently claiming exhaustive discovery across ACPI PCI host bridges.
  No BAR sizing or config writes.
- `DeviceRegistry<D,R>` is **single-writer** until future locking. `DeviceId` has
  slot + generation. Mutations reject stale handles, duplicate addresses, unsupported
  matches, resource overlaps, unverified ranges and insufficient capacity. Claims
  preflight the entire request before applying it. Fault/removal revoke metadata claims
  and increment generation; generation saturation retires the slot on remove.
- `DriverMatch::Exact` matches a specific vendor/device pair, optionally class;
  `DriverMatch::Class` is opt-in for deliberately generic drivers.
- States: Discovered → Matched → Claimed → ProbedReadOnly.
  `Active` is reserved for future verified hardware initialization; no method
  in this stage can transition to it.
- `ResourceRange` uses half-open intervals; an explicit
  `ResourceSource::FirmwareValidated` is required even for **metadata claims**.
  This is a model-level assertion; it does **not** authenticate a firmware allocator.
  Live MMIO/IRQ/DMA is always disabled.
- `McfgTable<N>` parses a caller-provided, length-bounded MCFG byte buffer.
  ECAM address uses `MCFG base + (absolute_bus << 20) + (device << 15)
  + (function << 12) + dword_offset`. The table entry's bus range bounds
  this arithmetic. Do not confuse the MCFG table parser with safe physical memory
  access or validated ACPI root discovery.

## Common PCI identity: AMD / VMware

| Field | VMware SVGA II | AMD Radeon read-only candidate |
| --- | --- | --- |
| Vendor ID | `0x15ad` | `0x1002` |
| Device ID | `0x0405` | **must be explicitly listed for an active driver** |
| PCI display class | `0x03` required | `0x03` required |
| Additional discrimination | Existing VMware BAR and IO decode checks remain in its driver | ASIC/IP revision, firmware and power/display stack are separately required |
| Meaning of discovery | Can be reported; not proof of accelerated scanout | `DetectedUnsupported` until explicit model+revision support |

**Do not classify vendor `0x1022` (AMD host/CPU platform) as an AMD graphics card.**
No GPU family may claim a device merely because a vendor ID matches. VMware and AMD
should consume the same `PciDeviceInfo`, then run driver-specific *read-only* validation.

## Hardware safety / concurrency gates

1. Never access ECAM until the ACPI table source is trusted, memory ranges are
   validated against the actual boot memory map, and a UC MMIO mapper exists.
2. Do not infer BAR length from readback; never size BARs with writes in this stage.
3. Mapping needs page alignment, cache-attribute and overlap checks, generation-safe
   handles, and revocation before hot-remove.
4. IRQ/MSI/MSI-X allocation requires a central vector owner, capability validation
   and synchronization with teardown.
5. DMA requires pinned pages, bus/IOVA vs CPU physical separation, cache coherence,
   and an IOMMU-domain policy or a documented privileged-driver exception.
6. `DeviceRegistry`'s `&mut self` mutation contract is not an SMP lock.
   Future shared ownership needs a kernel lock with interrupt-context rules.
7. Device fault/removal here invalidates **metadata only**, not live hardware.
   Do not attach active drivers until IRQ, DMA, MMIO teardown are implemented.

## Validation matrix

**Host unit tests** (inside new Rust modules):
- Maximum BDF/segment, invalid device/function and legacy offset/segment denial
- VMware and AMD strict identities, AMD chipset or non-display negative cases
- Single vs multifunction, bridge traversal, malformed header and invalid bridge
- Empty scan and bounded inventory overflow
- Duplicate device identity and stale generation handles after removal/fault
- Exact driver match, wrong-driver claims, overlap and transactional claim behavior
- Unverified/overflowed resource denial and disabled HAL
- Valid MCFG with nonzero start bus, invalid length/checksum/reserved fields,
  overlapping allocations, excessive entry count, arithmetic overflow and unmapped ECAM

**Host command:** `cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline`

**Bare metal compilation:** `cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features vm-demo`

**Real hardware:** not performed; no VMware runtime or physical AMD/Intel/NVIDIA
testing is claimed. A passing bare-metal compile does not imply IRQ/DMA/display works.

## Suggested integration after this PR

1. Wire the **existing** `src/pci_io.rs` reader through `LegacyConfig` and
   construct one kernel-owned PCI inventory, without removing legacy callers until
   snapshot compatibility is proven. Start with serial diagnostics, not binding.
2. Add ACPI root bridge discovery + memory-map validation, then an actual mapped
   read-only ECAM backend with serialization; preserve CF8 fallback.
3. Integrate storage, VMware and AMD matchers one consumer at a time;
   introduce real resource claims only when authoritative resource sizing and
   safe teardown exist.

No ProcessManager, VMware driver, desktop, partitions or physical disk operations
are changed by this branch.
