#!/usr/bin/env python3
"""Validate / optionally import a private BMP wallpaper for ZillaOS ISO.

Normal builds are unchanged when assets/wallpaper.bmp is absent. This script
never accesses physical guest disks. The optional import command uses Pillow;
the validate + stage commands use Python's standard library only.
"""
from __future__ import annotations

import argparse
import shutil
import struct
import sys
from pathlib import Path

MAX_WIDTH = 1024
MAX_HEIGHT = 768
MAX_BYTES = 4 * 1024 * 1024
MODULE_MARKER = "zillaos-wallpaper"
KERNEL_LINE = "    path: boot():/boot/zillaos-kernel"


def validate_bmp(path: Path) -> tuple[int, int, int]:
    size = path.stat().st_size
    if not 54 <= size <= MAX_BYTES:
        raise ValueError(f"BMP file must be 54..{MAX_BYTES} bytes (got {size})")
    raw = path.read_bytes()
    if raw[:2] != b"BM" or struct.unpack_from("<I", raw, 2)[0] != size:
        raise ValueError("Not a Windows BMP or declared file size does not match")
    offset, dib = struct.unpack_from("<II", raw, 10)
    if dib != 40 or offset < 54 or offset > size:
        raise ValueError("Only 40-byte Windows BITMAPINFOHEADER is supported")
    width, signed_height = struct.unpack_from("<ii", raw, 18)
    planes, bpp = struct.unpack_from("<HH", raw, 26)
    compression = struct.unpack_from("<I", raw, 30)[0]
    declared_image = struct.unpack_from("<I", raw, 34)[0]
    if not 1 <= width <= MAX_WIDTH or signed_height == 0 or abs(signed_height) > MAX_HEIGHT:
        raise ValueError(f"Image must be 1..{MAX_WIDTH} x 1..{MAX_HEIGHT}")
    if planes != 1 or bpp not in (24, 32) or compression != 0:
        raise ValueError("Only uncompressed 24- or 32-bpp BI_RGB BMP is supported")
    height = abs(signed_height)
    stride = (width * (bpp // 8) + 3) & ~3
    bitmap_size = stride * height
    if offset + bitmap_size > size or declared_image not in (0, bitmap_size):
        raise ValueError("BMP pixel rows or image size exceed the file")
    return width, height, bpp


def stage_asset(asset: Path, iso_root: Path, config: Path) -> tuple[int, int, int]:
    dims = validate_bmp(asset)
    original = config.read_text(encoding="utf-8")
    if "module_path:" in original or "module_string:" in original:
        raise ValueError("Unexpected modules in base config; refuse accidental duplicate injection")
    count = original.count(KERNEL_LINE)
    if count != 6:
        raise ValueError(f"Expected 6 ZillaOS boot entries; found {count}")
    injection = (
        KERNEL_LINE
        + "\n    module_path: boot():/boot/wallpaper.bmp"
        + "\n    module_string: " + MODULE_MARKER
    )
    patched = original.replace(KERNEL_LINE, injection)
    target = iso_root / "boot" / "wallpaper.bmp"
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(asset, target)
    (iso_root / "limine.conf").write_text(patched, encoding="utf-8")
    return dims


def import_image(image: Path, output: Path) -> tuple[int, int, int]:
    try:
        from PIL import Image, ImageOps  # type: ignore[import-not-found]
    except ImportError as exc:
        raise ValueError(
            "PNG/JPEG conversion requires Pillow: python3 -m pip install pillow; "
            "or save as 24-bit BMP in Paint/GIMP"
        ) from exc
    # An explicit pixel cap protects the converter from huge decompression.
    Image.MAX_IMAGE_PIXELS = 64 * 1024 * 1024
    with Image.open(image) as original:
        original.load()
        rgb = ImageOps.exif_transpose(original).convert("RGB")
        rgb.thumbnail((MAX_WIDTH, MAX_HEIGHT), Image.Resampling.LANCZOS)
        output.parent.mkdir(parents=True, exist_ok=True)
        candidate = output.with_name(output.name + ".tmp")
        try:
            rgb.save(candidate, format="BMP")
            dims = validate_bmp(candidate)
            candidate.replace(output)
        finally:
            candidate.unlink(missing_ok=True)
    return dims


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    check = sub.add_parser("validate")
    check.add_argument("asset", type=Path)
    stage = sub.add_parser("stage")
    stage.add_argument("asset", type=Path)
    stage.add_argument("--iso-root", type=Path, required=True)
    stage.add_argument("--config", type=Path, required=True)
    convert = sub.add_parser("import")
    convert.add_argument("image", type=Path)
    convert.add_argument("--output", type=Path, default=Path("assets/wallpaper.bmp"))
    args = parser.parse_args(argv)
    try:
        if args.command == "validate":
            dims = validate_bmp(args.asset)
        elif args.command == "stage":
            dims = stage_asset(args.asset, args.iso_root, args.config)
        else:
            dims = import_image(args.image, args.output)
    except (OSError, ValueError) as err:
        print(f"Wallpaper asset error: {err}", file=sys.stderr)
        return 2
    print(f"Wallpaper BMP ready: {dims[0]}x{dims[1]} {dims[2]} bpp")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
