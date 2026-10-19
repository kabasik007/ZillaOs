# ZillaOS paging architecture — groundwork (v0.0.2-dev)

**Not yet an active page-table manager.** The `src/paging.rs` module implements pure, host-testable x86-64 address metadata: canonical 48/57-bit virtual addresses, PML5/PML4/PDPT/PD/PT indices and 4 KiB offsets, and a checked 64-bit non-leaf table entry representation.

## Constraints

- Do not interpret physical addresses as kernel pointers. Query the HHDM offset from Limine and use it **only** for mapped, owned USABLE frames.
- Validate physical addresses with CPUID, alignment and type before turning them into page-table entries.
- NX bits require CPUID NX support plus EFER.NXE enabled; the current kernel only **reports** NX support, it does not configure or enforce NX.
- Do not enable USER, GLOBAL, HUGE or device-memory mappings without explicit ownership/protection rules.
- A valid canonical virtual address is **not** proof that its page is mapped, executable, writable or belongs to a process.
- The current Limine page tables belong to the bootloader and must not be freed while active. Before reclaiming them, create and switch to kernel-owned page tables.

## Phased plan

1. **Metadata (implemented source):** test canonicality, calculate page indices, create checked non-leaf PTEs.
2. **Page-table observer:** read CR3; walk present boot paging structures without writing; bounds-check each HHDM address before reading.
3. **Kernel-owned tables:** reserve and zero page-table frames, install kernel and HHDM mappings with a documented cache/permission model.
4. **Switch:** load CR3 after mapping code, stack, IDT, framebuffer and all live boot structures; test negative cases.
5. **Fault handling:** create GDT/TSS/IDT and a robust #PF diagnostic; assert that a bad mapping reports CR2/error flags and does not silently corrupt kernel state.
6. **Processes:** isolated address spaces, separate user/supervisor flags, copy-to/from-user with recoverable failure semantics.
7. **Hardening:** enforce W^X/NX, capabilities, randomized mappings, IOMMU/DMA isolation and safe SMP TLB shootdowns.

## Host-only tests

```bash
cargo test --lib --target x86_64-unknown-linux-gnu --offline
```

Expected independent checks: 4/5-level canonical boundaries, invalid address rejection, exact 9-bit table indices, 4 KiB frame alignment, no unsupported huge-page flags or physical addresses above the CPU limit.

Official protocol reference: https://github.com/limine-bootloader/limine-protocol/blob/trunk/PROTOCOL.md
