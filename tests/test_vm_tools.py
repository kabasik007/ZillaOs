"""Fast host-side tests: intentionally no QEMU, Rust or network dependency."""
from pathlib import Path
import tempfile
import unittest

from scripts import smoke_test, run_vm


class BootEvidenceTests(unittest.TestCase):
    def test_success_requires_both_signals(self):
        self.assertTrue(smoke_test.boot_passed(
            33, "ZILLAOS_MEMORY_READY\nZILLAOS_FRAMEBUFFER_READY\nZILLAOS_BOOT_OK\n"))

    def test_missing_memory_validation_fails(self):
        self.assertFalse(smoke_test.boot_passed(33, "ZILLAOS_BOOT_OK"))

    def test_missing_success_marker_fails(self):
        self.assertFalse(smoke_test.boot_passed(33, "UEFI booted, not our kernel"))

    def test_failure_marker_even_with_success_fails(self):
        self.assertFalse(smoke_test.boot_passed(
            33, "ZILLAOS_MEMORY_READY\nZILLAOS_BOOT_OK\nZILLAOS_PANIC\n"))

    def test_wrong_exit_even_with_marker_fails(self):
        self.assertFalse(smoke_test.boot_passed(0, "ZILLAOS_MEMORY_READY\nZILLAOS_BOOT_OK"))

    def test_explicit_fail_marker_fails(self):
        self.assertFalse(smoke_test.boot_passed(
            33, "ZILLAOS_MEMORY_READY\nZILLAOS_BOOT_FAIL\nZILLAOS_BOOT_OK"))


class VmConfigurationTests(unittest.TestCase):
    def test_ovmf_requires_matching_pair(self):
        with tempfile.TemporaryDirectory() as path:
            code = Path(path, "OVMF_CODE.fd")
            vars_image = Path(path, "OVMF_VARS.fd")
            code.write_bytes(b"test-code")
            vars_image.write_bytes(b"test-vars")
            self.assertEqual(
                run_vm.find_ovmf(code, vars_image),
                (code.resolve(), vars_image.resolve()),
            )

    def test_incomplete_ovmf_override_fails(self):
        with self.assertRaises(ValueError):
            run_vm.find_ovmf(Path("/tmp/ovmf-code"), None)

    def test_nonexistent_ovmf_pair_rejected(self):
        with tempfile.TemporaryDirectory() as path:
            self.assertIsNone(run_vm.find_ovmf(
                Path(path, "missing-code"), Path(path, "missing-vars")))


if __name__ == "__main__":
    unittest.main()
