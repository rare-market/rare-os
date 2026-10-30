#!/usr/bin/env python3
"""Quiet-window check for launcher restarts (apply-restart.sh): exits 0 after SECS seconds with no
physical input, 1 the moment someone touches the device (prints what), 2 on a usage error.

Reads (never grabs) the pads, keyboard, mouse and power button the launcher sees, with the journey
runner's rules for what counts (buttons and keys always, sticks past 30 % of travel, triggers past
10 %, hats off centre), plus the i8042 / touchscreen / xpad interrupt counters. The journey runner's
own virtual pad ("Rare Journeys") is not physical input and is skipped. Without python-evdev only the
interrupt counters are used.

    idle.py SECS
"""
import os
import re
import select
import sys
import time

IRQ_RX = re.compile(r"i8042|GDIX|goodix|xpad", re.I)
DEV_RX = re.compile(r"X-Box|Xbox|Steam Deck|InputPlumber Keyboard|InputPlumber Mouse|Power Button", re.I)


def irq_counts():
    out = {}
    try:
        for line in open("/proc/interrupts"):
            if IRQ_RX.search(line):
                parts = line.split()
                out[parts[-1]] = sum(int(x) for x in parts[1:] if x.isdigit())
    except OSError:
        pass
    return out


def open_devices():
    try:
        from evdev import InputDevice, ecodes as e, list_devices
    except ImportError:
        return {}, None
    devs = {}
    for p in list_devices():
        try:
            d = InputDevice(p)
        except OSError:
            continue
        try:
            keys = d.capabilities().get(e.EV_KEY, [])
            pad = e.BTN_THUMBL in keys and e.BTN_THUMBR in keys
            motion = e.INPUT_PROP_ACCELEROMETER in d.input_props() or "Motion" in d.name
            if (pad or DEV_RX.search(d.name)) and not motion and "Rare Journeys" not in d.name:
                absinfo = {c: i for c, i in d.capabilities().get(e.EV_ABS, [])}
                devs[d.fd] = (d, absinfo)
                continue
        except OSError:
            pass
        d.close()
    return devs, e


def significant(ev, absinfo, e):
    if ev.type == e.EV_KEY:
        return True
    if ev.type == e.EV_REL:
        return abs(ev.value) >= 3
    if ev.type == e.EV_ABS:
        if ev.code in (e.ABS_HAT0X, e.ABS_HAT0Y):
            return ev.value != 0
        info = absinfo.get(ev.code)
        if not info or info.max <= info.min:
            return False
        span = info.max - info.min
        if info.min < 0:
            return abs(ev.value - (info.max + info.min) / 2) > 0.3 * span / 2
        return ev.value - info.min > 0.1 * span
    return False


def main():
    if len(sys.argv) != 2:
        print(__doc__.strip().splitlines()[-1].strip(), file=sys.stderr)
        return 2
    secs = float(sys.argv[1])
    devs, e = open_devices()
    base = irq_counts()
    end = time.monotonic() + secs
    while True:
        left = end - time.monotonic()
        if left <= 0:
            return 0
        r = select.select(list(devs), [], [], min(left, 0.25))[0] if devs else (time.sleep(min(left, 0.25)) or [])
        for fd in r:
            d, absinfo = devs[fd]
            try:
                for ev in d.read():
                    if significant(ev, absinfo, e):
                        print(f"input: {d.name}")
                        return 1
            except OSError:  # unplugged
                devs.pop(fd, None)
        c = irq_counts()
        moved = [k for k in c if c[k] != base.get(k, c[k])]
        if moved:
            print(f"input: interrupts {', '.join(moved)}")
            return 1


if __name__ == "__main__":
    sys.exit(main())
