#!/usr/bin/env bash
# ============================================================================
# check-styles.sh — dependency-free design-system guardrail.
#
#   FAIL: a component references a CSS custom property that is not defined in
#         src/lib/styles/tokens.css and is not a known runtime-set var. This is
#         the exact class of bug that made panels render transparent/inherited.
#   WARN: raw hex colours and raw font-sizes inside component <style> blocks
#         (should use tokens). Non-failing — a drift meter, not a gate.
#
# Usage:  bash scripts/check-styles.sh          (from repo root)
# Exit:   non-zero if any undefined token reference is found.
# ============================================================================
set -uo pipefail
cd "$(dirname "$0")/.." || exit 2

TOKENS="src/lib/styles/tokens.css"
SVELTE_FILES=$(find src -name '*.svelte')

# Vars that are legitimately set at runtime (inline style / JS setProperty),
# or provided by the shell/layout, so they won't appear as definitions in tokens.css.
RUNTIME_VARS="--col-color --ws-color --inspector-bar-height --page-max --qa-zoom --drawer-w-lg"

defined=$(grep -oE '^\s*--[a-z0-9-]+:' "$TOKENS" | tr -d ' :' | sort -u)
defined="$defined
$(printf '%s\n' $RUNTIME_VARS)"

used=$(grep -rhoE 'var\(--[a-z0-9-]+' $SVELTE_FILES | sed 's/var(//' | sort -u)

fail=0
echo "── Undefined token references (FAIL) ──"
for v in $used; do
  if ! grep -qxF -- "$v" <<< "$defined"; then
    echo "  ✗ $v"
    grep -rn "var($v[,)]" $SVELTE_FILES | sed 's/^/      /' | head -4
    fail=1
  fi
done
[ "$fail" -eq 0 ] && echo "  ✓ none — every var() resolves to a defined token"

echo ""
echo "── Drift meter (WARN, non-failing) ──"
hex=$(grep -rhoiE '#[0-9a-f]{3,8}\b' $SVELTE_FILES | grep -vi '%23' | wc -l | tr -d ' ')
fs=$(grep -rhoE 'font-size:\s*[0-9.]+(rem|px)' $SVELTE_FILES | wc -l | tr -d ' ')
echo "  raw hex colours in components:  $hex   (prefer var(--…))"
echo "  raw font-size values:           $fs   (prefer var(--fs-*))"

echo ""
if [ "$fail" -ne 0 ]; then
  echo "RESULT: FAIL — fix undefined token references above."
  exit 1
fi
echo "RESULT: OK"
