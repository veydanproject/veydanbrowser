#!/usr/bin/env bash
# Runs ON THE SERVER: swaps `current` and `previous`.
#
#   remote-rollback.sh <dir>
set -euo pipefail

DIR="$1"
HEALTH_SECS="${VPUSH_HEALTH_SECS:-30}"
STOP_CMD="${VPUSH_STOP_CMD:-systemctl stop vpush}"
START_CMD="${VPUSH_START_CMD:-systemctl start vpush}"
RESET_CMD="${VPUSH_RESET_CMD-systemctl reset-failed vpush}"
SUDO=""; [ "$(id -u)" -eq 0 ] || SUDO="sudo"
[ -n "${VPUSH_NO_SUDO:-}" ] && SUDO=""

say() { echo "   [server] $*"; }
die() { echo "   [server] FAILED: $*" >&2; exit 1; }

[ -L "$DIR/previous" ] || die "there is no previous release"
NOW="$(readlink "$DIR/current")"
BACK="$(readlink "$DIR/previous")"
[ -x "$DIR/$BACK/vpush" ] || die "$BACK is gone from the server"

point() {
  $SUDO ln -sfn "$2" "$DIR/.$1.new"
  $SUDO mv -T "$DIR/.$1.new" "$DIR/$1"
}

say "going from $NOW back to $BACK"
$SUDO $STOP_CMD || true
point current "$BACK"
point previous "$NOW"
[ -n "$RESET_CMD" ] && { $SUDO $RESET_CMD >/dev/null 2>&1 || true; }
$SUDO $START_CMD || true

deadline=$(( $(date +%s) + HEALTH_SECS ))
while [ "$(date +%s)" -lt "$deadline" ]; do
  if $SUDO "$DIR/current/vpush" ctl --config "$DIR/etc/vpush.toml" health >/dev/null 2>&1; then
    say "alive on $BACK"
    exit 0
  fi
  sleep 1
done
die "$BACK does not answer. Look at the log: make vpush-logs"
