#!/usr/bin/env python3
"""Read-only preflight on the device: changes nothing (no input, no capture, no restarts).
Prints whether everything the runner needs is there, and watches physical input for --watch seconds
so the idle guard can be seen working."""
import argparse
import glob
import json
import os
import re
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
sys.path.insert(0, HERE)
import runner as R  # noqa: E402
from jlib import spec  # noqa: E402

OK = []
BAD = []


def item(ok, what):
    (OK if ok else BAD).append(what)
    print(("  ok    " if ok else "  MISS  ") + what)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--watch", type=float, default=10)
    a = ap.parse_args()
    lp = R.launcher_pid()
    item(lp is not None, f"rare-launcher running (pid {lp})")
    app = None
    if lp:
        m = re.search(r"app-steam-app(\d+)-", open(f"/proc/{lp}/cgroup").read())
        app = m.group(1) if m else None
        item(True, f"launcher app id: {app or '1380012613 (STEAM_GAME atom; its scope does not carry one)'}")
        disp = R.proc_env(lp, "DISPLAY") or ":0"
        item(True, f"launcher DISPLAY {disp}, binary {R.Runner._launcher_version(lp)}")
    else:
        disp = ":0"
    item(R.Runner._proc_running(r"rare-session-client"), "rare-session-client (boot-home restarts only the launcher)")
    out = subprocess.run(["xprop", "-display", disp, "-root", "GAMESCOPE_FOCUSED_APP"], capture_output=True, text=True).stdout
    focus = (re.findall(r"\d+", out) or ["?"])[0]
    item(focus == (app or "1380012613"), f"GAMESCOPE_FOCUSED_APP {focus} (the launcher in front)")
    try:
        pad = R.IPPad()
        item(True, f"InputPlumber composite device {pad.path} (SendEvent allowed for wheel)")
    except Exception as e:  # noqa
        item(False, f"InputPlumber: {e}")
    try:
        import gi

        gi.require_version("Gst", "1.0")
        from gi.repository import Gst

        Gst.init(None)
        miss = [e for e in ("pipewiresrc", "appsink", "videoscale", "videoconvert", "vapostproc", "vah264enc", "h264parse", "matroskamux")
                if not Gst.ElementFactory.find(e)]
        item(not miss, "GStreamer elements" + (f" missing {miss}" if miss else ""))
        cap = R.GstCapture.__new__(R.GstCapture)
        item(True, f"gamescope PipeWire node {cap._node()}")
    except Exception as e:  # noqa
        item(False, f"capture: {e}")
    try:
        import evdev  # noqa

        item(True, "python-evdev")
    except ImportError:
        item(False, "python-evdev")
    item(bool(subprocess.run(["which", "ffmpeg"], capture_output=True).stdout), "ffmpeg (clips)")
    p = R.Power.__new__(R.Power)
    p.gm = (glob.glob("/sys/class/drm/card*/device/gpu_metrics") or [None])[0]
    p.pa = (glob.glob("/sys/class/drm/card*/device/hwmon/hwmon*/power1_average") or [None])[0]
    p.busy = (glob.glob("/sys/class/drm/card*/device/gpu_busy_percent") or [None])[0]

    class Sink:
        def write(self, s):
            self.s = s

    p.f = Sink()
    p.sample()
    item(True, "power sample t,socket,cpu,gfx,soc,busy,mhz = " + p.f.s.strip())
    rt = os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}")
    probe = os.path.exists(os.path.join(rt, "rare-journey.log"))
    item(True, f"journey probe log present from an earlier run: {probe} (the probe needs a launcher built with src/nav/journey.ts)")
    js = spec.load_all(os.path.join(os.path.dirname(HERE), "journeys"))
    item(True, f"{len(js)} journeys load: {', '.join(j['name'] for j in js)}")
    print(f"  irq   {R.irq_counts()}")

    # Physical input watch (read-only, no grab).
    class L:
        def emit(self, *a, **k):
            pass

        def say(self, m):
            print(m)

    class S:
        def abort(self, r):
            pass

    class Inj:
        def on_evdev(self, *a):
            return False

    class Rn:
        exempt_until = 0

    iw = R.InputWatch(L(), S(), Inj(), Rn())
    iw.start()
    time.sleep(2.5)
    print(f"  input devices watched: {sorted(d.name for d in iw.devs.values())}")
    seen = []
    end = time.monotonic() + a.watch
    last = None
    while time.monotonic() < end:
        if iw.last_physical != last and iw.last_physical:
            last = iw.last_physical
            seen.append(iw.last_desc)
        time.sleep(0.05)
    print(f"  physical input in {a.watch:g} s: {len(seen)} {'(idle)' if not seen else seen[:5]}")
    print(f"\n{len(OK)} ok, {len(BAD)} missing")
    return 1 if BAD else 0


if __name__ == "__main__":
    sys.exit(main())
