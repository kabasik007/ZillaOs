#!/usr/bin/env bash
# Independent FAT32 EFI read-back with standard mtools, never a disk mount.
# Only reads a named Installer Lab RAW file created in WSL HOME.
set -Eeuo pipefail
cd "$(dirname "$0")/.."
NAME="${1:-zillaos-uefi.img}"
if (( $# > 1 )) || [[ ! "$NAME" =~ ^zillaos-[a-z0-9][a-z0-9-]*\.img$ ]]; then
    echo "Expected exactly one safe Installer Lab image name" >&2
    exit 2
fi
IMAGE="$HOME/.local/share/zillaos/installer-lab/$NAME"
if [[ ! -f "$IMAGE" || -L "$IMAGE" ]]; then
    echo "No safe regular Installer Lab RAW image found: $IMAGE" >&2
    exit 2
fi
for binary in python3 mdir mcopy cmp mktemp; do
    command -v "$binary" >/dev/null 2>&1 || {
        echo "Missing $binary (on Ubuntu: sudo apt install mtools)" >&2
        exit 2
    }
done

# The custom GPT/FAT32 verifier must pass before any third-party parser
# reads the image. Both tools open the raw image read-only.
python3 scripts/installer_fat32_verify.py "$IMAGE" --check-boot
ESP_BYTE_OFFSET=1048576
FAT_IMAGE="${IMAGE}@@${ESP_BYTE_OFFSET}"
TMP="$(mktemp -d)"
trap 'rm -rf -- "$TMP"' EXIT

echo "[mtools] EFI System Partition root directory"
mdir -i "$FAT_IMAGE" ::/
echo "[mtools] EFI/BOOT directory"
mdir -i "$FAT_IMAGE" ::/EFI/BOOT
echo "[mtools] boot directory"
mdir -i "$FAT_IMAGE" ::/BOOT

# mcopy source is FAT32 and destination is a newly created temporary
# host path; it NEVER writes to the raw image or an attached disk.
mcopy -i "$FAT_IMAGE" ::/EFI/BOOT/BOOTX64.EFI "$TMP/BOOTX64.EFI"
mcopy -i "$FAT_IMAGE" ::/BOOT/KERNEL.ELF "$TMP/KERNEL.ELF"
mcopy -i "$FAT_IMAGE" ::/LIMINE.CONF "$TMP/LIMINE.CONF"

python3 - "$TMP" <<'PY'
import pathlib
import sys
sys.path.insert(0, 'scripts')
from installer_lab import load_uefi_assets
expected = load_uefi_assets()
folder = pathlib.Path(sys.argv[1])
mapping = {
    'EFI/BOOT/BOOTX64.EFI': 'BOOTX64.EFI',
    'BOOT/KERNEL.ELF': 'KERNEL.ELF',
    'LIMINE.CONF': 'LIMINE.CONF',
}
for guest, extracted in mapping.items():
    on_disk = (folder / extracted).read_bytes()
    if on_disk != expected[guest]:
        raise SystemExit('FAIL: mtools read-back differs from build: ' + guest)
    print('PASS: FAT32 mtools read-back matches', guest, 'bytes=', len(on_disk))
print('ZILLAOS_MTOOLS_ESP_VERIFIED')
PY
