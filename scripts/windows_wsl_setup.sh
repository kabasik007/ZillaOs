#!/usr/bin/env bash
# ZillaOS Windows/WSL bootstrap. Never touches physical disks or VM images.
# --check: report dependencies without installing anything.
# --install: explicit apt installation inside Ubuntu.
# --install-rust: additionally install official rustup if absent, then pinned toolchain.
set -Eeuo pipefail
cd "$(dirname "$0")/.."

MODE="${1:---check}"
if (( $# > 0 )); then shift; fi
INSTALL_RUST=0
WITH_QEMU=0
case "$MODE" in --check|--install) ;; *)
    echo "Usage: bash scripts/windows_wsl_setup.sh [--check|--install] [--install-rust] [--with-qemu]" >&2
    exit 2 ;;
esac
for option in "$@"; do
    case "$option" in
        --install-rust) INSTALL_RUST=1 ;;
        --with-qemu) WITH_QEMU=1 ;;
        *) echo "Unknown option: $option" >&2; exit 2 ;;
    esac
done
if [[ "$MODE" == "--check" && "$INSTALL_RUST" -eq 1 ]]; then
    echo "--install-rust requires explicit --install mode" >&2
    exit 2
fi
if [[ ! -r /etc/os-release ]]; then
    echo "Run from Ubuntu under WSL2 (missing /etc/os-release)" >&2
    exit 2
fi
# Never assume we're on Ubuntu based solely on the Windows distribution name.
. /etc/os-release
if [[ "${ID:-}" != "ubuntu" && "${ID_LIKE:-}" != *ubuntu* ]]; then
    echo "This bootstrap was designed for Ubuntu; detected: ${ID:-unknown}" >&2
    exit 2
fi

if [[ "$MODE" == "--install" ]]; then
    if ! command -v apt-get >/dev/null 2>&1; then
        echo "Ubuntu apt-get missing" >&2
        exit 2
    fi
    # Rust host tests are native x86_64 Linux executables. They require the
    # system C toolchain and the LLD linker, even though the OS ELF itself
    # targets x86_64-unknown-none.
    packages=(ca-certificates curl tar xz-utils git python3 xorriso build-essential lld)
    if [[ "$WITH_QEMU" -eq 1 ]]; then
        packages+=(qemu-system-x86 qemu-system-gui ovmf mtools qemu-utils)
    fi
    needed=()
    for package in "${packages[@]}"; do
        if ! dpkg-query -W -f='${Status}' "$package" 2>/dev/null |
            grep -qx 'install ok installed'; then
            needed+=("$package")
        fi
    done
    if (( ${#needed[@]} > 0 )); then
        printf '[ZillaOS] Installing missing Ubuntu packages: %s\n' "${needed[*]}"
        echo '[ZillaOS] sudo may ask for the Ubuntu account password.'
        sudo apt-get update
        sudo apt-get install -y --no-install-recommends "${needed[@]}"
    else
        echo '[ZillaOS] All Ubuntu packages are already installed; skipping apt.'
    fi
fi

# rustup's normal Ubuntu installation places cargo into ~/.cargo/bin,
# which bash -lc launched from PowerShell might not otherwise inherit.
if [[ -f "$HOME/.cargo/env" ]]; then
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
fi
if [[ "$MODE" == "--install" && "$INSTALL_RUST" -eq 1 ]]; then
    if ! command -v rustup >/dev/null 2>&1; then
        echo "[ZillaOS] Installing rustup from the official HTTPS installer by explicit request."
        temp="$(mktemp -d)"
        trap 'rm -rf -- "$temp"' EXIT
        curl --fail --location --proto '=https' --tlsv1.2 \
            https://sh.rustup.rs --output "$temp/rustup-init.sh"
        sh "$temp/rustup-init.sh" -y --profile minimal --no-modify-path
        # shellcheck disable=SC1091
        source "$HOME/.cargo/env"
    fi
    echo "[ZillaOS] Installing repo-pinned Rust 1.90.0 and bare-metal target"
    rustup toolchain install 1.90.0 --profile minimal \
        --component rustfmt --component clippy \
        --target x86_64-unknown-none
fi

echo "[ZillaOS] Dependency status:"
missing=0
for binary in bash python3 curl sha256sum tar find xorriso cargo rustc rustup cc ld.lld; do
    if command -v "$binary" >/dev/null 2>&1; then
        printf "  OK      %s\n" "$binary"
    else
        printf "  MISSING %s\n" "$binary"
        missing=1
    fi
done
# The host Rust test binary links through cc with -fuse-ld=lld on the
# pinned Linux toolchain. Check actual linkability, not just command names.
if command -v cc >/dev/null 2>&1 && command -v ld.lld >/dev/null 2>&1; then
    probe_dir="$(mktemp -d)"
    if printf 'int main(void){return 0;}\n' |
        cc -x c - -fuse-ld=lld -o "$probe_dir/zillaos-link-probe" 2>"$probe_dir/linker-stderr"; then
        echo "  OK      native host link via cc -fuse-ld=lld"
    else
        echo "  FAILED  native host link via cc -fuse-ld=lld"
        cat "$probe_dir/linker-stderr" >&2
        cc_path="$(command -v cc)"
        echo "  DIAG    cc resolves to: $cc_path" >&2
        ls -l -- "$cc_path" >&2 || true
        resolved_cc="$(readlink -f -- "$cc_path" 2>/dev/null || true)"
        if [[ -n "$resolved_cc" && "$resolved_cc" != "$cc_path" ]]; then
            echo "  DIAG    cc symlink target: $resolved_cc" >&2
            ls -l -- "$resolved_cc" >&2 || true
        fi
        echo "  FIX     Run -Action Setup (Ubuntu: build-essential + lld)" >&2
        echo "  NOTE    If Permission denied persists after Setup, inspect WSL file permissions and mount options." >&2
        missing=1
    fi
    rm -rf -- "$probe_dir"
fi
if command -v rustup >/dev/null 2>&1; then
    if rustup toolchain list | grep -q '^1\.90\.0-'; then
        echo "  OK      Rust toolchain 1.90.0"
    else
        echo "  MISSING Rust toolchain 1.90.0 (run Setup with -InstallRust)"
        missing=1
    fi
    if rustup target list --installed --toolchain 1.90.0 |
       grep -qx 'x86_64-unknown-none'; then
        echo "  OK      x86_64-unknown-none target"
    else
        echo "  MISSING x86_64-unknown-none target"
        missing=1
    fi
fi
if [[ "$WITH_QEMU" -eq 1 ]]; then
    for binary in qemu-system-x86_64 mdir mcopy; do
        if ! command -v "$binary" >/dev/null 2>&1; then
            echo "  MISSING $binary (run Setup with -WithQemu)"
            missing=1
        fi
    done
fi
if [[ "$missing" -ne 0 ]]; then
    echo "[ZillaOS] Prerequisites incomplete. ISO build will not start yet. On Windows run -Action Setup."
    exit 2
fi
echo "[ZillaOS] Ubuntu/WSL toolchain ready to build ISO."
