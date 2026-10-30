#!/usr/bin/env python3
"""A virtual controller for device focus checks: the launcher reads pads from evdev (gilrs), so its
View-hold overlay and View + Menu way out only answer a real evdev pad, never keys. Needs write
access to /dev/uinput (the seat user has it on Rare OS and Bazzite).

  pad.py step...    step: a button (a b x y lb rb select start guide up down left right),
                    a chord held for S seconds as  select+start:0.8  or  select:0.7,
                    or wait:<seconds>
The pad appears as "Rare Test Pad" (BUS_VIRTUAL 1209:0001, so InputPlumber leaves it alone), lives
for the steps, then goes away. idle.py ignores it.
Needs a live idle guard (guard-run.sh, beat.py): before every press the guard's heartbeat must be
under 1 s old, else it presses nothing more, removes the pad (releasing everything), calls
beat.lost() (stop, restore while armed) and exits 4.
"""
import fcntl, os, signal, struct, sys, time

EV_SYN, EV_KEY, EV_ABS = 0, 1, 3
UI_SET_EVBIT, UI_SET_KEYBIT, UI_SET_ABSBIT, UI_DEV_CREATE, UI_DEV_DESTROY = 0x40045564, 0x40045565, 0x40045567, 0x5501, 0x5502
BTN = {"a": 304, "b": 305, "x": 307, "y": 308, "lb": 310, "rb": 311, "select": 314, "start": 315, "guide": 316, "l3": 317, "r3": 318}
HAT = {"left": (16, -1), "right": (16, 1), "up": (17, -1), "down": (17, 1)}
AXES = [0, 1, 2, 3, 4, 5, 16, 17]
NAME = b"Rare Test Pad"


def create():
    fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
    for ev in (EV_KEY, EV_ABS, EV_SYN):
        fcntl.ioctl(fd, UI_SET_EVBIT, ev)
    for code in BTN.values():
        fcntl.ioctl(fd, UI_SET_KEYBIT, code)
    for a in AXES:
        fcntl.ioctl(fd, UI_SET_ABSBIT, a)
    absmax, absmin = [0] * 64, [0] * 64
    for a in (0, 1, 3, 4):
        absmin[a], absmax[a] = -32768, 32767
    for a in (2, 5):
        absmin[a], absmax[a] = 0, 255
    for a in (16, 17):
        absmin[a], absmax[a] = -1, 1
    dev = struct.pack("80sHHHHi", NAME, 0x06, 0x1209, 0x0001, 1, 0)
    dev += struct.pack("64i", *absmax) + struct.pack("64i", *absmin) + struct.pack("64i", *([0] * 64)) + struct.pack("64i", *([0] * 64))
    os.write(fd, dev)
    fcntl.ioctl(fd, UI_DEV_CREATE)
    return fd


def emit(fd, typ, code, val):
    t = time.time()
    os.write(fd, struct.pack("llHHi", int(t), int((t % 1) * 1e6), typ, code, val))


def sync(fd):
    emit(fd, EV_SYN, 0, 0)


def press(fd, names, hold, ok=lambda: True):
    """Holds the buttons for `hold` s, or until ok() turns False (the guard went away)."""
    for n in names:
        if n in HAT:
            emit(fd, EV_ABS, HAT[n][0], HAT[n][1])
        else:
            emit(fd, EV_KEY, BTN[n], 1)
    sync(fd)
    end = time.monotonic() + hold
    while time.monotonic() < end and ok():
        time.sleep(min(0.05, max(0.0, end - time.monotonic())))
    for n in names:
        if n in HAT:
            emit(fd, EV_ABS, HAT[n][0], 0)
        else:
            emit(fd, EV_KEY, BTN[n], 0)
    sync(fd)


def load_beat():
    """beat.py next to this file (imported by path: python3 -I leaves the script dir off sys.path)."""
    import importlib.util
    spec = importlib.util.spec_from_file_location("rare_beat", os.path.join(os.path.dirname(os.path.realpath(__file__)), "beat.py"))
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


class GuardLost(Exception):
    pass


def main():
    beat = load_beat()
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))  # so the pad is removed on a kill too

    def guard_ok():
        why = beat.check()
        if why:
            raise GuardLost(why)

    def pause(secs):
        end = time.monotonic() + secs
        while time.monotonic() < end:
            guard_ok()
            time.sleep(min(0.05, max(0.0, end - time.monotonic())))

    guard_ok()
    fd = create()
    lost = None
    try:
        pause(float(os.environ.get("PAD_SETTLE", "2")))
        for step in sys.argv[1:]:
            if step.startswith("wait:"):
                pause(float(step[5:]))
                continue
            guard_ok()
            names, _, hold = step.partition(":")
            press(fd, names.split("+"), float(hold) if hold else 0.08, ok=lambda: beat.check() is None)
            print(f"{time.time():.3f} pad {step}", flush=True)
            pause(float(os.environ.get("PAD_GAP", "0.6")))
    except GuardLost as e:
        lost = str(e)
        print(f"{time.time():.3f} stop: {lost}", flush=True)
    finally:
        fcntl.ioctl(fd, UI_DEV_DESTROY)
        os.close(fd)
    if lost:
        beat.lost(lost)
        sys.exit(4)


if __name__ == "__main__":
    main()
