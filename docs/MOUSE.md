# ZillaOS mouse support — PS/2 research preview (D0.6)

**Status:** source committed to `main` on 2026-10-20. Compilation and
real QEMU mouse interaction **NOT YET VERIFIED**.

## Scope

- QEMU q35 with emulated i8042 PS/2 keyboard and standard 3-byte mouse.
- Kernel-side, polling-only prototype; no timer, IDT IRQ1/IRQ12 or external
  user-mode input service. A real OS must move input out of privileged kernel
  code when process isolation and capabilities are implemented.
- Mouse setup failure is time-bounded and is **not fatal**: F1–F7, Tab and
  keyboard shortcuts still work with a keyboard-only desktop.

## Data path

```text
QEMU PS/2 mouse + keyboard
      |
ports 0x60, 0x64  (single reader, polling, bounded init)
      |
src/input.rs: InputEvent::{Mouse,Key}
      +-- keyboard: ScancodeDecoder (translated set 1)
      +-- mouse: MouseDecoder (standard 3-byte packets)
      |
src/desktop.rs: Desktop::handle_mouse
      +-- pointer position clipped to viewport
      +-- Dock hit testing / open app
      +-- window focus / close / minimize / maximize
      +-- title dragging (exclusive to focused normal window)
      +-- launcher click actions
      |
src/graphics.rs: framebuffer render with cursor drawn last
```

## Initialization

`Input::initialize()` first drains a bounded number of stale i8042
bytes, sends AUX enable `0xA8`, reads and preserves the controller's
configuration other than the AUX clock-disable and IRQ12 bits, and sends
`0xF6` (standard mouse defaults) and `0xF4` (enable reporting) via
`0xD4` forwarding. Each mouse command requires `0xFA` ACK. If the
controller is absent, an operation times out or a command is rejected,
the GUI continues keyboard-only.

The decoder validates the always-set first-byte bit 3, rejects overflow
flags, respects sign bits 4/5 for 9-bit relative motion and converts
device-up to screen-down movement. No wheel, extra mouse buttons, or
USB HID are supported yet.

## UI interactions

- Move: changes small software cursor position, clipped to the screen.
- Left click Dock: open or restore Files, Terminal or Settings.
- Left click an existing window: focus and raise it.
- Left button held on title bar: drag the focused window within viewport.
- Window title dots: red = hide/close demo view; yellow = minimize;
  green = maximize/restore.
- Left click launcher item: open the demo app.
- Right/middle buttons: decoded but currently intentionally ignored.

These windows are **demo views**, not processes. Files shows static folder
names and Terminal does not execute commands.

## Validation

First verify the existing boot, fault and keyboard tests. Mouse regression:

```bash
git switch main
git pull origin main
cargo test --lib --target x86_64-unknown-linux-gnu --offline
bash scripts/build.sh demo
python3 scripts/desktop_smoke.py --timeout 90
```

The headless GUI test uses QEMU's documented HMP `mouse_move` and
`mouse_button` commands. PASS requires serial evidence:

```text
ZILLAOS_MOUSE_READY
ZILLAOS_MOUSE_MOTION_DETECTED
ZILLAOS_MOUSE_LEFT_DOWN
ZILLAOS_MOUSE_LEFT_UP
```

It also checks the original three-window keyboard focus/minimize/restore
sequence and stores a screenshot plus serial log. The mouse events are
**not proof of correct hit-testing on screen**. Host-side Rust tests cover
the decoder, Dock clicks, pointer clamping, title dragging and launcher
selection. A manual or screenshot-based mouse target test remains open.

## Limitations / risks

- QEMU 8042 research driver only. Native PCs may have only USB or a
  different controller state; no ACPI enumeration occurs.
- The i8042 is polled without IRQs. Busy-polling consumes CPU, and full
  framebuffer redraw on mouse movement is inefficient.
- Firmware keyboard configuration and translated scan codes are
  assumptions, with fallback when mouse init fails. A real PS/2 driver
  needs controller detection, full self-test, queueing, IRQ handling and
  coexistence with USB legacy firmware.
- Mouse packet decoding may require additional synchronization behavior
  for unusual devices. Wheel mice (4-byte packets) are not supported.
- Limine-provided framebuffer mapping is still accessed directly by a
  privileged kernel thread.
- No compositor, double buffering, damage tracking, cursor hardware
  plane, font shaping, touchpad, gestures or assistive pointer features.

References:
- https://wiki.osdev.org/PS/2_Mouse
- https://wiki.osdev.org/I8042_PS/2_Controller
- https://www.qemu.org/docs/master/system/monitor
