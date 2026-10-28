"""Standalone read-only GPT image verifier tests (no disk devices)."""
from pathlib import Path
import os
import struct
import sys
import tempfile
import unittest
import zlib

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
from gpt_image_verify import GptInvalid, verify_image


def make_image(path: Path, sector: int = 512) -> None:
    total = 512
    table_sectors = 16384 // sector
    image = bytearray(total * sector)
    image[510:512] = b'\x55\xaa'
    image[446 + 4] = 0xee
    struct.pack_into('<II', image, 446 + 8, 1, total - 1)
    entry_data = bytearray(16384)
    entry_data[0:16] = b'\x01' * 16
    entry_data[16:32] = b'\x11' * 16
    first = 2 + table_sectors
    last = total - table_sectors - 2
    struct.pack_into('<QQ', entry_data, 32, first, first + 40)
    image[2 * sector:2 * sector + 16384] = entry_data
    backup_lba = total - 1 - table_sectors
    image[backup_lba * sector:backup_lba * sector + 16384] = entry_data

    def make_header(own: int, opposite: int, table_start: int) -> bytes:
        header = bytearray(sector)
        header[0:8] = b'EFI PART'
        struct.pack_into('<III', header, 8, 0x10000, 92, 0)
        struct.pack_into('<QQQQ', header, 24, own, opposite, first, last)
        header[56:72] = b'\x43' * 16
        struct.pack_into('<QIII', header, 72, table_start, 128, 128,
                         zlib.crc32(entry_data) & 0xffffffff)
        struct.pack_into('<I', header, 16, zlib.crc32(header[:92]) & 0xffffffff)
        return bytes(header)

    image[sector:2 * sector] = make_header(1, total - 1, 2)
    image[(total - 1) * sector:total * sector] = make_header(total - 1, 1, backup_lba)
    path.write_bytes(image)


class GptImageTests(unittest.TestCase):
    def test_complete_gpt_for_both_sector_sizes(self):
        with tempfile.TemporaryDirectory() as d:
            for sector in (512, 4096):
                path = Path(d) / 'blank.img'
                make_image(path, sector)
                report = verify_image(path, sector)
                self.assertEqual(report['status'], 'valid')
                self.assertTrue(report['primary_and_backup_match'])
                self.assertEqual(len(report['partitions']), 1)

    def test_corrupt_backup_header(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / 'bad.img'
            make_image(path)
            with path.open('r+b') as f:
                f.seek(-512, os.SEEK_END)
                f.write(b'X')
            with self.assertRaisesRegex(GptInvalid, 'signature'):
                verify_image(path)

    def test_backup_table_mismatch_even_when_crc_matches(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / 'bad.raw'
            make_image(path)
            total = 512
            backup = (total - 33) * 512
            with path.open('r+b') as f:
                f.seek(backup + 20)
                f.write(b'Q')
                f.seek(backup)
                modified = f.read(16384)
                table_crc = zlib.crc32(modified) & 0xffffffff
                f.seek((total - 1) * 512)
                header = bytearray(f.read(512))
                struct.pack_into('<I', header, 88, table_crc)
                struct.pack_into('<I', header, 16, 0)
                struct.pack_into('<I', header, 16, zlib.crc32(header[:92]) & 0xffffffff)
                f.seek((total - 1) * 512)
                f.write(header)
            with self.assertRaisesRegex(GptInvalid, 'differ'):
                verify_image(path)

    def test_rejects_vmdk_symlink_and_unaligned_image(self):
        with tempfile.TemporaryDirectory() as d:
            raw = Path(d) / 'test.img'
            make_image(raw)
            with self.assertRaisesRegex(GptInvalid, 'only raw'):
                verify_image(raw.with_suffix('.vmdk'))
            link = Path(d) / 'link.img'
            link.symlink_to(raw)
            with self.assertRaisesRegex(GptInvalid, 'safely'):
                verify_image(link)
            raw.write_bytes(b'hello')
            with self.assertRaisesRegex(GptInvalid, 'too small'):
                verify_image(raw)

    def test_never_writes_to_read_only_image(self):
        with tempfile.TemporaryDirectory() as d:
            image = Path(d) / 'readonly.img'
            make_image(image)
            image.chmod(0o444)
            before = image.read_bytes()
            self.assertEqual(verify_image(image)['status'], 'valid')
            self.assertEqual(image.read_bytes(), before)


if __name__ == '__main__':
    unittest.main()
