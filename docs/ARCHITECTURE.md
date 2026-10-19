# ZillaOS — Architecture v0.1
**Date:** 2026-10-19  
**Status:** Proposed baseline. No implementation is claimed.

## 1. Architectural mandate

An original, Rust-first x86-64 operating system with a **minimal privileged kernel**, independent address spaces, IPC over capability-checked endpoints, and isolated user-space servers. Prioritize auditable invariants, portability between emulated platforms, and incremental vertical slices over feature breadth.

### Guiding invariants

- All kernel entry points treat user-provided bytes, pointers, lengths, and handles as untrusted.
- Every mapped physical page has an accountable owner or explicitly defined shared ownership.
- Each process has its own user address space; kernel mappings never become writable by user mode.
- No W+X user/kernel mapping without a documented, narrow exception.
- Capabilities, not global service IDs or ambient access, authorize privileged operations.
- A fault in a normal user process is contained to that process; *device failures and kernel bugs are separately handled*.
- Platform-specific code lives under `arch/x86_64`, never scattered across higher layers.
- All `unsafe` code has a stated invariant, tests, and a reviewable boundary.

## 2. Deliberately narrow target

| Domain | MVP decision | Deferred |
| --- | --- | --- |
| ISA | x86-64, 4 KiB pages | arm64 / riscv64 |
| Firmware | UEFI under QEMU/OVMF | BIOS, native PC boot |
| Boot | Limine + version-pinned Limine protocol | Own UEFI bootloader |
| Toolchain | stable Rust where possible; `x86_64-unknown-none` kernel | complicated build-std/nightly unless required |
| CPU | single processor, ring 0/ring 3 | SMP, CPU hotplug |
| Devices | QEMU serial, timer, later virtio-pci block | USB, Wi-Fi, hardware GPU |
| Image | x86-64 ELF kernel + reproducible disk/ISO | Installer, signed secure-boot chain |
| Networking | no network in kernel MVP | user-space stack |
| Desktop | serial/framebuffer diagnostics | Wayland, Windows GUI compatibility |

**Limine contract:** bootloader supplies the initial memory map, framebuffer (when available), and other needed boot responses. Validate request/revision support and all response metadata. Preserve boot data until copied or its reclaimable pages are safe to free. Once firmware boot services are gone, they must never be called.

## 3. Component model

```text
              USER MODE (ring 3)                      KERNEL MODE (ring 0)
+--------------------------------------------+  +-------------------------------------+
| Applications (later)                       |  | arch/x86_64                         |
+--------------------------------------------+  | traps, syscall entry, paging,      |
| init / supervisor | console | VFS (later) |  | local timer, IRQ handling           |
+--------------------------------------------+  +-------------------------------------+
| Virtio driver services (later)              |<->| kernel core                         |
+--------------------------------------------+IPC| thread scheduling, address spaces,  |
| User runtime: syscall stubs, capabilities  |  | IPC endpoints, capability tables,  |
+--------------------------------------------+  | memory ownership, process lifecycle|
                                                +-------------------------------------+
                                                | RAM / CPU / interrupts / PCI       |
                                                +-------------------------------------+
```

The trusted computing base contains the kernel, privileged boot transition, and necessary platform code. `init` can supervise services but **must not** receive omnipotent hardware permissions by default. User-space drivers are preferred when feasible, not guaranteed for all hardware.

## 4. Kernel subsystems and initialization order

1. `boot`: validate boot protocol, copy firmware/bootloader information, establish serial log.
2. `arch/x86_64`: GDT/TSS/IDT, exception handlers, interrupt and timer setup, safe syscall transition.
3. `memory`: physical frame bookkeeping, kernel virtual address mappings, page-table management, guarded stack allocation, ownership accounting.
4. `task`: thread structures, single-CPU round-robin scheduler, explicit blocking/wakeup and fault lifecycle.
5. `capability`: per-process generation-tagged handle table and explicit rights.
6. `ipc`: bounded message endpoints; `send`, `receive`, optional call/reply; caps transferred only by policy.
7. `process`: validated ELF user program loading, private page tables, least-privilege bootstrap capability set.
8. `syscall`: small stable entry-point layer that validates pointers and translates handles to kernel objects.
9. `irq`: interrupt registration, acknowledgement and controlled grants to an authorized driver service.
10. `diagnostics`: serial output, panic records, boot milestone signals and QEMU test hooks.

Each layer is introduced only when exercised by an end-to-end test.

## 5. Memory model

- The firmware-supplied physical memory map is not blindly trusted. Check overlaps, boundaries, alignment, overflow and memory-type classification.
- Start with a simple physical bitmap/frame allocator. Switch to a more sophisticated allocator **only after profiling**.
- Use independent page tables for processes; prevent user access to kernel pages by hardware page flags.
- Reserve guard pages around kernel and user stacks; mark non-code pages NX; enforce W^X policy.
- Never expose kernel virtual pointers to an unprivileged process. Map shared buffers only through explicit memory objects and rights.
- Check integer arithmetic for address/size conversion before mapping; validate canonical x86-64 virtual addresses.
- Boot memory and page-table structures must have a documented release/reclaim policy.
- ASLR, COW, swapping, NUMA, demand paging and huge pages are future work — not MVP claims.

## 6. IPC and capability design

### Kernel objects
`Process`, `Thread`, `Endpoint`, `MemoryObject`, `AddressSpace`, `IrqBinding`.

### Handle rights
Illustrative rights: `READ`, `WRITE`, `MAP`, `GRANT`, `CONTROL`, `IRQ_ACK`. Rights are defined per object type, never silently inferred from an identifier. Kernel validates `(slot, generation, rights, object type)`.

### IPC v0
- Bounded inline message payload (initial size is a tested constant, **TBD**).
- Per-endpoint queue with bounded capacity and explicit backpressure.
- Kernel copies message bytes between address spaces (no user pointer retained after syscall).
- Cap transfer is explicit and may only **reduce** rights.
- Shared memory / zero-copy available only as a future opt-in, with mapping lifetime controls.
- Service discovery requires explicit delegated capabilities, not ambient unrestricted namespace access.
- Generation reuse must not accidentally resurrect stale handles. Revocation strategy must be tested before exposure.

### Failure semantics
A user service can be killed and restarted by a supervisor; IPC peers receive an explicit endpoint-closed / peer-gone error. Resource cleanup revokes outstanding handles and mappings according to object lifetime rules. Not every device failure is recoverable without hardware reset.

## 7. Syscall interface — provisional

Do **not** freeze syscall numbers until a user process is exercised. Candidate minimum surface:

- `thread_yield()`, `thread_exit(status)`
- `endpoint_create()`, `endpoint_send()`, `endpoint_recv()`
- `cap_close()`, `cap_transfer()`
- `memory_object_create()`, `memory_map()`, `memory_unmap()`
- `process_spawn()` for authorized process-managing services
- `irq_bind()` / `irq_ack()` for authorized driver services

Rules: bounded sizes, checked integer overflow, explicit error enums, versioned structs/ABI and forward-compatible reserved fields. System call ABI, error codes and process launch contract become formal ADRs at 0.0.4.

## 8. Drivers, device access, and DMA

- Earliest console/interrupt code is necessarily privileged.
- First real device is a QEMU **virtio-pci block device**, deliberately after process isolation and capability IPC.
- PCI configuration space access and IRQ routing are a controlled, typed service interface.
- **Critical:** user-space drivers and separate page tables do **not** prevent arbitrary DMA; IOMMU configuration, DMA buffer ownership and device reset are mandatory for genuine containment on native hardware.
- Until an IOMMU isolation path is verified, drivers with DMA are considered trusted or explicitly limited to QEMU labs.
- No claims of secure native hardware driver isolation during 0.1.x.

## 9. Boot and runtime artifacts

```text
QEMU/OVMF -> Limine -> ZillaOS ELF kernel
                    -> copy BootInfo, init CPU + memory
                    -> start scheduler -> user init -> console server
                                                -> later VFS / virtio service
```

Artifacts: kernel ELF, Limine config/EFI components, bootable ISO or raw disk image, serial test logs, QEMU test report.

## 10. Design alternatives evaluated

| Model | Advantage | Why not v0 |
| --- | --- | --- |
| Monolithic kernel | Fewer privilege crossings; simpler initial drivers | Larger trusted kernel and tighter fault coupling |
| Hybrid kernel | Flexible performance and driver partitioning | Can drift into unbounded kernel-space complexity |
| **Small capability-oriented microkernel** | Explicit isolation + testable security boundaries | More IPC/capability complexity, driver infrastructure work |

**Decision:** use microkernel *as architecture*, implement one tested slice at a time. If benchmarks later show a fundamental performance or complexity issue, write an ADR and change the design rather than protect the label.

## 11. Explicit non-goals

Windows and Linux executable compatibility, a production browser, native GPU drivers, gaming performance, full POSIX compliance, self-hosting compiler, formal verification, and secure boot are **not** part of the first year baseline.

## 12. Reference specifications

- Rust bare metal target: https://doc.rust-lang.org/rustc/platform-support/x86_64-unknown-none.html
- Limine protocol: https://github.com/limine-bootloader/limine-protocol/blob/trunk/PROTOCOL.md
- Limine UEFI usage: https://github.com/limine-bootloader/limine/blob/v12.x/USAGE.md
- seL4 microkernel reference: https://docs.sel4.systems/

Do not confuse inspiration from seL4 with possessing its formal verification proofs.
