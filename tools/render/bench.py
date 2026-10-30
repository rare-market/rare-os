#!/usr/bin/env python3
"""Device side of the Rare Render measurements (same rules as tools/journeys/flash: copy next to
pad.py, beat.py, idle.py and guard-run.sh in ~/flashcap and run only under a live idle guard).

  bench.py press <label> <step>...   capture gamescope's output while a virtual pad presses steps;
                                     writes <label>.raw/.pts (flash/analyze.py reads them) and
                                     <label>.presses.json (press times); analyse on the Mac with
                                     tools/render/latency.py (the device has no numpy).
          steps: a b x y lb rb up down left right, hold:<button>:<secs>, wait:<secs>
  bench.py idle <secs> <regex>       still screen: CPU % of one core for processes matching regex
                                     (plus both Xwayland), gamescope separately, gpu_busy, APU W,
                                     PSS and GPU memory (amdgpu fdinfo) of the matching processes
  bench.py cold <label> <old> [new]  restarts the launcher (rare-session-client keeps the session)
                                     with the capture running; latency.py cold reads the result

Every press goes out only while the guard's heartbeat is fresh (beat.check), exactly like pad.py;
a lost guard stops the run, removes the pad and calls beat.lost() (restore while armed).
All times are CLOCK_MONOTONIC.
"""
import importlib.util, json, os, re, signal, subprocess, sys, threading, time

HERE = os.path.dirname(os.path.realpath(__file__))
W, H = 640, 400


def load(name):
    spec = importlib.util.spec_from_file_location("rr_" + name, os.path.join(HERE, name + ".py"))
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


beat = load("beat")
pad = load("pad")
now = time.monotonic_ns


class GuardLost(Exception):
    pass


def guard_ok():
    why = beat.check()
    if why:
        raise GuardLost(why)


def pause(secs):
    end = time.monotonic() + secs
    while time.monotonic() < end:
        guard_ok()
        time.sleep(min(0.02, max(0.0, end - time.monotonic())))


class Capture:
    """pipewiresrc on gamescope's node, 640x400 RGB, CLOCK_MONOTONIC stamps (runner.py's method)."""

    def __init__(self, out, label):
        import gi
        gi.require_version("Gst", "1.0")
        from gi.repository import Gst
        Gst.init(None)
        self.Gst = Gst
        self.raw = open(os.path.join(out, label + ".raw"), "wb")
        self.pts = []
        self.lock = threading.Lock()
        node = self._node()
        desc = (f"pipewiresrc path={node} do-timestamp=true keepalive-time=0 ! queue max-size-buffers=3 leaky=downstream ! video/x-raw "
                f"! videoscale method=bilinear ! video/x-raw,width={W},height={H} ! videoconvert ! video/x-raw,format=RGB,width={W},height={H} "
                f"! appsink name=s emit-signals=true sync=false max-buffers=240 drop=false")
        self.pipe = Gst.parse_launch(desc)
        clock = Gst.SystemClock.obtain()
        clock.set_property("clock-type", Gst.ClockType.MONOTONIC)
        self.pipe.use_clock(clock)
        self.pipe.get_by_name("s").connect("new-sample", self._on)

    def _node(self):
        d = json.loads(subprocess.run(["pw-dump"], capture_output=True, text=True, timeout=10).stdout or "[]")
        for o in d:
            p = (o.get("info") or {}).get("props") or {}
            if o.get("type", "").endswith(":Node") and p.get("node.name") == "gamescope" and p.get("media.class") == "Video/Source":
                return o["id"]
        raise RuntimeError("no gamescope PipeWire node")

    def _on(self, sink):
        Gst = self.Gst
        s = sink.emit("pull-sample")
        if s is None:
            return Gst.FlowReturn.OK
        arr = now()
        buf = s.get_buffer()
        ok, info = buf.map(Gst.MapFlags.READ)
        if not ok:
            return Gst.FlowReturn.OK
        try:
            data = bytes(info.data)
        finally:
            buf.unmap(info)
        if len(data) != W * H * 3:
            stride = len(data) // H
            data = b"".join(data[y * stride:y * stride + W * 3] for y in range(H))
        t = arr
        if buf.pts != Gst.CLOCK_TIME_NONE:
            cand = self.pipe.get_base_time() + buf.pts
            if 0 <= arr - cand < 500_000_000:
                t = cand
        with self.lock:
            self.raw.write(data)
            self.pts.append(t)
        return Gst.FlowReturn.OK

    def start(self):
        self.pipe.set_state(self.Gst.State.PLAYING)
        end = time.monotonic() + 5
        while time.monotonic() < end and not self.pts:
            time.sleep(0.05)

    def stop(self, out, label):
        Gst = self.Gst
        self.pipe.send_event(Gst.Event.new_eos())
        self.pipe.get_bus().timed_pop_filtered(5 * Gst.SECOND, Gst.MessageType.EOS | Gst.MessageType.ERROR)
        self.pipe.set_state(Gst.State.NULL)
        with self.lock:
            self.raw.close()
            open(os.path.join(out, label + ".pts"), "w").write("\n".join(f"{t / 1e9:.6f}" for t in self.pts) + "\n")
            return list(self.pts)


def cmd_press(label, steps):
    out = os.path.join(os.environ.get("FLASHCAP") or os.path.expanduser("~/flashcap"), label)
    os.makedirs(out, exist_ok=True)
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
    guard_ok()
    fd = pad.create()
    cap = Capture(out, label)
    presses, lost = [], None
    try:
        pause(float(os.environ.get("PAD_SETTLE", "3")))
        cap.start()
        pause(0.8)
        gap = float(os.environ.get("PAD_GAP", "0.6"))
        for st in steps:
            if st.startswith("wait:"):
                pause(float(st[5:]))
                continue
            guard_ok()
            hold = 0.06
            name = st
            if st.startswith("hold:"):
                _, name, h = st.split(":")
                hold = float(h)
            t = now()
            ev = name if name in pad.HAT else None
            pad.press(fd, [name], hold, ok=lambda: beat.check() is None)
            presses.append({"step": st, "t": t})
            pause(gap)
        pause(1.0)
    except GuardLost as e:
        lost = str(e)
    finally:
        import fcntl
        fcntl.ioctl(fd, pad.UI_DEV_DESTROY)
        os.close(fd)
        pts = cap.stop(out, label)
    if lost:
        print("stop:", lost)
        beat.lost(lost)
        sys.exit(4)
    json.dump(presses, open(os.path.join(out, label + ".presses.json"), "w"))
    print(f"{label}: {len(pts)} frames, {len(presses)} presses in {out}")


def pids(rx):
    r = re.compile(rx)
    out = []
    for p in os.listdir("/proc"):
        if p.isdigit():
            try:
                comm = open(f"/proc/{p}/comm").read().strip()
                cmd = open(f"/proc/{p}/cmdline", "rb").read().replace(b"\0", b" ").decode(errors="replace")
            except OSError:
                continue
            if r.search(comm) or r.search(cmd.split(" ")[0]):
                out.append(int(p))
    return out


def ticks(ps):
    t = 0
    for p in ps:
        try:
            f = open(f"/proc/{p}/stat").read().rsplit(")", 1)[1].split()
            t += int(f[11]) + int(f[12])
        except OSError:
            pass
    return t


def pss_kib(ps):
    t = 0
    for p in ps:
        try:
            for l in open(f"/proc/{p}/smaps_rollup"):
                if l.startswith("Pss:"):
                    t += int(l.split()[1])
        except OSError:
            pass
    return t


def gpu_mem_kib(ps):
    """amdgpu fdinfo: drm-memory-vram + drm-memory-gtt per DRM client, summed (each client once)."""
    seen, t = set(), 0
    for p in ps:
        try:
            fds = os.listdir(f"/proc/{p}/fdinfo")
        except OSError:
            continue
        for fd in fds:
            try:
                s = open(f"/proc/{p}/fdinfo/{fd}").read()
            except OSError:
                continue
            m = re.search(r"drm-client-id:\s+(\d+)", s)
            if not m or m.group(1) in seen:
                continue
            seen.add(m.group(1))
            for k in ("drm-memory-vram", "drm-memory-gtt"):
                mm = re.search(k + r":\s+(\d+)\s*(\w+)", s)
                if mm:
                    t += int(mm.group(1)) * {"KiB": 1, "MiB": 1024, "GiB": 1024 * 1024}.get(mm.group(2), 1)
    return t


def cmd_idle(secs, rx):
    secs = float(secs)
    ps = pids(rx)
    xw = pids(r"^Xwayland$")
    gs = pids(r"^gamescope")
    hz = os.sysconf("SC_CLK_TCK")
    dev = "/sys/class/drm/card1/device"
    hw = next((f"/sys/class/hwmon/{h}" for h in os.listdir("/sys/class/hwmon") if open(f"/sys/class/hwmon/{h}/name").read().strip() == "amdgpu"), None)
    t0, x0, g0 = ticks(ps), ticks(xw), ticks(gs)
    busy, watts, n = 0, 0, 0
    end = time.monotonic() + secs
    while time.monotonic() < end:
        guard_ok()
        busy += int(open(f"{dev}/gpu_busy_percent").read())
        if hw:
            watts += int(open(f"{hw}/power1_average").read())
        n += 1
        time.sleep(0.5)
    t1, x1, g1 = ticks(ps), ticks(xw), ticks(gs)
    comms = {}
    for p in ps:
        try:
            c = open(f"/proc/{p}/comm").read().strip()
            comms[c] = comms.get(c, 0) + 1
        except OSError:
            pass
    r = {"procs": comms, "cpu_pct": round((t1 - t0) / hz / secs * 100, 1), "xwayland_pct": round((x1 - x0) / hz / secs * 100, 1),
         "gamescope_pct": round((g1 - g0) / hz / secs * 100, 1), "gpu_busy_avg": round(busy / n, 1), "apu_w_avg": round(watts / n / 1e6, 2),
         "pss_mib": round(pss_kib(ps) / 1024, 1), "gpu_mem_mib": round(gpu_mem_kib(ps) / 1024, 1)}
    print(json.dumps(r))


def cmd_cold(label, rx, new_rx=None):
    out = os.path.join(os.environ.get("FLASHCAP") or os.path.expanduser("~/flashcap"), label)
    os.makedirs(out, exist_ok=True)
    run = os.environ.get("XDG_RUNTIME_DIR", "/run/user/1000")
    guard_ok()
    old = pids(rx)
    cap = Capture(out, label)
    cap.start()
    pause(1.0)
    open(os.path.join(run, "rare-launcher-restart"), "w").close()
    t_kill = now()
    for p in old:
        try:
            os.kill(p, signal.SIGTERM)
        except OSError:
            pass
    t_new = None
    end = time.monotonic() + 20
    while time.monotonic() < end:
        ps = [p for p in pids(new_rx or rx) if p not in old]
        if ps:
            t_new = now()
            break
        time.sleep(0.005)
    try:
        pause(12.0)
    except GuardLost as e:
        pts = cap.stop(out, label)
        beat.lost(str(e))
        sys.exit(4)
    pts = cap.stop(out, label)
    r = {"label": label, "t_kill": t_kill, "t_new": t_new, "frames": len(pts)}
    json.dump(r, open(os.path.join(out, label + ".cold.json"), "w"))
    print(json.dumps(r))


if __name__ == "__main__":
    a = sys.argv[1:]
    try:
        if a[0] == "press":
            cmd_press(a[1], a[2:])
        elif a[0] == "idle":
            cmd_idle(a[1], a[2])
        elif a[0] == "cold":
            cmd_cold(a[1], a[2], a[3] if len(a) > 3 else None)
        else:
            raise SystemExit(__doc__)
    except GuardLost as e:
        print("stop:", e)
        beat.lost(str(e))
        sys.exit(4)
