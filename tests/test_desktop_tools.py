"""Pure host-side checks for headless desktop smoke evidence and PPM parser."""
from pathlib import Path
import tempfile
import unittest

from scripts.desktop_smoke import has_line, has_window_state, screenshot_dimensions


class DesktopSmokeTests(unittest.TestCase):
    def test_exact_markers_required(self):
        log = "ZILLAOS_DESKTOP_READY\r\nZILLAOS_DESKTOP_INPUT_POLL\n"
        self.assertTrue(has_line(log, "ZILLAOS_DESKTOP_READY"))
        self.assertFalse(has_line(log, "ZILLAOS_DESKTOP_STATE_CHANGED"))
        self.assertFalse(has_line("prefix ZILLAOS_DESKTOP_READY suffix", "ZILLAOS_DESKTOP_READY"))

    def test_window_evidence_matches_one_contiguous_event(self):
        log = (
            "ZILLAOS_DESKTOP_STATE_CHANGED\r\n"
            "ZILLAOS_DESKTOP_VISIBLE=0x0000000000000003\r\n"
            "ZILLAOS_DESKTOP_FOCUS=SETTINGS\r\n"
        )
        self.assertTrue(has_window_state(log, 3, "SETTINGS"))
        self.assertFalse(has_window_state(log, 2, "SETTINGS"))
        self.assertFalse(has_window_state(log, 3, "FILES"))

    def test_unrelated_focus_and_count_are_not_combined(self):
        log = (
            "ZILLAOS_DESKTOP_STATE_CHANGED\n"
            "ZILLAOS_DESKTOP_VISIBLE=0x0000000000000002\n"
            "ZILLAOS_DESKTOP_FOCUS=TERMINAL\n"
            "ZILLAOS_DESKTOP_STATE_CHANGED\n"
            "ZILLAOS_DESKTOP_VISIBLE=0x0000000000000003\n"
            "ZILLAOS_DESKTOP_FOCUS=SETTINGS\n"
        )
        self.assertFalse(has_window_state(log, 2, "SETTINGS"))
        self.assertTrue(has_window_state(log, 2, "TERMINAL"))
        self.assertTrue(has_window_state(log, 3, "SETTINGS"))

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
