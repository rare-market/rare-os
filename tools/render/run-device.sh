#!/usr/bin/env bash
# Mac side: copy the bench and the native binary to the SuiPlay (~/rrbench), start rr-run.sh there
# detached (it survives an ssh drop), and fetch the results when it is done.
#   tools/render/run-device.sh <rare-render binary> [fetch]
# Results land in $RESULTS (default $TMPDIR/rare-render-bench): run.log, captures, stats logs;
# fetch then runs the device budget gates (budget.py --device) and exits 1 if one fails.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
flash="$here/../journeys/flash"
HOST=${HOST:-suiplay}
out=${RESULTS:-${TMPDIR:-/tmp}/rare-render-bench}
mkdir -p "$out"
if [[ ${2:-} != fetch ]]; then
  bin=${1:?binary}
  ssh "$HOST" 'mkdir -p ~/rrbench'
  scp -q "$flash/beat.py" "$flash/idle.py" "$flash/guard-run.sh" "$flash/pad.py" "$here/bench.py" "$here/rr-wrap.sh" "$here/rr-restore.sh" "$here/rr-run.sh" "$HOST:rrbench/"
  scp -q "$bin" "$HOST:rrbench/rare-render"
  ssh "$HOST" 'chmod +x ~/rrbench/*.sh ~/rrbench/rare-render && cd ~/rrbench && FLASHCAP=$HOME/rrbench setsid nohup ./rr-run.sh >/dev/null 2>&1 </dev/null & sleep 1; tail -3 ~/rrbench/run.log'
  exit 0
fi
ssh "$HOST" 'cd ~/rrbench && tar cf - run.log rare-render.log guard.log stats-*.log */ 2>/dev/null' | tar xf - -C "$out"
echo "fetched to $out"
# The device frame-budget gates (CPU per frame p99 < 4 ms, no layout while animating, at most one
# layout per input, no interval over 25 ms while focus moves) over every native stats log.
rc=0
for f in "$out"/stats-*.log; do
  [[ -e $f ]] || continue
  python3 "$here/budget.py" --device "$f" || rc=1
done
exit $rc
