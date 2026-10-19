# Early physical memory management (M2 foundation)

**Status: source implemented in main, execution NOT VERIFIED.**
This is **not** a complete virtual memory manager, general allocator, or
production frame manager. No physical pages are mapped, zeroed or freed yet.

## Data flow

```text
Limine base revision 6
   | memmap response (bootloader-reclaimable structures)
   v
src/limine.rs: memory_map()
   | copy up to 128 entries into kernel-owned fixed array
   v
zillaos_kernel::memory::FrameAllocator::from_regions()
   | validate ranges, reject overlaps and 64-bit overflow
   | keep usable type=0 pages, reserve page zero and other types
   | align to 4096-byte boundaries, sort ascending
   v
reserve_frame() -> PhysicalFrame{address}
   | returns 4096-byte-aligned unique *physical address*
   | NO dereferencing or MMU configuration
   v
two reserved test frames, serial "ZILLAOS_MEMORY_READY"
```

## Safety and compatibility assumptions

- The Limine pointer graph is trusted only during boot. It is copied before
  any future attempt to reclaim bootloader memory.
- Count limit 128 is explicit and deliberately bounded for early boot; a
  larger map is rejected rather than silently truncated.
- Any unknown memory type is reserved. Only `LIMINE_MEMMAP_USABLE` (type 0)
  can produce frames. Bootloader reclaimable, executable, ACPI, framebuffer,
  and reserved ranges are **not** included.
- Zero-length ranges, overlapping ranges (including overlaps between usable
  and reserved), integer overflow, and physical addresses above 52 bits are
  rejected rather than guessing. The 52-bit bound is a provisional
  x86-64 ceiling, not runtime CPUID detection.
- The low first 4 KiB page is intentionally never allocated.
- Misaligned edge fragments are never included in returned frames.
- Allocation is **bump-only** across sorted usable ranges. There is no
  free, reclaim, reuse, zeroing, memory sanitization, ownership transfer,
  IOMMU or page table API yet.
- A frame **address** is not a Rust pointer. A future paging/HHDM module
  must safely map it before touching it. In particular, never assume a
  physical address is an identity-mapped virtual address.
- In the current boot demonstration, the allocator remains local to
  `_start`. Future kernel subsystems must not create independent allocators
  over the same boot map, or they could double-allocate frames.

## Host-side Rust tests

In Ubuntu/WSL2 after installing Rust:

```bash
cargo test --lib --target x86_64-unknown-linux-gnu --offline
```

Tests cover alignment, ordering, uniqueness, exhaustion, memory region
overlaps, unknown/reserved types, zero page exclusion, arithmetic overflow,
empty entries, physical limit and region count ceiling.

## Emulator smoke

```bash
bash scripts/build.sh test
python3 scripts/smoke_test.py --timeout 90
```

Expected on the serial port before `ZILLAOS_BOOT_OK`:

```text
ZILLAOS_MEMMAP_REGIONS=0x...
ZILLAOS_USABLE_FRAMES=0x...
ZILLAOS_TEST_FRAME_1=0x...
ZILLAOS_TEST_FRAME_2=0x...
ZILLAOS_MEMORY_READY
```

The test harness checks both `ZILLAOS_MEMORY_READY` and
`ZILLAOS_BOOT_OK` and QEMU's exit code 33. Build/boot gates have not yet
passed in this execution environment, and cannot be called complete.

## Requirements for full M2 verification (future)

1. Independently compile and run all Rust host tests.
2. Confirm QEMU/OVMF boots; capture serial log and build versions.
3. Determine physical address width using CPUID, rather than a fixed
   conservative limit.
4. Add verified page-table walker with explicit HHDM base and mappings.
5. Add guarded kernel stacks, exception vectors and a page-fault test.
6. Add ownership rules and a longer-lived synchronized physical allocator.
7. Prove that no page is allocated twice, and no reserved range is returned.

Official spec: https://github.com/limine-bootloader/limine-protocol/blob/trunk/PROTOCOL.md
