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
| **D0: Desktop preview** | committed source | Native RGB32 desktop, responsive layout, topbar, Dock, mock Files/Terminal/Settings, launcher; F1–F5, Tab, Esc; Rust state tests |
| **D0-Q: Emulator validation** | next blocking step | Rust compilation PASS; QEMU/OVMF `demo` shows desktop; input switches window; screenshot + serial logs archived |
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
