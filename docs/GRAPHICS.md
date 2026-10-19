# Zilla Graphics Engine — software composition foundation (D1a)

**State (2026-10-20): source committed directly to `main`; compilation,
actual QEMU boot and performance measurement are NOT VERIFIED.**

## What changed

Previously every pointer movement triggered `graphics::render()`, redrawing
the entire background, Dock, every window and the cursor directly into
Limine's RGB32 framebuffer.

The new graphics pipeline uses a **fixed, heap-free RGB32 backing buffer**:

```text
Desktop events (QEMU keyboard / mouse)
      |
      +--- window/theme/launcher changes --------------------+
      |                                                      |
      v                                                      v
full repaint                                          buffer in .bss
    -> wallpaper -> panels -> windows -> Dock -> launcher     |
                                                             v
                                                    one frame copy to Limine RGB32
                                                             |
                                                         software cursor
      |
      +--- only pointer location/button changes
                  |
           restore old cursor rectangle from backing buffer
           restore new cursor rectangle from backing buffer
           draw cursor directly on display (no full scene recompute)
```

This is **software scene buffering**, not a GPU flip chain, swapchain,
hardware double-buffer swap, vertical synchronization, or user-space
compositor. Whole-frame transfers may still visibly tear.

## Strict capacity and fallback

| Setting | Value |
| --- | --- |
| Backing dimensions | up to 1280 × 800 |
| Backing storage | `1280 * 800 * 4 = 4,096,000` bytes |
| Storage lifetime | single static `.bss` allocation, no heap/stack |
| RGB format | bootloader-validated 32-bit channel masks |
| Cursor damage max | two rectangles of 12 × 20 px = at most 480 pixels |
| Larger framebuffer | explicitly falls back to bounded direct rendering |
| Runtime status | `ZILLAOS_GRAPHICS_BUFFERED_READY` or `ZILLAOS_GRAPHICS_DIRECT_FALLBACK` |
| First fast cursor path | `ZILLAOS_GRAPHICS_CURSOR_DAMAGE_OK` |

For illustration only: a 1024×768 full-screen copy transfers 786,432
pixels, while the pointer-only fast path restores at most 480 pixels
before redrawing the pointer. **This is an algorithmic comparison, not
a measured performance or latency gain.**

## Correctness and safety boundaries

- `src/raster.rs` defines bounded capacity, rectangle clipping,
  checked stride and surface length validation, cursor damage geometry,
  and `fill_rect` for packed RGB32 data.
- Independent Rust host tests use sentinel pixels to show that a
  clipped rectangle does not corrupt padding or outside memory.
- `src/graphics.rs` is the only layer holding the global offscreen
  backing store. An `UnsafeCell` plus a documented single-CPU
  `Sync` contract limits writes to the early bootstrap execution
  path. This assumption **will not hold** after enabling SMP,
  preemption or a separate compositor process; synchronization must
  be added before those milestones.
- `BACKBUFFER_VALID` becomes true only after a complete scene
  transfer. The fast cursor path checks both the flag and screen
  dimensions. It restores from the cursor-free backing scene and
  writes the new pointer directly.
- The mode falls back to direct framebuffer drawing if the monitor
  resolution is outside the fixed backing capacity.
- A valid RGB32 framebuffer comes from Limine; physical display
  memory remains privileged and is not accessible to independent apps.
- Full-frame flushes are not synchronized with display refresh;
  true tearing elimination needs a display-aware compositor and driver.
- No claims of hard real-time responsiveness or sustained FPS are made.

## Verification

Run after prerequisites in [VM.md](VM.md):

```bash
git switch main
git pull origin main
cargo fmt --all -- --check
cargo test --lib --target x86_64-unknown-linux-gnu --offline
python3 -m unittest discover -s tests -v
bash scripts/build.sh demo
python3 scripts/desktop_smoke.py --timeout 90
```

The QEMU GUI regression now:
1. Expects normal desktop and window-manager serial readiness.
2. Opens and cycles sample windows using virtual keyboard events.
3. Captures the first P6 framebuffer screenshot.
4. Sends QEMU PS/2 mouse motion; requires the mouse motion marker and,
   for a buffered resolution, the fast cursor-damage marker.
5. Captures a second screenshot and checks that a **small, nonzero**
   number of RGB bytes changed (not merely the text of a serial log).
6. Tests mouse left-button down/up serial evidence.
7. Saves screenshot pairs and serial logs for investigation.

A missing / malformed / identical / drastically changed screenshot
is **FAIL**. No verified QEMU output exists in this execution environment.
Do not mark D1a as a release until the actual Rust build, host tests,
boot and screenshot regression have passed.

## Next engineering tasks

1. Convert all layout positions to a DPI/logical-space representation.
2. Replace the 5×7 ASCII font with validated font rasterization and UTF-8
   shaping (Ukrainian text must be supported).
3. Introduce per-window display lists, real dirty-rect repaint for window
   movement and scroll, not only cursor movement.
4. Add a page-backed allocator and dynamic/resizable backbuffer beyond the
   1280×800 fixed bootstrap bound.
5. After process/IPC support, migrate the compositor into user space with
   capability-controlled surfaces, synchronization and fault isolation.
