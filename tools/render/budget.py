#!/usr/bin/env python3
"""Frame-budget gates over a Rare Render stats log (RR_STATS=<file>, format in engine/src/stats.rs).

  budget.py <rr-stats.log> [...]          CI: structural gates fail, timing gates only warn
  budget.py --device <rr-stats.log> [...] on the SuiPlay: every gate fails the run
  budget.py --json ...                    one JSON object per log on stdout

Gates:
  cpu      engine CPU per frame (latch to paint) p99 under 4 ms
  layout   no relayout per animation frame: after the frame that took an input, the frames that
           follow back to back (under 50 ms apart) are the animation; layouts on two of them in a
           row fail. A single layout inside the window (a state change landing mid-animation, such
           as Home's stage following focus after 140 ms) is listed as deferred-layout, a warning
  per-input at most one layout frame per input window
  hitch    no frame interval over 25 ms inside an animation window (focus moves)

An input window starts at a frame that latched input (its `inputs` field, or an `in` line handled
inside it). A log without input (a golden shot whose RR_SCRIPT bypasses the input queue) falls
back to idle gaps: the first frame after more than 50 ms of quiet opens a window. The startup
frames before the first window are not gated (first layout, image arrivals).
A 12th field on `f` lines, when the engine writes one, is 1 while something animates; with it
an animation window is exactly those frames.
"""
from __future__ import annotations

import json
import sys

CPU_P99_MS = 4.0
HITCH_MS = 25.0
GAP_US = 50_000


def pct(v, p):
    if not v:
        return None
    s = sorted(v)
    k = (len(s) - 1) * p / 100
    lo = int(k)
    hi = min(lo + 1, len(s) - 1)
    return round(s[lo] + (s[hi] - s[lo]) * (k - lo), 3)


def parse(path):
    frames, ins = [], []
    for line in open(path):
        p = line.split()
        if not p:
            continue
        if p[0] == "f" and len(p) >= 12:
            r = [int(x) for x in p[1:]]
            frames.append({"n": r[0], "t0": r[1], "t1": r[2], "cpu_us": r[3], "layout": r[8], "inputs": r[9], "ivl_us": r[10],
                           "anim": r[11] if len(r) > 11 else None})
        elif p[0] == "in" and len(p) >= 5:
            ins.append({"t_k": int(p[1]), "button": p[2], "repeat": int(p[3]), "t_h": int(p[4])})
    return frames, ins


def windows(frames, ins):
    """Index lists of animation windows; the first index of each is the frame that took the input."""
    starts = set()
    method = "inputs"
    for i, f in enumerate(frames):
        if f["inputs"] > 0:
            starts.add(i)
    for e in ins:
        i = next((i for i, f in enumerate(frames) if f["t1"] >= e["t_h"]), None)
        if i is not None:
            starts.add(i)
    if not starts:
        method = "gaps"
        for i, f in enumerate(frames):
            if i > 0 and f["ivl_us"] > GAP_US:
                starts.add(i)
    out = []
    for s in sorted(starts):
        w = [s]
        j = s + 1
        while j < len(frames) and j not in starts:
            f = frames[j]
            if f["anim"] is not None:
                if not f["anim"]:
                    break
            elif f["ivl_us"] > GAP_US:
                break
            w.append(j)
            j += 1
        out.append(w)
    return out, method


def check(path, device=False):
    frames, ins = parse(path)
    res = {"log": path, "frames": len(frames), "inputs": len(ins), "gates": {}, "ok": True}
    if not frames:
        res["gates"]["frames"] = {"ok": False, "detail": "no f lines (RR_STATS unset or the program never drew)", "gating": True}
        res["ok"] = False
        return res
    wins, method = windows(frames, ins)
    res["window_method"] = method
    res["windows"] = len(wins)
    gated = [i for w in wins for i in w]
    cpu = [frames[i]["cpu_us"] / 1000 for i in gated] or [f["cpu_us"] / 1000 for f in frames]
    p99 = pct(cpu, 99)
    res["cpu_ms"] = {"p50": pct(cpu, 50), "p99": p99, "max": max(cpu)}
    res["gates"]["cpu"] = {"ok": p99 is not None and p99 < CPU_P99_MS, "detail": f"p99 {p99} ms (limit {CPU_P99_MS})", "gating": device}
    # A layout on two animation frames in a row is a relayout per frame (the thing that must never
    # happen); a single one inside the window is a state change landing mid-animation (Home's
    # stage following focus after 140 ms): listed, not failed, until the engine logs why it laid out.
    per_frame, single = [], []
    for w in wins:
        for k, i in enumerate(w[1:], 1):
            if frames[i]["layout"]:
                (per_frame if frames[w[k - 1]]["layout"] and k > 1 or (k + 1 < len(w) and frames[w[k + 1]]["layout"])
                 else single).append(frames[i]["n"])
    res["gates"]["layout"] = {"ok": not per_frame, "gating": True,
                              "detail": f"layout on consecutive animation frames {per_frame[:10]}" if per_frame else "no relayout per animation frame"}
    res["gates"]["deferred-layout"] = {"ok": not single, "gating": False,
                                       "detail": f"{len(single)} single layouts inside animations (state changes mid-animation) at frames {single[:10]}" if single else "none"}
    multi = [frames[w[0]]["n"] for w in wins if sum(frames[i]["layout"] for i in w) > 1]
    res["gates"]["per-input"] = {"ok": not multi, "detail": f"more than one layout after the input at frames {multi[:10]}" if multi else "at most one layout per input", "gating": True}
    ivl = [frames[i]["ivl_us"] / 1000 for w in wins for i in w[1:]]
    over = [round(x, 1) for x in ivl if x > HITCH_MS]
    res["interval_ms"] = {"p50": pct(ivl, 50), "p99": pct(ivl, 99), "n": len(ivl)}
    res["gates"]["hitch"] = {"ok": not over, "detail": f"{len(over)} of {len(ivl)} intervals over {HITCH_MS:g} ms while moving {over[:8]}" if over else f"0 of {len(ivl)} intervals over {HITCH_MS:g} ms", "gating": device}
    if not wins:
        for g in ("layout", "deferred-layout", "per-input", "hitch"):
            res["gates"][g]["detail"] += " (no input window in the log)"
    res["ok"] = all(g["ok"] or not g["gating"] for g in res["gates"].values())
    return res


def main(argv):
    device = "--device" in argv
    as_json = "--json" in argv
    logs = [a for a in argv if not a.startswith("--")]
    if not logs:
        print(__doc__)
        return 2
    ok = True
    for p in logs:
        r = check(p, device)
        ok &= r["ok"]
        if as_json:
            print(json.dumps(r))
            continue
        print(f"{p}: {'PASS' if r['ok'] else 'FAIL'} ({r['frames']} frames, {r.get('windows', 0)} windows by {r.get('window_method', '-')})")
        for name, g in r["gates"].items():
            mark = "ok  " if g["ok"] else ("FAIL" if g["gating"] else "warn")
            print(f"  {mark} {name}: {g['detail']}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
