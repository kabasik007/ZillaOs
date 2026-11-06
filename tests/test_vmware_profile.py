"""Portable verification of the Windows VMware profile generator.

These tests validate the committed template, NOT VMware's actual acceptance
of a generated VMX file. Real Workstation UEFI boot remains a separate gate.
"""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]
PS_SCRIPT = ROOT / "scripts" / "vmware.ps1"
BUILD_SCRIPT = ROOT / "scripts" / "build_vmware.sh"

def profile_values() -> dict[str, str]:
    source = PS_SCRIPT.read_text(encoding="utf-8")
    match = re.search(r"\$vmx = @'\n(.*?)\n'@", source, flags=re.DOTALL)
    if match is None:
        raise ValueError("VMware VMX here-string not found")
    options = {}
    for line in match.group(1).splitlines():
        if line and not line.startswith("#"):
            key, sep, value = line.partition(" = ")
            if not sep or key in options:
                raise ValueError(f"Malformed or duplicate VMX property: {line}")
            options[key] = value.strip('"')
    return options


class VMwareProfileTests(unittest.TestCase):
    def test_firmware_is_unsigned_uefi_with_single_cpu(self):
        options = profile_values()
        self.assertEqual(options["firmware"], "efi")
        self.assertEqual(options["uefi.secureBoot.enabled"], "FALSE")
        self.assertEqual(options["numvcpus"], "1")
        self.assertEqual(options["memsize"], "1024")
        self.assertEqual(options["guestOS"], "other-64")

    def test_only_iso_drive_exists(self):
        options = profile_values()
        self.assertEqual(options["sata0:0.deviceType"], "cdrom-image")
        self.assertEqual(options["sata0:0.fileName"], "@@ISO@@")
        self.assertEqual(options["sata0:0.startConnected"], "TRUE")
        self.assertEqual(options["scsi0.present"], "FALSE")
        self.assertEqual(options["nvme0.present"], "FALSE")
        self.assertFalse(any(value.endswith(".vmdk") for value in options.values()))

    def test_network_and_usb_passthrough_disabled(self):
        options = profile_values()
        self.assertEqual(options["ethernet0.present"], "FALSE")
        self.assertEqual(options["usb.present"], "FALSE")
        self.assertEqual(options["mouse.vusb.enable"], "FALSE")
        self.assertEqual(options["keyboard.vusb.enable"], "FALSE")
        self.assertEqual(options["mks.enable3d"], "FALSE")

    def test_com1_logging_and_iso_validation_in_powershell(self):
        options = profile_values()
        self.assertEqual(options["serial0.present"], "TRUE")
        self.assertEqual(options["serial0.fileType"], "file")
        self.assertEqual(options["serial0.fileName"], "@@SERIAL@@")
        src = PS_SCRIPT.read_text(encoding="utf-8")
        self.assertIn("Test-Path -LiteralPath $IsoPath -PathType Leaf", src)
        self.assertIn("startConnected", src)
        self.assertIn('Start-Process -FilePath $exe', src)
        # The VMware helper delegates builds to the repaired Windows entrypoint.
        # Requiring the old implicit WSL shell would reintroduce broken backslashes.
        self.assertIn('"windows.ps1") -Action Build -Distro $Distro', src)
        self.assertNotIn('wsl.exe -d $Distro -- bash -lc', src)
        self.assertNotIn('wslpath -a $RepoRoot', src)

    def test_preserved_vmx_checks_actual_cd_dvd_mapping_without_overwriting(self):
        src = PS_SCRIPT.read_text(encoding="utf-8")
        self.assertIn("[IO.File]::ReadAllText($vmxPath)", src)
        self.assertIn("sata0:0", src)
        self.assertIn("newly", src.lower().replace("newly built iso", "newly"))
        self.assertIn("DIFFERENT ISO", src)
        self.assertIn("Existing VMX CD/DVD mapping verified", src)
        self.assertIn("[StringComparison]::OrdinalIgnoreCase", src)
        self.assertIn("Power off the VM", src)
        self.assertIn("if (-not $Force)", src)
        # The pre-existing VMX branch must finish before the only VMX write.
        self.assertLess(src.index("if (-not $Force)"), src.index("[IO.File]::WriteAllText($vmxPath"))

    def test_vmware_launcher_searches_32bit_and_64bit_install_paths(self):
        src = PS_SCRIPT.read_text(encoding="utf-8")
        self.assertIn("function Get-ZillaVmwareExe", src)
        self.assertIn(r"${env:ProgramFiles(x86)}", src)
        self.assertIn(r"$env:ProgramFiles", src)
        self.assertIn(r"VMware\VMware Workstation\vmware.exe", src)
        self.assertIn(r"VMware\VMware Workstation\bin\vmware.exe", src)
        self.assertIn(r"Broadcom\VMware Workstation\bin\vmware.exe", src)
        self.assertIn("Get-Command -Name \"vmware.exe\"", src)
        self.assertIn("Test-Path -LiteralPath $candidate -PathType Leaf", src)
        self.assertEqual(src.count("Open-ZillaVmware $vmxPath"), 2)
        self.assertEqual(src.count("Start-Process -FilePath $exe"), 1)
        self.assertNotIn("VMware executable not located.", src)

    def test_linux_build_wrapper_generates_demo_uefi_image(self):
        src = BUILD_SCRIPT.read_text(encoding="utf-8")
        self.assertIn("cargo test --lib", src)
        self.assertIn("bash scripts/build.sh demo", src)
        self.assertIn("xorriso -indev", src)
        self.assertIn("zillaos-0.0.2-dev-demo-uefi.iso", src)


if __name__ == "__main__":
    unittest.main()
