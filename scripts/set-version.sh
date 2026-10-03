#!/usr/bin/env bash
set -euo pipefail

TO="${1:?version required}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

echo "$TO" > "$ROOT/VERSION"
sed -i "s/\"version\": \"[^\"]*\"/\"version\": \"$TO\"/" "$ROOT/package.json"
sed -i "s/\"version\": \"[^\"]*\"/\"version\": \"$TO\"/" "$ROOT/src-tauri/tauri.conf.json"
sed -i "0,/^version = \"[^\"]*\"/{s/^version = \"[^\"]*\"/version = \"$TO\"/}" "$ROOT/src-tauri/Cargo.toml"
# The app's own entry in its lock file: without it `cargo --locked` refuses
# the tree (the lock said 4.0.4 while the crate was 4.0.7).
sed -i "/^name = \"veydanspace\"$/{n;s/^version = \"[^\"]*\"/version = \"$TO\"/}" "$ROOT/src-tauri/Cargo.lock"

IFS=. read -r NMAJOR NMINOR NPATCH <<<"$TO"
CODE=$((2000000 + NMAJOR * 10000 + NMINOR * 100 + NPATCH))
sed -i "s/\"versionCode\": [0-9]*/\"versionCode\": $CODE/" "$ROOT/src-tauri/tauri.android.conf.json"
