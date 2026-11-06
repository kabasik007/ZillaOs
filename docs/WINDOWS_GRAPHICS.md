# ZillaOS: Windows PE64 and VMware graphics — first implementation slice

## Exactly what works today

- **Windows PE32+ x86-64 inspector** in `src/compat/windows_pe.rs`:
  validates DOS/PE signatures, x64 machine type, optional header, entry RVA,
  image base/size, file/section alignment, section boundaries, import and
  relocation directory bounds. No file bytes are executed.
- **Optional host diagnostic tool** `pe-inspect` uses that SAME Rust parser,
  not a parallel implementation. No kernel mode or Wine required for inspection.
- **Passive PCI graphics discovery** in `src/video/mod.rs` and
  `src/pci_io.rs`: detect vendor `15ad` / device `0405` (VMware SVGA II),
  report the actual adapter in Settings and COM1. PCI config read-only.
- **UEFI/Limine resolution menu**: Automatic, 800x600, 1024x768,
  1280x800, 1600x900, 1920x1080, 32-bit. If a requested mode is not
  supported by VMware UEFI, firmware/bootloader picks an available mode.

### This is NOT implemented yet

- Running `.exe` (PE parsing is not a Windows loader).
- `kernel32.dll`, `ntdll.dll`, `user32.dll`, Win32 API, IAT import
  resolution, executable user-space memory mappings or process isolation.
- Switching physical resolution while the kernel is running.
- VMware SVGA FIFO setup, 2D/3D GPU acceleration, shared GPU resources.
- Persistent files in ZillaOS or importing Windows host `.exe` into its RAMFS.

These are distinct milestones. Do not infer "working Windows applications"
or "hardware acceleration active" from detection messages.

## Inspect a real Windows EXE with the common parser

On Ubuntu/WSL with the Rust 1.90.0 toolchain installed:

```bash
cd /mnt/m/projects/ZillaOs
cargo +1.90.0 run --offline --target x86_64-unknown-linux-gnu \
  --features pe-inspector-cli --bin pe-inspect -- \
  /mnt/c/Windows/System32/notepad.exe
```

For an arbitrary EXE, replace the last argument with its path visible to
WSL (e.g. `/mnt/m/Downloads/test.exe`). The tool will display image-base,
section count, subsystem and whether the PE declares imports/relocations.
It does **not** load it into memory or execute it. 32-bit x86 EXEs are
explicitly unsupported in this first PE parser.

`cargo build --release --target x86_64-unknown-none --features vm-demo`
does **not** bundle this std host utility in the boot ISO: it requires
`pe-inspector-cli` specifically.

## Runtime video architecture: no fake resolution switch

Current GUI is a Rust software renderer writing the Limine-provided
UEFI framebuffer. The static backbuffer covers up to 1280x800; larger
modes use a direct CPU renderer which can be slow.

Runtime physical mode setting on VMware requires:

1. Detect PCI device 15ad:0405, validate BAR0 I/O, verify device version
   and read limits/capability registers without altering graphics mode.
2. Map only GPU BARs into kernel virtual memory with correct cache flags,
   validate VRAM size, FIFO MMIO range and register bounds.
3. Initialise version-negotiated SVGA FIFO with a bounded command queue;
   initially issue **SVGA_CMD_UPDATE** rectangles only. Test with no 3D.
4. Implement safe `set_mode(width,height,bpp)`: validate against device
   limits, configure SVGA, read back stride/offset, reallocate framebuffers,
   recalculate Desktop/WindowManager/Pointer bounds, repaint and rollback
   if unsupported.
5. Only then add accelerated 2D commands, then independently negotiate
   SVGA3D and safe GPU surfaces. No graphics command is sent in the current
   build, so there is currently **no hardware acceleration**.

A feature flag and VM snapshots should protect any future experimental
SVGA writes until QEMU/VMware smoke tests pass.

## Architecture towards Windows process execution

```text
Files / App Center
     |
ApplicationManager
     |
ProcessManager (missing)
     |
ExecutableLoader
     |- Zilla Native ELF (missing)
     |- Windows PE32+ inspector (implemented)
     |- PE image mapper, relocations, IAT loader (missing)
     |
WindowsRuntime (missing)
     |- minimal ntdll/kernel32 (future)
     |- Win32 graphics/input/filesystem API (future)
     |
Sandboxed user process (ring3, syscalls, paging, missing)
```

Start with a tiny **x64 Windows console Hello World** compiled with
MinGW and a minimal set of `kernel32.dll` imports once the user-space
runtime is safe. Then implement more APIs iteratively. Complex Windows
applications and gaming require a much larger Windows ABI and graphics
stack.

## Source references

- Limine v12 configuration, per-entry `resolution`:
  https://github.com/Limine-Bootloader/Limine/blob/v12.x/CONFIG.md
- VMware SVGA device reference and FIFO sample code:
  https://github.com/prepare/vmware-svga/blob/master/doc/svga_interface.txt
- VMware SVGA-II concepts and PCI ports:
  https://wiki.osdev.org/VMWare_SVGA-II
- Wine documentation and Win32 API translation:
  https://www.winehq.org/?page=documentation
