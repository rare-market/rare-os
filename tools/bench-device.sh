#!/usr/bin/env bash
# Sample power, thermals, CPU and FPS on the SuiPlay over ssh (read only).
# Usage: tools/bench-device.sh <label> [seconds=120] [ssh-host=suiplay]
# Writes bench/<date>-<label>.csv and bench/<date>-<label>.txt (summary), prints the summary.
set -euo pipefail

LABEL="${1:?usage: bench-device.sh <label> [seconds] [host]}"
SECS="${2:-120}"
HOST="${3:-suiplay}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/bench"
BASE="$OUT/$(date +%Y-%m-%d)-$LABEL"
mkdir -p "$OUT"

# Remote sampler: python3 on the device, nothing written outside memory.
# Sources: APU power = amdgpu hwmon power1_average; battery = BAT0 power_now (only
# while Discharging); Tctl = k10temp; edge = amdgpu; fan = any hwmon fan*_input
# (blank when no EC fan driver is loaded); CPU MHz = scaling_cur_freq on all cpus;
# top procs = /proc deltas (percent of one core); FPS/focus = gamescope stats pipe
# (-T path, lines "fps=<f>" and "focus=<app>", fps of the focused window).
read -r -d '' REMOTE <<'PY' || true
import glob, os, sys, time, threading, re
secs = int(sys.argv[1])
def rd(p):
    try:
        with open(p) as f: return f.read().strip()
    except Exception: return ""
def hw(name):
    for h in glob.glob("/sys/class/hwmon/hwmon*"):
        if rd(h + "/name") == name: return h
apu = glob.glob("/sys/class/drm/card*/device/hwmon/hwmon*/power1_average")
apu = apu[0] if apu else ""
k10 = hw("k10temp"); gpu = hw("amdgpu")
fans = glob.glob("/sys/class/hwmon/hwmon*/fan*_input")
cpus = glob.glob("/sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_cur_freq")
fps = {"fps": "", "focus": ""}
def pipe():
    for p in glob.glob("/run/user/1000/gamescope.*/stats.pipe"):
        try:
            with open(p) as f:
                for line in f:
                    k, _, v = line.strip().partition("=")
                    if k in fps: fps[k] = v
        except Exception: pass
threading.Thread(target=pipe, daemon=True).start()
hz = os.sysconf("SC_CLK_TCK"); me = os.getpid()
def procs():
    d = {}
    for p in glob.glob("/proc/[0-9]*"):
        pid = int(p[6:])
        if pid == me: continue
        s = rd(p + "/stat")
        i = s.rfind(")")
        if i < 0: continue
        f = s[i+2:].split()
        d[pid] = (int(f[11]) + int(f[12]), rd(p + "/comm"))
    return d
print("t,apu_w,batt_w,batt_status,tctl_c,edge_c,fan_rpm,cpu_avg_mhz,cpu_max_mhz,fps,focus,top5")
prev = procs(); pt = time.time(); t0 = pt
for n in range(1, secs + 1):
    time.sleep(max(0, t0 + n - time.time()))
    now = time.time(); cur = procs(); dt = now - pt
    agg = {}
    for pid, (j, c) in cur.items():
        if pid in prev:
            agg[c] = agg.get(c, 0) + (j - prev[pid][0]) / hz / dt * 100
    top = "|".join("%s:%.0f" % (re.sub(r"[,|:]", "_", c), v) for c, v in sorted(agg.items(), key=lambda x: -x[1])[:5])
    prev, pt = cur, now
    def num(s, div=1):
        try: return "%.3f" % (float(s) / div)
        except Exception: return ""
    st = rd("/sys/class/power_supply/BAT0/status")
    bw = num(rd("/sys/class/power_supply/BAT0/power_now"), 1e6) if st == "Discharging" else ""
    fr = [int(x) for x in map(rd, cpus) if x.isdigit()]
    fan = num(rd(fans[0])) if fans else ""
    print(",".join([str(n), num(rd(apu), 1e6) if apu else "", bw, st,
        num(rd(k10 + "/temp1_input"), 1000) if k10 else "",
        num(rd(gpu + "/temp1_input"), 1000) if gpu else "", fan,
        "%.0f" % (sum(fr) / len(fr) / 1000) if fr else "", "%.0f" % (max(fr) / 1000) if fr else "",
        fps["fps"], fps["focus"], top]), flush=True)
PY

echo "sampling $HOST for ${SECS}s -> $BASE.csv" >&2
ssh -o ConnectTimeout=10 "$HOST" python3 -c "'$(printf '%s' "$REMOTE" | sed "s/'/'\\\\''/g")'" "$SECS" > "$BASE.csv"

python3 -I - "$BASE.csv" "$LABEL" "$HOST" <<'PY' | tee "$BASE.txt"
import csv, sys, collections
path, label, host = sys.argv[1:4]
rows = list(csv.DictReader(open(path)))
def col(k):
    out = []
    for r in rows:
        try: out.append(float(r[k]))
        except (ValueError, TypeError, KeyError): pass
    return out
def pct(v, p):
    s = sorted(v); i = (len(s) - 1) * p; lo = int(i); hi = min(lo + 1, len(s) - 1)
    return s[lo] + (s[hi] - s[lo]) * (i - lo)
print("bench %s on %s, %d samples at 1 Hz" % (label, host, len(rows)))
print("%-14s %8s %8s %8s %8s" % ("metric", "avg", "p50", "p95", "max"))
for name, k in [("APU W", "apu_w"), ("battery W", "batt_w"), ("Tctl C", "tctl_c"), ("edge C", "edge_c"),
                ("fan RPM", "fan_rpm"), ("CPU avg MHz", "cpu_avg_mhz"), ("CPU max MHz", "cpu_max_mhz"), ("FPS", "fps")]:
    v = col(k)
    if v: print("%-14s %8.2f %8.2f %8.2f %8.2f" % (name, sum(v)/len(v), pct(v, .5), pct(v, .95), max(v)))
    else: print("%-14s %s" % (name, "n/a (no data)"))
print("battery status: " + ", ".join("%s x%d" % kv for kv in collections.Counter(r["batt_status"] for r in rows).items()))
print("focus: " + ", ".join("%s x%d" % kv for kv in collections.Counter(r["focus"] or "-" for r in rows).items()))
tot = collections.Counter(); 
for r in rows:
    for e in filter(None, r["top5"].split("|")):
        c, _, v = e.rpartition(":"); tot[c] += float(v)
print("top processes (mean % of one core): " + ", ".join("%s %.1f" % (c, v / len(rows)) for c, v in tot.most_common(5)))
PY
