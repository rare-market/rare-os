#!/usr/bin/env bash
# Offline parser checks: no device needed.
d="$(cd "$(dirname "$0")/.." && pwd)"
python3 -I "$d/timing.py" selftest || exit 1
python3 -I - "$d" <<'PY'
import sys; sys.path.insert(0, sys.argv[1]); import summarize as s
ft = s.mango_frametimes(sys.argv[1] + "/test/mangohud-fixture.csv"); r = s.lows(ft)
assert len(ft) == 1000 and abs(r["fps_1pct_low"] - 20) < 0.01 and abs(r["fps_avg"] - 58.82) < 0.01, r
print("mangohud parser ok")
PY
