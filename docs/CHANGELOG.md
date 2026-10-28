# Changelog

## Unreleased — ZillaOS v0.0.2-dev (2026-10-19)

**State:** sources committed in `main`; cross-compile and QEMU/OVMF execution have **NOT RUN** in the current environment. This is not a release.

### Read-only primary GPT validation library

- Added `src/gpt.rs`, a fixed-memory `no_std` verifier for protective MBR, GPT primary header (including CRC32), GPT partition-array CRC, 512/4096-byte logical sectors and bounded nonoverlapping partition geometry.
- Added synthetic Rust unit tests for valid images, CRC corruption, hybrid MBR, overlap, unsupported entry metadata, and read failures. No disk access or write API is included; no backup GPT or filesystem inspection yet.
- Source committed directly to `main`; actual compilation and Rust unit execution remain unverified here. See [GPT.md](GPT.md).

### VMware read-only PCI storage-controller inventory

- Added `src/storage_pci.rs`: a pure `no_std` PCI mass-storage class inventory, fixed eight-controller cap, multifunction handling and unit tests for VMware AHCI/NVMe example headers.
- Added `src/pci_io.rs`: VM-only PCI mechanism #1 config-header reader (writes only the 0xcf8 address selector, reads 0xcfc; no device config or sector writes).
- Early-boot COM1 now records PCI storage class/vendor/device/bus/slot/function and explicit `ZILLAOS_STORAGE_BLOCK_DRIVERS_UNAVAILABLE` marker.
- Zilla Setup now has eight pages with a read-only storage-controller summary and a visible `INSTALLATION DISABLED` warning. The number of selectable disks is hard-coded to zero until a real read-only block driver exists.
- QEMU GUI smoke captures a separate storage-step screenshot and drives eight Enter presses; CI retains both default/custom captures.
- **Source-only:** actual Rust compilation and VMware/QEMU PCI detection not yet validated. See [STORAGE.md](STORAGE.md).

### Read-only installer geometry planning and expanded OOBE tests

- Added `src/install_plan.rs`: pure `no_std` GPT/EFI + system partition preview calculator, handling 512/4096-byte logical sectors, capacity boundaries, 1-MiB front/end reserves, overflow checks and unit tests.
- NO disk enumeration, block driver, partition writing or formatting path exists; do not confuse calculated geometry with a functioning installer.
- Added custom automated first-run QEMU scenario with English, Poland/Warsaw, `lab01` and light theme. CI retains both screenshot sets.
- Corrected misleading startup log: a missing graphical framebuffer now produces `ZILLAOS_SETUP_NOT_COMPLETED` rather than a false successful setup log.
- Compilation/VMware/QEMU verification remains outstanding. See [INSTALLATION.md](INSTALLATION.md).

### Zilla Setup — First-run OOBE preview

- At start of `vm-demo`, show a dedicated dark/light bilingual Ukrainian/English setup wizard before displaying the desktop.
- Added 7 stages: Welcome, Language, Region, Time Zone, Device name, Appearance, Review; protected ASCII hostname (3–24 characters) and back navigation.
- Added Ukrainian bitmap glyphs and native RGB32 welcome-screen cards, left progress sidebar, navigation buttons and cursor click targets.
- First-run choices (language, region, timezone, device label, theme) stay available to the live Desktop in RAM and are logged to COM1. They are NOT persisted to disk and do not set RTC or install anything.
- QEMU GUI regression captures a setup screenshot and drives all seven steps by synthetic Enter presses before continuing existing desktop tests.
- **Implementation committed in source; Rust compilation, QEMU/VMware GUI boot not verified.** See [SETUP.md](SETUP.md).

### Zilla Graphics Engine — D1a

- Added no_std RGB32 raster geometry, checked span-filling and cursor damage tests.
- Added 4,096,000-byte static backing scene supporting up to 1280×800 without heap or a large stack allocation. Larger displays use the original clipped direct renderer.
- Full desktop redraw composes into BSS then transfers a complete scene to Limine. Pointer-only movements restore at most two 12×20 cursor regions and redraw a direct software cursor instead of repainting all windows.
- Headless QEMU screenshot regression compares actual RGB data before/after pointer motion and saves the screenshot pair in CI artifacts.
- **Not yet measured or proven by a real Rust build/QEMU run.** No GPU, vsync or hardware swapchain; full-frame transfers may still tear. See [GRAPHICS.md](GRAPHICS.md).

### Mouse and pointer interaction — D0.6

- Added hardware-independent Rust decoder for signed three-byte PS/2 mouse packets and unit tests.
- Replaced keyboard-only 8042 reader with a shared keyboard/mouse polling adapter. AUX mouse initialization uses bounded waits and falls back to keyboard-only.
- Added a software cursor and screen-bounded pointer coordinates; Dock and launcher click targets, window focus, title-bar dragging and red/yellow/green window controls.
- Extended headless QEMU GUI smoke with actual HMP mouse movement and button-injection checks for `ZILLAOS_MOUSE_READY`, `ZILLAOS_MOUSE_MOTION_DETECTED`, `ZILLAOS_MOUSE_LEFT_DOWN` and `ZILLAOS_MOUSE_LEFT_UP`.
- This is **source only; Rust compilation and guest PS/2 behavior remain NOT VERIFIED**. See [MOUSE.md](MOUSE.md).

### Experimental Zilla Desktop interface

- Added a pure Rust `no_std` desktop state and keyboard-decoding module with host-side unit tests.
- Implemented a dark/light framebuffer desktop preview: top menu bar, centered Dock, wallpaper, a sample Files/Terminal/Settings window, and an F1 launcher.
- Added polling QEMU PS/2 translated-set-1 keyboard adapter for F1–F5, Tab and Escape.
- `vm-demo` now enters an event loop and redraws on recognized state changes instead of halting at the boot splash.
- Added a three-window no_std floating window manager with independent visibility, z-order, focus, minimize, maximize, and screen-bounded keyboard geometry. Dock now indicates active/visible/minimized states.
- Added F6/F7 and arrow/Shift+arrow input handling and extended QEMU GUI smoke to assert focus, visible counts, minimize/restore and screenshot.
- **Important:** multiple *demo* windows do not imply separate processes or working file/terminal applications. Runtime tests remain NOT VERIFIED.
- GUI serial markers: `ZILLAOS_DESKTOP_READY`, `ZILLAOS_DESKTOP_INPUT_POLL`, `ZILLAOS_DESKTOP_STATE_CHANGED`.
- **NOT VERIFIED:** Rust compile, framebuffer appearance, QEMU keyboard support. No real file operations, shell execution, mouse, or separate UI process.
- See [DESKTOP_ROADMAP.md](DESKTOP_ROADMAP.md) and [DESKTOP_ARCHITECTURE.md](DESKTOP_ARCHITECTURE.md).

### CPU & memory bootstrap

- Detect x86-64 physical/virtual address width and NX support through CPUID.
- Fix a Limine interoperability bug: non-usable memory-map entries can overlap each other; usable entries must never overlap anything.
- Bound the kernel-owned memory map to 256 descriptors; request 256 KiB kernel stack.
- Enforce CPU physical-address width when reserving usable 4 KiB frames.
- Request Limine HHDM dynamically (never hardcode a direct-map offset).
- Reserve two distinct usable pages, write/read patterns through HHDM, verify contents and zero both pages before reporting memory readiness.
- Validate HHDM virtual addresses against the CPUID virtual-address width (48/57 bits).
- Add host-testable page index/canonical address functions and typed non-leaf page-table entry builders.

### Fatal CPU exception handling (early foundation)

- Corrected invalid literal newline escapes in `src/serial.rs` inline-assembly code.
- Added a minimal single-core x86-64 IDT for fatal #UD, #GP and #PF, with serial vector/error/RIP diagnostics and CR2 for page faults.
- Added host-testable page-fault flag decoder, and separate `fault` ISO that runs `UD2` only with the test feature.
- Added negative QEMU verdict (requires vector 6, error 0, IDT marker and exit 35) alongside normal exit-33 boot test.
- Full GDT/TSS/IST, complete IDT coverage and safe recovery remain unimplemented. See [EXCEPTIONS.md](EXCEPTIONS.md).

### Testing and developer experience

- QEMU smoke verifies CPU, distinct aligned frames, HHDM, readback and boot markers **plus** the debug-exit code.
- Host Python tests cover forged/missing serial evidence and invalid VM firmware overrides.
- Rust unit tests cover usable vs reserved overlap, address widths, frame allocation, canonical addresses and PTE metadata.
- Manual-only GitHub Actions workflow saves a QEMU serial transcript when run.
- Test/demo/fault ISO output names now use `zillaos-0.0.2-dev-*.iso`. Each mode gets its own matching ELF artifact.

### Deliberately not implemented

- No active page table manipulation, CR3 switch, recoverable page-fault handling, own GDT/TSS/IST, scheduler, ring-3 processes, persistence, device drivers, or secure native hardware boot.
- The experimental RAM probe runs only within Limine's trusted early-boot mappings.
- Passing source-consistency checks is **not** proof that Rust builds or that the guest boots.

### Release blockers

- [ ] Reproducible `cargo fmt --check` and `cargo test --lib` on Rust 1.90.
- [ ] Successful `cargo build --release --target x86_64-unknown-none`.
- [ ] Verified Limine 12.9.4 ISO and boot under QEMU/OVMF.
- [ ] `ZILLAOS_FRAME_PROBE_OK`, `ZILLAOS_MEMORY_READY`, `ZILLAOS_BOOT_OK` observed with QEMU exit 33.
- [ ] Negative UD2 handler test exits QEMU 35 with vector 6 and saved diagnostics.
- [ ] Negative boot tests and serial logs archived.
