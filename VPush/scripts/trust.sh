#!/usr/bin/env bash
# Records the server's key, once:  make vpush-trust
#
# Every other command talks only to a server that shows this key. It is
# taken from this user's own known_hosts when the server was visited before;
# otherwise it is asked of the server and shown, and recorded only after a
# "yes": compare the fingerprint with what the hosting panel or the server's
# console says (`ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub`).
set -euo pipefail

VPUSH_TRUSTING=1 source "$(dirname "${BASH_SOURCE[0]}")/remote.sh"

# As ssh names a server in known_hosts: bare for port 22, in brackets otherwise.
NAME="$VPUSH_HOST"
[ "$VPUSH_PORT" = 22 ] || NAME="[$VPUSH_HOST]:$VPUSH_PORT"

if [ -s "$KNOWN_HOSTS" ]; then
  echo ">> the key of $NAME is already recorded in $KNOWN_HOSTS:"
  ssh-keygen -lf "$KNOWN_HOSTS" | sed 's/^/   /'
  echo "   delete the file to record it anew"
  exit 0
fi

FOUND="$(mktemp)"
trap 'rm -f "$FOUND"' EXIT

if [ -f "$HOME/.ssh/known_hosts" ] && ssh-keygen -F "$NAME" -f "$HOME/.ssh/known_hosts" 2>/dev/null | grep -v '^#' > "$FOUND" && [ -s "$FOUND" ]; then
  echo ">> $NAME is known to this user already; its key, from ~/.ssh/known_hosts:"
else
  echo ">> asking $NAME for its key"
  ssh-keyscan -T 10 -p "$VPUSH_PORT" -t ed25519,ecdsa,rsa "$VPUSH_HOST" 2>/dev/null > "$FOUND"
  [ -s "$FOUND" ] || { echo "vpush: $NAME did not answer" >&2; exit 1; }
  ssh-keygen -lf "$FOUND" | sed 's/^/   /'
  echo "   Compare with the server's own word before saying yes: whoever"
  echo "   answers here gets the password of every command after."
  printf "   Record this key? [y/N] "
  read -r answer < /dev/tty
  case "$answer" in y|Y|yes) ;; *) echo "   not recorded"; exit 1 ;; esac
  FOUND_SHOWN=1
fi

[ -n "${FOUND_SHOWN:-}" ] || ssh-keygen -lf "$FOUND" | sed 's/^/   /'
install -m 600 "$FOUND" "$KNOWN_HOSTS"
echo ">> recorded in $KNOWN_HOSTS"
