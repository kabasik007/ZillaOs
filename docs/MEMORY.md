# ZillaOS — early physical RAM manager (M2 foundation)

**State:** initial code committed directly to `main`. Rust/QEMU boot is **NOT VERIFIED** as of 2026-10-19.

## Boot sequence

```text
Limine 12.9.4 / base revision 6
   | memory map (max 256 copied entries), HHDM offset
   v
CPUID: physical address width, virtual 48/57-bit width, NX feature
   v
FrameAllocator::from_regions_with_physical_bits()
   | validates zero length, overflow and safe overlap rules
   | reserves every non-USABLE region (including ACPI and modules)
   | only allocates type-0 usable RAM, aligned to 4096 bytes
   v
reserve two unique physical 4 KiB frames
   v
map via Limine HHDM (not guessed identity mapping)
   | write/read two different deterministic 4096-byte patterns
   | compare every u64 word; zero both frames after validation
   v
serial: ZILLAOS_FRAME_PROBE_OK / ZILLAOS_MEMORY_READY
```

## Corrected Limine memory-map semantics

The Limine specification explicitly states:
- Entries are sorted by base; usable and bootloader-reclaimable regions are 4 KiB aligned.
- USABLE regions are guaranteed **not** to overlap any other entry.
- Non-USABLE entries **may** overlap each other and need not be aligned.

We therefore reject overlaps involving any usable region, but allow overlap between non-usable entries. This removes a boot-blocking false-positive from the initial allocator.

The firmware map is considered trusted during boot and copied to a fixed array before bootloader-reclaimable memory could ever be repurposed.

## Guarantees and limits

- Capacity: 256 entries; maps exceeding this fail explicitly (never silently truncated).
- Conservative physical limit: 32 to 52 address bits from CPUID. Frames above the reported address limit are never returned.
- Lowest 4 KiB physical page is never allocated.
- Only type 0 RAM can be reserved. ACPI, bootloader reclaimable, firmware, framebuffer and unknown types cannot be allocated.
- No double allocation from a single allocator instance; no free/reclaim yet.
- Two-page HHDM experiment writes only within two specifically reserved, exclusive and mapped usable pages; both are cleared afterward.
- This test **does not** establish RAM ECC correctness, physical memory isolation, safe DMA, full MMU control, or protection from a malicious bootloader.
- The allocator remains bootstrap-local: later subsystems must receive shared ownership of a singleton frame manager, not independently reconstruct it from the original map.
- The HHDM exists only because Limine maps it; the eventual native page-table manager must install its own mappings before bootloader page tables can be reclaimed.

## Tests (written, not yet executed end-to-end)

```bash
cargo test --lib --target x86_64-unknown-linux-gnu --offline
bash scripts/build.sh test
python3 scripts/smoke_test.py --timeout 90
```

The serial smoke test accepts PASS only when it sees a unique CPU physical-width line, nonzero usable frame count, two different page-aligned addresses below CPU's physical limit, a valid HHDM offset, the exact frame-probe success marker, the memory-ready marker, the boot marker and QEMU exit code 33.

See [PAGING.md](PAGING.md) and [VALIDATION.md](VALIDATION.md) for remaining safety work.
