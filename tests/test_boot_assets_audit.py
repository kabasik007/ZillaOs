"""Synthetic positive/negative boot artifact checks, no disk I/O."""
import struct
import unittest
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
from boot_assets_audit import (BootAssetError, validate_pe_uefi_x64,
    validate_elf64_x64, validate_limine_config, validate_assets)


def valid_pe() -> bytes:
    data = bytearray(1024)
    data[:2] = b'MZ'
    pe = 128
    struct.pack_into('<I', data, 0x3c, pe)
    data[pe:pe + 4] = b'PE\0\0'
    struct.pack_into('<HHIIIHH', data, pe + 4, 0x8664, 1, 0, 0, 0, 240, 0)
    opt = pe + 24
    struct.pack_into('<H', data, opt, 0x20b)
    struct.pack_into('<I', data, opt + 16, 0x1000)
    struct.pack_into('<I', data, opt + 56, 0x2000)
    struct.pack_into('<H', data, opt + 68, 10)
    section = opt + 240
    struct.pack_into('<IIII', data, section + 8, 0x200, 0x1000, 0x200, 0x200)
    struct.pack_into('<I', data, section + 36, 0x60000020)  # CODE | EXECUTE | READ
    return bytes(data)


def valid_elf() -> bytes:
    data = bytearray(1024)
    data[:4] = b'\x7fELF'
    data[4:7] = b'\x02\x01\x01'
    struct.pack_into('<HHI', data, 16, 2, 62, 1)
    struct.pack_into('<QQ', data, 24, 0xffffffff80000040, 64)
    struct.pack_into('<HHH', data, 52, 64, 56, 1)
    struct.pack_into('<IIQQQQQQ', data, 64, 1, 5, 512, 0xffffffff80000000,
                     0, 64, 128, 512)
    return bytes(data)


def valid_config() -> bytes:
    return b'timeout: 0\n/ZillaOS\n    protocol: limine\n    path: boot():/boot/kernel.elf\n'


class AuditTests(unittest.TestCase):
    def test_valid_synthetic_images(self):
        result = validate_assets({'EFI/BOOT/BOOTX64.EFI':valid_pe(),
            'BOOT/KERNEL.ELF':valid_elf(), 'LIMINE.CONF':valid_config()})
        self.assertEqual(result['uefi_loader']['subsystem'],'EFI application')
        self.assertEqual(result['kernel']['load_segments'],1)

    def test_reject_bare_mz(self):
        with self.assertRaises(BootAssetError): validate_pe_uefi_x64(b'MZBOOT')

    def test_reject_bare_elf(self):
        with self.assertRaises(BootAssetError): validate_elf64_x64(b'\x7fELF'+b'KERNEL')

    def test_reject_arm_efi_loader(self):
        b=bytearray(valid_pe()); struct.pack_into('<H',b,132,0xaa64)
        with self.assertRaisesRegex(BootAssetError,'x86-64'): validate_pe_uefi_x64(bytes(b))

    def test_reject_non_efi_pe_subsystem(self):
        b=bytearray(valid_pe()); struct.pack_into('<H',b,128+24+68,3)
        with self.assertRaisesRegex(BootAssetError,'subsystem'): validate_pe_uefi_x64(bytes(b))

    def test_reject_bad_pe_offset(self):
        b=bytearray(valid_pe()); struct.pack_into('<I',b,0x3c,0xfffffff0)
        with self.assertRaisesRegex(BootAssetError,'pointer'): validate_pe_uefi_x64(bytes(b))

    def test_reject_arm_elf(self):
        b=bytearray(valid_elf()); struct.pack_into('<H',b,18,183)
        with self.assertRaisesRegex(BootAssetError,'x86-64'): validate_elf64_x64(bytes(b))

    def test_reject_elf_out_of_file_segment(self):
        b=bytearray(valid_elf()); struct.pack_into('<Q',b,64+32,2048)
        with self.assertRaisesRegex(BootAssetError,'bounds'): validate_elf64_x64(bytes(b))

    def test_reject_missing_elf_load(self):
        b=bytearray(valid_elf()); struct.pack_into('<I',b,64,4)
        with self.assertRaisesRegex(BootAssetError,'no loadable'): validate_elf64_x64(bytes(b))

    def test_reject_efi_entry_not_executable(self):
        b=bytearray(valid_pe())
        section=128+24+240
        struct.pack_into('<I',b,section+36,0x40000040)  # READ-only DATA
        with self.assertRaisesRegex(BootAssetError,'not in executable'):
            validate_pe_uefi_x64(bytes(b))

    def test_reject_efi_section_out_of_file(self):
        b=bytearray(valid_pe())
        section=128+24+240
        struct.pack_into('<I',b,section+20,0xfffffff0)
        with self.assertRaisesRegex(BootAssetError,'raw bytes'):
            validate_pe_uefi_x64(bytes(b))

    def test_reject_elf_entry_not_executable(self):
        b=bytearray(valid_elf())
        struct.pack_into('<Q',b,24,0xffffffff80001000)
        with self.assertRaisesRegex(BootAssetError,'not in executable'):
            validate_elf64_x64(bytes(b))

    def test_reject_elf_non_executable_load(self):
        b=bytearray(valid_elf())
        struct.pack_into('<I',b,68,4)  # PF_R without PF_X
        with self.assertRaisesRegex(BootAssetError,'not in executable'):
            validate_elf64_x64(bytes(b))

    def test_reject_elf_bad_alignment(self):
        b=bytearray(valid_elf())
        struct.pack_into('<Q',b,64+48,4096)
        with self.assertRaisesRegex(BootAssetError,'alignment'):
            validate_elf64_x64(bytes(b))

    def test_reject_wrong_limine_path(self):
        with self.assertRaisesRegex(BootAssetError,'path mismatch'):
            validate_limine_config(valid_config().replace(b'kernel.elf',b'other.elf'))

    def test_reject_wrong_protocol(self):
        with self.assertRaisesRegex(BootAssetError,'protocol'):
            validate_limine_config(valid_config().replace(b'protocol: limine',b'protocol: linux'))

    def test_reject_incomplete_assets(self):
        with self.assertRaisesRegex(BootAssetError,'exactly three'):
            validate_assets({'EFI/BOOT/BOOTX64.EFI':valid_pe()})


if __name__=='__main__': unittest.main()
