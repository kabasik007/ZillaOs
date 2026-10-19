# ZillaOS — Architecture Decision Records (ADR) index

We keep consequential choices explicit. Status `Accepted for planning` is not proof of implementation.

## ADR-0001 — Original OS rather than Linux distribution
**Status:** Accepted for planning · **2026-10-19**  
**Decision:** Write ZillaOS kernel and user space; Linux/Windows kernel dependencies are outside the runtime architecture. Third-party *bootloader* and *development tools* are allowed.  
**Reasons:** The goal is to learn and control kernel/memory/IPC architecture.  
**Cost:** Longer route to useful software and driver support.  
**Revisit:** If the primary product goal becomes immediate end-user desktop usability, split off a separate Linux-based edition instead of quietly redefining this kernel.

## ADR-0002 — Rust-first
**Status:** Accepted for planning · **2026-10-19**  
**Decision:** `#![no_std]` Rust for kernel; `unsafe` minimized, isolated and documented.  
**Reasons:** safer memory ownership model for most kernel code.  
**Cost:** low-level boot/interrupt/platform abstractions still require `unsafe`; language does not prove kernel correctness.  
**Revisit:** Assembly stubs and external ABI interoperability are valid exceptions.

## ADR-0003 — Capability-oriented microkernel
**Status:** Accepted for planning · **2026-10-19**  
**Decision:** Keep privilege boundary small; processes, threads, address spaces, IPC and capability lookup remain in kernel. Services and most drivers run at user privilege when feasible.  
**Reasons:** isolation, explicit authority, independent service restart.  
**Cost:** IPC overhead, hard system architecture, initialization dependency chain.  
**Revisit:** after measuring M4 IPC throughput/latency and M5 failure recovery.

## ADR-0004 — Limine for bootstrapping
**Status:** Accepted for planning · **2026-10-19**  
**Decision:** Use version-pinned Limine bootloader and protocol with UEFI/QEMU; do **not** author a bespoke UEFI loader for the first milestone.  
**Reasons:** enables prompt validation of kernel design rather than firmware infrastructure.  
**Cost:** a maintained boot dependency; must track protocol revision changes and test image assembly.  
**Revisit:** only if boot-protocol constraints block a defined capability.

## ADR-0005 — One architecture, one virtual platform
**Status:** Accepted for planning · **2026-10-19**  
**Decision:** QEMU q35 + OVMF x86-64, single CPU, serial first. Virtio-pci block after ring-3 isolation.  
**Reasons:** deterministic test environment, low hardware risk, reproducible failures.  
**Cost:** native hardware and SMP delayed.

## ADR-0006 — No compatibility promise
**Status:** Accepted for planning · **2026-10-19**  
**Decision:** No Windows .exe, Linux syscall ABI, POSIX compliance or gaming compatibility guarantee in MVP.  
**Reasons:** compatibility commitments would dominate small-kernel development.  
**Revisit:** through separate research milestones after core IPC/VFS works.

## Open design decisions (resolve before implementation of corresponding phase)

| ID | Open question | Deadline |
| --- | --- | --- |
| ADR-0007 | exact syscall calling convention and ABI versioning | before M3 |
| ADR-0008 | cap handle bit layout, generation and revocation semantics | before M4 |
| ADR-0009 | message size, queueing fairness and timeout model | before M4 |
| ADR-0010 | process image/initramfs format and ELF contract | before M3/M5 |
| ADR-0011 | DMA/IOMMU strategy and native driver trust | before virtio/native milestones |
| ADR-0012 | repository software license, contribution rules, SBOM policy | before external contributions |
| ADR-0013 | SMP scheduling and shootdown model | after M6 |

## Rules for changing an ADR

Propose a replacement ADR in a PR with measurable evidence, migration implications and negative tests. Record supersession rather than silently changing earlier rationale.
