#!/usr/bin/bash
# Device side: restart only the launcher in a render mode (render.rs RARE_WEBKIT) until reboot, wait
# until it is in front again, and print whether its web process loaded Mesa.
#   mode.sh <gpu|cpu|dmabuf|wayland>
# The restart blanks the screen for a moment; rare-session-client keeps the gamescope session up.
set -u
mode=${1:?mode}
export DISPLAY=${DISPLAY:-:0}
run=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
echo "$mode" >"$run/rare-webkit"
old=$(pgrep -x rare-launcher)
touch "$run/rare-launcher-restart"; pkill -x rare-launcher
for _ in $(seq 60); do sleep 0.5; p=$(pgrep -x rare-launcher); [[ -n $p && $p != "$old" ]] && break; done
for _ in $(seq 40); do sleep 0.5; xprop -root GAMESCOPE_FOCUSED_APP | grep -qw 1380012613 && break; done
sleep "${SETTLE:-6}"
p=$(pgrep -x rare-launcher)
echo "launcher $p $(tr '\0' '\n' </proc/$p/environ | grep -E '^RARE_WEBKIT=') focused:$(xprop -root GAMESCOPE_FOCUSED_APP | cut -d= -f2)"
for w in $(pgrep -f WebKitWebProcess); do echo "WebKitWebProcess $w: $(grep -oE '(radeonsi|libgallium[^ ]*|libEGL_mesa)' /proc/$w/maps | sort -u | tr '\n' ' ')"; done
