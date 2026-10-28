#!/usr/bin/env bash
# Build a VMware Workstation-compatible ZillaOS UEFI desktop ISO.
# Run from Ubuntu/WSL2; the resulting ISO is opened by Windows VMware.
set -Eeuo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
ROOT="$PWD"

for tool in cargo rustc curl sha256sum tar find xorriso python3; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        printf 'MISSING: %s\n' "$tool" >&2
        if [[ "$tool" == "cargo" || "$tool" == "rustc" ]]; then
            echo 'Install Rust via https://rustup.rs/ and run: source "$HOME/.cargo/env"' >&2
        else
            echo 'On Ubuntu: sudo apt update && sudo apt install -y curl ca-certificates xorriso python3' >&2
        fi
        exit 2
    fi
done

if ! rustup target list --installed 2>/dev/null | grep -qx 'x86_64-unknown-none'; then
    echo 'Missing bare-metal Rust target. Run: rustup target add x86_64-unknown-none --toolchain 1.90.0' >&2
    exit 2
fi

echo '[1/3] Host Python tests'
python3 -m unittest discover -s tests -v

echo '[2/3] Rust library tests'
cargo test --lib --target x86_64-unknown-linux-gnu --offline

echo '[3/3] Building UEFI demo image (Limine)'
bash scripts/build.sh demo

echo '[3b/3] Checking x86-64 EFI loader, ELF kernel and Limine configuration'
python3 scripts/check_boot_assets.py

ISO="$ROOT/dist/zillaos-0.0.2-dev-demo-uefi.iso"
if [[ ! -s "$ISO" ]]; then
    echo "Missing expected ISO: $ISO" >&2
    exit 5
fi

echo 'Checking ISO9660/UEFI El Torito metadata:'
xorriso -indev "$ISO" -report_el_torito plain 2>&1
printf '\nVMware-ready ISO: %s\n' "$ISO"
if command -v wslpath >/dev/null 2>&1; then
    printf 'Windows ISO path: '
    wslpath -w "$ISO"
fi
sha256sum "$ISO"
