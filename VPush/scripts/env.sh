#!/usr/bin/env bash
# Build environment of VPush — meant to be *sourced*.
#
# Inside the Veydan Space repository the project-local toolchain is used
# (../.toolchains). Anywhere else, whatever cargo is on PATH.

VPUSH_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export VPUSH_DIR

if [ -x "$VPUSH_DIR/../.toolchains/cargo/bin/cargo" ]; then
  VPUSH_TOOLS="$(cd "$VPUSH_DIR/../.toolchains" && pwd)"
  export RUSTUP_HOME="$VPUSH_TOOLS/rustup"
  export CARGO_HOME="$VPUSH_TOOLS/cargo"
  export PATH="$CARGO_HOME/bin:$PATH"
else
  # Tools VPush fetches for itself (zig) go here when it lives alone.
  VPUSH_TOOLS="$VPUSH_DIR/.tools"
fi
export VPUSH_TOOLS

if ! command -v cargo >/dev/null 2>&1; then
  echo "vpush: cargo not found; install Rust from https://rustup.rs" >&2
  return 1 2>/dev/null || exit 1
fi
