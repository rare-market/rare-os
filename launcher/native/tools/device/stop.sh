#!/usr/bin/bash
# Run the fallback launcher (AppImage) until reboot.
D=$HOME/rare-native
run=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
fb=$(cat "$D/fallback-bin" 2>/dev/null)
[[ -x $fb ]] || fb=$HOME/Applications/Rare-Launcher-x86_64.AppImage
[[ -x $fb ]] || fb=/usr/bin/rare-launcher
echo "$fb" >"$run/rare-launcher-bin"
touch "$run/rare-launcher-restart"
pkill -TERM -f "$D/rare-launcher-native"
echo "back to: $fb"
