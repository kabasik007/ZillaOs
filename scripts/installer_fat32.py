"""FAT32 ESP writer for an exclusively new O_EXCL Installer Lab image descriptor.

No host device or path opens. Supports UEFI files as 8.3 and one VFAT LFN.
"""
from __future__ import annotations
import math
import os
import struct
from dataclasses import dataclass

class Fat32Error(ValueError):
    pass

@dataclass(frozen=True)
class Geometry:
    sector_size: int
    sector_count: int
    first_lba: int
    sectors_per_cluster: int
    sectors_per_fat: int
    reserved_sectors: int
    cluster_count: int

    @property
    def cluster_bytes(self) -> int:
        return self.sector_size * self.sectors_per_cluster

    @property
    def data_sector(self) -> int:
        return self.reserved_sectors + 2 * self.sectors_per_fat

    def cluster_offset(self, cluster: int) -> int:
        if not 2 <= cluster < self.cluster_count + 2:
            raise Fat32Error('cluster outside FAT32 allocation range')
        return (self.first_lba + self.data_sector +
                (cluster - 2) * self.sectors_per_cluster) * self.sector_size


def geometry(sector_size: int, first_lba: int, sector_count: int) -> Geometry:
    if sector_size not in (512, 4096) or first_lba < 1 or sector_count < 65536:
        raise Fat32Error('unsupported FAT32 ESP geometry')
    spc = 8 if sector_size == 512 else 1
    reserved = 32
    sectors_per_fat = 1
    for _ in range(32):
        data_sectors = sector_count - reserved - 2 * sectors_per_fat
        if data_sectors < 0:
            raise Fat32Error('FAT32 volume too small')
        clusters = data_sectors // spc
        new = math.ceil((clusters + 2) * 4 / sector_size)
        if new == sectors_per_fat:
            break
        sectors_per_fat = new
    else:
        raise Fat32Error('FAT sizing did not converge')
    clusters = (sector_count - reserved - 2 * sectors_per_fat) // spc
    if not (65525 <= clusters < 0x0ffffff5) or (clusters + 2) * 4 > sectors_per_fat * sector_size:
        raise Fat32Error('not a valid FAT32 cluster count')
    return Geometry(sector_size, sector_count, first_lba, spc, sectors_per_fat, reserved, clusters)


def _short_name(name: str) -> bytes:
    name = name.upper()
    if name in ('.', '..'):
        return name.ljust(11).encode('ascii')
    base, dot, ext = name.partition('.')
    if not (1 <= len(base) <= 8 and len(ext) <= 3) or (dot and not ext) or '.' in ext:
        raise Fat32Error('only FAT32 8.3 names supported: ' + name)
    allowed = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_$~!#%&-{}()@^'
    if any(ch not in allowed for ch in base + ext):
        raise Fat32Error('invalid short name')
    return (base.ljust(8) + ext.ljust(3)).encode('ascii')


def _short_checksum(short: bytes) -> int:
    checksum = 0
    for byte in short:
        checksum = (((checksum & 1) << 7) + (checksum >> 1) + byte) & 0xff
    return checksum


def _long_name_entry(name: str, short: bytes) -> bytes:
    encoded = name.encode('utf-16le')
    chars = [encoded[i:i+2] for i in range(0, len(encoded), 2)]
    if len(chars) > 12:
        raise Fat32Error('long filename exceeds single VFAT directory entry')
    chars.append(b'\x00\x00')
    chars.extend([b'\xff\xff'] * (13 - len(chars)))
    entry = bytearray(32)
    entry[0] = 0x41
    entry[11] = 0x0f
    entry[13] = _short_checksum(short)
    for j, offset in enumerate((1,3,5,7,9,14,16,18,20,22,24,28,30)):
        entry[offset:offset+2] = chars[j]
    return bytes(entry)


def _entry(name: str, start: int, size: int, *, directory: bool = False) -> bytes:
    if start < 2 or size < 0 or size > 0xffffffff:
        raise Fat32Error('file pointer or size invalid')
    e = bytearray(32)
    e[:11] = _short_name(name)
    e[11] = 0x10 if directory else 0x20
    struct.pack_into('<H', e, 20, start >> 16)
    struct.pack_into('<H', e, 26, start & 0xffff)
    struct.pack_into('<I', e, 28, size)
    return bytes(e)


def create_esp(fd: int, *, sector_size: int, esp_first: int, esp_last: int,
               files: dict[str, bytes] | None = None) -> dict:
    """Format ESP only in an exclusively held *new file* descriptor."""
    files = {} if files is None else files
    expected = {'EFI/BOOT/BOOTX64.EFI', 'BOOT/KERNEL.ELF', 'LIMINE.CONF'}
    if files and set(files) != expected:
        raise Fat32Error('incomplete/unexpected EFI staging files')
    if any(not isinstance(b, bytes) or not b or len(b) > 16*1024*1024 for b in files.values()):
        raise Fat32Error('invalid or oversized boot asset')
    g = geometry(sector_size, esp_first, esp_last - esp_first + 1)
    if (esp_last + 1)*sector_size > os.fstat(fd).st_size:
        raise Fat32Error('ESP extends beyond image')
    boot = bytearray(sector_size)
    boot[0:3] = b'\xeb\x58\x90'
    boot[3:11] = b'ZILLAOS '
    struct.pack_into('<HBHBHHBHHHII', boot, 11,
                     sector_size, g.sectors_per_cluster, 32, 2, 0, 0, 0xf8,
                     0, 63, 255, g.first_lba, g.sector_count)
    struct.pack_into('<IHHIHH', boot, 36, g.sectors_per_fat, 0, 0, 2, 1, 6)
    boot[64] = 0x80
    boot[66] = 0x29
    struct.pack_into('<I', boot, 67, 0x5a494c4c)
    boot[71:82] = b'ZILLAOS EFI'
    boot[82:90] = b'FAT32   '
    boot[510:512] = b'\x55\xaa'
    boot[sector_size-2:sector_size] = b'\x55\xaa'
    fsinfo = bytearray(sector_size)
    struct.pack_into('<I', fsinfo, 0, 0x41615252)
    struct.pack_into('<I', fsinfo, 484, 0x61417272)
    struct.pack_into('<I', fsinfo, 488, 0xffffffff)
    struct.pack_into('<I', fsinfo, 492, 0xffffffff)
    struct.pack_into('<I', fsinfo, 508, 0xaa550000)
    base = esp_first * sector_size

    def write(relative: int, data: bytes) -> None:
        if relative < 0 or relative + len(data) > g.sector_count * sector_size:
            raise Fat32Error('write outside ESP')
        if os.pwrite(fd, data, base + relative) != len(data):
            raise Fat32Error('short ESP write')

    write(0, boot)
    write(sector_size, fsinfo)
    write(6*sector_size, boot)
    write(7*sector_size, fsinfo)
    fats = bytearray(g.sectors_per_fat * sector_size)
    struct.pack_into('<III', fats, 0, 0x0ffffff8, 0xffffffff, 0x0fffffff)
    next_cluster = 3

    def alloc(payload: bytes) -> int:
        nonlocal next_cluster
        need = max(1, math.ceil(len(payload) / g.cluster_bytes))
        first = next_cluster
        if first + need > g.cluster_count + 2:
            raise Fat32Error('ESP out of clusters')
        for i in range(need):
            cl = first + i
            nxt = 0x0fffffff if i == need - 1 else cl + 1
            struct.pack_into('<I', fats, cl*4, nxt)
            chunk = payload[i*g.cluster_bytes:(i+1)*g.cluster_bytes]
            if chunk:
                write(g.cluster_offset(cl)-base, chunk)
        next_cluster += need
        return first

    if files:
        efi_cl = alloc(b'')
        boot_cl = alloc(b'')
        kernel_dir_cl = alloc(b'')
        efibin_cl = alloc(files['EFI/BOOT/BOOTX64.EFI'])
        kernel_cl = alloc(files['BOOT/KERNEL.ELF'])
        config_cl = alloc(files['LIMINE.CONF'])

        def directory(cl: int, entries: list[bytes]) -> None:
            payload = b''.join(entries)
            if len(payload)+32 > g.cluster_bytes:
                raise Fat32Error('directory too large')
            write(g.cluster_offset(cl)-base, payload + b'\x00'*32)

        limine_alias = _short_name('LIMINE~1.CNF')
        directory(2, [_entry('EFI', efi_cl, 0, directory=True),
                      _entry('BOOT', kernel_dir_cl, 0, directory=True),
                      _long_name_entry('LIMINE.CONF', limine_alias),
                      _entry('LIMINE~1.CNF', config_cl, len(files['LIMINE.CONF']))])
        directory(efi_cl, [_entry('.', efi_cl, 0, directory=True),
                           _entry('..', 2, 0, directory=True),
                           _entry('BOOT', boot_cl, 0, directory=True)])
        directory(boot_cl, [_entry('.', boot_cl, 0, directory=True),
                            _entry('..', efi_cl, 0, directory=True),
                            _entry('BOOTX64.EFI', efibin_cl, len(files['EFI/BOOT/BOOTX64.EFI']))])
        directory(kernel_dir_cl, [_entry('.', kernel_dir_cl, 0, directory=True),
                                  _entry('..', 2, 0, directory=True),
                                  _entry('KERNEL.ELF', kernel_cl, len(files['BOOT/KERNEL.ELF']))])
    write(32*sector_size, fats)
    write((32+g.sectors_per_fat)*sector_size, fats)
    return {'fat32': True, 'sector_size': sector_size,
            'cluster_count': g.cluster_count, 'cluster_bytes': g.cluster_bytes,
            'fat_sectors': g.sectors_per_fat, 'staged_files': list(files)}
