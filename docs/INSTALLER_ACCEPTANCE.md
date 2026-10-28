# ZillaOS Installer V2.5 — independent FAT32 and one-command acceptance

**Status: source committed to `main` on 2026-10-28.**
The new scripts and tests have NOT completed a real mtools/QEMU/VMware
run in the assistant environment. They are future acceptance gates, not
evidence that the guest installer already works.

## Why another verifier?

The existing GPT+FAT32 Python checker is written by the same project
as the FAT32 writer. Bugs can agree with each other. The new
`scripts/verify_esp_mtools.sh` asks a **separate FAT32 implementation**
(mtools, available in Ubuntu) to list and extract the UEFI files.
It then compares their bytes with the pinned Limine/ELF/config artifacts
prepared by `load_uefi_assets()`.

This tests real FAT32 compatibility rather than simply replaying
our own parsing logic. The script never mounts a device or writes to
the source RAW file. It only reads the restricted Installer Lab image
under `~/.local/share/zillaos/installer-lab/` and writes temporary
extracted copies into a fresh directory.

### Install requirements

```bash
sudo apt update
sudo apt install -y mtools qemu-system-x86 ovmf xorriso
```

You still need Rust 1.90, bare-metal target, built Limine and the
actual ZillaOS demo ELF to run the full acceptance path.

## Option 1: safe source/ISO preflight (no new disk image)

```bash
cd /mnt/m/projects/ZillaOs
git pull origin main
source "$HOME/.cargo/env"

bash scripts/installer_acceptance.sh --preflight
```

This invokes `build_vmware.sh`, including Python/Rust host tests and
the ISO build, then a strict PE32+/ELF64/Limine audit.
It **does write ordinary project build output and test logs** but does
not create a new RAW/VMDK disk image.

## Option 2: create and verify a NEW disposable RAW disk

This requires the exact confirmation argument:

```bash
bash scripts/installer_acceptance.sh --create CREATE-NEW-VM-IMAGE
```

It performs preflight first; only if it succeeds does it create a new
12-GiB sparse image with GPT+FAT32, stage the actual Limine/kernel
files, verify its bytes with the project reader, then use independent
`mtools` to read back all three staged files.

Each invocation uses a new random image name. The underlying lab
writer uses `O_EXCL`, so **existing images cannot be overwritten**.
The resulting image is inside Ubuntu HOME, never a physical Windows
disk or an existing VMware VM.

## Option 3: also require a no-ISO UEFI boot under QEMU

```bash
bash scripts/installer_acceptance.sh --boot CREATE-NEW-VM-IMAGE
```

After the other checks, the runner starts the **headless graphical QEMU
smoke test directly from the staged RAW disk** (read-only, no ISO/network,
temporary OVMF). It drives all **nine Setup steps**, checks the Desktop,
window manager, keyboard and screenshots, and then parses the **COM1 file
written by that exact run**. Logs and PPM screenshots use unique per-run
names. `scripts/boot_evidence.py` additionally rejects panic, wrong event
order, duplicate boot markers, missing framebuffer and missing Setup input
loop.

The command only succeeds if the GUI, screenshots and COM1 startup evidence pass. An
expiration/timeout is not by itself treated as proof of success.

## Individual independent FAT32 check

After creating a new staged image with an approved name:

```bash
bash scripts/verify_esp_mtools.sh zillaos-uefi.img
```

This prints the FAT32 root, `EFI/BOOT` and `BOOT` directories,
extracts `BOOTX64.EFI`, `KERNEL.ELF`, `LIMINE.CONF`, compares each
with the actual build files, and prints `ZILLAOS_MTOOLS_ESP_VERIFIED`
only on success.

## Logs and release gates

All run reports are kept under `build/logs/`:
- `installer-acceptance-build.log`
- `installer-acceptance-boot-assets.json`
- `installer-acceptance-stage.json`
- `installer-acceptance-esp.json`
- `installer-acceptance-mtools.log`
- `installer-acceptance-qemu.log`
- `installer-acceptance-boot-evidence.json`

The manual GitHub Actions `boot-smoke.yml` workflow has been amended
to install mtools and run this independent ESP test before its
no-ISO QEMU check. Its outcome is **not known** until the workflow
has actually executed.

**Not delivered yet:** guest storage driver, real disk enumeration,
root filesystem, persistent settings, rollback, or a completed in-guest
installer. See [Storage](STORAGE.md), [Installer V2.1](EFI_BOOT_DISK.md)
and [VMware installed-disk instructions](VMWARE_INSTALLED.md).

Keep VMware physical-disk passthrough disabled and use only a new
disposable virtual disk for all tests.
