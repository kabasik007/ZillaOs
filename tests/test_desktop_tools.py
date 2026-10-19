"""Pure host-side checks for headless desktop smoke evidence and PPM parser."""
from pathlib import Path
import tempfile
import unittest

from scripts.desktop_smoke import has_line, screenshot_dimensions


class DesktopSmokeTests(unittest.TestCase):
    def test_exact_markers_required(self):
        log = "ZILLAOS_DESKTOP_READY\r\nZILLAOS_DESKTOP_INPUT_POLL\n"
        self.assertTrue(has_line(log, "ZILLAOS_DESKTOP_READY"))
        self.assertFalse(has_line(log, "ZILLAOS_DESKTOP_STATE_CHANGED"))
        self.assertFalse(has_line("prefix ZILLAOS_DESKTOP_READY suffix", "ZILLAOS_DESKTOP_READY"))

    def test_valid_colorful_ppm(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "screen.ppm"
            pixels = bytes([20, 120, 180]) * (320 * 100) + bytes([255, 150, 80]) * (320 * 100)
            p.write_bytes(b"P6\n320 200\n255\n" + pixels)
            self.assertEqual(screenshot_dimensions(p), (320, 200))

    def test_blank_ppm_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "blank.ppm"
            p.write_bytes(b"P6\n320 200\n255\n" + bytes(320 * 200 * 3))
            self.assertIsNone(screenshot_dimensions(p))

    def test_truncated_ppm_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "bad.ppm"
            p.write_bytes(b"P6\n320 200\n255\n" + bytes(20))
            self.assertIsNone(screenshot_dimensions(p))


if __name__ == "__main__":
    unittest.main()
