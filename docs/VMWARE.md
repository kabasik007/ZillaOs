# ZillaOS on VMware Workstation (Windows + WSL2)

**Status:** VMware launch profile prepared in `main` (2026-10-27).
Rust cross-build, ISO boot and graphical operation on VMware are **NOT YET VERIFIED**.
This is a bare-metal UEFI kernel experiment, not a Windows installer.

## Target configuration

- Host: Windows 10/11 with VMware Workstation Pro; Ubuntu in WSL2 builds
  ZillaOS source into an ISO stored on the same Windows drive.
- Firmware: **UEFI**, with **Secure Boot disabled** (the custom Limine
  loader is not signed for the VM's default Secure Boot trust store).
- Guest OS selection: **Other / Other 64-bit**.
- vCPUs: **1** (bootstrap is not SMP-safe yet).
- RAM: **1024 MiB** (512 MiB may suffice; 1 GiB leaves margin for GUI).
- CD/DVD: the `demo` ISO, connected at power-on as virtual CD-ROM.
- Display: basic VMware SVGA, **3D acceleration disabled**. The kernel
  requests a generic UEFI GOP framebuffer from Limine, *not* SVGA3D.
- Virtual disks: **NONE**. Files demo has no real storage backend.
- Network: **NONE**. ZillaOS has no networking driver or firewall yet.
- USB passthrough/shared folders: **NONE**.
- Input: request legacy PS/2 keyboard and mouse; fallback keyboard-only
  if VMware's virtual input controller differs from QEMU's. Not validated.
- Serial debug: guest COM1 (I/O `0x3f8`) written to
  `build\vmware\ZillaOS\zillaos-com1.log` on the Windows host.

This setup uses only a read-only ISO. It **never writes to a physical
disk**. The `build\vmware` directory is ignored by Git and contains
the VMware configuration, its firmware state and runtime logs.

## A. Build UEFI ISO in Ubuntu / WSL2

On Windows PowerShell, enter Ubuntu:

```powershell
wsl -d Ubuntu
```

Then **in Ubuntu**:

```bash
cd /mnt/m/projects/ZillaOs
git switch main
git pull origin main

sudo apt update
sudo apt install -y git curl ca-certificates xorriso python3 build-essential

# Install Rust from the official rustup installer IF rustc/cargo are absent:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustup toolchain install 1.90.0 --profile minimal --component rustfmt
rustup target add x86_64-unknown-none --toolchain 1.90.0

# Run Python tests, Rust tests, build ISO and inspect El Torito boot metadata:
bash scripts/build_vmware.sh
```

If rustup is already installed, do not reinstall it; run
`source "$HOME/.cargo/env"` and check `cargo --version`.

Expected output ISO:

```text
/mnt/m/projects/ZillaOs/dist/zillaos-0.0.2-dev-demo-uefi.iso
M:\projects\ZillaOs\dist\zillaos-0.0.2-dev-demo-uefi.iso
```

The second line is the Windows path to attach to VMware. WSL's `/mnt/m`
maps to Windows `M:\`. If this clone is on a different drive, adapt
the path. The build must pass; **do not** create an empty or renamed ISO
to work around an error. Limine downloads are verified by a SHA-256 pin.

## B. Generate/open VMware test machine in Windows PowerShell

Exit Ubuntu (`exit`) or open a separate **Windows PowerShell** window:

```powershell
cd M:\projects\ZillaOs
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\vmware.ps1 -Launch
```

This script checks that the ISO exists, then creates:

```text
M:\projects\ZillaOs\build\vmware\ZillaOS\ZillaOS.vmx
M:\projects\ZillaOs\build\vmware\ZillaOS\zillaos-com1.log   (created on guest output)
```

If VMware does not open automatically: **File > Open > ZillaOS.vmx**,
then click **Power on this virtual machine**.

One-step alternative **after Rust is installed in Ubuntu**:

```powershell
cd M:\projects\ZillaOs
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\vmware.ps1 -Build -Launch
```

`-Build` invokes the WSL Ubuntu build first. If the VMX already exists,
the script preserves its configuration and virtual firmware settings;
use `-Force` only when the VM is **powered off** and the profile truly
needs to be recreated. Never overwrite a running VMX.

## C. Manual VMware creation (if the .vmx profile is not accepted)

1. VMware Workstation > **File > New Virtual Machine** > **Custom**.
2. Choose **I will install the operating system later**.
3. Guest type **Other**, version **Other 64-bit** (not Windows/Linux).
4. Firmware **UEFI**, disable **Secure Boot**. If this isn't shown during
   creation: VM Settings > Options > Advanced > Firmware type = UEFI,
   Secure Boot unchecked.
5. 1 processor, 1 core, 1024 MB RAM.
6. Network connection: **Do not use a network connection**, or remove
   the network adapter in VM settings.
7. Do not create/use a hard disk. If the wizard created one, remove the
   virtual hard disk from this *new test VM only* before powering on.
8. Add **CD/DVD (SATA)**, set **Use ISO image file** to
   `M:\projects\ZillaOs\dist\zillaos-0.0.2-dev-demo-uefi.iso`.
   Enable **Connect at power on**.
9. Display: leave a basic virtual graphics adapter; disable
   **Accelerate 3D graphics**.
10. Optionally add **Serial Port > Output to file** to capture COM1
    output; use a path inside this disposable VM directory.
11. Start VM; if an EFI boot manager appears, select its virtual CD/DVD.

VMware menu labels vary by Workstation version. UEFI virtual hardware
and CD-ROM boot options are documented by Broadcom. The ISO uses
Limine's UEFI El Torito boot image.

## First-launch Zilla Setup screen

The `demo` ISO now displays an **interactive setup wizard before the desktop**.
Press **Enter seven times** to accept defaults and reach Desktop, or use
arrows/Tab to change selections, Esc to go back, and Backspace/ASCII typing
on the device-name screen. Supported steps: Welcome, Language, Region,
Time Zone, PC name, Theme, Review. Default: Ukrainian, Ukraine,
Europe/Kyiv, `zilla-pc`, dark mode.
The prototype **does not install ZillaOS to a disk**. All values are
RAM-only and the wizard runs again after each VM reboot. For a real
installer architecture see [SETUP.md](SETUP.md).

## D. What we should see

**Potential startup sequence** (expected, not observed on VMware):

```text
VMware UEFI firmware
       |
Limine menu / ELF kernel
       |
ZillaOS v0.0.2-dev
ZILLAOS_IDT_READY
ZILLAOS_CPU_READY
ZILLAOS_FRAME_PROBE_OK
ZILLAOS_MEMORY_READY
ZILLAOS_BOOT_OK
ZILLAOS_DEMO_READY
ZILLAOS_SETUP_READY
# Seven interactive setup screens, then:
ZILLAOS_SETUP_COMPLETE
ZILLAOS_SETUP_VOLATILE_ONLY
ZILLAOS_DESKTOP_READY
ZILLAOS_WINDOW_MANAGER_READY
       |
Zilla Desktop: wallpaper, top bar, Dock, mock Files window
```

Try **F1** Launcher, **F2** Files, **F3** Terminal, **F4** Settings,
**F5** change theme, **F6** minimize, **F7** maximize, **Tab** change
focused window, arrows move, Shift+arrows resize, Esc close.

The VMware virtual mouse may not match the QEMU PS/2 driver. If mouse
does not work but keys do, this is a driver compatibility issue, **not**
proof of a broken display. Do not expect functional disk I/O, shell,
guest filesystem, VMware Tools, graphics acceleration or Win32 apps.

## E. Diagnose a black screen or immediate shutdown

**First**, read Windows COM1 log:

```powershell
Get-Content "M:\projects\ZillaOs\build\vmware\ZillaOS\zillaos-com1.log" -Tail 100
```

If no serial log exists, check VMware VM Settings > Serial Port and
whether the guest has reached `serial::init()`. VMware's own log lives
alongside the VMX as `vmware.log`.

**Common symptoms:**
- **No ISO/Operating system not found:** no built ISO attached, firmware
  type wrong, CD/DVD disconnected or ISO lacks boot metadata. Rerun
  `bash scripts/build_vmware.sh`; ensure UEFI, not legacy BIOS.
- **Security Violation:** disable UEFI Secure Boot.
- **Limine appears, then black screen:** check COM1 for kernel errors,
  `ZILLAOS_FRAMEBUFFER_UNAVAILABLE` or `ZILLAOS_BOOT_FAIL`. The
  framebuffer format/resolution may differ from QEMU.
- **Keyboard works, mouse does not:** VMware input path may be USB HID
  or a differently configured PS/2 controller; ZillaOS driver supports
  only translated PS/2 keyboard and a standard PS/2 mouse.
- **VM hangs in test mode:** do not attach the `test` or `fault`
  ISO; the QEMU-only debug exit port `0xf4` has no equivalent expected
  on VMware. Always attach `demo` ISO.
- **VMX invalid hardware version:** create the VM manually using the
  matching hardware compatibility level for the installed Workstation.

Please keep the VM **disconnected from the network and host disks**.
Only run this development image as a disposable virtual machine.

## References

- [VMware Workstation: create a VM (Broadcom)](https://knowledge.broadcom.com/external/article?legacyId=1018415)
- [VMware Workstation EFI notes (Broadcom)](https://community.broadcom.com/vmware-cloud-foundation/viewdocument/using-efiuefi-firmware-in-a-vmware)
- [Limine UEFI/hybrid ISO usage](https://github.com/Limine-Bootloader/Limine/blob/v12.x/USAGE.md)
- [ZillaOS build instructions](BUILD.md)
