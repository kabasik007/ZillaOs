# ZillaOS — Zilla Desktop roadmap

**Created:** 2026-10-19  
**Source of truth:** `main`.  
**Status:** The initial RGB32 framebuffer + keyboard prototype is implemented in source; Rust compilation and QEMU boot **have not been verified**.

## Product vision

Zilla Desktop is an original workspace taking **interaction patterns**, not code, branding or copied assets, from:
- macOS: quiet top menu bar, centered dock, consistent app visuals;
- Windows: discoverable windows, system settings, file manager and keyboard navigation;
- KDE Plasma: extensive customization and extensible window/panel behavior.

Original Zilla design system: dark graphite/navy, cyan/teal accents, high-contrast labels, rounded controls, and optional bright mode. Native rendering on Rust. It is **not** macOS/Windows/Linux and reuses none of their UI code.

## Two architectural phases

```text
NOW — early QEMU research
-------------------------
Limine RGB32 framebuffer
  -> kernel-private direct-pixel drawing
  -> Zilla Desktop state machine (no_std)
  -> PS/2 keyboard polling in QEMU
  -> 1 visible sample window, dock, top panel
(no user processes / no isolation)

TARGET — actual operating-system desktop
----------------------------------------
Applications: Zilla Files / Zilla Terminal / Settings
         |
User-mode UI toolkit + accessible text/widget primitives
         |
User-mode window manager / software compositor
         |
Display service + keyboard / pointer device services
         |
Capability-protected IPC to filesystem and process servers
         |
ZillaOS microkernel + hardware drivers
```

The current framebuffer prototype is a **UI spike**; it is not proof that the process, IPC, VFS or device architecture is ready. We must move the compositor out of the kernel when process isolation exists.

## Delivery milestones

| Stage | Target | Demonstration / gate |
| --- | --- | --- |
| **Storage inventory v0.1** | source committed; runtime unverified | passive VM-only PCI controller scan, storage onboarding page, installation disabled |
| **Installer-plan v0.1 (read-only)** | source committed; runtime unverified | Checked GPT/ESP/system partition arithmetic for 512/4096-byte sectors; no storage writes or disk discovery |
| **OOBE v0.1: First-run Setup** | source committed, runtime unverified | Ukrainian/English wizard, region/timezone, hostname, theme; framebuffer screenshot and Enter navigation; preferences RAM-only |
| **D0: Desktop preview** | committed source | Native RGB32 desktop, responsive layout, topbar, Dock, mock Files/Terminal/Settings, launcher; F1–F5, Tab, Esc; Rust state tests |
| **D0.6: PS/2 mouse preview** | source committed; integration unverified | QEMU AUX mouse, pointer, Dock clicks, title-bar dragging, RGB32 cursor, optional keyboard-only fallback |
| **D0.5: Window manager state** | source committed; integration unverified | Three independent demo windows, focus, z-order, minimize/maximize, keyboard movement and resizing; screenshot regression |
| **D0-Q: Emulator validation** | next blocking step | Rust compilation PASS; QEMU/OVMF `demo` shows desktop; input switches window; screenshot + serial logs archived |
| **D1a: Buffered software graphics** | source committed; integration unverified | 4 MB RGB32 offscreen scene (≤1280×800), cursor-only damage restoration, PPM screenshot-diff regression |
| **D1: Graphics primitives** | after D0-Q | stable pixel and font layers, verified bounds/clipping, dirty-rect redraw, console and scalable text |
| **D2: Input** | after CPU/IRQ foundation | reliable keyboard event queue, scancode translation, mouse/pointer, PS/2 and virtio-input with bounds tests |
| **D3: Windowing MVP** | after D1/D2 | multiple windows, z-order, focus, drag, minimize, resize, keyboard shortcuts, double buffering |
| **D4: System applications** | after VFS/process MVP | real file browsing through VFS, actual process-backed terminal, persistent settings |
| **D5: Compositor service** | after user-mode isolation + IPC | user-space compositor with capability-gated framebuffers/input; fault restart without kernel crash |
| **D6: Design System v1** | after compositing | icons and SVG/vector ingestion, font rasterization, theming, HiDPI, animations, accessibility |
| **D7: Daily-driver research** | long term | multiple displays, GPU acceleration, clipboard/drag-drop, localization, power/audio/notifications |

## D0 keyboard contract (QEMU translated PS/2 set 1)

| Key | Behavior |
| --- | --- |
| F1 | show/hide application launcher |
| F2 | open Files sample window |
| F3 | open Terminal sample window |
| F4 | open Settings sample window |
| F5 | switch dark/light appearance |
| Tab | cycle through available demo windows |
| Esc | close launcher or hide active window |

This intentionally **does not** support pointer input or typing terminal commands yet. The Files entries are **static sample folders**, not actual VFS content. Screen updates are direct-to-framebuffer, not an isolated compositor; software CPU may redraw slowly.

## D0-Q acceptance and safety

- [ ] Kernel `test` image emits boot markers and exits QEMU 33.
- [ ] Rust `cargo test --lib` passes Desktop state, layout and scancode tests.
- [ ] `demo` ISO boots in a QEMU window and outputs `ZILLAOS_DESKTOP_READY`.
- [ ] F2/F3/F4 switch the visible application, F1 opens launcher, F5 changes theme.
- [ ] No memory access outside Limine-validated RGB32 surface (tested/instrumented).
- [ ] Screenshot and serial transcript are saved from a **real** QEMU boot.
- [ ] No physical disks, external networks or host volumes exposed to VM.

- [ ] QEMU PS/2 mouse initializes, moves cursor, reports left press/release; Dock clicks and title dragging verified in actual guest.

- [ ] Buffered framebuffer logs `ZILLAOS_GRAPHICS_BUFFERED_READY`; cursor-only motion logs `ZILLAOS_GRAPHICS_CURSOR_DAMAGE_OK` and changes only local PPM pixels.

**Do not close D0-Q based on GitHub commits or synthetic screenshots.**

## Non-goals for D0

Actual app processes, Windows/Linux/macOS binary compatibility, transparency,
blur, hardware GPU, browser, networking, persistent files, a real shell,
multi-monitor, mouse movement, secure driver isolation.

## Development order

1. Make kernel baseline compile/run; fix build/boot blockers before GUI claims.
2. Boot framebuffer desktop via `bash scripts/build.sh demo`.
3. Log and manually exercise QEMU keyboard input; capture evidence.
4. Add a testable pixel/dirty-rectangle layer and window state machine.
5. Build a user-mode compositor **only** after the process/IPC prerequisites.

Other plans: [Kernel roadmap](ROADMAP.md), [Architecture](ARCHITECTURE.md),
[Build & VM quickstart](VM.md).

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


D0.6 reference: [Mouse bootstrap and known limits](MOUSE.md).


D1a graphics contract: [GRAPHICS.md](GRAPHICS.md).


First-run product onboarding: [SETUP.md](SETUP.md). Disk installer,
persistent settings and user accounts are explicitly NOT implemented.


Future installation planning: [INSTALLATION.md](INSTALLATION.md). Before any
disk write, require read-only device discovery, target selection, explicit
confirmation and verified recovery/rollback.


Storage-controller discovery (not disks) is documented in [STORAGE.md](STORAGE.md).
