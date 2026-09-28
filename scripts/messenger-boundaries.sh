#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Veydan Project
# SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1
#
# Dependency-boundary check for the messenger module (docs/messenger-spec.md §4.2).
# Fails when a messenger crate reaches outside its allowed dependencies or
# when messenger UI imports host code beyond the allowed primitives.
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0
say() { echo "::error::$*"; fail=1; }

# 1. Messenger crates must not depend on Tauri or the host crate tree.
for toml in src-tauri/crates/messenger/*/Cargo.toml; do
  crate=$(basename "$(dirname "$toml")")
  if grep -qE '^\s*tauri' "$toml"; then say "$crate: depends on tauri"; fi
  if grep -qE 'path\s*=\s*"\.\./\.\./' "$toml"; then say "$crate: path dependency outside crates/messenger"; fi
  if grep -qE 'veydan-sync|veydanspace' "$toml"; then say "$crate: depends on host crates"; fi
done

# 2. Per-crate allow-list of intra-messenger dependencies.
allowed() {
  case "$1" in
    core)      echo "" ;;
    store)     echo "messenger-core" ;;
    transport) echo "messenger-core" ;;
    identity)  echo "messenger-core" ;;
    ingress)   echo "messenger-core messenger-store messenger-identity" ;;
    contacts)  echo "messenger-core messenger-store" ;;
    dm)        echo "messenger-core messenger-store messenger-contacts" ;;
    media)     echo "messenger-core messenger-store" ;;
    groups)    echo "messenger-core messenger-store messenger-media messenger-dm" ;;
    runtime)   echo "messenger-core messenger-store messenger-transport messenger-identity messenger-ingress messenger-contacts messenger-dm messenger-media messenger-groups" ;;
    testkit)   echo "messenger-core messenger-store messenger-runtime" ;;
    *)         echo "__unknown__" ;;
  esac
}
for toml in src-tauri/crates/messenger/*/Cargo.toml; do
  crate=$(basename "$(dirname "$toml")")
  allow=$(allowed "$crate")
  [ "$allow" = "__unknown__" ] && { say "$crate: not in the dependency matrix (update scripts/messenger-boundaries.sh and the spec)"; continue; }
  # [dependencies] section only; dev-dependencies may use testkit freely.
  deps=$(awk '/^\[dependencies\]/{f=1;next} /^\[/{f=0} f && /^messenger-/{print $1}' "$toml" | sed 's/\..*//')
  for d in $deps; do
    case " $allow " in
      *" $d "*) ;;
      *) say "$crate: '$d' is not an allowed dependency" ;;
    esac
  done
done

# 3. Messenger UI may import only its own files, ui primitives, i18n, Icon and Svelte/Tauri APIs.
while IFS= read -r line; do
  file=${line%%:*}
  spec=$(echo "$line" | sed -nE "s/.*from ['\"]([^'\"]+)['\"].*/\1/p")
  case "$spec" in
    ""|./*|../*|\$lib/messenger/*|\$lib/components/ui/*|\$lib/i18n|\$lib/Icon.svelte|\$app/*|svelte|svelte/*|@tauri-apps/*) ;;
    *) say "$file imports '$spec' (messenger UI may not depend on host modules)" ;;
  esac
done < <(grep -rnE "^\s*import .* from ['\"]" src/lib/messenger --include='*.ts' --include='*.svelte' || true)

if [ "$fail" -ne 0 ]; then
  echo "messenger boundaries: FAILED"
  exit 1
fi
echo "messenger boundaries: ok"
