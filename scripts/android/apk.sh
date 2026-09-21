#!/usr/bin/env bash
# Standalone Android APK: test (debug-signed, install) or release (release-signed).
#   scripts/android/apk.sh test
#   scripts/android/apk.sh release
set -euo pipefail

MODE="${1:-}"
if [ "$MODE" != "test" ] && [ "$MODE" != "release" ]; then
  echo ">> usage: apk.sh test|release"
  exit 1
fi

ANDROID_SCRIPTS_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$ANDROID_SCRIPTS_DIR/android-env.sh"
cd "$ROOT_DIR"

newest_apk() {
  find "$ROOT_DIR/src-tauri/gen/android/app/build/outputs/apk" \
    -name "$1" -printf "%T@\t%p\n" 2>/dev/null | sort -nr | cut -f2- | head -n1
}

ensure_debug_keystore() {
  local ks="$HOME/.android/debug.keystore"
  if [ -f "$ks" ]; then
    echo "$ks"
    return
  fi
  ks="$ROOT_DIR/.toolchains/android-debug.keystore"
  if [ ! -f "$ks" ]; then
    mkdir -p "$(dirname "$ks")"
    keytool -genkeypair -keystore "$ks" -alias androiddebugkey \
      -keyalg RSA -keysize 2048 -validity 10000 \
      -storepass android -keypass android \
      -dname "CN=Android Debug,O=Android,C=US"
  fi
  echo "$ks"
}

ensure_release_keystore() {
  local ks="$ROOT_DIR/.toolchains/android-release.keystore"
  local envf="$ROOT_DIR/.toolchains/android-release.env"
  mkdir -p "$ROOT_DIR/.toolchains"
  if [ ! -f "$ks" ] || [ ! -f "$envf" ]; then
    local pass
    pass="$(openssl rand -hex 16)"
    keytool -genkeypair -keystore "$ks" -alias veydan \
      -keyalg RSA -keysize 2048 -validity 10000 \
      -storepass "$pass" -keypass "$pass" \
      -dname "CN=Veydan,O=Veydan Project,C=US"
    cat > "$envf" <<EOT
ANDROID_RELEASE_KS_PASS=$pass
ANDROID_RELEASE_KEY_ALIAS=veydan
EOT
    echo ">> Created release keystore $ks (keep .toolchains/android-release.* )"
  fi
  echo "$ks"
}

echo ">> Building standalone APK ($MODE)"
pnpm tauri android build

unsigned="$(newest_apk '*-unsigned.apk')"
if [ -z "$unsigned" ]; then
  echo ">> No unsigned APK. tauri android build failed?"
  exit 1
fi

signer="$(ls -1 "$ANDROID_HOME"/build-tools/*/apksigner 2>/dev/null | tail -n1)"
if [ -z "$signer" ]; then
  echo ">> apksigner not found"
  exit 1
fi

if [ "$MODE" = "test" ]; then
  ks="$(ensure_debug_keystore)"
  alias="androiddebugkey"
  storepass="android"
  keypass="android"
  signed="${unsigned%-unsigned.apk}-test.apk"
else
  ks="$(ensure_release_keystore)"
  # shellcheck disable=SC1091
  source "$ROOT_DIR/.toolchains/android-release.env"
  alias="${ANDROID_RELEASE_KEY_ALIAS:-veydan}"
  storepass="${ANDROID_RELEASE_KS_PASS:?missing ANDROID_RELEASE_KS_PASS}"
  keypass="$storepass"
  signed="${unsigned%-unsigned.apk}-release.apk"
fi

echo ">> Signing $unsigned -> $signed"
"$signer" sign --ks "$ks" --ks-key-alias "$alias" \
  --ks-pass "pass:$storepass" --key-pass "pass:$keypass" \
  --out "$signed" "$unsigned"

if [ "$MODE" = "test" ]; then
  echo ">> Installing $signed"
  adb install -r "$signed"
else
  echo ">> Release APK: $signed"
fi
