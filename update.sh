#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DEV_PREFIX="$SCRIPT_DIR/.dev-prefix"
PKG_DIR="$DEV_PREFIX/usr/lib/x86_64-linux-gnu/pkgconfig"

export PKG_CONFIG_PATH="$PKG_DIR:/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig"
export CFLAGS="-I$DEV_PREFIX/usr/include"
export CXXFLAGS="-I$DEV_PREFIX/usr/include"
export RUSTFLAGS="-L $DEV_PREFIX/usr/lib/x86_64-linux-gnu -L /usr/lib/x86_64-linux-gnu"

source "$HOME/.cargo/env" 2>/dev/null || true

export NVM_DIR="$HOME/.config/nvm"
[ -s "$NVM_DIR/nvm.sh" ] && \. "$NVM_DIR/nvm.sh"

cd "$SCRIPT_DIR"

echo ">> Building Veydan Browser..."
pnpm tauri build --no-bundle

echo ""
echo ">> Done! Binary: $SCRIPT_DIR/src-tauri/target/release/veydanbrowser"
echo ">> Restart the app from the menu to use the updated version."
