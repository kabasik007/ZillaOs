# ZillaOS Paging Audit — milestone 1 (read-only)

## Scope and limitations

The existing `src/kernel/paging.rs` contains metadata for page-table
entries. Milestone 1 adds an independent, host-testable x86-64 walker
at `src/kernel/page_table_walk.rs` for 4-level and 5-level paging,
4 KiB/2 MiB/1 GiB mappings, and inherited USER/RW/NX flags.

An explicitly opt-in `src/page_table_observer.rs` reads active CR3
and CR4 from Ring 0, validates every page-table physical address against
the Limine memory map, then reads only HHDM-mapped table entries. It
observes kernel code, current boot stack and one exclusively reserved
USABLE frame. It rejects unexpected USER mappings and missing required
permissions. It does not modify any PTE, remap memory or switch CR3.

**This is not process isolation.** The observer's proof applies only to
three sampled virtual addresses, not every existing mapping. In
particular, inherited HHDM mappings can be supervisor RWX. GDT/TSS,
syscall trap, independent address spaces, process execution and
fault recovery are still missing.

Limine protocol base revision 6 maps BOOTLOADER_RECLAIMABLE and USABLE
RAM via the HHDM, and says its active page tables are in bootloader-
reclaimable memory. **Never free or overwrite these tables** while
CR3 references them. Other physical addresses must not be speculatively
dereferenced. This feature only reads page-table pages belonging to
the current mapped memory regions.

## Local validation

```bash
cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features paging-audit
bash scripts/build.sh audit
python3 scripts/smoke_test.py --iso dist/zillaos-0.0.2-dev-audit-uefi.iso --timeout 90
```

When the opt-in ISO passes QEMU, the serial log contains
`ZILLAOS_PAGING_AUDIT_CR3=0x...`, three page sizes,
`ZILLAOS_PAGING_AUDIT_OK` and `ZILLAOS_BOOT_OK`. Without the
feature, the original boot and desktop paths are unchanged.

## Next hardware implementation checkpoint

1. Create a single shared physical frame owner with explicit ownership
   and a teardown protocol (current early allocator is bump-only).
2. Allocate kernel-owned, zeroed page-table frames using the verified
   HHDM; clone only safe supervisor mappings, preserving cache flags.
3. Validate independent CR3 mappings for kernel code, IDT, stack,
   HHDM, firmware and framebuffer before atomic CR3 switch.
4. Install GDT/TSS and guarded Ring 0 stack, complete IDT user-fault
   handlers, and only then map user RX/RW pages.
5. Prove a fixed Zilla Native test can enter Ring 3, call write/exit,
   and return to desktop without kernel panic. PE64 remains blocked.

Reference: https://github.com/limine-bootloader/limine-protocol/blob/trunk/PROTOCOL.md
