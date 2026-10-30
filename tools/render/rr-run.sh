#!/usr/bin/bash
# Native launcher (rare-render) against the current launcher on the SuiPlay. Runs ON the device,
# detached (run-device.sh starts it). Every step runs under the armed idle guard; foreign input or a
# lost guard stops all input and restores (rr-restore.sh) at once.
#
#   1. refuse to start next to another harness, or when Rare is not in front
#   2. 5 minutes with no input (idle.py 300), then the guard, armed
#   3. current launcher (the image's, cpu mode): cold start, 20 s still, shelf sweep, tab tour
#   4. native launcher, the same, plus a second sweep with RR_LATENCY=1
#   5. restore what was there (override or none), launcher on Home, guard off
set -u
export FLASHCAP=${FLASHCAP:-$HOME/rrbench}
export RARE_RESTORE=$FLASHCAP/rr-restore.sh
export DISPLAY=:0
R=$FLASHCAP
run=$XDG_RUNTIME_DIR
LOG=$R/run.log
log() { echo "$(date +%T) $*" | tee -a "$LOG"; }
others() { ps -eo args | grep -qE "[d]evice/runner.py|[f]lashcap/capture.sh|[f]lashcap/eikey.py|[f]lashcap/pad.py|[f]lashcap/variant.sh"; }
focused() { xprop -root GAMESCOPE_FOCUSED_APP | grep -qw 1380012613; }
lostcheck() { [[ -e $R/INPUT ]] && { log "STOP: $(cat "$R/INPUT")"; exit 4; }; python3 -I "$R/beat.py" check >/dev/null || { log "STOP: guard"; exit 4; }; }
bench() { lostcheck; python3 -I "$R/bench.py" "$@" 2>&1 | tee -a "$LOG"; local rc=${PIPESTATUS[0]}; [[ $rc == 4 ]] && { log "STOP: guard lost in bench"; exit 4; }; lostcheck; }
switch_to() {  # switch_to <override path or empty> <label>; restarts only the launcher, measures it
    local bin=$1 label=$2 old=$3 new=$4
    if [[ -n $bin ]]; then echo "$bin" >"$run/rare-launcher-bin"; else rm -f "$run/rare-launcher-bin"; fi
    bench cold "$label" "$old" "$new"
    for _ in $(seq 40); do focused && break; sleep 0.5; done
    sleep 8
}

rm -f "$R/INPUT" "$R/ARMED" "$R/KEEP-NATIVE"
: >"$LOG"
others && { log "another harness is running: not starting"; exit 3; }
focused || { log "Rare is not in front: not starting"; exit 3; }
cat "$run/rare-launcher-bin" 2>/dev/null >"$R/saved-bin" || : >"$R/saved-bin"
log "override before the test: '$(cat "$R/saved-bin")'"
log "idle precheck: 5 min with no input"
python3 -I "$R/idle.py" 300 "$R/PRE-INPUT" >>"$LOG" 2>&1 || { log "input during the precheck: not starting"; exit 2; }
others && { log "another harness started meanwhile: not starting"; exit 3; }
focused || { log "Rare not in front after the precheck: not starting"; exit 3; }
setsid nohup "$R/guard-run.sh" 3600 >/dev/null 2>&1 </dev/null &
for _ in $(seq 40); do python3 -I "$R/beat.py" check >/dev/null && break; sleep 0.1; done
python3 -I "$R/beat.py" check >/dev/null || { log "guard did not start"; exit 1; }
touch "$R/ARMED"
log "guard up, armed"

SWEEP=(right right right right right right left left left left left left hold:right:2.5 wait:0.6 hold:left:2.5)
TABS=(rb rb rb rb rb lb lb lb lb lb)

log "== current launcher (image, cpu)"
switch_to "" base rare-launcher rare-launcher
bench idle 20 'rare-launcher|WebKit'
PAD_GAP=0.5 bench press base-sweep "${SWEEP[@]}"
PAD_GAP=1.0 bench press base-tabs "${TABS[@]}"
bench idle 20 'rare-launcher|WebKit'

log "== native (rare-render)"
touch "$R/KEEP-NATIVE"
: >"$R/native.env"
switch_to "$R/rr-wrap.sh" native rare-launcher '^rare-render$'
grep -E "first frame|gpu:|window:|pad:" "$R/rare-render.log" | tail -6 | tee -a "$LOG"
p=$(pgrep -x rare-render); log "native pid $p layers: $(grep -oE 'VkLayer[A-Za-z_]*|libvulkan_radeon' /proc/$p/maps 2>/dev/null | sort -u | tr '\n' ' ')"
bench idle 20 '^rare-render$'
PAD_GAP=0.5 bench press native-sweep "${SWEEP[@]}"
PAD_GAP=1.0 bench press native-tabs "${TABS[@]}"
bench idle 20 '^rare-render$'
log "== native, a second cold start (warm disk cache)"
switch_to "$R/rr-wrap.sh" native2 '^rare-render$' '^rare-render$'
log "== native, RR_LATENCY=1"
echo "RR_LATENCY=1" >"$R/native.env"
switch_to "$R/rr-wrap.sh" native-l1 '^rare-render$' '^rare-render$'
PAD_GAP=0.5 bench press native-l1-sweep "${SWEEP[@]}"
: >"$R/native.env"

log "== restore"
rm -f "$R/ARMED" "$R/KEEP-NATIVE"
"$R/rr-restore.sh" >>"$LOG" 2>&1
python3 -I "$R/beat.py" stop >/dev/null 2>&1
log "done: override now '$(cat "$run/rare-launcher-bin" 2>/dev/null)', launcher $(pgrep -x rare-launcher), focused $(xprop -root GAMESCOPE_FOCUSED_APP | cut -d= -f2)"
