#!/usr/bin/env python3
"""Rare OS journey runner. Runs ON the SuiPlay (or with --mock anywhere) and drives the real UI:
controller input through InputPlumber, frames from gamescope's PipeWire node, focus from
GAMESCOPE_FOCUSED_APP, the launcher's journey probe, power from amdgpu gpu_metrics, crashes from
coredumpctl / panic logs / process exits. Writes a run directory for jlib.analyze.

Started by tools/journeys/run.sh. It refuses to run without --i-have-a-window.

Safety: hold L3 + R3 for 1 s on the device to abort; the file STOP in the run's parent directory
aborts too; every journey has a hard timeout; after every journey (and on any exit) it releases
all buttons, quits what the journey started and returns to the launcher's Home.
The input guard (InputWatch) is a hard dependency: a press, stick move or key goes out only while
the guard is alive with a heartbeat under 1 s old. If it exits or stalls, or the owner touches the
device, the run aborts at once and sends nothing more (releases only); cleanup then restores through
the probe and by stopping what the journey started, without pressing anything.
"""
from __future__ import annotations

import argparse
import atexit
import fcntl
import glob
import json
import os
import random
import re
import signal
import struct
import subprocess
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
from jlib import frames as F  # noqa: E402
from jlib import spec  # noqa: E402


def now() -> int:
    return time.monotonic_ns()


class Abort(Exception):
    pass


class StepFail(Exception):
    def __init__(self, reason, info=None):
        super().__init__(reason)
        self.info = info or {}


class Skip(Exception):
    pass


# ---------------------------------------------------------------- event log

class Log:
    def __init__(self, out):
        self.f = open(os.path.join(out, "events.jsonl"), "a", buffering=1)
        self.lock = threading.Lock()
        self.ctx = {}

    def emit(self, type_, t=None, **kw):
        e = {"t": t if t is not None else now(), "type": type_}
        e.update(kw)
        with self.lock:
            self.f.write(json.dumps(e, default=str) + "\n")
        return e

    def say(self, msg):
        sys.stdout.write(time.strftime("%H:%M:%S ") + msg + "\n")
        sys.stdout.flush()


# ---------------------------------------------------------------- safety

# Physical-input quiet time (interrupts) before the first injected event (owner's rule, 2026-10-27).
IRQ_IDLE_S = 60


class _NoPad:
    """Before the device is idle: refuses every press (nothing may reach the device yet)."""

    held = set()

    def set(self, button, down):
        if down:
            raise Abort("no input before the idle check")

    def stick(self, side, x, y):
        raise Abort("no input before the idle check")

    def release_all(self):
        pass

class Safety:
    def __init__(self, log, stop_file, max_s):
        self.log = log
        self.stop_file = stop_file
        self.deadline_run = now() + int(max_s * 1e9)
        self.reason = None
        self.journey_deadline = None
        self.ev = threading.Event()
        # The input guard (InputWatch, MockGuard in --mock) is a hard dependency: once it is
        # registered, the run aborts as soon as it exits or its heartbeat is 1 s old.
        self.guard = None
        # Why no more input may go out at all (the guard is gone, or the owner touched the device).
        # Set once, never cleared: cleanup then restores without pressing anything.
        self.lockout = None

    def abort(self, reason):
        if self.reason is None:
            self.reason = reason
            self.log.emit("abort", reason=reason)
            self.log.say(f"ABORT: {reason}")
        self.ev.set()

    def lock(self, reason):
        """Abort and refuse every further press (releases still go out)."""
        if self.lockout is None:
            self.lockout = reason
            self.log.emit("input_lockout", reason=reason)
        self.abort(reason)

    def guard_ok(self):
        """None while input may go out, else why not."""
        if self.lockout:
            return self.lockout
        why = self.guard.dead() if self.guard is not None else "no input guard"
        if why:
            self.lock(why)
        return why

    def check(self):
        if self.guard is not None and self.lockout is None:
            self.guard_ok()
        if self.reason is None and self.stop_file and os.path.exists(self.stop_file):
            self.abort("stop file")
        if self.reason is None and now() > self.deadline_run:
            self.abort("run time limit")
        if self.reason:
            raise Abort(self.reason)
        if self.journey_deadline and now() > self.journey_deadline:
            raise Abort("journey timeout")

    def sleep(self, ms):
        end = now() + int(ms * 1e6)
        while True:
            self.check()
            left = (end - now()) / 1e9
            if left <= 0:
                return
            self.ev.wait(min(left, 0.02))


# ---------------------------------------------------------------- input

class IPPad:
    """InputPlumber CompositeDevice.SendEvent: the same path the physical buttons take."""

    def __init__(self):
        from gi.repository import Gio, GLib  # noqa

        self.GLib = GLib
        self.bus = Gio.bus_get_sync(Gio.BusType.SYSTEM, None)
        self.path = self._find()
        self.held = set()

    def _call(self, path, iface, method, args, sig_reply=None):
        return self.bus.call_sync("org.shadowblip.InputPlumber", path, iface, method, args, sig_reply, 0, 1000, None)

    def _find(self):
        xml = self._call("/org/shadowblip/InputPlumber", "org.freedesktop.DBus.Introspectable", "Introspect", None).unpack()[0]
        names = re.findall(r'<node name="(CompositeDevice\d+)"', xml)
        if not names:
            raise RuntimeError("InputPlumber has no composite device")
        for n in names:
            p = "/org/shadowblip/InputPlumber/" + n
            caps = self._call(p, "org.freedesktop.DBus.Properties", "Get",
                              self.GLib.Variant("(ss)", ("org.shadowblip.Input.CompositeDevice", "Capabilities"))).unpack()[0]
            if any(c.startswith("Gamepad:") for c in caps):
                return p
        return "/org/shadowblip/InputPlumber/" + names[0]

    def send(self, cap, value):
        V = self.GLib.Variant
        if isinstance(value, bool):
            v = V("b", value)
        elif isinstance(value, (list, tuple)):
            v = V("ad", [float(x) for x in value])
        else:
            v = V("d", float(value))
        self._call(self.path, "org.shadowblip.Input.CompositeDevice", "SendEvent", V("(sv)", (cap, v)))

    def set(self, button, down):
        cap = spec.BUTTONS[button]
        self.send(cap, (1.0 if down else 0.0) if ":Trigger:" in cap else bool(down))
        (self.held.add if down else self.held.discard)(button)

    def stick(self, side, x, y):
        self.send("Gamepad:Axis:LeftStick" if side == "left" else "Gamepad:Axis:RightStick", [x, y])

    def release_all(self):
        # Directions and triggers always (a press whose call timed out may have landed), then
        # whatever else is down.
        for b in sorted(set(self.held) | {b for b in ("up", "down", "left", "right", "lt", "rt") if b in spec.BUTTONS}):
            try:
                self.set(b, False)
            except Exception:
                pass
        for side in ("left", "right"):
            try:
                self.stick(side, 0.0, 0.0)
            except Exception:
                pass

    def clear_target(self):
        """InputPlumber's own virtual pad back to neutral without SendEvent: a press whose release
        call failed (0.83.0's SendEvent panics and times out) stays down on that pad, and the
        launcher reads it as held. Entering the Always intercept mode clears every target's state;
        the mode it was in is put back at once."""
        V, iface = self.GLib.Variant, "org.shadowblip.Input.CompositeDevice"
        props = "org.freedesktop.DBus.Properties"
        cur = self._call(self.path, props, "Get", V("(ss)", (iface, "InterceptMode"))).unpack()[0]
        if cur != 2:
            self._call(self.path, props, "Set", V("(ssv)", (iface, "InterceptMode", V("u", 2))))
            time.sleep(0.1)
            self._call(self.path, props, "Set", V("(ssv)", (iface, "InterceptMode", V("u", cur))))

    def close(self):
        self.release_all()


# Physical input from /proc/interrupts, the check tools/release/build-native.sh and native-install.sh use before they
# restart the launcher: keyboard (i8042), touchscreen (Goodix) and pad interrupts. Injected input
# (InputPlumber, uinput) never raises these, so any change is a person touching the device.
IRQ_RX = re.compile(r"i8042|GDIX|goodix|xpad", re.I)


def irq_counts():
    out = {}
    try:
        for line in open("/proc/interrupts"):
            if not IRQ_RX.search(line):
                continue
            parts = line.split()
            out[parts[-1]] = sum(int(x) for x in parts[1:] if x.isdigit())
    except Exception:
        pass
    return out


class IrqWatch(threading.Thread):
    """Locks input out and releases everything the moment a physical-input interrupt fires."""

    def __init__(self, log, safety, release, runner=None):
        super().__init__(daemon=True)
        self.log, self.safety, self.release, self.runner = log, safety, release, runner
        self.base = irq_counts()
        self.running = True

    def run(self):
        while self.running:
            time.sleep(0.1)
            c = irq_counts()
            moved = [k for k in c if c[k] != self.base.get(k, c[k])]
            if moved and self.runner is not None and now() < getattr(self.runner, "exempt_until", 0):
                # An attended step (sleep and wake): the owner's press is expected.
                self.base = c
                continue
            if moved:
                self.release()
                self.safety.lock(f"physical input (interrupts: {', '.join(moved)})")
                self.running = False


# After the last neutral report and before a virtual pad is destroyed: a reader that has not read
# the report yet when the device goes loses it (evdev returns ENODEV, not the queued events).
RELEASE_SETTLE_S = 0.05


class UinputPad:
    """Fallback: a virtual Xbox-style pad (evdev uinput). Steam and gilrs see it as a second pad."""

    def __init__(self):
        from evdev import AbsInfo, UInput, ecodes as e  # noqa

        self.e = e
        btn = [e.BTN_SOUTH, e.BTN_EAST, e.BTN_NORTH, e.BTN_WEST, e.BTN_TL, e.BTN_TR, e.BTN_SELECT, e.BTN_START,
               e.BTN_MODE, e.BTN_THUMBL, e.BTN_THUMBR]
        ax = AbsInfo(0, -32768, 32767, 16, 128, 0)
        tr = AbsInfo(0, 0, 1023, 0, 0, 0)
        hat = AbsInfo(0, -1, 1, 0, 0, 0)
        caps = {e.EV_KEY: btn, e.EV_ABS: [(e.ABS_X, ax), (e.ABS_Y, ax), (e.ABS_RX, ax), (e.ABS_RY, ax), (e.ABS_Z, tr),
                                          (e.ABS_RZ, tr), (e.ABS_HAT0X, hat), (e.ABS_HAT0Y, hat)]}
        self.ui = UInput(caps, name="Rare Journeys Pad", vendor=0x1209, product=0x5253, version=1)
        self.kb = UInput({e.EV_KEY: [e.KEY_VOLUMEUP, e.KEY_VOLUMEDOWN]}, name="Rare Journeys Keys")
        self.held = set()
        self.map = {"a": e.BTN_SOUTH, "b": e.BTN_EAST, "x": e.BTN_WEST, "y": e.BTN_NORTH, "lb": e.BTN_TL, "rb": e.BTN_TR,
                    "select": e.BTN_SELECT, "start": e.BTN_START, "guide": e.BTN_MODE, "l3": e.BTN_THUMBL, "r3": e.BTN_THUMBR}
        time.sleep(1.0)  # let gilrs and Steam see the new device

    def set(self, button, down):
        e = self.e
        if button in self.map:
            self.ui.write(e.EV_KEY, self.map[button], 1 if down else 0)
        elif button in ("lt", "rt"):
            self.ui.write(e.EV_ABS, e.ABS_Z if button == "lt" else e.ABS_RZ, 1023 if down else 0)
        elif button in ("up", "down"):
            self.ui.write(e.EV_ABS, e.ABS_HAT0Y, (-1 if button == "up" else 1) if down else 0)
        elif button in ("left", "right"):
            self.ui.write(e.EV_ABS, e.ABS_HAT0X, (-1 if button == "left" else 1) if down else 0)
        elif button in ("volume_up", "volume_down"):
            self.kb.write(e.EV_KEY, e.KEY_VOLUMEUP if button == "volume_up" else e.KEY_VOLUMEDOWN, 1 if down else 0)
            self.kb.syn()
        self.ui.syn()
        (self.held.add if down else self.held.discard)(button)

    def stick(self, side, x, y):
        e = self.e
        a, b = (e.ABS_X, e.ABS_Y) if side == "left" else (e.ABS_RX, e.ABS_RY)
        self.ui.write(e.EV_ABS, a, int(x * 32767))
        self.ui.write(e.EV_ABS, b, int(y * 32767))
        self.ui.syn()

    def release_all(self):
        """Every button, hat, trigger and stick back to neutral, whatever `held` says (a press
        whose bookkeeping was lost must not stay down), in one report."""
        e = self.e
        if self.ui is None:
            return
        for code in self.map.values():
            self.ui.write(e.EV_KEY, code, 0)
        for code in (e.ABS_X, e.ABS_Y, e.ABS_RX, e.ABS_RY, e.ABS_Z, e.ABS_RZ, e.ABS_HAT0X, e.ABS_HAT0Y):
            self.ui.write(e.EV_ABS, code, 0)
        self.ui.syn()
        if self.kb is not None:
            for code in (e.KEY_VOLUMEUP, e.KEY_VOLUMEDOWN):
                self.kb.write(e.EV_KEY, code, 0)
            self.kb.syn()
        self.held.clear()

    def close(self):
        """Every button and axis back to neutral, a frame for the readers to take it (events
        still queued when a device goes away are lost: the launcher would keep a direction held),
        then the virtual devices are destroyed."""
        try:
            self.release_all()
        except Exception:
            pass
        time.sleep(RELEASE_SETTLE_S)
        for d in (getattr(self, "ui", None), getattr(self, "kb", None)):
            try:
                if d is not None:
                    d.close()
            except Exception:
                pass
        self.ui = self.kb = None


class FallbackPad:
    """InputPlumber first, the virtual uinput pad the moment it fails.

    InputPlumber 0.83's CompositeDevice.SendEvent can hang until the D-Bus timeout ("Timeout was
    reached"); the 2026-10-27 baseline died on it before the first journey. The first failed call
    switches every later edge to the uinput pad: buttons that were down on InputPlumber go down
    again on the new pad, then the edge that failed is sent there. A down edge that timed out is
    assumed not delivered (InputPlumber dropped it), so it is sent once more on the new pad."""

    def __init__(self, primary, make_fallback, log):
        self.primary, self.make_fallback, self.log = primary, make_fallback, log
        self.active = primary
        self.switched = None

    @property
    def path(self):
        return getattr(self.active, "path", "uinput")

    @property
    def held(self):
        return self.active.held

    def _switch(self, err, what):
        held = sorted(b for b in self.primary.held if b != what)
        self.log.emit("pad_fallback", reason=f"{type(err).__name__}: {err}", during=what, held=held)
        self.log.say(f"input: InputPlumber failed ({err}); switching to the virtual pad")
        self.active = self.make_fallback()
        self.switched = str(err)
        # Best effort: let go of whatever InputPlumber still holds, off this thread (each call can hang 1 s).
        # Its pad is cleared through the intercept mode first: that works when SendEvent does not.
        prim = self.primary

        def let_go():
            clear = getattr(prim, "clear_target", None)
            if clear is not None:
                try:
                    clear()
                except Exception:
                    pass
            prim.release_all()

        threading.Thread(target=let_go, daemon=True).start()
        for b in held:
            self.active.set(b, True)

    def set(self, button, down):
        if self.active is self.primary:
            try:
                return self.primary.set(button, down)
            except Exception as e:  # noqa
                self._switch(e, button)
        return self.active.set(button, down)

    def stick(self, side, x, y):
        if self.active is self.primary:
            try:
                return self.primary.stick(side, x, y)
            except Exception as e:  # noqa
                self._switch(e, f"{side}_stick")
        return self.active.stick(side, x, y)

    def release_all(self):
        try:
            self.active.release_all()
        except Exception:
            pass

    def close(self):
        for p in {id(self.active): self.active, id(self.primary): self.primary}.values():
            try:
                (getattr(p, "close", None) or p.release_all)()
            except Exception:
                pass


class MockIPPad:
    """Mock InputPlumber: forwards to the mock pad until its fail_after-th call, then times out
    like SendEvent on inputplumber 0.83 (and drops that event)."""

    def __init__(self, inner, fail_after):
        self.inner, self.fail_after, self.calls = inner, fail_after, 0
        self.path = "/org/shadowblip/InputPlumber/CompositeDevice0 (mock)"
        self.held = set()

    def _call(self):
        self.calls += 1
        if self.calls >= self.fail_after:
            raise RuntimeError("g-io-error-quark: Timeout was reached (24)")

    def set(self, button, down):
        self._call()
        self.inner.set(button, down)
        (self.held.add if down else self.held.discard)(button)

    def stick(self, side, x, y):
        self._call()
        self.inner.stick(side, x, y)

    def release_all(self):
        pass


# ---------------------------------------------------------------- capture

PIPELINES = [
    # (name, description). {src} is pipewiresrc, {thumbs} the thumbnail branch, {video} the file.
    ("va-shm", "{src} ! video/x-raw ! tee name=t t. ! {thumbs} t. ! queue max-size-buffers=30 max-size-bytes=0 max-size-time=0 ! "
               "vapostproc ! video/x-raw(memory:VAMemory),format=NV12 ! vah264enc rate-control=cqp qpi=24 qpp=27 key-int-max=30 ! "
               "h264parse ! matroskamux ! filesink location={file}"),
    ("va-dmabuf", "{src} ! video/x-raw(memory:DMABuf) ! tee name=t t. ! queue max-size-buffers=3 leaky=downstream ! vapostproc ! "
                  "video/x-raw,width={w},height={h} ! videoconvert ! video/x-raw,format=GRAY8,width={w},height={h} ! "
                  "appsink name=thumbs emit-signals=true sync=false max-buffers=240 drop=false "
                  "t. ! queue max-size-buffers=30 max-size-bytes=0 max-size-time=0 ! vapostproc ! video/x-raw(memory:VAMemory),format=NV12 ! "
                  "vah264enc rate-control=cqp qpi=24 qpp=27 key-int-max=30 ! h264parse ! matroskamux ! filesink location={file}"),
    ("thumbs-shm", "{src} ! video/x-raw ! {thumbs}"),
    ("thumbs-dmabuf", "{src} ! video/x-raw(memory:DMABuf) ! vapostproc ! video/x-raw,width={w},height={h} ! videoconvert ! "
                      "video/x-raw,format=GRAY8,width={w},height={h} ! appsink name=thumbs emit-signals=true sync=false max-buffers=240 drop=false"),
]
THUMBS = ("queue max-size-buffers=3 leaky=downstream ! videoscale method=bilinear ! video/x-raw,width={w},height={h} ! videoconvert ! "
          "video/x-raw,format=GRAY8,width={w},height={h} ! appsink name=thumbs emit-signals=true sync=false max-buffers=240 drop=false")


class FrameSink:
    """Common frame bookkeeping for the real and the mock capture."""

    def __init__(self, out, log, w=F.W, h=F.H):
        self.w, self.h = w, h
        self.tb = open(os.path.join(out, "thumbs.bin"), "wb")
        self.fc = open(os.path.join(out, "frames.csv"), "w")
        self.fc.write(F.FRAMES_HEADER)
        self.metric = F.Metric()
        self.idx = 0
        self.lock = threading.Lock()
        self.last_frame_ns = 0
        self.last_change_ns = 0
        self.first_ns = None
        self.log = log

    def push(self, px: bytes, t_ns: int, arrival_ns: int):
        m = self.metric.feed(px)
        with self.lock:
            self.tb.write(px)
            self.fc.write(F.frame_row(self.idx, t_ns, arrival_ns, m))
            self.idx += 1
            self.last_frame_ns = t_ns
            if self.first_ns is None:
                self.first_ns = t_ns
            if F.is_changed(m[3], m[4]):
                self.last_change_ns = t_ns

    def flush(self):
        with self.lock:
            self.tb.flush()
            self.fc.flush()

    def close(self):
        with self.lock:
            self.tb.close()
            self.fc.close()


class GstCapture:
    def __init__(self, out, log, sink: FrameSink, video=True, node=None, only=None):
        import gi

        gi.require_version("Gst", "1.0")
        from gi.repository import Gst  # noqa

        Gst.init(None)
        self.Gst = Gst
        self.out, self.log, self.sink = out, log, sink
        self.node = node or self._node()
        self.video = video
        self.only = only
        self.pipe = None
        self.name = None
        self.file = None
        self.errors = []

    def _node(self):
        d = json.loads(subprocess.run(["pw-dump"], capture_output=True, text=True, timeout=10).stdout or "[]")
        for o in d:
            p = (o.get("info") or {}).get("props") or {}
            if o.get("type", "").endswith(":Node") and p.get("node.name") == "gamescope" and p.get("media.class") == "Video/Source":
                return o["id"]
        raise RuntimeError("no gamescope PipeWire node (is gamescope running?)")

    def _on_sample(self, appsink):
        Gst = self.Gst
        s = appsink.emit("pull-sample")
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
        w, h = self.sink.w, self.sink.h
        if len(data) != w * h:
            stride = len(data) // h
            data = b"".join(data[y * stride:y * stride + w] for y in range(h))
        t = arr
        if buf.pts != Gst.CLOCK_TIME_NONE:
            cand = self.pipe.get_base_time() + buf.pts
            if 0 <= arr - cand < 500_000_000:
                t = cand
        self.sink.push(data, t, arr)
        return Gst.FlowReturn.OK

    def start(self):
        Gst = self.Gst
        src = f"pipewiresrc path={self.node} do-timestamp=true keepalive-time=0 ! queue max-size-buffers=3 leaky=downstream"
        cands = [c for c in PIPELINES if (self.video or c[0].startswith("thumbs")) and (not self.only or c[0] == self.only)]
        for name, desc in cands:
            if "vah264enc" in desc and not Gst.ElementFactory.find("vah264enc"):
                continue
            self.file = os.path.join(self.out, "video.mkv") if "filesink" in desc else None
            text = desc.format(src=src, thumbs=THUMBS.format(w=self.sink.w, h=self.sink.h), w=self.sink.w, h=self.sink.h, file=self.file)
            try:
                pipe = Gst.parse_launch(text)
            except Exception as e:  # noqa
                self.errors.append(f"{name}: {e}")
                continue
            clock = Gst.SystemClock.obtain()
            clock.set_property("clock-type", Gst.ClockType.MONOTONIC)
            pipe.use_clock(clock)
            app = pipe.get_by_name("thumbs")
            self.pipe = pipe
            app.connect("new-sample", self._on_sample)
            pipe.set_state(Gst.State.PLAYING)
            bus = pipe.get_bus()
            end = now() + 5_000_000_000
            ok, err = False, None
            while now() < end:
                msg = bus.timed_pop_filtered(100 * Gst.MSECOND, Gst.MessageType.ERROR)
                if msg:
                    err = msg.parse_error()[0].message
                    break
                caps = app.get_static_pad("sink").get_current_caps()
                if caps is not None:
                    ok = True
                    break
            if ok:
                self.name = name
                self.caps = caps.to_string()
                self.bus_thread = threading.Thread(target=self._bus, daemon=True)
                self.bus_thread.start()
                return {"pipeline_name": name, "caps": self.caps, "node": self.node, "w": self.sink.w, "h": self.sink.h,
                        "video": self.file, "base_time_ns": pipe.get_base_time()}
            self.errors.append(f"{name}: {err or 'no caps in 5 s'}")
            pipe.set_state(Gst.State.NULL)
            self.pipe = None
            if self.file and os.path.exists(self.file):
                os.unlink(self.file)
        raise RuntimeError("no capture pipeline worked: " + "; ".join(self.errors))

    def _bus(self):
        Gst = self.Gst
        bus = self.pipe.get_bus()
        while self.pipe is not None:
            msg = bus.timed_pop_filtered(200 * Gst.MSECOND, Gst.MessageType.ERROR | Gst.MessageType.WARNING)
            if msg is None:
                continue
            if msg.type == Gst.MessageType.ERROR:
                self.log.emit("capture_error", text=msg.parse_error()[0].message)
                return
            self.log.emit("capture_warning", text=msg.parse_warning()[0].message)

    def stop(self):
        if not self.pipe:
            return
        Gst = self.Gst
        pipe = self.pipe
        pipe.send_event(Gst.Event.new_eos())
        pipe.get_bus().timed_pop_filtered(5 * Gst.SECOND, Gst.MessageType.EOS | Gst.MessageType.ERROR)
        self.pipe = None
        pipe.set_state(Gst.State.NULL)


# ---------------------------------------------------------------- watchers

# Process names (comm, 15 characters) of a launcher: the web one (rare-launcher) and the native one
# (Rare Render: the POC's rare-render, rare-launcher-native). RARE_LAUNCHER_COMMS overrides.
LAUNCHER_COMMS = tuple(x for x in os.environ.get("RARE_LAUNCHER_COMMS", "rare-launcher,rare-launcher-n,rare-render").split(",") if x)


def launcher_pid():
    for p in glob.glob("/proc/[0-9]*/comm"):
        try:
            if open(p).read().strip() in LAUNCHER_COMMS:
                return int(p.split("/")[2])
        except Exception:
            pass
    return None


def proc_env(pid, key):
    try:
        for kv in open(f"/proc/{pid}/environ", "rb").read().split(b"\0"):
            if kv.startswith(key.encode() + b"="):
                return kv.split(b"=", 1)[1].decode()
    except Exception:
        pass
    return None


class FocusWatch(threading.Thread):
    def __init__(self, log, display):
        super().__init__(daemon=True)
        self.log, self.display = log, display
        self.app = None
        self.window = None
        self.p = None

    def run(self):
        env = dict(os.environ, DISPLAY=self.display)
        while True:
            self.p = subprocess.Popen(["xprop", "-root", "-spy", "GAMESCOPE_FOCUSED_APP", "GAMESCOPE_FOCUSED_WINDOW"],
                                      stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, env=env, bufsize=1)
            for line in self.p.stdout:
                m = re.match(r"(GAMESCOPE_FOCUSED_APP|GAMESCOPE_FOCUSED_WINDOW)\(CARDINAL\) = (\d+)", line)
                if not m:
                    continue
                if m.group(1) == "GAMESCOPE_FOCUSED_APP" and m.group(2) != self.app:
                    self.app = m.group(2)
                    self.log.emit("focus_app", app=self.app, window=self.window)
                elif m.group(1) == "GAMESCOPE_FOCUSED_WINDOW":
                    self.window = m.group(2)
            time.sleep(1)

    def stop(self):
        if self.p:
            self.p.terminate()


class ProbeTail(threading.Thread):
    """Follows $XDG_RUNTIME_DIR/rare-journey.log (launcher journey probe)."""

    def __init__(self, log, rt):
        super().__init__(daemon=True)
        self.log = log
        self.flag = os.path.join(rt, "rare-journey")
        self.path = os.path.join(rt, "rare-journey.log")
        self.cmd = os.path.join(rt, "rare-journey.cmd")
        self.state = {"screen": None, "focus": None, "ingame": None}
        self.on = False
        self.seq = 0
        self.lines = []
        self.lock = threading.Lock()
        self.running = True

    def enable(self):
        open(self.path, "w").close()
        open(self.flag, "w").write("tools/journeys\n")

    def disable(self):
        for p in (self.flag, self.cmd):
            try:
                os.unlink(p)
            except FileNotFoundError:
                pass

    def command(self, c):
        """Queues one setup command. The launcher reads the whole file and deletes it every 250 ms,
        so a second command written over an unread first one used to replace it (the 2026-10-27
        baseline: "close" then "tab:home" lost the close). Wait for the last one to be taken, then
        keep whatever is still unread in front of the new line."""
        end = time.monotonic() + 1.0
        while os.path.exists(self.cmd) and time.monotonic() < end:
            time.sleep(0.02)
        try:
            old = open(self.cmd).read()
        except FileNotFoundError:
            old = ""
        tmp = self.cmd + ".tmp"
        open(tmp, "w").write(old + c + "\n")
        os.replace(tmp, self.cmd)

    def run(self):
        f = None
        buf = ""
        while self.running:
            if f is None:
                try:
                    f = open(self.path)
                except FileNotFoundError:
                    time.sleep(0.05)
                    continue
            chunk = f.read()
            if not chunk:
                time.sleep(0.01)
                continue
            buf += chunk
            while "\n" in buf:
                line, buf = buf.split("\n", 1)
                ns, _, text = line.partition(" ")
                if not ns.isdigit():
                    continue
                t = int(ns)
                self.log.emit("probe", t=t, text=text)
                with self.lock:
                    self.on = True
                    self.seq += 1
                    self.lines.append((t, text))
                    del self.lines[:-400]
                    for k in ("screen", "focus", "ingame"):
                        if text.startswith(k + " "):
                            self.state[k] = text[len(k) + 1:]

    def since(self, t):
        with self.lock:
            return [x for x in self.lines if x[0] >= t]


EVIOCSCLOCKID = 0x400445A0


IRQ_RX = re.compile(r"i8042|GDIX|goodix|xpad", re.I)
DEV_RX = re.compile(r"X-Box|Xbox|Steam Deck|InputPlumber Keyboard|InputPlumber Mouse|Power Button", re.I)


def irq_counts() -> dict:
    """Interrupt totals of the physical input lines: i8042 (keys), the Goodix touchscreen, xpad."""
    out = {}
    try:
        for line in open("/proc/interrupts"):
            if IRQ_RX.search(line):
                p = line.split()
                n = sum(int(x) for x in p[1:] if x.isdigit())
                out[p[0].rstrip(":") + " " + p[-1]] = n
    except OSError:
        pass
    return out


class Guard(threading.Thread):
    """Base of the input guard. The runner depends on it: the injector sends a press only while the
    guard is alive and its heartbeat (one per loop) is under MAX_AGE_NS old, and the run aborts with
    no further presses (Safety.lock) the moment the guard exits for any reason other than teardown.
    Subclasses implement loop() and call self.tick() at least every 250 ms."""

    MAX_AGE_NS = 1_000_000_000

    def __init__(self, log, safety):
        super().__init__(daemon=True)
        self.log, self.safety = log, safety
        self.running = True
        self.beat = now()
        self.exit_reason = None

    def tick(self):
        self.beat = now()

    def dead(self):
        """None while the guard is alive with a fresh heartbeat, else why not."""
        if not self.is_alive():
            return f"input guard exited ({self.exit_reason or 'not running'})"
        age = now() - self.beat
        if age >= self.MAX_AGE_NS:
            return f"input guard heartbeat is {age / 1e6:.0f} ms old"
        return None

    def run(self):
        try:
            self.loop()
            self.exit_reason = "returned"
        except BaseException as e:  # noqa
            self.exit_reason = f"{type(e).__name__}: {e}"
            import traceback
            self.log.emit("note", text="input guard crashed: " + traceback.format_exc()[-1500:])
        finally:
            if self.running:  # not a teardown stop: stop the run now
                self.safety.lock(f"input guard exited ({self.exit_reason})")

    def loop(self):
        raise NotImplementedError


class MockGuard(Guard):
    """--mock: a guard with a heartbeat and nothing to watch. --mock-guard exit:S kills it after S
    seconds (the loop raises), stall:S stops its heartbeat while the thread stays alive."""

    def __init__(self, log, safety, fault=""):
        super().__init__(log, safety)
        kind, _, at = (fault or "").partition(":")
        self.fault, self.fault_at = (kind, float(at)) if kind else (None, 0)

    def loop(self):
        t0 = time.monotonic()
        while self.running:
            if self.fault and time.monotonic() - t0 >= self.fault_at:
                self.log.emit("guard_fault", kind=self.fault)
                if self.fault == "exit":
                    raise RuntimeError("mock: the guard was killed")
                while self.running:  # stall: alive, no heartbeat
                    time.sleep(0.05)
                return
            self.tick()
            time.sleep(0.05)


class InputWatch(Guard):
    """Physical-input guard. Reads (never grabs) the pad, keyboard, mouse and power-button devices the
    launcher sees, plus the i8042 and touchscreen interrupt counters.
      mode "idle": any input counts (the pre-run quiet period);
      mode "run":  input that is not the echo of what the runner injected aborts the run.
    Also times injected input (evdev echo) and watches the L3 + R3 abort chord."""

    def __init__(self, log, safety, injector, runner):
        super().__init__(log, safety)
        self.inj, self.runner = injector, runner
        self.devs = {}
        self.absinfo = {}
        self.down = {}
        self.mode = "idle"
        self.last_physical = None
        self.last_desc = ""
        self.irq = irq_counts()

    def _scan(self):
        try:
            from evdev import InputDevice, ecodes as e, list_devices
        except ImportError:
            return
        for p in list_devices():
            if p in self.devs:
                continue
            try:
                d = InputDevice(p)
            except Exception:
                continue
            try:
                keys = d.capabilities().get(e.EV_KEY, [])
                pad = e.BTN_THUMBL in keys and e.BTN_THUMBR in keys
                motion = e.INPUT_PROP_ACCELEROMETER in d.input_props() or "Motion" in d.name
                if (pad or DEV_RX.search(d.name)) and not motion and "Rare Journeys" not in d.name:
                    fcntl.ioctl(d.fd, EVIOCSCLOCKID, struct.pack("i", 1))  # CLOCK_MONOTONIC stamps
                    self.devs[p] = d
                    self.absinfo[p] = {c: i for c, i in d.capabilities().get(e.EV_ABS, [])}
                else:
                    d.close()
            except Exception:
                d.close()

    def significant(self, path, ev, e):
        if ev.type == e.EV_KEY:
            return True
        if ev.type == e.EV_REL:
            return abs(ev.value) >= 3
        if ev.type == e.EV_ABS:
            if ev.code in (e.ABS_HAT0X, e.ABS_HAT0Y):
                return ev.value != 0
            info = self.absinfo.get(path, {}).get(ev.code)
            if not info or info.max <= info.min:
                return False
            span = info.max - info.min
            if info.min < 0:  # stick: away from centre
                return abs(ev.value - (info.max + info.min) / 2) > 0.3 * span / 2
            return ev.value - info.min > 0.1 * span  # trigger
        return False

    def physical(self, t, desc):
        self.last_physical = t
        self.last_desc = desc
        if self.mode == "run":
            if now() < self.runner.exempt_until:
                self.log.emit("note", t=t, text=f"input during an exempt window: {desc}")
                return
            self.log.emit("foreign_input", t=t, desc=desc)
            # Let go of everything first: the person's own presses must work at once.
            self.runner.release_pad()
            self.safety.lock(f"physical input ({desc}): the device is in use")

    def loop(self):
        try:
            from evdev import ecodes as e
        except ImportError:
            self.log.emit("note", text="python-evdev missing: input guard on interrupts only")
            e = None
        import select

        last_scan = last_irq = 0
        while self.running:
            self.tick()
            if time.monotonic() - last_irq > 0.25:
                cur = irq_counts()
                for k, v in cur.items():
                    if v > self.irq.get(k, v):
                        self.physical(now(), f"interrupt {k} +{v - self.irq[k]}")
                self.irq = cur
                last_irq = time.monotonic()
            if e is None:
                time.sleep(0.25)
                continue
            if time.monotonic() - last_scan > 2:
                self._scan()
                last_scan = time.monotonic()
            if not self.devs:
                time.sleep(0.1)
                continue
            r, _, _ = select.select(list(self.devs.values()), [], [], 0.1)
            for d in r:
                try:
                    for ev in d.read():
                        if ev.type not in (e.EV_KEY, e.EV_ABS, e.EV_REL):
                            continue
                        t = ev.sec * 1_000_000_000 + ev.usec * 1000
                        if ev.type == e.EV_KEY:
                            if ev.value:
                                self.down[ev.code] = t
                            else:
                                self.down.pop(ev.code, None)
                        injected = self.inj.on_evdev(t, ev.type, ev.code, ev.value)
                        if not injected and self.significant(d.path, ev, e):
                            self.physical(t, f"{d.name}: {e.EV[ev.type]} {ev.code}={ev.value}")
                except OSError:
                    self.devs = {k: v for k, v in self.devs.items() if v is not d}
            a, b = self.down.get(e.BTN_THUMBL), self.down.get(e.BTN_THUMBR)
            if a and b and now() - max(a, b) > 1_000_000_000:
                self.safety.lock("abort chord (L3 + R3)")


class ProcWatch(threading.Thread):
    WATCH = LAUNCHER_COMMS + ("gamescope", "gamescope-wl", "inputplumber", "Xwayland")

    def __init__(self, log, start_epoch):
        super().__init__(daemon=True)
        self.log = log
        self.start_epoch = start_epoch
        self.running = True
        self.expected = {}
        self.seen_dumps = set()
        rt = os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}")
        self.panics = {p: self._size(p) for p in (os.path.join(rt, "rare-launcher-panic.log"), os.path.expanduser("~/.cache/rare/panic.log"))}
        self.pids = self._pids()
        self.webkit = self._webkit()

    @staticmethod
    def _size(p):
        try:
            return os.path.getsize(p)
        except OSError:
            return 0

    def _pids(self):
        out = {}
        for p in glob.glob("/proc/[0-9]*/comm"):
            try:
                c = open(p).read().strip()
            except Exception:
                continue
            if c in self.WATCH:
                out.setdefault(c, set()).add(int(p.split("/")[2]))
        return out

    def _webkit(self):
        lp = launcher_pid()
        out = set()
        if not lp:
            return out
        for p in glob.glob("/proc/[0-9]*/stat"):
            try:
                st = open(p).read()
                comm = st[st.index("(") + 1:st.rindex(")")]
                ppid = int(st[st.rindex(")") + 2:].split()[1])
            except Exception:
                continue
            if comm.startswith("WebKitWebProces") and ppid == lp:
                out.add(int(p.split("/")[2]))
        return out

    def expect_exit(self, name, secs):
        self.expected[name] = time.monotonic() + secs

    def poll(self):
        cur = self._pids()
        for name, pids in self.pids.items():
            gone = pids - cur.get(name, set())
            if gone and self.expected.get(name, 0) > time.monotonic():
                self.log.emit("note", text=f"{name} exited as planned")
            elif gone:
                self.log.emit("crash", source="exit", detail=f"{name} pid {','.join(map(str, sorted(gone)))} exited",
                              process=name, restarted=bool(cur.get(name)))
        self.pids = cur
        live = {p for p in self.webkit if os.path.exists(f"/proc/{p}")}
        if live != self.webkit and launcher_pid():
            self.log.emit("crash", source="exit", detail=f"launcher web process {sorted(self.webkit - live)} exited", process="WebKitWebProcess")
            self.webkit = self._webkit()
        for p, size in self.panics.items():
            s = self._size(p)
            if s > size:
                try:
                    tail = open(p, errors="replace").read()[size:].strip().splitlines()[-3:]
                except Exception:
                    tail = []
                self.log.emit("crash", source="panic", detail=f"{os.path.basename(p)}: {' | '.join(tail)[:400]}")
                self.panics[p] = s

    def dumps(self):
        try:
            out = subprocess.run(["coredumpctl", "list", f"--since=@{int(self.start_epoch)}", "--no-pager", "--json=short"],
                                 capture_output=True, text=True, timeout=10).stdout
            rows = json.loads(out) if out.strip() else []
        except Exception:
            return
        for r in rows:
            key = (r.get("pid"), r.get("time"))
            if key in self.seen_dumps:
                continue
            self.seen_dumps.add(key)
            self.log.emit("crash", source="coredump", detail=f"{r.get('exe')} pid {r.get('pid')} signal {r.get('sig')}")

    def run(self):
        n = 0
        while self.running:
            try:
                self.poll()
                if n % 6 == 0:
                    self.dumps()
            except Exception as e:  # noqa
                self.log.emit("note", text=f"procwatch: {e}")
            n += 1
            time.sleep(0.5)


class Power(threading.Thread):
    """10 Hz APU power from amdgpu gpu_metrics v2.1 (socket, CPU cores, GFX), else power1_average."""

    def __init__(self, out):
        super().__init__(daemon=True)
        self.f = open(os.path.join(out, "power.csv"), "w", buffering=1)
        self.f.write("t_ns,socket_w,cpu_w,gfx_w,soc_w,gfx_busy,gfx_mhz\n")
        self.gm = (glob.glob("/sys/class/drm/card*/device/gpu_metrics") or [None])[0]
        self.pa = (glob.glob("/sys/class/drm/card*/device/hwmon/hwmon*/power1_average") or [None])[0]
        self.busy = (glob.glob("/sys/class/drm/card*/device/gpu_busy_percent") or [None])[0]
        self.running = True

    def sample(self):
        sock = cpu = gfx = soc = mhz = None
        if self.gm:
            try:
                d = open(self.gm, "rb").read()
                _, fmt, cont = struct.unpack_from("<HBB", d, 0)
                if fmt == 2 and len(d) >= 66:
                    u = struct.unpack_from("<33H", d, 0)
                    if u[20] != 0xFFFF:
                        sock = u[20] / 1000
                    cores = [c for c in u[24:32] if c != 0xFFFF]
                    cpu = (u[21] if u[21] != 0xFFFF else sum(cores)) / 1000
                    gfx = u[23] / 1000 if u[23] != 0xFFFF else None
                    soc = u[22] / 1000 if u[22] != 0xFFFF else None
                    mhz = u[32] if u[32] != 0xFFFF else None
            except Exception:
                pass
        if sock is None and self.pa:
            try:
                sock = int(open(self.pa).read()) / 1e6
            except Exception:
                pass
        busy = None
        if self.busy:
            try:
                busy = int(open(self.busy).read())
            except Exception:
                pass
        f = lambda v: "" if v is None else ("%.3f" % v)  # noqa
        self.f.write(f"{now()},{f(sock)},{f(cpu)},{f(gfx)},{f(soc)},{'' if busy is None else busy},{'' if mhz is None else mhz}\n")

    def run(self):
        while self.running:
            self.sample()
            time.sleep(0.1)


# ---------------------------------------------------------------- mock world (runs anywhere)

class MockWorld:
    """A tiny launcher model for exercising the runner without a device: tabs, overlays, a game,
    a probe log, focus and frames that animate after each press (with optional injected faults)."""

    TABS = ["home", "library", "games", "wallet", "trade", "market", "friends", "settings"]

    def __init__(self, log, sink, probe, focus, faults):
        self.log, self.sink, self.probe, self.focus = log, sink, probe, focus
        self.tab, self.overlay, self.focus_i, self.app = "home", None, 0, "1380012613"
        self.running = []
        self.cache = {}
        self.faults = dict(faults)
        self.presses = {}
        self.lock = threading.Lock()
        self.frame = self._scene()
        self.focus.app = self.app
        self.log.emit("focus_app", app=self.app)

    def _scene(self):
        import zlib

        key = f"{self.tab}|{self.overlay}|{self.focus_i}|{self.app}"
        if key in self.cache:
            return self.cache[key]
        base = 40 + zlib.crc32(key.encode()) % 160
        rows = [bytes((base + (x * 2 + y) % 50) & 255 for x in range(F.W)) for y in range(F.H)]
        fx = 8 + (self.focus_i % 6) * 14
        for y in range(40, 52):
            r = bytearray(rows[y])
            r[fx:fx + 12] = b"\xff" * 12
            rows[y] = bytes(r)
        self.cache[key] = b"".join(rows)
        return self.cache[key]

    def plog(self, text, t=None):
        t = t or now()
        if os.path.exists(self.probe.flag):
            with open(self.probe.path, "a") as f:
                f.write(f"{t} {text}\n")

    def screen(self):
        return f"screen tab={self.tab} overlay={self.overlay or '-'} running={len(self.running)} launching=-"

    def press(self, buttons):
        key = "+".join(buttons)
        n = self.presses[key] = self.presses.get(key, 0) + 1
        fault = self.faults.get(f"{key}:{n}")
        b = buttons[0] if len(buttons) == 1 else key
        before = self._scene()
        with self.lock:
            if b in ("rb", "lb"):
                i = self.TABS.index(self.tab)
                self.overlay = None
                self.tab = self.TABS[(i + (1 if b == "rb" else -1)) % len(self.TABS)]
            elif b in ("start", "guide+a"):
                self.overlay = None if self.overlay == "qam" else "qam"
            elif b == "guide":
                # Like App.tsx: the Rare menu; pressed again, Home. Over a game the menu opens too.
                if self.overlay == "raremenu" and self.app == "1380012613":
                    self.overlay, self.tab = None, "home"
                else:
                    self.overlay, self.app = "raremenu", "1380012613"
            elif b == "b" and self.overlay == "running":
                self.overlay, self.app = None, "2000000001"
                self.plog("ingame mode=- game=-")
            elif b == "b":
                self.overlay = None
            elif b in ("down", "right"):
                self.focus_i += 1
            elif b in ("up", "left"):
                self.focus_i = max(0, self.focus_i - 1)
            elif b == "a" and not self.overlay and self.tab == "library":
                self.overlay = "game:steam:1"
            elif b == "a" and self.overlay and self.overlay.startswith("game:"):
                self.running.append(self.overlay[5:])
                self.app = "2000000001"
            elif b == "select+start" and self.running:
                self.app = "1380012613"
                self.overlay = "running"
                self.plog(f"ingame mode=running game={self.running[0]}")
        after = self._scene()
        threading.Thread(target=self._animate, args=(before, after, fault), daemon=True).start()

    def command(self, c):
        if c.startswith("tab:"):
            self.tab, self.overlay = c[4:], None
        elif c.startswith("open:game:"):
            self.overlay = "game:" + c[10:]
        elif c == "close":
            self.overlay = None
        elif c == "state":
            self.plog(self.screen())
            return
        elif c.startswith("quit:"):
            self.running, self.app = [], "1380012613"
        self._animate(self.frame, self._scene(), None)

    def _animate(self, a, b, fault):
        steps = 6
        mix = [bytes(int(x + (y - x) * k / steps) for x, y in zip(a, b)) for k in range(1, steps + 1)]
        time.sleep(0.03)
        self.plog(self.screen())
        label = "Play" if (self.overlay or "").startswith("game:") else f"Item {self.focus_i}"
        self.plog(f"focus button@row \"{label}\" at 0,0 10x10")
        if self.app != self.focus.app:
            self.focus.app = self.app
            self.log.emit("focus_app", app=self.app)
        t = now()
        for k in range(1, steps + 1):
            if fault == "black" and k == 3:
                fr = bytes(F.W * F.H)
            elif fault == "flash" and k == steps:
                self.sink.push(b, now(), now())
                time.sleep(1 / 60)
                fr = bytes(255 - v for v in b)
                self.sink.push(fr, now(), now())
                time.sleep(1 / 60)
                fr = b
            elif fault == "hitch" and k == 3:
                time.sleep(0.2)
                fr = mix[k - 1]
            else:
                fr = mix[k - 1]
            self.sink.push(fr, now(), now())
            time.sleep(1 / 60)
        self.plog("painted", t + 30_000_000)
        self.frame = b


class MockPad:
    def __init__(self, world_ref):
        self.world_ref = world_ref
        self.held = []

    def set(self, button, down):
        # Like the launcher, a press acts on the way down; a chord's buttons go down within a few ms.
        if down:
            if not self.held:
                threading.Timer(0.01, self._fire).start()
            self.held.append(button)
        else:
            self.held = [x for x in self.held if x != button]

    def _fire(self):
        if self.held:
            self.world_ref[0].press(list(self.held))

    def stick(self, side, x, y):
        pass

    def release_all(self):
        self.held = []


# ---------------------------------------------------------------- the runner

class Injector:
    """Presses buttons through the pad, logging each edge; matches evdev echoes for delivery time."""

    def __init__(self, pad, log, safety):
        self.pad, self.log, self.safety = pad, log, safety
        self.pending = []
        self.ctx = {}
        self.lock = threading.Lock()
        self.foreign = 0
        self.expect_until = 0
        self.refused = 0

    def gate(self, what):
        """Raises Abort unless a live input guard has a fresh heartbeat (and the owner never
        touched the device during the run). Every press, stick move and key goes through here."""
        why = self.safety.guard_ok()
        if why:
            self.refused += 1
            self.log.emit("input_refused", what=what, reason=why, journey=self.ctx.get("journey"), step=self.ctx.get("step"))
            raise Abort(self.safety.reason or why)

    def stick(self, side, x, y):
        if x or y:
            self.gate(f"{side}_stick")
        self.pad.stick(side, x, y)

    def edge(self, button, down):
        if down:
            self.gate(button)  # releases always go out: they put the pad back
        t_call = now()
        self.expect_until = t_call + 300_000_000
        self.pad.set(button, down)
        t = now()
        e = {"journey": self.ctx.get("journey"), "step": self.ctx.get("step"), "button": button,
             "action": "down" if down else "up", "t_call": t_call}
        with self.lock:
            self.pending.append(e)
            del self.pending[:-20]
        self.log.emit("input", t=t, **e)
        return e

    def on_evdev(self, t, typ, code, value) -> bool:
        """True when the event is the echo of something the runner injected."""
        with self.lock:
            for e in self.pending:
                if "t_evdev" not in e and 0 <= t - e["t_call"] < 150_000_000:
                    e["t_evdev"] = t
                    self.log.emit("input_echo", t=t, button=e["button"], action=e["action"], t_call=e["t_call"],
                                  journey=e["journey"], step=e["step"])
                    return True
        if t <= self.expect_until:
            return True
        self.foreign += 1
        return False


class Runner:
    def __init__(self, a):
        self.a = a
        self.out = a.out
        os.makedirs(self.out, exist_ok=True)
        self.log = Log(self.out)
        self.safety = Safety(self.log, a.stop_file, a.max_minutes * 60)
        self.rt = os.environ.get("XDG_RUNTIME_DIR") or f"/run/user/{os.getuid()}"
        if a.mock:
            self.rt = os.path.join(self.out, "mock-rt")
            os.makedirs(self.rt, exist_ok=True)
        self.exempt_until = 0
        self.cleaning = False
        self.sink = FrameSink(self.out, self.log)
        self.capture = None
        self.threads = []
        self.volume = None
        self.meta = {"run_id": os.path.basename(self.out.rstrip("/")), "started_epoch": time.time(), "started_ns": now(),
                     "mock": bool(a.mock), "host": os.uname().nodename}

    # ---- setup / teardown

    def setup(self):
        a = self.a
        self.probe = ProbeTail(self.log, self.rt)
        if a.mock:
            self.focus = FocusWatch(self.log, ":0")
            world_ref = [None]
            self.pad = MockPad(world_ref)
            if a.mock_ipfail:
                # InputPlumber that times out on its Nth call: the runner must carry on on the virtual pad.
                self.pad = FallbackPad(MockIPPad(MockPad(world_ref), a.mock_ipfail), lambda: MockPad(world_ref), self.log)
            faults = dict(f.split("=", 1)[::-1] for f in (a.mock_faults or "").split(",") if "=" in f)
            # --mock-faults black=rb:2,flash=lb:1  ->  {"rb:2": "black", ...}
            self.world = MockWorld(self.log, self.sink, self.probe, self.focus, faults)
            world_ref[0] = self.world
            self.meta.update(launcher_app="1380012613", launcher_version="mock", os_version="mock",
                             capture={"pipeline_name": "mock", "w": F.W, "h": F.H})
            self.inj = Injector(self.pad, self.log, self.safety)
            self.guard = MockGuard(self.log, self.safety, a.mock_guard)
            self.guard.start()
            self.threads.append(self.guard)
            self.safety.guard = self.guard
            self.probe.enable()
            self.probe.start()
            if a.mock_foreign:
                # Someone picks the device up mid-run: the same path InputWatch takes.
                iw = InputWatch(self.log, self.safety, self.inj, self)
                iw.mode = "run"
                threading.Timer(a.mock_foreign, lambda: iw.physical(now(), "mock: X-Box pad EV_KEY 304=1")).start()
            return
        lp = launcher_pid()
        if not lp:
            raise SystemExit("rare-launcher is not running; start the Rare session first")
        self.display = proc_env(lp, "DISPLAY") or ":0"
        # Nothing is injected (no pad made, no probe event) until the device is idle: the pad and
        # the injector come after wait_idle and the interrupt check below.
        self.pad = None
        self.inj = Injector(_NoPad(), self.log, self.safety)
        self.focus = FocusWatch(self.log, self.display)
        self.focus.start()
        self.threads.append(self.focus)
        # The launcher's app id: its gamescope scope name, else what is focused now.
        app = None
        try:
            m = re.search(r"app-steam-app(\d+)-", open(f"/proc/{lp}/cgroup").read())
            app = m.group(1) if m else None
        except Exception:
            pass
        time.sleep(0.5)
        self.meta["launcher_app"] = app or a.launcher_app
        self.meta["launcher_pid"] = lp
        # Mandatory idle guard: nobody touched the device for --idle-min and Rare is in front.
        self.input = InputWatch(self.log, self.safety, self.inj, self)
        self.input.start()
        self.threads.append(self.input)
        self.safety.guard = self.input  # from here on, no live guard means no input and an abort
        self.wait_idle()
        self.wait_irq_idle(IRQ_IDLE_S)
        self.pad = self._make_pad()
        self.inj.pad = self.pad
        self.irq = IrqWatch(self.log, self.safety, self.release_pad, self)
        self.irq.start()
        self.threads.append(self.irq)
        self.input.mode = "run"
        self.meta["launcher_version"] = self._launcher_version(lp)
        self.meta["os_version"] = self._os_version()
        self.meta["display"] = self.display
        try:
            v = subprocess.run(["wpctl", "get-volume", "@DEFAULT_AUDIO_SINK@"], capture_output=True, text=True, timeout=5).stdout
            m = re.search(r"Volume: ([0-9.]+)", v)
            self.volume = m.group(1) if m else None
        except Exception:
            pass
        # Launcher settings before the run: some journeys flip settings (rumble, UI size); the report
        # lists what changed and `run.sh restore-config` puts it back.
        self.cfg_dir = os.path.expanduser("~/.config/rare")
        self.cfg_before = {}
        for f in glob.glob(os.path.join(self.cfg_dir, "*")):
            if os.path.isfile(f) and os.path.getsize(f) < 1_000_000:
                self.cfg_before[os.path.basename(f)] = open(f, "rb").read()
        os.makedirs(os.path.join(self.out, "config-before"), exist_ok=True)
        for n, b in self.cfg_before.items():
            open(os.path.join(self.out, "config-before", n), "wb").write(b)
        self.power = Power(self.out)
        self.power.start()
        self.threads.append(self.power)
        self.procs = ProcWatch(self.log, self.meta["started_epoch"])
        self.procs.start()
        self.threads.append(self.procs)
        if not a.no_capture:
            self.capture = GstCapture(self.out, self.log, self.sink, video=not a.no_video, only=a.pipeline)
            self.meta["capture"] = self.capture.start()
            self.log.say(f"capture: {self.meta['capture']['pipeline_name']} {self.meta['capture']['caps']}")
        else:
            self.meta["capture"] = {"pipeline_name": "none"}
        self.probe.enable()
        self.probe.start()

    def _make_pad(self):
        """InputPlumber when it answers, else the virtual uinput pad, chosen without asking: a
        SendEvent that hangs (inputplumber 0.83) must never end a run. A centred left stick is
        the probe (it moves nothing). Later failures switch over through FallbackPad."""
        try:
            ip = IPPad()
            ip.stick("left", 0.0, 0.0)
        except Exception as e:  # noqa
            if self.a.ip_only:
                raise SystemExit(f"InputPlumber injection unavailable ({e}) and --ip-only is set")
            self.log.emit("pad_fallback", reason=f"{type(e).__name__}: {e}", during="probe", held=[])
            self.log.say(f"input: InputPlumber unavailable ({e}); using the virtual pad")
            self.meta["input"] = "uinput (InputPlumber failed at start)"
            return UinputPad()
        self.meta["input"] = "inputplumber " + ip.path
        return ip if self.a.ip_only else FallbackPad(ip, UinputPad, self.log)

    def release_pad(self):
        """Every button and stick to neutral now (any thread; errors ignored)."""
        p = getattr(self, "pad", None)
        if p is not None:
            try:
                p.release_all()
            except Exception:
                pass

    def close_pad(self):
        """Neutral, then the virtual pad (if one was made) destroyed. Safe to call twice."""
        p, self.pad = getattr(self, "pad", None), None
        if p is not None:
            try:
                (getattr(p, "close", None) or p.release_all)()
            except Exception:
                pass

    def wait_irq_idle(self, secs):
        """No physical-input interrupt for `secs` in a row (the tools/release/build-native.sh check), else wait on."""
        give_up = now() + int(max(self.a.wait_max, self.a.idle_min) * 60e9)
        base, since = irq_counts(), now()
        self.log.say(f"idle guard: waiting for {secs} s with no input interrupts")
        while True:
            self.safety.check()
            c = irq_counts()
            if c != base:
                base, since = c, now()
            if now() - since >= secs * 1e9:
                self.log.emit("irq_idle_ok", quiet_s=secs)
                return
            if now() > give_up:
                raise SystemExit("idle guard: input interrupts did not stop")
            time.sleep(0.2)

    def wait_idle(self):
        """No physical input for idle_min minutes with the launcher in front; gives up after wait_max."""
        idle_ns = int(self.a.idle_min * 60e9)
        give_up = now() + int(max(self.a.wait_max, self.a.idle_min) * 60e9)
        launcher = str(self.meta["launcher_app"])
        quiet_since = now()
        last_say = 0
        self.log.say(f"idle guard: waiting for {self.a.idle_min:g} min with no input and Rare in front (gives up after {max(self.a.wait_max, self.a.idle_min):g} min)")
        while True:
            self.safety.check()
            t = now()
            if self.input.last_physical and self.input.last_physical > quiet_since:
                quiet_since = self.input.last_physical
                self.log.emit("note", text=f"idle guard: input ({self.input.last_desc}), waiting again")
                self.log.say(f"idle guard: someone is using it ({self.input.last_desc}); timer restarted")
            if self.focus.app != launcher:
                quiet_since = t
            if t - quiet_since >= idle_ns:
                self.log.emit("idle_ok", quiet_s=round((t - quiet_since) / 1e9))
                self.log.say("idle guard: idle, starting")
                return
            if t > give_up:
                raise SystemExit(f"idle guard: the device was not idle for {self.a.idle_min:g} min "
                                 f"(last input: {self.input.last_desc or 'none'}, focused app {self.focus.app}, launcher {launcher})")
            if t - last_say > 60e9:
                left = (idle_ns - (t - quiet_since)) / 60e9
                self.log.say(f"idle guard: {left:.1f} min to go (focused app {self.focus.app})")
                last_say = t
            time.sleep(0.5)

    @staticmethod
    def _launcher_version(pid):
        # Never run the binary (a second launcher could start); its path and build time identify it.
        appimage = proc_env(pid, "APPIMAGE")
        try:
            exe = appimage or os.readlink(f"/proc/{pid}/exe")
            return f"{exe} ({time.strftime('%Y-%m-%d %H:%M', time.localtime(os.path.getmtime(exe)))})"
        except Exception:
            return "?"

    @staticmethod
    def _os_version():
        try:
            for line in open("/etc/os-release"):
                if line.startswith("VERSION="):
                    return line.split("=", 1)[1].strip().strip('"')
        except Exception:
            pass
        return "?"

    def teardown(self):
        if getattr(self, "irq", None):
            self.irq.running = False
        self.close_pad()
        try:
            self.probe.disable()
            self.probe.running = False
        except Exception:
            pass
        if self.capture:
            try:
                self.capture.stop()
            except Exception:
                pass
        for t in self.threads:
            for attr in ("running",):
                if hasattr(t, attr):
                    setattr(t, attr, False)
        if getattr(self, "focus", None):
            self.focus.stop()
        if self.volume and not self.a.mock:
            subprocess.run(["wpctl", "set-volume", "@DEFAULT_AUDIO_SINK@", self.volume], capture_output=True, timeout=5)
        self.sink.flush()
        if self.capture and getattr(self.capture, "file", None) and self.sink.first_ns:
            self.meta["capture"]["video_t0_ns"] = self.sink.first_ns
        if getattr(self, "cfg_before", None) is not None:
            changed = []
            for n, b in self.cfg_before.items():
                p = os.path.join(self.cfg_dir, n)
                if not os.path.exists(p) or open(p, "rb").read() != b:
                    changed.append(n)
            self.meta["config_changed"] = changed
            if changed:
                self.log.say(f"launcher settings changed by the run: {', '.join(changed)} (run.sh restore-config {self.meta['run_id']} puts them back)")
        self.meta["ended_ns"] = now()
        json.dump(self.meta, open(os.path.join(self.out, "meta.json"), "w"), indent=1, default=str)
        self.sink.close()

    # ---- primitives

    def press(self, buttons, hold_ms=80):
        sent = []
        try:
            for b in buttons:
                self.inj.edge(b, True)
                sent.append(b)
            # Cleanup still has to finish after an abort, so it sleeps without the abort check.
            time.sleep(hold_ms / 1000) if self.cleaning else self.safety.sleep(hold_ms)
        finally:
            for b in reversed(sent):
                self.inj.edge(b, False)

    def settled(self, quiet_ms=300, timeout_ms=3000, since=None):
        """Waits until no frame changed for quiet_ms (min 150 ms after `since`)."""
        since = since or now()
        end = since + int(timeout_ms * 1e6)
        self.safety.sleep(150)
        while now() < end:
            last = max(self.sink.last_change_ns, since)
            if now() - last >= quiet_ms * 1e6:
                return True
            self.safety.sleep(20)
        return False

    def cond(self, w: dict, t0: int) -> tuple:
        """(all true, what is not yet true)."""
        missing = []
        st = self.probe.state
        launcher = str(self.meta.get("launcher_app"))
        for k, v in w.items():
            if k in ("timeout_ms",):
                continue
            if k == "focus_app":
                have = self.focus.app
                ok = (have == launcher) if v == "launcher" else (have not in (None, "0", launcher)) if v == "not_launcher" else (have == str(v))
                if not ok:
                    missing.append(f"focus_app={have} (want {v})")
            elif k == "screen":
                from jlib.analyze import parse_screen, screen_matches

                if not screen_matches(v, parse_screen(st["screen"])):
                    missing.append(f"screen={st['screen']} (want {v})")
            elif k == "focus":
                if not st["focus"] or not re.search(v, st["focus"]):
                    missing.append(f"focus={st['focus']} (want /{v}/)")
            elif k == "ingame":
                if not st["ingame"] or not all(f"{a}={b}" in st["ingame"] for a, b in v.items()):
                    missing.append(f"ingame={st['ingame']} (want {v})")
            elif k == "probe_text":
                if not any(re.search(v, x[1]) for x in self.probe.since(t0)):
                    missing.append(f"no probe line /{v}/")
            elif k == "changed":
                if (self.sink.last_change_ns > t0) != bool(v):
                    missing.append("screen unchanged" if v else "screen changed")
            elif k == "settled":
                if now() - max(self.sink.last_change_ns, t0) < int(v) * 1e6:
                    missing.append("still moving")
            elif k == "process":
                if not self._proc_running(v):
                    missing.append(f"no process /{v}/")
            else:
                missing.append(f"unknown condition {k}")
        return not missing, missing

    def wait_for(self, w, t0=None):
        t0 = t0 or now()
        end = t0 + int(w.get("timeout_ms", 5000) * 1e6)
        while True:
            ok, missing = self.cond(w, t0)
            if ok:
                return (now() - t0) / 1e6
            if now() > end:
                raise StepFail("timed out waiting: " + "; ".join(missing))
            self.safety.sleep(25)

    @staticmethod
    def _proc_running(rx):
        for p in glob.glob("/proc/[0-9]*/cmdline"):
            try:
                if re.search(rx, open(p, "rb").read().replace(b"\0", b" ").decode(errors="replace")):
                    return True
            except Exception:
                pass
        return False

    def need_ok(self, j):
        for n in j["needs"]:
            if n == "probe" and not self.probe.on:
                raise Skip("needs the launcher journey probe (launcher with src/nav/journey.ts)")
            if n == "root_or_attended" and not (self.a.rtc_driver or self.a.attended or self.a.mock):
                raise Skip("needs --sudo (run.sh arms the RTC wake) or --attended (owner presses power)")
            if n == "video" and not (self.capture and self.capture.file):
                raise Skip("needs video capture")
            if n == "session_client" and not self.a.mock and not self._proc_running(r"rare-session-client"):
                raise Skip("the launcher is not under rare-session-client, so a restart would end the session")
            if n == "network" and not self.a.mock:
                r = subprocess.run(["nmcli", "-t", "-g", "CONNECTIVITY", "general"], capture_output=True, text=True, timeout=5)
                if r.stdout.strip() not in ("full", ""):
                    raise Skip(f"network connectivity is {r.stdout.strip()}")
            if n == "steam" and not self.a.mock and not self._proc_running(r"(^|/)steam( |$)|steamwebhelper"):
                self.log.emit("note", text="Steam is not running; the journey starts it")

    # ---- steps

    def step(self, j, idx, st):
        k = st["kind"]
        info = {}
        if k == "press":
            if "a" in st["press"] and self.probe.on and not st.get("allow_unsafe"):
                f = self.probe.state.get("focus") or ""
                if spec.DENY_A_FOCUS.search(f):
                    raise StepFail(f"refused to press A on {f}")
            for r in range(st["repeat"]):
                t = now()
                self.press(st["press"], st["hold_ms"])
                if r + 1 < st["repeat"]:
                    self.safety.sleep(st["gap_ms"])
            self._after(st, t)
        elif k == "stick":
            s = st["stick"]
            t = now()
            moved = []
            try:
                for side in ("left", "right"):
                    if side in s:
                        self.inj.stick(side, *s[side])
                        moved.append(side)
                        self.log.emit("input", journey=j["name"], step=idx, button=f"{side}_stick", action="down", t_call=t)
                self.safety.sleep(s["ms"])
            finally:
                for side in moved:
                    self.inj.stick(side, 0.0, 0.0)
            self._after(st, t)
        elif k in ("key", "type"):
            t = now()
            env = dict(os.environ, DISPLAY=getattr(self, "display", ":0"))
            args = ["xdotool", "key", "--clearmodifiers", st["key"]] if k == "key" else ["xdotool", "type", "--delay", "40", st["type"]]
            self.inj.gate(f"{k}:{st[k]}")
            self.log.emit("input", journey=j["name"], step=idx, button=f"{k}:{st[k]}", action="down", t_call=t)
            if not self.a.mock:
                # xdotool type takes 40 ms a character: stop it the moment the guard goes away.
                p = subprocess.Popen(args, env=env)
                try:
                    end = time.monotonic() + 30
                    while p.poll() is None and time.monotonic() < end:
                        self.inj.gate(f"{k}:{st[k]}")
                        time.sleep(0.02)
                finally:
                    if p.poll() is None:
                        p.kill()
                        p.wait()
            self._after(st, t)
        elif k == "wait":
            self.safety.sleep(int(st["wait"]))
        elif k == "wait_for":
            info["waited_ms"] = self.wait_for(st["wait_for"])
        elif k == "nav":
            n = st["nav"]
            if not self.probe.on:
                raise Skip("nav needs the launcher probe")
            target = n["until"].get("focus")
            for i in range(n["max"] + 1):
                self.safety.sleep(60)
                if target and re.search(target, self.probe.state.get("focus") or ""):
                    info["presses"] = i
                    break
                if i == n["max"]:
                    raise StepFail(f"focus never reached /{target}/ (at {self.probe.state.get('focus')})")
                self.press([n["press"]], 80)
                self.safety.sleep(n["gap_ms"])
        elif k == "goto":
            if not self.probe.on and not self.a.mock:
                raise Skip("goto needs the launcher probe")
            t = now()
            if self.a.mock:
                self.world.command(st["goto"])
            else:
                self.probe.command(st["goto"])
            self.wait_for({"probe_text": r"^screen ", "timeout_ms": 3000}, t)
            self.settled(300, 3000, t)
        elif k == "sleep_wake":
            info.update(self.sleep_wake(j, idx, st))
        elif k == "restart_launcher":
            info.update(self.restart_launcher(j, idx, st))
        elif k == "mark":
            self.log.emit("note", text=str(st["mark"]))
        return info

    def restart_launcher(self, j, idx, st):
        """Ends the launcher the way tools/webkit-ab.sh does; rare-session-client starts it again."""
        if self.a.mock:
            t = now()
            self.log.emit("input", journey=j["name"], step=idx, button="restart_launcher", action="down", t_call=t)
            self.world._animate(self.world.frame, bytes(F.W * F.H), None)
            self.world._animate(bytes(F.W * F.H), self.world._scene(), None)
            return {}
        old = launcher_pid()
        flag = os.path.join(self.rt, "rare-launcher-restart")
        open(flag, "w").close()
        self.procs.expect_exit("rare-launcher", 40)
        t = now()
        self.log.emit("input", journey=j["name"], step=idx, button="restart_launcher", action="down", t_call=t)
        os.kill(old, signal.SIGTERM)
        end = time.monotonic() + 40
        new = None
        while time.monotonic() < end:
            self.safety.sleep(100)
            new = launcher_pid()
            if new and new != old:
                break
        if not new or new == old:
            raise StepFail("the launcher did not come back within 40 s")
        self.meta["launcher_pid"] = new
        self._after(dict(st, settle_timeout_ms=st.get("settle_timeout_ms", 15000), quiet_ms=st.get("quiet_ms", 1000)), t)
        self.procs.webkit = self.procs._webkit()
        return {"new_pid": new, "start_ms": round((now() - t) / 1e6)}

    def _after(self, st, t):
        if st.get("after_ms") is not None:
            self.safety.sleep(st["after_ms"])
        else:
            self.settled(st.get("quiet_ms", 300), st.get("settle_timeout_ms", 3000), t)
        if st.get("wait_for"):
            pass

    def sleep_wake(self, j, idx, st):
        """Presses A on the focused Sleep button, then waits for suspend and resume. RTC wake with --sudo."""
        s = st["sleep_wake"]
        f = self.probe.state.get("focus") or ""
        if not self.a.mock and not re.search(r'"Sleep"', f):
            raise StepFail(f"Sleep is not focused ({f})")
        if self.a.rtc_driver and not self.a.mock:
            # run.sh on the Mac holds the sudo password (in memory) and arms the RTC when asked.
            req, ack = os.path.join(self.out, "rtc-request"), os.path.join(self.out, "rtc-armed")
            for p in (req, ack):
                if os.path.exists(p):
                    os.unlink(p)
            open(req, "w").write(f"{int(s['seconds'])}\n")
            end = time.monotonic() + 20
            while not os.path.exists(ack):
                if time.monotonic() > end:
                    raise StepFail("run.sh did not arm the RTC wake alarm within 20 s")
                self.safety.sleep(100)
            if open(ack).read().strip() != "ok":
                raise StepFail("arming the RTC wake alarm failed: " + open(ack).read().strip()[:200])
        else:
            self.log.say("ATTENDED: the device will sleep; press the power button to wake it")
        boot0 = time.clock_gettime_ns(time.CLOCK_BOOTTIME) - now()
        t = now()
        # Waking (power button, resume noise on i8042/touch) is not someone using the device.
        self.exempt_until = t + int((25 + s["seconds"] + 120) * 1e9)
        self.press(["a"], 80)
        if self.a.mock:
            return {"slept_s": 0, "resumed_ns": now()}
        end_sleep = t + 20_000_000_000
        slept = None
        while now() < end_sleep:
            self.safety.sleep(100)
            gap = time.clock_gettime_ns(time.CLOCK_BOOTTIME) - now() - boot0
            if gap > 2_000_000_000:
                slept = gap
                break
        if slept is None:
            raise StepFail("the device did not sleep within 20 s")
        resumed = now()
        self.exempt_until = resumed + 5_000_000_000
        self.log.emit("note", text=f"resumed after {slept / 1e9:.1f} s asleep")
        return {"slept_s": round(slept / 1e9, 1), "resumed_ns": resumed}

    # ---- cleanup

    def snapshot(self):
        if self.a.mock:
            return {"scopes": set(), "steam": set()}
        scopes = set()
        try:
            out = subprocess.run(["systemctl", "--user", "list-units", "--plain", "--no-legend", "app-steam-app*"],
                                 capture_output=True, text=True, timeout=5).stdout
            scopes = {l.split()[0] for l in out.splitlines() if l.strip()}
        except Exception:
            pass
        return {"scopes": scopes, "steam": self._steam_game_pids()}

    @staticmethod
    def _steam_game_pids():
        out = set()
        for p in glob.glob("/proc/[0-9]*/environ"):
            try:
                env = open(p, "rb").read()
            except Exception:
                continue
            if b"SteamAppId=" in env or b"SteamGameId=" in env:
                pid = int(p.split("/")[2])
                try:
                    comm = open(f"/proc/{pid}/comm").read().strip()
                except Exception:
                    continue
                if comm not in LAUNCHER_COMMS + ("steam", "steamwebhelper", "bash", "sh"):
                    out.add(pid)
        return out

    def cleanup(self, before):
        """Back to Home with nothing the journey started still running. Returns (ok, notes)."""
        self.cleaning = True
        try:
            return self._cleanup(before)
        finally:
            self.cleaning = False

    def _cleanup(self, before):
        notes = []
        self.inj.ctx = {"journey": self.inj.ctx.get("journey"), "step": "cleanup"}
        try:
            if self.pad is not None:
                self.pad.release_all()
        except Exception as e:  # noqa
            notes.append(f"release: {e}")
        launcher = str(self.meta.get("launcher_app"))

        def wait(fn, secs):
            end = time.monotonic() + secs
            while time.monotonic() < end:
                if fn():
                    return True
                time.sleep(0.1)
            return fn()

        # 1. Quit what the journey started, the launcher's way first.
        after = self.snapshot()
        new_scopes = after["scopes"] - before["scopes"]
        new_steam = after["steam"] - before["steam"]
        running = "running=0" not in (self.probe.state.get("screen") or "running=0")
        if self.probe.on and (running or new_scopes or new_steam or self.focus.app != launcher):
            if self.a.mock:
                self.world.command("quit:all")
            else:
                self.probe.command("quit:all")
            if not wait(lambda: self.focus.app == launcher and not (self._steam_game_pids() - before["steam"]), 15):
                notes.append("quit:all did not finish in 15 s")
        # 2. Hard stop of anything still left.
        if not self.a.mock:
            after = self.snapshot()
            for sc in after["scopes"] - before["scopes"]:
                subprocess.run(["systemctl", "--user", "stop", sc], capture_output=True, timeout=15)
                notes.append(f"stopped {sc}")
            left = self._steam_game_pids() - before["steam"]
            if left:
                for pid in left:
                    try:
                        os.kill(pid, signal.SIGTERM)
                    except Exception:
                        pass
                time.sleep(5)
                for pid in self._steam_game_pids() - before["steam"]:
                    try:
                        os.kill(pid, signal.SIGKILL)
                    except Exception:
                        pass
                notes.append(f"terminated Steam game processes {sorted(left)}")
            # The launcher restarts itself under rare-session-client if it died.
            if not wait(lambda: launcher_pid() is not None, 30):
                return False, notes + ["launcher is not running"]
        # 3. Close sheets and go Home. Never with Guide: it toggles the Rare menu (pressed on Home it
        #    opens the menu, the 2026-10-27 baseline's "not Home: overlay=raremenu"). B closes the top
        #    sheet or goes back a tab and does nothing on a bare Home, so it is pressed until the probe
        #    says Home, at most 4 times; the probe's close and tab:home finish the job without input.
        def at_home():
            return self.focus.app == launcher and (self.probe.state.get("screen") or "").startswith("tab=home overlay=-")

        try:
            if self.focus.app != launcher and not self.a.mock:
                # Something else is in front after the quit: View + Menu held brings the launcher back.
                self.press(["select", "start"], 900)
                time.sleep(0.8)
            for _ in range(4):
                if self.probe.on and wait(at_home, 0.4):
                    break
                self.press(["b"], 60)
                time.sleep(0.3)
        except Abort:
            pass
        if self.probe.on and not wait(at_home, 0.5):
            self.command("close")
            self.command("tab:home")
            wait(at_home, 4)
        ok, why = self.confirm_home()
        if not ok:
            notes.append(why)
        return ok, notes

    def command(self, c):
        """A probe setup command (the simulated launcher takes it directly)."""
        if self.a.mock:
            self.world.command(c)
        else:
            self.probe.command(c)

    def confirm_home(self, timeout=3.0):
        """Home, confirmed: the launcher is the focused app and a screen line written after this
        call (the probe answers "state") says tab=home with no overlay. A stale line from before a
        press cannot pass. Without the probe only the focused app can be checked."""
        launcher = str(self.meta.get("launcher_app"))
        end = time.monotonic() + timeout
        while self.focus.app != launcher and time.monotonic() < end:
            time.sleep(0.05)
        if self.focus.app != launcher:
            return False, f"focused app {self.focus.app} is not the launcher"
        if not self.probe.on:
            self.log.emit("home_confirmed", probe=False)
            return True, ""
        t0 = now()
        self.command("state")
        last = None
        while time.monotonic() < end + 1.0:
            fresh = [x for x in self.probe.since(t0) if x[1].startswith("screen ")]
            if fresh:
                last = fresh[-1][1][len("screen "):]
                if last.startswith("tab=home overlay=-") and self.focus.app == launcher:
                    self.log.emit("home_confirmed", probe=True, screen=last)
                    return True, ""
            time.sleep(0.05)
        if last is None:
            return False, "Home not confirmed: the probe did not answer state"
        return False, f"not Home: {last}"

    # ---- journeys

    def journey(self, j):
        self.inj.ctx = {"journey": j["name"], "step": None}
        self.log.emit("journey_start", journey=j["name"])
        self.log.say(f"journey {j['name']}: {j['title']}")
        status, reason = "completed", ""
        before = self.snapshot()
        try:
            self.need_ok(j)
        except Skip as e:
            self.log.emit("journey_end", journey=j["name"], status="skipped", reason=str(e))
            self.log.say(f"  skipped: {e}")
            return True
        self.safety.journey_deadline = now() + int(j["timeout_s"] * 1e9)
        try:
            if j["start"] == "home":
                ok, notes = self.cleanup(before)
                if not ok:
                    raise StepFail("could not start from Home: " + "; ".join(notes))
                before = self.snapshot()
            for idx, st in enumerate(j["steps"]):
                self.safety.check()
                self.inj.ctx = {"journey": j["name"], "step": idx}
                self.log.emit("step_start", journey=j["name"], step=idx, name=st["name"], kind=st["kind"])
                try:
                    info = self.step(j, idx, st)
                    if st["kind"] != "wait_for" and st.get("wait_for"):
                        info["waited_ms"] = self.wait_for(st["wait_for"])
                    self.log.emit("step_end", journey=j["name"], step=idx, status="ok", info=info)
                except Skip as e:
                    self.log.emit("step_end", journey=j["name"], step=idx, status="skipped", reason=str(e))
                    status, reason = "skipped", str(e)
                    break
                except StepFail as e:
                    self.log.emit("step_end", journey=j["name"], step=idx, status="failed", reason=str(e), info=e.info)
                    self.log.say(f"  step {idx + 1} {st['name']}: FAILED {e}")
                    if not st.get("continue_on_fail"):
                        status, reason = "failed", f"step {idx + 1}: {e}"
                        break
                except Abort as e:
                    self.log.emit("step_end", journey=j["name"], step=idx, status="failed", reason=str(e))
                    raise
        except Abort as e:
            status, reason = ("timeout" if str(e) == "journey timeout" else "aborted"), str(e)
        except StepFail as e:
            status, reason = "failed", str(e)
        except Exception as e:  # noqa
            status, reason = "error", f"{type(e).__name__}: {e}"
        finally:
            self.safety.journey_deadline = None
            ok, notes = self.cleanup(before)
            self.log.emit("journey_end", journey=j["name"], status=status, reason=reason,
                          cleanup={"ok": ok, "notes": notes})
            self.sink.flush()
            self.log.say(f"  {status}{(': ' + reason) if reason else ''}; cleanup {'ok' if ok else 'FAILED ' + '; '.join(notes)}")
        if not ok:
            self.safety.abort("cleanup could not get back to Home; stopping the run")
        return ok and self.safety.reason is None

    def run(self, journeys):
        self.meta["journeys"] = journeys
        try:
            self.setup()
        except BaseException:
            # Aborted or failed before the run: nothing may stay pressed or plugged in.
            self.close_pad()
            try:
                self.teardown()
            except Exception:
                pass
            raise
        json.dump(self.meta, open(os.path.join(self.out, "meta.json"), "w"), indent=1, default=str)
        self.log.emit("run_start", launcher_app=self.meta.get("launcher_app"))
        try:
            # Wake the probe: it turns on at the first press after the flag file appears. B, never
            # Guide: on Home B does nothing, Guide opens the Rare menu.
            self.inj.ctx = {"journey": None, "step": "preflight"}
            self.press(["b"], 60)
            end = time.monotonic() + 4
            while time.monotonic() < end and not self.probe.on:
                time.sleep(0.05)
            self.meta["probe"] = self.probe.on
            self.log.say(f"probe {'on' if self.probe.on else 'off (launcher without the journey probe: screen/focus checks and nav steps are skipped)'}")
            for _ in range(self.a.repeat):
                for j in journeys:
                    if self.safety.reason:
                        break
                    if not self.journey(j):
                        break
        except Abort:
            pass
        finally:
            try:
                self.inj.ctx = {"journey": None, "step": "final"}
                self.cleanup(self.snapshot())
            except Exception as e:  # noqa
                self.log.emit("note", text=f"final cleanup: {e}")
            self.log.emit("run_end", reason=self.safety.reason or "done", foreign_inputs=self.inj.foreign)
            self.teardown()
            open(os.path.join(self.out, "done"), "w").write((self.safety.reason or "done") + "\n")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--i-have-a-window", action="store_true", help="required: the device is yours to drive now")
    ap.add_argument("--out", required=True)
    ap.add_argument("--journeys", default=os.path.join(os.path.dirname(HERE), "journeys"))
    ap.add_argument("--only", default="")
    ap.add_argument("--skip", default="")
    ap.add_argument("--tags", default="")
    ap.add_argument("--repeat", type=int, default=1)
    ap.add_argument("--max-minutes", type=float, default=45)
    ap.add_argument("--stop-file", default="")
    ap.add_argument("--no-video", action="store_true")
    ap.add_argument("--no-capture", action="store_true")
    ap.add_argument("--pipeline", default=None, help="force one capture pipeline (va-shm, va-dmabuf, thumbs-shm, thumbs-dmabuf)")
    ap.add_argument("--allow-uinput", action="store_true", help="kept for old scripts: the virtual pad fallback is automatic now")
    ap.add_argument("--ip-only", action="store_true", help="never fall back to the virtual uinput pad when InputPlumber fails")
    ap.add_argument("--rtc-driver", action="store_true", help="run.sh arms the RTC wake alarm when the runner asks (it holds the sudo password)")
    ap.add_argument("--attended", action="store_true", help="the owner is at the device (sleep/wake waits for a power press)")
    ap.add_argument("--idle-min", type=float, default=5, help="required quiet minutes before starting (minimum 5 on the device)")
    ap.add_argument("--wait-max", type=float, default=0, help="keep waiting for the device to go idle up to this many minutes")
    ap.add_argument("--launcher-app", default="1380012613", help="launcher app id when its scope name does not say")
    ap.add_argument("--mock", action="store_true", help="no device: a simulated launcher (for tests)")
    ap.add_argument("--mock-foreign", type=float, default=0, help="mock only: physical input after this many seconds")
    ap.add_argument("--mock-faults", default="", help="mock only: black=rb:2,flash=lb:1,hitch=down:3")
    ap.add_argument("--mock-ipfail", type=int, default=0, help="mock only: InputPlumber times out on its Nth call")
    ap.add_argument("--mock-guard", default="", help="mock only: exit:S kills the input guard after S seconds, stall:S stops its heartbeat")
    a = ap.parse_args()
    if not a.i_have_a_window and not a.mock:
        print("refusing to drive the device without --i-have-a-window", file=sys.stderr)
        return 2
    if not a.mock and a.idle_min < 5:
        print("the idle guard needs at least 5 minutes without input (owner's rule)", file=sys.stderr)
        return 2
    js = spec.load_all(a.journeys, only=[x for x in a.only.split(",") if x] or None,
                       skip=[x for x in a.skip.split(",") if x] or None, tags=[x for x in a.tags.split(",") if x] or None)
    r = Runner(a)
    lock_path = os.path.join(r.rt, "rare-journeys.lock")
    lock = open(lock_path, "w")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        print("another journey run is active", file=sys.stderr)
        return 3

    def on_signal(sig, _):
        r.release_pad()
        r.safety.lock(f"signal {sig}")

    for s in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        signal.signal(s, on_signal)
    atexit.register(r.close_pad)
    try:
        r.run(js)
    finally:
        r.close_pad()
    return 0


if __name__ == "__main__":
    sys.exit(main())
