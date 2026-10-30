#!/usr/bin/env python3
"""Proves the analysis on synthetic captures, no device needed.
  python3 tools/journeys/selftest.py [--keep DIR] [--quick]

1. Synthetic runs: scripted frame timelines with known black, blank, flash, stale frames, hitches
   and late responses, plus clean crossfades, slides and repeated presses that must NOT be flagged.
2. Video path (needs ffmpeg or `pip install imageio-ffmpeg`): the same faults encoded into an mp4,
   decoded back and found at the right frame.
3. Runner end to end with the simulated launcher (--mock), faults injected on chosen presses;
   physical input, and the input guard killed or stalled mid-run, abort with no further input.
3e. InputPlumber timing out mid-run switches to the virtual pad; 3f. cleanup never presses Guide;
   3g. the virtual pad is destroyed only after every input went back to neutral and was read.
3c. The flash tools' idle guard (flash/beat.py, guard-run.sh, eikey.py): killed, stalled, its
   supervisor killed, or never started, the keys stop within the heartbeat limit and the armed
   session is restored.
4. Any captures in bench/ (*.mp4, *.mkv, *.webm, and tools/journeys/flash <label>.raw + .pts) or
   $JOURNEY_CAPTURES (paths separated by ':'): scanned and listed.
Exit 0 when every assertion holds.
"""
from __future__ import annotations

import glob
import json
import os
import random
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from jlib import analyze as A  # noqa: E402
from jlib import frames as F  # noqa: E402
from jlib import report as R  # noqa: E402
from jlib import spec  # noqa: E402

FAILS = []


def check(cond, what):
    print(("  ok    " if cond else "  FAIL  ") + what)
    if not cond:
        FAILS.append(what)


# ---- synthetic scenes

def scene(seed: int) -> bytes:
    """A launcher-like picture: dark backdrop, a row of tiles, a focus ring (seeded)."""
    r = random.Random(seed)
    px = bytearray(F.W * F.H)
    bg = r.randrange(25, 60)
    for i in range(len(px)):
        px[i] = bg
    for t in range(5):
        x0, y0 = 4 + t * 18, 18 + r.randrange(0, 6)
        shade = r.randrange(70, 200)
        for y in range(y0, y0 + 22):
            for x in range(x0, x0 + 15):
                px[y * F.W + x] = shade
    f = r.randrange(0, 5)
    for y in (16, 41):
        for x in range(2 + f * 18, 21 + f * 18):
            px[y * F.W + x] = 250
    return bytes(px)


def blend(a, b, k):
    return bytes(int(x + (y - x) * k) for x, y in zip(a, b))


def slide(a, b, k):
    """b slides in from the right over a."""
    cut = int(F.W * (1 - k))
    rows = []
    for y in range(F.H):
        ra, rb = a[y * F.W:(y + 1) * F.W], b[y * F.W:(y + 1) * F.W]
        rows.append(ra[:cut] + rb[:F.W - cut])
    return b"".join(rows)


class Timeline:
    """Builds a run directory: frames at chosen times, inputs, steps, probe lines."""

    def __init__(self, d):
        self.d = d
        os.makedirs(d, exist_ok=True)
        self.t = 1_000_000_000_000
        self.frames = []
        self.events = []
        self.cur = scene(0)
        self.steps = []
        self.idx = -1

    def frame(self, px, dt_ms=16.667):
        self.t += int(dt_ms * 1e6)
        self.frames.append((self.t, px))
        self.cur = px

    def idle(self, ms):
        self.t += int(ms * 1e6)

    def ev(self, type_, **kw):
        e = {"t": kw.pop("t", self.t), "type": type_}
        e.update(kw)
        self.events.append(e)
        self.events.sort(key=lambda x: x["t"])

    def step(self, name, expect=None, kind="press"):
        if self.idx >= 0:
            self.ev("step_end", journey="synthetic", step=self.idx, status="ok")
        self.idx += 1
        st = {"name": name, "kind": kind, "expect": dict(spec.DEFAULT_EXPECT, **(expect or {}))}
        self.steps.append(st)
        self.idle(50)
        self.ev("step_start", journey="synthetic", step=self.idx, name=name, kind=kind)

    def press(self, button="rb", latency_ms=40):
        """A press; the next frame() lands latency_ms after the button went down."""
        self.idle(5)
        down = self.t
        self.ev("input", journey="synthetic", step=self.idx, button=button, action="down", t_call=down - 300_000)
        self.ev("input", journey="synthetic", step=self.idx, button=button, action="up", t_call=down + 80_000_000, t=down + 80_000_000)
        self.t = down + int((latency_ms - 16.667) * 1e6)

    def anim(self, to, n=8, how=blend, dt=16.667):
        a = self.cur
        for k in range(1, n + 1):
            self.frame(how(a, to, k / n), dt)

    def write(self):
        self.ev("step_end", journey="synthetic", step=self.idx, status="ok")
        self.idle(400)
        self.ev("journey_end", journey="synthetic", status="completed")
        ev0 = {"t": self.events[0]["t"] - 1000, "type": "journey_start", "journey": "synthetic"}
        meta = {"run_id": os.path.basename(self.d), "launcher_app": "1380012613", "capture": {"pipeline_name": "synthetic", "w": F.W, "h": F.H},
                "journeys": [{"name": "synthetic", "title": "synthetic faults", "steps": self.steps}]}
        json.dump(meta, open(os.path.join(self.d, "meta.json"), "w"))
        with open(os.path.join(self.d, "events.jsonl"), "w") as f:
            for e in [ev0] + self.events:
                f.write(json.dumps(e) + "\n")
        m = F.Metric()
        with open(os.path.join(self.d, "thumbs.bin"), "wb") as tb, open(os.path.join(self.d, "frames.csv"), "w") as fc:
            fc.write(F.FRAMES_HEADER)
            for i, (t, px) in enumerate(self.frames):
                tb.write(px)
                fc.write(F.frame_row(i, t, t, m.feed(px)))


def build_synthetic(d):
    tl = Timeline(d)
    tl.frame(scene(0))
    tl.idle(500)
    # 0 clean crossfade, fast
    tl.step("clean crossfade", {"respond_ms": 150, "settle_ms": 400})
    tl.press(latency_ms=40)
    tl.anim(scene(1))
    tl.idle(600)
    # 1 clean slide
    tl.step("clean slide", {"respond_ms": 150})
    tl.press()
    tl.anim(scene(2), n=10, how=slide)
    tl.idle(600)
    # 2 black frames in the middle of the transition
    tl.step("black mid-transition")
    tl.press()
    tl.anim(blend(tl.cur, scene(3), 0.4), n=3)
    tl.frame(bytes(F.W * F.H))
    tl.frame(bytes(F.W * F.H))
    tl.anim(scene(3), n=4)
    tl.idle(600)
    # 3 white flash (blank) between screens
    tl.step("white flash")
    tl.press()
    tl.frame(b"\xf0" * (F.W * F.H))
    tl.anim(scene(4), n=4)
    tl.idle(600)
    # 4 one garbage frame that comes straight back (flash, A-B-A)
    tl.step("garbage frame")
    tl.press()
    tl.anim(scene(5), n=6)
    junk = scene(999)
    tl.frame(junk)
    tl.frame(scene(5))
    tl.idle(600)
    # 5 the old screen comes back for a frame (stale)
    tl.step("stale frame")
    before = tl.cur
    tl.press()
    tl.anim(scene(6), n=6)
    tl.frame(before)
    tl.frame(scene(6))
    tl.idle(600)
    # 6 hitch: 180 ms between two moving frames
    tl.step("hitch", {"hitch_ms": 100})
    tl.press()
    tl.anim(blend(tl.cur, scene(7), 0.5), n=4)
    tl.idle(180)
    tl.anim(scene(7), n=4)
    tl.idle(600)
    # 7 slow response: 260 ms before anything moves
    tl.step("slow response", {"respond_ms": 150})
    tl.press(latency_ms=260)
    tl.anim(scene(8))
    tl.idle(600)
    # 8 no response at all
    tl.step("no response", {"respond_ms": 150})
    tl.press()
    tl.idle(800)
    # 9 repeated presses (4 x down 250 ms apart): no hitch, no fault
    tl.step("repeated presses", {"hitch_ms": 100})
    for k in range(4):
        tl.press("down")
        tl.anim(scene(20 + k), n=5)
        tl.idle(150)
    tl.idle(600)
    # 10 game launch that goes through black, black allowed
    tl.step("launch through black", {"no": ["flash", "stale"]})
    tl.press("a")
    tl.anim(bytes(F.W * F.H), n=4)
    for _ in range(30):
        tl.frame(bytes(F.W * F.H))
    tl.anim(scene(30), n=6)
    tl.idle(600)
    tl.write()


def test_synthetic(tmp):
    print("1. synthetic run with known faults")
    d = os.path.join(tmp, "synthetic")
    build_synthetic(d)
    res = A.analyze(d)
    R.write(d, res)
    steps = {s["name"]: s for s in res["journeys"][0]["steps"]}

    def kinds(n):
        return sorted({f["kind"] for f in steps[n]["metrics"].get("faults", [])})

    check(steps["clean crossfade"]["verdict"] == "pass", f"clean crossfade passes ({steps['clean crossfade']['verdict']}: {steps['clean crossfade']['reasons'] + steps['clean crossfade']['warns']})")
    r = steps["clean crossfade"]["metrics"]["respond_ms"]
    check(r is not None and 35 <= r <= 60, f"crossfade response measured ~40 ms (got {r})")
    check(steps["clean slide"]["verdict"] == "pass", f"clean slide passes ({steps['clean slide']['reasons'] + steps['clean slide']['warns']})")
    check(kinds("black mid-transition") == ["black"], f"black mid-transition found as black ({kinds('black mid-transition')})")
    bf = steps["black mid-transition"]["metrics"]["faults"]
    check(bf and bf[0]["frames"] == 2, f"black run is 2 frames ({bf and bf[0]['frames']})")
    check(kinds("white flash") == ["blank"], f"white flash found as blank ({kinds('white flash')})")
    check(kinds("garbage frame") == ["flash"], f"garbage frame found as flash ({kinds('garbage frame')})")
    check(kinds("stale frame") == ["stale"], f"old screen coming back found as stale ({kinds('stale frame')})")
    h = steps["hitch"]
    check(h["verdict"] == "fail" and any("hitch" in x for x in h["reasons"]), f"180 ms gap fails as a hitch ({h['reasons']})")
    g = h["metrics"]["max_gap_ms"]
    check(g is not None and 190 <= g <= 200, f"hitch gap measured ~197 ms (got {g})")
    s = steps["slow response"]
    check(s["verdict"] == "fail" and 255 <= s["metrics"]["respond_ms"] <= 265, f"slow response fails at ~260 ms ({s['metrics']['respond_ms']})")
    n = steps["no response"]
    check(n["verdict"] == "fail" and n["metrics"]["respond_ms"] is None, f"no response fails ({n['reasons']})")
    rp = steps["repeated presses"]
    check(rp["verdict"] == "pass", f"repeated presses pass ({rp['reasons'] + rp['warns']})")
    lb = steps["launch through black"]
    check(lb["verdict"] == "warn" and kinds("launch through black") == ["black"], f"allowed black is a warning, not a failure ({lb['verdict']})")
    check(os.path.getsize(os.path.join(d, "report.html")) > 2000, "report.html written with filmstrips")
    cl = json.load(open(os.path.join(d, "clips.json")))
    check(len(cl) >= 7, f"clip windows for failing steps ({len(cl)})")
    return d


def test_video(tmp, src_dir):
    print("2. video path: encode the synthetic frames, decode, find the same faults")
    ff = F.find_ffmpeg()
    if not ff:
        print("  skip  no ffmpeg (FFMPEG=... or pip install imageio-ffmpeg)")
        return
    th = F.Thumbs(os.path.join(src_dir, "thumbs.bin"))
    fr = F.read_frames(os.path.join(src_dir, "frames.csv"))
    frames = [th[i] for i in range(th.n)]
    times = [(f[1] - fr[0][1]) / 1e9 for f in fr]
    vid = os.path.join(tmp, "synthetic.mp4")
    F.encode_video(frames, times, vid, ff)
    out = os.path.join(tmp, "decoded")
    n = F.ingest_video(vid, out, ff)
    check(n == len(frames), f"decoded every frame ({n} of {len(frames)})")
    want = A.scan(src_dir)
    got = A.scan(out)
    key = lambda fs: [(f["kind"], f["first"], f["frames"]) for f in fs]  # noqa
    check(key(got["faults"]) == key(want["faults"]), f"same faults at the same frames after the mp4 round trip ({key(got['faults'])})")
    dt = max((abs(a["at_s"] - b["at_s"]) for a, b in zip(got["faults"], want["faults"])), default=0)
    check(dt < 0.002, f"fault times match within 2 ms ({dt * 1000:.2f} ms)")
    # Constant-rate clip with injected faults at known frames.
    base = [blend(scene(1), scene(2), (i % 60) / 60) for i in range(240)]
    for i in (50, 51):
        base[i] = bytes(F.W * F.H)
    base[120] = b"\xff" * (F.W * F.H)
    base[170] = scene(77)  # mid-crossfade (frame 180 is where the loop restarts)
    cfr = os.path.join(tmp, "cfr.mp4")
    F.encode_video(base, 60, cfr, ff)
    out2 = os.path.join(tmp, "cfr")
    F.ingest_video(cfr, out2, ff)
    s = A.scan(out2)
    k = [(f["kind"], f["first"]) for f in s["faults"]]
    check(("black", 50) in k and ("blank", 120) in k and ("flash", 170) in k, f"60 fps clip: black@50, blank@120, flash@170 found ({k})")
    check(len(k) == 3, f"and nothing else ({len(k)} faults)")


def test_mock_runner(tmp):
    print("3. runner end to end against the simulated launcher (faults on chosen presses)")
    out = os.path.join(tmp, "mock")
    r = subprocess.run([sys.executable, os.path.join(HERE, "device", "runner.py"), "--mock", "--out", out,
                        "--only", "tabs,quick-settings,game-page", "--mock-faults", "black=rb:2,flash=lb:3,hitch=rb:5"],
                       capture_output=True, text=True, timeout=300)
    check(r.returncode == 0 and os.path.exists(os.path.join(out, "done")), "mock run finished" + ("" if r.returncode == 0 else f": {r.stderr[-400:]}"))
    res = A.analyze(out)
    R.write(out, res)
    tabs = next(j for j in res["journeys"] if j["name"] == "tabs")
    bad = {s["name"]: s for s in tabs["steps"] if s["verdict"] == "fail"}
    check(set(bad) == {"RB to games", "RB to market", "LB to market"}, f"exactly the 3 injected faults fail ({sorted(bad)})")
    check(any("black" in x for x in bad.get("RB to games", {}).get("reasons", [])), "RB #2 black frame found")
    check(any("hitch" in x for x in bad.get("RB to market", {}).get("reasons", [])), "RB #5 hitch found")
    check(any("flash" in x for x in bad.get("LB to market", {}).get("reasons", [])), "LB #3 flash found")
    others = [j for j in res["journeys"] if j["name"] != "tabs"]
    # warn (a soft timing threshold) happens on a loaded CI box; only a fail is a finding here.
    check(all(j["verdict"] in ("pass", "warn") for j in others), "quick-settings and game-page do not fail: " + ", ".join(f"{j['name']}={j['verdict']}" for j in others))
    ok_resp = [s["metrics"]["respond_ms"] for s in tabs["steps"] if s["metrics"].get("respond_ms")]
    check(ok_resp and all(30 <= x <= 150 for x in ok_resp), f"responses measured ({min(ok_resp)}-{max(ok_resp)} ms)")
    ev = [json.loads(l) for l in open(os.path.join(out, "events.jsonl"))]
    ends = [e for e in ev if e["type"] == "journey_end"]
    check(all(e["cleanup"]["ok"] for e in ends), "cleanup returned Home after every journey")
    guide = [e for e in ev if e["type"] == "input" and e["button"] == "guide" and e.get("step") in ("cleanup", "preflight", "final")]
    check(not guide, f"cleanup and the probe wake-up never press Guide (it toggles the Rare menu) ({len(guide)})")
    conf = [e for e in ev if e["type"] == "home_confirmed"]
    check(len(conf) >= 2 * len(ends) and all(e.get("probe") for e in conf), f"Home confirmed by a fresh probe line before and after every journey ({len(conf)})")


def test_ipfail(tmp):
    print("3e. InputPlumber times out mid-run: the runner switches to the virtual pad and carries on")
    out = os.path.join(tmp, "mock-ipfail")
    r = subprocess.run([sys.executable, os.path.join(HERE, "device", "runner.py"), "--mock", "--out", out,
                        "--only", "tabs,quick-settings", "--mock-ipfail", "7"], capture_output=True, text=True, timeout=300)
    ev = [json.loads(l) for l in open(os.path.join(out, "events.jsonl"))]
    fb = [e for e in ev if e["type"] == "pad_fallback"]
    check(r.returncode == 0 and open(os.path.join(out, "done")).read().startswith("done"), "run finished" + ("" if r.returncode == 0 else f": {r.stderr[-300:]}"))
    check(len(fb) == 1, f"switched to the virtual pad once ({len(fb)})")
    res = A.analyze(out)
    verdicts = {j["name"]: j["verdict"] for j in res["journeys"]}
    check(verdicts == {"tabs": "pass", "quick-settings": "pass"}, f"both journeys pass after the switch ({verdicts})")
    ends = [e for e in ev if e["type"] == "journey_end"]
    check(all(e["cleanup"]["ok"] for e in ends), "cleanup returned Home after every journey")


def test_guide_cleanup(tmp):
    print("3f. a journey that leaves the Rare menu open: cleanup gets Home without Guide")
    jd = os.path.join(tmp, "guide-journeys")
    os.makedirs(jd, exist_ok=True)
    open(os.path.join(jd, "menu.yaml"), "w").write(
        "name: menu\ntitle: Open the Rare menu and leave it open\ntimeout_s: 30\nsteps:\n"
        "  - name: Guide opens the menu\n    press: guide\n    expect: {screen: {overlay: raremenu}}\n")
    out = os.path.join(tmp, "mock-guide")
    r = subprocess.run([sys.executable, os.path.join(HERE, "device", "runner.py"), "--mock", "--out", out, "--journeys", jd],
                       capture_output=True, text=True, timeout=120)
    ev = [json.loads(l) for l in open(os.path.join(out, "events.jsonl"))]
    ends = [e for e in ev if e["type"] == "journey_end"]
    check(r.returncode == 0 and len(ends) == 1, "run finished" + ("" if r.returncode == 0 else f": {r.stderr[-300:]}"))
    check(ends and ends[0]["cleanup"]["ok"], f"cleanup closed the menu and confirmed Home ({ends and ends[0]['cleanup']})")
    guide = [e for e in ev if e["type"] == "input" and e["button"] == "guide" and e["action"] == "down"]
    check(len(guide) == 1 and guide[0].get("step") == 0, f"Guide pressed once, by the journey only ({[(e.get('step')) for e in guide]})")


def test_pad_release(tmp):
    print("3g. the virtual pad goes away neutral: every axis, hat and button released and read before it is destroyed")
    import types
    sys.path.insert(0, os.path.join(HERE, "device"))
    import runner as RN  # noqa: E402
    names = ["BTN_SOUTH", "BTN_EAST", "BTN_NORTH", "BTN_WEST", "BTN_TL", "BTN_TR", "BTN_SELECT", "BTN_START", "BTN_MODE",
             "BTN_THUMBL", "BTN_THUMBR", "ABS_X", "ABS_Y", "ABS_RX", "ABS_RY", "ABS_Z", "ABS_RZ", "ABS_HAT0X", "ABS_HAT0Y",
             "KEY_VOLUMEUP", "KEY_VOLUMEDOWN"]
    e = types.SimpleNamespace(EV_KEY=1, EV_ABS=3, **{n: 100 + i for i, n in enumerate(names)})
    log = []

    class Dev:
        def __init__(self, name):
            self.name = name

        def write(self, t, c, v):
            log.append(("w", self.name, t, c, v))

        def syn(self):
            log.append(("syn", self.name))

        def close(self):
            log.append(("close", self.name))

    sleeps = []
    real_sleep = RN.time.sleep
    RN.time.sleep = lambda s: (sleeps.append(s), log.append(("sleep", s)))
    try:
        p = object.__new__(RN.UinputPad)
        p.e, p.ui, p.kb, p.held = e, Dev("pad"), Dev("keys"), set()
        p.map = {"a": e.BTN_SOUTH, "b": e.BTN_EAST, "x": e.BTN_WEST, "y": e.BTN_NORTH, "lb": e.BTN_TL, "rb": e.BTN_TR,
                 "select": e.BTN_SELECT, "start": e.BTN_START, "guide": e.BTN_MODE, "l3": e.BTN_THUMBL, "r3": e.BTN_THUMBR}
        p.set("right", True)
        p.set("a", True)
        p.stick("left", -1.0, 0.0)
        # The harness lost track of what is down (a crash between the press and its bookkeeping).
        p.held.clear()
        log.clear()
        p.close()
    finally:
        RN.time.sleep = real_sleep
    last = {}
    for x in log:
        if x[0] == "w" and x[1] == "pad":
            last[(x[2], x[3])] = x[4]
    check(last.get((e.EV_ABS, e.ABS_HAT0X)) == 0, "the hat is back at 0 although nothing was recorded held")
    check(last.get((e.EV_ABS, e.ABS_X)) == 0 and last.get((e.EV_ABS, e.ABS_Y)) == 0, "the left stick is back at rest")
    check(last.get((e.EV_KEY, e.BTN_SOUTH)) == 0, "A is released")
    kinds = [x[0] for x in log]
    closed = kinds.index("close") if "close" in kinds else -1
    check(closed > 0 and "syn" in kinds[:closed] and "sleep" in kinds[kinds.index("syn"):closed],
          "neutral is synced, then a frame passes, then the device is destroyed")
    check(p.ui is None and p.kb is None, "nothing is left alive")


def test_abort(tmp):
    print("3b. physical input mid-run aborts and the runner still returns Home")
    out = os.path.join(tmp, "mock-abort")
    r = subprocess.run([sys.executable, os.path.join(HERE, "device", "runner.py"), "--mock", "--out", out,
                        "--only", "tabs,quick-settings", "--mock-foreign", "2.5"], capture_output=True, text=True, timeout=120)
    ev = [json.loads(l) for l in open(os.path.join(out, "events.jsonl"))]
    ends = [e for e in ev if e["type"] == "journey_end"]
    check(r.returncode == 0 and open(os.path.join(out, "done")).read().startswith("physical input"), "run stopped on physical input")
    check(len(ends) == 1 and ends[0]["status"] == "aborted", f"tabs ended aborted, quick-settings never started ({[(e['journey'], e['status']) for e in ends]})")
    check(ends and ends[0]["cleanup"]["ok"], "cleanup returned Home after the abort")
    t_abort = next(x["t"] for x in ev if x["type"] == "abort")
    after = [e for e in ev if e["type"] == "input" and e["action"] == "down" and e["t"] > t_abort]
    check(not after, f"no press after the abort, cleanup included ({len(after)})")
    check(any(e["type"] == "input_lockout" for e in ev), "the owner's input locks the injector")
    sys.path.insert(0, os.path.join(HERE, "device"))
    import runner as RN  # noqa

    class Ev:
        def __init__(self, type_, code, value):
            self.type, self.code, self.value = type_, code, value

    class Info:
        def __init__(self, lo, hi):
            self.min, self.max = lo, hi

    class E:
        EV_KEY, EV_ABS, EV_REL, ABS_HAT0X, ABS_HAT0Y = 1, 3, 2, 16, 17

    iw = RN.InputWatch.__new__(RN.InputWatch)
    iw.absinfo = {"p": {0: Info(-32768, 32767), 2: Info(0, 1023)}}
    sig = [iw.significant("p", Ev(3, 0, 2000), E), iw.significant("p", Ev(3, 0, 20000), E), iw.significant("p", Ev(3, 2, 40), E),
           iw.significant("p", Ev(3, 2, 900), E), iw.significant("p", Ev(1, 304, 1), E), iw.significant("p", Ev(3, 16, 0), E)]
    check(sig == [False, True, False, True, True, False], f"stick drift and light trigger noise ignored, real presses count ({sig})")


def _held_at_end(ev):
    held = set()
    for e in ev:
        if e["type"] == "input" and "_stick" not in e["button"] and ":" not in e["button"]:
            (held.add if e["action"] == "down" else held.discard)(e["button"])
    return held


def test_guard_loss(tmp):
    print("3c. the input guard dies or stalls mid-run: abort at once, nothing more pressed, Home again")
    for kind, at, limit_ms in (("exit", 2.5, 100), ("stall", 2.5, 1100)):
        out = os.path.join(tmp, f"mock-guard-{kind}")
        r = subprocess.run([sys.executable, os.path.join(HERE, "device", "runner.py"), "--mock", "--out", out,
                            "--only", "tabs,quick-settings", "--mock-guard", f"{kind}:{at}"], capture_output=True, text=True, timeout=120)
        ev = [json.loads(l) for l in open(os.path.join(out, "events.jsonl"))]
        fault = next((e for e in ev if e["type"] == "guard_fault"), None)
        abort = next((e for e in ev if e["type"] == "abort"), None)
        done = open(os.path.join(out, "done")).read() if os.path.exists(os.path.join(out, "done")) else ""
        check(r.returncode == 0 and "input guard" in done, f"{kind}: run stopped on the guard ({done.strip() or r.stderr[-300:]})")
        check(bool(fault and abort) and (abort["t"] - fault["t"]) / 1e6 <= limit_ms,
              f"{kind}: aborted {((abort['t'] - fault['t']) / 1e6) if fault and abort else -1:.0f} ms after the guard went (limit {limit_ms} ms)")
        stale_at = fault["t"] + (0 if kind == "exit" else 1_000_000_000) if fault else 0
        late = [e for e in ev if e["type"] == "input" and e["action"] == "down" and fault and e["t"] > stale_at]
        check(not late, f"{kind}: no press once the guard was gone ({len(late)}), cleanup and the final cleanup included")
        check(not _held_at_end(ev), f"{kind}: nothing left held ({sorted(_held_at_end(ev))})")
        ends = [e for e in ev if e["type"] == "journey_end"]
        check(len(ends) == 1 and ends[0]["status"] == "aborted" and ends[0]["cleanup"]["ok"],
              f"{kind}: tabs aborted, quick-settings never started, cleanup back Home without pressing ({[(e['journey'], e['status'], e['cleanup']['ok']) for e in ends]})")
        check(any(e["type"] == "input_lockout" for e in ev), f"{kind}: injector locked")


def test_flash_guard(tmp):
    """flash/: guard-run.sh + beat.py + eikey.py (dry keys) with a fake restore."""
    import signal as S
    import time as T
    print("3d. flash tools: the idle guard is a hard dependency of the keys")
    fl = os.path.join(HERE, "flash")

    def setup(name):
        d = os.path.join(tmp, "flash-" + name)
        os.makedirs(os.path.join(d, "evdev"), exist_ok=True)  # not "input": INPUT is the flag (case-insensitive APFS)
        restore = os.path.join(d, "restore-fake.sh")
        open(restore, "w").write(f"#!/bin/sh\necho restored >> '{d}/restored'\n")
        os.chmod(restore, 0o755)
        env = dict(os.environ, FLASHCAP=d, RARE_INPUT_DIR=os.path.join(d, "evdev"), RARE_RESTORE=restore, RARE_KEYS_DRY="1")
        return d, env

    def start_guard(d, env):
        g = subprocess.Popen(["bash", os.path.join(fl, "guard-run.sh"), "60"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(60):
            if subprocess.run([sys.executable, "-I", os.path.join(fl, "beat.py"), "check"], env=env, capture_output=True).returncode == 0:
                return g
            T.sleep(0.05)
        return g

    def keys(d, env):
        log = open(os.path.join(d, "keys.log"), "w")
        return subprocess.Popen([sys.executable, "-I", os.path.join(fl, "eikey.py"), "--gap", "0.1"] + ["e"] * 300,
                                env=env, stdout=log, stderr=subprocess.STDOUT)

    def key_times(d):
        out = []
        for line in open(os.path.join(d, "keys.log")):
            p = line.split()
            if len(p) == 2 and p[1] == "e":
                out.append(float(p[0]))
        return out

    def guard_pid(d):
        return int(open(os.path.join(d, "GUARD.beat")).read().split()[0])

    for name, how, limit in (("kill", "SIGKILL the guard", 0.35), ("stall", "SIGSTOP the guard", 1.4),
                             ("supervisor", "SIGKILL the supervisor", 1.4)):
        d, env = setup(name)
        g = start_guard(d, env)
        open(os.path.join(d, "ARMED"), "w").close()
        k = keys(d, env)
        T.sleep(0.8)
        idle_pid = guard_pid(d)
        t_kill = T.time()
        if name == "kill":
            os.kill(idle_pid, S.SIGKILL)
        elif name == "stall":
            os.kill(idle_pid, S.SIGSTOP)
        else:
            os.kill(g.pid, S.SIGKILL)
        try:
            k.wait(timeout=5)
        except subprocess.TimeoutExpired:
            k.kill()
        for _ in range(60):  # restore runs in whoever noticed first
            if os.path.exists(os.path.join(d, "restored")):
                break
            T.sleep(0.05)
        ts = key_times(d)
        check(len(ts) >= 5, f"{name}: keys flowed while the guard was alive ({len(ts)})")
        last = max(ts) - t_kill if ts else 0
        check(k.returncode is not None and last <= limit, f"{name} ({how}): keys stopped {last * 1000:.0f} ms after (limit {limit * 1000:.0f} ms), injector exit {k.returncode}")
        check(os.path.exists(os.path.join(d, "restored")) and open(os.path.join(d, "restored")).read().count("restored") == 1,
              f"{name}: armed session restored exactly once")
        check(not os.path.exists(os.path.join(d, "ARMED")), f"{name}: disarmed")
        try:
            os.kill(idle_pid, S.SIGKILL)
        except OSError:
            pass
        g.kill() if g.poll() is None else None
    d, env = setup("none")
    k = keys(d, env)
    k.wait(timeout=5)
    check(k.returncode == 4 and not key_times(d), f"no guard: not a single key ({len(key_times(d))}), exit {k.returncode}")
    d, env = setup("stop")
    g = start_guard(d, env)
    open(os.path.join(d, "ARMED"), "w").close()
    subprocess.run([sys.executable, "-I", os.path.join(fl, "beat.py"), "stop"], env=env, timeout=10)
    g.wait(timeout=5)
    check(g.returncode == 0 and not os.path.exists(os.path.join(d, "restored")) and os.path.exists(os.path.join(d, "ARMED")),
          "beat.py stop ends the guard on purpose without a restore")


def test_captures(tmp):
    print("4. existing captures")
    roots = [os.path.join(HERE, "..", "..", "bench")] + [p for p in os.environ.get("JOURNEY_CAPTURES", "").split(":") if p]
    vids = []
    for r in roots:
        if os.path.isfile(r):
            vids.append(r)
        for ext in ("mp4", "mkv", "webm", "mov"):
            vids += glob.glob(os.path.join(r, "**", "*." + ext), recursive=True)
    raws = []
    for r in roots:
        raws += [p[:-4] for p in glob.glob(os.path.join(r, "**", "*.raw"), recursive=True) if os.path.exists(p[:-4] + ".pts")]
    for base in raws:
        try:
            out = os.path.join(tmp, "raw-" + os.path.basename(base))
            n = F.ingest_raw(os.path.dirname(base), os.path.basename(base), out)
            s = A.scan(out)
            big = [f for f in s["faults"] if f["kind"] in ("flash", "stale") and not f.get("minor")]
            print(f"  scan  {base}.raw (tools/journeys/flash capture): {n} frames, {len(big)} flash(es) over {A.FLASH_FAIL * 100:g}% at frames {[f['first'] for f in big]}")
        except ImportError:
            print(f"  skip  {base}.raw needs numpy")
    if not vids and not raws:
        print("  none in bench/ (set JOURNEY_CAPTURES=path[:path] to scan others)")
        return
    if vids and not F.find_ffmpeg():
        print("  skip  no ffmpeg")
        return
    for v in vids:
        out = os.path.join(tmp, "cap-" + os.path.basename(v))
        try:
            F.ingest_video(v, out)
            s = A.scan(out)
            kinds = {}
            for f in s["faults"]:
                kinds[f["kind"]] = kinds.get(f["kind"], 0) + 1
            print(f"  scan  {v}: {s['frames']} frames, {s['seconds']} s, frame {s['nominal_frame_ms']} ms, faults {kinds or 'none'}")
            for f in s["faults"][:8]:
                print(f"          {f['kind']} at {f['at_s']} s ({f['frames']} frames)")
        except Exception as e:  # noqa
            print(f"  scan  {v}: {e}")


def main():
    keep = None
    if "--keep" in sys.argv:
        keep = sys.argv[sys.argv.index("--keep") + 1]
    tmp = keep or tempfile.mkdtemp(prefix="journeys-selftest-")
    os.makedirs(tmp, exist_ok=True)
    print("0. journey files")
    try:
        js = spec.load_all(os.path.join(HERE, "journeys"))
        check(len(js) >= 10, f"{len(js)} journeys load and validate")
    except spec.SpecError as e:
        check(False, f"journeys: {e}")
    for bad, why in (({"steps": [{"press": ["l3", "r3"]}]}, "abort chord"), ({"steps": [{"press": "select"}]}, "select alone"),
                     ({"steps": [{"press": "zz"}]}, "unknown button")):
        try:
            spec.normalize(bad, "x.yaml")
            check(False, f"rejects {why}")
        except spec.SpecError:
            check(True, f"rejects {why}")
    check(bool(spec.DENY_A_FOCUS.search('button@qs "Power off" at 1,2 3x4')) and not spec.DENY_A_FOCUS.search('button@qs "Sleep" at 1,2 3x4'),
          "A is refused on Power off, allowed on Sleep")
    d = test_synthetic(tmp)
    test_video(tmp, d)
    if "--quick" not in sys.argv:
        test_mock_runner(tmp)
        test_abort(tmp)
        test_guard_loss(tmp)
        test_ipfail(tmp)
        test_pad_release(tmp)
        test_guide_cleanup(tmp)
    test_flash_guard(tmp)
    test_captures(tmp)
    print(f"\n{'ALL OK' if not FAILS else str(len(FAILS)) + ' FAILED'}  (work dir {tmp})")
    if not keep and not FAILS:
        shutil.rmtree(tmp, ignore_errors=True)
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
