# ZillaOS — local VM quickstart (Windows + Ubuntu/WSL2)

ZillaOS v0.0.2-dev is a **bare-metal Rust kernel**, not a Linux application.
Use **QEMU with OVMF firmware** as a disposable x86-64 virtual machine. The
bootable ISO stays in `dist/` and is attached as read-only CD media;
the script never maps the host disk into the guest.

**Current verification:** no successful QEMU boot or Rust compile has been
observed as of 2026-10-19. The following instructions are a reproducible
procedure and must not be mistaken for a passing test report.

## On Windows 11

1. Open PowerShell as Administrator and install WSL2 + Ubuntu if not installed:

   ```powershell
   wsl --install -d Ubuntu
   ```

   A Windows restart may be required. Follow the Ubuntu first-run setup.
   A modern WSLg-enabled Windows setup is required for QEMU's GTK window.
   If no graphical display is available, use the headless smoke test.

2. In Ubuntu (WSL terminal), install Linux dependencies:

   ```bash
   sudo apt update
   sudo apt install -y git curl ca-certificates xorriso qemu-system-x86 qemu-system-gui ovmf python3
   ```

3. Install Rust from the [official rustup instructions](https://rustup.rs/).
   Avoid running unfamiliar third-party setup scripts with elevated privileges.
   Our `rust-toolchain.toml` pins Rust 1.90.0 and its bare-metal target.
   After rustup is installed, open a new terminal or source `~/.cargo/env`.

4. Get source, run the environment diagnostic, and build both images:

   ```bash
   git clone https://github.com/kabasik007/ZillaOs.git
   cd ZillaOs
   python3 scripts/doctor.py
   bash scripts/build.sh test
   cargo test --lib --target x86_64-unknown-linux-gnu --offline
   python3 -m unittest discover -s tests -v
   python3 scripts/smoke_test.py --timeout 90
   bash scripts/build.sh demo
   ```

5. Launch a **visible** virtual machine (Windows 11 WSLg, X11 or similar
   graphical Linux setup):

   ```bash
   python3 scripts/run_vm.py --display gtk
   ```

   Expected after validated Limine boot: ZillaOS splash on framebuffer and
   `ZILLAOS_CPU_READY`, `ZILLAOS_FRAME_PROBE_OK`, `ZILLAOS_MEMORY_READY`, `ZILLAOS_BOOT_OK`, `ZILLAOS_DEMO_READY` in Ubuntu terminal.
   Stop with Ctrl+C or by closing the QEMU window.
   **Demo mode intentionally does not exit automatically.** It is only a
   proof-of-boot screen, not a desktop or login system.

## On native Ubuntu / Debian

Follow steps 2–5 directly, without WSL. On remote Linux servers with no GUI:

```bash
bash scripts/build.sh test
python3 scripts/smoke_test.py --timeout 90
```

This is the recommended deployment-free test. QEMU uses software CPU
emulation (`accel=tcg`), so no KVM or nested virtualization is required.

## Distinct boot modes

| Mode | ISO | Guest behavior |
| --- | --- | --- |
| `test` | `dist/zillaos-0.0.2-dev-test-uefi.iso` | Print serial marker and exit QEMU with expected status 33 |
| `demo` | `dist/zillaos-0.0.2-dev-demo-uefi.iso` | Draw framebuffer logo, print serial marker, stay running |

For both modes, Limine 12.9.3 is downloaded on first use and SHA-256
verified against the value supplied in GitHub's official release page.
Rust has no crates.io dependencies at M1.

## Troubleshooting

- **`DISPLAY is not set` / GTK missing:** install `qemu-system-gui`,
  verify WSLg, or use the headless test mode instead.
- **OVMF pair not found:** install `ovmf`; `doctor.py` checks known paths.
- **Rust target missing:** `rustup target add x86_64-unknown-none --toolchain 1.90.0`.
- **Blank VM:** check serial output for `ZILLAOS_BOOT_FAIL`, dump ELF
  segments via `readelf -l target/x86_64-unknown-none/release/zillaos-kernel`,
  and inspect QEMU error output.
- **QEMU timeout:** test **fails**, even if firmware or Limine displayed.
- **No framebuffer:** serial test can still pass, but graphical demo is
  limited until an RGB32 framebuffer is exposed.

## Safety

Only disposable VM firmware variables are writable. QEMU attaches the ISO
as CD-ROM; no `/dev/sd*`, raw host disk, bridged network, SSH key, or
personal file share is added to the guest. Running untested kernels on
physical hardware is **not** part of the pre-release M1/M2 development phase.

See also: [Build](BUILD.md), [Architecture](ARCHITECTURE.md),
[Validation](VALIDATION.md).
