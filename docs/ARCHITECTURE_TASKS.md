# ZillaOS execution architecture — owners, milestones and acceptance

## What is a process, thread, core, and task?

A **process** owns an isolated virtual address space (its own CR3/PML4),
security context and resource handles. It may own **many threads**.
A **thread** owns registers, its own user stack and kernel stack, lifecycle,
affinity and scheduling policy; threads of one process may share the same
process address space and handles. A **CPU logical core** executes at most one
thread at a time. **Multitasking** switches threads over time on one core.
**Multithreading** lets one process own several independently schedulable
threads. **SMP** runs threads in parallel on multiple cores. These are
different milestones; a Rust no_std module does not make them automatic.

The current 2026-11-06 normal ZillaOS Desktop runs an early kernel event
loop on one boot CPU. Its Files/Settings/Terminal windows are not processes.
The opt-in QEMU Ring 3 fixture **does** perform a real CPL3 transition,
`int 0x80`, clean exit and recoverable user faults, but it is not integrated
into a preemptive scheduler or the Desktop.

## Intended high-level hybrid architecture

```text
+---------------- USER SPACE (Ring 3) ---------------------+
|  Apps as isolated processes                              |
|  Compositor / Window Server     File Manager  Task Manager|
|  libc / Zilla ABI / optional Win32 compatibility layer    |
|                  IPC / capability handles                 |
+----------------- SYSCALL BOUNDARY -----------------------+
|  Kernel Ring 0                                           |
|  ProcessTable + ThreadTable + TaskSnapshot               |
|  Per-core scheduler / timer / IPC / sync primitives       |
|  VMM: page tables, page allocator, protected mappings     |
|  Driver services: PCI, storage, network, GPU, input       |
|  Exceptions: user fault -> terminate only owning task      |
|  Auditable permissions: least privilege and W^X           |
+----------------- HARDWARE -------------------------------+
|  x86-64 logical CPUs, APIC/x2APIC, TSC, IOMMU, PCIe       |
+----------------------------------------------------------+
```

No fake `CPU 98%`, `512 MB per process`, PID or background task is
allowed. Resource accounting must be measured by the actual allocator,
page table and scheduler before surfacing it in Task Manager.

## Workstreams — parallel agent ownership

| Workstream | Files / area owned | Deliverable, acceptance |
|---|---|---|
| UI / Task Manager | `src/apps/*`, `src/shell/*`, `src/graphics.rs`, `src/ui/*` | Launch/focus/minimize, taskbar Start/Tasks, no fake PIDs, host hit-tests + QEMU screenshot |
| Process & thread kernel | `src/kernel/process/*`, `src/services/process_manager.rs`, `src/services/task_snapshot.rs` | TID, lifecycle, ready queues, affinity guards, consistent snapshots, unit tests |
| Real context switch | isolated new `src/kernel/switch/*` and test feature | Save/restore full register context; two independent Ring3 threads yield in QEMU; traps recover |
| SMP bring-up | isolated `src/kernel/smp/*` + new test feature | Enumerate APIC IDs, start APs, per-CPU GDT/TSS/IST + idle thread, no races under QEMU `-smp 2` |
| Virtual memory | `src/kernel/paging.rs` and dedicated mapper | Per-process CR3, frame refcounts, W^X/NX, user-stack guards; full failure rollback |
| IPC & security | dedicated kernel IPC/capability modules | Typed handles, PID ownership, no arbitrary cross-process writes, denial tests |
| VMware SVGA | `src/video/*`, `src/drivers/gpu/*` | Safe PCI BAR mapping, FIFO, mode readback, rollback, GPU fence; not fake vsync |
| PE64 compatibility | `src/loaders/*`, `src/compat/*` | Validated sections, imports, relocations, sandboxed Windows ABI; never kernel execution |
| Build / QA | `scripts/*`, `.github/workflows/*` | WSL/Windows builder reproducible, VMware UEFI smoke, QEMU fault and graphics evidence |

**Integration boundary:** `src/main.rs`, `src/lib.rs`, `Cargo.toml` and
shared workflow YAML are cross-cutting. Only the integration task merges
changes to these simultaneously. Before each commit fetch the current
`main` HEAD, compare expected blob SHAs, create a non-forced commit,
then run CI. Never overwrite another chat's concurrent work.

## Sequential implementation milestones

### M0 — already demonstrable
- Limine/UEFI boots an x86-64 Rust `no_std` kernel.
- Desktop + windows + Mac-like Dock / Windows-like Start and Tasks.
- Ring 3 embedded fixture `write/exit`, user guard page #PF and access
  denial on a present kernel page in QEMU.
- Pure no_std ProcessTable, ThreadTable, CPU0 selection policy and
  read-only TaskSnapshot. **No continuous user-process execution.**

### M1 — cooperative two-thread native execution
- Allocate own kernel stacks and user stacks for each TID.
- Save/restore callee-saved and full trap-frame registers.
- `SYS_YIELD` trap with deterministic A/B/A/B serial trace.
- Confirm TID + PID separation and same-process address-space sharing.
- On user #PF, only the failing thread/process exits; original kernel
  continuation remains alive. No timer scheduling yet.

### M2 — preemptive one-core scheduler
- PIT/APIC timer, monotonic ticks and explicit EOI.
- Kernel/user interrupt-safe locking, no allocation in IRQ.
- Fair scheduler with priorities, idle thread, timer quantum and sleep/wake.
- QEMU stress test yields correct output and stable task accounting.

### M3 — SMP (multiple CPUs)
- ACPI MADT parsing, LAPIC/x2APIC IDs and rendezvous.
- Boot application processors only after GDT/TSS/IST and stack mapping.
- Per-CPU runqueues and work-stealing under protected synchronization.
- Stable test with QEMU `-smp 2`, `-smp 4`; default VMware remains 1 vCPU
  until SMP test gates pass.
- Memory barriers, atomics and TLB shootdowns are mandatory before
  concurrent page-table modification.

### M4 — real apps and observability
- ELF64 process loader, dynamic memory allocation and isolated process
  handle tables, privileges and IPC.
- Move Task Manager/Window Server into unprivileged processes.
- Live process tree, thread list, CPU time, real page/RSS counters, resource
  limits, per-core graphs; only then offer End Task with permissions.
- Load a controlled Win64 console PE through a separate compatibility
  subsystem, not by assuming Windows kernel ABI compatibility.

## Design constraints

- W^X: no writable+executable user pages.
- User faults never escalate to kernel panic by default; kernel faults
  still fail closed.
- Kernel code executes at Ring 0; untrusted application code at Ring 3.
- Each CPU has distinct kernel stacks and descriptor tables before SMP.
- IPC validates sender identity, handle rights, lengths and ownership.
- Interrupt handlers never block on ordinary mutexes.
- PCI MMIO requires authoritative BAR sizes and safe caching policy;
  no direct guest writes to untrusted physical addresses.
- Never silently enable network, microphones, filesystem writes, disk
  installation or hardware 3D acceleration on an experimental ISO.

## Windows build/run

```powershell
cd M:\projects\ZillaOs
git pull --ff-only origin main
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows.ps1 -Action BuildRun
```

The VM must be fully powered off rather than suspended when replacing
the mounted development ISO. The QEMU GUI CI captures serial logs and
screenshots; the real VMware video and input paths must still be checked
on the Windows host.
