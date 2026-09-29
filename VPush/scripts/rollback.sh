#!/usr/bin/env bash
# Back to the release that ran before the current one:  make vpush-rollback
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/remote.sh"
run_remote remote-rollback.sh "$VPUSH_REMOTE_DIR"
