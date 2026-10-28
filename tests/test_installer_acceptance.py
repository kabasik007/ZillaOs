"""Acceptance script only touches disposable lab RAW paths on explicit opt-in."""
from pathlib import Path
import shutil
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]
ACCEPT = ROOT / 'scripts/installer_acceptance.sh'


class InstallerAcceptanceTests(unittest.TestCase):
    def test_syntax_and_nonconsensual_operations_are_refused_before_build(self):
        bash = shutil.which('bash')
        if not bash:
            self.skipTest('Bash missing')
        subprocess.run([bash, '-n', str(ACCEPT)], check=True)
        for args in (
            ['--create'],
            ['--boot'],
            ['--invalid'],
            ['--preflight', 'CREATE-NEW-VM-IMAGE'],
        ):
            result = subprocess.run([bash, str(ACCEPT), *args],
                                    cwd=ROOT, capture_output=True, text=True)
            self.assertEqual(result.returncode, 2, (args, result.stderr))

    def test_build_gate_precedes_disk_creation(self):
        script = ACCEPT.read_text(encoding='utf-8')
        self.assertLess(script.index('bash scripts/build_vmware.sh'),
                        script.index('python3 scripts/installer_lab.py'))
        self.assertIn('set -Eeuo pipefail', script)
        self.assertIn('if [[ "$MODE" == "--preflight" ]]', script)
        self.assertIn('python3 scripts/check_boot_assets.py', script)
        self.assertIn('secrets.token_hex(6)', script)
        self.assertIn('--create --confirm CREATE-NEW-VM-IMAGE', script)

    def test_read_only_inspection_before_qemu_and_logs_are_from_current_run(self):
        s = ACCEPT.read_text(encoding='utf-8')
        self.assertLess(s.index('python3 scripts/installer_fat32_verify.py "$IMAGE"'),
                        s.index('bash scripts/verify_esp_mtools.sh'))
        self.assertLess(s.index('bash scripts/verify_esp_mtools.sh'),
                        s.index('timeout 100s bash scripts/test_uefi_disk_qemu.sh'))
        self.assertIn("sed -n 's/^Serial log: //p'", s)
        self.assertIn('installer-uefi-disk-serial*.log', s)
        self.assertIn('python3 scripts/boot_evidence.py "$SERIAL"', s)
        self.assertNotIn('mount /dev/', s)
        self.assertNotIn('parted ', s)
        self.assertNotIn('mkfs.', s)


if __name__ == '__main__':
    unittest.main()
