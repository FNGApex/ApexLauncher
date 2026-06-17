#!/usr/bin/env bash
#
# ApexLauncher build dispatcher — the single entrypoint for building, testing,
# and running the app across every dev environment.
#
#   WSL2        Mirror the source tree onto the native Windows filesystem
#               (C:\Users\drgor\Documents\GitHub\ApexLauncher) and build it there
#               via scripts/apex-build.bat. We do NOT build inside WSL: the Linux
#               Tauri target needs GTK/WebKit libs that aren't installed, and the
#               Windows toolchain reading the repo over the \\wsl.localhost UNC
#               path (the old approach) is slow and forces CARGO_INCREMENTAL=0.
#               Building on native NTFS restores incremental compilation.
#
#   macOS/Linux Build natively in place. (A real Linux build needs the GTK/WebKit
#               dev libs; macOS needs Xcode command line tools.)
#
# Usage:
#   scripts/build.sh [check|test|build|dev] [extra args...]
#
#   check   cargo check + tsc --noEmit (fast typecheck, both sides)
#   test    cargo test (extra args are forwarded as a test filter)   [default]
#   build   produce a real installable bundle for smoke testing (tauri build)
#   dev     launch the dev window (HMR)
#
# Examples:
#   scripts/build.sh                       # run the full Rust test suite
#   scripts/build.sh test core::download   # run only matching tests
#   scripts/build.sh check                 # quick typecheck before committing
#   scripts/build.sh build                 # bundle the app
#
set -euo pipefail

MODE="${1:-test}"
[ $# -gt 0 ] && shift || true
EXTRA=("$@")

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

is_wsl() { grep -qi microsoft /proc/version 2>/dev/null; }

ensure_node() { [ -d node_modules ] || npm install; }

# ---- WSL: mirror to native Windows FS, then build there --------------------
run_wsl() {
  local WIN_WSL_DIR="/mnt/c/Users/drgor/Documents/GitHub/ApexLauncher"
  local WIN_DOS_BAT='C:\Users\drgor\Documents\GitHub\ApexLauncher\scripts\apex-build.bat'

  echo "[build] WSL detected — mirroring source to $WIN_WSL_DIR"
  mkdir -p "$WIN_WSL_DIR"

  # Mirror with --delete so the Windows tree always matches WSL exactly,
  # *except* for the expensive build caches (target/, node_modules/), which the
  # Windows side owns and rebuilds with native binaries. Excluded paths are
  # neither copied nor deleted on the receiving side.
  rsync -rlt --delete --modify-window=1 --no-perms --no-owner --no-group \
    --exclude '.git/' \
    --exclude 'node_modules/' \
    --exclude 'src-tauri/target/' \
    --exclude 'src-tauri/gen/' \
    --exclude '.worktrees/' \
    --exclude 'dist/' \
    --exclude 'tmp/' \
    --exclude '.claude/.scratchpad/' \
    "$REPO_ROOT/" "$WIN_WSL_DIR/"

  echo "[build] running native Windows build — mode=$MODE"
  # Run from /mnt/c so cmd.exe doesn't warn about a UNC cwd.
  ( cd /mnt/c && cmd.exe /c "$WIN_DOS_BAT" "$MODE" "${EXTRA[@]}" )
}

# ---- macOS / Linux: native build in place ----------------------------------
run_native() {
  command -v cargo >/dev/null 2>&1 || . "$HOME/.cargo/env" 2>/dev/null || true
  case "$MODE" in
    check)
      cargo check --manifest-path src-tauri/Cargo.toml
      ensure_node
      npx tsc --noEmit
      ;;
    test)
      cargo test --manifest-path src-tauri/Cargo.toml ${EXTRA[@]+"${EXTRA[@]}"}
      ;;
    build)
      ensure_node
      npm run tauri build
      echo "[build] bundle: src-tauri/target/release/bundle"
      ;;
    dev)
      ensure_node
      npm run tauri dev
      ;;
    *)
      echo "[build] unknown mode '$MODE' (use: check|test|build|dev)" >&2
      exit 2
      ;;
  esac
}

if is_wsl; then run_wsl; else run_native; fi
echo "[build] done ($MODE)"
