# ZillaOS Installer — read-only partition planner v0.1

**Status:** pure Rust calculations committed to `main`. There is **no**
functional installer, **individual disk enumeration**, write API, GPT writer, filesystem,
boot-entry installer or persistent settings. Rust/QEMU/VMware behavior has
not yet been verified.

## Architecture and safety

```text
Future block driver (NOT IMPLEMENTED)
   -> enumerate real devices read-only
   -> identify model/serial/capacity/sector size
   -> user explicitly selects a device
   -> preview_for(DiskGeometry)   <-- implemented, PURE function
   -> show loss warning + full plan (future)
   -> separately confirm disk ID and data loss (future)
   -> create recovery metadata & GPT (future)
   -> filesystem and UEFI loader installation (future)
   -> verify reads and recover/rollback failures (future)
```

The source includes a **passive PCI mass-storage controller inventory**,
which does not identify individual disks, capacities or partitions. See
[STORAGE.md](STORAGE.md). The arithmetic-only partition planner currently stops at
`preview_for`. A separate [primary + backup GPT verifier](GPT.md) now parses
synthetic sector data via a read-only callback, but no driver can supply
real disk sectors yet. No APIs that
modify sectors are exposed by `src/install_plan.rs`.

### Planned partition geometry

For a hypothetical **blank dedicated disk** with logical blocks of
512 or 4096 bytes:

| Area | Position | Capacity |
| --- | --- | --- |
| Protective MBR + GPT/boot metadata reservation | first 1 MiB | 1 MiB |
| EFI System Partition (ESP) | begins at 1 MiB | 512 MiB |
| ZillaOS System (planned) | after ESP | remaining space, at least 8 GiB |
| GPT backup reservation | last 1 MiB | 1 MiB |

Minimum modeled drive size is **8 GiB + 512 MiB + 2 MiB**. This is
only a planner threshold, NOT a recommended realistic installation
size. Actual root filesystem overhead, filesystem allocation, recovery
and applications are not accounted for.

The module checks:
- Nonzero disk capacity;
- Supported 512/4096-byte logical sectors;
- Byte capacity divisible by sector size;
- Checked LBA arithmetic and reserve spacing;
- No partition overlap, insufficient-root rejection;
- Plans never extending into the final GPT reservation;
- Rejection of a plan reused for a differently sized disk.

Unit tests include 32-GiB sector-512/4096 layouts, 0/6-GiB/520-byte
unsupported geometries, capacity mismatch, minimum capacity, and near
`u64::MAX` geometry. There is no simulated actual block device: tests
only execute arithmetic.

### Safety rules for a real installation milestone

1. Prefer **virtual disks** in VMware/QEMU. Do not pass through any host
   physical disk until the installer is mature and independently audited.
2. The installer **never** silently selects a disk. Devices must be
   distinguishable by model/serial/size and selectable explicitly.
3. When devices contain partitions or signatures, show exact impacts,
   backup guidance and require a deliberate typed confirmation with
   the target disk identity.
4. Never write anything during disk scanning, partition preview or
   first-run language/region/hostname setup.
5. Storage writes must be a separate privileged service requiring
   capability-limited access to the selected block device.
6. Implement preflight checks, cancellation states, verification and
   recovery. No unsafe multi-step partial install on failure.
7. Explicitly verify that a Windows host disk/shared host filesystem
   is **never exposed** to the development VM.

### Test the planner without touching a disk

On Ubuntu WSL, after installing Rust:

```bash
cd /mnt/m/projects/ZillaOs
git pull origin main
cargo test --lib --target x86_64-unknown-linux-gnu --offline
```

For GUI onboarding checks in QEMU:

```bash
bash scripts/build.sh demo
python3 scripts/desktop_smoke.py --timeout 90
python3 scripts/desktop_smoke.py --setup-profile custom --timeout 90 \
  --output build/logs/desktop-qemu-custom.ppm
```

Default profile: Ukrainian/Ukraine/Kyiv/zilla-pc/Dark.
Custom profile: English/Poland/Warsaw/lab01/Light.
Actual QEMU results have not yet been observed.

Related: [First-run Setup](SETUP.md), [VMware testing](VMWARE.md),
[Desktop roadmap](DESKTOP_ROADMAP.md).
