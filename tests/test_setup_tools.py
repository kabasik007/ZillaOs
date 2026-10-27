"""Host tests for ordered first-run setup evidence, no QEMU required."""
import unittest
from scripts.desktop_smoke import setup_passed

GOOD = "\n".join([
    "ZILLAOS_CPU_READY",
    "ZILLAOS_SETUP_READY",
    *[f"ZILLAOS_SETUP_STEP=0x{i:016x}" for i in range(1, 8)],
    "ZILLAOS_SETUP_COMPLETE",
    "ZILLAOS_SETUP_LANGUAGE=uk",
    "ZILLAOS_SETUP_TIMEZONE=Europe/Kyiv",
    "ZILLAOS_SETUP_HOSTNAME=zilla-pc",
    "ZILLAOS_SETUP_REGION=UA",
    "ZILLAOS_SETUP_THEME=DARK",
    "ZILLAOS_SETUP_VOLATILE_ONLY",
    "ZILLAOS_DESKTOP_READY",
]) + "\n"


class SetupEvidenceTests(unittest.TestCase):
    def test_complete_default_setup_passes(self):
        self.assertTrue(setup_passed(GOOD))

    def test_missing_step_cannot_skip_welcome(self):
        self.assertFalse(setup_passed(
            GOOD.replace("ZILLAOS_SETUP_STEP=0x0000000000000004\n", "")))

    def test_out_of_order_steps_fail(self):
        altered = GOOD.replace(
            "ZILLAOS_SETUP_STEP=0x0000000000000001\n"
            "ZILLAOS_SETUP_STEP=0x0000000000000002",
            "ZILLAOS_SETUP_STEP=0x0000000000000002\n"
            "ZILLAOS_SETUP_STEP=0x0000000000000001",
        )
        self.assertFalse(setup_passed(altered))

    def test_missing_volatile_warning_fails(self):
        self.assertFalse(setup_passed(GOOD.replace(
            "ZILLAOS_SETUP_VOLATILE_ONLY", "DISK_INSTALL_COMPLETE")))

    def test_unexpected_nondefault_settings_need_explicit_test(self):
        self.assertFalse(setup_passed(
            GOOD.replace("ZILLAOS_SETUP_LANGUAGE=uk", "ZILLAOS_SETUP_LANGUAGE=en")))


if __name__ == "__main__":
    unittest.main()
