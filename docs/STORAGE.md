# ZillaOS Storage Discovery — PCI inspection v0.1

**Committed to `main`, 2026-10-28. Source-level implementation only; compilation, QEMU boot and VMware behavior are not verified.**

## What the current code actually does

The development `vm-demo` ISO queries conventional PCI configuration
mechanism #1, bus group 0, by **reading** identity/class fields on buses
0–255, devices 0–31 and valid functions 0–7. It collects at most 8
mass-storage *controllers* in a fixed `no_std` inventory.

- `src/storage_pci.rs` — pure, host-testable PCI address construction,
  PCI class parsing, enumerator with mock configuration-register data,
  bounded controller inventory, overflow marker.
- `src/pci_io.rs` — VMware/QEMU-only access to x86 ports `0xcf8` and
  `0xcfc`; writing the configuration **address selector** at 0xcf8
  and then reading configuration **data** at 0xcfc. There are **no**
  writes to PCI device configuration registers.
- `src/main.rs` — early scan, serial class/vendor/device/BDF diagnostics,
  inventory passed to the first-boot Setup wizard.
- `src/setup.rs` — nine-stage wizard including a new **Storage**
  information page; `selectable_disks()` remains strictly zero.
- `src/graphics.rs` — read-only controller summary and explicit
  **DISKS NOT VERIFIED / INSTALLATION DISABLED** warning.

PCI class = 0x01 is recognized as a mass-storage controller. Subclasses:
SCSI (0x00), IDE (0x01), RAID (0x04), AHCI (0x06, interface 0x01), NVMe
(0x08, interface 0x02), or another storage interface. For reference,
VMware's `0x15ad` vendor is recognized for the UI label; the code does
not rely on hard-coded VMware device IDs to discover a controller.

**A PCI controller is NOT a disk.** One controller can have zero, one or
several drives, including CD/DVD devices. A controller inventory supplies
neither drive identity nor capacity nor partition contents, so these
fields must never be invented. A SATA CD-ROM controller from the boot
ISO may be visible even though the VM has no hard disk.

## Safety contract

1. **No disk I/O:** never issue ATA IDENTIFY, AHCI commands, NVMe admin
   commands, SCSI requests or sector reads from this module. Those need
   a separate carefully reviewed read-only device driver and validated
   mapped resources.
2. **No PCI device writes:** do not probe/size BARs by writing ones;
   do not enable bus mastering, alter the command register, configure
   DMA, touch PCI interrupts or reset controllers.
3. **No install targets:** zero selectable disks, even if controllers
   were found; the Wizard screen cannot initiate any installation.
4. **VM-only:** legacy config I/O is limited to `vm-demo` on x86-64;
   the code is **not** an ACPI/MCFG-based native machine PCI subsystem.
5. **Single CPU:** the port address/data transaction is not protected
   against concurrent cores or interrupt handlers. IRQs/preemption
   must not access these ports until proper serialization exists.
6. **No physical disks attached to the VMware research VM.** Test only
   read-only ISO. A disposable blank VMDK can be considered AFTER a
   read-only virtual block driver actually exists.

## Early-boot COM1 telemetry

```text
ZILLAOS_STORAGE_PCI_SCAN_READY
ZILLAOS_STORAGE_CONTROLLER_COUNT=0x0000000000000001
ZILLAOS_STORAGE_CONTROLLER=SATA / AHCI VENDOR=0x... DEVICE=0x... BUS=0x... SLOT=0x... FUNC=0x...
ZILLAOS_STORAGE_BLOCK_DRIVERS_UNAVAILABLE
ZILLAOS_SETUP_READY
```

The count and device values above are **illustrative**, not observed.
No controller may be present on an unsupported VM chipset. The scan
may report `ZILLAOS_STORAGE_CONTROLLER_LIST_TRUNCATED` if more than
eight controllers are discovered.

## Updated first-run sequence

1. Welcome
2. Language
3. Region
4. Time zone
5. PC name
6. Theme
7. **Storage** — read-only controller list; disk selection disabled
8. Install Plan — shows missing installation prerequisites, with writes disabled
9. Review — proceed into the RAM-only Zilla Desktop, not disk installation

Nine Enter presses accept default settings.

## Verify on WSL2 + VMware / QEMU

In Ubuntu, after Rust installation:

```bash
cd /mnt/m/projects/ZillaOs
git switch main
git pull origin main
cargo test --lib --target x86_64-unknown-linux-gnu --offline
python3 -m unittest discover -s tests -v
bash scripts/build_vmware.sh
```

Then in Windows PowerShell (only if the ISO exists):

```powershell
cd M:\projects\ZillaOs
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\vmware.ps1 -Launch
Get-Content .\build\vmware\ZillaOS\zillaos-com1.log -Tail 100
```

For automated QEMU screenshot evidence:

```bash
python3 scripts/desktop_smoke.py --timeout 90
python3 scripts/desktop_smoke.py --setup-profile custom --timeout 90 \
  --output build/logs/desktop-qemu-custom.ppm
```

Artifacts include `desktop-qemu-setup.ppm`,
`desktop-qemu-storage.ppm`, `desktop-qemu.ppm`, and cursor images.
The custom test also captures `desktop-qemu-custom-storage.ppm`.

**Gate:** actual Rust compilation, serial log and screenshot are required
before claiming PCI discovery or GUI are operational in a VM.

## Executable host image verification

`scripts/gpt_image_verify.py` now verifies **raw file** GPT integrity
and partition ranges on WSL/Linux without a Rust compiler. It rejects
`.vmdk` and never opens disks for writing. Five tests were run locally;
see [GPT.md](GPT.md). This is not a VMware guest disk driver.

## Standalone GPT verifier added

The source now contains a pure `no_std` GPT parser with separate primary-only and strict primary-plus-backup modes in
[`src/gpt.rs`](../src/gpt.rs) with CRC32, protective-MBR, geometry, partition overlap,
backup-header/backup-array consistency and synthetic image tests. **It is not wired to any real
block device** because there is no read-only sector driver yet. See
[GPT.md](GPT.md). PCI controller discovery still does not enumerate disks.

## Next technical milestone — not yet implemented

- Enumerate *individual* AHCI, NVMe or SCSI devices via a typed,
  non-DMA/non-writing controller query interface, starting with the
  most common VMware emulated controller.
- Read stable disk identity and logical-sector capacity safely;
  distinguish disks, CD-ROMs and unsupported devices.
- Integrate the existing **primary GPT verifier** with an isolated
  read-only sector reader; independently validate backup GPT and existing
  filesystem signatures before offering any target selection.
- Only after device identity and reliable read-only probing: an
  explicitly chosen **disposable virtual disk**, detailed impact
  confirmation and an isolated storage write service.
- Never expose host disks or any destructive option in the existing
  preview OS.

References:
- https://wiki.osdev.org/PCI
- https://wiki.osdev.org/PCI_Express
- https://github.com/vmware/open-vm-tools/blob/master/open-vm-tools/lib/include/vm_device_version.h
