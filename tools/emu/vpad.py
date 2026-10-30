#!/usr/bin/env python3
"""A virtual pad that reports exactly like the SuiPlay's InputPlumber target, for emulator input
tests on the build machine (docs/EMULATION.md, "Proof").

It makes two uinput devices, in this order, as the SuiPlay has them:

1. "Microsoft X-Box 360 pad" 045e:028e v0200: the raw pad (xpad). InputPlumber grabs it on the
   device, so it never sends anything here either. An emulator bound to it sees no presses.
2. "Microsoft X-Box One Elite 2 pad" 045e:0b00 v0001: InputPlumber's xbox-elite target
   (src/input/target/xpad.rs): the same keys (incl. BTN_TRIGGER_HAPPY1-8), axes, ranges, fuzz,
   flat and force feedback.

Commands come one per line on a FIFO (default /run/rare-vpad.fifo):
  press <BTN>  [ms]     e.g. press START 200   (BTN_ prefix optional; A B X Y TL TR SELECT START
                                                MODE THUMBL THUMBR, or DUP DDOWN DLEFT DRIGHT)
  axis <ABS> <value> [ms]   e.g. axis Z 255 300, axis X -32768 300 (then back to rest)
  neutral
  quit
Every device is put back to rest and destroyed on exit (SIGTERM, SIGINT, quit).
Run as root; it makes the event nodes readable by everyone so emulators started as the user see them.
"""
import os
import signal
import sys
import time

import evdev
from evdev import AbsInfo, UInput, ecodes as e

FIFO = sys.argv[1] if len(sys.argv) > 1 else "/run/rare-vpad.fifo"

STICK = AbsInfo(value=0, min=-32768, max=32767, fuzz=16, flat=128, resolution=1)
TRIG = AbsInfo(value=0, min=0, max=255, fuzz=0, flat=0, resolution=1)
HAT = AbsInfo(value=0, min=-1, max=1, fuzz=0, flat=0, resolution=1)
AXES = [(e.ABS_X, STICK), (e.ABS_Y, STICK), (e.ABS_RX, STICK), (e.ABS_RY, STICK), (e.ABS_Z, TRIG), (e.ABS_RZ, TRIG), (e.ABS_HAT0X, HAT), (e.ABS_HAT0Y, HAT)]
BASE_KEYS = [e.BTN_SOUTH, e.BTN_EAST, e.BTN_NORTH, e.BTN_WEST, e.BTN_TL, e.BTN_TR, e.BTN_SELECT, e.BTN_START, e.BTN_MODE, e.BTN_THUMBL, e.BTN_THUMBR]
FF = [e.FF_RUMBLE, e.FF_PERIODIC, e.FF_SQUARE, e.FF_TRIANGLE, e.FF_SINE, e.FF_GAIN]


def make(name, product, version, keys, ff):
    caps = {e.EV_KEY: keys, e.EV_ABS: AXES}
    if ff:
        caps[e.EV_FF] = ff
    return UInput(caps, name=name, vendor=0x045E, product=product, version=version, bustype=e.BUS_USB, max_effects=16 if ff else 0)


raw = make("Microsoft X-Box 360 pad", 0x028E, 0x0200, [e.BTN_A, e.BTN_B, e.BTN_X, e.BTN_Y, e.BTN_TL, e.BTN_TR, e.BTN_SELECT, e.BTN_START, e.BTN_MODE, e.BTN_THUMBL, e.BTN_THUMBR], [e.FF_RUMBLE])
time.sleep(0.3)
pad = make("Microsoft X-Box One Elite 2 pad", 0x0B00, 0x0001, BASE_KEYS + [e.BTN_TRIGGER_HAPPY1 + i for i in range(8)], FF)
time.sleep(0.5)

for u in (raw, pad):
    for node in [u.device.path] + [os.path.join("/dev/input", j) for j in os.listdir(os.path.join("/sys/class/input", os.path.basename(u.device.path), "device")) if j.startswith("js")]:
        os.chmod(node, 0o666)
    print(f"{u.name}: {u.device.path}", flush=True)

REST = {e.ABS_X: 0, e.ABS_Y: 0, e.ABS_RX: 0, e.ABS_RY: 0, e.ABS_Z: 0, e.ABS_RZ: 0, e.ABS_HAT0X: 0, e.ABS_HAT0Y: 0}
DPAD = {"DUP": (e.ABS_HAT0Y, -1), "DDOWN": (e.ABS_HAT0Y, 1), "DLEFT": (e.ABS_HAT0X, -1), "DRIGHT": (e.ABS_HAT0X, 1)}


def neutral():
    for code in BASE_KEYS:
        pad.write(e.EV_KEY, code, 0)
    for code, v in REST.items():
        pad.write(e.EV_ABS, code, v)
    pad.syn()


def done(*_):
    try:
        neutral()
        pad.close()
        raw.close()
    finally:
        try:
            os.unlink(FIFO)
        except OSError:
            pass
        os._exit(0)


def ff_loop():
    """Answers force feedback uploads as InputPlumber does (or the game's EVIOCSFF blocks) and logs
    each upload and play, which is the rumble proof."""
    import select

    while True:
        select.select([pad.fd], [], [])
        try:
            for ev in pad.read():
                if ev.type == e.EV_UINPUT and ev.code == e.UI_FF_UPLOAD:
                    up = pad.begin_upload(ev.value)
                    up.retval = 0
                    eff = up.effect
                    strong = weak = 0
                    if eff.type == e.FF_RUMBLE:
                        strong, weak = eff.u.ff_rumble_effect.strong_magnitude, eff.u.ff_rumble_effect.weak_magnitude
                    print(f"ff upload type={eff.type} strong={strong} weak={weak}", flush=True)
                    pad.end_upload(up)
                elif ev.type == e.EV_UINPUT and ev.code == e.UI_FF_ERASE:
                    er = pad.begin_erase(ev.value)
                    er.retval = 0
                    pad.end_erase(er)
                elif ev.type == e.EV_FF:
                    print(f"ff play id={ev.code} value={ev.value}", flush=True)
        except OSError:
            pass


import threading

threading.Thread(target=ff_loop, daemon=True).start()

signal.signal(signal.SIGTERM, done)
signal.signal(signal.SIGINT, done)

try:
    os.unlink(FIFO)
except OSError:
    pass
os.mkfifo(FIFO, 0o666)
os.chmod(FIFO, 0o666)
neutral()
print("ready", flush=True)

while True:
    with open(FIFO) as f:
        for line in f:
            w = line.split()
            if not w:
                continue
            cmd = w[0]
            try:
                if cmd == "quit":
                    done()
                elif cmd == "neutral":
                    neutral()
                elif cmd == "press":
                    name = w[1].upper()
                    ms = int(w[2]) if len(w) > 2 else 200
                    if name in DPAD:
                        code, v = DPAD[name]
                        pad.write(e.EV_ABS, code, v)
                        pad.syn()
                        time.sleep(ms / 1000)
                        pad.write(e.EV_ABS, code, 0)
                    else:
                        code = e.ecodes[name if name.startswith("BTN_") else "BTN_" + name]
                        pad.write(e.EV_KEY, code, 1)
                        pad.syn()
                        time.sleep(ms / 1000)
                        pad.write(e.EV_KEY, code, 0)
                    pad.syn()
                elif cmd == "axis":
                    code = e.ecodes["ABS_" + w[1].upper()]
                    ms = int(w[3]) if len(w) > 3 else 200
                    pad.write(e.EV_ABS, code, int(w[2]))
                    pad.syn()
                    time.sleep(ms / 1000)
                    pad.write(e.EV_ABS, code, REST.get(code, 0))
                    pad.syn()
                print("ok", line.strip(), flush=True)
            except Exception as x:  # a bad line never takes the pad down
                print("err", line.strip(), x, flush=True)
