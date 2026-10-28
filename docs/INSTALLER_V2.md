# ZillaOS Installer V2 — ambitious, safe implementation track

**Date:** 2026-10-28. **Status:** partially implemented in `main`.
This is a real developer laboratory for creating and independently
verifying *test GPT disk images*, **not yet a complete installed OS**.

## What is genuinely functional

1. **Native first-run GUI**: 9 pages, including the read-only Storage
   controller inventory, the new **Install Plan** prerequisites screen
   and final Review. Installation writes remain disabled.
2. **Kernel partition planner**: pure `no_std` arithmetic for EFI and
   system partition sizes, disk boundaries and 512/4096-byte sectors.
3. **Kernel GPT verifier**: primary + backup headers and arrays,
   CRCs, geometric boundaries and overlap checks; still host-test
   source only, because a guest block driver is absent.
4. **Host RAW GPT verifier**: `scripts/gpt_image_verify.py`, used
   independently after test-image creation.
5. **Installer Lab**: `scripts/installer_lab.py`, an actual GPT
   writer **exclusively into a newly created regular test image**,
   never to an existing file, device, VMDK or Windows host disk.
   Tests create 12-GiB *sparse files* and verify their primary and
   backup GPT with the independent reader.

## Boot evidence and executable entry checks (V2.4)

`boot_assets_audit.py` now checks mapped executable entrypoints for PE32+
and ELF64, while `boot_evidence.py` validates actual COM1 markers from
one QEMU/VMware boot log. See [BOOT_DIAGNOSTICS.md](BOOT_DIAGNOSTICS.md).
This improves detection; it does not establish that ZillaOS already boots.

## UEFI boot artifact preflight (V2.3)

`python3 scripts/check_boot_assets.py` now verifies compiled Limine
PE32+ x86-64 EFI metadata, Rust ELF64/x86-64 kernel load segments,
and the exact Limine config path. `--stage-uefi` cannot create an
image from malformed or wrong-architecture boot assets. Twelve
synthetic unit tests passed locally, but real boot verification
remains an independent requirement.

## Installer Lab stages

```text
                 DRY-RUN [default]
                       |
     proposed 12-GiB layout, JSON report
                       |
     --create + typed confirmation only
                       |
   new O_EXCL raw file in WSL home sandbox
                       |
   sparse resize / protective MBR / GPT
                       |
   EFI partition (512 MiB) + ZillaOS system
                       |
   backup entries + GPT backup header
                       |
   fsync -> close -> independent GPT verifier
                       |
       report GUID, boundaries, success
```

**Installer V2.1:** FAT32 ESP formatting and staging real Limine EFI/
kernel/config build artifacts are now implemented for a *new* raw image.
The experimental ZillaOS root partition remains unformatted and the actual
UEFI no-ISO boot is still **unverified**. See [EFI_BOOT_DISK.md](EFI_BOOT_DISK.md). A simulator is not the real guest installer.

### Default partition proposal (12 GiB, 512-byte sectors)

| Area | Geometry |
| --- | --- |
| Protective MBR and primary GPT reserve | first 1 MiB |
| EFI System Partition | 512 MiB, starts at LBA 2048 |
| ZillaOS System Partition | remaining bytes until final 1-MiB guard |
| Backup GPT table and header | last sectors of the disk |

Each partition receives a unique GUID. The ESP uses the standard UEFI
partition type GUID; the experimental ZillaOS system partition uses
a ZillaOS-specific type GUID. Neither partition is formatted.

## Isolated VMware boot from installed VMDK

A [new Windows PowerShell launcher](VMWARE_INSTALLED.md) is now available.
It invokes a read-only RAW verifier in WSL, exports a **new** disposable
VMDK and generates a safe UEFI VMware VMX with no ISO. This closes the
manual VM-preparation workflow, but **not** the guest boot proof gate.

## How to use safely on your Windows + WSL2 setup

In **Ubuntu/WSL**:

```bash
cd /mnt/m/projects/ZillaOs
git pull origin main

# 1. No changes to any file; prints the planned disk layout:
python3 scripts/installer_lab.py --size-gib 12 --sector-size 512

# 2. Run all Python regression tests (including safe image creation):
python3 -m unittest discover -s tests -v

# 3. Optional: explicitly create ONE new sparse lab file in Ubuntu HOME.
#    This does NOT happen during the dry run or automated builds:
python3 scripts/installer_lab.py --name zillaos-lab.img \
  --size-gib 12 --create --confirm CREATE-NEW-VM-IMAGE

# 4. Independently inspect the image without modifications:
python3 scripts/gpt_image_verify.py \
  "$HOME/.local/share/zillaos/installer-lab/zillaos-lab.img" \
  --sector-size 512
```

Expected created file:
`~/.local/share/zillaos/installer-lab/zillaos-lab.img`.

**It is intentionally NOT created in `M:\projects`**, to avoid
the uncertain sparse-file allocation behavior of Windows-mounted
DrvFs/NTFS volumes. Even on WSL ext4, a 12-GiB sparse file can grow to
12 GiB if another tool fills it, so check disk capacity first.

`installer_lab.py` refuses existing paths (O_EXCL), unsafe
filenames, unsupported disk sizes, missing typed confirmation,
symlinked lab directories/ancestors and non-POSIX execution. It uses
only a controlled output directory below your HOME and never opens
system block devices. It never deletes an existing image.

### Run the kernel tests separately

```bash
source "$HOME/.cargo/env"
cargo test --lib --target x86_64-unknown-linux-gnu --offline
bash scripts/build_vmware.sh
```

The Rust compiler and actual VMware emulator are **not available in
the assistant runtime**. Do not treat Python host tests as proof of
Rust compilation or VM boot.

## Verification evidence

The GPT image lab code and seven test cases were run locally in Python
on 2026-10-28 (together with the five existing GPT image verification
tests): **12/12 tests passed**. The exact Git blob SHAs of the committed
lab code and test file were checked against the local copies.
This validates only the Python host-path operations on temporary
ordinary files.

## Release gates: toward a genuinely bootable installed ZillaOS

| Milestone | Status | Required proof |
| --- | --- | --- |
| Host read-only disk integrity | Implemented | 12 Python unit tests |
| Create clean dual-GPT raw image | Implemented | Independent primary+backup GPT readback |
| Native graphical install plan | Source committed | QEMU/VMware screenshot pending |
| Rust kernel and setup UI build | Unverified | Rust compile, host tests and UEFI boot |
| VMware disk model/capacity detection | Missing | AHCI/SCSI/NVMe read-only identification |
| Read existing virtual disk GPT safely | Missing | Guest block driver + strict GPT verifier |
| Format ESP FAT32 in fresh test image | Python code + tests passed | Independent FSInfo/FAT/boot sector read-back |
| Stage EFI loader + kernel to ESP | Python code + tests passed | Byte-for-byte readback, actual QEMU boot pending |
| Root filesystem + settings persistence | Missing | Reboot retaining locale, hostname, timezone |
| In-guest disk confirmation UI | Missing | Explicit disk identity typed confirmation |
| Real installer transactions and recovery | Missing | Fail injection, rollback, fresh VM image only |

**Never install directly to a physical Windows or Linux disk at this
phase. Never auto-select the first disk.** On initial VMware tests,
keep no disk attached; the test lab raw file isn't a valid OS disk yet.

Related: [SETUP.md](SETUP.md), [STORAGE.md](STORAGE.md),
[GPT.md](GPT.md), [VMWARE.md](VMWARE.md), [INSTALLATION.md](INSTALLATION.md).
