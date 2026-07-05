#!/usr/bin/env bash
# Project-local toolchain bootstrap + environment.
#
# Installs Rust and Node entirely inside .toolchains/ so the host system
# (/usr, $HOME) stays untouched. Everything lives under the project dir and
# is git-ignored. Idempotent — safe to source on every run.
#
# This file is meant to be *sourced* by dev.sh (it exports env vars).

# SCRIPT_DIR is expected to be exported by the caller (dev.sh). Fall back to
# resolving it here so the script also works when sourced standalone.
: "${SCRIPT_DIR:=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}"

TC_DIR="$SCRIPT_DIR/.toolchains"
export RUSTUP_HOME="$TC_DIR/rustup"
export CARGO_HOME="$TC_DIR/cargo"
NODE_DIR="$TC_DIR/node"
NODE_VERSION="v24.18.0"

mkdir -p "$TC_DIR"

# --- Rust (rustup + cargo, minimal profile) ---
if [ ! -x "$CARGO_HOME/bin/cargo" ]; then
  echo ">> Installing Rust into .toolchains/ ..."
  curl -sSf --proto '=https' --tlsv1.2 https://sh.rustup.rs -o /tmp/_rb_rustup.sh
  RUSTUP_HOME="$RUSTUP_HOME" CARGO_HOME="$CARGO_HOME" \
    sh /tmp/_rb_rustup.sh -y --no-modify-path --profile minimal >/dev/null
  rm -f /tmp/_rb_rustup.sh
fi
export PATH="$CARGO_HOME/bin:$PATH"

# --- Node (portable tarball) ---
if [ ! -x "$NODE_DIR/bin/node" ]; then
  echo ">> Installing Node $NODE_VERSION into .toolchains/ ..."
  tarball="node-$NODE_VERSION-linux-x64"
  curl -sSL "https://nodejs.org/dist/$NODE_VERSION/$tarball.tar.xz" -o /tmp/_rb_node.tar.xz
  mkdir -p "$NODE_DIR"
  tar -xJf /tmp/_rb_node.tar.xz -C "$NODE_DIR" --strip-components=1
  rm -f /tmp/_rb_node.tar.xz
fi
export PATH="$NODE_DIR/bin:$PATH"

# --- pnpm (installed globally *inside* the portable Node prefix) ---
if [ ! -x "$NODE_DIR/bin/pnpm" ]; then
  echo ">> Installing pnpm into .toolchains/node ..."
  npm install -g pnpm@latest >/dev/null 2>&1
fi

# Keep the pnpm content-addressable store inside the project too.
export npm_config_store_dir="$SCRIPT_DIR/.pnpm-store"
