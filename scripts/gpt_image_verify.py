#!/usr/bin/env python3
"""Read-only GPT verifier for *regular raw image files*, not VMDK/devices.

Checks primary AND backup GPT; rejects mismatches and invalid geometry.
Never opens with write permissions, never invokes partitioning tools.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import stat
import struct
import sys
import zlib


class GptInvalid(ValueError):
    pass


def require(ok: bool, message: str) -> None:
    if not ok:
        raise GptInvalid(message)


def crc(data: bytes) -> int:
    return zlib.crc32(data) & 0xFFFFFFFF


def verify_image(path: Path, sector_size: int = 512) -> dict:
    require(sector_size in (512, 4096), "sector size must be 512 or 4096")
    require(path.suffix.lower() in (".img", ".raw", ".bin"),
            "only raw .img/.raw/.bin images accepted; VMDK not supported")
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        fd = os.open(path, flags)
    except OSError as exc:
        raise GptInvalid(f"cannot open image safely: {exc}") from exc
    try:
        st = os.fstat(fd)
        require(stat.S_ISREG(st.st_mode), "not a regular image file")
        require(st.st_size >= 128 * sector_size, "image too small")
        require(st.st_size % sector_size == 0, "unaligned image size")
        total = st.st_size // sector_size

        def read_sector(lba: int) -> bytes:
            require(0 <= lba < total, "sector outside image")
            data = os.pread(fd, sector_size, lba * sector_size)
            require(len(data) == sector_size, "short image read")
            return data

        mbr = read_sector(0)
        require(mbr[510:512] == b"\x55\xaa", "protective MBR signature missing")
        protective = 0
        for i in range(4):
            entry = mbr[446 + 16 * i:462 + 16 * i]
            kind = entry[4]
            if kind == 0xEE:
                require(struct.unpack_from("<I", entry, 8)[0] == 1,
                        "protective MBR must start at LBA 1")
                require(struct.unpack_from("<I", entry, 12)[0] == min(total - 1, 0xFFFFFFFF),
                        "protective MBR capacity mismatch")
                protective += 1
            else:
                require(entry == bytes(16), "hybrid or malformed MBR")
        require(protective == 1, "exactly one protective MBR entry required")

        def parse_header(lba: int, opposite: int) -> dict:
            h = read_sector(lba)
            require(h[:8] == b"EFI PART", "missing GPT signature")
            revision, size, expected_crc, reserved = struct.unpack_from("<4I", h, 8)
            require(revision == 0x10000 and 92 <= size <= sector_size and reserved == 0,
                    "unsupported or malformed GPT header")
            require(h[size:] == bytes(sector_size - size), "nonzero GPT header padding")
            image = bytearray(h[:size])
            image[16:20] = b"\0" * 4
            require(crc(image) == expected_crc, "GPT header CRC mismatch")
            own, alt, first, last = struct.unpack_from("<4Q", h, 24)
            require(own == lba and alt == opposite, "GPT header LBA mismatch")
            require(2 <= first <= last < total - 1, "GPT usable range invalid")
            guid = h[56:72]
            require(guid != bytes(16), "empty disk GUID")
            array_lba, count, entry_size, array_crc = struct.unpack_from("<QIII", h, 72)
            require(count == 128 and entry_size == 128, "unsupported GPT entry layout")
            array_sectors = 16384 // sector_size
            require(2 <= array_lba <= total - array_sectors - 1,
                    "GPT entry array outside disk")
            if lba == 1:
                require(array_lba + array_sectors <= first, "primary array overlaps usable data")
            else:
                require(array_lba >= last + 1 and
                        array_lba + array_sectors <= total - 1,
                        "backup array overlaps usable data")
            entries = b"".join(read_sector(array_lba + i) for i in range(array_sectors))
            require(crc(entries) == array_crc, "GPT entry-array CRC mismatch")
            return dict(guid=guid, first=first, last=last,
                        array=entries, array_crc=array_crc, array_lba=array_lba)

        primary = parse_header(1, total - 1)
        backup = parse_header(total - 1, 1)
        require(primary["guid"] == backup["guid"] and
                primary["first"] == backup["first"] and
                primary["last"] == backup["last"], "primary/backup GPT header disagreement")
        require(backup["array_lba"] == total - 1 - 16384 // sector_size,
                "backup GPT array at unexpected LBA")
        require(primary["array"] == backup["array"],
                "primary and backup partition entries differ")

        partitions: list[dict] = []
        used_guids: set[bytes] = set()
        for n in range(128):
            e = primary["array"][n * 128:(n + 1) * 128]
            if e[:16] == bytes(16):
                require(e == bytes(128), "unused GPT entry contains data")
                continue
            unique = e[16:32]
            first, last = struct.unpack_from("<QQ", e, 32)
            require(unique != bytes(16) and unique not in used_guids,
                    "duplicate or missing partition GUID")
            used_guids.add(unique)
            require(primary["first"] <= first <= last <= primary["last"],
                    "partition outside usable range")
            require(all(last < p["first_lba"] or first > p["last_lba"]
                        for p in partitions), "overlapping partitions")
            require(len(partitions) < 32, "too many partitions for current ZillaOS inspector")
            partitions.append(dict(index=n + 1, first_lba=first, last_lba=last,
                                   sectors=last - first + 1))
        return dict(status="valid", image=str(path), sector_size=sector_size,
                    total_sectors=total, disk_guid=primary["guid"].hex(),
                    primary_and_backup_match=True, partitions=partitions,
                    warning="Read-only inspection, not authorization to install")
    finally:
        os.close(fd)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path, help="regular raw .img/.raw/.bin image")
    parser.add_argument("--sector-size", type=int, choices=(512, 4096), default=512)
    args = parser.parse_args()
    try:
        print(json.dumps(verify_image(args.image, args.sector_size), indent=2))
        return 0
    except GptInvalid as exc:
        print(f"GPT INVALID: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
