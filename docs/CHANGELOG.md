# Changelog

## Unreleased — ZillaOS v0.0.2-dev (2026-10-19)

**State:** sources committed in `main`; cross-compile and QEMU/OVMF execution have **NOT RUN** in the current environment. This is not a release.

### CPU & memory bootstrap

- Detect x86-64 physical/virtual address width and NX support through CPUID.
- Fix a Limine interoperability bug: non-usable memory-map entries can overlap each other; usable entries must never overlap anything.
- Bound the kernel-owned memory map to 256 descriptors; request 256 KiB kernel stack.
- Enforce CPU physical-address width when reserving usable 4 KiB frames.
- Request Limine HHDM dynamically (never hardcode a direct-map offset).
- Reserve two distinct usable pages, write/read patterns through HHDM, verify contents and zero both pages before reporting memory readiness.
- Validate HHDM virtual addresses against the CPUID virtual-address width (48/57 bits).
- Add host-testable page index/canonical address functions and typed non-leaf page-table entry builders.

### Testing and developer experience

- QEMU smoke verifies CPU, distinct aligned frames, HHDM, readback and boot markers **plus** the debug-exit code.
- Host Python tests cover forged/missing serial evidence and invalid VM firmware overrides.
- Rust unit tests cover usable vs reserved overlap, address widths, frame allocation, canonical addresses and PTE metadata.
- Manual-only GitHub Actions workflow saves a QEMU serial transcript when run.
- Test/demo ISO output names now use `zillaos-0.0.2-dev-*.iso`.

### Deliberately not implemented

- No active page table manipulation, CR3 switch, page fault handler, GDT/IDT, scheduler, ring-3 processes, persistence, device drivers, or secure native hardware boot.
- The experimental RAM probe runs only within Limine's trusted early-boot mappings.
- Passing source-consistency checks is **not** proof that Rust builds or that the guest boots.

### Release blockers

- [ ] Reproducible `cargo fmt --check` and `cargo test --lib` on Rust 1.90.
- [ ] Successful `cargo build --release --target x86_64-unknown-none`.
- [ ] Verified Limine 12.9.3 ISO and boot under QEMU/OVMF.
- [ ] `ZILLAOS_FRAME_PROBE_OK`, `ZILLAOS_MEMORY_READY`, `ZILLAOS_BOOT_OK` observed with QEMU exit 33.
- [ ] Negative boot tests and serial logs archived.
