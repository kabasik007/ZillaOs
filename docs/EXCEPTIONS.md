# x86-64 exception foundation — ZillaOS 0.0.2-dev

**Status (2026-10-19): committed to `main`, build and QEMU execution NOT VERIFIED.**

## Implemented source

The bootstrap now loads its own static 256-entry IDT (16-byte gates) using
`LIDT`, borrowing Limine's active ring-0 `CS` selector. Only these vectors
have executable handlers:

| Vector | Name | CPU pushes an error code? | Diagnostic |
| --- | --- | --- | --- |
| 6 | #UD — invalid opcode | No | RIP |
| 13 | #GP — general protection | Yes | error code, RIP |
| 14 | #PF — page fault | Yes | error code, RIP, CR2 |

All three handlers stop execution and exit the QEMU `isa-debug-exit`
device with status 35; they do NOT resume the offending instruction,
recover a process, or attempt to schedule another task.

Assembly stubs follow the x86-64 exception stack layout, capture RIP and
error code before normalizing the stack alignment to the SysV calling
convention and calling Rust `zilla_exception_fatal`.
A pure Rust `no_std` library implements page-fault error-flag decoding
for unit tests, avoiding any assumption that actual faults have occurred.

## Fault-mode integration test (planned execution)

```bash
git switch main
git pull origin main

python3 -m unittest discover -s tests -v
cargo test --lib --target x86_64-unknown-linux-gnu --offline

bash scripts/build.sh fault
python3 scripts/smoke_test.py --expect-fault --timeout 90
```

Unlike the normal boot test (`test` ISO), `fault` creates a separately
compiled ISO with `exception-self-test` enabled; `_start` executes the
invalid `UD2` instruction **after IDT installation**.

**PASS requires all of:**
- `ZILLAOS_IDT_READY` (exact serial line),
- `ZILLAOS_EXCEPTION_VECTOR=0x0000000000000006`,
- `ZILLAOS_EXCEPTION_ERROR=0x0000000000000000`,
- a real RIP value from the handler,
- `ZILLAOS_EXCEPTION_FATAL` (exact serial line),
- QEMU exit code **35**, not 33 or a timeout,
- absence of ordinary `ZILLAOS_BOOT_OK` and generic panic/failure markers.

A QEMU reset, triple fault or a nonzero exit without the expected serial
record is **FAIL**, not a successful exception test. In normal `test` mode,
QEMU must still exit 33 with its CPU/RAM/boot markers and `ZILLAOS_IDT_READY`.

## Explicit safety limitations

- No independent GDT/TSS or IST is installed: stack overflow or #DF can
  still lead to a triple fault. Do **not** deliberately test stack faults.
- Non-listed IDT entries are not initialized: the kernel still keeps
  external hardware interrupts disabled. This cannot be used for an
  ordinary device IRQ or userspace exception system yet.
- No ring-3 transition, privilege switch or reentrant handler is supported.
- Fatal handlers use the current stack and serial diagnostics; on a corrupt
  kernel stack they are not guaranteed to work.
- QEMU-specific exit port `0xf4` is not a production machine recovery path.
- This work does not yet justify describing M2 as complete, let alone M3.

## Next engineering milestones

1. Verify clean Rust formatting, cross-build and both normal/fault images
   in an actual QEMU/OVMF run with saved logs.
2. Add a private GDT/TSS and dedicated guarded IST stacks.
3. Expand safe IDT coverage and establish IRQ masking/acknowledgment policy.
4. Introduce kernel-owned page tables, CR3 validation and #PF tests.
5. Only then consider process isolation and recoverable ring-3 faults.

See [ROADMAP](ROADMAP.md), [VALIDATION](VALIDATION.md) and
[official x86-64 architecture manual](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html).
