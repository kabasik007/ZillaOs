"""Host tests for ordered first-run setup evidence, no QEMU required."""
import unittest
from scripts.desktop_smoke import setup_passed, storage_scan_passed, setup_transition_steps, qemu_boot_media_args

GOOD = "\n".join([
    "ZILLAOS_CPU_READY",
    "ZILLAOS_STORAGE_PCI_SCAN_READY",
    "ZILLAOS_STORAGE_CONTROLLER_COUNT=0x0000000000000001",
    "ZILLAOS_STORAGE_BLOCK_DRIVERS_UNAVAILABLE",
    "ZILLAOS_SETUP_READY",
    *[f"ZILLAOS_SETUP_STEP=0x{i:016x}" for i in range(1, 10)],
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
    def test_qemu_keyboard_driver_covers_all_nine_setup_transitions(self):
        self.assertEqual(list(setup_transition_steps()), list(range(1, 10)))
        self.assertEqual(len(list(setup_transition_steps())), 9)

    def test_disk_smoke_uses_read_only_ahci_without_any_cdrom(self):
        from pathlib import Path
        path = Path("/home/test/.local/share/zillaos/installer-lab/zillaos-test.img")
        args = qemu_boot_media_args(None, path)
        self.assertIn("-device", args)
        self.assertIn("ich9-ahci,id=zillasata", args)
        self.assertIn("readonly=on", " ".join(args))
        self.assertIn("bootindex=1", " ".join(args))
        self.assertNotIn("-cdrom", args)
        self.assertNotIn("order=d", " ".join(args))
        with self.assertRaises(ValueError):
            qemu_boot_media_args(path, path)
        with self.assertRaises(ValueError):
            qemu_boot_media_args(None, None)

    def test_live_iso_smoke_remains_available(self):
        from pathlib import Path
        iso = Path("/tmp/test.iso")
        args = qemu_boot_media_args(iso, None)
        self.assertEqual(args, ["-cdrom", str(iso), "-boot", "order=d"])

    def test_complete_default_setup_passes(self):
        self.assertTrue(setup_passed(GOOD))

    def test_custom_setup_preserves_nondefault_choices(self):
        custom = (GOOD
            .replace("ZILLAOS_SETUP_LANGUAGE=uk", "ZILLAOS_SETUP_LANGUAGE=en")
            .replace("ZILLAOS_SETUP_TIMEZONE=Europe/Kyiv", "ZILLAOS_SETUP_TIMEZONE=Europe/Warsaw")
            .replace("ZILLAOS_SETUP_HOSTNAME=zilla-pc", "ZILLAOS_SETUP_HOSTNAME=lab01")
            .replace("ZILLAOS_SETUP_REGION=UA", "ZILLAOS_SETUP_REGION=PL")
            .replace("ZILLAOS_SETUP_THEME=DARK", "ZILLAOS_SETUP_THEME=LIGHT"))
        self.assertTrue(setup_passed(custom, "custom"))
        self.assertFalse(setup_passed(custom, "default"))
        self.assertFalse(setup_passed(GOOD, "custom"))

    def test_storage_scan_requires_count_and_no_disk_driver(self):
        self.assertTrue(storage_scan_passed(GOOD))
        self.assertFalse(storage_scan_passed(GOOD.replace(
            "ZILLAOS_STORAGE_CONTROLLER_COUNT=0x0000000000000001", "")))
        self.assertFalse(storage_scan_passed(GOOD.replace(
            "ZILLAOS_STORAGE_CONTROLLER_COUNT=0x0000000000000001",
            "ZILLAOS_STORAGE_CONTROLLER_COUNT=0x0000000000000009")))
        self.assertFalse(storage_scan_passed(GOOD.replace(
            "ZILLAOS_STORAGE_BLOCK_DRIVERS_UNAVAILABLE", "")))
        self.assertFalse(storage_scan_passed(GOOD +
            "ZILLAOS_INSTALLATION_WRITES_ENABLED\n"))

    def test_unknown_test_profile_rejected(self):
        self.assertFalse(setup_passed(GOOD, "unsafe"))

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
