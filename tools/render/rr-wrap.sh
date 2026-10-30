#!/usr/bin/bash
# The native launcher under test, run by rare-session-client through $XDG_RUNTIME_DIR/rare-launcher-bin.
# Whatever happens to it, the next launcher the session client starts is the one that was there before
# (the override saved in saved-bin, or the image's), so the gamescope session never ends with it.
R=${FLASHCAP:-$HOME/rrbench}
run=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
[[ -r $R/native.env ]] && { set -a; . "$R/native.env"; set +a; }
export RR_STATS="$R/stats-$(date +%s).log"
"$R/rare-render" "$@" >>"$R/rare-render.log" 2>&1
rc=$?
if [[ ! -e $R/KEEP-NATIVE ]]; then
    saved=$(cat "$R/saved-bin" 2>/dev/null)
    if [[ -n $saved ]]; then echo "$saved" >"$run/rare-launcher-bin"; else rm -f "$run/rare-launcher-bin"; fi
fi
touch "$run/rare-launcher-restart"
exit $rc
