#!/usr/bin/env python3
"""ZillaOS Installer Lab: GPT-only disposable sparse RAW images.

No host disk is opened. No existing file is changed. Creates new file only
in an isolated WSL/Linux HOME folder, only on explicit --create + typed confirmation.
Optional FAT32 ESP formatting and UEFI asset staging run only inside
the exclusively-created image. The root partition stays unformatted.
"""
from __future__ import annotations
import argparse
import json
import os
from pathlib import Path
import re
import stat
import struct
import sys
import uuid
import zlib

from gpt_image_verify import verify_image
from installer_fat32 import create_esp, Fat32Error
from installer_fat32_verify import verify_fat32_image
from boot_assets_audit import validate_assets

MIB = 1024 * 1024
GIB = 1024 * MIB
ESP_SIZE = 512 * MIB
RESERVE = MIB
CONFIRM = 'CREATE-NEW-VM-IMAGE'
NAME = re.compile(r'zillaos-[a-z0-9](?:[a-z0-9-]{0,46}[a-z0-9])?\.img\Z')
# Real UEFI ESP type; ZillaOS root uses a private experimental GUID.
ESP_TYPE = uuid.UUID('c12a7328-f81f-11d2-ba4b-00a0c93ec93b')
ROOT_TYPE = uuid.UUID('47a1acc0-65ef-4bd4-a29e-6415d07afa90')

class UnsafeRequest(ValueError):
    pass


def plan_image(size_gib: int = 12, sector: int = 512) -> dict:
    if type(size_gib) is not int or not 12 <= size_gib <= 64:
        raise UnsafeRequest('size_gib must be 12..64 (sparse; VM test only)')
    if sector not in (512, 4096):
        raise UnsafeRequest('sector must be 512 or 4096')
    total = size_gib * GIB // sector
    esp_first = RESERVE // sector
    esp_last = esp_first + ESP_SIZE // sector - 1
    root_first = esp_last + 1
    root_last = total - RESERVE // sector - 1
    if root_last - root_first + 1 < 8 * GIB // sector:
        raise UnsafeRequest('root partition would be too small')
    return dict(size_gib=size_gib, sector_size=sector, total_sectors=total,
                esp_first=esp_first, esp_last=esp_last,
                system_first=root_first, system_last=root_last,
                partitions=[dict(name='ZillaOS EFI', first_lba=esp_first, last_lba=esp_last),
                            dict(name='ZillaOS System', first_lba=root_first, last_lba=root_last)],
                bootable=False, formatted=False, installation_enabled=False,
                note='PROTOTYPE GPT ONLY; no FAT32, root filesystem, bootloader or kernel installed')


def _gpt_header(sector: int, own: int, alternate: int, total: int,
                table_lba: int, table_crc: int, disk_guid: uuid.UUID) -> bytes:
    out = bytearray(sector)
    out[:8] = b'EFI PART'
    struct.pack_into('<III', out, 8, 0x00010000, 92, 0)
    table_sectors = 16384 // sector
    struct.pack_into('<QQQQ', out, 24, own, alternate,
                     2 + table_sectors, total - 2 - table_sectors)
    out[56:72] = disk_guid.bytes_le
    struct.pack_into('<QIII', out, 72, table_lba, 128, 128, table_crc)
    struct.pack_into('<I', out, 16, zlib.crc32(out[:92]) & 0xffffffff)
    return bytes(out)


def _new_entry(part_type: uuid.UUID, partition_guid: uuid.UUID,
               first: int, last: int, label: str) -> bytes:
    e = bytearray(128)
    e[0:16] = part_type.bytes_le
    e[16:32] = partition_guid.bytes_le
    struct.pack_into('<QQQ', e, 32, first, last, 0)
    e[56:56 + len(label) * 2] = label.encode('utf-16le')
    return bytes(e)


def create_lab_image(root: Path, name: str, *, size_gib: int = 12,
                     sector: int = 512, confirmation: str = '',
                     format_esp: bool = False,
                     boot_files: dict[str, bytes] | None = None) -> dict:
    if confirmation != CONFIRM:
        raise UnsafeRequest('explicit confirmation required: ' + CONFIRM)
    if not NAME.fullmatch(name):
        raise UnsafeRequest('name must match zillaos-<simple-lowercase-name>.img')
    plan = plan_image(size_gib, sector)
    if boot_files and not format_esp:
        raise UnsafeRequest('UEFI files require FAT32 ESP formatting')
    if os.name != 'posix' or not hasattr(os, 'pwrite'):
        raise UnsafeRequest('run only in Linux / WSL Ubuntu')
    root = Path(root)
    if any(parent.is_symlink() for parent in (root, *root.parents)):
        raise UnsafeRequest('symlinked lab directory or ancestor is forbidden')
    root.mkdir(parents=True, exist_ok=True)
    if not root.is_dir() or root.is_symlink():
        raise UnsafeRequest('lab directory must be real directory')
    dest = root / name
    fd = None
    created = False
    try:
        flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, 'O_CLOEXEC', 0) | getattr(os, 'O_NOFOLLOW', 0)
        fd = os.open(dest, flags, 0o600)
        created = True
        if not stat.S_ISREG(os.fstat(fd).st_mode):
            raise UnsafeRequest('new target must be a regular file')
        total = plan['total_sectors']
        os.ftruncate(fd, size_gib * GIB)  # sparse hole, no multi-GiB RAM or prefill
        mbr = bytearray(sector)
        mbr[510:512] = b'\x55\xaa'
        mbr[446 + 4] = 0xee
        struct.pack_into('<II', mbr, 446 + 8, 1, min(total - 1, 0xffffffff))
        entries = bytearray(16384)
        disk_guid = uuid.uuid4()
        efi_guid, root_guid = uuid.uuid4(), uuid.uuid4()
        entries[:128] = _new_entry(ESP_TYPE, efi_guid, plan['esp_first'], plan['esp_last'], 'ZillaOS EFI')
        entries[128:256] = _new_entry(ROOT_TYPE, root_guid, plan['system_first'], plan['system_last'], 'ZillaOS System')
        arr_crc = zlib.crc32(entries) & 0xffffffff
        tbl_count = 16384 // sector
        backup_array_lba = total - 1 - tbl_count
        primary_header = _gpt_header(sector, 1, total - 1, total, 2, arr_crc, disk_guid)
        backup_header = _gpt_header(sector, total - 1, 1, total, backup_array_lba, arr_crc, disk_guid)
        for location, contents in [(0, mbr), (sector, primary_header), (2 * sector, entries),
                                   (backup_array_lba * sector, entries), ((total - 1) * sector, backup_header)]:
            if os.pwrite(fd, contents, location) != len(contents):
                raise UnsafeRequest('short image write')
        fat_report = create_esp(fd, sector_size=sector,
            esp_first=plan['esp_first'], esp_last=plan['esp_last'],
            files=boot_files) if format_esp else None
        os.fsync(fd)
        os.close(fd)
        fd = None
        verified = verify_image(dest, sector)
        if len(verified['partitions']) != 2:
            raise UnsafeRequest('post-create verification failed')
        for actual, intended in zip(verified['partitions'], plan['partitions']):
            if actual['first_lba'] != intended['first_lba'] or actual['last_lba'] != intended['last_lba']:
                raise UnsafeRequest('partition boundaries failed read-back')
        fat_verified = verify_fat32_image(dest, sector, boot_files) if format_esp else None
        if fat_report and not fat_verified['fat32_valid']:
            raise UnsafeRequest('FAT32 verification failed')
        return {'created': str(dest), 'plan': plan,
                'esp_fat32': fat_verified,
                'gpt_verified': True, 'partition_count': 2,
                'disk_guid': str(disk_guid),
                'note': ('UEFI boot assets staged; boot NOT verified' if boot_files else
                         'GPT/FAT32 sandbox image; NOT bootable' if format_esp else
                         'GPT-only sandbox image; NOT bootable')}
    except BaseException:
        # Only clean up the file which this invocation created exclusively.
        # Never remove or truncate an existing path.
        if created:
            try: dest.unlink(missing_ok=True)
            except OSError: pass
        raise
    finally:
        if fd is not None: os.close(fd)


def load_uefi_assets() -> dict[str, bytes]:
    """Load files generated by the actual Rust/UEFI ISO builder, not mocks."""
    root = Path(__file__).resolve().parent.parent
    sources = {
        'EFI/BOOT/BOOTX64.EFI': root/'build/iso-root/EFI/BOOT/BOOTX64.EFI',
        'BOOT/KERNEL.ELF': root/'dist/zillaos-0.0.2-dev-demo-kernel.elf',
        'LIMINE.CONF': root/'boot/limine.conf',
    }
    result = {}
    for dest, source in sources.items():
        if source.is_symlink() or not source.is_file():
            raise UnsafeRequest(f'missing/untrusted built boot asset: {source}')
        size = source.stat().st_size
        if not 0 < size <= 16 * 1024 * 1024:
            raise UnsafeRequest(f'boot file empty or too large: {source}')
        result[dest] = source.read_bytes()
    if not result['EFI/BOOT/BOOTX64.EFI'].startswith(b'MZ'):
        raise UnsafeRequest('Limine EFI loader does not have PE DOS signature')
    if not result['BOOT/KERNEL.ELF'].startswith(b'\x7fELF'):
        raise UnsafeRequest('kernel is not an ELF image')
    config = result['LIMINE.CONF'].decode('utf-8')
    old = 'boot():/boot/zillaos-kernel'
    if config.count(old) != 1:
        raise UnsafeRequest('unexpected Limine kernel path in boot/limine.conf')
    result['LIMINE.CONF'] = config.replace(old,'boot():/boot/kernel.elf').encode('utf-8')
    # Check actual PE32+ EFI executable + ELF64/x86-64 PT_LOAD and Limine path.
    # A magic MZ/ELF prefix alone never proves a boot artifact is valid.
    validate_assets(result)
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--name', default='zillaos-lab.img')
    parser.add_argument('--size-gib', type=int, default=12)
    parser.add_argument('--sector-size', type=int, choices=(512, 4096), default=512)
    parser.add_argument('--fat32', action='store_true',
        help='also format new image EFI partition as FAT32')
    parser.add_argument('--stage-uefi', action='store_true',
        help='stage built Limine EFI binary, kernel and config in FAT32 ESP')
    parser.add_argument('--create', action='store_true', help='create ONE new sparse image; default is read-only dry run')
    parser.add_argument('--confirm', default='', help='type CREATE-NEW-VM-IMAGE to create')
    args = parser.parse_args()
    try:
        plan = plan_image(args.size_gib, args.sector_size)
        if not args.create:
            print(json.dumps({'mode': 'DRY_RUN', 'plan': plan}, indent=2))
        else:
            root = Path.home() / '.local' / 'share' / 'zillaos' / 'installer-lab'
            assets = load_uefi_assets() if args.stage_uefi else None
            print(json.dumps(create_lab_image(root, args.name,
                size_gib=args.size_gib, sector=args.sector_size,
                confirmation=args.confirm,
                format_esp=args.fat32 or args.stage_uefi,
                boot_files=assets), indent=2))
        return 0
    except (UnsafeRequest, OSError, ValueError) as exc:
        print('INSTALLER LAB REFUSED:', exc, file=sys.stderr)
        return 2

if __name__ == '__main__':
    raise SystemExit(main())
