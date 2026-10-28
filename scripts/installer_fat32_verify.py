"""Independent read-only FAT32 ESP verifier for new GPT RAW lab images.

Checks BPB, FSInfo/backup, dual FAT, cluster chains and staged file bytes.
No writes, device handles, VMDK parsing or mount operations.
"""
from __future__ import annotations
import os
from pathlib import Path
import stat
import struct
from gpt_image_verify import verify_image

class Fat32Invalid(ValueError):
    pass


def verify_fat32_image(path: Path, sector_size: int = 512,
                       expected_files: dict[str, bytes] | None = None) -> dict:
    verified = verify_image(path, sector_size)
    partition = verified['partitions'][0]
    first = partition['first_lba'] * sector_size
    last = (partition['last_lba'] + 1) * sector_size
    flags = os.O_RDONLY | getattr(os, 'O_NOFOLLOW', 0) | getattr(os, 'O_CLOEXEC', 0)
    fd = os.open(path, flags)
    try:
        if not stat.S_ISREG(os.fstat(fd).st_mode):
            raise Fat32Invalid('raw regular file required')
        def take(pos: int, count: int) -> bytes:
            if count < 0 or pos < first or pos + count > last:
                raise Fat32Invalid('FAT access outside EFI partition')
            data = os.pread(fd, count, pos)
            if len(data) != count:
                raise Fat32Invalid('short read')
            return data
        boot = take(first, sector_size)
        bps = struct.unpack_from('<H', boot, 11)[0]
        spc = boot[13]
        reserved, fat_count = struct.unpack_from('<HB', boot, 14)
        total = struct.unpack_from('<I', boot, 32)[0]
        per_fat = struct.unpack_from('<I', boot, 36)[0]
        root = struct.unpack_from('<I', boot, 44)[0]
        if (bps != sector_size or spc == 0 or spc & (spc - 1) or
            reserved < 32 or fat_count != 2 or per_fat < 1 or root != 2 or
            boot[82:90] != b'FAT32   ' or boot[510:512] != b'\x55\xaa' or
            total != partition['sectors']):
            raise Fat32Invalid('invalid FAT32 boot parameter block')
        count = (total - reserved - fat_count * per_fat) // spc
        if not 65525 <= count < 0x0ffffff5 or (count + 2) * 4 > per_fat * bps:
            raise Fat32Invalid('invalid FAT32 capacity')
        fsinfo = take(first + bps, bps)
        if (struct.unpack_from('<I', fsinfo, 0)[0] != 0x41615252 or
            struct.unpack_from('<I', fsinfo, 484)[0] != 0x61417272 or
            struct.unpack_from('<I', fsinfo, 508)[0] != 0xaa550000):
            raise Fat32Invalid('invalid FAT32 FSInfo')
        if take(first + 6*bps, bps) != boot or take(first + 7*bps, bps) != fsinfo:
            raise Fat32Invalid('backup FAT32 boot/FSInfo mismatch')
        fat_pos = first + reserved * bps
        fat_bytes = per_fat * bps
        fats = take(fat_pos, fat_bytes)
        if fats != take(fat_pos + fat_bytes, fat_bytes):
            raise Fat32Invalid('FAT copies disagree')

        def fat(cl: int) -> int:
            if cl < 2 or cl >= count + 2:
                raise Fat32Invalid('invalid cluster number')
            return struct.unpack_from('<I', fats, 4 * cl)[0] & 0x0fffffff
        if fat(2) < 0x0ffffff8:
            raise Fat32Invalid('root cluster not allocated')
        data_start = first + (reserved + fat_count * per_fat) * bps
        cluster_bytes = bps * spc

        def read_chain(initial: int, size: int | None = None) -> bytes:
            chunks = []
            visited = set()
            cl = initial
            while True:
                if cl in visited or len(visited) > 8192:
                    raise Fat32Invalid('FAT cycle or oversized chain')
                visited.add(cl)
                chunks.append(take(data_start + (cl - 2) * cluster_bytes, cluster_bytes))
                nxt = fat(cl)
                if nxt >= 0x0ffffff8:
                    break
                if nxt == 0:
                    raise Fat32Invalid('free cluster in allocated chain')
                cl = nxt
            result = b''.join(chunks)
            if size is not None:
                if size > len(result) or (size > 0 and size < len(result) - cluster_bytes):
                    raise Fat32Invalid('file size/chain mismatch')
                return result[:size]
            return result

        def ls(cl: int) -> dict[str, tuple[int, int, bool]]:
            data = read_chain(cl)
            found = {}
            pending_lfn = None
            for j in range(0, len(data), 32):
                e = data[j:j+32]
                if e[0] == 0:
                    break
                if e[0] == 0xe5:
                    continue
                attr = e[11]
                if attr == 0x0f:
                    if e[0] != 0x41 or pending_lfn is not None:
                        raise Fat32Invalid('unsupported LFN chain')
                    encoded = b''.join(e[k:k+2] for k in
                        (1,3,5,7,9,14,16,18,20,22,24,28,30))
                    name = encoded.decode('utf-16le').split('\x00',1)[0].rstrip('\uffff')
                    if not name or len(name) > 13:
                        raise Fat32Invalid('invalid LFN')
                    pending_lfn = (name, e[13])
                    continue
                short = e[:11]
                name = (e[:8].decode('ascii').rstrip() +
                        ('.' + e[8:11].decode('ascii').rstrip() if e[8:11].strip() else ''))
                if pending_lfn is not None:
                    checksum = 0
                    for byte in short:
                        checksum = (((checksum & 1) << 7) + (checksum >> 1) + byte) & 255
                    if checksum != pending_lfn[1]:
                        raise Fat32Invalid('LFN short name checksum mismatch')
                    name = pending_lfn[0]
                    pending_lfn = None
                if name in found:
                    raise Fat32Invalid('duplicate directory name')
                start = (struct.unpack_from('<H',e,20)[0] << 16) | struct.unpack_from('<H',e,26)[0]
                size = struct.unpack_from('<I',e,28)[0]
                found[name] = (start, size, bool(attr & 0x10))
            return found

        entries = ls(root)
        present = {}
        if expected_files is not None:
            paths = {
                'EFI/BOOT/BOOTX64.EFI': ('EFI','BOOT','BOOTX64.EFI'),
                'BOOT/KERNEL.ELF': ('BOOT','KERNEL.ELF'),
                'LIMINE.CONF': ('LIMINE.CONF',),
            }
            if set(expected_files) != set(paths):
                raise Fat32Invalid('incomplete expected boot file map')
            for path, parts in paths.items():
                node = entries
                for component in parts[:-1]:
                    if component not in node or not node[component][2]:
                        raise Fat32Invalid('missing boot directory '+component)
                    node = ls(node[component][0])
                if parts[-1] not in node or node[parts[-1]][2]:
                    raise Fat32Invalid('missing boot file '+path)
                start, size, _ = node[parts[-1]]
                payload = read_chain(start, size)
                if payload != expected_files[path]:
                    raise Fat32Invalid('boot file content mismatch '+path)
                present[path] = len(payload)
        return {'fat32_valid': True, 'sector_size': bps,
                'cluster_bytes': cluster_bytes, 'cluster_count': count,
                'fat_copies_match': True, 'staged_files': present}
    finally:
        os.close(fd)

def main() -> int:
    import argparse
    import json
    import sys
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('image', type=Path)
    parser.add_argument('--sector-size', type=int, choices=(512,4096), default=512)
    parser.add_argument('--check-boot', action='store_true',
                        help='compare EFI files with current built ZillaOS assets')
    args = parser.parse_args()
    try:
        files = None
        if args.check_boot:
            from installer_lab import load_uefi_assets
            files = load_uefi_assets()
        print(json.dumps(verify_fat32_image(args.image, args.sector_size, files), indent=2))
        return 0
    except (Fat32Invalid, OSError, ValueError) as exc:
        print('FAT32 INVALID:', str(exc), file=sys.stderr)
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
