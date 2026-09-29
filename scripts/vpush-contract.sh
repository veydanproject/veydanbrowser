#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Veydan Project
# SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1
#
# The push server and the messenger share no code: either can be taken out
# of this repository by itself. What they share is the protocol, written
# down as examples in VPush/spec/golden. The messenger's tests read a copy
# of that folder; this check fails when the copy differs.
#
#   scripts/vpush-contract.sh          compare
#   scripts/vpush-contract.sh --sync   take the server's examples
set -euo pipefail
cd "$(dirname "$0")/.."

SERVER=VPush/spec/golden
CLIENT=src-tauri/crates/messenger/push/tests/golden

if [ ! -d "$SERVER" ]; then
  echo "vpush contract: $SERVER is not here (VPush lives elsewhere now): nothing to compare with"
  exit 0
fi

if [ "${1:-}" = "--sync" ]; then
  rm -rf "$CLIENT"
  cp -r "$SERVER" "$CLIENT"
  echo "vpush contract: $CLIENT is a copy of $SERVER again"
  exit 0
fi

if diff -r "$SERVER" "$CLIENT"; then
  echo "vpush contract: ok"
else
  echo "::error::the messenger's copy of the push protocol examples differs from the server's"
  echo "vpush contract: FAILED. After making sure the client still speaks the protocol:"
  echo "  scripts/vpush-contract.sh --sync"
  exit 1
fi
