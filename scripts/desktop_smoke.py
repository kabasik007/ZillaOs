#!/usr/bin/env python3
"""Headless QEMU GUI smoke: prove framebuffer boot, keyboard event, screenshot.

This does NOT assert that Files or Terminal are functional applications.
It only validates the kernel-owned early RGB32 desktop demo in QEMU.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import time

from smoke_test import OVMF_PAIRS, ROOT

DEFAULT_ISO = ROOT / "dist" / "zillaos-0.0.2-dev-demo-uefi.iso"
REQUIRED = ("ZILLAOS_DESKTOP_READY", "ZILLAOS_DESKTOP_INPUT_POLL")
KEY_MARKER = "ZILLAOS_DESKTOP_STATE_CHANGED"
FAILURE = ("ZILLAOS_BOOT_FAIL", "ZILLAOS_PANIC", "ZILLAOS_EXCEPTION_FATAL")


def has_line(log: str, line: str) -> bool:
    return line in {value.strip() for value in log.splitlines()}


def screenshot_dimensions(path: Path) -> tuple[int, int] | None:
    """Validate complete non-flat binary PPM (P6) from QEMU screendump."""
    try:
        with path.open("rb") as f:
            if f.readline().strip() != b"P6":
                return None
            header = f.readline().strip()
            while header.startswith(b"#"):
                header = f.readline().strip()
            w, h = (int(n) for n in header.split())
            depth = int(f.readline())
            if not (320 <= w <= 8192 and 200 <= h <= 8192 and depth == 255):
                return None
            pixels = f.read()
            if len(pixels) != w * h * 3:
                return None
            # Inspect a spread of pixels. A black firmware screen or empty
            # headless device must not count as a rendered desktop.
            sample = pixels[::max(3, len(pixels) // 1800)]
            if len(set(sample)) < 4:
                return None
            return (w, h)
    except (OSError, ValueError, OverflowError):
        return None


def read_log(path: Path) -> str:
    try:
        return path.read_text(errors="replace")
    except OSError:
        return ""


def wait_until(condition, process: subprocess.Popen, limit: float) -> bool:
    while time.monotonic() < limit:
        if condition():
            return True
        if process.poll() is not None:
            return False
        time.sleep(0.1)
    return False


def monitor_command(path: Path, command: str) -> str:
    """Send one QEMU HMP command over a private temporary UNIX socket."""
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
        sock.settimeout(5)
        sock.connect(str(path))
        sock.sendall((command + "\n").encode("ascii"))
        time.sleep(0.5)
        try:
            return sock.recv(8192).decode("utf-8", errors="replace")
        except socket.timeout:
            return ""


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--iso", type=Path, default=DEFAULT_ISO)
    p.add_argument("--timeout", type=int, default=90)
    p.add_argument("--output", type=Path, default=ROOT / "build/logs/desktop-qemu.ppm")
    args = p.parse_args()

    iso = args.iso.resolve()
    if args.timeout < 10:
        p.error("--timeout must be >= 10 seconds")
    if not iso.is_file() or not iso.stat().st_size:
        print(f"FAIL: missing demo ISO {iso}", file=sys.stderr)
        return 2
    if shutil.which("qemu-system-x86_64") is None:
        print("FAIL: QEMU is unavailable", file=sys.stderr)
        return 2

    pair = next(((Path(a), Path(b)) for a, b in OVMF_PAIRS
                 if Path(a).is_file() and Path(b).is_file()), None)
    if pair is None:
        print("FAIL: OVMF CODE/VARS pair unavailable", file=sys.stderr)
        return 2

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.unlink(missing_ok=True)
    with tempfile.TemporaryDirectory(prefix="zdesk-") as work:
        workdir = Path(work)
        firmware = workdir / "OVMF_VARS.fd"
        shutil.copyfile(pair[1], firmware)
        monitor = workdir / "monitor.sock"
        log = args.output.parent / "desktop-qemu.log"
        cmd = [
            "qemu-system-x86_64",
            "-machine", "q35,accel=tcg",
            "-cpu", "qemu64",
            "-m", "512M",
            "-smp", "1",
            "-vga", "std",
            "-display", "none",
            "-net", "none",
            "-serial", "stdio",
            "-monitor", f"unix:{monitor},server,nowait",
            "-no-reboot",
            "-no-shutdown",
            "-drive", f"if=pflash,format=raw,unit=0,readonly=on,file={pair[0]}",
            "-drive", f"if=pflash,format=raw,unit=1,file={firmware}",
            "-cdrom", str(iso),
            "-boot", "order=d",
        ]
        deadline = time.monotonic() + args.timeout
        with log.open("w") as output:
            guest = subprocess.Popen(cmd, stdout=output, stderr=subprocess.STDOUT, text=True)
            try:
                if not wait_until(
                    lambda: all(has_line(read_log(log), m) for m in REQUIRED),
                    guest, deadline
                ):
                    print("FAIL: desktop ready/input markers not observed", file=sys.stderr)
                    return 1
                if any(has_line(read_log(log), m) for m in FAILURE):
                    print("FAIL: kernel failure marker", file=sys.stderr)
                    return 1
                if not wait_until(monitor.exists, guest, deadline):
                    print("FAIL: QEMU HMP monitor socket missing", file=sys.stderr)
                    return 1
                monitor_command(monitor, "sendkey f3")
                if not wait_until(lambda: has_line(read_log(log), KEY_MARKER), guest, deadline):
                    print("FAIL: injected F3 produced no desktop state change", file=sys.stderr)
                    return 1

                screenshot_path = workdir / "frame.ppm"
                reply = monitor_command(monitor, f"screendump {screenshot_path}")
                if "error" in reply.lower():
                    print(f"FAIL: screenshot: {reply}", file=sys.stderr)
                    return 1
                if not wait_until(screenshot_path.exists, guest, deadline):
                    print("FAIL: no screenshot written by QEMU", file=sys.stderr)
                    return 1
                dimensions = screenshot_dimensions(screenshot_path)
                if dimensions is None:
                    print("FAIL: empty, flat or malformed screenshot", file=sys.stderr)
                    return 1
                shutil.copyfile(screenshot_path, args.output)
                print(f"PASS: ZillaOS GUI shown, F3 event received, screenshot {dimensions[0]}x{dimensions[1]}")
                print(f"Screen: {args.output}; serial: {log}")
                return 0
            finally:
                if guest.poll() is None:
                    guest.terminate()
                    try:
                        guest.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        guest.kill()
                        guest.wait(timeout=5)


if __name__ == "__main__":
    sys.exit(main())
