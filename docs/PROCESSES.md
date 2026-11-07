# ZillaOS — native ProcessManager, milestone 0

## Status

The first committed slice is **host-testable process metadata**, NOT an
isolated running process. The GUI remains the existing built-in application
preview. All external execution remains disabled. No Windows EXE is launched.

The foundation lives in `src/kernel/process/` and
`src/services/process_manager.rs`. It adds:

- Monotonic nonzero PID allocation and a fixed limit of 16 process records.
- Created → Ready → Running → Exited → reaped lifecycle **model**.
- Exit reason and user-fault records, without claiming hardware recovery.
- Deterministic cooperative round-robin selection of ready process IDs.
- Lower-canonical x86-64 user VA policy (`0x10000..0x800000000000`),
  4 KiB aligned non-overlapping regions, maximum 32 regions / 256 MiB.
- An explicitly unmapped planned guard page below a writable user stack.
- W^X policy at metadata validation time; executable entry must map RX.
- A native v0 syscall **validation contract**: `write(1, buf, size)`,
  `exit(code)`, strict range/length checks. No trap handler or dereference
  of user pointers is installed.
- A service gateway that consistently refuses to execute a process until
  hardware isolation is available. `ApplicationManager` remains unchanged.

`ProcessState::Running` denotes a bookkeeping transition in host tests,
**not** proof that CPU instructions ran at Ring 3.

## Next prerequisites for a real native process

1. Keep a single global owner of the existing physical frame allocator,
   including teardown/reclamation and clear ownership of page-table frames.
2. Build independent kernel-owned x86-64 tables and a safe CR3 transition.
   Validate HHDM and kernel code/stack/IDT mappings before switching.
3. Install GDT with user code/data selectors, TSS.rsp0 and guarded kernel
   stack, a recoverable IDT and a safe interrupt/syscall entry frame.
4. Install real U/S and NX permissions, user text + stack frames,
   unmapped guard page and copy-from-user fault handling. Check CPU NXE.
5. Add controlled IRETQ entry to a minimal Zilla Native code fixture
   (not a Windows program), kernel-controlled `write/exit` trap,
   user #PF/#GP/#UD termination and return to the desktop without panic.
6. Add a no-heap ELF64 static loader after fixed-code-fixture execution
   works; verify exact ABI and input bounds.
7. Only after the native Ring 3 smoke passes: PE64 image mapper,
   relocations/imports, then the smallest tested Win32 console ABI.

Do not import arbitrary EXE files from Files/App Center while the
hardware boundary is incomplete. Do not execute PE payloads in Ring 0.

## Local/CI checks

```bash
cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features vm-demo
cargo +1.90.0 fmt --all -- --check
```

The existing boot + desktop QEMU workflow is manual; the new
`process-foundation.yml` runs narrow compile/test checks on changes
to process modules. It does not prove Ring 3 or VMware execution.

## Milestone 1 — opt-in active paging observer

A new read-only page-table walker and an opt-in Limine CR3 observer
are documented in [PAGING_AUDIT.md](PAGING_AUDIT.md). They validate
actual supervisor access to kernel text, stack and an owned RAM frame
without changing mapping permissions. This is **not** a kernel-owned
address space or an isolated Native Process; no Ring 3 entry exists yet.

## Milestone 3 — opt-in single Ring 3 native fixture

`native-ring3` uses the previously tested staged PML4. For disposable
QEMU-only testing, it installs its own GDT, TSS with RSP0/IST and IDT
with an `int 0x80` DPL3 gateway. The only user program is a 40-byte,
audited x86-64 fixture copied into a page marked RX. It writes a
message from the user RW/NX data page and invokes `exit(0)`. The
interrupt gateway verifies the originating CS privilege level and the
existing native syscall validation contract, then bounds-copies from
the known mapped data page. A saved kernel continuation restores the
original CR3 and original kernel stack. User #UD/#GP/#PF routes record
a fault; kernel-mode faults remain fatal.

```bash
bash scripts/build.sh ring3
python3 scripts/smoke_test.py --iso dist/zillaos-0.0.2-dev-ring3-uefi.iso --timeout 90
```

**Limitations:** Single-core, one embedded fixture only. Maskable interrupts
remain off; no preemption, no user ELF loader, no Windows PE loading,
no general safe copy-from-user, no teardown of allocated page tables,
no guarded TSS.RSP0 kernel stack, no multi-process isolation claims,
no GUI integration and no arbitrary binary execution. The normal
boot and Desktop stay unchanged. The console syscall uses software
`INT 0x80`, not the x86-64 `SYSCALL` instruction.

## Milestone 4 — guard-page negative test

`native-ring3-guard-fault` changes only the controlled fixture's last
instructions. Instead of `exit(0)` it attempts to read `0x7fe000`, the
unmapped guard page below its user stack. The CPU should deliver #PF
with CPL3. The Ring 0 trap records the fault and jumps back to the
saved kernel continuation; no user instruction after the fault resumes.

```bash
bash scripts/build.sh ring3-guard
python3 scripts/smoke_test.py --iso dist/zillaos-0.0.2-dev-ring3-guard-uefi.iso --timeout 90
```

Expected serial evidence is `ZILLAOS_NATIVE_USER_FAULT_VECTOR=0x...0e`,
`ZILLAOS_NATIVE_USER_GUARD_FAULT_OK`, and normal kernel
`ZILLAOS_BOOT_OK`; no fatal kernel exception. This test does not prove
every user/kernel permission boundary or general fault recovery.

## Milestone 5 — present kernel-page privilege boundary

The opt-in `native-ring3-kernel-fault` fixture attempts to read the
actual mapped higher-half `_start` kernel code page from CPL3.
Expected #PF has `P=1`, `U/S=1`, `W/R=0`, proving that the present
supervisor PML4 mapping rejects this user read. The handler must still
recover to the saved kernel continuation without a panic.

```bash
bash scripts/build.sh ring3-kernel
python3 scripts/smoke_test.py --iso dist/zillaos-0.0.2-dev-ring3-kernel-uefi.iso --timeout 90
```

The success marker is `ZILLAOS_NATIVE_USER_KERNEL_ACCESS_DENIED`.
The controlled test checks one supervisor mapping and one CPU only;
it is not a claim of hardened kernel isolation against arbitrary code.

## Milestone 6 — two actual user contexts with cooperative yielding (opt-in)

`native-ring3-threads` adds a separate QEMU-only binary that enters two
different CPL3 entry points inside the **same** isolated PML4, each with its
own mapped RW/NX 4KiB user stack and a separate unmapped stack guard.
The `INT 0x80` trap saves 14 GP registers and the 5-qword user IRET frame;
`SYS_YIELD=3` swaps the saved frame with the next CPU0 Ready TID selected
by `ThreadTable`. The test program checks its preserved RBX/R12 counter,
voluntarily yields three times in each thread (expected trace `ABABAB`),
and both threads call `SYS_EXIT=2` without causing a kernel panic.
The existing saved supervisor continuation restores the original CR3 and
kernel stack after both finish. No ELF/PE files are executed.

```bash
bash scripts/build.sh ring3-threads
python3 scripts/smoke_test.py --iso dist/zillaos-0.0.2-dev-ring3-threads-uefi.iso --timeout 90
```

This is **true cooperative user context switching**, not timer preemption,
SMP, arbitrary thread creation or a production ProcessManager backend.
Only CPU0 is online, interrupts remain masked and all code is a fixed,
audited 42-byte fixture. The GUI Desktop is unaffected.

## Milestone 7 — isolated xAPIC timer preemption experiment

The optional `native-ring3-preempt` QEMU ISO runs two independent user
threads that never call `SYS_YIELD`. The kernel prepares a new PML4
with two separate user RW/NX stacks, guard pages, and **one supervisor-
only uncached APIC MMIO page at VA 0x007fb000** (U/S=0, PCD/PWT=1,
NX=1). This mapping is installed in the owned staged page tables, never
into Limine's original root.

Before user execution, CPU0 temporarily switches to the staged root
with IF=0, configures the local xAPIC MMIO timer, then restores the
original CR3. Entry to CPL3 specifically uses
`enter_staged_with_timer()`, which sets `RFLAGS.IF=1` on IRETQ.
The IDT vector 0x20 trap saves **all 15 GP registers and the 5-qword
CPL3 IRET frame**, acknowledges via xAPIC EOI MMIO offset 0xB0,
and forcibly switches to the other ready user thread.

Eight real timer ticks must produce the trace `ABABABAB`. On each
interrupt the dispatcher verifies that the interrupted thread's RBX
counter increased, its distinct R12 sentinel is intact, CS/SS select
CPL3, RFLAGS has IF and no IOPL/DF, and RIP/RSP remain in the
thread's own code and stack. After tick eight, the old kernel
continuation restores its CR3 and stack, disables the test timer
under a temporary staged CR3, and reports success.

```bash
bash scripts/build.sh ring3-preempt
python3 scripts/smoke_test.py --iso dist/zillaos-0.0.2-dev-ring3-preempt-uefi.iso --timeout 45
```

**Strict limits:** one bootstrap CPU, two preloaded fixed x64 spin loops,
no scheduler priority policy, no timer clock calibration, no user FPU/SIMD
context, no kernel preemption, no multi-process CR3 switches, no APIC
discovery or SMP, no production Task Manager integration. An observed
timer tick is not a CPU percentage or a reliable millisecond.

### Failure history and recovered root cause

- The initial PIT/8259 test reached Ring3 and timed out without IRQ
  evidence. The next PIC route diagnostic reported LAPIC enabled and
  PIC IRQ masks originally 0xff. This did *not* establish that the
  UEFI PIC configuration was incapable of working.
- Review found a deterministic bug: preemption called the ordinary
  `enter_staged()` with `RFLAGS.IF=0`, so **all maskable IRQs were
  disabled**. The timer experiment now uses the dedicated IF-enabled
  entry; non-timer QEMU paths remain with IF=0.
- An attempted x2APIC MSR backend compiled, but QEMU TCG's
  `-cpu max` did not advertise CPUID.x2APIC, and the fixture rejected
  it safely without touching unavailable MSRs.
- The current experiment uses xAPIC MMIO instead, with a validated
  supervisor-only uncached mapping and a temporary Ring0 CR3 switch.
  Do not count it as successful unless the dedicated QEMU CI job
  confirms eight interrupts, both threads' forward progress, and
  a clean return.

### CI dependency boundary

The Process Foundation workflow tracks the process modules and the
shared `src/shell/personalization.rs` source, since any `no_std`
library compile failure blocks both GUI and non-GUI kernels. A
concurrent personalization change once produced an undefined variable
and broke process builds despite not editing process files.
