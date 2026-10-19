#!/usr/bin/env python3
"""Read-only ZillaOS host requirements diagnostic; never installs anything."""
from __future__ import annotations

import argparse
from pathlib import Path
import shutil
import subprocess
import sys

OVMF_PAIRS = [
    ("/usr/share/OVMF/OVMF_CODE_4M.fd", "/usr/share/OVMF/OVMF_VARS_4M.fd"),
    ("/usr/share/OVMF/OVMF_CODE.fd", "/usr/share/OVMF/OVMF_VARS.fd"),
    ("/usr/share/edk2/ovmf/OVMF_CODE.fd", "/usr/share/edk2/ovmf/OVMF_VARS.fd"),
]


def has_ovmf() -> bool:
    return any(Path(a).is_file() and Path(b).is_file() for a, b in OVMF_PAIRS)


def check_environment() -> tuple[list[str], list[str]]:
    """Return deterministic (available, missing) required component names."""
    required = (
        "cargo", "rustc", "curl", "tar", "sha256sum", "find",
        "xorriso", "qemu-system-x86_64", "python3",
    )
    available = [name for name in required if shutil.which(name) is not None]
    missing = [name for name in required if name not in available]
    (available if has_ovmf() else missing).append("OVMF firmware pair")
    return available, missing


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--quiet", action="store_true", help="only output missing components")
    args = parser.parse_args()
    available, missing = check_environment()

    if not args.quiet:
        for name in available:
            print(f"[OK]      {name}")
    for name in missing:
        print(f"[MISSING] {name}")

    if missing:
        print("\nInstall missing dependencies; this diagnostic changes nothing.")
        return 1
    print("\nHost dependencies found. Actual Rust build and QEMU boot still need testing.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
