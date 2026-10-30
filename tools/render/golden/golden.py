#!/usr/bin/env python3
"""Golden images for the native launcher (Rare Render): render every screen state from fixtures,
diff against the approved PNGs, compare with the web launcher's screenshots, and gate frame budgets.

  golden.py build                       cargo build --release of every program the states use
  golden.py render [--res R,..] [--only ID,..]   render each state at each resolution (DISPLAY + Vulkan)
  golden.py diff                        compare with launcher/native/tests/golden/approved (exit 1 on a fail)
  golden.py approve [--only ..] [--res ..]       copy the rendered PNGs over the approved ones (after review)
  golden.py report [--web DIR]          report.html (golden diffs, native vs web) and parity.html
  golden.py ci [--web DIR]              render + diff + budget + report, inside the golden container
  golden.py container [--web DIR] [...] host side: build the binaries, then run `ci` in the Fedora 44
                                        lavapipe container (tools/render/golden/run.sh does the podman part)
Common: --out DIR (default target/golden), --res 1920x1200,1280x800,1920x1080,3840x2160.

States live in launcher/native/tests/golden/states/*.json (one file per screen area, owned by that
area's workstream). Approved images: launcher/native/tests/golden/approved/<WxH>/<state>.png.

The shot contract a program implements (the POC's rare-render and the native launcher):
  RR_SHOT=<png>        render offscreen once the screen is settled and write it (RGBA PNG)
  RR_SHOT_EXIT=1       exit after the shot
  RR_WINDOWED=WxH      window size (the harness runs one Xvfb screen big enough for 4K)
  RR_SCREEN=<name>     which screen state to build (programs with more than one)
  RR_SCRIPT=a,b,..     pad presses applied one by one, each after the screen settles
  RR_FIXTURE=<json>    the state's fixture: {"lib": {...}, "stream": {...}, ...} from
                       launcher/native/tests/fixtures, image URLs replaced by local placeholder files
  RR_LIBRARY=<json>    the same library as ~/.cache/rare/library.json ({"library": ...})
  RR_NO_SYSTEM_FONTS=1 bundled fonts only
  RR_STATS=<file>      frame log for tools/render/budget.py
  RR_TEXT_LOOK=chrome  set for the extra render compared with the web shot (Blink's text look)
  RR_CLOCK="9:41 AM"   the top bar's clock, for programs that take it from the environment
Wall clock: libfaketime pins it at the fixtures' instant (2026-10-27 09:41 in New York, TZ set),
the monotonic clock runs free so animations play and settle. No network in the container.
Programs may define more switches of their own; a state passes them in its "env".
"""
from __future__ import annotations

import argparse
import glob
import hashlib
import json
import os
import shutil
import signal
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "../../.."))
TESTS = os.path.join(REPO, "launcher/native/tests")
STATES_DIR = os.path.join(TESTS, "golden/states")
APPROVED = os.path.join(TESTS, "golden/approved")
FIXTURES = os.path.join(TESTS, "fixtures")
FONT = os.path.join(REPO, "launcher/render/assets/fonts/Inter-Bold.ttf")
SHOTS_MJS = os.path.join(REPO, "launcher/scripts/shots.mjs")
RESOLUTIONS = ["1920x1200", "1280x800", "1920x1080", "3840x2160"]
WEB_SIZES = ("1920x1200", "1280x800")  # what scripts/shots.mjs shoots
TZ = "America/New_York"
FAKETIME = "@2026-10-27 09:41:00"  # local time in TZ = index.json fixedMs (13:41 UTC)
CLOCK_TEXT = "9:41 AM"  # RR_CLOCK, for programs that draw the clock from it rather than libc
SHOT_TIMEOUT = 90

sys.path.insert(0, HERE)


def say(*a):
    print(*a, flush=True)


# ---------------------------------------------------------------- states

def load_states():
    progs, states = {}, []
    for f in sorted(glob.glob(os.path.join(STATES_DIR, "*.json"))):
        d = json.load(open(f))
        for k, v in (d.get("programs") or {}).items():
            if k in progs and progs[k] != v:
                raise SystemExit(f"{f}: program {k} defined twice with different settings")
            progs[k] = v
        for s in d.get("states") or []:
            s = dict(s)
            s["file"] = os.path.relpath(f, REPO)
            if any(x["id"] == s["id"] for x in states):
                raise SystemExit(f"{f}: state id {s['id']} is used twice")
            states.append(s)
    for s in states:
        if s["program"] not in progs:
            raise SystemExit(f"{s['file']}: state {s['id']} names unknown program {s['program']}")
    return progs, states


def fixture_index():
    return json.load(open(os.path.join(FIXTURES, "index.json")))


def fixture_set(s, idx):
    """The fixture set of a state: its own 'fixture', else the one its web shot uses, else default."""
    return s.get("fixture") or idx["shots"].get(s.get("web") or "", "default")


def merged_fixture(name, idx):
    mods = idx["sets"][name]["modules"]
    return {m: json.load(open(os.path.join(FIXTURES, m, f"{v}.json"))) for m, v in mods.items()}


def web_shots():
    """Every screen state the web launcher has: the names in every shot script (shots.mjs, which
    also runs shots-wallet-*.mjs, then the area scripts shots-*.mjs, *-shots.mjs)."""
    import re

    names = []
    d = os.path.dirname(SHOTS_MJS)
    srcs = [SHOTS_MJS] + sorted(set(glob.glob(os.path.join(d, "shots-*.mjs")) + glob.glob(os.path.join(d, "*-shots.mjs"))))
    for p in srcs:
        for m in re.finditer(r'\{\s*name: "([^"]+)"', open(p).read()):
            if m.group(1) not in names:
                names.append(m.group(1))
    return names


def selected(states, only):
    if not only:
        return states
    want = set(only.split(","))
    return [s for s in states if s["id"] in want or s["program"] in want]


def res_list(a):
    return [r for r in (a.res or ",".join(RESOLUTIONS)).split(",") if r]


# ---------------------------------------------------------------- build

def target_dir(manifest):
    t = os.environ.get("CARGO_TARGET_DIR")
    if t:
        return t
    out = subprocess.run(["cargo", "metadata", "--format-version", "1", "--no-deps", "--manifest-path", manifest],
                         capture_output=True, text=True, check=True).stdout
    return json.loads(out)["target_directory"]


def cmd_build(a):
    progs, states = load_states()
    used = {s["program"] for s in selected(states, a.only)}
    bindir = os.path.join(a.out, "bin")
    os.makedirs(bindir, exist_ok=True)
    failed = []
    for name in sorted(used):
        p = progs[name]
        manifest = os.path.join(REPO, p["manifest"])
        say(f"build {name}: cargo build --release --bin {p['bin']} ({p['manifest']})")
        dst = os.path.join(bindir, name)
        if os.path.exists(dst):
            os.unlink(dst)
        # One program failing to build must not hide the others' results: its states show the error.
        r = subprocess.run(["cargo", "build", "--release", "--manifest-path", manifest, "--bin", p["bin"]])
        if r.returncode != 0:
            failed.append(name)
            continue
        shutil.copy2(os.path.join(target_dir(manifest), "release", p["bin"]), dst)
    if failed:
        say(f"build failed: {', '.join(failed)}")
    return 1 if failed else 0


# ---------------------------------------------------------------- render

def start_xvfb():
    """One X screen big enough for every resolution; returns (proc, display)."""
    if os.environ.get("DISPLAY") and not os.environ.get("RR_GOLDEN_XVFB"):
        return None, os.environ["DISPLAY"]
    if not shutil.which("Xvfb"):
        return None, ""  # headless programs (Gpu::headless) need no display
    disp = ":97"
    p = subprocess.Popen(["Xvfb", disp, "-screen", "0", "3840x2160x24", "-nolisten", "tcp", "-noreset"],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    for _ in range(100):
        if os.path.exists(f"/tmp/.X11-unix/X{disp[1:]}"):
            break
        time.sleep(0.05)
    return p, disp


def faketime_lib():
    for p in ("/usr/lib64/faketime/libfaketime.so.1", "/usr/lib/x86_64-linux-gnu/faketime/libfaketime.so.1", "/usr/lib/faketime/libfaketime.so.1"):
        if os.path.exists(p):
            return p
    return None


def shot_env(a, s, res, fx_path, lib_path, png, stats, home):
    w, h = res.split("x")
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": home,
        "XDG_RUNTIME_DIR": home,
        "XDG_CACHE_HOME": os.path.join(home, "cache"),
        "DISPLAY": a.display,
        "TZ": TZ,
        "LANG": "C.UTF-8",
        "RR_SHOT": png,
        "RR_SHOT_EXIT": "1",
        "RR_WINDOWED": f"{w}x{h}",
        "RR_NO_SYSTEM_FONTS": "1",
        "RR_FIXTURE": fx_path,
        "RR_LIBRARY": lib_path,
        "RR_STATS": stats,
        "RR_CLOCK": CLOCK_TEXT,
        "RUST_BACKTRACE": "1",
    }
    # The container sets WGPU_BACKEND=vulkan and the lavapipe ICD; elsewhere wgpu picks.
    for k in ("WGPU_BACKEND", "VK_DRIVER_FILES", "VK_ICD_FILENAMES", "LIBGL_ALWAYS_SOFTWARE", "LP_NUM_THREADS"):
        if os.environ.get(k):
            env[k] = os.environ[k]
    if s.get("screen"):
        env["RR_SCREEN"] = s["screen"]
    if s.get("script"):
        env["RR_SCRIPT"] = s["script"]
    for k, v in (s.get("env") or {}).items():
        env[k] = str(v)
    ft = faketime_lib()
    if ft and not a.real_clock:
        env.update(LD_PRELOAD=ft, FAKETIME=FAKETIME, FAKETIME_DONT_FAKE_MONOTONIC="1", FAKETIME_DONT_RESET="1")
    return env


def run_shot(binary, env, timeout, log_path):
    t0 = time.monotonic()
    with open(log_path, "w") as log:
        p = subprocess.Popen([binary], env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = p.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(p.pid, signal.SIGKILL)
            p.wait()
            code = "timeout"
    return code, round(time.monotonic() - t0, 2)


def cmd_render(a):
    from art import localize, titles_from

    progs, states = load_states()
    idx = fixture_index()
    todo = selected(states, a.only)
    xvfb, a.display = start_xvfb()
    work = os.path.join(a.out, "work")
    os.makedirs(work, exist_ok=True)
    results = load_results(a.out)
    try:
        for s in todo:
            setname = fixture_set(s, idx)
            raw = merged_fixture(setname, idx)
            fx = localize(raw, os.path.join(work, "art"), FONT, titles_from(raw))
            fx_path = os.path.join(work, f"fixture-{setname}.json")
            json.dump(fx, open(fx_path, "w"))
            lib_path = os.path.join(work, f"library-{setname}.json")
            json.dump({"library": (fx.get("lib") or {}).get("library")}, open(lib_path, "w"))
            binary = os.path.join(a.out, "bin", s["program"])
            for res in s.get("res") or res_list(a):
                if res not in res_list(a):
                    continue
                os.makedirs(os.path.join(a.out, res), exist_ok=True)
                os.makedirs(os.path.join(a.out, "logs", res), exist_ok=True)
                png = os.path.join(a.out, res, f"{s['id']}.png")
                stats = os.path.join(a.out, "logs", res, f"{s['id']}.stats")
                home = os.path.join(work, "home", s["id"], res)
                shutil.rmtree(home, ignore_errors=True)
                os.makedirs(home)
                for p in (png, stats):
                    if os.path.exists(p):
                        os.unlink(p)
                env = shot_env(a, s, res, fx_path, lib_path, png, stats, home)
                if not os.path.exists(binary):
                    open(os.path.join(a.out, "logs", res, f"{s['id']}.log"), "w").write(f"{s['program']} did not build (see the build step)\n")
                    code, secs = "no binary", 0.0
                else:
                    code, secs = run_shot(binary, env, s.get("timeout", SHOT_TIMEOUT), os.path.join(a.out, "logs", res, f"{s['id']}.log"))
                ok = os.path.exists(png)
                r = results.setdefault(s["id"], {}).setdefault(res, {})
                r.update(render="ok" if ok else "error", exit=code, secs=secs, fixture=setname)
                if not ok:
                    r["render_error"] = tail(os.path.join(a.out, "logs", res, f"{s['id']}.log"))
                say(f"render {s['id']:<28} {res:>9}  {'ok' if ok else 'ERROR'}  {secs:5.1f} s  exit {code}")
                # The comparison with the web shot uses Chrome's text look (unhinted, Blink
                # baseline; WS 2): one more render at the web sizes, kept apart from the golden.
                if ok and s.get("web") and res in WEB_SIZES and os.path.exists(binary):
                    wl = os.path.join(a.out, "weblook", res)
                    os.makedirs(wl, exist_ok=True)
                    env2 = dict(env, RR_SHOT=os.path.join(wl, f"{s['id']}.png"), RR_STATS="", RR_TEXT_LOOK="chrome")
                    run_shot(binary, env2, s.get("timeout", SHOT_TIMEOUT), os.path.join(a.out, "logs", res, f"{s['id']}.weblook.log"))
    finally:
        if xvfb:
            xvfb.terminate()
        save_results(a.out, results)
    bad = [(sid, res) for sid, rs in results.items() for res, r in rs.items() if r.get("render") == "error"]
    return 1 if bad else 0


def tail(path, n=12):
    try:
        return "".join(open(path, errors="replace").readlines()[-n:])
    except OSError:
        return ""


def load_results(out):
    p = os.path.join(out, "results.json")
    return json.load(open(p)) if os.path.exists(p) else {}


def save_results(out, r):
    json.dump(r, open(os.path.join(out, "results.json"), "w"), indent=1, sort_keys=True)


# ---------------------------------------------------------------- diff / approve

def cmd_diff(a):
    import imgdiff

    _, states = load_states()
    results = load_results(a.out)
    fails = 0
    for s in selected(states, a.only):
        for res in s.get("res") or res_list(a):
            if res not in res_list(a):
                continue
            r = results.setdefault(s["id"], {}).setdefault(res, {})
            png = os.path.join(a.out, res, f"{s['id']}.png")
            ap = os.path.join(APPROVED, res, f"{s['id']}.png")
            if not os.path.exists(png):
                r["golden"] = "no render"
                fails += 1
                continue
            if not os.path.exists(ap):
                r["golden"] = "unapproved"
                say(f"diff   {s['id']:<28} {res:>9}  unapproved (golden.py approve --only {s['id']} after review)")
                continue
            os.makedirs(os.path.join(a.out, "diff", res), exist_ok=True)
            d = imgdiff.compare(png, ap, os.path.join(a.out, "diff", res, f"{s['id']}.png"))
            r["golden"] = d["status"]
            r["golden_detail"] = d
            if d["status"] == "fail":
                fails += 1
                os.makedirs(os.path.join(a.out, "approved", res), exist_ok=True)
                shutil.copy2(ap, os.path.join(a.out, "approved", res, f"{s['id']}.png"))
            say(f"diff   {s['id']:<28} {res:>9}  {d['status']}  {d.get('reason', '')}")
    save_results(a.out, results)
    return 1 if fails else 0


def cmd_approve(a):
    _, states = load_states()
    n = 0
    for s in selected(states, a.only):
        for res in s.get("res") or res_list(a):
            if res not in res_list(a):
                continue
            png = os.path.join(a.out, res, f"{s['id']}.png")
            if not os.path.exists(png):
                continue
            dst = os.path.join(APPROVED, res, f"{s['id']}.png")
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            from PIL import Image

            Image.open(png).convert("RGB").save(dst, optimize=True)
            n += 1
            say(f"approved {os.path.relpath(dst, REPO)}")
    say(f"{n} images approved; commit them with the change that moved them")
    return 0


# ---------------------------------------------------------------- budget

def cmd_budget(a):
    sys.path.insert(0, os.path.join(HERE, ".."))
    import budget

    _, states = load_states()
    results = load_results(a.out)
    fails = 0
    for s in selected(states, a.only):
        if not s.get("budget"):
            continue
        res = s.get("budget_res", "1920x1200")
        log = os.path.join(a.out, "logs", res, f"{s['id']}.stats")
        if not os.path.exists(log) or not os.path.getsize(log):
            if os.path.exists(os.path.join(a.out, res, f"{s['id']}.png")):
                # Rendered but no frame log: the program does not write RR_STATS in shot mode yet.
                results.setdefault(s["id"], {}).setdefault(res, {})["budget"] = {
                    "ok": True, "missing": True, "gates": {"stats": {"ok": False, "gating": False, "detail": "no RR_STATS log from the shot run"}}}
                say(f"budget {s['id']:<28} {res:>9}  no RR_STATS log (not gated)")
            continue
        b = budget.check(log, device=False)
        results.setdefault(s["id"], {}).setdefault(res, {})["budget"] = b
        fails += not b["ok"]
        say(f"budget {s['id']:<28} {res:>9}  {'ok' if b['ok'] else 'FAIL'}  " + "; ".join(f"{k}: {g['detail']}" for k, g in b["gates"].items() if not g["ok"]))
    save_results(a.out, results)
    return 1 if fails else 0


# ---------------------------------------------------------------- report

def cmd_report(a):
    import report

    progs, states = load_states()
    report.write(a.out, states, load_results(a.out), web_shots(), fixture_index(), a.web, res_list(a), REPO, APPROVED)
    say(f"report: {os.path.join(a.out, 'index.html')}")
    return 0


# ---------------------------------------------------------------- ci / container

def env_info():
    info = {}
    try:
        info["vulkan"] = subprocess.run(["vulkaninfo", "--summary"], capture_output=True, text=True, timeout=20).stdout[-1500:]
    except Exception as e:  # noqa
        info["vulkan"] = f"vulkaninfo: {e}"
    try:
        info["mesa"] = subprocess.run(["rpm", "-q", "mesa-vulkan-drivers"], capture_output=True, text=True, timeout=10).stdout.strip()
    except Exception:
        pass
    try:
        info["os"] = open("/etc/os-release").read().split("PRETTY_NAME=")[1].split("\n")[0].strip('"')
    except Exception:
        pass
    return info


def cmd_ci(a):
    os.makedirs(a.out, exist_ok=True)
    json.dump(env_info(), open(os.path.join(a.out, "env.json"), "w"), indent=1)
    rc_render = cmd_render(a)
    rc_diff = cmd_diff(a)
    rc_budget = cmd_budget(a)
    cmd_report(a)
    say(f"golden ci: render {'ok' if not rc_render else 'FAILED'}, diff {'ok' if not rc_diff else 'FAILED'}, budget {'ok' if not rc_budget else 'FAILED'}")
    return 1 if (rc_render or rc_diff or rc_budget) else 0


def cmd_container(a):
    if not a.no_build:
        cmd_build(a)
    args = ["bash", os.path.join(HERE, "run.sh"), os.path.abspath(a.out)]
    if a.web:
        args.append(os.path.abspath(a.web))
    extra = []
    if a.only:
        extra += ["--only", a.only]
    if a.res:
        extra += ["--res", a.res]
    env = dict(os.environ, GOLDEN_ARGS=" ".join(extra))
    return subprocess.run(args, env=env).returncode


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("cmd", choices=["build", "render", "diff", "approve", "budget", "report", "ci", "container"])
    ap.add_argument("--out", default=os.path.join(REPO, "target/golden"))
    ap.add_argument("--res", default="")
    ap.add_argument("--only", default="")
    ap.add_argument("--web", default="", help="web screenshots (scripts/shots.mjs output) for the side-by-side report")
    ap.add_argument("--real-clock", action="store_true", help="do not pin the wall clock with libfaketime")
    ap.add_argument("--no-build", action="store_true", help="container: use the binaries already in OUT/bin")
    a = ap.parse_args()
    a.out = os.path.abspath(a.out)
    a.display = os.environ.get("DISPLAY", "")
    return {"build": cmd_build, "render": cmd_render, "diff": cmd_diff, "approve": cmd_approve, "budget": cmd_budget,
            "report": cmd_report, "ci": cmd_ci, "container": cmd_container}[a.cmd](a)


if __name__ == "__main__":
    sys.exit(main())
