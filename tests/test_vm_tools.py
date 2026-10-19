"""Host-only black-box verdict tests; no QEMU or Rust required."""
from pathlib import Path
import tempfile
import unittest

from scripts import smoke_test, run_vm


GOOD = (
    "ZillaOS v0.0.2-dev | x86-64 | Limine\r\n"
    "ZILLAOS_IDT_READY\r\n"
    "ZILLAOS_CPU_PHYSICAL_BITS=0x0000000000000024\r\n"
    "ZILLAOS_CPU_VIRTUAL_BITS=0x0000000000000030\r\n"
    "ZILLAOS_CPU_READY\r\n"
    "ZILLAOS_MEMMAP_REGIONS=0x0000000000000010\r\n"
    "ZILLAOS_USABLE_FRAMES=0x0000000000001234\r\n"
    "ZILLAOS_TEST_FRAME_1=0x0000000000001000\r\n"
    "ZILLAOS_TEST_FRAME_2=0x0000000000002000\r\n"
    "ZILLAOS_HHDM_OFFSET=0xffff800000000000\r\n"
    "ZILLAOS_FRAME_PROBE_OK\r\n"
    "ZILLAOS_MEMORY_READY\r\n"
    "ZILLAOS_BOOT_OK\r\n"
)


FAULT = (
    "ZillaOS v0.0.2-dev | x86-64 | Limine\r\n"
    "ZILLAOS_IDT_READY\r\n"
    "ZILLAOS_EXCEPTION_VECTOR=0x0000000000000006\r\n"
    "ZILLAOS_EXCEPTION_ERROR=0x0000000000000000\r\n"
    "ZILLAOS_EXCEPTION_RIP=0xffffffff80001000\r\n"
    "ZILLAOS_EXCEPTION_FATAL\r\n"
)


class BootEvidenceTests(unittest.TestCase):
    def test_complete_evidence_passes(self):
        self.assertTrue(smoke_test.boot_passed(33, GOOD))

    def test_substring_marker_does_not_pass(self):
        self.assertFalse(smoke_test.boot_passed(
            33, GOOD.replace("ZILLAOS_BOOT_OK", "boot: ZILLAOS_BOOT_OK is not ready")))

    def test_missing_idt_ready_fails(self):
        self.assertFalse(smoke_test.boot_passed(
            33, GOOD.replace("ZILLAOS_IDT_READY", "NO_IDT")))

    def test_wrong_exit_fails(self):
        self.assertFalse(smoke_test.boot_passed(0, GOOD))

    def test_missing_frame_probe_fails(self):
        self.assertFalse(smoke_test.boot_passed(
            33, GOOD.replace("ZILLAOS_FRAME_PROBE_OK", "FRAME_PROBE_UNAVAILABLE")))

    def test_missing_memory_ready_fails(self):
        self.assertFalse(smoke_test.boot_passed(
            33, GOOD.replace("ZILLAOS_MEMORY_READY", "NO_MEMORY")))

    def test_aliased_frames_fail(self):
        self.assertFalse(smoke_test.boot_passed(
            33, GOOD.replace("ZILLAOS_TEST_FRAME_2=0x0000000000002000",
                             "ZILLAOS_TEST_FRAME_2=0x0000000000001000")))

    def test_unaligned_frame_fails(self):
        self.assertFalse(smoke_test.boot_passed(
            33, GOOD.replace("ZILLAOS_TEST_FRAME_2=0x0000000000002000",
                             "ZILLAOS_TEST_FRAME_2=0x0000000000002001")))

    def test_address_exceeding_cpuid_width_fails(self):
        self.assertFalse(smoke_test.boot_passed(
            33, GOOD.replace("ZILLAOS_TEST_FRAME_2=0x0000000000002000",
                             "ZILLAOS_TEST_FRAME_2=0x0000002000000000")))

    def test_invalid_hhdm_fails(self):
        self.assertFalse(smoke_test.boot_passed(
            33, GOOD.replace("0xffff800000000000", "0x0000000000000000")))

    def test_duplicate_value_line_fails(self):
        self.assertFalse(smoke_test.boot_passed(
            33, GOOD + "ZILLAOS_USABLE_FRAMES=0x0000000000001234\n"))

    def test_bad_failure_marker_fails(self):
        self.assertFalse(smoke_test.boot_passed(33, GOOD + "ZILLAOS_PANIC\n"))

    def test_parser_accepts_crlf(self):
        self.assertEqual(
            smoke_test.serial_hex("ZILLAOS_CPU_PHYSICAL_BITS=0x24\r\n",
                                  "ZILLAOS_CPU_PHYSICAL_BITS"),
            36)


class ExceptionEvidenceTests(unittest.TestCase):
    def test_ud2_trap_evidence_passes(self):
        self.assertTrue(smoke_test.fault_passed(35, FAULT))

    def test_triple_fault_exit_does_not_pass(self):
        self.assertFalse(smoke_test.fault_passed(0, FAULT))

    def test_wrong_exception_vector_fails(self):
        self.assertFalse(smoke_test.fault_passed(
            35, FAULT.replace("=0x0000000000000006", "=0x000000000000000e")))

    def test_fault_without_diagnostics_fails(self):
        self.assertFalse(smoke_test.fault_passed(
            35, FAULT.replace("ZILLAOS_EXCEPTION_FATAL", "KERNEL_RESET")))

    def test_normal_boot_does_not_pass_as_fault(self):
        self.assertFalse(smoke_test.fault_passed(33, GOOD))


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
