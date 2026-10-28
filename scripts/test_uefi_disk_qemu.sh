#!/usr/bin/env bash
# Boot an existing ZillaOS Installer Lab UEFI RAW disk, READ-ONLY, in QEMU.
# Does not attach host disks, never writes RAW or firmware templates.
set -Eeuo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"
NAME="zillaos-uefi.img"
if (( $# > 0 )); then NAME="$1"; fi
if [[ ! "$NAME" =~ ^zillaos-[a-z0-9][a-z0-9-]*\.img$ ]]; then
    echo "Use only a ZillaOS lab image name such as zillaos-uefi.img" >&2
    exit 2
fi
DISPLAY_ARGS=(-display gtk)
if (( $# > 1 )); then
    if [[ "$2" == "--headless" ]]; then
        DISPLAY_ARGS=(-display none -vga std)
    else
        echo "Only --headless accepted as second argument" >&2
        exit 2
    fi
fi
IMAGE="$HOME/.local/share/zillaos/installer-lab/$NAME"
if [[ ! -f "$IMAGE" || -L "$IMAGE" ]]; then
    echo "Missing or unsafe lab RAW image: $IMAGE" >&2
    exit 2
fi
for bin in qemu-system-x86_64 python3; do
    command -v "$bin" >/dev/null || { echo "Missing $bin" >&2; exit 2; }
done
python3 scripts/installer_fat32_verify.py "$IMAGE" --check-boot

CODE=""
VARS=""
for base in /usr/share/OVMF /usr/share/ovmf; do
    if [[ -f "$base/OVMF_CODE_4M.fd" && -f "$base/OVMF_VARS_4M.fd" ]]; then
        CODE="$base/OVMF_CODE_4M.fd"
        VARS="$base/OVMF_VARS_4M.fd"
        break
    fi
    if [[ -f "$base/OVMF_CODE.fd" && -f "$base/OVMF_VARS.fd" ]]; then
        CODE="$base/OVMF_CODE.fd"
        VARS="$base/OVMF_VARS.fd"
        break
    fi
done
if [[ -z "$CODE" ]]; then
    echo "No matching OVMF_CODE / OVMF_VARS pair. sudo apt install ovmf" >&2
    exit 2
fi

mkdir -p build/logs
TMP="$(mktemp -d)"
trap 'rm -rf -- "$TMP"' EXIT
cp -- "$VARS" "$TMP/OVMF_VARS.fd"
SERIAL="$ROOT/build/logs/installer-uefi-disk-serial.log"
echo "Launching READ-ONLY UEFI disk: $IMAGE"
echo "Serial log: $SERIAL"
echo "No ISO. No host disk passthrough. OVMF variable changes are temporary."
qemu-system-x86_64 \
    -machine q35,accel=tcg -cpu qemu64 -smp 1 -m 1024 \
    -drive if=pflash,format=raw,unit=0,readonly=on,file="$CODE" \
    -drive if=pflash,format=raw,unit=1,file="$TMP/OVMF_VARS.fd" \
    -drive if=none,format=raw,readonly=on,id=zilladisk,file="$IMAGE" \
    -device ich9-ahci,id=zillasata \
    -device ide-hd,bus=zillasata.0,drive=zilladisk,bootindex=1 \
    -nic none -serial "file:$SERIAL" "${DISPLAY_ARGS[@]}"
