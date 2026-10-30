#!/usr/bin/env python3
"""Self-test of the golden tooling, no GPU needed: the diff rule, the budget gates on synthetic
stats logs, placeholder art, fixtures, and a report over fake results.
  python3 tools/render/golden/test_golden.py"""
from __future__ import annotations

import json
import os
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(HERE, ".."))

import numpy as np  # noqa: E402
from PIL import Image  # noqa: E402

import art  # noqa: E402
import budget  # noqa: E402
import golden  # noqa: E402
import imgdiff  # noqa: E402
import report  # noqa: E402

FAILS = []


def check(cond, what):
    print(("  ok    " if cond else "  FAIL  ") + what)
    if not cond:
        FAILS.append(what)


def png(path, arr):
    Image.fromarray(arr.astype(np.uint8)).save(path)


def test_diff(tmp):
    print("diff rule: more than 0.1 % of pixels off by more than 2 levels fails")
    base = np.full((200, 300, 3), 40, np.uint8)
    png(f"{tmp}/a.png", base)
    b = base.copy()
    b[:, :] += 2  # everything off by exactly 2: equal
    png(f"{tmp}/b.png", b)
    check(imgdiff.compare(f"{tmp}/b.png", f"{tmp}/a.png")["status"] == "pass", "a 2-level shift everywhere passes")
    c = base.copy()
    c[0:6, 0:10] = 200  # 60 px of 60000 = 0.1 %: at the limit, passes
    png(f"{tmp}/c.png", c)
    r = imgdiff.compare(f"{tmp}/c.png", f"{tmp}/a.png", f"{tmp}/c-diff.png")
    check(r["status"] == "pass" and r["pixels"] == 60, f"0.1 % of pixels off passes ({r})")
    c[6, 0] = 200  # one more pixel: over
    png(f"{tmp}/c.png", c)
    r = imgdiff.compare(f"{tmp}/c.png", f"{tmp}/a.png", f"{tmp}/c-diff.png")
    check(r["status"] == "fail" and r["bbox"] == [0, 0, 10, 7] and os.path.exists(f"{tmp}/c-diff.png"), f"61 pixels fail with a bbox and a diff image ({r.get('bbox')})")
    png(f"{tmp}/d.png", np.zeros((100, 300, 3)))
    check(imgdiff.compare(f"{tmp}/d.png", f"{tmp}/a.png")["status"] == "fail", "a size change fails")
    f = imgdiff.side_by_side(f"{tmp}/c.png", f"{tmp}/a.png", f"{tmp}/sbs.jpg")
    check(os.path.exists(f"{tmp}/sbs.jpg") and 0 < f < 0.01, f"side by side written ({f * 100:.2f} % differ)")


def stats_log(path, frames, ins=()):
    with open(path, "w") as o:
        o.write("start 0\n")
        t = 1_000_000_000
        for i, (gap_ms, layout, inputs, cpu_us) in enumerate(frames, 1):
            t += int(gap_ms * 1e6)
            o.write(f"f {i} {t - 2_000_000} {t} {cpu_us} 100 200 300 4 {layout} {inputs} {int(gap_ms * 1000) if i > 1 else 0}\n")
        for e in ins:
            o.write(e + "\n")


def test_budget(tmp):
    print("budget gates on synthetic RR_STATS logs")
    good = [(16.7, 1, 0, 900)] + [(500, 1, 1, 1200)] + [(16.7, 0, 0, 300)] * 12 + [(800, 1, 1, 1100)] + [(16.7, 0, 0, 280)] * 12
    stats_log(f"{tmp}/good.stats", good)
    r = budget.check(f"{tmp}/good.stats")
    check(r["ok"] and r["window_method"] == "inputs" and r["windows"] == 2, f"clean run passes ({r['gates']})")
    bad = list(good)
    bad[5] = bad[6] = (16.7, 1, 0, 300)  # relayout on consecutive animation frames
    stats_log(f"{tmp}/layout.stats", bad)
    r = budget.check(f"{tmp}/layout.stats")
    check(not r["ok"] and not r["gates"]["layout"]["ok"], "a relayout on consecutive animation frames fails")
    one = list(good)
    one[1] = (500, 0, 1, 1200)  # the input moves focus without a layout (a transform)
    one[8] = (16.7, 1, 0, 300)  # one deferred state change mid-animation
    stats_log(f"{tmp}/one.stats", one)
    r = budget.check(f"{tmp}/one.stats")
    check(r["ok"] and not r["gates"]["deferred-layout"]["ok"], "a single deferred layout is listed, not failed")
    two = list(good)
    two[8] = (16.7, 1, 0, 300)  # the input's own layout plus one mid-animation
    stats_log(f"{tmp}/two.stats", two)
    check(not budget.check(f"{tmp}/two.stats")["ok"], "two layouts after one input fail (per input)")
    hitch = list(good)
    hitch[6] = (40, 0, 0, 300)
    stats_log(f"{tmp}/hitch.stats", hitch)
    r = budget.check(f"{tmp}/hitch.stats")
    check(r["ok"] and not r["gates"]["hitch"]["ok"], "a 40 ms interval only warns in CI")
    r = budget.check(f"{tmp}/hitch.stats", device=True)
    check(not r["ok"], "and fails on the device")
    slow = [(g, l, i, 6000) for g, l, i, _ in good]
    stats_log(f"{tmp}/slow.stats", slow)
    check(not budget.check(f"{tmp}/slow.stats", device=True)["ok"], "CPU p99 over 4 ms fails on the device")
    gaps = [(g, l, 0, c) for g, l, _, c in good]
    stats_log(f"{tmp}/gaps.stats", gaps)
    r = budget.check(f"{tmp}/gaps.stats")
    check(r["ok"] and r["window_method"] == "gaps" and r["windows"] == 2, f"no input lines: windows from idle gaps ({r.get('windows')})")


def test_art_and_fixtures(tmp):
    print("fixtures and placeholder art")
    idx = golden.fixture_index()
    check(len(idx["sets"]) >= 50 and "default" in idx["sets"], f"{len(idx['sets'])} fixture sets")
    check(all(idx["shots"][k] in idx["sets"] for k in idx["shots"]), "every web shot maps to a set")
    fx = golden.merged_fixture("default", idx)
    games = fx["lib"]["library"]["games"]
    check(any(g["title"] == "Hades II" for g in games) and fx["lib"]["device"]["battery"]["percent"] == 82, "default set: the mock library and device")
    loc = art.localize(fx, f"{tmp}/art", golden.FONT, art.titles_from(fx))
    covers = [g["art"]["cover"][0] for g in loc["lib"]["library"]["games"] if g["art"]["cover"]]
    check(covers and all(os.path.isabs(c) and os.path.exists(c) for c in covers if not c.startswith("games/")), "Steam art becomes local placeholder files")
    check(not any(isinstance(v, str) and art.is_image_url(v) for v in json.dumps(loc).split('"')), "no image URL left")
    a1 = open(covers[0], "rb").read()
    os.unlink(covers[0])
    art.localize(fx, f"{tmp}/art", golden.FONT, art.titles_from(fx))
    check(open(covers[0], "rb").read() == a1, "placeholder art is the same bytes when drawn again")


def test_report(tmp):
    print("report over fake results")
    _, states = golden.load_states()
    check(any(s["id"] == "poc-home" for s in states), f"{len(states)} states load")
    out = f"{tmp}/run"
    os.makedirs(f"{out}/1920x1200")
    png(f"{out}/1920x1200/poc-home.png", np.full((120, 192, 3), 30))
    res = {"poc-home": {"1920x1200": {"render": "ok", "golden": "unapproved", "fixture": "default"}}}
    web = f"{tmp}/web"
    os.makedirs(web)
    png(f"{web}/home-1920x1200.png", np.full((120, 192, 3), 34))
    report.write(out, states, res, golden.web_shots(), golden.fixture_index(), web, ["1920x1200"], golden.REPO, golden.APPROVED)
    h = open(f"{out}/index.html").read()
    check("poc-home" in h and "not ported" in h and "settings-power" in h, "dashboard lists native and web states")
    check(os.path.exists(f"{out}/sbs/poc-home-1920x1200.jpg"), "side by side against the web shot")


def main():
    with tempfile.TemporaryDirectory() as tmp:
        test_diff(tmp)
        test_budget(tmp)
        test_art_and_fixtures(tmp)
        test_report(tmp)
    print("ALL OK" if not FAILS else f"{len(FAILS)} FAILED")
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
