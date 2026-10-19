#!/usr/bin/env bash
# Produce a pinned Limine UEFI ISO for automated test or interactive VM demo.
set -Eeuo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
ROOT="$PWD"

MODE="${1:-test}"
case "$MODE" in
    test|demo) ;;
    *) echo "Usage: bash scripts/build.sh [test|demo]" >&2; exit 2 ;;
esac
if (( $# > 1 )); then
    echo "Usage: bash scripts/build.sh [test|demo]" >&2
    exit 2
fi

LIMINE_VERSION="12.9.3"
LIMINE_SHA256="e02a473c6dc6df25159697f94930272397f21fd86e5d4eeee61bba72c95ec8d2"
LIMINE_URL="https://github.com/limine-bootloader/limine/releases/download/v${LIMINE_VERSION}/limine-binary.tar.xz"
DOWNLOAD_DIR="$ROOT/build/downloads"
UNPACK_DIR="$ROOT/build/limine-${LIMINE_VERSION}"
STAGING="$ROOT/build/iso-root"
ISO="$ROOT/dist/zillaos-0.0.1-${MODE}-uefi.iso"
ARCHIVE="$DOWNLOAD_DIR/limine-binary-${LIMINE_VERSION}.tar.xz"

for command in cargo curl sha256sum tar find xorriso; do
    if ! command -v "$command" >/dev/null 2>&1; then
        printf 'Missing required tool: %s\n' "$command" >&2
        exit 2
    fi
done

mkdir -p "$DOWNLOAD_DIR" "$ROOT/dist"

if [[ ! -f "$ARCHIVE" ]]; then
    printf 'Downloading pinned Limine %s\n' "$LIMINE_VERSION"
    curl --fail --location --retry 3 --connect-timeout 15 --output "$ARCHIVE.tmp" "$LIMINE_URL"
    mv "$ARCHIVE.tmp" "$ARCHIVE"
fi

if ! printf '%s  %s\n' "$LIMINE_SHA256" "$ARCHIVE" | sha256sum --check --status; then
    echo "ERROR: Limine archive SHA-256 mismatch" >&2
    exit 3
fi

if [[ ! -d "$UNPACK_DIR" ]]; then
    mkdir -p "$UNPACK_DIR"
    tar --extract --xz --file "$ARCHIVE" --directory "$UNPACK_DIR" --no-same-owner
fi

find_limine_file() {
    local name="$1"
    local located
    located="$(find "$UNPACK_DIR" -type f -name "$name" -print -quit)"
    if [[ -z "$located" ]]; then
        printf 'Limine binary not found: %s\n' "$name" >&2
        exit 4
    fi
    printf '%s' "$located"
}

echo "Building ZillaOS kernel (mode: $MODE)"
if [[ "$MODE" == "demo" ]]; then
    cargo build --release --target x86_64-unknown-none --offline --features vm-demo
else
    cargo build --release --target x86_64-unknown-none --offline --no-default-features
fi

KERNEL="$ROOT/target/x86_64-unknown-none/release/zillaos-kernel"
test -s "$KERNEL" || { echo "Missing kernel ELF" >&2; exit 5; }

rm -rf -- "$STAGING"
mkdir -p "$STAGING/EFI/BOOT" "$STAGING/boot"
cp "$(find_limine_file BOOTX64.EFI)" "$STAGING/EFI/BOOT/BOOTX64.EFI"
cp "$(find_limine_file limine-uefi-cd.bin)" "$STAGING/boot/limine-uefi-cd.bin"
cp "$ROOT/boot/limine.conf" "$STAGING/limine.conf"
cp "$KERNEL" "$STAGING/boot/zillaos-kernel"

xorriso -as mkisofs -R -r -J \
    --efi-boot boot/limine-uefi-cd.bin \
    -efi-boot-part --efi-boot-image \
    -o "$ISO" "$STAGING"

test -s "$ISO"
printf 'ZillaOS %s ISO: %s\n' "$MODE" "$ISO"
printf 'SHA256: '
sha256sum "$ISO"
