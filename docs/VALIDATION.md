# ZillaOS — threat model, tests, and delivery gates

**State:** M1 plus v0.0.2-dev M2 CPU/physical/HHDM integration source committed in main; **Rust compile, host tests and QEMU smoke checks have NOT RUN** in this environment. Do not mark these gates PASS without actual recorded evidence.

## 1. Assets and trusted boundaries

Protected assets: kernel integrity, address-space confidentiality/integrity, capabilities, process lifetimes, boot artifacts, host files when QEMU runs, future device DMA mappings.

Potential adversary: a buggy or malicious unprivileged process crafting syscall/IPC inputs; a compromised user-space driver; malformed ELF, boot image, initramfs, filesystem contents, PCI responses.

Trusted computing base: kernel, boot transition and unavoidable platform privileged functions. Bootloader and firmware are external trusted startup components unless future verification is implemented.

**Important limitations:** Rust `unsafe`, logic errors, privilege mapping errors, shared-memory bugs, interrupts, and DMA can undermine confinement. QEMU process isolation is not equivalent to native hardware security.

## 2. Threat → mitigation → independent test

| Threat | Intended mitigation | PASS evidence (future) |
| --- | --- | --- |
| User pointer to kernel address | validate range and permissions before copying | invalid pointer results in error/fault confined to process |
| Stale/fake capability | slot+generation+rights checking | stale and forged handles always rejected |
| Message size overflow | bounded messages, checked arithmetic, quotas | max/max+1 and overflow inputs do not crash kernel |
| User process crash | separate ring-3 address space and fault handler | crash leaves a second process and kernel alive |
| Infinite send flood | bounded endpoint queues and backpressure | memory stays bounded |
| Executable data page | W^X, NX where supported | attempt to execute NX page faults |
| Memory double allocation | ownership-aware physical frame allocator | randomized invariant test detects overlaps |
| User driver DMA attack | IOMMU or treat unisolated driver as trusted | no native DMA isolation claim until hardware test |
| Broken boot image | boot protocol validation, structured failures | explicit failed boot with timeout and failure logs |
| Unexpected host disk write | ephemeral QEMU image, no raw block device binding | host canary checksum unchanged |

## 3. Test layers

- **Host tests:** pure Rust logic of allocators, handle tables, page math, object lifetimes, serialization; fuzz input parsers when feasible.
- **Static checks:** `cargo fmt --check`, `cargo clippy` for host-testable modules, dependency audit and explicit `unsafe` review.
- **Cross compile:** reproducible `x86_64-unknown-none` build with pinned toolchain, protocol and linker options.
- **QEMU black-box:** serial log markers, timeout handling, controlled QEMU exit codes, deliberately invalid startup config, crash injection.
- **Security regression:** page permissions, arbitrary user pointers, stale handle reuse, malformed ELF/IPC, reboot/fault recovery.
- **Performance baseline:** only once IPC exists; benchmark latency and context switches with fixed QEMU/host settings and report variance.

## 4. Reporting template

```text
ZillaOS test run
Commit: <sha>
Rust/QEMU/OVMF/Limine versions: <exact versions>
Host: <os + cpu>
Build: PASS | FAIL | NOT RUN
Boot: PASS | FAIL | NOT RUN
Memory tests: PASS | FAIL | NOT RUN
User isolation: PASS | FAIL | NOT RUN
IPC capability tests: PASS | FAIL | NOT RUN
Artifacts: <paths/CI URLs>
Observations: <facts, logs, reproducer>
```

**Never** use PASS for unexecuted tests. Every broken check blocks declaring a milestone Verified.

## 5. Safe experimentation

- Start with QEMU virtual storage, never bind real disks during early kernel development.
- Do not boot experimental kernel on primary PC until hardening/recovery and explicit risk review.
- Enable serial logs, symbols and a debug build before multi-stage fault testing.
- Dependencies/bootloader binaries have pinned versions and verified checksums or provenance before CI.
- Release provenance and vulnerability review become mandatory before public binary releases.

## 6. Immediate audit checklist (documentation phase)

- [x] Canonical project goal and scope written.
- [x] Boot-chain model and trust assumptions written.
- [x] Kernel/user boundary and capability IPC proposal written.
- [x] Milestone gates defined.
- [x] Kernel source exists (M1 bootstrap + early M2 CPUID, HHDM readback and paging metadata; not yet compiled).
- [x] Host Rust memory/paging unit tests written (not yet run).
- [x] QEMU serial smoke test checks CPU/HHDM/frame integrity and guards against forged success markers.
- [x] Fatal IDT (#UD/#GP/#PF) and opt-in UD2 fault-mode test implemented in source (NOT RUN).
- [ ] Negative UD2 QEMU test recorded: vector 6, error code 0, exit 35.
- [ ] Bounded two-frame HHDM write/read/zero integration probe actually observed in QEMU.
- [ ] Rust host memory unit tests executed.
- [ ] Clean reproducible build executed.
- [ ] QEMU boot observed.
- [ ] Process crash contained.
- [ ] IPC isolation independently tested.

References: https://docs.sel4.systems/ and https://doc.rust-lang.org/rustc/platform-support/x86_64-unknown-none.html.
