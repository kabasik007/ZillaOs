"""Safety and syntax tests for independent FAT32 mtools inspection."""
import shutil
import subprocess
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / 'scripts' / 'verify_esp_mtools.sh'


class EspMtoolsContractTests(unittest.TestCase):
    def test_bash_syntax_and_rejected_path_traversal(self):
        bash = shutil.which('bash')
        if bash is None:
            self.skipTest('bash not installed')
        subprocess.run([bash, '-n', str(SCRIPT)], check=True)
        result = subprocess.run([bash, str(SCRIPT), '../host-disk.img'],
                                cwd=ROOT, text=True, capture_output=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn('Expected exactly one safe', result.stderr)

    def test_only_lab_image_is_used_as_input(self):
        script = SCRIPT.read_text(encoding='utf-8')
        self.assertIn('IMAGE="$HOME/.local/share/zillaos/installer-lab/$NAME"', script)
        self.assertIn('python3 scripts/installer_fat32_verify.py "$IMAGE" --check-boot', script)
        self.assertIn('ESP_BYTE_OFFSET=1048576', script)
        self.assertIn('mdir -i "$FAT_IMAGE" ::/', script)
        self.assertIn('mcopy -i "$FAT_IMAGE" ::/LIMINE.CONF', script)
        self.assertIn("from installer_lab import load_uefi_assets", script)
        self.assertIn("ZILLAOS_MTOOLS_ESP_VERIFIED", script)
        self.assertNotIn('mount ', script)
        self.assertNotIn('losetup', script)
        self.assertNotIn('/dev/sd', script)

    def test_no_mutation_of_existing_raw_image(self):
        script = SCRIPT.read_text(encoding='utf-8')
        self.assertIn('TMP="$(mktemp -d)"', script)
        self.assertIn('rm -rf -- "$TMP"', script)
        self.assertNotIn('mformat', script)
        self.assertNotIn('mmd ', script)
        self.assertNotIn('mcopy -o', script)
        self.assertNotIn('qemu-img convert', script)


if __name__ == '__main__':
    unittest.main()
