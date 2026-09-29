#!/usr/bin/env bash
# How the scripts reach the server — meant to be *sourced*.
#
# Settings come from deploy.env (see deploy/deploy.example). One ssh
# connection is shared by all steps, so a password is asked once.

source "$(dirname "${BASH_SOURCE[0]}")/env.sh"

DEPLOY_ENV="${VPUSH_DEPLOY_ENV:-}"
if [ -z "$DEPLOY_ENV" ]; then
  # Next to the Makefile, or next to the example it was copied from.
  for candidate in "$VPUSH_DIR/deploy.env" "$VPUSH_DIR/deploy/deploy.env"; do
    [ -f "$candidate" ] && { DEPLOY_ENV="$candidate"; break; }
  done
fi
if [ ! -f "$DEPLOY_ENV" ]; then
  echo "vpush: no deploy.env" >&2
  echo "       cp deploy/deploy.example deploy.env   and put the server in it" >&2
  exit 1
fi
# shellcheck disable=SC1090
source "$DEPLOY_ENV"

: "${VPUSH_HOST:?VPUSH_HOST is not set in $DEPLOY_ENV}"
VPUSH_USER="${VPUSH_USER:-root}"
VPUSH_PORT="${VPUSH_PORT:-22}"
VPUSH_REMOTE_DIR="${VPUSH_REMOTE_DIR:-/opt/vpush}"

mkdir -p "$HOME/.ssh"
SSH_OPTS=(
  -p "$VPUSH_PORT"
  -o ControlMaster=auto
  -o "ControlPath=$HOME/.ssh/vpush-%C"
  -o ControlPersist=120
  -o ServerAliveInterval=15
)
SCP_OPTS=(
  -P "$VPUSH_PORT"
  -o ControlMaster=auto
  -o "ControlPath=$HOME/.ssh/vpush-%C"
  -o ControlPersist=120
)
if [ -n "${VPUSH_SSH_KEY:-}" ]; then
  SSH_OPTS+=(-i "$VPUSH_SSH_KEY")
  SCP_OPTS+=(-i "$VPUSH_SSH_KEY")
fi
REMOTE="$VPUSH_USER@$VPUSH_HOST"

# A password from deploy.env is handed to ssh through its own askpass hook,
# so nothing has to be installed and the password is never on a command line.
if [ -n "${VPUSH_PASSWORD:-}" ]; then
  ASKPASS="$(mktemp "${TMPDIR:-/tmp}/vpush-askpass-XXXXXX")"
  chmod 700 "$ASKPASS"
  printf '#!/bin/sh\nprintf "%%s\\n" "$VPUSH_PASSWORD"\n' > "$ASKPASS"
  trap 'rm -f "$ASKPASS"' EXIT
  export VPUSH_PASSWORD SSH_ASKPASS="$ASKPASS" SSH_ASKPASS_REQUIRE=force
  # Every question of ssh goes to the hook, and it knows one answer. A server
  # seen for the first time is accepted and remembered; one whose key changed
  # is still refused.
  PASSWORD_OPTS=(
    -o StrictHostKeyChecking=accept-new
    -o PreferredAuthentications=password,keyboard-interactive
    -o PubkeyAuthentication=no
    -o NumberOfPasswordPrompts=1
  )
  SSH_OPTS+=("${PASSWORD_OPTS[@]}")
  SCP_OPTS+=("${PASSWORD_OPTS[@]}")
fi

# Variables the server-side scripts read, passed through as given here.
remote_env() {
  local out="" v
  for v in VPUSH_KEEP_RELEASES VPUSH_KEEP_BACKUPS VPUSH_HEALTH_SECS; do
    [ -n "${!v:-}" ] && out+="$v=$(printf %q "${!v}") "
  done
  echo "$out"
}

# run_remote <script> <args...>: the script travels over stdin.
run_remote() {
  local script="$1"; shift
  local args="" a
  for a in "$@"; do args+=" $(printf %q "$a")"; done
  ssh "${SSH_OPTS[@]}" "$REMOTE" "$(remote_env)bash -s --$args" < "$VPUSH_DIR/scripts/$script"
}

# For commands a person watches (logs): gets a terminal.
run_remote_tty() {
  ssh -t "${SSH_OPTS[@]}" "$REMOTE" "$@"
}

upload() {  # upload <local> <remote path>
  scp -q "${SCP_OPTS[@]}" "$1" "$REMOTE:$2"
}

sudo_prefix() {
  [ "$VPUSH_USER" = root ] || echo "sudo "
}
