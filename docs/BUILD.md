# Build / run ZillaOS v0.0.2-dev

**Status:** code committed; runtime build and QEMU boot require a suitable toolchain and **have not been independently verified** merely by committing source files.

## Requirements (Linux host or WSL2)

- Rust via rustup; repo pins **1.90.0** with `x86_64-unknown-none` target.
- `curl`, `sha256sum`, `tar`, `xorriso`, `python3`, `qemu-system-x86_64`, and OVMF firmware.
- Debian/Ubuntu: `sudo apt install -y xorriso qemu-system-x86 ovmf curl` (and `rustup` installed separately).
- Internet **only for first download** of pinned Limine 12.9.4; the kernel has no crates.io dependencies.

### Build

```bash
git clone https://github.com/kabasik007/ZillaOs.git
cd ZillaOs
bash scripts/build.sh
```

For a readable graphical QEMU window instead of an auto-exiting smoke test:

```bash
bash scripts/build.sh demo
python3 scripts/run_vm.py --display gtk
```

The demo first opens the new interactive Zilla Setup wizard (nine pages,
RAM-only settings, no disk installation). After completing Setup, it shows
source for an experimental directly rendered Zilla Desktop: top panel, Dock, sample app window and keyboard actions (F1/F2/F3/F4/F5/Tab/Esc). It stays running until the VM is stopped. It is **not** a real user-space compositor or functional file manager; appearance and input await QEMU verification. The test build is created with
`bash scripts/build.sh test` and generates
`dist/zillaos-0.0.2-dev-test-uefi.iso`; the demo image is
`dist/zillaos-0.0.2-dev-demo-uefi.iso`.

The build script checks the downloaded Limine binary archive against SHA-256
`9880cc04681262214db9b824ed8a5c55296955027d8caa9f55ab52a6dd85d6a0`
before unpacking it, compiles the bare-metal Rust kernel, and creates
`dist/zillaos-0.0.2-dev-test-uefi.iso` (or `-demo-uefi.iso`) with Limine's UEFI CD boot image.

### Run both normal boot and controlled CPU fault tests

```bash
bash scripts/build.sh test
python3 scripts/smoke_test.py --timeout 90
bash scripts/build.sh fault
python3 scripts/smoke_test.py --expect-fault --timeout 90
```

The `fault` image deliberately executes UD2; passing means CPU exception vector 6 is reported and QEMU exits 35. See [EXCEPTIONS.md](EXCEPTIONS.md).

### Run the controlled boot smoke test

```bash
cargo test --lib --target x86_64-unknown-linux-gnu --offline
python3 scripts/smoke_test.py --timeout 90
```

Expected serial markers:

```text
ZillaOS v0.0.2-dev | x86-64 | Limine
ZILLAOS_CPU_PHYSICAL_BITS=0x...
ZILLAOS_CPU_VIRTUAL_BITS=0x...
ZILLAOS_CPU_READY
ZILLAOS_MEMMAP_REGIONS=0x...
ZILLAOS_USABLE_FRAMES=0x...
ZILLAOS_TEST_FRAME_1=0x...
ZILLAOS_TEST_FRAME_2=0x...
ZILLAOS_HHDM_OFFSET=0x...
ZILLAOS_FRAME_PROBE_OK
ZILLAOS_MEMORY_READY
ZILLAOS_BOOT_OK
```

Host-only tests (run even before Rust or QEMU are installed):

```bash
python3 -m unittest discover -s tests -v
python3 scripts/doctor.py
```

The test requires CPU, HHDM and frame-probe evidence, the memory/boot readiness markers and QEMU's debug-exit status 33;
fail markers, hangs and missing image files must cause test failure.
OVMF variable storage is copied to a new temporary directory for each run.

### VMware Workstation on Windows

Use `bash scripts/build_vmware.sh` from WSL Ubuntu to run host tests,
build the `demo` ISO and inspect UEFI boot metadata. Use
`scripts/vmware.ps1` from **Windows PowerShell** to generate and open
a disposable 1-vCPU UEFI VMware Workstation VM with its virtual CD/DVD
pointing to that ISO. The VM has no virtual disk or network. See
[VMWARE.md](VMWARE.md) for exact commands, COM1 logs and troubleshooting.
Neither VMware boot nor graphical behavior is verified yet.

### Windows

For initial development use Ubuntu under **WSL2** for ISO build scripting; see [Windows + Linux VM Quickstart](VM.md).
QEMU + OVMF can run on Windows or under a supported Linux environment.
Do **not** write boot images to actual physical disks. The current kernel
is a QEMU lab artifact; debug-exit I/O port `0xf4` is test-specific.

### GitHub Actions

`.github/workflows/boot-smoke.yml` runs only when manually dispatched, to
avoid unexpectedly consuming GitHub minutes. The workflow uses a clean
Ubuntu runner, builds the ISO and checks QEMU exit/serial markers. A published
workflow file is **not** proof of a successful run.

### Troubleshooting

- `Missing required tool: xorriso` — install host dependency.
- Limine SHA-256 mismatch — discard cached archive; never suppress the check.
- No OVMF_CODE/VARS — install `ovmf` or pass `--ovmf-code` and `--ovmf-vars`.
- Rust compilation errors — report exact compiler output and version.
- QEMU timeout — inspect console output and generated ELF with `readelf -l`.
- CPU reset/triple fault — debug with QEMU/GDB; don't infer success from ISO creation.

### Inputs and provenance

Official [Limine releases](https://github.com/Limine-Bootloader/Limine/releases) and
[protocol](https://github.com/limine-bootloader/limine-protocol/blob/trunk/PROTOCOL.md).
The bootloader is external to ZillaOS; we do not vendor its code here.

## Completion gate

This is **M1 in progress**, not verified. Mark it Verified only after a clean
build, successful QEMU run, negative test and stored build/test evidence.
