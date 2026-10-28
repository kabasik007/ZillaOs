# ZillaOS Installer V2.1 — FAT32 ESP and staged UEFI boot disk

**State: 2026-10-28.** Host-side Python creation and read-back tested. The
Rust kernel, Limine boot and VMware/QEMU UEFI boot **have not** been run
in the assistant's environment. This is **not** a completed OS installer.

## Achieved

Installer Lab can now, while creating a *new exclusively owned* sparse
RAW image, write all of the following:

1. Protective MBR and matching primary/backup GPT (existing feature).
2. The 512-MiB FAT32 EFI System Partition, including boot sector,
   FSInfo, backup boot sectors, **two identical FAT tables** and cluster
   allocation metadata.
3. Optional EFI files:
   - `/EFI/BOOT/BOOTX64.EFI` — the actual Limine boot application
     copied from the most recent successful ISO build.
   - `/boot/kernel.elf` — the actual ZillaOS `vm-demo` kernel ELF.
   - `/limine.conf` — staged from `boot/limine.conf`, with its
     kernel path rewritten to `boot():/boot/kernel.elf`.
4. A FAT32-compatible VFAT long-name directory record for
   `limine.conf`. This file cannot be stored literally as FAT 8.3
   because its extension is four letters; writing just a short alias
   would make Limine's config lookup unreliable.
5. Post-write independent read-back verifying GPT, FAT32 boot metadata,
   backup sector, mirrored FATs, directories, cluster chains, and
   exact contents of the staged UEFI files.

The **ZillaOS System partition remains unformatted**. There is no root
filesystem, persistent account database, in-guest disk-write service or
storage driver. No real disks are enumerated or selected by the guest.

## Security boundaries

- Default CLI mode is **dry run**. No disk/image file is created.
- Creating an image requires both `--create` and a typed
  `--confirm CREATE-NEW-VM-IMAGE` string.
- Only *new* regular `.img` files with restricted names in
  `~/.local/share/zillaos/installer-lab` are permitted.
- Uses `O_CREAT | O_EXCL | O_NOFOLLOW` on Linux; refuses existing
  images, directory symlinks, VMDK, non-POSIX hosts and device paths.
- FAT32 formatter receives ONLY the exclusive output file descriptor,
  never a disk name/path; each write is bounded by the ESP.
- The output is sparse and **can grow to its entire 12–64 GiB logical
  size** if an external application writes through all its sectors.
- Independent verification happens only after closing the writer. If
  anything fails, only the just-created output is cleaned up.
- Boot testing uses a **read-only** disk attachment, no network, no
  physical disk passthrough and a temporary OVMF variable store.

## Independent FAT32 proof (Installer V2.5)

Use [Installer Acceptance](INSTALLER_ACCEPTANCE.md) for a single command
that compiles/tests Rust, creates a **new** test RAW on explicit approval,
validates GPT+FAT32, reads the ESP with independent `mtools` and optionally
requires actual no-ISO QEMU boot evidence. Manual CI also runs `mtools`
before starting QEMU. None of these new checks has yet been observed
passing on actual boot media in the assistant environment.

## In Ubuntu / WSL2

Start by synchronizing with GitHub:

```bash
cd /mnt/m/projects/ZillaOs
git pull origin main

# Run the complete host Python suite (includes FAT32/GPT tests).
python3 -m unittest discover -s tests -v
```

### A. Safe dry run

```bash
python3 scripts/installer_lab.py --size-gib 12 --fat32
```

**Nothing is created** without `--create` and confirmation.

### B. New GPT + FAT32 disk, without EFI boot files

```bash
python3 scripts/installer_lab.py \
  --name zillaos-fat32.img --size-gib 12 --fat32 \
  --create --confirm CREATE-NEW-VM-IMAGE

python3 scripts/installer_fat32_verify.py \
  "$HOME/.local/share/zillaos/installer-lab/zillaos-fat32.img"
```

This image has GPT+FAT32 but is **not bootable** (empty ESP).

### Boot artifact preflight (Installer V2.3)

Before formatting or staging the EFI test disk, validate the **real**
Limine and kernel build outputs:

```bash
python3 scripts/check_boot_assets.py
```

This read-only command checks PE32+ x86-64 EFI application headers,
ELF64/x86-64 program headers and load segment bounds, and the exact
Limine kernel path. It rejects fake MZ/ELF files, wrong architectures
and malformed configuration. It does not imply that the kernel actually
boots in VMware; real VM runtime logs are still required. The same
audit is called by `--stage-uefi` and in GitHub Actions. Twelve new
synthetic Python tests passed locally; the full Rust/UEFI build is
still unverified.

### C. Staged UEFI disk with actual ZillaOS boot files

First, the **real Rust cross-build and UEFI ISO build must pass**:

```bash
source "$HOME/.cargo/env"
cargo test --lib --target x86_64-unknown-linux-gnu --offline
bash scripts/build_vmware.sh
```

Then create a **different** new image:

```bash
python3 scripts/installer_lab.py \
  --name zillaos-uefi.img --size-gib 12 --stage-uefi \
  --create --confirm CREATE-NEW-VM-IMAGE
```

The `--stage-uefi` option requires the **actual** files created by
`build_vmware.sh`: a PE/COFF `BOOTX64.EFI`, a Rust ELF kernel,
and a compatible Limine configuration. Incomplete/mismatched artifacts
are rejected; file contents are checked byte for byte after writing.
Do **not** use synthetic test files for a real boot attempt.

```bash
python3 scripts/installer_fat32_verify.py \
  "$HOME/.local/share/zillaos/installer-lab/zillaos-uefi.img" \
  --check-boot
```

### D. Test a UEFI disk with no ISO under QEMU

Install QEMU, OVMF and GUI support if absent:

```bash
sudo apt install -y qemu-system-x86 qemu-system-gui ovmf
bash scripts/test_uefi_disk_qemu.sh zillaos-uefi.img
```

The runner validates the GPT/FAT32 files first, then starts QEMU with
UEFI firmware, 1 CPU, 1 GiB RAM, the RAW image **read-only** and an
isolated temporary OVMF NVRAM. It passes **no ISO**, so if ZillaOS
Desktop appears, that is direct evidence of booting from the installed
EFI partition (but not yet evidence of persistence or filesystem
operation). Preserve `build/logs/installer-uefi-disk-serial.log`.

**No successful UEFI boot is claimed** until a real QEMU test log and
screen capture are available. UEFI implementation behavior, firmware
drivers and Rust boot semantics require runtime confirmation.

## Automated VMware Workstation path (preferred)

Use the new [VMware installed-disk launcher](VMWARE_INSTALLED.md). It
independently verifies the staged RAW disk and then creates a **fresh**
VMDK plus isolated UEFI VMware VMX with no CD, network, USB or shared
folders. Unlike a manual conversion, it refuses an existing destination.
Boot is not yet verified: retain COM1 evidence from the actual VM.

## VMware Workstation path

VMware Workstation generally requires a **VMDK**, not our RAW file.
Do not rename `.img` to `.vmdk`. After independent RAW validation,
a separate **new** VMDK can be made using `qemu-img convert`:

```bash
sudo apt install -y qemu-utils
mkdir -p /mnt/m/projects/ZillaOs/build/vmware-installed
RAW="$HOME/.local/share/zillaos/installer-lab/zillaos-uefi.img"
VMDK="/mnt/m/projects/ZillaOs/build/vmware-installed/zillaos-uefi-test.vmdk"
test ! -e "$VMDK" || { echo "VMDK already exists — refuse overwrite"; exit 2; }
qemu-img convert -f raw -O vmdk -S 4k "$RAW" "$VMDK"
qemu-img info "$VMDK"
```

Use **a new disposable VMware virtual machine**, not your Windows disk
or existing work VM. Choose x86-64 Other, **UEFI without Secure Boot**,
1 CPU, 1 GiB RAM, no network, no CD/DVD image, and attach that **new
VMDK as the only virtual disk**. This avoids exposing any host physical
drive to the experimental kernel.

VMware boot has **not** been verified; if it fails, check VMware
`vmware.log` and COM1 serial output. A failed boot does not prove the
GPT was not written; the kernel itself still needs cross-compilation
and validation. See [VMWARE.md](VMWARE.md).

## Boot-evidence diagnostics (V2.4)

The tightened PE32+ and ELF64 preflight checks require the EFI/ELF entry
point to lie inside an executable mapped region, and validate segment
alignment/bounds before any test disk is staged. After QEMU boots, inspect
COM1 with `python3 scripts/boot_evidence.py <serial-log-path>`; a panic,
missing Setup or unavailable framebuffer yields a failing JSON verdict.
Repeat QEMU runs preserve prior logs under unique names. See
[BOOT_DIAGNOSTICS.md](BOOT_DIAGNOSTICS.md). Actual VM runtime remains
unverified.

## Next milestones

1. Run `cargo fmt`, `cargo test`, the ISO build and QEMU boot in
   actual WSL2; repair Rust errors before feature expansion.
2. Prove the staged image is UEFI-bootable **without** an ISO in QEMU.
3. Prove boot via VMDK in VMware Workstation.
4. Add simple persistent settings and an independently verified root
   filesystem in a **new** lab image, not an existing Windows disk.
5. After a safe read-only guest block driver exists, let the GUI list
   verified virtual disks and show a data-loss impact plan.
6. Only then implement a transactional in-guest installer for explicitly
   selected disposable virtual disks with rollback and failure tests.

References: [Installer V2](INSTALLER_V2.md), [GPT](GPT.md),
[Storage](STORAGE.md), [Setup](SETUP.md).
