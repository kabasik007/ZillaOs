#!/usr/bin/env python3
"""QEMU/OVMF serial smoke test for ZillaOS M1; only uses disposable VM images."""
from __future__ import annotations

import argparse
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_ISO = ROOT / "dist" / "zillaos-0.0.1-test-uefi.iso"
OVMF_PAIRS = [
    ("/usr/share/OVMF/OVMF_CODE_4M.fd", "/usr/share/OVMF/OVMF_VARS_4M.fd"),
    ("/usr/share/OVMF/OVMF_CODE.fd", "/usr/share/OVMF/OVMF_VARS.fd"),
    ("/usr/share/edk2/ovmf/OVMF_CODE.fd", "/usr/share/edk2/ovmf/OVMF_VARS.fd"),
]
SUCCESS_MARKER = "ZILLAOS_BOOT_OK"
MEMORY_MARKER = "ZILLAOS_MEMORY_READY"
FAILURE_MARKERS = ("ZILLAOS_PANIC", "ZILLAOS_BOOT_FAIL")
EXPECTED_EXIT = 33  # QEMU debug-exit: (0x10 << 1) | 1.


def boot_passed(exit_code: int, output: str) -> bool:
    """Evidence gate: BOTH correct QEMU exit status and serial marker required."""
    return (
        exit_code == EXPECTED_EXIT
        and SUCCESS_MARKER in output
        and MEMORY_MARKER in output
        and not any(marker in output for marker in FAILURE_MARKERS)
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--iso", type=Path, default=DEFAULT_ISO)
    parser.add_argument("--timeout", type=int, default=60)
    parser.add_argument("--ovmf-code", type=Path)
    parser.add_argument("--ovmf-vars", type=Path)
    args = parser.parse_args()

    if args.timeout < 1:
        parser.error("--timeout must be positive")

    iso = args.iso.resolve()
    if not iso.is_file() or iso.stat().st_size == 0:
        print(f"FAIL: missing or empty boot image: {iso}", file=sys.stderr)
        return 2

    if not shutil.which("qemu-system-x86_64"):
        print("FAIL: qemu-system-x86_64 is not installed", file=sys.stderr)
        return 2

    if args.ovmf_code or args.ovmf_vars:
        if not args.ovmf_code or not args.ovmf_vars:
            parser.error("both --ovmf-code and --ovmf-vars are required together")
        code, vars_template = args.ovmf_code, args.ovmf_vars
    else:
        code, vars_template = next(
            ((Path(a), Path(b)) for a, b in OVMF_PAIRS if Path(a).is_file() and Path(b).is_file()),
            (None, None),
        )

    if code is None or vars_template is None or not code.is_file() or not vars_template.is_file():
        print("FAIL: matching OVMF_CODE and OVMF_VARS images not found", file=sys.stderr)
        return 2

    with tempfile.TemporaryDirectory(prefix="zillaos-qemu-") as tempdir:
        vars_copy = Path(tempdir) / "OVMF_VARS.fd"
        shutil.copyfile(vars_template, vars_copy)
        command = [
            "qemu-system-x86_64",
            "-machine", "q35,accel=tcg",
            "-cpu", "qemu64",
            "-m", "512M",
            "-smp", "1",
            "-vga", "std",
            "-net", "none",
            "-display", "none",
            "-monitor", "none",
            "-serial", "stdio",
            "-no-reboot",
            "-no-shutdown",
            "-device", "isa-debug-exit,iobase=0xf4,iosize=0x04",
            "-drive", f"if=pflash,format=raw,unit=0,readonly=on,file={code}",
            "-drive", f"if=pflash,format=raw,unit=1,file={vars_copy}",
            "-cdrom", str(iso),
            "-boot", "order=d",
        ]
        print(f"Booting {iso.name} in isolated QEMU/OVMF (timeout {args.timeout}s)", flush=True)
        try:
            result = subprocess.run(
                command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                text=True, errors="replace", timeout=args.timeout, check=False,
            )
        except subprocess.TimeoutExpired as exc:
            output = exc.stdout or b""
            if isinstance(output, bytes):
                output = output.decode("utf-8", errors="replace")
            print(output)
            print("FAIL: QEMU boot timed out", file=sys.stderr)
            return 1
        except OSError as exc:
            print(f"FAIL: could not launch QEMU: {exc}", file=sys.stderr)
            return 2

        output = result.stdout
        print(output)
        if not boot_passed(result.returncode, output):
            print(
                f"FAIL: exit={result.returncode}, marker={SUCCESS_MARKER in output}",
                file=sys.stderr,
            )
            return 1
        print("PASS: ZillaOS booted, validated Limine response and exited via QEMU device")
        return 0


if __name__ == "__main__":
    sys.exit(main())
