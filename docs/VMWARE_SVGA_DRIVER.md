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

## Stage 2 — opt-in SVGA ID handshake (2026-11-06)

Normal Windows BuildRun and `scripts/build.sh demo` remain **read-only**.
The VMware interface specification requires writing the highest supported
interface ID to `SVGA_REG_ID` and checking its readback. The opt-in
`negotiation.rs` tries IDs 2, 1 and 0 at most once each, and stops on
the first exact match. `probe_with_negotiation` requires a validated
VMware PCI BAR0 before calling the writer; the platform writer
allowlists REG_ID and known ID values only. The final ID is read again
and mismatches fail closed. Nothing writes WIDTH, HEIGHT, ENABLE or FIFO.

The pure `fifo::encode_update` implementation validates the complete
five-DWORD `SVGA_CMD_UPDATE` packet and rejects rectangles outside the
supplied surface, zero sizes and arithmetic overflow. It **does not**
send any command to the device. BAR1/BAR2 aperture mapping and
actual FIFO sync remain unimplemented.

Normal ISO (no SVGA value-port writes):

```bash
bash scripts/build.sh demo
```

Experimental ISO (separate file, VM snapshot recommended):

```bash
bash scripts/build.sh demo-svga-id
```

The experimental artifact is
`dist/zillaos-0.0.2-dev-demo-svga-id-uefi.iso`.
Set VMware's CD/DVD ISO path manually to this file; default Windows
BuildRun does not select it. An opt-in boot logs
`ZILLAOS_SVGA_ID_NEGOTIATION=OPT_IN` to COM1 before the probe.
Compare that to a normal build, which must never log that marker.
Watch for `ZILLAOS_SVGA_PROBE=READ_ONLY_OK` or a FALLBACK status.
No claim of physical resolution changes, GPU/FIFO initialization,
2D/3D acceleration or real VMware validation is made at this point.

Reference: https://github.com/prepare/vmware-svga/blob/master/doc/svga_interface.txt
Reference: https://github.com/prepare/vmware-svga/blob/master/lib/vmware/svga_reg.h

## Stage 3 — FIFO transaction model and PCI MMIO resource preflight

**Implemented, but not connected to physical GPU memory:**

- mmio.rs checks independent explicit PCI BAR1/BAR2 resource lengths,
  power-of-two alignment, physical-address limits and overlap, in
  addition to device-reported framebuffer/VRAM/FIFO requirements.
- queue.rs defines a no_std FIFO backend interface with payload writes
  separated from publication of FIFO_NEXT_CMD. A failed write or
  synchronization poisons the queue to avoid reusing indeterminate state.
- try_enqueue checks FIFO header bounds, free space and 10 KiB
  minimum FIFO data area. A full queue returns InsufficientSpace
  without publishing or overwriting any memory.
- synchronize uses a caller-provided bounded BUSY poll budget. This
  is not a clock-based timeout because the current early kernel
  has no monotonic timer integrated into this graphics code.
- Tests use an in-memory mock FIFO to verify wraparound, publication
  order, header corruption, missing space, partial writes, and timeout.

**Security and hardware invariants:**

1. The PCI BAR register gives only a base address, not resource size.
   Never treat SVGA_REG_VRAM_SIZE or SVGA_REG_MEM_SIZE as proof of
   the PCI BAR aperture length. mmio.rs requires external authoritative
   resource metadata; currently the kernel cannot provide that proof.
2. No device FIFO backend has been added. The mock can only write
   Rust memory. It cannot issue actual SVGA_CMD_UPDATE commands.
3. A future backend must own a separate UC-mapped MMIO region with
   correct page permissions/cache policy, acquire sole FIFO writer
   ownership, prevent concurrent mode changes, and enforce a hardware
   release/ordering barrier before updating NEXT_CMD.
4. Never set CONFIG_DONE, ENABLE or SYNC while no mapped FIFO is
   available. On failed preflight, retain the Limine GOP framebuffer.
5. FIFO commands require successful SVGA ID negotiation and at least
   SVGA_ID_1, followed by device-capability verification.

**Test entrypoint:**

```bash
cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --features vm-demo --offline
```

**VMware:** normal BuildRun should still boot the same desktop and
produce the same ZILLAOS_SVGA_PROBE COM1 markers as Stage 2.
No FIFO marker is emitted because no FIFO hardware writes occur.

Next blocker: implement a PCI resource allocator that knows BAR
aperture sizes and a kernel-owned MMIO mapping API; implement proper
device fences and a real FIFO backend only after that.

References:
- https://github.com/prepare/vmware-svga/blob/master/doc/svga_interface.txt
- https://github.com/prepare/vmware-svga/blob/master/lib/vmware/svga_reg.h
