#!/usr/bin/env bash
# Looking at the server from here.
#
#   server.sh status          which release runs, is it alive
#   server.sh logs [args]     follow the log (args go to journalctl)
#   server.sh ctl <args>      vpush ctl on the server: ctl log set relay=debug --for 15m
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/remote.sh"

S="$(sudo_prefix)"
D="$VPUSH_REMOTE_DIR"
quote() { local out="" a; for a in "$@"; do out+=" $(printf %q "$a")"; done; echo "$out"; }

cmd="${1:-status}"; shift || true
case "$cmd" in
  status)
    run_remote_tty "echo current:  \$(readlink $D/current 2>/dev/null || echo none); \
      echo previous: \$(readlink $D/previous 2>/dev/null || echo none); \
      echo releases: \$(ls -1t $D/releases 2>/dev/null | tr '\n' ' '); \
      ${S}systemctl is-active vpush; \
      ${S}$D/current/vpush ctl --config $D/etc/vpush.toml health"
    ;;
  logs)
    if [ $# -eq 0 ]; then set -- -f -n 200; fi
    run_remote_tty "${S}journalctl -u vpush -o cat$(quote "$@")"
    ;;
  ctl)
    run_remote_tty "${S}$D/current/vpush ctl --config $D/etc/vpush.toml$(quote "$@")"
    ;;
  *)
    echo "usage: server.sh status | logs [journalctl args] | ctl <args>" >&2
    exit 1
    ;;
esac
