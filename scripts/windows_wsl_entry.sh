#!/usr/bin/env bash
# WSL entry point: pass repository path as argv, never through shell interpolation.
set -Eeuo pipefail
if (( $# < 2 )); then
  echo "Usage: windows_wsl_entry.sh REPO_DIR {check|setup|build|smoke} [--with-qemu]" >&2
  exit 2
fi
REPO_DIR="$1"
MODE="$2"
shift 2
if [[ ! -f "$REPO_DIR/Cargo.toml" || ! -f "$REPO_DIR/scripts/windows_wsl_setup.sh" ]]; then
  echo "Invalid ZillaOS repo in WSL: $REPO_DIR" >&2
  exit 2
fi
cd -- "$REPO_DIR"
if [[ -f "$HOME/.cargo/env" ]]; then
  source "$HOME/.cargo/env"
fi
case "$MODE" in
  check)
    exec bash scripts/windows_wsl_setup.sh --check "$@"
    ;;
  setup)
    exec bash scripts/windows_wsl_setup.sh --install --install-rust "$@"
    ;;
  build)
    if (( $# != 0 )); then echo "build takes no additional arguments" >&2; exit 2; fi
    bash scripts/windows_wsl_setup.sh --check
    mkdir -p build/logs
    bash scripts/build_vmware.sh 2>&1 | tee build/logs/windows-iso-build.log
    ;;
  smoke)
    if (( $# != 0 )); then echo "smoke takes no additional arguments" >&2; exit 2; fi
    exec python3 scripts/desktop_smoke.py --timeout 120
    ;;
  *)
    echo "Unknown WSL action: $MODE" >&2
    exit 2
    ;;
esac
