"""Turns a run directory into per-step metrics and verdicts. Pure Python 3.9+.

Run directory (written by device/runner.py, or by selftest.py):
  meta.json      run facts (launcher app id, capture size, probe on/off, journeys as run)
  events.jsonl   {"t": CLOCK_MONOTONIC ns, "type": ...} inputs, steps, probe lines, focus, crashes
  frames.csv     one row per captured frame (jlib.frames.FRAMES_HEADER)
  thumbs.bin     the frames as W x H gray, for flash/stale checks and filmstrips
  power.csv      t_ns,socket_w,cpu_w,gfx_w,soc_w,gfx_busy,gfx_mhz
"""
from __future__ import annotations

import bisect
import json
import os
import re

from . import frames as F

QUIET_MS = 300        # no change for this long = settled
TRANSIENT_MS = 120    # a flash is gone again within this long
FLASH_PIX = 24        # tools/journeys/flash: luma step that counts as a changed pixel
FLASH_MIN = 0.001     # share of the screen a flash must change to count at all ...
FLASH_FAIL = 0.005    # ... and to fail (smaller ones are warnings)
HITCH_FACTOR = 2.5    # a gap between two moving frames this many nominal intervals long is a hitch


def load_events(path: str) -> list:
    out = []
    if os.path.exists(path):
        with open(path) as f:
            for line in f:
                line = line.strip()
                if line:
                    try:
                        out.append(json.loads(line))
                    except ValueError:
                        pass
    out.sort(key=lambda e: e.get("t", 0))
    return out


def load_power(path: str) -> list:
    rows = []
    if not os.path.exists(path):
        return rows
    with open(path) as f:
        head = f.readline().strip().split(",")
        for line in f:
            p = line.strip().split(",")
            if len(p) != len(head):
                continue
            r = {}
            for k, v in zip(head, p):
                try:
                    r[k] = float(v) if v != "" else None
                except ValueError:
                    r[k] = None
            rows.append(r)
    return rows


class Run:
    def __init__(self, d: str):
        self.dir = d
        self.meta = json.load(open(os.path.join(d, "meta.json"))) if os.path.exists(os.path.join(d, "meta.json")) else {}
        self.events = load_events(os.path.join(d, "events.jsonl"))
        fp = os.path.join(d, "frames.csv")
        self.frames = F.read_frames(fp) if os.path.exists(fp) else []
        self.ft = [f[1] for f in self.frames]
        cap = self.meta.get("capture") or {}
        self.thumbs = F.Thumbs(os.path.join(d, "thumbs.bin"), cap.get("w", F.W), cap.get("h", F.H))
        self.power = load_power(os.path.join(d, "power.csv"))
        self.pt = [r.get("t_ns") or 0 for r in self.power]
        self.launcher_app = str(self.meta.get("launcher_app") or "")
        self.nominal_ms = self._nominal()

    def _nominal(self) -> float:
        iv = []
        for a, b in zip(self.frames, self.frames[1:]):
            if F.is_changed(b[6], b[7]) and F.is_changed(a[6], a[7]):
                iv.append((b[1] - a[1]) / 1e6)
        iv = sorted(x for x in iv if x > 0)
        if len(iv) < 10:
            return 1000.0 / 60
        return max(4.0, iv[len(iv) // 2])

    def frames_between(self, a: int, b: int) -> tuple:
        return bisect.bisect_right(self.ft, a), bisect.bisect_right(self.ft, b)

    def power_between(self, a: int, b: int) -> dict:
        i, j = bisect.bisect_left(self.pt, a), bisect.bisect_right(self.pt, b)
        rows = self.power[i:j]
        out = {}
        for k in ("socket_w", "cpu_w", "gfx_w"):
            v = [r[k] for r in rows if r.get(k) is not None]
            if v:
                out[k] = round(sum(v) / len(v), 2)
                out[k + "_max"] = round(max(v), 2)
        return out


def _ms(a, b):
    return None if a is None or b is None else round((b - a) / 1e6, 1)


def frame_faults(run: Run, i0: int, i1: int, base_idx: int | None, end_idx: int | None) -> list:
    """Black/blank runs, A-B-A transients (flash or stale) between frame indexes [i0, i1)."""
    fr = run.frames
    out = []
    base = fr[base_idx] if base_idx is not None and base_idx >= 0 else None
    base_black = base is not None and F.is_black(base[3], base[5])
    base_blank = base is not None and F.is_blank(base[3], base[4], base[5])

    def runs(pred):
        cur = []
        for i in range(i0, i1):
            if pred(fr[i]):
                cur.append(i)
            elif cur:
                yield cur
                cur = []
        if cur:
            yield cur

    for kind, pred, exempt in (
        ("black", lambda f: F.is_black(f[3], f[5]), base_black),
        ("blank", lambda f: F.is_blank(f[3], f[4], f[5]), base_blank),
    ):
        for r in runs(pred):
            # A run that just continues the state before the input is not a fault.
            if exempt and r[0] == i0:
                continue
            out.append({"kind": kind, "first": r[0], "last": r[-1], "frames": len(r),
                        "ms": _ms(fr[r[0]][1], fr[r[-1] + 1][1] if r[-1] + 1 < len(fr) else fr[r[-1]][1]) or 0.0,
                        "t": fr[r[0]][1]})
    # Flash: the tools/journeys/flash criterion (the GPU-mode flash seen on the SuiPlay, 2026-10-27):
    # a run of 1 or 2 frames that differs from the frames on both sides while those two agree,
    # measured as the share of pixels whose luma moved more than FLASH_PIX. Here it must also be gone
    # again within TRANSIENT_MS, since gamescope only sends a frame when something changed.
    if run.thumbs.n:
        b0 = run.thumbs[base_idx] if base is not None else b""
        covered = set()
        for k in (1, 2):
            for i in range(max(i0, 1), min(i1, len(fr) - k)):
                if i in covered or fr[i][7] < FLASH_MIN:
                    continue
                if (fr[i + k][1] - fr[i][1]) / 1e6 > TRANSIENT_MS:
                    continue
                a, b = run.thumbs[i - 1], run.thumbs[i + k]
                mids = [run.thumbs[j] for j in range(i, i + k)]
                m = min([F.chg_frac(x, a, FLASH_PIX) for x in mids] + [F.chg_frac(x, b, FLASH_PIX) for x in mids])
                if m <= FLASH_MIN or F.chg_frac(a, b, FLASH_PIX) >= 0.3 * m:
                    continue
                if any(F.is_black(fr[j][3], fr[j][5]) or F.is_blank(fr[j][3], fr[j][4], fr[j][5]) for j in range(i, i + k)):
                    continue  # reported as black/blank
                covered.update(range(i, i + k))
                stale = bool(b0) and F.chg_frac(a, b0, FLASH_PIX) > 3 * FLASH_MIN and \
                    any(F.chg_frac(x, b0, FLASH_PIX) < 0.3 * m for x in mids)
                out.append({"kind": "stale" if stale else "flash", "first": i, "last": i + k - 1, "frames": k,
                            "ms": _ms(fr[i][1], fr[i + k][1]) or 0.0, "t": fr[i][1], "area": round(m, 4),
                            "minor": m < FLASH_FAIL})
    else:
        # Without thumbnails: a luminance spike that comes straight back.
        for i in range(max(i0, 1), min(i1, len(fr) - 1)):
            a, b, c = fr[i - 1][3], fr[i][3], fr[i + 1][3]
            if abs(a - c) < 6 and abs(b - a) > 25 and abs(b - c) > 25:
                out.append({"kind": "flash", "first": i, "last": i, "frames": 1, "ms": _ms(fr[i][1], fr[i + 1][1]), "t": fr[i][1]})
    out.sort(key=lambda x: x["first"])
    return out


def motion(run: Run, t0: int, t_end: int, inputs=None) -> dict:
    """Response, settle and hitch figures for frames after an input at t0. `inputs` are the times of
    later presses in the same step (repeats): a pause that contains a press is not a hitch."""
    inputs = sorted(inputs or [])

    def press_between(a, b):
        k = bisect.bisect_right(inputs, a)
        return k < len(inputs) and inputs[k] <= b
    fr = run.frames
    i0, i1 = run.frames_between(t0, t_end)
    pre0, _ = run.frames_between(t0 - 500_000_000, t0)
    pre = fr[pre0:i0]
    noisy = sum(1 for f in pre if F.is_changed(f[6], f[7]) and f[1] > t0 - 300_000_000) >= 3
    chg_min = F.CHG_MIN
    if noisy:
        cs = sorted(f[7] for f in pre)
        chg_min = max(F.CHG_MIN, 2 * cs[int(len(cs) * 0.9)] if cs else F.CHG_MIN)
    changed = [i for i in range(i0, i1) if fr[i][7] >= chg_min or (not noisy and fr[i][6] >= F.DIFF_MIN)]
    out = {"frames": i1 - i0, "noisy": noisy, "respond_ms": None, "settle_ms": None, "max_gap_ms": None,
           "hitches": 0, "frame_p95_ms": None, "repeats": 0}
    if not changed:
        return out
    out["respond_ms"] = _ms(t0, fr[changed[0]][1])
    # Settled: the last change followed by QUIET_MS without one (or the end of the step).
    last = changed[0]
    for a, b in zip(changed, changed[1:]):
        if (fr[b][1] - fr[a][1]) / 1e6 > QUIET_MS and not press_between(fr[a][1], fr[b][1]):
            break
        last = b
    out["settle_ms"] = _ms(t0, fr[last][1])
    moving = [i for i in changed if i <= last]
    gaps = [(fr[b][1] - fr[a][1]) / 1e6 for a, b in zip(moving, moving[1:]) if not press_between(fr[a][1], fr[b][1])]
    if gaps:
        gs = sorted(gaps)
        out["max_gap_ms"] = round(gs[-1], 1)
        out["frame_p95_ms"] = round(gs[min(len(gs) - 1, int(len(gs) * 0.95))], 1)
        out["hitches"] = sum(1 for g in gaps if g > HITCH_FACTOR * run.nominal_ms)
    # Repeated frames inside the motion: the compositor delivered the same picture again.
    out["repeats"] = sum(1 for i in range(changed[0] + 1, last) if fr[i][6] == 0.0)
    out["_first"] = changed[0]
    out["_last"] = last
    return out


def _probe_state(events: list, t: int) -> dict:
    st = {"screen": None, "focus": None, "ingame": None}
    for e in events:
        if e["t"] > t:
            break
        if e["type"] == "probe":
            txt = e.get("text", "")
            for k in ("screen", "focus", "ingame"):
                if txt.startswith(k + " "):
                    st[k] = txt[len(k) + 1:]
    return st


def parse_screen(s: str | None) -> dict:
    if not s:
        return {}
    return dict(kv.split("=", 1) for kv in s.split() if "=" in kv)


def screen_matches(want: dict, have: dict) -> bool:
    for k, v in want.items():
        hv = have.get(k)
        if hv is None:
            return False
        if k == "overlay" and v != "-" and ":" not in str(v):
            hv = hv.split(":", 1)[0]
        if str(v) != hv:
            return False
    return True


def focus_app_at(events: list, t: int):
    v = None
    for e in events:
        if e["t"] > t:
            break
        if e["type"] == "focus_app":
            v = str(e.get("app"))
    return v


def app_matches(want: str, have, launcher: str) -> bool:
    if have is None:
        return False
    if want == "launcher":
        return have == launcher
    if want == "not_launcher":
        return have != launcher and have not in ("0", "")
    return str(want) == have


def analyze(d: str) -> dict:
    run = Run(d)
    ev = run.events
    probe_on = any(e["type"] == "probe" for e in ev)
    journeys = {j["name"]: j for j in run.meta.get("journeys", [])}
    res = {"meta": run.meta, "nominal_frame_ms": round(run.nominal_ms, 2), "probe": probe_on, "journeys": [],
           "frames": len(run.frames), "crashes": [e for e in ev if e["type"] == "crash"]}
    cur = None
    steps = {}
    for e in ev:
        t = e["type"]
        if t == "journey_start":
            cur = {"name": e["journey"], "title": journeys.get(e["journey"], {}).get("title", e["journey"]), "t0": e["t"],
                   "status": "running", "reason": "", "steps": [], "crashes": []}
            res["journeys"].append(cur)
        elif t == "journey_end" and cur:
            cur["t1"] = e["t"]
            cur["status"] = e.get("status", "completed")
            cur["reason"] = e.get("reason", "")
            cur["cleanup"] = e.get("cleanup")
        elif t == "step_start" and cur:
            s = {"idx": e["step"], "name": e.get("name", ""), "kind": e.get("kind", ""), "t0": e["t"], "inputs": [],
                 "live": "", "live_reason": ""}
            cur["steps"].append(s)
            steps[(cur["name"], e["step"])] = s
        elif t == "step_end" and cur:
            s = steps.get((cur["name"], e["step"]))
            if s:
                s["t1"] = e["t"]
                s["live"] = e.get("status", "ok")
                s["live_reason"] = e.get("reason", "")
                s["live_info"] = e.get("info") or {}
        elif t == "input" and cur:
            s = steps.get((cur["name"], e.get("step")))
            if s is not None:
                s["inputs"].append(e)
        elif t == "crash" and cur:
            cur["crashes"].append(e)
    for j in res["journeys"]:
        spec = journeys.get(j["name"], {})
        specs = spec.get("steps", [])
        bad = []
        for s in j["steps"]:
            st = specs[s["idx"]] if s["idx"] < len(specs) else {}
            judge_step(run, s, st, probe_on)
            if s["verdict"] == "fail":
                bad.append(s)
        if j["crashes"]:
            j["verdict"] = "fail"
        elif j["status"] in ("timeout", "aborted", "failed", "error"):
            j["verdict"] = "fail"
        elif j["status"] == "skipped":
            j["verdict"] = "skip"
        elif bad:
            j["verdict"] = "fail"
        elif any(s["verdict"] == "warn" for s in j["steps"]):
            j["verdict"] = "warn"
        else:
            j["verdict"] = "pass"
    res["worst"] = worst(res)
    res["summary"] = {v: sum(1 for j in res["journeys"] if j["verdict"] == v) for v in ("pass", "warn", "fail", "skip")}
    res["steps_summary"] = {v: sum(1 for j in res["journeys"] for s in j["steps"] if s["verdict"] == v) for v in ("pass", "warn", "fail", "skip")}
    return res


def judge_step(run: Run, s: dict, spec: dict, probe_on: bool):
    exp = spec.get("expect") or {}
    reasons, warns = [], []
    t_end = s.get("t1") or (s["t0"] + 5_000_000_000)
    downs = [e for e in s["inputs"] if e.get("action") == "down"]
    t_in = downs[0]["t"] if downs else None
    if s["kind"] == "sleep_wake":
        t_in = (s.get("live_info") or {}).get("resumed_ns") or t_in
    m = {}
    if s.get("live") == "failed":
        reasons.append(s.get("live_reason") or "failed on the device")
    elif s.get("live") == "skipped":
        s.update(verdict="skip", reasons=[s.get("live_reason", "")], warns=[], metrics={})
        return
    if t_in is not None and run.frames:
        mo = motion(run, t_in, t_end, [e["t"] for e in downs[1:]])
        m.update({k: v for k, v in mo.items() if not k.startswith("_")})
        i0, i1 = run.frames_between(t_in, t_end)
        base = i0 - 1 if i0 > 0 else None
        faults = frame_faults(run, i0, i1, base, i1 - 1 if i1 > i0 else None)
        m["faults"] = faults
        banned = set(exp.get("no", []))
        for f in faults:
            area = f" {round(f['area'] * 100, 1)}% of the screen" if f.get("area") is not None else ""
            line = f"{f['kind']} {f['frames']} frame{'s' if f['frames'] != 1 else ''} ({f['ms']} ms{area}) at +{_ms(t_in, f['t'])} ms"
            (reasons if f["kind"] in banned and not f.get("minor") else warns).append(line)
        if "respond_ms" in exp:
            if mo["respond_ms"] is None:
                reasons.append(f"no change on screen (wanted within {exp['respond_ms']} ms)")
            elif mo["respond_ms"] > exp["respond_ms"]:
                reasons.append(f"responded in {mo['respond_ms']} ms (wanted {exp['respond_ms']})")
            if mo["noisy"]:
                warns.append("screen was already animating; response time is approximate")
        if "settle_ms" in exp and mo["settle_ms"] is not None and mo["settle_ms"] > exp["settle_ms"]:
            reasons.append(f"settled in {mo['settle_ms']} ms (wanted {exp['settle_ms']})")
        if exp.get("change") is False and mo["respond_ms"] is not None:
            reasons.append("the screen changed (wanted no change)")
        if exp.get("change") is True and mo["respond_ms"] is None:
            reasons.append("nothing changed on screen")
        hm = exp.get("hitch_ms")
        if hm and mo["max_gap_ms"] and mo["max_gap_ms"] > hm:
            reasons.append(f"hitch: {mo['max_gap_ms']} ms between two moving frames (limit {hm})")
        elif mo["hitches"]:
            warns.append(f"{mo['hitches']} late frame{'s' if mo['hitches'] > 1 else ''} (gap over {HITCH_FACTOR}x {round(run.nominal_ms, 1)} ms)")
        if mo["repeats"]:
            warns.append(f"{mo['repeats']} repeated frame{'s' if mo['repeats'] > 1 else ''} during motion")
        m["clip"] = [t_in - 700_000_000, min(t_end, t_in + 6_000_000_000) + 500_000_000]
        m["film"] = _film(run, mo, faults)
    elif t_in is not None and not run.frames:
        warns.append("no capture frames")
    # Probe: what the launcher says is on screen.
    if exp.get("screen") or exp.get("focus"):
        if not probe_on:
            warns.append("screen/focus not checked: launcher probe off")
        else:
            st = _probe_state(run.events, t_end)
            if exp.get("screen"):
                have = parse_screen(st["screen"])
                if not screen_matches(exp["screen"], have):
                    reasons.append(f"screen is {st['screen']} (wanted {exp['screen']})")
                elif t_in is not None:
                    for e in run.events:
                        if e["type"] == "probe" and t_in <= e["t"] <= t_end and e.get("text", "").startswith("screen ") \
                                and screen_matches(exp["screen"], parse_screen(e["text"][7:])):
                            m["screen_ms"] = _ms(t_in, e["t"])
                            break
                    for e in run.events:
                        if e["type"] == "probe" and e.get("text") == "painted" and e["t"] >= t_in and e["t"] <= t_end:
                            m["painted_ms"] = _ms(t_in, e["t"])
                            break
            if exp.get("focus"):
                if not st["focus"] or not re.search(exp["focus"], st["focus"]):
                    reasons.append(f"focus is {st['focus']} (wanted /{exp['focus']}/)")
    if exp.get("focus_app"):
        have = focus_app_at(run.events, t_end)
        if not app_matches(str(exp["focus_app"]), have, run.launcher_app):
            reasons.append(f"focused app is {have} (wanted {exp['focus_app']})")
    fa = [e for e in run.events if e["type"] == "focus_app" and s["t0"] <= e["t"] <= t_end]
    m["focus_changes"] = [{"t_ms": _ms(t_in or s["t0"], e["t"]), "app": e.get("app")} for e in fa]
    if exp.get("max_focus_changes") is not None and len(fa) > exp["max_focus_changes"]:
        reasons.append(f"{len(fa)} focus changes (wanted at most {exp['max_focus_changes']})")
    crashes = [e for e in run.events if e["type"] == "crash" and s["t0"] <= e["t"] <= t_end + 2_000_000_000]
    for c in crashes:
        reasons.append(f"crash: {c.get('source')} {c.get('detail', '')}".strip())
    pw = run.power_between(t_in or s["t0"], t_end)
    m["power"] = pw
    if exp.get("power_max_w") and pw.get("socket_w") and pw["socket_w"] > exp["power_max_w"]:
        reasons.append(f"APU {pw['socket_w']} W (limit {exp['power_max_w']})")
    if exp.get("fps_min") and m.get("frames") and t_in is not None:
        secs = max(0.001, (t_end - t_in) / 1e9)
        fps = m["frames"] / secs
        m["capture_fps"] = round(fps, 1)
        if fps < exp["fps_min"]:
            reasons.append(f"{round(fps, 1)} fps captured (wanted {exp['fps_min']})")
    if downs:
        dl = [e for e in downs if e.get("t_evdev")]
        if dl:
            m["inject_ms"] = round(max((e["t_evdev"] - e["t_call"]) / 1e6 for e in dl), 2)
    s["metrics"] = m
    s["reasons"] = reasons
    s["warns"] = warns
    s["verdict"] = "fail" if reasons else ("warn" if warns else "pass")


def _film(run: Run, mo: dict, faults: list) -> list:
    """Frame indexes for a filmstrip: around the first fault, else the response."""
    if faults:
        c = faults[0]["first"]
    elif mo.get("_first") is not None:
        c = mo["_first"]
    else:
        return []
    return [i for i in range(c - 3, c + 6) if 0 <= i < len(run.frames)]


def worst(res: dict) -> dict:
    rows = [(j["name"], s) for j in res["journeys"] for s in j["steps"] if s.get("metrics")]

    def top(key, n=10, rev=True):
        r = [(j, s) for j, s in rows if s["metrics"].get(key) is not None]
        r.sort(key=lambda x: x[1]["metrics"][key], reverse=rev)
        return [{"journey": j, "step": s["name"], "idx": s["idx"], key: s["metrics"][key]} for j, s in r[:n]]

    faults = [{"journey": j, "step": s["name"], "idx": s["idx"], **{k: v for k, v in f.items() if k in ("kind", "frames", "ms")}}
              for j, s in rows for f in s["metrics"].get("faults", [])]
    power = [(j, s) for j, s in rows if s["metrics"].get("power", {}).get("socket_w") is not None]
    power.sort(key=lambda x: x[1]["metrics"]["power"]["socket_w"], reverse=True)
    return {
        "respond_ms": top("respond_ms"),
        "settle_ms": top("settle_ms"),
        "max_gap_ms": top("max_gap_ms"),
        "faults": faults,
        "power": [{"journey": j, "step": s["name"], "idx": s["idx"], **s["metrics"]["power"]} for j, s in power[:10]],
    }


def scan(d: str) -> dict:
    """Whole-capture scan with no journey (a plain video or a capture made by hand)."""
    run = Run(d)
    n = len(run.frames)
    faults = frame_faults(run, 0, n, None, None) if n else []
    t0 = run.frames[0][1] if n else 0
    for f in faults:
        f["at_s"] = round((f["t"] - t0) / 1e9, 3)
    dur = (run.frames[-1][1] - t0) / 1e9 if n > 1 else 0
    return {"frames": n, "seconds": round(dur, 2), "nominal_frame_ms": round(run.nominal_ms, 2), "faults": faults}
