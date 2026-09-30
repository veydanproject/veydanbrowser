#!/usr/bin/env bash
# Runs ON THE SERVER: swaps `current` and `previous`.
#
#   remote-rollback.sh <dir>
set -euo pipefail

DIR="$1"
HEALTH_SECS="${VPUSH_HEALTH_SECS:-30}"
STEADY_SECS="${VPUSH_STEADY_SECS:-8}"
STOP_CMD="${VPUSH_STOP_CMD:-systemctl stop vpush}"
START_CMD="${VPUSH_START_CMD:-systemctl start vpush}"
RESET_CMD="${VPUSH_RESET_CMD-systemctl reset-failed vpush}"
SUDO=""; [ "$(id -u)" -eq 0 ] || SUDO="sudo"
[ -n "${VPUSH_NO_SUDO:-}" ] && SUDO=""

# Sent ahead of this script by ssh; sourced when it is run by hand.
declare -F alive >/dev/null || source "$(dirname "${BASH_SOURCE[0]}")/remote-lib.sh"

CONFIG="$DIR/etc/vpush.toml"

[ -L "$DIR/previous" ] || die "there is no previous release"
NOW="$(readlink "$DIR/current")"
BACK="$(readlink "$DIR/previous")"
[ -x "$DIR/$BACK/vpush" ] || die "$BACK is gone from the server"

say "going from $NOW back to $BACK"
$SUDO $STOP_CMD || true
point current "$BACK"
point previous "$NOW"
start

# The same test a new release has to pass: up, and staying up.
if alive; then
  say "alive on $BACK"
  exit 0
fi

# It does not stay up either. The release that was running is put back, so
# that the links say what is on the server; which of the two to look at is
# for a person.
$SUDO $STOP_CMD || true
point current "$NOW"
point previous "$BACK"
start
die "$BACK does not stay up; back on $NOW. Look at the log: make vpush-logs"
