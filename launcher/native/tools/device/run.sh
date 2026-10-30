#!/usr/bin/bash
# The native Rare launcher, started by rare-session-client (RARE_LAUNCHER_BIN, set for the session by
# ~/.config/environment.d/rare-native.conf, or $XDG_RUNTIME_DIR/rare-launcher-bin).
# A normal exit (0, SIGINT, SIGTERM: an update restart or a quit) starts the native launcher again.
# A crash puts the fallback launcher in $XDG_RUNTIME_DIR/rare-launcher-bin, which is gone at the next
# boot, so the session never ends or loops on a broken build and the next boot tries native again.
# The fallback is $D/fallback-bin, else the AppImage, else the image's /usr/bin/rare-launcher.
D=$HOME/rare-native
run=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
[[ -r $D/native.env ]] && { set -a; . "$D/native.env"; set +a; }
# This launch runs the newest installed files, so a restart left pending by an install is done.
rm -f "$D/.restart-pending"
"$D/rare-launcher-native" "$@" >>"$D/native.log" 2>&1
rc=$?
case $rc in
0 | 130 | 143) ;;
*)
    fb=$(cat "$D/fallback-bin" 2>/dev/null)
    [[ -x $fb ]] || fb=$HOME/Applications/Rare-Launcher-x86_64.AppImage
    [[ -x $fb ]] || fb=/usr/bin/rare-launcher
    echo "$fb" >"$run/rare-launcher-bin"
    echo "[$(date +%T)] native exited $rc: fallback $fb until reboot" >>"$D/native.log"
    ;;
esac
touch "$run/rare-launcher-restart"
exit $rc
