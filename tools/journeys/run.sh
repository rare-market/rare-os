#!/usr/bin/env bash
# Rare OS experience journeys: drives the real SuiPlay through every journey in tools/journeys/journeys
# and writes an objective report. Run from the Mac. See tools/journeys/README.md.
#
#   tools/journeys/run.sh check                      read-only preflight on the device (changes nothing)
#   tools/journeys/run.sh run --i-have-a-window [options]
#       --only a,b  --skip a,b  --tags core          which journeys (default: all)
#       --sudo                                       sleep/wake: asks for the sudo password here, arms the RTC over ssh
#       --attended                                   sleep/wake: the owner presses power to wake
#       --no-video                                   thumbnails only (no mp4 clips)
#       --wait-idle MIN                              keep waiting up to MIN minutes for the device to go idle
#       --repeat N  --max-minutes M
#   tools/journeys/run.sh abort                      stop a run (the runner cleans up and returns Home)
#   tools/journeys/run.sh restore-config RUN_ID      put back launcher settings a run changed (restarts the launcher)
#   tools/journeys/run.sh pull RUN_ID                fetch a finished run, analyse, cut clips
#   tools/journeys/run.sh analyze DIR                re-analyse a local run
#   tools/journeys/run.sh selftest | lint
#
# Env: HOST (suiplay), OUT (bench/journeys). Device files live in /var/tmp/rare-journeys.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
HOST="${HOST:-suiplay}"
OUT="${OUT:-$ROOT/bench/journeys}"
RD=/var/tmp/rare-journeys
PY="${PYTHON:-python3}"
r() { ssh -o ConnectTimeout=10 -o ServerAliveInterval=15 "$HOST" "$@"; }

push() {
    (cd "$HERE" && COPYFILE_DISABLE=1 tar -cf - --exclude '__pycache__' jlib device journeys) |
        r "rm -rf $RD/code && mkdir -p $RD/code $RD/runs && tar -C $RD/code -xf -"
}

finish() { # $1 = run id: pull, analyse, clips, report
    local id="$1" dst="$OUT/$1"
    mkdir -p "$dst"
    r "tar -C $RD/runs/$id --exclude video.mkv --exclude clips -cf - ." | tar -C "$dst" -xf -
    "$PY" "$HERE/analyze.py" "$dst" || true
    if r "test -f $RD/runs/$id/video.mkv"; then
        r "cat > $RD/runs/$id/clips.json" <"$dst/clips.json"
        r "cd $RD/code && python3 device/clips.py $RD/runs/$id" || true
        r "tar -C $RD/runs/$id -cf - clips 2>/dev/null" | tar -C "$dst" -xf - 2>/dev/null || true
        "$PY" "$HERE/analyze.py" "$dst" >/dev/null || true
        echo "full video stays on the device: $HOST:$RD/runs/$id/video.mkv"
    fi
    echo "report: $dst/report.html"
}

cmd="${1:-}"; shift || true
case "$cmd" in
check)
    push
    r "cd $RD/code && python3 device/check.py --watch ${WATCH:-10}"
    ;;
run)
    window=0 sudo=0 args=()
    while (($#)); do
        case "$1" in
        --i-have-a-window) window=1 ;;
        --sudo) sudo=1; args+=(--rtc-driver) ;;
        --wait-idle) args+=(--wait-max "$2"); shift ;;
        --only|--skip|--tags|--repeat|--max-minutes) args+=("$1" "$2"); shift ;;
        --attended|--no-video|--allow-uinput|--ip-only) args+=("$1") ;;
        *) echo "unknown option $1" >&2; exit 2 ;;
        esac
        shift
    done
    ((window)) || { echo "refusing: pass --i-have-a-window (the device is yours to drive now)" >&2; exit 2; }
    "$PY" "$HERE/analyze.py" --lint >/dev/null
    pw=""
    if ((sudo)); then read -r -s -p "sudo password for $HOST (kept in memory, used only to arm the RTC wake alarm): " pw; echo; fi
    id="$(date +%Y%m%d-%H%M%S)"
    push
    r "rm -f $RD/STOP; cd $RD/code && setsid nohup python3 device/runner.py --i-have-a-window --out $RD/runs/$id --stop-file $RD/STOP ${args[*]:-} >$RD/runs/$id.log 2>&1 </dev/null & echo started run $id"
    trap 'echo; echo "stopping: the runner cleans up and returns Home"; r "touch $RD/STOP" || true' INT
    off=0
    while :; do
        sleep 2
        chunk="$(r "tail -c +$((off + 1)) $RD/runs/$id.log 2>/dev/null" || true)"
        if [[ -n "$chunk" ]]; then printf '%s\n' "$chunk"; off=$((off + $(printf '%s\n' "$chunk" | wc -c))); fi
        if ((sudo)) && r "test -f $RD/runs/$id/rtc-request -a ! -f $RD/runs/$id/rtc-armed" 2>/dev/null; then
            secs="$(r "cat $RD/runs/$id/rtc-request")"
            if printf '%s\n' "$pw" | r "sudo -S -p '' sh -c 'echo 0 > /sys/class/rtc/rtc0/wakealarm; echo +$secs > /sys/class/rtc/rtc0/wakealarm'" 2>/dev/null; then
                r "echo ok > $RD/runs/$id/rtc-armed"
            else
                r "echo 'sudo failed' > $RD/runs/$id/rtc-armed"
            fi
        fi
        r "test -f $RD/runs/$id/done" 2>/dev/null && break
        r "pgrep -f 'runner.py --i-have-a-window --out $RD/runs/$id' >/dev/null" 2>/dev/null || { echo "runner exited"; break; }
    done
    trap - INT
    pw=""
    r "tail -c +$((off + 1)) $RD/runs/$id.log" || true
    r "test -d $RD/runs/$id" && finish "$id"
    ;;
abort) r "touch $RD/STOP" && echo "asked the runner to stop" ;;
restore-config)
    # Puts back ~/.config/rare as it was before a run, then restarts only the launcher.
    id="${1:?run id}"
    r "set -e; cp -f $RD/runs/$id/config-before/* \$HOME/.config/rare/ && touch \$XDG_RUNTIME_DIR/rare-launcher-restart && pkill -x rare-launcher && echo restored"
    ;;
pull) finish "${1:?run id}" ;;
analyze) "$PY" "$HERE/analyze.py" "${1:?run dir}" ;;
selftest) "$PY" "$HERE/selftest.py" "$@" ;;
lint) "$PY" "$HERE/analyze.py" --lint ;;
*) sed -n '2,20p' "$0"; exit 2 ;;
esac
