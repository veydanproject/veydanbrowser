#!/usr/bin/env bash
# Runs ON THE SERVER, fed through ssh by deploy.sh.
#
#   remote-activate.sh <dir> <release> <uploaded binary> <sha256>
#
# Puts the release in place, switches to it and checks that it lives.
# If it does not, the previous release is put back.
set -euo pipefail

DIR="$1"; REL="$2"; UPLOAD="$3"; SHA="$4"
KEEP_RELEASES="${VPUSH_KEEP_RELEASES:-5}"
KEEP_BACKUPS="${VPUSH_KEEP_BACKUPS:-10}"
HEALTH_SECS="${VPUSH_HEALTH_SECS:-30}"
# Replaced in tests, where there is no systemd.
STOP_CMD="${VPUSH_STOP_CMD:-systemctl stop vpush}"
START_CMD="${VPUSH_START_CMD:-systemctl start vpush}"
RESET_CMD="${VPUSH_RESET_CMD-systemctl reset-failed vpush}"
# A release counts as alive when it has answered for this long without a restart.
STEADY_SECS="${VPUSH_STEADY_SECS:-8}"

SUDO=""; [ "$(id -u)" -eq 0 ] || SUDO="sudo"
[ -n "${VPUSH_NO_SUDO:-}" ] && SUDO=""

# Sent ahead of this script by ssh; sourced when it is run by hand.
declare -F alive >/dev/null || source "$(dirname "${BASH_SOURCE[0]}")/remote-lib.sh"

CONFIG="$DIR/etc/vpush.toml"
[ -d "$DIR/releases" ] || die "$DIR is not set up; run 'make vpush-bootstrap' once"
[ -f "$CONFIG" ] || die "no config at $CONFIG"

echo "$SHA  $UPLOAD" | sha256sum -c --status - || die "checksum of the upload does not match"

# --- put the release in place ------------------------------------------------
$SUDO mkdir -p "$DIR/releases/$REL"
$SUDO install -m 0755 "$UPLOAD" "$DIR/releases/$REL/vpush"
rm -f "$UPLOAD"
NEW="$DIR/releases/$REL/vpush"
say "release $REL is on the server"

# The new binary reads the live config before anything is switched.
$SUDO "$NEW" check-config --config "$CONFIG" >/dev/null \
  || die "the new release does not accept $CONFIG; nothing was changed"

# --- switch ------------------------------------------------------------------
OLD=""
[ -L "$DIR/current" ] && OLD="$(readlink "$DIR/current")"

say "stopping"
$SUDO $STOP_CMD || true

if [ -f "$DIR/data/vpush.db" ]; then
  $SUDO mkdir -p "$DIR/data/backups"
  backup="$DIR/data/backups/vpush-$(date -u +%Y%m%dT%H%M%SZ)-before-$REL.db"
  # A snapshot made by the database itself: what was still in the
  # write-ahead log is in it too, which a copy of the file would miss
  # after a stop that was not clean.
  if ! $SUDO "$NEW" backup --config "$CONFIG" --out "$backup" >/dev/null; then
    start
    die "the database could not be saved; nothing was switched, the old release is started again"
  fi
  $SUDO chown --reference="$DIR/data/vpush.db" "$backup"
  $SUDO chmod 0640 "$backup"
  say "database saved to $backup"
  ls -1t "$DIR"/data/backups/*.db 2>/dev/null | tail -n +"$((KEEP_BACKUPS + 1))" \
    | while read -r f; do $SUDO rm -f "$f"; done
fi

point current "releases/$REL"
say "starting $REL"
start

if alive; then
  [ -n "$OLD" ] && [ "$OLD" != "releases/$REL" ] && point previous "$OLD"
  say "alive: $($SUDO "$DIR/current/vpush" ctl --config "$CONFIG" health | tr -d ' \n')"
  # Old releases go, except the ones the two links point at.
  ls -1t "$DIR/releases" | tail -n +"$((KEEP_RELEASES + 1))" | while read -r r; do
    case "releases/$r" in
      "$(readlink "$DIR/current")"|"$(readlink "$DIR/previous" 2>/dev/null)") ;;
      *) $SUDO rm -rf "$DIR/releases/$r" ;;
    esac
  done
  exit 0
fi

# --- it did not come up ------------------------------------------------------
say "release $REL did not answer in ${HEALTH_SECS}s"
$SUDO $STOP_CMD || true
if [ -n "$OLD" ]; then
  point current "$OLD"
  start
  if alive; then
    die "rolled back to $OLD, which is alive. Look at the log: make vpush-logs"
  fi
  die "rolled back to $OLD, and it does not answer either. Look at the log: make vpush-logs"
fi
die "there was no release before this one to go back to"
