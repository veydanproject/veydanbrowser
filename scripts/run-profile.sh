#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Veydan Project
# SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1
#
# One more instance of the dev build with its own data directory (--workdir),
# next to a running `make dev`: it reuses that build and its
# vite server, and unlike dev.sh kills neither.
#
# usage: scripts/run-profile.sh <workdir>     (or: make dev-profile WORKDIR=<dir>)
# No -u: build-env.sh reads variables that may be unset.
set -eo pipefail

dir="${1:-${WORKDIR:-}}"
if [ -z "$dir" ]; then
  echo ">> usage: make dev-profile WORKDIR=<dir>" >&2
  exit 1
fi
# Relative to where the command was typed, not to the repo.
VEYDAN_WORKDIR="$(realpath -m "$dir")"
export VEYDAN_WORKDIR

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
export SCRIPT_DIR
source "$SCRIPT_DIR/build-env.sh" >/dev/null

BIN="$SCRIPT_DIR/src-tauri/target/debug/veydanspace"
if [ ! -x "$BIN" ]; then
  echo ">> no dev build yet: start 'make dev' first" >&2
  exit 1
fi
if ! (exec 3<>/dev/tcp/127.0.0.1/1420) 2>/dev/null; then
  echo ">> the dev server is not running: start 'make dev' first" >&2
  exit 1
fi

echo ">> profile: $VEYDAN_WORKDIR"
exec "$SCRIPT_DIR/webkit-run.sh" "$BIN"
