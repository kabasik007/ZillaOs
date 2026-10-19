# ZillaOS

**ZillaOS** is an experimental, independent operating system project: a Rust-first, capability-oriented microkernel with isolated user-space services. This is a learning and engineering project. The **first kernel bootstrap source is committed**, but a successful boot has not yet been independently verified.

> Status (2026-10-19): **v0.0.2-dev** committed directly to **main**. CPUID, Limine HHDM, RAM readback/zero test, paging metadata and negative tests are implemented in source. **Rust build and QEMU boot NOT VERIFIED**.

## Mission / Мета

Build a small, comprehensible, testable OS from scratch. A crash in an unprivileged service should not crash the kernel; each component should have only the resources and privileges it needs.

**First engineering milestone (M1):** Boot a Rust kernel in QEMU/OVMF, sanity-check Limine memory-map metadata, emit a serial success marker and pass an automated exit-status smoke test. The isolated process, IPC and restart goals are separate later milestones (M3–M5).

## Architecture at a glance

```text
Applications / future graphical shell              [user space]
           |
     System services: init, console, VFS, net
           |
     Driver services: virtio-blk, virtio-net, input
           |
       Capability-based IPC / syscalls
================ TRUST BOUNDARY ====================
   ZillaOS microkernel: scheduler, address spaces,
   memory ownership, IPC, IRQ routing, capabilities
================ HARDWARE BOUNDARY ==================
                 x86-64 + UEFI
```

*Language:* Rust `no_std` in the kernel; minimal audited `unsafe` at hardware and FFI boundaries.

*Bootstrap:* Limine bootloader and its boot protocol (not a custom UEFI implementation in v0.x), x86-64 ELF kernel, QEMU + OVMF. Boot components are third-party tooling, **not Linux**.

*Scope:* single architecture (x86-64), one CPU initially, QEMU virtio devices first, framebuffer/serial console before GUI. Native hardware and GPU acceleration come much later.

## Why this is not "Windows in a month"

We are intentionally **not** targeting Win32/.exe compatibility, Linux ABI, Steam games, or general-purpose drivers for MVP. These are independent, enormous compatibility projects. A user-space filesystem, networking stack, window server, and package format only become in-scope after the kernel primitives work.

## Milestones

| Version | Demonstration / Evidence |
| --- | --- |
| 0.0.1 — Boot | Reproducible image boots in QEMU/OVMF; serial `ZILLAOS_BOOT_OK`; CI checks exit status |
| 0.0.2 — Platform | Memory-map validation, physical allocator, page tables, exceptions, timer |
| 0.0.3 — Isolation | Ring 3 process, separate address spaces, fault in user process does not panic kernel |
| 0.0.4 — Microkernel | IPC endpoints, capability handles, grants/revocation, scheduler |
| 0.0.5 — Services | init and console in user space; restart crashed console service |
| 0.1.0 — Systems MVP | Minimal VFS, ramfs/initramfs, virtio-blk, deterministic end-to-end tests |
| 0.2+ — Desktop research | input, compositor, GUI, network and driver roadmap |

Each milestone has explicit **PASS / FAIL** acceptance tests, not just screenshots. Details: [ROADMAP](docs/ROADMAP.md).

## Documentation

- [Build and QEMU smoke test](docs/BUILD.md) — first kernel source, pinned Limine archive and reproducible test commands.
- [Run in VM on Windows and Linux](docs/VM.md) — WSL2 setup, graphical demo, host dependency checks.
- [Architecture](docs/ARCHITECTURE.md) — security boundaries, kernel components, memory model, IPC and interface contracts.
- [Early physical memory manager](docs/MEMORY.md) — bounded Limine map copy, CPU physical limits, frame-readback experiments.
- [Paging design](docs/PAGING.md) — canonical addresses and typed page-table metadata.
- [Changelog](docs/CHANGELOG.md) — v0.0.2-dev details and release blockers.
- [Roadmap and acceptance criteria](docs/ROADMAP.md).
- [Architecture decisions](docs/DECISIONS.md).
- [Threat model and testing](docs/VALIDATION.md).

## Engineering rules

1. Prefer a smaller verified vertical slice over prematurely designing all subsystems.
2. No global unrestricted device access in user space. Use capability handles and explicit resource grants.
3. Drivers are user-space **where technically feasible**; some bootstrap/platform support remains privileged.
4. Treat DMA and hardware IOMMU boundaries explicitly; process isolation alone does not isolate unrestricted DMA.
5. All system calls and IPC messages must validate lengths, permissions and ownership; no raw trusted user pointers.
6. All ABI changes are versioned; no unreviewed breaking changes after first user process.
7. Unit tests, QEMU smoke tests, panic-injection tests and regression tests are mandatory as milestones grow.
8. No unverified claims of memory safety, fault tolerance or security.

## References

- [Rust bare-metal x86-64 target](https://doc.rust-lang.org/rustc/platform-support/x86_64-unknown-none.html)
- [Limine boot protocol specification](https://github.com/limine-bootloader/limine-protocol/blob/trunk/PROTOCOL.md)
- [Limine UEFI usage](https://github.com/limine-bootloader/limine/blob/v12.x/USAGE.md)
- [seL4 concepts (inspiration, not reused code)](https://docs.sel4.systems/)

## Project governance

Canonical repository: [kabasik007/ZillaOs](https://github.com/kabasik007/ZillaOs). Default development branch: `main` for the architecture bootstrap; follow-on code changes should be reviewed through branches/PRs. License decision: **TBD before importing dependencies or accepting external contributions**.

Methodology: [Be Ambitious Pro](https://app.notion.com/p/3fb91a1af397814782cac435379a82a7) — outcome-led engineering, independent validation, real PASS/FAIL evidence.

## M1 development commands

```bash
python3 scripts/doctor.py
python3 -m unittest discover -s tests -v
cargo test --lib --target x86_64-unknown-linux-gnu --offline
bash scripts/build.sh test
python3 scripts/smoke_test.py --timeout 90
bash scripts/build.sh demo
python3 scripts/run_vm.py --display gtk
```

This creates distinct test and graphical-demo UEFI images, and verifies test-mode boot in QEMU/OVMF when prerequisites are present. Demo mode draws a framebuffer splash and remains running. The [GitHub Actions workflow](.github/workflows/boot-smoke.yml) is **manual only** to avoid consuming CI minutes automatically. Source publication is not the same as a passing test. The M2 boot integration additionally requires `ZILLAOS_CPU_READY`, `ZILLAOS_FRAME_PROBE_OK` and `ZILLAOS_MEMORY_READY`; none has been observed in a real emulator run yet.
