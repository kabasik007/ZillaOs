# ZillaOS — staged roadmap with PASS/FAIL gates

**Baseline:** 2026-10-19. Dates are deliberately not promised. Progress is measured by boot artifacts and tests, not estimates.

## Definition of Done — every milestone

- Code builds reproducibly from a clean checkout using documented versions.
- Relevant host-side tests, static checks and QEMU smoke tests pass.
- Negative/failure cases are tested, not merely happy path.
- Build/test logs and failures are attached to commits or CI runs.
- A demo has a deterministic command and an unambiguous expected signal.
- No unreviewed `unsafe` change; associated invariants and tests are documented.
- Status is **Planned**, **In progress**, **Verified**, or **Blocked**. Do not mark Verified from a screenshot.

## M0 — Planning baseline (docs only)

**Scope:** README, architecture, security, delivery roadmap, repository naming.  
**PASS:** all documents committed, paths resolve, claims reflect real repository state.  
**FAIL:** missing/contradictory plans.  
**Not equivalent to a running kernel.**

## M1 — v0.0.1 Boot

**Current implementation status (2026-10-19):** source committed for kernel, linker, Limine build script, QEMU smoke test, bounded RGB32 framebuffer boot splash, graphical demo-mode VM launcher, host environment doctor, host-side Python unit tests and manually dispatched CI. **Verification: NOT RUN for Rust build and QEMU boot** — toolchain/emulator unavailable in this execution environment. Source-consistency checks passed; these do not imply an executable kernel. Do not mark this milestone complete until all runtime gates pass.

**Build:** pinned Rust toolchain, `x86_64-unknown-none`, Limine protocol + UEFI under QEMU/OVMF, boot image, serial console and framebuffer diagnostic optional.

**PASS:** 
1. clean scripted build creates named kernel ELF and boot image;
2. QEMU/OVMF executes it without host interaction;
3. serial log has a unique `ZILLAOS_BOOT_OK` marker;
4. a script asserts QEMU exit code and log match, failing on timeout;
5. negative test with missing kernel image fails predictably;
6. CI records exact toolchain and bootloader versions.

**FAIL:** boot solely via debugger injection, serial prints without actual boot, missing artifact/test logs.

## M2 — v0.0.2 Memory and CPU platform

**Progress:** v0.0.2-dev in `main`: 256-entry Limine map copy, CPUID physical/virtual address width, conservative bump-only 4 KiB frame planner, HHDM RAM pattern/readback/zero test, canonical paging metadata, and host-side Rust/Python test suites. **None of the Rust build, unit-test, and QEMU runtime gates is yet verified** in this environment. Added: minimal **fatal** IDT for #UD/#GP/#PF and opt-in UD2 integration-test ISO. Still missing: active page tables, own GDT/TSS/IST, complete IDT, recoverable fault handling, owned CR3, persistent allocation state, and recorded emulator evidence. See [EXCEPTIONS.md](EXCEPTIONS.md). See [MEMORY.md](MEMORY.md), [PAGING.md](PAGING.md), [CHANGELOG.md](CHANGELOG.md).

**Build:** checked boot memory map, frame allocator, checked paging, kernel heap if needed, IDT/exception handling, timer tick and robust panic diagnostics.

**PASS:** frame allocation/free randomized host tests, guard-page fault test, page permissions checks, intentional kernel fault produces expected diagnostics, deterministic QEMU test.

**FAIL:** silent memory corruption, unexpected double allocation, unexpected return from fatal exceptions.

## M3 — v0.0.3 User-mode isolation

**Build:** ELF user loader, ring-3 transition, per-process page tables, process/thread context, round-robin scheduler, safe user pointer copying.

**PASS:** two independently scheduled user processes; write to unmapped / kernel memory terminates only offender; surviving process continues with kernel alive; no privilege escalation path in tested syscalls.

**FAIL:** user fault causes panic, processes share unrestricted address space, kernel blindly dereferences user pointers.

## M4 — v0.0.4 IPC and capabilities

**Build:** endpoint create/send/receive, generation-tagged capability slots, minimal rights model, bounds-checked messages and queue backpressure.

**PASS:** two processes exchange known message; without endpoint capability, request gets access-denied; forged/stale handle rejected; fuzz-like malformed messages do not crash kernel; deterministic timeout and cleanup.

**FAIL:** cross-process raw-pointer dependence, unconstrained queue growth, ambient root-style service authority.

## M5 — v0.0.5 User-space services and supervision

**Build:** `init` user process, console server, process restart and resource cleanup.

**PASS:** force-kill user-space console server; supervisor recreates it; independent process and kernel remain running; stale handles fail safely; memory accounting returns to baseline within expected cache reservations.

**FAIL:** reboot required to recover a normal user-space service.

## M6 — v0.1.0 Systems MVP

**Build:** initramfs/ramfs, VFS server, first virtio-pci block path under clearly described DMA trust model, read-only block reads before writes.

**PASS:** read a known file from ramfs and verified test block image; corruption/invalid boundaries rejected; host-integrity test guarantees test VM cannot modify a host disk; end-to-end tests produce reproducible output.

**FAIL:** raw host disks used in CI; file reads occur through privileged shortcuts without documented rationale.

## Zilla Setup — First-run experience (experimental)

Added a bilingual Ukrainian/English welcome wizard before the `vm-demo`
desktop. Preferences for region, time zone, hostname and theme are only
stored in RAM; it **does not** install or format disks or set a clock.
Runtime verification remains open. See [SETUP.md](SETUP.md).

## Experimental GUI spike — Zilla Desktop (parallel to M1/M2 integration)

**State:** source for a directly rendered framebuffer desktop, keyboard state, sample windows, dock, launcher and themes has been committed directly to `main`; this does **not** complete M1/M2/M7. Compile and QEMU/UI interaction evidence remain **NOT VERIFIED**. The experimental D1a source update adds a fixed 4 MB offscreen RGB32 graphics buffer and bounded cursor-only pixel restoration, with PPM screenshot comparison — see [GRAPHICS.md](GRAPHICS.md). The D0.6 update adds a QEMU PS/2 mouse driver, bounded mouse decoder, software cursor and pointer hit tests — see [MOUSE.md](MOUSE.md). The D0.5 update adds a pure floating-window manager with three demo windows, focus, z-order, minimization, maximization and keyboard geometry; actual execution still awaits QEMU. See [Desktop Roadmap](DESKTOP_ROADMAP.md) and [Desktop Architecture](DESKTOP_ARCHITECTURE.md). Real apps, secure compositing and user-space isolation remain dependent on M3–M6.

## M7 — v0.2.0 Usable developer prototype (future)

**Scope candidates:** compositor/framebuffer rendering, input events, a text terminal, network service and expanded driver isolation.

**PASS criteria to be specified by ADRs only after M6 results.** A pretty screenshot is not sufficient proof of an isolated desktop system.

## Post-MVP research backlog

- x86-64 SMP, 64-bit process ABI stability, crash dumps and symbols.
- IOMMU isolation, native hardware validation, input/USB and power management.
- Capability delegation/revocation complexity, fuzzing syscall surfaces.
- Reproducible signed images, A/B updates, recovery mode.
- Optional POSIX-compatibility layer, executable loaders and GUI design.

## Execution discipline

One milestone → one minimal PR → measured outcome → postmortem if failed → next milestone. Refuse feature work that bypasses a milestone's prerequisites unless explicitly labelled as an isolated research spike. Reserve hardware/native experiments for after QEMU baseline and enforce no risk to the user's machine.

## First five implementation tasks (after planning)

1. **BOOT-01**: version pin Rust, Limine, QEMU, OVMF and document host prerequisites.
2. **BOOT-02**: minimal `no_std` Rust entrypoint and linker layout with explicit panic handler.
3. **BOOT-03**: make bootable UEFI image via script (no undocumented manual preparation).
4. **BOOT-04**: QEMU serial logger and `ZILLAOS_BOOT_OK` marker.
5. **BOOT-05**: automated QEMU smoke test with timeout, non-zero failure exit, clean CI report.

BOOT-01 through BOOT-05 have initial source implementations committed; VM demo and source-level test harness were added. None has been validated by a successful end-to-end QEMU run yet. **Current status: IN PROGRESS / NOT VERIFIED**.
