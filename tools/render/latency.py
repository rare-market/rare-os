#!/usr/bin/env python3
"""Mac side of tools/render/bench.py (needs numpy). Reads a capture pulled back from the device.

  latency.py press <dir> <label>   per press: input (uinput write, CLOCK_MONOTONIC) to the first
                                   composited frame that changed, and settle; frame intervals while
                                   the screen moves (p50/p95/p99, count over 25 ms)
  latency.py cold <dir> <label>    kill -> new process -> first non-black frame -> settled
  latency.py stats <rr-stats.log>  rare-render's own frame log: CPU per frame, intervals, input to
                                   submit

A frame is "changed" when more than 0.2 % of its pixels moved by more than 24 luma steps (640x400),
the same threshold as flash/analyze.py.
"""
import json, os, sys

import numpy as np

W, H = 640, 400


def load(d, label):
    raw = np.fromfile(os.path.join(d, f"{label}.raw"), np.uint8)
    n = raw.size // (W * H * 3)
    luma = raw[: n * W * H * 3].reshape(n, H, W, 3).astype(np.int16).mean(axis=3)
    pts = [int(round(float(x) * 1e9)) for x in open(os.path.join(d, f"{label}.pts")).read().split()][:n]
    return luma, pts


def changed(a, b):
    return float((np.abs(a - b) > 24).mean())


def pct(v, p):
    if not v:
        return None
    return round(float(np.percentile(np.array(v), p)), 2)


def press(d, label):
    luma, pts = load(d, label)
    presses = json.load(open(os.path.join(d, f"{label}.presses.json")))
    n = len(pts)
    diff = [0.0] + [changed(luma[i], luma[i - 1]) for i in range(1, n)]
    res, moving = [], []
    for k, p in enumerate(presses):
        t = p["t"]
        end = presses[k + 1]["t"] if k + 1 < len(presses) else t + 1_500_000_000
        base = max([i for i in range(n) if pts[i] <= t] or [0])
        first = next((i for i in range(base + 1, n) if pts[i] < end and changed(luma[i], luma[base]) > 0.002), None)
        mv = [i for i in range(base + 1, n) if pts[i] < end and diff[i] > 0.0005]
        if mv:
            moving += [(pts[i] - pts[i - 1]) / 1e6 for i in range(mv[0] + 1, mv[-1] + 1)]
        res.append({"step": p["step"], "latency_ms": round((pts[first] - t) / 1e6, 1) if first is not None else None,
                    "settle_ms": round((pts[mv[-1]] - t) / 1e6, 1) if mv else None, "moving_frames": len(mv)})
    lat = [r["latency_ms"] for r in res if r["latency_ms"] is not None]
    out = {"label": label, "frames": n, "presses": len(presses), "answered": len(lat),
           "latency_ms": {"p50": pct(lat, 50), "p95": pct(lat, 95), "p99": pct(lat, 99), "min": min(lat) if lat else None, "max": max(lat) if lat else None},
           "settle_ms_p50": pct([r["settle_ms"] for r in res if r["settle_ms"]], 50),
           "interval_moving_ms": {"p50": pct(moving, 50), "p95": pct(moving, 95), "p99": pct(moving, 99), "n": len(moving),
                                  "over_25ms": int(sum(1 for x in moving if x > 25))},
           "per_press": res}
    json.dump(out, open(os.path.join(d, f"{label}.result.json"), "w"), indent=1)
    print(json.dumps({k: v for k, v in out.items() if k != "per_press"}))


def cold(d, label):
    luma, pts = load(d, label)
    c = json.load(open(os.path.join(d, f"{label}.cold.json")))
    t_new, t_kill = c["t_new"], c["t_kill"]
    n = len(pts)
    after = [i for i in range(n) if pts[i] > (t_new or t_kill)]
    first = next((i for i in after if (luma[i] > 20).mean() > 0.05), None)
    mv = [i for i in after if changed(luma[i], luma[i - 1]) > 0.0005]
    last = mv[-1] if mv else None
    black = [i for i in after if (luma[i] < 12).mean() > 0.9]
    out = {"label": label, "kill_to_new_process_ms": round((t_new - t_kill) / 1e6, 1) if t_new else None,
           "new_process_to_first_frame_ms": round((pts[first] - t_new) / 1e6, 1) if first is not None and t_new else None,
           "new_process_to_settled_ms": round((pts[last] - t_new) / 1e6, 1) if last is not None and t_new else None,
           "black_frames": len(black), "frames": n}
    print(json.dumps(out))


def stats(path):
    f, ins, first, mem = [], [], None, []
    for l in open(path):
        p = l.split()
        if not p:
            continue
        if p[0] == "f" and len(p) >= 12:
            f.append([int(x) for x in p[1:12]])
        elif p[0] == "in":
            ins.append((int(p[1]), p[2], int(p[3]), int(p[4])))
        elif p[0] == "first":
            first = float(p[2])
        elif p[0] == "mem":
            mem.append((int(p[2]), int(p[3]), int(p[4])))
    cpu = [r[3] / 1000 for r in f]
    acq = [r[4] / 1000 for r in f]
    sub = [r[5] / 1000 for r in f]
    ivl = [r[10] / 1000 for r in f if 0 < r[10] < 100_000]
    # input (kernel stamp) to the submit of the first frame that latched it
    lat = []
    for t_k, name, rep, t_h in ins:
        fr = next((r for r in f if r[1] <= t_h <= r[2] or r[1] >= t_h), None)
        if fr:
            lat.append((fr[2] - t_k) / 1e6)
    out = {"frames": len(f), "first_frame_ms": first,
           "cpu_ms_per_frame": {"p50": pct(cpu, 50), "p95": pct(cpu, 95), "p99": pct(cpu, 99), "max": max(cpu) if cpu else None},
           "acquire_wait_ms": {"p50": pct(acq, 50), "p95": pct(acq, 95)}, "submit_ms": {"p50": pct(sub, 50), "p99": pct(sub, 99)},
           "interval_ms": {"p50": pct(ivl, 50), "p95": pct(ivl, 95), "p99": pct(ivl, 99), "n": len(ivl)},
           "input_to_submit_ms": {"p50": pct(lat, 50), "p95": pct(lat, 95), "n": len(lat)},
           "prims_max": max((r[6] for r in f), default=0), "layouts": sum(r[8] for r in f),
           "rss_mib_last": round(mem[-1][0] / 1024, 1) if mem else None, "textures_mib_last": round(mem[-1][1] / 2**20, 1) if mem else None}
    print(json.dumps(out))


if __name__ == "__main__":
    a = sys.argv[1:]
    {"press": lambda: press(a[1], a[2]), "cold": lambda: cold(a[1], a[2]), "stats": lambda: stats(a[1])}[a[0]]()
