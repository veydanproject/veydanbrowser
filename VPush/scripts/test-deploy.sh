#!/usr/bin/env bash
# Checks the server-side deploy scripts without a server: a directory plays
# /opt/vpush, a background process plays systemd.
#
#   make test-deploy      (needs dist/vpush: make release)
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
cd "$VPUSH_DIR"
[ -x dist/vpush ] || { echo "no dist/vpush; run: make release" >&2; exit 1; }

ROOT="$(mktemp -d /tmp/vpush-deploytest-XXXXXX)"
DIR="$ROOT/opt"
trap 'bash "$ROOT/stop.sh" 2>/dev/null || true; rm -rf "$ROOT"' EXIT

mkdir -p "$DIR/releases" "$DIR/etc" "$DIR/data"
port=$(( 20000 + RANDOM % 20000 ))
cat > "$DIR/etc/vpush.toml" <<CONF
public_url = "http://localhost:$port"
[server]
listen = "127.0.0.1:$port"
[admin]
socket = "$ROOT/admin.sock"
[store]
path = "$DIR/data/vpush.db"
CONF
# The database is made by the first release; the deploy of the second saves it.

cat > "$ROOT/start.sh" <<START
#!/usr/bin/env bash
setsid "$DIR/current/vpush" serve --config "$DIR/etc/vpush.toml" >> "$ROOT/log" 2>&1 &
echo \$! > "$ROOT/pid"
START
cat > "$ROOT/stop.sh" <<STOP
#!/usr/bin/env bash
[ -f "$ROOT/pid" ] && kill "\$(cat "$ROOT/pid")" 2>/dev/null
rm -f "$ROOT/pid"
for i in \$(seq 50); do [ -e "$ROOT/admin.sock" ] || break; sleep 0.1; done
exit 0
STOP

export VPUSH_NO_SUDO=1 VPUSH_HEALTH_SECS=6 VPUSH_STEADY_SECS=2 VPUSH_KEEP_RELEASES=2 VPUSH_RESET_CMD=
export VPUSH_START_CMD="bash $ROOT/start.sh" VPUSH_STOP_CMD="bash $ROOT/stop.sh"

pass=0
ok()   { echo "ok    $*"; pass=$((pass + 1)); }
fail() { echo "FAIL  $*"; echo "--- server log"; cat "$ROOT/log" 2>/dev/null; exit 1; }
expect() { local what="$1"; shift; "$@" || fail "$what"; ok "$what"; }

activate() {  # activate <release> <file>
  cp "$2" "$ROOT/upload"
  bash scripts/remote-activate.sh "$DIR" "$1" "$ROOT/upload" "$(sha256sum "$2" | cut -d' ' -f1)"
}
current()  { readlink "$DIR/current"; }
previous() { readlink "$DIR/previous" 2>/dev/null || echo none; }
alive()    { "$DIR/current/vpush" ctl --config "$DIR/etc/vpush.toml" health >/dev/null 2>&1; }

# A release that passes check-config and then dies.
cat > "$ROOT/broken" <<'BROKEN'
#!/usr/bin/env bash
[ "$1" = check-config ] && exit 0
exit 1
BROKEN
chmod +x "$ROOT/broken"

echo "== first release"
activate r1 dist/vpush > "$ROOT/out" 2>&1 || { cat "$ROOT/out"; fail "first release"; }
expect "current is r1"            [ "$(current)" = releases/r1 ]
expect "no previous yet"          [ "$(previous)" = none ]
expect "server is alive"          alive
expect "the first release made the database" [ -f "$DIR/data/vpush.db" ]

echo "== second release"
activate r2 dist/vpush > "$ROOT/out" 2>&1 || { cat "$ROOT/out"; fail "second release"; }
expect "current is r2"            [ "$(current)" = releases/r2 ]
expect "previous is r1"           [ "$(previous)" = releases/r1 ]
expect "database was saved"       [ "$(ls "$DIR/data/backups" | wc -l)" = 1 ]
expect "server is alive"          alive

echo "== a release that does not start"
if activate r3 "$ROOT/broken" > "$ROOT/out" 2>&1; then fail "broken release was accepted"; fi
expect "deploy reported failure"  grep -q "rolled back to releases/r2, which is alive" "$ROOT/out"
expect "current is r2 again"      [ "$(current)" = releases/r2 ]
expect "previous is still r1"     [ "$(previous)" = releases/r1 ]
expect "server is alive"          alive

echo "== a release that answers and falls a moment later"
# It passes check-config, starts, answers the health check, and is gone in
# a second: what a crash right after the start looks like.
cat > "$ROOT/flaky" <<FLAKY
#!/usr/bin/env bash
if [ "\$1" = serve ]; then
  "$VPUSH_DIR/dist/vpush" "\$@" &
  pid=\$!
  sleep 1
  kill \$pid
  wait \$pid
  exit 1
fi
exec "$VPUSH_DIR/dist/vpush" "\$@"
FLAKY
chmod +x "$ROOT/flaky"
if activate r3b "$ROOT/flaky" > "$ROOT/out" 2>&1; then fail "a release that fell was taken for alive"; fi
expect "deploy reported failure"  grep -q "rolled back to releases/r2, which is alive" "$ROOT/out"
expect "current is r2 again"      [ "$(current)" = releases/r2 ]
expect "server is alive"          alive

echo "== a wrong checksum"
cp dist/vpush "$ROOT/upload"
if bash scripts/remote-activate.sh "$DIR" r4 "$ROOT/upload" deadbeef > "$ROOT/out" 2>&1; then
  fail "wrong checksum was accepted"
fi
expect "upload refused"           grep -q "checksum" "$ROOT/out"
expect "nothing was installed"    [ ! -e "$DIR/releases/r4" ]
expect "server is alive"          alive

echo "== a config the new release refuses"
cp "$DIR/etc/vpush.toml" "$ROOT/good.toml"
echo 'surprise = 1' >> "$DIR/etc/vpush.toml"
if activate r5 dist/vpush > "$ROOT/out" 2>&1; then fail "bad config was accepted"; fi
expect "reason is given"          grep -q "nothing was changed" "$ROOT/out"
expect "current is still r2"      [ "$(current)" = releases/r2 ]
cp "$ROOT/good.toml" "$DIR/etc/vpush.toml"
expect "server was not touched"   alive

echo "== rollback by hand"
bash scripts/remote-rollback.sh "$DIR" > "$ROOT/out" 2>&1 || { cat "$ROOT/out"; fail "rollback"; }
expect "current is r1"            [ "$(current)" = releases/r1 ]
expect "previous is r2"           [ "$(previous)" = releases/r2 ]
expect "server is alive"          alive

echo "== old releases are removed"
activate r6 dist/vpush > "$ROOT/out" 2>&1 || { cat "$ROOT/out"; fail "release r6"; }
expect "current and previous stay" [ -d "$DIR/releases/r6" -a -d "$DIR/releases/r1" ]
expect "the broken one is gone"    [ ! -e "$DIR/releases/r3" ]

echo
echo "deploy scripts: $pass checks passed"
