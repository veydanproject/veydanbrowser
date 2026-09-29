#!/usr/bin/env bash
# Prepares a fresh server, once:  make vpush-bootstrap
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/remote.sh"
cd "$VPUSH_DIR"

echo ">> preparing $REMOTE:$VPUSH_REMOTE_DIR"
upload deploy/vpush.service "/tmp/vpush.service.$$"
upload deploy/vpush.example.toml "/tmp/vpush.example.toml.$$"
run_remote remote-bootstrap.sh "$VPUSH_REMOTE_DIR" "/tmp/vpush.service.$$" "/tmp/vpush.example.toml.$$"
