"""Host source contract checks for disposable VMware installed-disk workflow.

Does not launch VMware or QEMU; real boot verification is a separate gate.
"""
from pathlib import Path
import re
import shutil
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]
EXPORT = ROOT / 'scripts/export_vmware_test.sh'
LAUNCHER = ROOT / 'scripts/vmware_installed.ps1'


class VmwareInstalledTests(unittest.TestCase):
    def test_export_shell_syntax_and_invalid_image_name_fails_closed(self):
        bash = shutil.which('bash')
        if bash is None:
            self.skipTest('Bash is not available on this host')
        subprocess.run([bash, '-n', str(EXPORT)], check=True)
        result = subprocess.run(
            [bash, str(EXPORT), '../danger.img'],
            capture_output=True, text=True
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn('Invalid lab image name', result.stderr)

    def test_new_only_export_and_preflight(self):
        text = EXPORT.read_text(encoding='utf-8')
        self.assertIn('scripts/installer_fat32_verify.py "$SOURCE" --check-boot', text)
        self.assertIn('if [[ -e "$DEST_DIR" || -L "$DEST_DIR" ]]', text)
        self.assertIn('mkdir -m 700 -- "$DEST_DIR"', text)
        self.assertIn('qemu-img convert -p -f raw -O vmdk', text)
        self.assertIn('subformat=monolithicSparse', text)
        self.assertNotIn('--force', text)
        self.assertNotIn('rm -rf', text)

    def test_windows_launcher_requests_confirmation_and_safe_wsl_export(self):
        text = LAUNCHER.read_text(encoding='utf-8')
        self.assertIn('if (-not $ConfirmNewVM)', text)
        self.assertIn('if (Test-Path -LiteralPath $VmFolder)', text)
        self.assertIn("wsl.exe -d $Distro -- bash -lc $command", text)
        self.assertIn("scripts/export_vmware_test.sh", text)
        self.assertIn("if ($LASTEXITCODE -ne 0)", text)
        self.assertNotIn('Remove-Item', text)

    def test_vmware_vmx_is_uefi_disk_only_and_isolated(self):
        text = LAUNCHER.read_text(encoding='utf-8')
        block = re.search(r"\$vmx = @'\n(.*?)\n'@", text, re.DOTALL)
        self.assertIsNotNone(block)
        opts = {}
        for line in block.group(1).splitlines():
            if line.strip():
                key, delim, value = line.partition(' = ')
                self.assertTrue(delim, line)
                self.assertNotIn(key, opts)
                opts[key] = value.strip('"')
        self.assertEqual(opts['firmware'], 'efi')
        self.assertEqual(opts['uefi.secureBoot.enabled'], 'FALSE')
        self.assertEqual(opts['numvcpus'], '1')
        self.assertEqual(opts['sata0:0.fileName'], '@@DISK@@')
        self.assertNotIn('sata0:0.deviceType', opts)
        self.assertFalse(any('cdrom' in value.lower() for value in opts.values()))
        self.assertEqual(opts['ethernet0.present'], 'FALSE')
        self.assertEqual(opts['usb.present'], 'FALSE')
        self.assertEqual(opts['sharedFolder.maxNum'], '0')
        self.assertEqual(opts['serial0.fileName'], '@@SERIAL@@')


if __name__ == '__main__':
    unittest.main()
