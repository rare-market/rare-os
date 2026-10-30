#!/usr/bin/bash
# Restore after a native test (the guard's RARE_RESTORE): the launcher setup that was there before
# (saved override or none) running and in front. Falls back to restarting the session.
R=${FLASHCAP:-$HOME/rrbench}
run=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
export DISPLAY=${DISPLAY:-:0}
rm -f "$R/KEEP-NATIVE"
saved=$(cat "$R/saved-bin" 2>/dev/null)
if [[ -n $saved ]]; then echo "$saved" >"$run/rare-launcher-bin"; else rm -f "$run/rare-launcher-bin"; fi
if pgrep -x rare-render >/dev/null; then
    touch "$run/rare-launcher-restart"
    pkill -x rare-render
fi
for _ in $(seq 60); do
    sleep 0.5
    pgrep -x rare-launcher >/dev/null && xprop -root GAMESCOPE_FOCUSED_APP | grep -qw 1380012613 && { echo "restored: $(pgrep -ax rare-launcher | head -1)"; exit 0; }
done
echo "launcher not back in 30 s: restarting the session"
systemctl --user restart gamescope-session-plus@rare.service
