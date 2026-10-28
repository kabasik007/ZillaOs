"""Tests for non-writing COM1 kernel/setup boot evidence checks."""
import tempfile
import unittest
from pathlib import Path
from scripts.boot_evidence import analyse, analyse_file

VALID = '\n'.join([
    'ZILLAOS_CPU_READY',
    'ZILLAOS_STORAGE_PCI_SCAN_READY',
    'ZILLAOS_SETUP_READY',
    'ZILLAOS_FRAMEBUFFER_READY',
    'ZILLAOS_BOOT_OK',
    'ZILLAOS_DEMO_READY',
    'ZILLAOS_SETUP_INPUT_POLL',
]) + '\n'

class BootEvidenceTests(unittest.TestCase):
    def test_complete_setup_boot_evidence(self):
        r = analyse(VALID)
        self.assertTrue(r['success'])
        self.assertTrue(r['setup_seen'])
        self.assertFalse(r['desktop_seen'])

    def test_empty_log_cannot_pass(self):
        self.assertEqual(analyse('')['classification'], 'kernel_boot_marker_missing')

    def test_kernel_marker_alone_not_enough_for_installer(self):
        self.assertFalse(analyse('ZILLAOS_BOOT_OK')['success'])
        self.assertTrue(analyse('ZILLAOS_BOOT_OK', require_setup=False)['success'])

    def test_reordered_markers_are_rejected(self):
        changed = VALID.replace('ZILLAOS_SETUP_READY\n', '').replace('ZILLAOS_BOOT_OK\n', 'ZILLAOS_BOOT_OK\nZILLAOS_SETUP_READY\n')
        self.assertEqual(analyse(changed)['classification'], 'unexpected_boot_marker_order')

    def test_boot_failure_overrides_success_markers(self):
        self.assertFalse(analyse(VALID + 'ZILLAOS_PANIC\n')['success'])
        self.assertFalse(analyse(VALID + 'ZILLAOS_BOOT_FAIL\n')['success'])

    def test_missing_storage_scan_marker_rejected(self):
        self.assertEqual(analyse(VALID.replace('ZILLAOS_STORAGE_PCI_SCAN_READY\n', ''))['classification'], 'storage_scan_marker_missing')

    def test_framebuffer_unavailable_rejected(self):
        self.assertEqual(analyse(VALID+'ZILLAOS_SETUP_NO_FRAMEBUFFER\n')['classification'], 'setup_framebuffer_unavailable')

    def test_missing_framebuffer_ready_is_rejected(self):
        log = VALID.replace('ZILLAOS_FRAMEBUFFER_READY\n', '')
        self.assertEqual(analyse(log)['classification'], 'framebuffer_ready_marker_missing')

    def test_missing_setup_input_poll_is_rejected(self):
        log = VALID.replace('ZILLAOS_SETUP_INPUT_POLL\n', '')
        self.assertEqual(analyse(log)['classification'], 'setup_input_poll_missing')

    def test_duplicated_old_boot_marker_is_rejected(self):
        log = VALID + 'ZILLAOS_BOOT_OK\n'
        self.assertEqual(analyse(log)['classification'], 'duplicate_boot_markers')

    def test_out_of_order_storage_scan_is_rejected(self):
        log = VALID.replace('ZILLAOS_STORAGE_PCI_SCAN_READY\n','') + 'ZILLAOS_STORAGE_PCI_SCAN_READY\n'
        self.assertEqual(analyse(log)['classification'], 'unexpected_boot_marker_order')

    def test_duplicate_setup_marker_is_rejected(self):
        log = VALID + 'ZILLAOS_SETUP_READY\n'
        self.assertEqual(analyse(log)['classification'], 'duplicate_setup_markers')

    def test_framebuffer_error_remains_fatal(self):
        log = VALID + 'ZILLAOS_SETUP_SKIPPED_NO_FRAMEBUFFER\n'
        self.assertEqual(analyse(log)['classification'], 'setup_framebuffer_unavailable')

    def test_file_and_symlink_behavior(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'serial.log'; p.write_text(VALID)
            self.assertTrue(analyse_file(p)['success'])
            symlink=Path(d)/'link.log'; symlink.symlink_to(p)
            with self.assertRaisesRegex(ValueError,'symlink'):
                analyse_file(symlink)

if __name__ == '__main__': unittest.main()
