#!/usr/bin/bash
# Runs inside the test VM, as root, once the launcher is up. Exit 0 pass, 1 fail, 77 skip.
#
# A VM has no real GPU, so rare-session runs the launcher in cage there, and the VM test never
# saw gamescope: the first boot on a SuiPlay stayed black because gamescope --steam only shows
# windows that carry an app id, and nothing tagged the launcher's. This runs Rare OS game mode
# the real way (rare-session -> gamescope-session-plus rare -> gamescope --steam) with
# gamescope's headless backend (Vulkan on lavapipe, no display needed) and checks that
# gamescope focuses the launcher's window, and that the launcher got its session environment.
#
# It needs a gamescope with --backend and a Vulkan device (lavapipe). gamescope-session-plus
# gives gamescope 5 s to answer on its socket; under TCG that can be too short, and the step
# then reports SKIP with the session log rather than a result it could not get.
set -uo pipefail
APPID=1380012613 # RARE_APPID in launcher/src-tauri/src/overlay/gamescope.rs
WAIT="${GS_WAIT:-600}"
LOG=/var/tmp/rare-gamescope-focus.log
uid=1000
user="$(id -nu "$uid" 2>/dev/null)" || { echo "SKIP: no user $uid"; exit 77; }
rt="/run/user/$uid"
GS='(^|/)gamescope( |$)|gamescope-session-plus' # the compositor and its session script, not this file

skip() { echo "SKIP: $*"; tail -n 40 "$LOG" 2>/dev/null; exit 77; }
fail() { echo "FAIL: $*"; diag; tail -n 60 "$LOG" 2>/dev/null; exit 1; }

command -v gamescope >/dev/null || skip "gamescope is not installed"
command -v xprop >/dev/null || skip "xprop is not installed"
gamescope --help 2>&1 | grep -q -- '--backend' || skip "this gamescope has no --backend option"
grep -q 'BACKEND' /usr/share/gamescope-session-plus/gamescope-session-plus || skip "gamescope-session-plus does not pass BACKEND"

# The cage session holds the launcher's single instance: stop it, keep the user's runtime dir.
loginctl enable-linger "$user"
systemctl start "user@$uid.service"
systemctl stop display-manager.service 2>/dev/null || systemctl stop sddm.service 2>/dev/null
sleep 3
pkill -x rare-launcher; pkill -x cage
sleep 2
rm -f /tmp/chimeraos-short-session-tracker

echo "starting rare-session with RARE_COMPOSITOR=gamescope BACKEND=headless"
runuser -u "$user" -- env -i HOME="$(getent passwd "$user" | cut -d: -f6)" USER="$user" PATH=/usr/local/bin:/usr/bin:/bin \
    XDG_RUNTIME_DIR="$rt" DBUS_SESSION_BUS_ADDRESS="unix:path=$rt/bus" \
    RARE_COMPOSITOR=gamescope BACKEND=headless LIBGL_ALWAYS_SOFTWARE=1 \
    setsid /usr/bin/rare-session >"$LOG" 2>&1 < /dev/null &

as_user() { runuser -u "$user" -- env DISPLAY="$1" ${XAUTH:+XAUTHORITY="$XAUTH"} "${@:2}"; }
nums() { sed -n 's/^[^=]*= *//p' | tr -d ' '; } # "NAME = 1, 2" -> "1,2"
launcher_env() { tr '\0' '\n' <"/proc/$1/environ" 2>/dev/null | sed -n "s/^$2=//p"; }

diag() {
    echo "--- diagnostics"
    ps -eo pid,args | grep -E 'gamescope|rare-launcher|rare-session|Xwayland' | grep -v grep
    [[ -n "${BASE:-}" ]] && for p in GAMESCOPE_FOCUSED_WINDOW GAMESCOPE_FOCUSED_APP GAMESCOPE_FOCUSABLE_WINDOWS GAMESCOPECTRL_BASELAYER_APPID GAMESCOPECTRL_BASELAYER_WINDOW; do
        as_user "$BASE" xprop -root "$p"
    done
}

t0=$SECONDS
pid=""
BASE=""
while (( SECONDS - t0 < WAIT )); do
    sleep 5
    pgrep -f "$GS" >/dev/null || pgrep -x rare-launcher >/dev/null \
        || skip "gamescope did not stay up in this VM (no usable Vulkan device, or too slow to answer under TCG)"
    pid="$(pgrep -x -u "$uid" rare-launcher | head -n1)"
    [[ -n "$pid" ]] || continue
    disp="$(launcher_env "$pid" DISPLAY)"
    XAUTH="$(launcher_env "$pid" XAUTHORITY)"
    [[ -n "$disp" ]] || continue
    BASE=""
    for d in "$disp" :0 :1 :2 :3; do
        if [[ "$(as_user "$d" xprop -root -notype GAMESCOPE_XWAYLAND_SERVER_ID 2>/dev/null | nums)" == 0 ]]; then BASE="$d"; break; fi
    done
    [[ -n "$BASE" ]] || continue
    focused="$(as_user "$BASE" xprop -root -notype GAMESCOPE_FOCUSED_WINDOW 2>/dev/null | nums)"
    focusable="$(as_user "$BASE" xprop -root -notype GAMESCOPE_FOCUSABLE_WINDOWS 2>/dev/null | nums)"
    # [window, appid, pid] triplets: the launcher's window is the one with Rare's app id and pid.
    rare_win="$(tr ',' '\n' <<<"$focusable" | paste - - - | awk -v a="$APPID" -v p="$pid" '$2 == a && $3 == p { print $1; exit }')"
    if [[ -n "$rare_win" && "$focused" == "$rare_win" ]]; then
        echo "launcher pid $pid on $disp, gamescope base $BASE: focused window $focused is the launcher's (app id $APPID) after $((SECONDS - t0)) s"
        ok=0
        [[ "$(launcher_env "$pid" RARE_SESSION_CLIENT)" == 1 ]] || { echo "FAIL: the launcher has no RARE_SESSION_CLIENT=1"; ok=1; }
        # The launcher picks WebKit's renderer itself (render.rs, RARE_WEBKIT); its web process shows the result.
        wp="$(pgrep -P "$pid" -f WebKitWebProcess | head -1)"
        [[ -n "$wp" ]] && echo "web process render env: $(tr '\0' '\n' </proc/"$wp"/environ | grep -E '^(WEBKIT_DISABLE_DMABUF_RENDERER|WEBKIT_DMABUF_RENDERER_FORCE_SHM|GDK_BACKEND)=' | tr '\n' ' ')"
        apps="$(as_user "$BASE" xprop -root -notype GAMESCOPECTRL_BASELAYER_APPID 2>/dev/null | nums)"
        [[ ",$apps," == *",$APPID,"* ]] || { echo "FAIL: base-layer app list '$apps' lacks $APPID"; ok=1; }
        (( ok == 0 )) && echo "PASS"
        pkill -f "$GS"; systemctl start display-manager.service 2>/dev/null
        exit $ok
    fi
done
[[ -z "$pid" ]] && skip "the launcher never started under gamescope within $WAIT s"
[[ -z "$BASE" ]] && skip "no gamescope Xwayland answered within $WAIT s"
fail "gamescope focuses '${focused:-nothing}', the launcher's window is '${rare_win:-not focusable}' (focusable: ${focusable:-none})"
