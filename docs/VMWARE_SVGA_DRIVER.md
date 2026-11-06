# ZillaOS VMware SVGA II — Stage 1

The new driver is **read-only discovery only**. The active display stays the Limine/UEFI GOP RGB32 software renderer. This stage **does not** switch resolution, initialize FIFO, accelerate drawing, or map GPU VRAM.

## Architecture

- src/video/mod.rs: mounts isolated driver module beside the existing passive video adapter.
- src/drivers/gpu/vmware_svga/pci.rs: finds VMware SVGA II 15ad:0405, checks PCI I/O decoding, BAR0 I/O, BAR1 framebuffer and BAR2 FIFO descriptors. Rejects 64-bit memory BARs until proper support is added.
- registers.rs and capabilities.rs: read current interface ID, dimension limits, VRAM/FB/FIFO sizes, capability flags, then validate conservative bounds.
- fifo.rs: host-testable pure FIFO ring pointer and free-space arithmetic, **no MMIO mapping**.
- src/pci_io.rs: x86 single-core I/O adapter + serial diagnostics.
- src/graphics.rs, src/ui/raster.rs and src/shell/window_manager.rs: remain unchanged.

The only SVGA I/O write is to the **INDEX selector** port (BAR0 + 0), followed by a read of the **VALUE** port (BAR0 + 1). There are **no** VALUE port writes, device enable/disable writes, mode changes, PCI configuration writes, GPU memory accesses or FIFO commands. The PCI BAR1/BAR2 bases are *not* dereferenced. Physical addresses and aperture lengths are not guessed. Version ID reads may return an unnegotiated ID; that is safely reported as an error and the boot framebuffer continues to work.

## COM1 diagnostics

After desktop startup, one of:
- ZILLAOS_SVGA_PROBE=READ_ONLY_OK
- ZILLAOS_SVGA_PROBE=NO_DEVICE
- ZILLAOS_SVGA_PROBE=FALLBACK_REASON (for example FALLBACK_ID_NEGOTIATION_REQUIRED)

A successful probe also reports ZILLAOS_SVGA_ID_HEX, MAX_WIDTH_HEX, MAX_HEIGHT_HEX, VRAM_BYTES_HEX, FB_BYTES_HEX, and for ID >= 1, FIFO_BYTES_HEX and CAPABILITIES_HEX.

These are *device register reports*, not proof of usable GPU memory, supported switchable video modes or accelerated drawing.

## Test commands

Run in WSL2 Ubuntu with Rust 1.90.0:

    cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline
    cargo +1.90.0 build --release --target x86_64-unknown-none --features vm-demo --offline
    cargo +1.90.0 fmt --all -- --check

The existing .github/workflows/boot-smoke.yml requires **manual** workflow_dispatch; GitHub pushes do not trigger it.

## VMware verification

1. Snapshot the VM; build the new UEFI ISO and boot desktop.
2. Verify Files, Settings, Notepad, App Center, mouse and keyboard are intact.
3. Capture COM1 and look for exactly one SVGA probe result with no panic.
4. When a valid ID is shown, record values and compare against VMware settings.
5. On fallback, make sure the GOP desktop remains operational.
6. Physical resolution changes still require the Limine boot menu; Settings → Display is not available yet.

## Next milestones

Version-negotiation opt-in, safe GPU MMIO mappings with verified aperture extents, bounded FIFO commands and timeout, DisplayManager with mock mode switching and rollback, only then 2D acceleration. Windows Runtime is out of scope.

Specifications: VMware svga_reg.h and svga_interface.txt; see docs/WINDOWS_GRAPHICS.md.
