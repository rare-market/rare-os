#!/usr/bin/bash
# Run the native launcher now: point the session client at run.sh and restart only the launcher.
# (Every boot does this by itself through ~/.config/environment.d/rare-native.conf.)
D=$HOME/rare-native
run=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
echo "$D/run.sh" >"$run/rare-launcher-bin"
touch "$run/rare-launcher-restart"
pkill -TERM -f "bash $D/run.sh"; pkill -TERM -f "$D/rare-launcher-native"
pkill -TERM -x rare-launcher
echo "native launcher starting"
