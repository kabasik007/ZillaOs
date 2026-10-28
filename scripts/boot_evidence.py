#!/usr/bin/env python3
"""Read-only COM1 boot evidence checker. No VM or disk modifications."""
from __future__ import annotations
import argparse
import json
from pathlib import Path
import stat
import sys

MAX_LOG = 4 * 1024 * 1024
FATAL = ('ZILLAOS_PANIC', 'ZILLAOS_BOOT_FAIL')


def analyse(lines: str, *, require_setup: bool = True) -> dict:
    events = [x.strip() for x in lines.splitlines() if x.strip()]
    def position(marker: str) -> int:
        try:
            return events.index(marker)
        except ValueError:
            return -1
    boot = position('ZILLAOS_BOOT_OK')
    setup = position('ZILLAOS_SETUP_READY')
    desktop = position('ZILLAOS_DESKTOP_READY')
    crash = next((x for x in FATAL if position(x) >= 0), None)
    error = None
    if crash:
        error = 'guest_' + crash.lower()
    elif boot < 0:
        error = 'kernel_boot_marker_missing'
    elif require_setup and setup < 0:
        error = 'setup_marker_missing'
    elif require_setup and setup >= boot:
        # Real vm-demo starts Setup BEFORE declaring BOOT_OK.
        error = 'unexpected_setup_boot_marker_order'
    elif require_setup and position('ZILLAOS_STORAGE_PCI_SCAN_READY') < 0:
        error = 'storage_scan_marker_missing'
    elif require_setup and position('ZILLAOS_SETUP_NO_FRAMEBUFFER') >= 0:
        error = 'setup_framebuffer_unavailable'
    elif require_setup and position('ZILLAOS_SETUP_SKIPPED_NO_FRAMEBUFFER') >= 0:
        error = 'setup_skipped_no_framebuffer'
    return dict(success=error is None, classification=error or 'boot_evidence_valid',
                kernel_boot_seen=boot >= 0, setup_seen=setup >= 0,
                desktop_seen=desktop >= 0, fatal_marker=crash,
                event_count=len(events), note='Serial evidence only; not a full installed OS verification')


def analyse_file(path: Path, *, require_setup: bool = True) -> dict:
    if path.is_symlink():
        raise ValueError('symlink log path refused')
    info = path.stat()
    if not stat.S_ISREG(info.st_mode) or info.st_size > MAX_LOG:
        raise ValueError('COM1 log must be a regular file of at most 4 MiB')
    return analyse(path.read_text(encoding='utf-8', errors='replace'), require_setup=require_setup)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('log', type=Path, help='COM1 serial log from CURRENT VM boot')
    parser.add_argument('--kernel-only', action='store_true', help='only require kernel boot marker')
    args = parser.parse_args()
    try:
        result = analyse_file(args.log, require_setup=not args.kernel_only)
        print(json.dumps(result, indent=2))
        return 0 if result['success'] else 1
    except (ValueError, OSError) as exc:
        print(json.dumps({'success': False, 'classification': 'unreadable_log', 'error': str(exc)}, indent=2))
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
