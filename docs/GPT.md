# ZillaOS GPT Inspector — primary + backup GPT (read-only)

**2026-10-28:** `src/gpt.rs` has been committed to `main` as a
standalone `no_std` library and exported by `src/lib.rs`.
The optional strict API also checks the backup GPT header and table. The Rust
unit tests are included in source but **have not been run** in this
execution environment (no `rustc` / `cargo` / QEMU available).

## Host-side executable GPT inspection (Python)

A separate [`scripts/gpt_image_verify.py`](../scripts/gpt_image_verify.py)
performs strict primary+backup GPT checks on **ordinary raw image files**,
independently of the Rust kernel. It is intentionally not a VMware VMDK
parser or a Windows physical disk reader. It opens only regular files with
`O_RDONLY` and `O_NOFOLLOW` (on WSL/Linux), rejects unexpected
extensions, and never writes sectors.

```bash
# No Rust needed for this host-only regression:
python3 -m unittest tests.test_gpt_image_verify -v

# Inspect a raw test image you already own (not .vmdk):
python3 scripts/gpt_image_verify.py /path/to/disposable-test.img --sector-size 512
```

The output is a JSON report (status, sector size, partition bounds and
integrity status), **not permission to install or modify the image**.
Five test cases for 512/4096-byte GPT, corrupt backup, valid-but-different
backup array, refusal of symlinks/VMDK, and unchanged read-only image
passed in a local Python runtime, 2026-10-28. Matching exact Git blob
hashes were confirmed after writing both files to `main`.
Kernel Rust/QEMU/VMware execution remains unverified. The Python tests
are independent; they do not establish that the Rust parser compiles.

## Purpose

The parser inspects the GPT of a hypothetical block device through
a caller-provided callback of the form

```rust
inspect_primary_gpt(
    logical_sector_size,
    total_sectors,
    |lba, destination: &mut [u8]| -> Result<(), ()> {
        // Future read-only, capacity-checked block driver copies one sector
        // into destination. No disk is available to ZillaOS yet.
        unimplemented!()
    },
);
```

There is **no block-device driver or real disk access** behind this API.
It cannot enumerate or select disks, write sectors, create partitions,
format devices, persist user settings or install a bootloader.

## APIs

- `inspect_primary_gpt(...)` — checks the protective MBR and primary GPT
  header + table. This is the compatibility/diagnostic API; it does **not**
  guarantee a valid backup GPT.
- `inspect_complete_gpt(...)` — calls the primary checker, then verifies
  the final-LBA backup GPT header (CRC32, disk GUID, opposite header LBAs,
  identical usable range and table geometry) and independently reads and
  CRC-checks the backup entry array. Both copies must report the same
  partition-table CRC. This is the preferred inspection API for future
  read-only disk-device integration.

Both APIs receive only a read callback. Neither has a write capability.

## Validation performed by the parser

- Accepts only a *trusted input geometry* of 512 or 4096 bytes per
  sector with at least six sectors. Out-of-bounds/failed reads abort.
- Reads LBA 0 and verifies a protective MBR signature, one 0xEE entry,
  starting at LBA 1 and spanning the known device size (capped at
  0xFFFFFFFF); rejects hybrid MBR and unexpected extra MBR entries.
- Reads the **primary** GPT header at LBA 1, verifies signature
  `EFI PART`, revision `0x00010000`, header size and reserved area.
- Validates the header CRC32 with its CRC field treated as zero.
- Validates current and backup header LBA, disk GUID, usable LBA range
  and separate forward/backward GPT table reservations.
- Requires standard 128-byte partition entries in a 16-KiB array
  (128 entries). Larger or atypical layouts are rejected, not guessed.
- Streams the entire primary entry-array CRC32 through a single 4-KiB
  scratch buffer; no heap allocations.
- Rejects nonzero unused entries, missing partition GUIDs, out-of-range
  partitions, overlapping spans, duplicate partition identities, and
  more than 32 populated partitions.
- `inspect_complete_gpt` additionally rejects missing/corrupt backup
  headers, misplaced backup arrays, CRC errors, and valid-but-inconsistent
  backup table content.
- Never silently repairs a corrupted GPT, reconstructs an alternate
  header, or treats a damaged primary header as safe to install over.

### Explicit limitations

`inspect_primary_gpt` does not validate backup contents; the new
`inspect_complete_gpt` **does** validate backup headers and tables.
The strict mode still does **not** detect in-use filesystems, BitLocker, ext4, NTFS, EFI signatures inside
partitions, hidden/recovery partitions or whether the disk is empty.
A successful primary GPT inspection **does not authorize installation**.

The parser intentionally rejects some otherwise valid GPT disks that
have nonstandard array sizes or over 32 partitions. This conservative
behavior is appropriate for an early-stage preview, not production.

## Host-only tests (no disks involved)

The module includes synthetic in-memory images for **512** and **4096**
byte sector devices. Tests cover:
- Valid protective MBR, GPT header CRC32 and entry-array CRC32.
- Two valid, separate partitions and complete matching primary/backup tables.
- Incorrect MBR signature, hybrid MBR or protective range.
- Invalid GPT signature, header/padding corruption and stale CRCs.
- Invalid partition entry bytes even when the table CRC is recalculated.
- Absent backup header, backup CRC corruption, shifted backup array,
  mismatched disk identity, mismatched but independently CRC-valid arrays,
  and failure to read the backup sector.
- Overlapping partitions and out-of-range LBAs despite valid CRCs.
- Malicious partition count, unsupported logical sector size and
  failed read callbacks.
- Reference CRC32 value `123456789 → 0xCBF43926`.

Run after installing Rust in WSL2:

```bash
cd /mnt/m/projects/ZillaOs
git pull origin main
source "$HOME/.cargo/env"
cargo test --lib --target x86_64-unknown-linux-gnu --offline
```

**Do not attach physical Windows disks to the VMware test machine.**
When safe read-only block-driver support exists, a separate disposable
virtual disk image can be attached and its GPT verified. Until then
the Setup storage page continues to show **INSTALLATION DISABLED**.

Related: [Storage inventory](STORAGE.md),
[Installation architecture](INSTALLATION.md),
[VMware guide](VMWARE.md).

Source: [UEFI 2.11 GPT disk layout](https://uefi.org/specs/UEFI/2.11/05_GUID_Partition_Table_Format.html)
