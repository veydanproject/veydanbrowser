#!/usr/bin/env bash
# Build, send to the server, switch, check. One command:
#
#   make vpush-deploy
#
# VPUSH_SKIP_BUILD=1 sends what is already in dist/.
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/remote.sh"
cd "$VPUSH_DIR"

[ "${VPUSH_SKIP_BUILD:-0}" = 1 ] || bash scripts/build-release.sh

[ -x dist/vpush ] || { echo "vpush: nothing to send, dist/vpush is missing" >&2; exit 1; }
REL="$(cat dist/RELEASE)"
SHA="$(cut -d' ' -f1 dist/vpush.sha256)"
UPLOAD="/tmp/vpush-upload-$REL-$$"

echo ">> sending $REL to $REMOTE:$VPUSH_REMOTE_DIR"
upload dist/vpush "$UPLOAD"
run_remote remote-activate.sh "$VPUSH_REMOTE_DIR" "$REL" "$UPLOAD" "$SHA"
echo ">> $REL is live on $VPUSH_HOST"
