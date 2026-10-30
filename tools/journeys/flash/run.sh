#!/usr/bin/env bash
# Flash check from the Mac: record the SuiPlay's screen while the launcher switches screens, pull the
# frames and look for flash frames (analyze.py). Optionally restarts the launcher in a render mode first.
#
#   tools/journeys/flash/run.sh [--mode gpu|cpu|dmabuf|wayland] <label> [steps...]
#   e.g. run.sh --mode gpu gpu-tabs e e e e e e e q q q q q q q
#
# Steps are capture.sh's (xdotool keys or wait:<s>); none records DUR seconds of whatever happens.
# Env: HOST (ssh host, suiplay), GAP, DUR, KEEP=1 keeps the frames on the device (~/flashcap),
# RESULTS (default $TMPDIR/rare-flash): results land in $RESULTS/<label>/ (frames are ~0.8 MB each,
# keep them out of git).
# Only run it when nobody is using the device: it sends keys and (with --mode) restarts the launcher.
# Keys go out only while the idle guard runs (guard-run.sh, started here for the capture and ended
# after it): foreign input, or the guard exiting or stalling, stops the keys at once.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
HOST=${HOST:-suiplay}
mode=""
if [[ ${1:-} == --mode ]]; then mode=$2; shift 2; fi
label=${1:?label}; shift
out="${RESULTS:-${TMPDIR:-/tmp}/rare-flash}/$label"
mkdir -p "$out"
ssh "$HOST" 'mkdir -p ~/flashcap'
scp -q "$here/capture.sh" "$here/mode.sh" "$here/beat.py" "$here/idle.py" "$here/eikey.py" "$here/guard-run.sh" "$here/restore.sh" "$HOST:flashcap/"
[[ -n $mode ]] && ssh "$HOST" "~/flashcap/mode.sh $mode" | tee "$out/mode.txt"
if (($#)); then
  ssh "$HOST" 'setsid nohup ~/flashcap/guard-run.sh 1800 >/dev/null 2>&1 </dev/null & for i in $(seq 30); do python3 -I ~/flashcap/beat.py check >/dev/null && exit 0; sleep 0.1; done; echo "idle guard did not start" >&2; exit 1'
  trap 'ssh "$HOST" "python3 -I ~/flashcap/beat.py stop" || true' EXIT
fi
ssh "$HOST" "GAP=${GAP:-1.2} DUR=${DUR:-10} ~/flashcap/capture.sh $label $*"
(($#)) && { ssh "$HOST" "python3 -I ~/flashcap/beat.py stop" || true; trap - EXIT; }
ssh "$HOST" "gzip -1 -c ~/flashcap/$label/$label.raw" | gunzip >"$out/$label.raw"
scp -q "$HOST:flashcap/$label/$label.pts" "$out/"
scp -q "$HOST:flashcap/$label/keys.log" "$out/" 2>/dev/null || true
[[ ${KEEP:-0} == 1 ]] || ssh "$HOST" "rm -rf ~/flashcap/$label"
python3 -I "$here/analyze.py" "$out" "$label" | tee "$out/result.txt"
