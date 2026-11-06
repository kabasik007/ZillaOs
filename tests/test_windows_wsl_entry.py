"""Safe executable checks for the fixed-argv Ubuntu entry script.

Uses temporary stub scripts, never launches WSL, Rust, QEMU, or VMware.
"""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
ENTRY = ROOT / "scripts/windows_wsl_entry.sh"


class WslEntryTests(unittest.TestCase):
    def setUp(self):
        self.bash = shutil.which("bash")
        if not self.bash:
            self.skipTest("bash unavailable")
        self.temp = tempfile.TemporaryDirectory(prefix="zillaos-wsl-test-")
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name) / "repo with spaces ' quotes"
        scripts = self.repo / "scripts"
        scripts.mkdir(parents=True)
        (self.repo / "Cargo.toml").write_text("[package]\n", encoding="utf-8")
        shutil.copyfile(ENTRY, scripts / "windows_wsl_entry.sh")
        (scripts / "windows_wsl_setup.sh").write_text(
            'echo "setup:$*"\n', encoding="utf-8")
        (scripts / "build_vmware.sh").write_text(
            'echo "stub-build-ok"\n', encoding="utf-8")
        (scripts / "desktop_smoke.py").write_text(
            'print("stub-smoke-ok")\n', encoding="utf-8")

    def invoke(self, *args):
        return subprocess.run(
            [self.bash, str(ENTRY), str(self.repo), *args],
            cwd=ROOT, capture_output=True, text=True)

    def test_bash_syntax(self):
        subprocess.run([self.bash, "-n", str(ENTRY)], check=True)

    def test_spaces_and_apostrophe_are_preserved(self):
        r = self.invoke("build")
        self.assertEqual(r.returncode, 0, (r.stdout, r.stderr))
        self.assertIn("stub-build-ok", r.stdout)
        log = self.repo / "build/logs/windows-iso-build.log"
        self.assertEqual(log.read_text(encoding="utf-8").strip(), "stub-build-ok")

    def test_tee_does_not_hide_build_failure(self):
        (self.repo / "scripts/build_vmware.sh").write_text(
            'echo "stub-build-failed"; exit 23\n', encoding="utf-8")
        r = self.invoke("build")
        self.assertEqual(r.returncode, 23, (r.stdout, r.stderr))
        self.assertIn("stub-build-failed",
                      (self.repo / "build/logs/windows-iso-build.log").read_text())

    def test_check_and_setup_do_not_interpolate_arguments(self):
        check = self.invoke("check", "--with-qemu")
        self.assertEqual(check.returncode, 0, check.stderr)
        self.assertIn("setup:--check --with-qemu", check.stdout)
        setup = self.invoke("setup")
        self.assertEqual(setup.returncode, 0, setup.stderr)
        self.assertIn("setup:--install --install-rust", setup.stdout)

    def test_bad_mode_fails_without_running_build(self):
        result = self.invoke("unsupported")
        self.assertEqual(result.returncode, 2)
        self.assertFalse((self.repo / "build").exists())


if __name__ == "__main__":
    unittest.main()
