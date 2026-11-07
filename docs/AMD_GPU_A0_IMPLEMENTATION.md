# ZillaOS — AMD GPU A0/A1 implementation and safety contract

**Stage:** A0 PCI identity/probe; A1 inert IP data structures (not initialization).
**Branch:** `feat/amd-gpu-discovery`. No GPU activation or AMD-specific boot probe is wired into `src/pci_io.rs`.

## Scope and ownership

- `src/drivers/gpu/amd/pci.rs` — pure `PciAddress`, `PciIdentity`, DWORD snapshot parser; `ReadOnlyPciInventory` trait, no port I/O and no PCI enumeration.
- `src/drivers/gpu/amd/device_ids.rs` — diagnostic family hints and **empty** `SUPPORTED_BOARDS` table.
- `src/drivers/gpu/amd/ip_blocks.rs` — display DCE/DCN, GMC, PSP, SMU, IH, SDMA and GFX version contracts. All IP revisions default to Unknown.
- `src/drivers/gpu/amd/mod.rs` — `match_amd_display`, explicit `DetectedUnsupported` versus `SupportedByExplicitTable`, one-address snapshot lookup and mock-driven unit tests.
- `src/drivers/gpu/mod.rs` — one-line module export only; existing VMware code untouched.

The Platform PCI agent owns real enumeration, bridge traversal, segment resources and future registry. The AMD probe consumes the immutable output; **do not add a second scanner**. `PciAddress::from_legacy_segment_zero` is a pure field adapter for current `storage_pci::PciLocation`; it does not perform CF8/CFC access. Nonzero PCI domains remain representable but are not promised to work with legacy CF8/CFC.

## Diagnostic IDs (not operational support)

| PCI vendor:device | Family-level diagnostic hint | Status in ZillaOS |
| --- | --- | --- |
| `1002:67df` | Polaris 10 | DetectedUnsupported |
| `1002:731f` | Navi 10 | DetectedUnsupported |
| `1002:73bf` | Navi 21 / Sienna Cichlid | DetectedUnsupported |
| Any other `1002:xxxx` with display class and VGA/3D/other display subclass | Unknown | DetectedUnsupported |

No revision, subsystem, memory size, connector, or firmware compatibility is deduced from these PCI IDs. `SUPPORTED_BOARDS` is intentionally empty. Never set it based solely on Linux compatibility: require an exact verified device/revision/subsystem, IP versions, safe firmware and hardware-specific lab evidence.

## Data flow (zero hardware changes)

```text
Platform PCI/DeviceRegistry snapshot (future shared owner)
   -> PciIdentity { segment:BDF, IDs, revision, class, subsystem }
   -> match_amd_display (PURE)
       |- 0xffff: Absent
       |- wrong vendor/class/subclass: NoMatch
       |- bad BDF / malformed identity: InvalidPciIdentity
       '- 1002 + display: DetectedUnsupported
           -> diagnostic ASIC family hint
           -> AsicIpInventory::unknown(...)
           -> no bind, no MMIO, no DMA, no firmware, no modeset
```

Future PCI Platform adapter should read `0x00`, `0x08`, `0x0c` and only **type-0** header `0x2c` for subsystem IDs, once per platform snapshot. `PciIdentity::from_config_dwords` parses these values; it does not read them itself. Avoid `0xffffffff` BAR sizing writes. Device ID alone does not prove GPU ownership, especially with platform functions.

## A1 IP-block contracts

Every family has separate versioned IP records:
- `DisplayDce` / `DisplayDcn`: mutually exclusive verified display path; neither is present by default
- `Gmc`: GPU memory and page-table/addressing responsibilities
- `Psp`: secure processor/firmware prerequisites
- `Smu`: power/thermal control prerequisites
- `Ih`: interrupt routing and fault handling
- `Sdma`: copy engine/queues
- `Gfx`: graphics command processor and graphics/compute queues

`IpDiscovery::Unknown` is deliberately distinct from `AbsentVerified` and `PresentVerified(IpVersion)`. A family hint cannot populate these versions. These structs have NO `init`, register, DMA, firmware or queue methods.

## Validation levels and expected outcomes

```bash
cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features vm-demo
cargo +1.90.0 fmt --all -- --check
python3 -m unittest discover -s tests -v
```

Test fixtures cover vendor `0x1022`, AMD non-display/audio/storage, VMware `15ad:0405`, unrecognized AMD GPU, absent `ffff`, malformed IDs/class/header/subsystem, full segment BDF, mismatched registry address, one-lookup-only mock and unknown IP revisions. The pure model cannot prove AMD hardware detection in VMware, modesetting or rendering.

**Not implemented:** active Radeon GPU driver, physical PCI integration/boot serial marker, DeviceRegistry claims, BAR mapping, MMIO reads/writes, PSP/SMU firmware, VRAM management, GPU scheduler, IH interrupts, SDMA transfers, GFX commands, EDID, DCN/DCE modeset, acceleration, reliable post-modeset GOP restore.

Future A2: select ONE actual test Radeon, record exact `1002:xxxx`, subsystem/revision, ASIC/IP versions, PCI topology and recovery log; follow PR #12 hardware lab plan. Do not enable bus mastering or GPU writes until resources, firmware, DMA/IOMMU and mode recovery are independently verified.

## References

- ZillaOS architecture proposal: https://github.com/kabasik007/ZillaOs/pull/12
- AMDGPU driver/IP architecture: https://docs.kernel.org/gpu/amdgpu/driver-core.html
- PCI driver model: https://docs.kernel.org/driver-api/pci/pci.html
- GPUOpen AMD architecture documentation: https://gpuopen.com/amd-gpu-architecture-programming-documentation/
- GPUOpen ISA specifications (NOT device register programming guides): https://gpuopen.com/machine-readable-isa/
- PCI identity evidence: https://drmdb.emersion.fr/properties/connector/EDID/devices?driver=amdgpu
- Polaris `67df` hardware evidence: https://www.phoronix.com/review/radeon-rx580-linux/2

No Linux driver code, firmware blobs or external headers are copied into ZillaOS. No VMware, Desktop, ProcessManager, WindowManager, boot/ISO or storage behavior is changed by these AMD model files.


## I-M1: canonical Platform DeviceRegistry integration (2026-11-07)

**Dependencies:** [Platform #17](https://github.com/kabasik007/ZillaOs/pull/17),
[Integration #19](https://github.com/kabasik007/ZillaOs/pull/19),
[Graphics #15](https://github.com/kabasik007/ZillaOs/pull/15),
[Architecture #12](https://github.com/kabasik007/ZillaOs/pull/12).

PR #13 is now based on `integration/20261107-cross-workstreams` because the
canonical Platform PCI API and `src/platform/amd_bridge.rs` live in #19,
not `main`. A two-parent non-force merge commit preserved the original A0
history while importing #19's exact tree. No second Platform source tree,
global registry or PCI scanner was added. **This is a temporary stacked-PR
dependency**, not a change to `main`, and must be reviewed by the integration
maintainer before either PR is merged.

### Authoritative data flow

```text
Platform PCI scan_topology (once, read-only)
    -> Platform PciInventory (bounded / segment-aware)
    -> Platform DeviceRegistry::import_inventory (single owner)
    -> DeviceRegistry::snapshot / get(DeviceId)
    -> platform::amd_bridge::diagnose_registry[_device]
    -> to_amd_identity (lossless compatibility conversion)
    -> amd::match_amd_display (strict A0 candidate filter)
    -> DetectedUnsupported + unknown ASIC/IP profile
    -> optional read-only DisplayManager handoff; NO output/modeset
```

- `AmdInventoryAdapter` is retained as a legacy read-only compatibility
  adapter for early tests. The canonical path is **DeviceRegistry**, not a
  separate AMD scan. It never calls `PciConfigRead` during diagnosis.
- `DeviceId {slot, generation}` is checked by the Platform registry on
  every lookup. Old generation after fault, removal or re-insertion is
  rejected; a `PciAddress` is never treated as a resource capability.
- `diagnose_registry_device` accepts **only unowned Discovered** AMD PCI
  display endpoints (header type 0, no bridge descriptor). It rejects
  malformed identity, missing subsystem pair, invalid BDF, invalid class,
  invalid device ID, claimed/matched devices and stale device IDs.
  Non-AMD and AMD non-display functions are ignored, not bound.
- `diagnose_registry` is a **bounded two-pass copy**. Its caller-supplied
  output remains unchanged on an error or too-small capacity. This does
  not mutate the registry, driver binding state or resource claims.
- `AmdReadOnlyDiagnostic` carries canonical `DeviceId` and preserved
  segment/BDF/device/subsystem/revision, plus an `AsicIpInventory` with
  unknown IP versions. `revalidate_diagnostic` must succeed before
  any future use of saved diagnostics. Its `may_touch_hardware()` is
  always false.

### ASIC and display boundaries

`ip_blocks.rs` now exposes distinct **Display DCE/DCN, GMC, PSP, SMU,
IH, SDMA, GFX** typed contracts with `Unknown` as the default for every
family. `typed_contracts` checks block order, rejects impossible major
version zero and conflicting simultaneous verified DCE/DCN display engines.
Even a synthetic `PresentVerified` IP record is *not hardware attestation*
and never authorizes initialization.

`amd/display_contract.rs` supplies an inert `AmdDisplayHandoff`:
`OutputId=None`, zero advertised display modes, explicit blocker
(unknown display engine, invalid IP contract, or absent native driver)
and `can_modeset=false` / `can_submit_commands=false`. It intentionally
does not implement `ModeBackend` or `VideoDisplayBackend`. The
DisplayManager owner will only connect a real adapter after validated
resource ownership, firmware/ASIC evidence, physical monitor readback
and a *tested* recovery path. GOP recovery cannot be assumed after
native GPU reprogramming.

### I-M1 host fixtures and acceptance levels

New negative tests cover stale `DeviceId`, wrong segment/BDF, foreign
driver binding/claimed states, malformed type-1 endpoint,
incomplete subsystem, bad device/class, duplicate/unknown ASIC hints,
insufficient diagnostic output capacity, invalid/misaligned IP metadata,
DCE/DCN conflict, fake DCN versions that **still cannot modeset**,
and a counted mock `scan_topology` proving that registry consumption
adds **zero new PCI configuration reads**.

- Source: Rust `no_std` read-only models and mock tests; **implemented**.
- Host tests, Rustfmt, bare-metal build: check the latest #13 CI runs and
  exact SHA, not merely previous A0 success.
- QEMU/VMware actual boot: **not tested by these mock fixtures**.
- Physical Radeon GPU: **not tested**, no specific board has an operational
  driver. `SUPPORTED_BOARDS` remains empty.
- Hardware writes, MMIO/DMA, firmware, ring commands, reset, display modeset:
  **not implemented**; no new opt-in flag or default ISO changes.

Before merging: reconcile #13's stacked dependency with Integration #19,
verify CI for the exact final SHA, ensure no unrelated changes are pulled
into this PR's diff, and keep the original VMware/Desktop/ProcessManager code
untouched.
