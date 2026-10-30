#!/usr/bin/env python3
"""UX journeys: the owner's most common tasks, driven on the real native launcher, headless.

Each file in tools/journeys/ux/ is one task (open Steam, uninstall a game, change the volume...)
with the exact presses that do it from Home and from inside a game. This runs the launcher
offscreen (RR_SHOT) with the journey probe on, the device's real library copied read only and the
fixture backend for the rest (no network, no system calls), plays the presses, reads the probe log
and checks that the task ended where it should in at most `max` presses (3 unless the file says).
A press that changes nothing is reported as a dead press.

  ux.py run  [--bin DIR] [--device DIR] [--only a,b] [--res WxH[,WxH]] [--jobs N] [--out DIR] [--post REPO]
             [--was]          play the `was:` presses instead (on a build from before the fix)
  ux.py table [--out DIR]     the summary table (task | presses before | presses after | fix)
  ux.py lint                  checks every journey file

--bin is a packaged rare-native folder (a release build's ~/rare-dev/out/<worktree>/rare-native, or
CI's). --device holds the device's files at their home paths (.cache/rare/library.json ...;
`ux.sh pull-device` copies them from the SuiPlay, read only). --post REPO writes the failures to
a report in REPO and pushes it (never blocks anything: exit 0 unless --strict).

Journey file:

  name: open-steam
  task: Open Steam
  max: 3                         # presses allowed (default 3)
  fix: Steam tile left of the first game on Home; X in the Rare menu
  home:                          # from Home, nothing open
    env: {RR_RUNNING: "steam:1145350"}     # optional extra launcher env
    steps: [left, a]
    done: {act: SteamBigPicture}
    was: [down, down, down, down, down, down, a]   # the presses it took before the fix
    was_none: true               # instead of was: there was no way at all from here
    was_same: true               # unchanged by the fix (same presses as steps)
    was_est: 19                  # an estimate where the old way can't be played headless
  game:                          # inside a game: `open` is the button that brings Rare up
    open: menu                   # menu (the Rare button), quick (the Quick button), switcher (Rare held)
    steps: [x]
    done: {act: SteamBigPicture}

done (all must hold at the end): act (regex on any act the presses ran), screen ({tab, overlay}
regexes), focus (regex on the focused element's text), osk (regex on the open keyboard's title),
ingame (regex on the in-game mode), log / nolog (some / no probe line matches, e.g. the top bar's
`topbar ... fits=false`).
"""
from __future__ import annotations

import argparse
import concurrent.futures as cf
import datetime as dt
import glob
import json
import os
import re
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "../.."))
UX = os.path.join(HERE, "ux")
sys.path.insert(0, HERE)
from jlib.spec import _load_text  # noqa: E402

DEFAULT_BIN = os.path.expanduser("~/rare-dev/uxj/bin")
DEFAULT_DEVICE = os.path.expanduser("~/rare-dev/uxj/device")
DEFAULT_OUT = os.path.expanduser("~/rare-dev/uxj/runs")
GAME = "steam:1145350"  # a game on the device's library (Hades II) for the in-game variants
BUTTONS = {"up", "down", "left", "right", "a", "b", "x", "y", "lb", "rb", "lt", "rt", "start", "menu", "select", "view", "guide", "rare", "qam", "quick", "guidehold", "exithold"}
OPEN_PRESS = {"menu": "guide", "quick": "qam", "switcher": "guidehold", "power": "power"}


def load_journeys(only=None):
    out = []
    for p in sorted(glob.glob(os.path.join(UX, "*.yaml"))):
        j = _load_text(p)
        j["_path"] = p
        if only and j["name"] not in only:
            continue
        out.append(j)
    return out


def lint(js) -> list:
    errs, names = [], set()
    for j in js:
        n = j.get("name", "?")
        if n in names:
            errs.append(f"{n}: duplicate name")
        names.add(n)
        for k in ("name", "task", "fix"):
            if not j.get(k):
                errs.append(f"{n}: no {k}")
        vs = [v for v in ("home", "game") if v in j]
        if not vs:
            errs.append(f"{n}: needs home: or game:")
        for v in vs:
            spec = j[v]
            for s in spec.get("steps", []) + spec.get("was", []):
                if not (s in BUTTONS or str(s).startswith(("wait:", "text:"))):
                    errs.append(f"{n}/{v}: unknown step {s}")
            if not spec.get("done"):
                errs.append(f"{n}/{v}: no done")
            if v == "game" and spec.get("open", "menu") not in OPEN_PRESS:
                errs.append(f"{n}/game: open must be one of {sorted(OPEN_PRESS)}")
    return errs


def presses_of(j, v, was=False) -> int:
    spec = j[v]
    n = sum(1 for s in spec.get("was" if was else "steps", []) if s in BUTTONS)
    if v == "game":
        n += 1  # the button that brought Rare up over the game
    return n


def run_one(j, v, res, a) -> dict:
    spec = j[v]
    name = f"{j['name']}-{v}-{res}"
    d = os.path.join(a.out, name)
    shutil.rmtree(d, ignore_errors=True)
    os.makedirs(os.path.join(d, "run"))
    os.makedirs(os.path.join(d, "home"))
    open(os.path.join(d, "run", "rare-journey"), "w").close()
    # The device's files at their home paths, copied (never the originals: the launcher may write).
    lib = os.path.join(a.device, ".cache/rare/library.json")
    for rel in (".config/rare/launcher.json", ".local/state/rare-native/played.json", ".cache/rare/knowledge.json"):
        src = os.path.join(a.device, rel)
        if os.path.exists(src):
            os.makedirs(os.path.dirname(os.path.join(d, "home", rel)), exist_ok=True)
            shutil.copyfile(src, os.path.join(d, "home", rel))
            os.chmod(os.path.join(d, "home", rel), 0o644)
    shot = os.path.join(d, "shot.png")
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": os.path.join(d, "home"),
        "XDG_RUNTIME_DIR": os.path.join(d, "run"),
        "TZ": "America/New_York",
        "LANG": "C.UTF-8",
        "RR_SILENT": "1",
        "RR_NO_IP": "1",
        "RR_FIXTURE": "1",
        "RR_WINDOWED": res,
        "RR_CLOCK": "9:41 AM",
        "RR_SHOT": shot,
        "RUST_BACKTRACE": "1",
    }
    for k in ("DISPLAY", "WAYLAND_DISPLAY", "VK_ICD_FILENAMES", "VK_DRIVER_FILES", "WGPU_BACKEND"):
        if os.environ.get(k):
            env[k] = os.environ[k]
    if os.path.exists(lib):
        env["RR_LIBRARY"] = lib
    if v == "game":
        rid = spec.get("game", GAME)
        env["RR_RUNNING"] = rid
        env["RR_OPEN"] = f"ingame:{rid}|{spec.get('open', 'menu')}|fixture"
    for k, val in (j.get("env") or {}).items():
        env[k] = str(val)
    for k, val in (spec.get("env") or {}).items():
        env[k] = str(val)
    steps = [str(s) for s in spec.get("was" if a.was else "steps", [])]
    env["RR_SCRIPT"] = ",".join(["wait:300"] + steps)
    t0 = time.monotonic()
    binary = os.path.join(a.bin, "rare-launcher-native")
    with open(os.path.join(d, "out.log"), "w") as log:
        try:
            code = subprocess.run([binary], cwd=a.bin, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=90).returncode
        except subprocess.TimeoutExpired:
            code = "timeout"
    secs = round(time.monotonic() - t0, 1)
    r = judge(j, v, os.path.join(d, "run", "rare-journey.log"), code, a.was)
    r.update(name=j["name"], task=j["task"], variant=v, res=res, secs=secs, shot=shot if os.path.exists(shot) else None, dir=d)
    return r


def parse_log(path):
    ev = []
    try:
        for line in open(path, encoding="utf-8", errors="replace"):
            parts = line.rstrip("\n").split(" ", 1)
            if len(parts) == 2:
                ev.append(parts[1])
    except FileNotFoundError:
        pass
    return ev


def judge(j, v, logpath, code, was=False) -> dict:
    spec = j[v]
    ev = parse_log(logpath)
    presses = presses_of(j, v, was)
    limit = 99 if was else int(j.get("max", 3))
    screen = {"tab": "", "overlay": "-"}
    focus, osk, ingame = "", "-", "-"
    acts, dead, seen_pad = [], [], 0
    changed = True
    for e in ev:
        if e.startswith("pad "):
            if seen_pad and not changed:
                dead.append(seen_pad)
            seen_pad += 1
            changed = False
            continue
        if e.startswith("screen "):
            kv = dict(x.split("=", 1) for x in e[7:].split() if "=" in x)
            screen = kv
        elif e.startswith("focus "):
            m = re.match(r'focus \S* ?"(.*)" at', e)
            focus = m.group(1) if m else e[6:]
        elif e.startswith("osk "):
            osk = e[4:]
        elif e.startswith("ingame "):
            m = re.match(r"ingame mode=(\S+)", e)
            ingame = m.group(1) if m else "-"
        elif e.startswith("act ") and seen_pad:
            acts.append(e[4:])
        elif e in ("probe on", "painted"):
            continue
        changed = True
    if seen_pad and not changed and not acts:
        dead.append(seen_pad)
    want = spec.get("was_done", spec["done"]) if was else spec["done"]
    why = []
    if code not in (0,):
        why.append(f"launcher exit {code}")
    if not ev:
        why.append("no probe log")
    if "act" in want and not any(re.search(want["act"], x) for x in acts):
        why.append(f"no act matching {want['act']!r} (ran: {', '.join(acts) or 'nothing'})")
    for k, rx in (want.get("screen") or {}).items():
        if not re.fullmatch(str(rx), screen.get(k, "")):
            why.append(f"screen {k}={screen.get(k, '')!r}, wanted {rx!r}")
    if "focus" in want and not re.search(want["focus"], focus):
        why.append(f"focus {focus!r}, wanted {want['focus']!r}")
    if "osk" in want and not re.search(want["osk"], osk):
        why.append(f"keyboard {osk!r}, wanted {want['osk']!r}")
    if "log" in want and not any(re.search(want["log"], e) for e in ev):
        why.append(f"no probe line matching {want['log']!r}")
    if "nolog" in want and any(re.search(want["nolog"], e) for e in ev):
        why.append("a probe line matches " + repr(want["nolog"]) + ": " + next(e for e in ev if re.search(want["nolog"], e)))
    if "ingame" in want and not re.fullmatch(want["ingame"], ingame):
        why.append(f"in-game {ingame!r}, wanted {want['ingame']!r}")
    if presses > limit:
        why.append(f"{presses} presses, at most {limit}")
    # A dead press is a press that changed nothing on screen: a dead end unless the journey says
    # the press is meant to do nothing visible.
    if dead and not spec.get("allow_dead") and not was:
        why.append("dead press " + ", ".join(f"#{n}" for n in dead))
    return {"ok": not why, "why": why, "presses": presses, "max": limit, "end": {"screen": screen, "focus": focus, "osk": osk, "acts": acts}}


def post(results, repo, run_id, commit):
    path = os.path.join(repo, "ux-failures.md")
    fails = [r for r in results if not r["ok"]]
    lines = [
        "# UX journey failures",
        "",
        "Written by `tools/journeys/ux.py run --post` (CI `UX journeys` on the build machine runner, after each push to main). It never blocks a build or an install.",
        "Each line: journey (from), presses, why, last screenshot (on the build machine). An empty list means every journey passed.",
        "",
        f"Last run {run_id} at {commit or '?'}: {len(results) - len(fails)} of {len(results)} passed.",
        "",
    ]
    for r in fails:
        lines.append(f"- {r['name']} ({r['variant']}, {r['res']}): {r['presses']} presses; {'; '.join(r['why'])}. Shot: builder:{r['shot'] or r['dir']}")
    text = "\n".join(lines) + "\n"
    old = open(path).read() if os.path.exists(path) else ""
    strip = lambda t: re.sub(r"Last run .*\n", "", re.sub(r"Shot: builder:\S+", "", t))
    if strip(old) == strip(text):
        print("ux-failures.md: no change")
        return
    with open(path, "w") as f:
        f.write(text)
    g = lambda *a: subprocess.run(["git", "-C", repo, *a], capture_output=True, text=True)
    g("add", path)
    msg = f"UX journeys: {len(fails)} failing" + (f" ({', '.join(sorted({r['name'] for r in fails})[:6])})" if fails else "")
    # Who commits: RARE_COMMIT_NAME / RARE_COMMIT_EMAIL, else ~/.config/rare/commit-identity (name, then email),
    # else git's own config.
    ident = []
    name, email = os.environ.get("RARE_COMMIT_NAME"), os.environ.get("RARE_COMMIT_EMAIL")
    idfile = os.path.expanduser("~/.config/rare/commit-identity")
    if not (name and email) and os.path.exists(idfile):
        lines = [l.strip() for l in open(idfile) if l.strip()]
        if len(lines) >= 2:
            name, email = lines[0], lines[1]
    if name and email:
        ident = ["-c", f"user.name={name}", "-c", f"user.email={email}"]
    c = g(*ident, "commit", "-m", msg)
    if c.returncode != 0:
        print("commit:", c.stdout, c.stderr)
        return
    for _ in range(4):
        if g("pull", "--rebase", "origin", "main").returncode == 0 and g("push", "origin", "HEAD:main").returncode == 0:
            print("posted", msg)
            return
        time.sleep(3)
    print("could not push ux-failures.md")


def table(js, results_path):
    res = {}
    if os.path.exists(results_path):
        for r in json.load(open(results_path))["results"]:
            res.setdefault((r["name"], r["variant"]), r)
    rows = ["| Task | Presses before (Home / in game) | Presses after (Home / in game) | Fix |", "|---|---|---|---|"]

    def before(j, v):
        if v not in j:
            return "n/a"
        if j[v].get("was_none"):
            return "no way"
        if j[v].get("was_same"):
            return str(presses_of(j, v))
        if j[v].get("was_est"):
            return f"about {j[v]['was_est']}"
        return str(presses_of(j, v, True)) if "was" in j[v] else "same"

    def after(j, v):
        if v not in j:
            return "n/a"
        r = res.get((j["name"], v))
        return f"{presses_of(j, v)}" + ("" if r is None or r["ok"] else " (fails)")

    for j in js:
        rows.append(f"| {j['task']} | {before(j, 'home')} / {before(j, 'game')} | {after(j, 'home')} / {after(j, 'game')} | {j['fix']} |")
    return "\n".join(rows)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["run", "table", "lint"])
    ap.add_argument("--bin", default=DEFAULT_BIN)
    ap.add_argument("--device", default=DEFAULT_DEVICE)
    ap.add_argument("--out", default=None)
    ap.add_argument("--only", default="")
    ap.add_argument("--res", default="1280x800")
    ap.add_argument("--jobs", type=int, default=24)
    ap.add_argument("--post", default="")
    ap.add_argument("--commit", default="")
    ap.add_argument("--strict", action="store_true")
    ap.add_argument("--was", action="store_true")
    a = ap.parse_args()
    only = set(x for x in a.only.split(",") if x)
    js = load_journeys(only)
    errs = lint(js)
    if a.cmd == "lint":
        print("\n".join(errs) or f"{len(js)} journeys ok")
        return 1 if errs else 0
    if errs:
        print("\n".join(errs))
        return 1
    run_id = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    a.out = a.out or os.path.join(DEFAULT_OUT, run_id)
    if a.cmd == "table":
        print(table(js, os.path.join(a.out, "results.json")))
        return 0
    os.makedirs(a.out, exist_ok=True)
    jobs = [(j, v, r) for j in js for v in ("home", "game") if v in j and not (a.was and "was" not in j[v]) for r in a.res.split(",")]
    t0 = time.monotonic()
    results = []
    with cf.ThreadPoolExecutor(max_workers=a.jobs) as ex:
        for r in ex.map(lambda t: run_one(t[0], t[1], t[2], a), jobs):
            results.append(r)
            print(f"{'ok  ' if r['ok'] else 'FAIL'} {r['name']:<24} {r['variant']:<4} {r['res']:<9} {r['presses']} presses {r['secs']:>5} s" + ("" if r["ok"] else "  " + "; ".join(r["why"])), flush=True)
    took = round(time.monotonic() - t0, 1)
    fails = [r for r in results if not r["ok"]]
    json.dump({"run": run_id, "secs": took, "results": results}, open(os.path.join(a.out, "results.json"), "w"), indent=1)
    print(f"\n{len(results) - len(fails)} of {len(results)} passed in {took} s; {a.out}")
    if a.post:
        try:
            post(results, a.post, run_id, a.commit)
        except Exception as e:  # posting never fails the run
            print("post failed:", e)
    return 1 if (fails and a.strict) else 0


if __name__ == "__main__":
    sys.exit(main())
