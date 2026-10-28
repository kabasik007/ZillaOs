#!/usr/bin/env bash
# ZillaOS installer acceptance gate. Only disposable new RAW images.
# --preflight (default): compile + validate boot assets; NO persistent lab image.
# Unit tests may create temporary disposable image files and clean them up.
# --create/--boot: requires an exact second argument as explicit consent.
set -Eeuo pipefail
cd "$(dirname "$0")/.."
MODE="${1:---preflight}"
CONSENT="${2:-}"
if (( $# > 2 )); then
    echo "Usage: bash scripts/installer_acceptance.sh [--preflight|--create|--boot] [CREATE-NEW-VM-IMAGE]" >&2
    exit 2
fi
case "$MODE" in
    --preflight)
        if [[ -n "$CONSENT" ]]; then
            echo "Unexpected confirmation for preflight" >&2
            exit 2
        fi
        ;;
    --create|--boot)
        if [[ "$CONSENT" != "CREATE-NEW-VM-IMAGE" ]]; then
            echo "Explicit confirmation required: CREATE-NEW-VM-IMAGE" >&2
            exit 2
        fi
        ;;
    *)
        echo "Unknown mode. Use --preflight, --create or --boot" >&2
        exit 2
        ;;
esac

mkdir -p build/logs
echo "[1/5] Rust + Python tests, ISO build, EFI/ELF/Limine validation"
# build_vmware.sh already includes full Python/Rust host tests and the ISO build.
# Pipefail preserves its exit code; a failing build never advances to disk creation.
bash scripts/build_vmware.sh 2>&1 | tee build/logs/installer-acceptance-build.log
python3 scripts/check_boot_assets.py > build/logs/installer-acceptance-boot-assets.json
if [[ "$MODE" == "--preflight" ]]; then
    echo "PASS: source/ISO preflight finished. No persistent Installer Lab disk created."
    exit 0
fi

echo "[2/5] Creating one NEW sparse GPT+FAT32+Limine RAW laboratory image"
# Each accepted run gets a distinct random basename. O_EXCL in the builder
# independently guarantees that an existing image cannot be overwritten.
TOKEN="$(python3 -c 'import secrets; print(secrets.token_hex(6))')"
IMAGE_NAME="zillaos-accept-$TOKEN.img"
python3 scripts/installer_lab.py \
    --name "$IMAGE_NAME" --size-gib 12 --stage-uefi \
    --create --confirm CREATE-NEW-VM-IMAGE \
    | tee build/logs/installer-acceptance-stage.json

echo "[3/5] Verifying GPT and FAT32 using independent Python reader"
IMAGE="$HOME/.local/share/zillaos/installer-lab/$IMAGE_NAME"
python3 scripts/installer_fat32_verify.py "$IMAGE" --check-boot \
    | tee build/logs/installer-acceptance-esp.json

echo "[4/5] Reading FAT32 with independent mtools"
bash scripts/verify_esp_mtools.sh "$IMAGE_NAME" \
    | tee build/logs/installer-acceptance-mtools.log
if [[ "$MODE" == "--create" ]]; then
    echo "PASS: test image created and verified, not booted."
    echo "Disposable RAW: $IMAGE"
    exit 0
fi

echo "[5/5] No-ISO UEFI QEMU boot: ONLY read-only RAW, no network or host disks"
RUN_LOG="build/logs/installer-acceptance-qemu.log"
set +e
timeout 100s bash scripts/test_uefi_disk_qemu.sh "$IMAGE_NAME" --headless \
    > "$RUN_LOG" 2>&1
RC=$?
set -e
if (( RC != 0 && RC != 124 )); then
    cat "$RUN_LOG"
    echo "FAIL: QEMU exited with code $RC" >&2
    exit 1
fi
cat "$RUN_LOG"
SERIAL="$(sed -n 's/^Serial log: //p' "$RUN_LOG" | tail -n 1)"
# Only analyze a regular COM1 file under our known build/logs directory.
case "$SERIAL" in
    "$PWD"/build/logs/installer-uefi-disk-serial*.log) ;;
    *) echo "FAIL: no trusted COM1 log path from current QEMU run" >&2; exit 1 ;;
esac
if [[ ! -f "$SERIAL" || -L "$SERIAL" ]]; then
    echo "FAIL: no COM1 log from this boot" >&2
    exit 1
fi
python3 scripts/boot_evidence.py "$SERIAL" \
    | tee build/logs/installer-acceptance-boot-evidence.json
echo "PASS: QEMU UEFI boot evidence recorded for $IMAGE_NAME"
echo "Disposable RAW: $IMAGE"
echo "Serial log: $SERIAL"
