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
