#!/usr/bin/env bash
# Export a verified Installer Lab RAW disk as a NEW disposable VMware VMDK.
# Linux/WSL only. Does not modify RAW, existing VMDKs or physical devices.
set -Eeuo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"
NAME="zillaos-uefi.img"
if (( $# > 0 )); then NAME="$1"; fi
if [[ ! "$NAME" =~ ^zillaos-[a-z0-9][a-z0-9-]*\.img$ ]]; then
    echo "Invalid lab image name (no directories accepted)" >&2
    exit 2
fi
SOURCE="$HOME/.local/share/zillaos/installer-lab/$NAME"
DEST_DIR="$ROOT/build/vmware-installed/ZillaOS-Installed-Test"
DEST="$DEST_DIR/zillaos-installed.vmdk"
for bin in python3 qemu-img; do
    command -v "$bin" >/dev/null || {
        echo "Missing required $bin. Install qemu-utils in Ubuntu." >&2
        exit 2
    }
done
if [[ ! -f "$SOURCE" || -L "$SOURCE" ]]; then
    echo "Only an existing non-symlinked Installer Lab RAW image is allowed" >&2
    exit 2
fi

# This proves the two GPT copies, FAT32 structure and byte-exact boot
# files. It does NOT prove that the kernel will boot in VMware.
python3 scripts/installer_fat32_verify.py "$SOURCE" --check-boot

# Reserving a fresh directory (not using mkdir -p) avoids overwrite races.
if [[ -e "$DEST_DIR" || -L "$DEST_DIR" ]]; then
    echo "VMware test directory already exists; refusing overwrite: $DEST_DIR" >&2
    exit 2
fi
# Only the safe parent is created recursively. The VM's own directory is
# reserved exclusively and is never reused or overwritten.
if [[ -L "$ROOT/build/vmware-installed" ]]; then
    echo "Refusing symlinked VMware output directory" >&2
    exit 2
fi
mkdir -p -- "$ROOT/build/vmware-installed"
mkdir -m 700 -- "$DEST_DIR"
trap 'echo "VMware export incomplete; inspect test directory before retrying." >&2' ERR

qemu-img convert -p -f raw -O vmdk -o subformat=monolithicSparse -S 4k \
    "$SOURCE" "$DEST"
test -s "$DEST"
qemu-img info --output=json -f vmdk "$DEST" > "$DEST_DIR/disk-info.json"
python3 - "$DEST_DIR/disk-info.json" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as f:
    data = json.load(f)
assert data["format"] == "vmdk", data
assert data["virtual-size"] >= 12 * 1024 ** 3, data
print("VMDK validated:", data["format"], data["virtual-size"], "bytes virtual")
PY
echo "VMDK_CREATED=$DEST"
echo "Original RAW preserved: $SOURCE"
echo "No VMware VM was powered on by this script."
