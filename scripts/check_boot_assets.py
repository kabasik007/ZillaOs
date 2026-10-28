#!/usr/bin/env python3
"""Non-writing boot artifact preflight before staging a ZillaOS EFI disk."""
from __future__ import annotations
import json
import sys
from boot_assets_audit import BootAssetError, validate_assets


def main() -> int:
    try:
        # Keep authoritative artifact paths and config rewriting in one place.
        from installer_lab import load_uefi_assets
        assets = load_uefi_assets()
        print(json.dumps({'status': 'boot-artifact-preflight-ok',
                          'assets': validate_assets(assets)}, indent=2))
        return 0
    except (BootAssetError, ValueError, OSError, UnicodeError) as exc:
        print('BOOT ARTIFACT PREFLIGHT FAILED:', exc, file=sys.stderr)
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
