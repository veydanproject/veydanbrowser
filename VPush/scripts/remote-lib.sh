# What the scripts that run ON THE SERVER share. It travels ahead of each of
# them over ssh (see run_remote), and is sourced when they are run by hand
# or by the tests. Only functions: the variables they read (DIR, CONFIG,
# SUDO, HEALTH_SECS, STEADY_SECS, START_CMD, RESET_CMD) are the script's.

say() { echo "   [server] $*"; }
die() { echo "   [server] FAILED: $*" >&2; exit 1; }

point() {  # point <link> <target>: atomic
  $SUDO ln -sfn "$2" "$DIR/.$1.new"
  $SUDO mv -T "$DIR/.$1.new" "$DIR/$1"
}

# Alive is more than answering once: a release that starts, answers, and
# falls a moment later would pass for alive while systemd starts it over
# and over. So it has to have been up for STEADY_SECS: the time it reports
# starts from nothing whenever it is started anew.
uptime_of() {
  $SUDO "$DIR/current/vpush" ctl --config "$CONFIG" health 2>/dev/null \
    | sed -n 's/.*"uptime_secs": *\([0-9]*\).*/\1/p'
}

alive() {
  local deadline=$(( $(date +%s) + HEALTH_SECS ))
  local up
  while [ "$(date +%s)" -lt "$deadline" ]; do
    up="$(uptime_of)"
    [ -n "$up" ] && [ "$up" -ge "$STEADY_SECS" ] && return 0
    sleep 1
  done
  return 1
}

# A release that kept falling leaves systemd unwilling to start the unit
# again for a while. The release that fixes it must not be held back by that.
start() {
  [ -n "$RESET_CMD" ] && { $SUDO $RESET_CMD >/dev/null 2>&1 || true; }
  $SUDO $START_CMD || true
}
