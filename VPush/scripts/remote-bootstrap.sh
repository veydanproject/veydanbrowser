#!/usr/bin/env bash
# Runs ON THE SERVER, once: user, directories, systemd unit, first config.
#
#   remote-bootstrap.sh <dir> <unit file> <example config>
#
# Safe to run again: what exists is left as it is.
set -euo pipefail

DIR="$1"; UNIT="$2"; EXAMPLE="$3"
SUDO=""; [ "$(id -u)" -eq 0 ] || SUDO="sudo"

say() { echo "   [server] $*"; }

if ! id vpush >/dev/null 2>&1; then
  $SUDO useradd --system --home-dir "$DIR" --shell /usr/sbin/nologin vpush
  say "user vpush created"
fi

$SUDO mkdir -p "$DIR/releases" "$DIR/etc/secrets" "$DIR/data/backups" "$DIR/log"
$SUDO chown root:vpush "$DIR/etc"
$SUDO chmod 0750 "$DIR/etc"
$SUDO chown vpush:vpush "$DIR/etc/secrets" "$DIR/data" "$DIR/data/backups" "$DIR/log"
$SUDO chmod 0700 "$DIR/etc/secrets"
$SUDO chmod 0750 "$DIR/data" "$DIR/log"

if [ ! -f "$DIR/etc/vpush.toml" ]; then
  $SUDO install -m 0640 -o root -g vpush "$EXAMPLE" "$DIR/etc/vpush.toml"
  say "config written to $DIR/etc/vpush.toml  <-- set public_url in it"
else
  say "config $DIR/etc/vpush.toml is kept"
fi

sed "s|/opt/vpush|$DIR|g" "$UNIT" | $SUDO tee /etc/systemd/system/vpush.service >/dev/null
$SUDO systemctl daemon-reload
$SUDO systemctl enable vpush >/dev/null 2>&1
rm -f "$UNIT" "$EXAMPLE"
say "systemd unit installed and enabled"
say "ready for: make vpush-deploy"
