#!/usr/bin/env bash
# Run the app on a connected Android device (or emulator).
#   scripts/android/dev.sh            # picks the connected device
#   scripts/android/dev.sh "<name>"   # explicit device/emulator name
set -e

ANDROID_SCRIPTS_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$ANDROID_SCRIPTS_DIR/android-env.sh"
cd "$ROOT_DIR"

# Free android-only ports if a stale session holds them (do not touch desktop 1420)
for port in 1430 1431; do
  pid=$(ss -ltnp 2>/dev/null | awk -v p=":$port" '$4 ~ p"$" {match($0, /pid=[0-9]+/); print substr($0, RSTART+4, RLENGTH-4)}' | head -n1)
  [ -n "$pid" ] && kill "$pid" 2>/dev/null || true
done

# First run: generate the Android Studio project under src-tauri/gen/android
if [ ! -d "$ROOT_DIR/src-tauri/gen/android" ]; then
  pnpm tauri android init
fi

# Physical device: Tauri would embed the WSL NAT IP, which the phone cannot reach.
# Tunnel android vite (1430) and HMR (1431) through adb; desktop keeps 1420.
adb reverse tcp:1430 tcp:1430
adb reverse tcp:1431 tcp:1431

exec pnpm tauri android dev --host 127.0.0.1 "$@"
