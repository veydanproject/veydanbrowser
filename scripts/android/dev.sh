#!/usr/bin/env bash
# Run the app on a connected Android device (or emulator).
#   scripts/android/dev.sh            # picks the connected device
#   scripts/android/dev.sh "<name>"   # explicit device/emulator name
set -e

ANDROID_SCRIPTS_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$ANDROID_SCRIPTS_DIR/android-env.sh"
cd "$ROOT_DIR"

# Free the dev ports if a stale session holds them (fuser is absent on WSL)
for port in 1420 1421; do
  pid=$(ss -ltnp 2>/dev/null | awk -v p=":$port" '$4 ~ p"$" {match($0, /pid=[0-9]+/); print substr($0, RSTART+4, RLENGTH-4)}' | head -n1)
  [ -n "$pid" ] && kill "$pid" 2>/dev/null || true
done

# First run: generate the Android Studio project under src-tauri/gen/android
if [ ! -d "$ROOT_DIR/src-tauri/gen/android" ]; then
  pnpm tauri android init
fi

# Physical device: Tauri would embed the WSL NAT IP, which the phone cannot reach.
# Tunnel dev server (1420) and HMR (1421) through adb instead and point the app at localhost.
adb reverse tcp:1420 tcp:1420
adb reverse tcp:1421 tcp:1421

exec pnpm tauri android dev --host 127.0.0.1 "$@"
