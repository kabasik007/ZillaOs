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
   bash scripts/build.sh fault
   python3 scripts/smoke_test.py --expect-fault --timeout 90
   bash scripts/build.sh demo
   ```

5. Launch a **visible** virtual machine (Windows 11 WSLg, X11 or similar
   graphical Linux setup):

   ```bash
   python3 scripts/run_vm.py --display gtk
   ```

   Expected after validated Limine boot: an interactive Zilla Setup welcome wizard (press Enter seven times to accept defaults), followed by an experimental Zilla Desktop with top panel, dock and sample Files window, plus
   `ZILLAOS_CPU_READY`, `ZILLAOS_FRAME_PROBE_OK`, `ZILLAOS_MEMORY_READY`, `ZILLAOS_BOOT_OK`, `ZILLAOS_DEMO_READY`, `ZILLAOS_DESKTOP_READY` in Ubuntu terminal. Keyboard: F1 Launcher, F2 Files, F3 Terminal, F4 Settings, F5 theme, F6 minimize, F7 maximize, Tab focus, arrows move, Shift+arrows resize, Esc hide.
   **Caution:** These are expected markers, not yet observed in a verified QEMU run.
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

## Zilla Setup welcome experience

In `vm-demo`, first boot shows Welcome, Language, Region, Time Zone,
Device name, Appearance and Review. Use Enter, Escape, arrows/Tab and the
optional PS/2 mouse. Preferences are held in RAM only (no OS installation or
writing the system clock). The welcome screen reappears on every reboot.
See [SETUP.md](SETUP.md) and [VMWARE.md](VMWARE.md).

## Interactive Zilla Desktop preview

```bash
git switch main
git pull origin main
cargo test --lib --target x86_64-unknown-linux-gnu --offline
bash scripts/build.sh demo
python3 scripts/run_vm.py --display gtk
```

In the QEMU window, use **F1** Launcher, **F2** Files, **F3** Terminal,
**F4** Settings, **F5** dark/light theme, **F6** minimize, **F7** maximize,
**Tab** focus, **arrows** to move, **Shift + arrows** to resize, **Esc** to
close an overlay/window. These views are examples only: Files does not
access the disk and Terminal does not execute shell commands yet.
The current input implementation polls translated PS/2 set-1 scancodes and
tries to initialize an emulated PS/2 mouse through the i8042 AUX port. In
QEMU it should allow a visible cursor, Dock clicks, title-bar dragging and
window buttons. If the mouse is missing, keyboard shortcuts still work.
The entire GUI and mouse path **requires actual QEMU verification**.

For automated GUI verification using a disposable headless QEMU machine
with keyboard injection and a framebuffer screenshot:

```bash
python3 -m unittest discover -s tests -v
python3 scripts/desktop_smoke.py --timeout 90
```

Expected artifacts: `build/logs/desktop-qemu-setup.ppm`,
`build/logs/desktop-qemu.log`,
`build/logs/desktop-qemu.ppm`, and
`build/logs/desktop-qemu-cursor.ppm` (after pointer motion). **No successful GUI run has yet been
observed in the current execution environment.** A passing test
requires `ZILLAOS_DESKTOP_READY`, `ZILLAOS_DESKTOP_INPUT_POLL`,
keyboard-triggered `ZILLAOS_DESKTOP_STATE_CHANGED`, mouse movement and
button-edge markers, plus a
non-flat QEMU framebuffer screenshot.

## Distinct boot modes

| Mode | ISO | Guest behavior |
| --- | --- | --- |
| `test` | `dist/zillaos-0.0.2-dev-test-uefi.iso` | Print serial marker and exit QEMU with expected status 33 |
| `demo` | `dist/zillaos-0.0.2-dev-demo-uefi.iso` | Draw keyboard-controlled desktop preview, stay running |
| `fault` | `dist/zillaos-0.0.2-dev-fault-uefi.iso` | Execute UD2, report vector 6 and intentionally exit QEMU 35 |

For both modes, Limine 12.9.4 is downloaded on first use and SHA-256
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

For **VMware Workstation on Windows** rather than QEMU, use the dedicated
[VMware UEFI setup and troubleshooting](VMWARE.md).

See also: [Build](BUILD.md), [Architecture](ARCHITECTURE.md),
[Validation](VALIDATION.md).
