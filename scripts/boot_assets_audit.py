#!/usr/bin/env python3
"""Validate staged x86_64 Limine, kernel ELF and boot configuration.

Pure, read-only validation of *file bytes*. No disk, VMDK, or OS installer
writes. This is a preflight check, not proof of a successful UEFI boot.
"""
from __future__ import annotations

import re
import struct


class BootAssetError(ValueError):
    """An untrusted, malformed or mismatched boot artifact."""


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise BootAssetError(message)


def validate_pe_uefi_x64(data: bytes) -> dict[str, int | str]:
    """Require PE32+ x86_64 EFI_APPLICATION, not just the DOS MZ stub."""
    _require(isinstance(data, bytes) and len(data) >= 128, 'EFI binary too short')
    _require(data[:2] == b'MZ', 'EFI DOS signature is missing')
    (pe_offset,) = struct.unpack_from('<I', data, 0x3c)
    _require(64 <= pe_offset <= len(data) - 4 - 20 - 70,
             'EFI PE header pointer outside file')
    _require(data[pe_offset:pe_offset + 4] == b'PE\0\0', 'EFI PE signature missing')
    coff = pe_offset + 4
    machine, section_count, _, _, _, optional_size, _ = struct.unpack_from('<HHIIIHH', data, coff)
    _require(machine == 0x8664, 'EFI loader is not x86-64')
    _require(1 <= section_count <= 96, 'EFI section count invalid')
    _require(optional_size >= 72, 'EFI optional header too small')
    optional = coff + 20
    _require(optional + optional_size <= len(data), 'EFI optional header truncated')
    (magic,) = struct.unpack_from('<H', data, optional)
    (entry_point,) = struct.unpack_from('<I', data, optional + 16)
    (image_size,) = struct.unpack_from('<I', data, optional + 56)
    (subsystem,) = struct.unpack_from('<H', data, optional + 68)
    _require(magic == 0x20b, 'EFI loader must be PE32+')
    _require(subsystem == 10, 'PE subsystem is not EFI application')
    _require(0 < entry_point < image_size, 'EFI entry point/image geometry invalid')
    section_start = optional + optional_size
    _require(section_start + section_count * 40 <= len(data),
             'EFI section headers truncated')
    executable_entry = False
    for index in range(section_count):
        sh = section_start + index * 40
        virtual_size, virtual_address, raw_size, raw_pointer = struct.unpack_from('<IIII', data, sh + 8)
        (characteristics,) = struct.unpack_from('<I', data, sh + 36)
        mapped_size = max(virtual_size, raw_size)
        _require(mapped_size > 0 and virtual_address < image_size and
                 mapped_size <= image_size - virtual_address,
                 'EFI section mapping outside image')
        _require(raw_size == 0 or
                 (raw_pointer <= len(data) and raw_size <= len(data) - raw_pointer),
                 'EFI section raw bytes outside file')
        if (characteristics & 0x20000000 and
            virtual_address <= entry_point < virtual_address + mapped_size):
            executable_entry = True
    _require(executable_entry, 'EFI entry point not in executable section')
    return {'format': 'PE32+', 'architecture': 'x86-64', 'subsystem': 'EFI application',
            'entry_rva': entry_point, 'sections': section_count, 'size_bytes': len(data)}


def validate_elf64_x64(data: bytes) -> dict[str, int | str]:
    """Validate ELF64 LE x86-64 header and bounded PT_LOAD file segments."""
    _require(isinstance(data, bytes) and len(data) >= 64, 'Kernel ELF too short')
    _require(data[:4] == b'\x7fELF', 'ELF signature missing')
    _require(data[4:7] == b'\x02\x01\x01', 'Kernel must be ELF64 little-endian')
    e_type, machine, version = struct.unpack_from('<HHI', data, 16)
    _require(e_type in (2, 3), 'Kernel ELF type is not executable or shared object')
    _require(machine == 62 and version == 1, 'Kernel ELF is not x86-64')
    entry, phoff = struct.unpack_from('<QQ', data, 24)
    header_size, phent_size, phnum = struct.unpack_from('<HHH', data, 52)
    _require(header_size == 64, 'Kernel ELF header size invalid')
    _require(entry != 0, 'Kernel ELF entry point is zero')
    _require(1 <= phnum <= 128 and phent_size == 56, 'Kernel ELF program headers invalid')
    _require(64 <= phoff <= len(data) and phnum * phent_size <= len(data) - phoff,
             'Kernel ELF program header table outside file')
    loads = 0
    executable_entry = False
    for i in range(phnum):
        off = phoff + i * phent_size
        (p_type,) = struct.unpack_from('<I', data, off)
        if p_type != 1:
            continue
        loads += 1
        (p_flags,) = struct.unpack_from('<I', data, off + 4)
        (p_offset, p_vaddr) = struct.unpack_from('<QQ', data, off + 8)
        (p_filesz, p_memsz, p_align) = struct.unpack_from('<QQQ', data, off + 32)
        _require(p_memsz > 0 and p_filesz <= p_memsz and p_offset <= len(data) and
                 p_filesz <= len(data) - p_offset,
                 'Kernel ELF loadable segment has invalid file bounds')
        _require(p_memsz <= (1 << 64) - 1 - p_vaddr,
                 'Kernel ELF segment virtual range overflows')
        _require(p_align in (0, 1) or
                 (p_align & (p_align - 1) == 0 and p_vaddr % p_align == p_offset % p_align),
                 'Kernel ELF segment alignment invalid')
        if p_flags & 1 and p_vaddr <= entry < p_vaddr + p_memsz:
            executable_entry = True
    _require(loads > 0, 'Kernel ELF contains no loadable program segment')
    _require(executable_entry, 'Kernel ELF entry point not in executable PT_LOAD')
    return {'format': 'ELF64', 'architecture': 'x86-64', 'entry_point': entry,
            'load_segments': loads, 'size_bytes': len(data)}


def validate_limine_config(data: bytes) -> dict[str, str | int]:
    _require(isinstance(data, bytes) and len(data) <= 64 * 1024, 'Limine config too large')
    try:
        config = data.decode('utf-8')
    except UnicodeDecodeError as exc:
        raise BootAssetError('Limine config is not UTF-8') from exc
    _require(re.search(r'(?m)^/ZillaOS\s*$', config) is not None,
             'Limine ZillaOS boot entry missing')
    matches_protocol = re.findall(r'(?mi)^\s*protocol:\s*(\S+)\s*$', config)
    _require(matches_protocol == ['limine'], 'Limine boot protocol mismatch')
    paths = re.findall(r'(?mi)^\s*path:\s*(\S+)\s*$', config)
    _require(paths == ['boot():/boot/kernel.elf'], 'Limine kernel path mismatch')
    return {'protocol': 'limine', 'kernel_path': paths[0], 'size_bytes': len(data)}


def validate_assets(files: dict[str, bytes]) -> dict[str, dict]:
    required = {'EFI/BOOT/BOOTX64.EFI', 'BOOT/KERNEL.ELF', 'LIMINE.CONF'}
    _require(set(files) == required, 'UEFI staging requires exactly three boot assets')
    return {
        'uefi_loader': validate_pe_uefi_x64(files['EFI/BOOT/BOOTX64.EFI']),
        'kernel': validate_elf64_x64(files['BOOT/KERNEL.ELF']),
        'limine': validate_limine_config(files['LIMINE.CONF']),
    }
