"""Static + safe negative execution checks for Windows ISO build entry points.

No Windows OS, WSL, packages, Rust or VM is launched by these tests.
"""
from pathlib import Path
import shutil
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]
BASH_SETUP = ROOT / "scripts/windows_wsl_setup.sh"
PS = ROOT / "scripts/windows.ps1"
CMD = ROOT / "ZillaOS-Windows.cmd"


class WindowsEntrypointTests(unittest.TestCase):
    def test_wsl_setup_script_syntax_and_invalid_flags(self):
        bash = shutil.which("bash")
        if not bash:
            self.skipTest("bash not available")
        subprocess.run([bash, "-n", str(BASH_SETUP)], check=True)
        for args in (["--unsupported"], ["--check", "--install-rust"]):
            result = subprocess.run([bash, str(BASH_SETUP), *args],
                                    cwd=ROOT, text=True, capture_output=True)
            self.assertEqual(result.returncode, 2, (args, result.stdout, result.stderr))

    def test_setup_has_explicit_install_and_pinned_rust(self):
        source = BASH_SETUP.read_text(encoding="utf-8")
        self.assertIn('MODE="${1:---check}"', source)
        self.assertIn('if [[ "$MODE" == "--install" ]]', source)
        self.assertIn('if [[ "$MODE" == "--install" && "$INSTALL_RUST" -eq 1 ]]', source)
        self.assertIn('sudo apt-get install', source)
        self.assertIn('rustup toolchain install 1.90.0', source)
        self.assertIn('--target x86_64-unknown-none', source)
        self.assertIn('xorriso', source)
        self.assertNotIn("dd if=", source)
        self.assertNotIn("mkfs.", source)
        self.assertNotIn("diskpart", source.lower())

    def test_powershell_wsl_build_and_iso_fingerprint(self):
        source = PS.read_text(encoding="utf-8")
        for action in ("Doctor", "Setup", "Build", "BuildRun", "Run", "OpenISO", "Smoke"):
            self.assertIn('"' + action + '"', source)
        self.assertIn("wsl.exe -d $Distro -- bash -lc", source)
        self.assertIn("wslpath -a $RepoRoot", source)
        self.assertIn("bash scripts/build_vmware.sh", source)
        self.assertIn("ZillaOS-Desktop-UEFI.iso", source)
        self.assertIn("Get-FileHash", source)
        self.assertIn("Move-Item -LiteralPath $temporary -Destination $FriendlyIso -Force", source)
        self.assertIn('ISO copy verification failed', source)
        self.assertIn('"vmware.ps1"', source)

    def test_windows_menu_routes_each_option_without_unconditional_goto(self):
        source = CMD.read_text(encoding="ascii")
        self.assertIn("choice /C 123456780 /N", source)
        self.assertIn('set "args="', source)
        self.assertIn("if not defined args goto menu", source)
        for option in range(1, 9):
            self.assertIn('if "%pick%"=="' + str(option) + '" set "args=', source)
        self.assertIn('if "%pick%"=="9" goto done', source)
        self.assertNotIn('" & goto run', source)
        self.assertIn('powershell.exe -NoProfile -ExecutionPolicy Bypass', source)

    def test_real_iso_pipeline_uses_uefi_el_torito_limine(self):
        builder = (ROOT / "scripts/build.sh").read_text(encoding="utf-8")
        vm = (ROOT / "scripts/build_vmware.sh").read_text(encoding="utf-8")
        self.assertIn('LIMINE_VERSION="12.9.4"', builder)
        self.assertIn("sha256sum --check --status", builder)
        self.assertIn('cargo build --release --target x86_64-unknown-none', builder)
        self.assertIn('xorriso -as mkisofs', builder)
        self.assertIn('--efi-boot boot/limine-uefi-cd.bin', builder)
        self.assertIn('bash scripts/build.sh demo', vm)
        self.assertIn('python3 scripts/check_boot_assets.py', vm)


if __name__ == "__main__":
    unittest.main()
