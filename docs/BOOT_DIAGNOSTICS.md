# ZillaOS boot diagnostics — Installer V2.4

Status (2026-10-28): source code committed on `main`. The Python checks
have been executed locally on synthetic boot artifacts/COM1 logs. The full
Rust kernel build and physical VMware/QEMU boot have **not** been verified
in this environment.

## New preflight hardening

`scripts/boot_assets_audit.py` now verifies:
- PE32+ EFI loader: x86-64, EFI Application subsystem, file-backed section
  bounds, mapped section range and entry RVA inside an **executable** section.
- ELF64 x86-64 kernel: valid PT_LOAD file offsets/sizes, alignment,
  bounded virtual addresses, and entry address inside an **executable**
  load segment.
- Limine configuration points at `boot():/boot/kernel.elf`.

It is not a cryptographic signature checker or proof of firmware behavior.
All these checks run before staging a **new** Installer Lab disk image.

## One-command COM1 analysis

In WSL Ubuntu, after a test boot:

```bash
cd /mnt/m/projects/ZillaOs
git pull origin main

python3 scripts/boot_evidence.py \
  build/logs/installer-uefi-disk-serial.log
```

Output is a JSON object including:
- `success`: did the required kernel and graphical Setup markers appear?
- `classification`: a grounded failure reason or `boot_evidence_valid`.
- `kernel_boot_seen`, `setup_seen`, `desktop_seen`.
- `fatal_marker` and `event_count`.

The verifier now requires **unique, correctly ordered** boot, PCI scan,
Setup, framebuffer and Setup input-loop markers. It rejects panic,
explicit boot failure, missing GUI proof, duplicate markers and incorrect
startup ordering. A bare `ZILLAOS_BOOT_OK` is insufficient. It does **not** claim that files were installed, that
settings persist or that disk drivers exist.

The QEMU runner now retains earlier serial logs by using a unique new
log filename for subsequent boots. Use the path printed by the runner
when analysing a repeat run, rather than relying on an old log.

GitHub Actions writes a JSON result at
`build/logs/installer-uefi-boot-evidence.json` and archives it with
the serial log. This CI workflow is **manual** and has not yet been
confirmed to run to completion.

## What to do with an error

- `kernel_boot_marker_missing`: inspect UEFI/OVMF loader, Limine
  configuration, kernel ELF and serial-port initialization.
- `setup_marker_missing`: kernel booted, but graphical onboarding
  was not reached; inspect kernel demo feature and Setup entry.
- `storage_scan_marker_missing`: check initialization order or guest
  PCI header scanning.
- `setup_framebuffer_unavailable`: inspect the UEFI GOP framebuffer,
  format and SVGA configuration.
- `guest_zillaos_panic` / `guest_zillaos_boot_fail`: inspect the
  immediately preceding COM1 output, then repair the kernel fault.

Never discard a failing log to make the test green; keep the full
`vmware.log`, the COM1 log and the latest QEMU execution report.

## No-ISO graphical installer evidence

`python3 scripts/desktop_smoke.py --disk <lab-raw-path> --timeout 120`
now boots an independently verified read-only Installer Lab RAW *without*
ISO and automatically presses Enter through all nine first-run screens.
It captures Setup, Storage, Install Plan and Desktop images and checks
actual keyboard-driven Desktop interaction. See
[INSTALLER_ACCEPTANCE.md](INSTALLER_ACCEPTANCE.md). This path must still
be run in QEMU/WSL before its success can be claimed.

## Next acceptance sequence

```bash
cd /mnt/m/projects/ZillaOs
git pull origin main
python3 -m unittest discover -s tests -v
source "$HOME/.cargo/env"
cargo test --lib --target x86_64-unknown-linux-gnu --offline
bash scripts/build_vmware.sh
python3 scripts/check_boot_assets.py
```

Only if the build succeeds, create a **new** disposable RAW and run the
read-only QEMU test described in [EFI_BOOT_DISK.md](EFI_BOOT_DISK.md).
After successful QEMU evidence, use the isolated
[VMware installed-disk flow](VMWARE_INSTALLED.md).

No physical host disks are accessible through these workflows. A real
in-guest installer is still blocked until safe block devices, explicit
selection, persistent filesystem and recovery exist.
