#!/usr/bin/env python3
"""A virtual multitouch touchscreen (uinput, type B slots) that plays gestures.

  vtouch.py MODE W H -- STEP...
  MODE: plain (a landscape W x H panel) or rot270 (a portrait-native H x W panel mounted turned,
  like the SuiPlay's: points are given in window coordinates and sent as the panel's raw ones).
  STEP: wait:MS | tap:X:Y | hold:X:Y | drag:X1:Y1:X2:Y2:MS | pinch:X:Y
Prints the event node, waits for the device to be picked up (SETTLE s), plays, destroys it.
"""
import fcntl, os, struct, sys, time

UI_SET_EVBIT, UI_SET_KEYBIT, UI_SET_ABSBIT, UI_SET_PROPBIT = 0x40045564, 0x40045565, 0x40045567, 0x4004556E
UI_DEV_SETUP, UI_ABS_SETUP, UI_DEV_CREATE, UI_DEV_DESTROY = 0x405C5503, 0x401C5504, 0x5501, 0x5502
EV_SYN, EV_KEY, EV_ABS = 0, 1, 3
BTN_TOUCH, BTN_TOOL_FINGER = 0x14A, 0x145
ABS_X, ABS_Y, ABS_MT_SLOT, ABS_MT_X, ABS_MT_Y, ABS_MT_TID = 0, 1, 0x2F, 0x35, 0x36, 0x39

mode, W, H = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
steps = sys.argv[sys.argv.index("--") + 1:]
# raw panel size
PW, PH = (W, H) if mode == "plain" else (H, W)

def raw(x, y):
    if mode == "plain":
        return int(x), int(y)
    # window (U, V) -> panel (u, v) with gamescope's 270 table inverted: u = 1 - V, v = U
    U, V = x / W, y / H
    return int(round((1 - V) * (PW - 1))), int(round(U * (PH - 1)))

fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
for ev in (EV_KEY, EV_ABS):
    fcntl.ioctl(fd, UI_SET_EVBIT, ev)
for k in (BTN_TOUCH, BTN_TOOL_FINGER):
    fcntl.ioctl(fd, UI_SET_KEYBIT, k)
fcntl.ioctl(fd, UI_SET_PROPBIT, 1)  # INPUT_PROP_DIRECT
def absset(code, mx):
    fcntl.ioctl(fd, UI_SET_ABSBIT, code)
    fcntl.ioctl(fd, UI_ABS_SETUP, struct.pack("HH6i", code, 0, 0, 0, mx, 0, 0, 10))
absset(ABS_X, PW - 1); absset(ABS_Y, PH - 1)
absset(ABS_MT_SLOT, 9); absset(ABS_MT_X, PW - 1); absset(ABS_MT_Y, PH - 1); absset(ABS_MT_TID, 65535)
name = b"Rare Test Touch"
fcntl.ioctl(fd, UI_DEV_SETUP, struct.pack("HHHH80sI", 0x18, 0x416, 0x38F, 1, name, 0))
fcntl.ioctl(fd, UI_DEV_CREATE)
time.sleep(0.3)
node = None
for d in os.listdir("/sys/class/input"):
    if d.startswith("event"):
        try:
            if open(f"/sys/class/input/{d}/device/name").read().strip() == name.decode():
                node = "/dev/input/" + d
        except OSError:
            pass
print("NODE", node, flush=True)
os.chmod(node, 0o666)
time.sleep(float(os.environ.get("SETTLE", "4")))

def emit(t, c, v):
    s, us = divmod(int(time.time() * 1e6), 1_000_000)
    os.write(fd, struct.pack("qqHHi", s, us, t, c, v))
def syn():
    emit(EV_SYN, 0, 0)
tid = [100]
def down(slot, x, y, first):
    rx, ry = raw(x, y)
    emit(EV_ABS, ABS_MT_SLOT, slot); tid[0] += 1; emit(EV_ABS, ABS_MT_TID, tid[0])
    emit(EV_ABS, ABS_MT_X, rx); emit(EV_ABS, ABS_MT_Y, ry)
    if first:
        emit(EV_KEY, BTN_TOUCH, 1); emit(EV_KEY, BTN_TOOL_FINGER, 1); emit(EV_ABS, ABS_X, rx); emit(EV_ABS, ABS_Y, ry)
    syn()
def move(slot, x, y, first):
    rx, ry = raw(x, y)
    emit(EV_ABS, ABS_MT_SLOT, slot); emit(EV_ABS, ABS_MT_X, rx); emit(EV_ABS, ABS_MT_Y, ry)
    if first:
        emit(EV_ABS, ABS_X, rx); emit(EV_ABS, ABS_Y, ry)
    syn()
def up(slot, last):
    emit(EV_ABS, ABS_MT_SLOT, slot); emit(EV_ABS, ABS_MT_TID, -1)
    if last:
        emit(EV_KEY, BTN_TOUCH, 0); emit(EV_KEY, BTN_TOOL_FINGER, 0)
    syn()

try:
    for st in steps:
        k, *a = st.split(":")
        a = [float(v) for v in a]
        print("STEP", st, flush=True)
        if k == "wait":
            time.sleep(a[0] / 1000)
        elif k in ("tap", "hold"):
            down(0, a[0], a[1], True); time.sleep(0.06 if k == "tap" else 0.9); up(0, True)
        elif k == "drag":
            x1, y1, x2, y2, ms = a
            n = max(2, int(ms / 12))
            down(0, x1, y1, True)
            for i in range(1, n + 1):
                time.sleep(ms / 1000 / n); f = i / n
                move(0, x1 + (x2 - x1) * f, y1 + (y2 - y1) * f, True)
            up(0, True)
        elif k == "pinch":
            x, y = a
            down(0, x - 50, y, True); down(1, x + 50, y, False)
            for i in range(1, 12):
                time.sleep(0.015); move(0, x - 50 - i * 15, y, True); move(1, x + 50 + i * 15, y, False)
            up(1, False); up(0, True)
        time.sleep(0.05)
finally:
    time.sleep(0.2)
    fcntl.ioctl(fd, UI_DEV_DESTROY)
    os.close(fd)
