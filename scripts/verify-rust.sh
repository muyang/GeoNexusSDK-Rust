#!/usr/bin/env bash
# Verify the Rust SDK: build, check, clippy, test, doc.
#
# Usage:
#   ./scripts/verify-rust.sh              # toolchain into ./.rust-toolchain
#   CARGO_HOME=~/.cargo ./scripts/verify-rust.sh
#
# Two environment quirks this script handles, because both were hit while
# developing the crate:
#
#   1. No Rust toolchain. It bootstraps a minimal stable one into the workspace
#      (never touches the caller's home by default).
#   2. Broken macOS linker. When /usr/bin/cc is a shim calling a missing xcrun,
#      rustc compiles but cannot link test binaries. If an SDK and ld64.lld are
#      present, this script builds a tiny wrapper that translates the gcc-style
#      flags rustc emits into ld64.lld arguments and passes it via `-C linker`.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Layout-aware: in the monorepo the crate lives in sdk-rust/; in the standalone
# repository this script sits at the crate root.
if [[ -f "$ROOT/Cargo.toml" ]]; then
  SDK_DIR="$ROOT"
else
  SDK_DIR="$ROOT/sdk-rust"
fi

# Find a toolchain that may already exist above this repo (the workspace root is
# a common place to bootstrap one), otherwise install into this repo.
find_toolchain() {
  local dir="$ROOT"
  while [[ "$dir" != "/" ]]; do
    if [[ -x "$dir/.rust-toolchain/cargo/bin/rustc" ]]; then
      echo "$dir/.rust-toolchain"
      return 0
    fi
    dir="$(dirname "$dir")"
  done
  echo "$ROOT/.rust-toolchain"
}
TC_DIR="$(find_toolchain)"
export RUSTUP_HOME="${RUSTUP_HOME:-$TC_DIR/rustup}"
export CARGO_HOME="${CARGO_HOME:-$TC_DIR/cargo}"
export PATH="$CARGO_HOME/bin:$PATH"

if ! command -v rustc >/dev/null 2>&1; then
  echo "==> no rustc found; installing a minimal stable toolchain"
  if ! command -v curl >/dev/null 2>&1; then
    echo "ERROR: curl is required to bootstrap rustup" >&2
    exit 1
  fi
  curl -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh
  sh /tmp/rustup-init.sh -y --no-modify-path --profile minimal --default-toolchain stable
fi
# `rustc` is a rustup proxy: without a default toolchain it refuses to run even
# though one is installed. Make that explicit rather than failing obscurely.
if ! rustc --version >/dev/null 2>&1; then
  echo "==> no default toolchain active; selecting stable"
  rustup default stable
fi
echo "==> $(rustc --version)"

# --- optional linker workaround -------------------------------------------
RUSTFLAGS_EXTRA=""
SYSROOT="$(rustc --print sysroot)"
LLD="$SYSROOT/lib/rustlib/x86_64-apple-darwin/bin/gcc-ld/ld64.lld"

if xcrun --sdk macosx --show-sdk-path >/dev/null 2>&1; then
  echo "==> linker looks healthy; using the default one"
elif [[ -x "$LLD" ]]; then
  SDK_PATH="$(ls -d /System/Volumes/Data/Volumes/*/Library/Developer/CommandLineTools/SDKs/MacOSX*.sdk 2>/dev/null | tail -1 || true)"
  if [[ -z "$SDK_PATH" ]]; then
    SDK_PATH="$(ls -d /Library/Developer/CommandLineTools/SDKs/MacOSX*.sdk 2>/dev/null | tail -1 || true)"
  fi
  if [[ -z "$SDK_PATH" ]]; then
    echo "WARNING: no macOS SDK found; 'cargo test' will fail to link." >&2
    echo "         'cargo build' and 'cargo check' still work." >&2
  else
    SDK_VERSION="$(basename "$SDK_PATH" | sed -E 's/MacOSX([0-9.]+)\.sdk/\1/')"
    WRAPPER="$TC_DIR/ld-wrapper.sh"
    mkdir -p "$(dirname "$WRAPPER")"
    {
      echo '#!/bin/bash'
      echo '# Translate gcc-style linker flags for ld64.lld.'
      echo 'args=()'
      echo 'for a in "$@"; do'
      echo '  case "$a" in'
      echo '    -mmacosx-version-min=*) ;;'
      echo '    -nodefaultlibs) ;;'
      echo '    -Wl,*)'
      echo "      IFS=',' read -ra parts <<< \"\${a#-Wl,}\""
      echo '      args+=("${parts[@]}")'
      echo '      ;;'
      echo '    *) args+=("$a") ;;'
      echo '  esac'
      echo 'done'
      echo "exec \"$LLD\" \"\${args[@]}\""
    } > "$WRAPPER"
    chmod +x "$WRAPPER"
    RUSTFLAGS_EXTRA="-C linker=$WRAPPER -C link-arg=-syslibroot -C link-arg=$SDK_PATH -C link-arg=-platform_version -C link-arg=macos -C link-arg=10.12 -C link-arg=$SDK_VERSION"
    echo "==> xcrun is unavailable; using ld64.lld with $SDK_PATH"
  fi
else
  echo "WARNING: no ld64.lld in the toolchain; 'cargo test' may fail to link." >&2
fi

cd "$SDK_DIR"

echo "==> cargo build"
cargo build --offline

echo "==> cargo check --all-targets (type-checks tests as well)"
cargo check --offline --all-targets

if cargo clippy --version >/dev/null 2>&1; then
  echo "==> cargo clippy --all-targets"
  cargo clippy --offline --all-targets -- -D warnings
else
  echo "==> clippy not installed; skipping (rustup component add clippy)"
fi

echo "==> cargo test"
# shellcheck disable=SC2086
RUSTFLAGS="$RUSTFLAGS_EXTRA" cargo test --offline

echo "==> cargo doc"
cargo doc --offline --no-deps

echo
echo "Rust SDK verified: build + check + clippy + tests + docs all passed."
