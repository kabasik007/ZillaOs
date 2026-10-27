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

if __package__:
    from .smoke_test import OVMF_PAIRS, ROOT
else:
    from smoke_test import OVMF_PAIRS, ROOT

DEFAULT_ISO = ROOT / "dist" / "zillaos-0.0.2-dev-demo-uefi.iso"
REQUIRED = ("ZILLAOS_DESKTOP_READY", "ZILLAOS_DESKTOP_INPUT_POLL", "ZILLAOS_WINDOW_MANAGER_READY", "ZILLAOS_MOUSE_READY")
SETUP_READY = "ZILLAOS_SETUP_READY"
SETUP_COMPLETE = "ZILLAOS_SETUP_COMPLETE"
KEY_MARKER = "ZILLAOS_DESKTOP_STATE_CHANGED"
FAILURE = ("ZILLAOS_BOOT_FAIL", "ZILLAOS_PANIC", "ZILLAOS_EXCEPTION_FATAL")


def has_line(log: str, line: str) -> bool:
    return line in {value.strip() for value in log.splitlines()}


def has_window_state(log: str, visible: int, focus: str) -> bool:
    """A single contiguous serial event must contain both the focus and count."""
    lines = [line.strip() for line in log.splitlines()]
    expected = [
        KEY_MARKER,
        f"ZILLAOS_DESKTOP_VISIBLE=0x{visible:016x}",
        f"ZILLAOS_DESKTOP_FOCUS={focus}",
    ]
    return any(lines[i:i + 3] == expected for i in range(len(lines) - 2))


def setup_passed(log: str, profile: str = "default") -> bool:
    """Require ordered setup transitions and explicit RAM-only preferences."""
    profiles = {
        "default": ("uk", "Europe/Kyiv", "zilla-pc", "UA", "DARK"),
        "custom": ("en", "Europe/Warsaw", "lab01", "PL", "LIGHT"),
    }
    if profile not in profiles:
        return False
    language, timezone, hostname, region, theme = profiles[profile]
    lines = [line.strip() for line in log.splitlines()]
    sequence = [
        SETUP_READY,
        *[f"ZILLAOS_SETUP_STEP=0x{step:016x}" for step in range(1, 8)],
        SETUP_COMPLETE,
        f"ZILLAOS_SETUP_LANGUAGE={language}",
        f"ZILLAOS_SETUP_TIMEZONE={timezone}",
        f"ZILLAOS_SETUP_HOSTNAME={hostname}",
        f"ZILLAOS_SETUP_REGION={region}",
        f"ZILLAOS_SETUP_THEME={theme}",
        "ZILLAOS_SETUP_VOLATILE_ONLY",
    ]
    cursor = 0
    for expected in sequence:
        try:
            cursor = lines.index(expected, cursor) + 1
        except ValueError:
            return False
    return True


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


def ppm_payload(path: Path) -> bytes | None:
    """Return P6 RGB data; header is allowed to vary in textual spacing."""
    try:
        with path.open("rb") as f:
            if f.readline().strip() != b"P6":
                return None
            dimensions = f.readline().strip()
            while dimensions.startswith(b"#"):
                dimensions = f.readline().strip()
            width, height = (int(v) for v in dimensions.split())
            if int(f.readline().strip()) != 255:
                return None
            if width <= 0 or height <= 0 or width > 8192 or height > 8192:
                return None
            data = f.read()
            return data if len(data) == width * height * 3 else None
    except (OSError, ValueError):
        return None


def changed_rgb_bytes(before: Path, after: Path) -> int | None:
    """Compare screenshot *pixels* after validating PPM dimensions."""
    dimensions = screenshot_dimensions(before)
    if dimensions is None or dimensions != screenshot_dimensions(after):
        return None
    left = ppm_payload(before)
    right = ppm_payload(after)
    if left is None or right is None or len(left) != len(right):
        return None
    return sum(a != b for a, b in zip(left, right))


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
    p.add_argument("--setup-profile", choices=("default", "custom"), default="default",
                   help="Keyboard-driven OOBE settings for the QEMU test")
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
                if not wait_until(lambda: has_line(read_log(log), SETUP_READY),
                                  guest, deadline):
                    print("FAIL: first-run Setup screen did not start", file=sys.stderr)
                    return 1
                if not wait_until(monitor.exists, guest, deadline):
                    print("FAIL: QEMU monitor unavailable", file=sys.stderr)
                    return 1

                # Capture actual first-boot setup BEFORE entering desktop.
                setup_capture = workdir / "setup.ppm"
                monitor_command(monitor, f"screendump {setup_capture}")
                if not wait_until(setup_capture.exists, guest, deadline):
                    print("FAIL: setup screen cannot be captured", file=sys.stderr)
                    return 1
                if screenshot_dimensions(setup_capture) is None:
                    print("FAIL: first-run setup screen appears blank", file=sys.stderr)
                    return 1
                setup_output = args.output.with_name(args.output.stem + "-setup.ppm")
                shutil.copyfile(setup_capture, setup_output)

                # Drive Welcome -> Language -> Region -> TimeZone -> Device
                # -> Appearance -> Review -> Finished. Settings remain RAM-only.
                for step in range(1, 8):
                    if args.setup_profile == "custom":
                        # Once a page opens, adjust the value BEFORE Enter.
                        if step in (2, 3, 4, 6):
                            monitor_command(monitor, "sendkey right")
                        elif step == 5:
                            # Replaces the selected `zilla-pc` suggestion.
                            for letter in "lab01":
                                monitor_command(monitor, f"sendkey {letter}")
                    monitor_command(monitor, "sendkey ret")
                    marker = f"ZILLAOS_SETUP_STEP=0x{step:016x}"
                    if not wait_until(lambda: has_line(read_log(log), marker), guest, deadline):
                        print(f"FAIL: setup did not advance to step {step}", file=sys.stderr)
                        return 1
                if not wait_until(
                    lambda: setup_passed(read_log(log), args.setup_profile)
                    and all(has_line(read_log(log), m) for m in REQUIRED),
                    guest, deadline
                ):
                    print("FAIL: setup sequence/defaults or desktop readiness incomplete",
                          file=sys.stderr)
                    return 1
                if any(has_line(read_log(log), m) for m in FAILURE):
                    print("FAIL: kernel failure marker", file=sys.stderr)
                    return 1
                if not wait_until(monitor.exists, guest, deadline):
                    print("FAIL: QEMU HMP monitor socket missing", file=sys.stderr)
                    return 1
                for key, count, focus in [
                    ("f3", 2, "TERMINAL"),
                    ("f4", 3, "SETTINGS"),
                    ("tab", 3, "FILES"),
                ]:
                    monitor_command(monitor, f"sendkey {key}")
                    if not wait_until(
                        lambda: has_window_state(read_log(log), count, focus),
                        guest,
                        deadline,
                    ):
                        print(f"FAIL: {key} did not produce {count} windows / {focus} focus",
                              file=sys.stderr)
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

                # Minimize and restore must not destroy the other windows.
                monitor_command(monitor, "sendkey f6")
                if not wait_until(
                    lambda: has_window_state(read_log(log), 2, "SETTINGS"),
                    guest,
                    deadline,
                ):
                    print("FAIL: minimizing focused Files did not reveal Settings",
                          file=sys.stderr)
                    return 1

                monitor_command(monitor, "sendkey f2")
                if not wait_until(
                    lambda: read_log(log).count("ZILLAOS_DESKTOP_FOCUS=FILES") >= 2
                    and has_window_state(read_log(log), 3, "FILES"),
                    guest,
                    deadline,
                ):
                    print("FAIL: restoring Files through F2 did not focus a third window",
                          file=sys.stderr)
                    return 1

                # HMP mouse_move and mouse_button are supported by QEMU's
                # monitor; expected serial markers come from actual PS/2
                # packets, not merely an injected command succeeding.
                monitor_command(monitor, "mouse_move 20 10")
                if not wait_until(
                    lambda: has_line(read_log(log), "ZILLAOS_MOUSE_MOTION_DETECTED"),
                    guest, deadline,
                ):
                    print("FAIL: no PS/2 mouse movement reached desktop", file=sys.stderr)
                    return 1
                if has_line(read_log(log), "ZILLAOS_GRAPHICS_BUFFERED_READY"):
                    if not wait_until(
                        lambda: has_line(read_log(log), "ZILLAOS_GRAPHICS_CURSOR_DAMAGE_OK"),
                        guest, deadline,
                    ):
                        print("FAIL: mouse motion did not use buffered cursor damage path",
                              file=sys.stderr)
                        return 1

                moved_screen = workdir / "frame-cursor.ppm"
                monitor_command(monitor, f"screendump {moved_screen}")
                if not wait_until(moved_screen.exists, guest, deadline):
                    print("FAIL: no pointer screenshot", file=sys.stderr)
                    return 1
                changes = changed_rgb_bytes(screenshot_path, moved_screen)
                if changes is None or not (1 <= changes <= 4000):
                    print(f"FAIL: mouse cursor redraw not isolated (changed RGB bytes={changes})",
                          file=sys.stderr)
                    return 1
                cursor_output = args.output.with_name(args.output.stem + "-cursor.ppm")
                shutil.copyfile(moved_screen, cursor_output)

                monitor_command(monitor, "mouse_button 1")
                if not wait_until(
                    lambda: has_line(read_log(log), "ZILLAOS_MOUSE_LEFT_DOWN"),
                    guest, deadline,
                ):
                    print("FAIL: left mouse press was not decoded", file=sys.stderr)
                    return 1
                monitor_command(monitor, "mouse_button 0")
                if not wait_until(
                    lambda: has_line(read_log(log), "ZILLAOS_MOUSE_LEFT_UP"),
                    guest, deadline,
                ):
                    print("FAIL: left mouse release was not decoded", file=sys.stderr)
                    return 1

                print(f"PASS: ZillaOS GUI 3 windows, focus, minimize/restore, PS/2 mouse motion+click; screenshot {dimensions[0]}x{dimensions[1]}")
                print(f"Screens: {setup_output}, {args.output}, {cursor_output}; serial: {log}")
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
