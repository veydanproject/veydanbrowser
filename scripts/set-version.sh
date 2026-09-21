#!/usr/bin/env bash
set -euo pipefail

TO="${1:?version required}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

echo "$TO" > "$ROOT/VERSION"
sed -i "s/\"version\": \"[^\"]*\"/\"version\": \"$TO\"/" "$ROOT/package.json"
sed -i "s/\"version\": \"[^\"]*\"/\"version\": \"$TO\"/" "$ROOT/src-tauri/tauri.conf.json"
sed -i "0,/^version = \"[^\"]*\"/{s/^version = \"[^\"]*\"/version = \"$TO\"/}" "$ROOT/src-tauri/Cargo.toml"

IFS=. read -r NMAJOR NMINOR NPATCH <<<"$TO"
CODE=$((2000000 + NMAJOR * 10000 + NMINOR * 100 + NPATCH))
sed -i "s/\"versionCode\": [0-9]*/\"versionCode\": $CODE/" "$ROOT/src-tauri/tauri.android.conf.json"
