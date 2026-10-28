"""Tests run on sparse temporary *new files*, never block devices."""
from pathlib import Path
import tempfile
import unittest
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
from installer_lab import CONFIRM, UnsafeRequest, create_lab_image, plan_image
from gpt_image_verify import verify_image


class InstallerLabTests(unittest.TestCase):
    def test_dry_run_is_nonwriting(self):
        result = plan_image(12, 512)
        self.assertFalse(result['bootable'])
        self.assertFalse(result['installation_enabled'])
        self.assertEqual(result['esp_first'], 2048)

    def test_new_sparse_image_end_to_end_for_both_sector_sizes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / 'installer-lab'
            for sector in (512, 4096):
                name = f'zillaos-sector-{sector}.img'
                report = create_lab_image(root, name, size_gib=12, sector=sector, confirmation=CONFIRM)
                path = root / name
                self.assertTrue(report['gpt_verified'])
                inspected = verify_image(path, sector)
                self.assertEqual(len(inspected['partitions']), 2)
                self.assertEqual(path.stat().st_size, 12 * 1024**3)
                self.assertTrue(path.stat().st_blocks * 512 < 16 * 1024**2)
                self.assertEqual([p['first_lba'] for p in inspected['partitions']],
                                 [p['first_lba'] for p in report['plan']['partitions']])

    def test_never_overwrites_existing_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / 'installer-lab'
            root.mkdir()
            path = root / 'zillaos-test.img'
            path.write_bytes(b'DO NOT TOUCH')
            with self.assertRaises(FileExistsError):
                create_lab_image(root, path.name, confirmation=CONFIRM)
            self.assertEqual(path.read_bytes(), b'DO NOT TOUCH')

    def test_missing_confirmation_never_creates_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / 'installer-lab'
            with self.assertRaises(UnsafeRequest):
                create_lab_image(root, 'zillaos-test.img')
            self.assertFalse(root.exists())

    def test_invalid_name_and_symlinked_directory_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / 'installer-lab'
            for name in ('../../host.img', '/dev/sda', 'zillaos-test.vmdk', 'test.img'):
                with self.assertRaises(UnsafeRequest):
                    create_lab_image(root, name, confirmation=CONFIRM)
            other = Path(tmp) / 'outside'
            other.mkdir()
            root.symlink_to(other, target_is_directory=True)
            with self.assertRaises(UnsafeRequest):
                create_lab_image(root, 'zillaos-test.img', confirmation=CONFIRM)
            self.assertEqual(list(other.iterdir()), [])

    def test_invalid_sizes_are_rejected(self):
        for size in (0, 8, 11, 65):
            with self.assertRaises(UnsafeRequest):
                plan_image(size, 512)
        with self.assertRaises(UnsafeRequest):
            plan_image(12, 520)

    def test_recreation_after_success_is_blocked_without_mutating_image(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            create_lab_image(root, 'zillaos-existing.img', confirmation=CONFIRM)
            image = root / 'zillaos-existing.img'
            before = image.stat().st_size
            with self.assertRaises(FileExistsError):
                create_lab_image(root, 'zillaos-existing.img', confirmation=CONFIRM)
            self.assertEqual(image.stat().st_size, before)
            self.assertEqual(verify_image(image)['status'], 'valid')

if __name__ == '__main__':
    unittest.main()
