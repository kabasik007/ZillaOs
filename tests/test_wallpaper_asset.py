"""Wallpaper BMP contract and optional Limine module staging tests."""
from __future__ import annotations

import struct
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
from wallpaper_asset import validate_bmp, stage_asset  # noqa: E402


def sample_bmp() -> bytes:
    # 3x2 RGB24. Each row occupies 12 bytes (9 color + 3 padding).
    data = bytearray(78)
    data[:2] = b"BM"
    struct.pack_into("<I", data, 2, len(data))
    struct.pack_into("<I", data, 10, 54)
    struct.pack_into("<I", data, 14, 40)
    struct.pack_into("<ii", data, 18, 3, 2)
    struct.pack_into("<HH", data, 26, 1, 24)
    data[54:63] = bytes([255,0,0, 0,255,0, 0,0,255])
    data[66:75] = bytes([255,255,255, 0,0,0, 0,255,255])
    return bytes(data)


class TestWallpaperAsset(unittest.TestCase):
    def test_valid_bmp_and_corrupt_header(self):
        with tempfile.TemporaryDirectory() as folder:
            bmp = Path(folder) / "asset.bmp"
            bmp.write_bytes(sample_bmp())
            self.assertEqual(validate_bmp(bmp), (3, 2, 24))
            broken = bytearray(sample_bmp())
            broken[30] = 1  # compressed format forbidden
            bmp.write_bytes(broken)
            with self.assertRaisesRegex(ValueError, "BI_RGB"):
                validate_bmp(bmp)

    def test_stage_injects_module_into_exactly_six_entries(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            bmp = root / "asset.bmp"
            bmp.write_bytes(sample_bmp())
            iso_root = root / "iso-root"
            config = root / "limine.conf"
            base = (ROOT / "boot/limine.conf").read_text(encoding="utf-8")
            config.write_text(base, encoding="utf-8")
            self.assertEqual(stage_asset(bmp, iso_root, config), (3, 2, 24))
            staged = (iso_root / "limine.conf").read_text(encoding="utf-8")
            self.assertEqual(staged.count("module_path: boot():/boot/wallpaper.bmp"), 6)
            self.assertEqual(staged.count("module_string: zillaos-wallpaper"), 6)
            self.assertEqual((iso_root / "boot/wallpaper.bmp").read_bytes(), bmp.read_bytes())
            self.assertEqual(config.read_text(encoding="utf-8"), base)

    def test_rejects_bad_input_before_modifying_staging(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            bmp = root / "asset.bmp"
            data = bytearray(sample_bmp())
            struct.pack_into("<i", data, 18, 1025)
            bmp.write_bytes(data)
            iso_root = root / "iso-root"
            config = root / "limine.conf"
            config.write_text((ROOT / "boot/limine.conf").read_text(), encoding="utf-8")
            with self.assertRaises(ValueError):
                stage_asset(bmp, iso_root, config)
            self.assertFalse((iso_root / "boot/wallpaper.bmp").exists())
            self.assertFalse((iso_root / "limine.conf").exists())

    def test_rejects_unknown_module_config_instead_of_overwriting(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            bmp = root / "asset.bmp"
            bmp.write_bytes(sample_bmp())
            config = root / "limine.conf"
            config.write_text(
                "module_path: boot():/other\n" + (ROOT / "boot/limine.conf").read_text()
            )
            with self.assertRaisesRegex(ValueError, "Unexpected modules"):
                stage_asset(bmp, root / "iso-root", config)


if __name__ == "__main__":
    unittest.main()
