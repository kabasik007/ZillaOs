"""FAT32+GPT installer tests: disposable sparse files, no block devices."""
from pathlib import Path
import sys
import tempfile
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
from installer_lab import CONFIRM, create_lab_image, UnsafeRequest, load_uefi_assets
from installer_fat32 import Fat32Error, geometry
from installer_fat32_verify import verify_fat32_image, Fat32Invalid
from gpt_image_verify import verify_image

BOOT_FILES = {
    'EFI/BOOT/BOOTX64.EFI': b'MZ' + bytes(range(256))*27,
    'BOOT/KERNEL.ELF': b'\x7fELF' + bytes(range(256))*39,
    'LIMINE.CONF': b'timeout: 0\n/ZillaOS\n    protocol: limine\n    path: boot():/boot/kernel.elf\n',
}

class InstallerFat32Tests(unittest.TestCase):
    def test_blank_esp_verifies_for_both_sector_sizes(self):
        with tempfile.TemporaryDirectory() as tmp:
            for sector in (512, 4096):
                name = f'zillaos-fat32-{sector}.img'
                img = Path(tmp)/name
                report = create_lab_image(Path(tmp), name, sector=sector,
                                          confirmation=CONFIRM, format_esp=True)
                self.assertIsNotNone(report['esp_fat32'])
                self.assertTrue(verify_fat32_image(img, sector)['fat_copies_match'])
                self.assertEqual(verify_image(img, sector)['status'], 'valid')
                self.assertLess(img.stat().st_blocks*512, 16*1024*1024)

    def test_boot_payload_identical_after_readback_for_both_sectors(self):
        with tempfile.TemporaryDirectory() as tmp:
            for sector in (512, 4096):
                name = f'zillaos-staged-{sector}.img'
                img = Path(tmp)/name
                report = create_lab_image(Path(tmp), name, sector=sector,
                              confirmation=CONFIRM, format_esp=True, boot_files=BOOT_FILES)
                lengths = {k:len(v) for k,v in BOOT_FILES.items()}
                self.assertEqual(report['esp_fat32']['staged_files'], lengths)
                self.assertEqual(verify_fat32_image(img,sector,BOOT_FILES)['staged_files'], lengths)
                self.assertEqual(verify_image(img,sector)['status'], 'valid')

    def test_incomplete_stage_fails_and_deletes_only_new_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            img = Path(tmp)/'zillaos-bad.img'
            with self.assertRaises(Fat32Error):
                create_lab_image(Path(tmp),img.name,confirmation=CONFIRM,
                                 format_esp=True,boot_files={'LIMINE.CONF': b'foo'})
            self.assertFalse(img.exists())
            with self.assertRaises(UnsafeRequest):
                create_lab_image(Path(tmp),img.name,confirmation=CONFIRM,
                                 boot_files=BOOT_FILES)
            self.assertFalse(img.exists())

    def test_existing_file_is_never_modified(self):
        with tempfile.TemporaryDirectory() as tmp:
            img = Path(tmp)/'zillaos-existing.img'
            img.write_bytes(b'HOST DATA MUST REMAIN')
            with self.assertRaises(FileExistsError):
                create_lab_image(Path(tmp),img.name,confirmation=CONFIRM,
                                 format_esp=True,boot_files=BOOT_FILES)
            self.assertEqual(img.read_bytes(),b'HOST DATA MUST REMAIN')

    def test_mirror_fat_corruption_is_detected(self):
        with tempfile.TemporaryDirectory() as tmp:
            img = Path(tmp)/'zillaos-mirror.img'
            result = create_lab_image(Path(tmp),img.name,confirmation=CONFIRM,format_esp=True)
            g=geometry(512,result['plan']['esp_first'],512*1024*1024//512)
            mirror = result['plan']['esp_first']+32+g.sectors_per_fat
            with img.open('r+b') as f:
                f.seek(mirror*512+8)
                f.write(b'\x01')
            with self.assertRaisesRegex(Fat32Invalid,'FAT copies disagree'):
                verify_fat32_image(img)

    def test_vfat_long_filename_checksum_is_verified(self):
        with tempfile.TemporaryDirectory() as tmp:
            img=Path(tmp)/'zillaos-lfn.img'
            result=create_lab_image(Path(tmp),img.name,confirmation=CONFIRM,
                                    format_esp=True,boot_files=BOOT_FILES)
            g=geometry(512,result['plan']['esp_first'],512*1024*1024//512)
            with img.open('r+b') as f:
                f.seek(g.cluster_offset(2)+64+13)
                original=f.read(1)
                f.seek(g.cluster_offset(2)+64+13)
                f.write(bytes([original[0]^1]))
            with self.assertRaisesRegex(Fat32Invalid,'LFN short name checksum mismatch'):
                verify_fat32_image(img,512,BOOT_FILES)

    def test_gpt_only_image_not_falsely_reported_as_fat32(self):
        with tempfile.TemporaryDirectory() as tmp:
            img = Path(tmp)/'zillaos-gpt.img'
            create_lab_image(Path(tmp),img.name,confirmation=CONFIRM)
            with self.assertRaises(Fat32Invalid):
                verify_fat32_image(img)

    def test_missing_actual_uefi_build_fails_closed(self):
        try:
            assets = load_uefi_assets()
        except UnsafeRequest:
            pass
        else:
            self.assertTrue(assets['EFI/BOOT/BOOTX64.EFI'].startswith(b'MZ'))
            self.assertTrue(assets['BOOT/KERNEL.ELF'].startswith(b'\x7fELF'))
            self.assertIn(b'/boot/kernel.elf',assets['LIMINE.CONF'])

    def test_fat32_cluster_geometry_inside_esp(self):
        for sector in (512,4096):
            g = geometry(sector,1024*1024//sector,512*1024*1024//sector)
            self.assertTrue(g.data_sector + g.cluster_count*g.sectors_per_cluster <= g.sector_count)
            self.assertGreaterEqual(g.cluster_count,65525)
            self.assertEqual(g.cluster_bytes,4096)

if __name__ == '__main__':
    unittest.main()
