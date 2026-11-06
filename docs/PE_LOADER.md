# ZillaOS — PE32+ x64 image mapper, first host-tested slice

## Scope

`src/loaders/pe64.rs` reuses the existing strict
`compat::windows_pe::inspect_pe64()` header inspector. It maps a
controlled AMD64 PE32+ file into a **caller-owned byte buffer** in
`no_std` Rust, without allocating or executing any file instructions.

It validates:

- PE32+ AMD64 and structural headers via the shared parser.
- A bounded maximum image of 16 MiB; destination size and overflow.
- 4096-byte section alignment and supported PE file alignments.
- Virtual section overlap, including page-rounded overlap.
- Read/write/execute metadata and rejection of W+X sections.
- Entry point contained in an executable section with raw initialized bytes.
- Zero-filled image gaps and uninitialized section tails.
- Relocation blocks: supports only DIR64 and ABSOLUTE entries,
  verifies block sizes, target alignment/ranges, arithmetic overflow.
- Preferred base vs relocated base, requires relocation directory if changed.
- **Fail closed for any import directory** until DLL dependency resolution
  and an isolated Win32 runtime are implemented.

On error the output buffer may contain partial bytes: callers must
discard it. The mapper only returns metadata; it does not assign
actual CPU page permissions, switch CR3, resolve imports or run code.

The Ring 3 native Hello fixture is **not** a PE loader. The Windows
`.exe` milestone is **not achieved** by this commit. Do not connect
`ApplicationManager::ForeignBinary` to the mapper until section PTE
installation, robust import handling, independent process lifecycle and
fault containment are independently tested.

## Host and bare-metal tests

```bash
cargo +1.90.0 test --lib --target x86_64-unknown-linux-gnu --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline
cargo +1.90.0 build --release --target x86_64-unknown-none --offline --features vm-demo
```

Synthetic fixtures check initialized data, BSS zero fill, W^X,
non-executable entry, imports rejected, missing relocations, DIR64
rebasing, malformed relocation blocks, wrong relocation types and
overlapping sections.

## Next steps

1. Build stricter PE64 import-table parsing and a resolver returning
   explicit errors for unavailable DLLs and symbols.
2. Add a `MappedImage` to user page-table installer with W^X/NX
   enforced by hardware, not just metadata.
3. Validate the real imported functions of a controlled MinGW PE64
   console Hello fixture.
4. Implement a small Windows userspace ABI bridge for
   `GetStdHandle`, `WriteFile`, `ExitProcess`, not a generic DLL
   registry in Ring 0.
5. Only after hardware-user execution is tested: run the controlled
   Windows EXE in a new process and return exit code 0.
