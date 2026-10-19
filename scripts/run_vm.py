#!/usr/bin/env python3
"""Launch the ZillaOS graphical demo safely in QEMU/OVMF (no host disks)."""
from __future__ import annotations

import argparse
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

if __package__:
    from .smoke_test import OVMF_PAIRS, ROOT
else:
    from smoke_test import OVMF_PAIRS, ROOT

DEMO_ISO = ROOT / "dist" / "zillaos-0.0.1-demo-uefi.iso"


def find_ovmf(code_override: Path | None, vars_override: Path | None) -> tuple[Path, Path] | None:
    if (code_override is None) != (vars_override is None):
        raise ValueError("Both --ovmf-code and --ovmf-vars must be supplied together")
    if code_override is not None:
        if code_override.is_file() and vars_override is not None and vars_override.is_file():
            return code_override.resolve(), vars_override.resolve()
        return None
    for code, vars_template in OVMF_PAIRS:
        a, b = Path(code), Path(vars_template)
        if a.is_file() and b.is_file():
            return a, b
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--iso", type=Path, default=DEMO_ISO)
    parser.add_argument("--display", choices=["gtk", "sdl", "none"], default="gtk")
    parser.add_argument("--memory", type=int, default=512, help="guest RAM MiB (256..4096)")
    parser.add_argument("--ovmf-code", type=Path)
    parser.add_argument("--ovmf-vars", type=Path)
    args = parser.parse_args()

    if not 256 <= args.memory <= 4096:
        parser.error("--memory must be 256..4096 MiB")
    iso = args.iso.resolve()
    if not iso.is_file() or iso.stat().st_size == 0:
        print("Demo ISO not found. Run: bash scripts/build.sh demo", file=sys.stderr)
        return 2
    if not shutil.which("qemu-system-x86_64"):
        print("qemu-system-x86_64 is missing; install qemu-system-x86", file=sys.stderr)
        return 2

    try:
        ovmf = find_ovmf(args.ovmf_code, args.ovmf_vars)
    except ValueError as exc:
        parser.error(str(exc))
    if ovmf is None:
        print("Matching OVMF_CODE/OVMF_VARS not found; install ovmf", file=sys.stderr)
        return 2

    code, vars_template = ovmf
    with tempfile.TemporaryDirectory(prefix="zillaos-demo-") as tmp:
        vars_copy = Path(tmp) / "OVMF_VARS.fd"
        shutil.copyfile(vars_template, vars_copy)
        command = [
            "qemu-system-x86_64",
            "-machine", "q35,accel=tcg",
            "-cpu", "qemu64",
            "-m", str(args.memory),
            "-smp", "1",
            "-vga", "std",
            "-display", args.display,
            "-serial", "stdio",
            "-monitor", "none",
            "-net", "none",
            "-device", "isa-debug-exit,iobase=0xf4,iosize=0x04",
            "-drive", f"if=pflash,format=raw,unit=0,readonly=on,file={code}",
            "-drive", f"if=pflash,format=raw,unit=1,file={vars_copy}",
            "-cdrom", str(iso),
            "-boot", "order=d",
        ]
        print(f"ZillaOS demo VM: {args.memory} MiB, 1 vCPU, display={args.display}", flush=True)
        print("Close the QEMU window or press Ctrl+C to stop the demo.", flush=True)
        try:
            result = subprocess.run(command, check=False)
            if result.returncode not in (0, 33):
                print(f"QEMU terminated with exit code {result.returncode}", file=sys.stderr)
                return 1
        except KeyboardInterrupt:
            print("\nVM stopped by user.")
        except OSError as exc:
            print(f"Cannot launch QEMU: {exc}", file=sys.stderr)
            return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
