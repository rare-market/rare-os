#!/usr/bin/bash
# Rare OS: Android apps and games through Waydroid. Written by the Rare launcher (the rare-android
# crate writes this file on start; changes here are overwritten).
#
#   rare-android run <package> [WxH]   the app fullscreen (cage inside gamescope) until it closes or Rare quits it
#   rare-android start | restart       the container only (restart applies its settings)
#   rare-android up                    Android in the background, with no window; returns once it has started
#   rare-android down                  stops Android (the session and the container)
#   rare-android install <apk>         installs an APK (Android must be up)
#   rare-android clear <package>       clears an app's data (Android must be up)
#   rare-android path <package>        the folder of an app's APK, inside Android
#   rare-android cmd <service> <args>  an Android shell command over binder (binder-cmd.py)
#
# The container's start, stop and the controller fix run as root through the base image's polkit
# actions (/usr/libexec/waydroid-container-*, org.bazzite.policykit.waydroid.*).
set -u
here=$(dirname "$(readlink -f "$0")")
state=${XDG_RUNTIME_DIR:-/tmp}/rare-android
# How the root helpers run: pkexec on the device (RARE_ANDROID_PRIV=sudo on a test box).
priv=${RARE_ANDROID_PRIV:-pkexec}
mkdir -p "$state"

log() { echo "[rare-android] $*" >&2; }
container_up() { [[ "$(systemctl is-active waydroid-container.service 2>/dev/null)" == active ]]; }
# Both can block while Android is still starting its services: bounded.
session_up() { timeout 10 waydroid status 2>/dev/null | grep -q 'Session:.*RUNNING'; }
booted() { [[ "$(timeout 5 waydroid prop get sys.boot_completed 2>/dev/null | tail -n1)" == 1 ]]; }

start_container() {
    container_up && return 0
    "$priv" /usr/libexec/waydroid-container-start >/dev/null 2>&1 || log "container start was refused"
    for _ in $(seq 60); do
        container_up && return 0
        sleep 0.5
    done
    log "the container did not start"
    return 1
}

stop_container() {
    container_up || return 0
    "$priv" /usr/libexec/waydroid-container-stop >/dev/null 2>&1 || log "container stop was refused"
}

wait_boot() {
    local n=${1:-240}
    for _ in $(seq "$n"); do
        booted && return 0
        sleep 1
    done
    log "Android did not finish starting"
    return 1
}

# The background session's compositor (cage with no outputs on screen), started by `up`.
headless_pid() { cat "$state/headless.pid" 2>/dev/null; }

stop_headless() {
    local p
    p=$(headless_pid)
    [[ -n "$p" ]] && kill "$p" 2>/dev/null
    rm -f "$state/headless.pid"
}

up() {
    start_container || return 1
    if ! session_up; then
        stop_headless
        local r
        for r in "" pixman; do
            (
                unset WAYLAND_DISPLAY DISPLAY
                export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1
                [[ -n "$r" ]] && export WLR_RENDERER="$r"
                setsid cage -- waydroid session start >"$state/headless.log" 2>&1 &
                echo $! >"$state/headless.pid"
            )
            sleep 3
            kill -0 "$(headless_pid)" 2>/dev/null && break
            log "cage did not stay up${r:+ with $r}"
        done
    fi
    wait_boot 240
}

down() {
    waydroid session stop >/dev/null 2>&1
    stop_headless
    stop_container
}

cmd=${1:-}
shift || true
case "$cmd" in
run)
    pkg=${1:?package}
    size=${2:-$(cat "$here/size" 2>/dev/null || echo 1920x1200)}
    start_container || exit 1
    # A background session (an install) gives way: the app gets one on screen.
    if session_up; then
        waydroid session stop >/dev/null 2>&1
        stop_headless
        sleep 1
    fi
    cage=""
    cleanup() {
        trap - TERM INT EXIT
        [[ -n "$cage" ]] && kill "$cage" 2>/dev/null
        waydroid session stop >/dev/null 2>&1
        wait 2>/dev/null
        stop_container
    }
    trap 'cleanup; exit 0' TERM INT
    trap cleanup EXIT
    # gamescope shows X11 windows: cage runs on its X11 backend and the app fills cage's output.
    (
        unset WAYLAND_DISPLAY
        export WLR_BACKENDS=x11
        exec cage -- bash -c 'wlr-randr --output X11-1 --custom-mode "$1" >/dev/null 2>&1; exec waydroid app launch "$2"' _ "$size" "$pkg"
    ) &
    cage=$!
    # Controllers: Android sees the pads once their devices are announced again after it started.
    (
        wait_boot 240 && sleep 2 && "$priv" /usr/libexec/waydroid-fix-controllers >/dev/null 2>&1
    ) &
    wait "$cage"
    ;;
start) start_container ;;
restart)
    stop_container
    start_container
    ;;
up) up ;;
down) down ;;
install)
    session_up || { log "Android is not running"; exit 1; }
    waydroid app install "${1:?apk}"
    ;;
clear)
    python3 -I "$here/binder-cmd.py" activity force-stop "${1:?package}" >/dev/null
    python3 -I "$here/binder-cmd.py" package clear "$1"
    ;;
path)
    python3 -I "$here/binder-cmd.py" package path "${1:?package}"
    ;;
cmd)
    python3 -I "$here/binder-cmd.py" "$@"
    ;;
*)
    echo "usage: rare-android run|up|down|install|clear|path|cmd" >&2
    exit 2
    ;;
esac
