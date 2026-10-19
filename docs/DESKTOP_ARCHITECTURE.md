# Zilla Desktop — architecture and interface specification

**Phase:** experiment / proof of concept, **not verified on QEMU yet**.

## System layers

`src/desktop.rs` is pure `no_std` GUI state/layout: app selection,
launcher visibility, dark/light mode, keyboard map and bounds. It has
host-testable unit tests. This logic must remain independent of the CPU
and the framebuffer to allow eventual user-space reuse.

`src/input.rs` is the current single-core **polling** PS/2 keyboard adapter
for QEMU q35. Translated set-1 scan codes feed the desktop event state
machine; unknown and release events are ignored. Do not assume every
modern PC has an i8042 device or that firmware left it enabled.

`src/window_manager.rs` is a pure no_std model of a three-window
floating workspace. It tracks focus, z-order, geometry and minimize/maximize
state without hardware access. `src/desktop.rs` sends translated keyboard
events to the model. None of these demo windows represents a real process.

`src/graphics.rs` is an experimental **immediate-mode** renderer. It
checks Limine RGB32 framebuffer metadata and writes bounded 32-bit pixels,
rendering a background, menu bar, app window, dock, launcher and fonts.
The renderer re-renders on state changes, and has no persistent global
framebuffer pointer. It does not allocate a heap.

`src/framebuffer.rs` remains the old minimal splash for the `test` and
`fault` images. `vm-demo` mode chooses the desktop renderer, keeping
automated tests deterministic and preserving old boot checks.

## Design system

| Token | Current use |
| --- | --- |
| `#0a1221`, `#122138` | wallpaper / top panel |
| `#17253a`, `#253b54` | window and header |
| `#19c7b0` | primary teal accent |
| `#18a9c1`, `#7d6bd1` | sample Files/Settings app colors |
| `#ebf5ff` | light foreground on dark panels |
| RGB32 | only format currently supported (checks masks, strides) |
| 320×200 minimum | compact layout with fallback if too small |

Visual structure: 42px top strip on typical displays; centered dock;
single centered program window; modal launcher; keyboard help along lower
edge when space is sufficient. The prototype includes a tiny built-in
5×7 ASCII uppercase bitmap font, avoiding external font loading.

## Render safety

- `limine::framebuffer()` checks pointer, RGB mask size, pitch,
  4-byte alignment, maximum dimensions and 64MiB maximum total surface.
- All drawing rectangles clamp to framebuffer bounds.
- No heap, hardware acceleration, dynamic OS resources, filesystem access,
  privilege transitions or unbounded GPU commands are issued by UI code.
- The framebuffer mapping is provided by Limine; when ZillaOS owns page
  tables, the display server must request a **new** authorized mapping.
- Pure state/layout tests are not pixel/memory-safety proofs; D0-Q needs a
  real QEMU boot, instrumentation and black-box evidence.

## Long-term service contracts

```text
UI applications (unprivileged)
       |
widget library (widgets/layout/text/events)
       |
window manager (focus/z-order/geometry)
       |
compositor service (frame damage, surfaces, display timing)
       |
display/input IPC services (capability rights)
       |
microkernel (address spaces, IPC, IRQ routing)
```

Every app surface must be an explicitly granted memory object; input
focus and injection must be authorized. No application gets arbitrary
video memory, global keyboard capture or unrestricted DMA by default.

## Accessibility / internationalization

Current bitmap glyphs are Latin uppercase only and GUI strings are
English developer labels. This is explicitly inadequate for a real UI.
Before D6: support Ukrainian, English, Russian and further languages
with Unicode shaping, larger font families, logical DPI, keyboard
layouts, screen reader hooks and high-contrast/keyboard-only navigation.

## Testing commands

```bash
cargo test --lib --target x86_64-unknown-linux-gnu --offline
bash scripts/build.sh test
python3 scripts/smoke_test.py --timeout 90
bash scripts/build.sh demo
python3 scripts/run_vm.py --display gtk
```

Expected **demo** serial lines after kernel boot, if framebuffer works:
`ZILLAOS_DESKTOP_READY` and `ZILLAOS_DESKTOP_INPUT_POLL`.
Every visible event change logs `ZILLAOS_DESKTOP_STATE_CHANGED`.

This is source documentation, not a passing test report.

## D0.5 — Multiple floating windows (source implemented, runtime NOT VERIFIED)

A new testable `src/window_manager.rs` now owns three independent demo window
records, visible/hidden/minimized/maximized states, active focus, z-order and
screen-bounded geometry. This is an **intermediate milestone before D3**,
not a user-space compositor and not real application processes.

**New controls:**

| Key | Action |
| --- | --- |
| F1 | Launcher |
| F2 / F3 / F4 | open or restore Files / Terminal / Settings |
| F5 | switch dark/light |
| F6 | minimize focused window |
| F7 | maximize or restore focused window |
| Tab | cycle focus among *visible* windows |
| Arrow keys | move focused normal window by 24 pixels |
| Shift + arrows | resize normal window by 24 pixels |
| Esc | close launcher or hide focused demo window |

Direct framebuffer rendering now draws each visible window in explicit
back-to-front order, including a distinct title treatment for focus.
Dock indicators distinguish active/visible/minimized windows. Hidden
or minimized demo windows retain their rect and can reopen through F2–F4.

**Automated D0.5 regression checks (written but not executed in QEMU):**
`cargo test --lib` tests window focus, ordering, geometry, maximize,
minimize and scancode modifiers. `scripts/desktop_smoke.py` now sends
F3, F4, Tab, F6, F2 to QEMU and checks the resulting serial snapshots
before saving a framebuffer screenshot.

Exact expected snapshots:
```text
F3  -> 2 visible, focus TERMINAL
F4  -> 3 visible, focus SETTINGS
Tab -> 3 visible, focus FILES
F6  -> 2 visible, focus SETTINGS
F2  -> 3 visible, focus FILES
```

**Limitations:** these are mock app windows within one privileged kernel
process. There is no mouse dragging, clipboard, real filesystem, terminal
execution, user-mode service or GPU acceleration. The bitmap desktop
is intended only for a disposable QEMU virtual machine, not native PCs.
