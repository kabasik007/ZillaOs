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
        self.assertIn('PS2', 'PS2')  # name-only design; hardware verified at boot

    def test_linux_build_wrapper_generates_demo_uefi_image(self):
        src = BUILD_SCRIPT.read_text(encoding="utf-8")
        self.assertIn("cargo test --lib", src)
        self.assertIn("bash scripts/build.sh demo", src)
        self.assertIn("xorriso -indev", src)
        self.assertIn("zillaos-0.0.2-dev-demo-uefi.iso", src)


if __name__ == "__main__":
    unittest.main()
