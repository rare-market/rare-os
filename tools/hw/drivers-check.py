#!/usr/bin/env python3
"""Read-only driver checks on the SuiPlay0X1. Nothing on the device is changed, except where a
mode says "root" and restores what it touched.

Run from the Mac without copying anything to the device:

    ssh suiplay python3 -I - buttons 20 < tools/hw/drivers-check.py
    ssh suiplay python3 -I - pad 20 < tools/hw/drivers-check.py
    ssh suiplay python3 -I - touch 10 < tools/hw/drivers-check.py
    ssh -t suiplay sudo python3 -I - imu < tools/hw/drivers-check.py      (root; Steam must have a game
                                                                       or the screen, so the pad is deck-uhid)
    ssh -t suiplay sudo python3 -I - latency 30 < tools/hw/drivers-check.py   (root; needs usbmon loaded)

buttons  Press LC, RC, the Rare button and Quick, one at a time, then each held 1 s. Shows the
         i8042 interrupts per second (proof the keys reach the kernel even while InputPlumber holds
         the keyboard) and, with the rare_suiplay capability map installed, InputPlumber's D-Bus
         InputEvent signals (ui_screenshot, ui_osk, ui_guide, ui_quick; 1 = down, 0 = up).
         Root adds the raw scancodes (i8042.debug on for the window, then put back).
pad      Move both sticks to every edge, circle them, pull both triggers fully, press every button.
         Reads the virtual pad InputPlumber makes (Xbox Elite or Steam Deck): stick range and rest
         value, the smallest non-zero deflection (the pad's own dead zone), trigger range, buttons.
touch    Drag one finger steadily on the screen for the whole window: Goodix interrupts per second
         (one per report) = the touch report rate.
imu      Put the device in each pose when asked. Reads "Steam Deck Motion Sensors" (hid-steam,
         SDL/DualSense frame: x right, y out of the screen, z toward the player) and says whether
         gravity lands on the axis the mount matrix promises.
latency  Press A (or any face button) about once a second. Pairs the pad's USB interrupt transfer
         (usbmon) with the matching EV_KEY on InputPlumber's virtual pad: USB to virtual-pad delay.
"""
import array
import ctypes
import fcntl
import glob
import os
import re
import select
import struct
import subprocess
import sys
import time

EV_FMT = "llHHi"
EV_SIZE = struct.calcsize(EV_FMT)
EV_SYN, EV_KEY, EV_ABS = 0, 1, 3
EVIOCSCLOCKID = 0x400445A0
CLOCK_REALTIME = 0


def devices():
    """(name, eventN) from /proc/bus/input/devices."""
    out, name = [], None
    for line in open("/proc/bus/input/devices"):
        if line.startswith("N: Name="):
            name = line.split("=", 1)[1].strip().strip('"')
        elif line.startswith("H: Handlers="):
            m = re.search(r"\bevent(\d+)", line)
            if m and name is not None:
                out.append((name, "/dev/input/event" + m.group(1)))
    return out


def find(*names):
    for n, path in devices():
        if any(x.lower() == n.lower() for x in names):
            return n, path
    return None, None


def irq_count(match):
    for line in open("/proc/interrupts"):
        if match in line:
            parts = line.split()
            return sum(int(p) for p in parts[1:] if p.isdigit())
    return None


def absinfo(fd, code):
    buf = array.array("i", [0] * 6)
    fcntl.ioctl(fd, 0x80184540 + code, buf)  # EVIOCGABS(code)
    return buf.tolist()  # value, min, max, fuzz, flat, resolution


def read_events(fd, timeout):
    r, _, _ = select.select([fd], [], [], timeout)
    if not r:
        return []
    data = os.read(fd, EV_SIZE * 64)
    return [struct.unpack(EV_FMT, data[i:i + EV_SIZE]) for i in range(0, len(data) - EV_SIZE + 1, EV_SIZE)]


def cmd_buttons(secs):
    mon = None
    if os.path.exists("/etc/inputplumber/capability_maps.d/rare_suiplay.yaml"):
        mon = subprocess.Popen(["gdbus", "monitor", "--system", "--dest", "org.shadowblip.InputPlumber"],
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, bufsize=1)
    root = os.geteuid() == 0
    dbg_old = None
    if root:
        p = "/sys/module/i8042/parameters/debug"
        dbg_old = open(p).read().strip()
        open(p, "w").write("1")
        mark = subprocess.run(["dmesg", "-x"], capture_output=True, text=True).stdout.count("\n")
    print(f"press LC, RC, Rare, Quick now ({secs} s)")
    last = irq_count("i8042")
    end = time.time() + secs
    try:
        while time.time() < end:
            t = time.time() + 1
            while time.time() < t:
                if mon:
                    r, _, _ = select.select([mon.stdout], [], [], max(0, t - time.time()))
                    if r:
                        line = mon.stdout.readline()
                        if "InputEvent" in line:
                            print("  ip:", line.strip()[-60:])
                else:
                    time.sleep(max(0, t - time.time()))
            now = irq_count("i8042")
            if now is not None and last is not None and now != last:
                print(f"  i8042 interrupts +{now - last}")
            last = now
    finally:
        if mon:
            mon.terminate()
        if root and dbg_old is not None:
            open("/sys/module/i8042/parameters/debug", "w").write(dbg_old)
    if root:
        lines = subprocess.run(["dmesg", "-x"], capture_output=True, text=True).stdout.splitlines()[mark:]
        codes = [m.group(1) for l in lines for m in [re.search(r"i8042: \[\d+\] ([0-9a-f]{2}) <- i8042 \(interrupt, 0", l)] if m]
        print("raw scancodes (set 1 translated; release = code | 0x80):", " ".join(codes) or "none")
        print("expected on BIOS 2.19_S20: LC 6c, RC 6d, Rare 6e, Quick 76 (hwdb: F15 F16 F17 F18)")


def cmd_pad(secs):
    name, path = find("Microsoft X-Box One Elite 2 pad", "Steam Deck")
    if not path:
        sys.exit("no InputPlumber virtual pad")
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
    axes = {0: "LX", 1: "LY", 3: "RX", 4: "RY", 2: "LT", 5: "RT", 16: "HAT_X", 17: "HAT_Y"}
    info = {c: absinfo(fd, c) for c in axes}
    seen = {c: [info[c][0], info[c][0]] for c in axes}
    small = {c: None for c in (0, 1, 3, 4)}
    keys = set()
    print(f"{name}: move sticks to every edge, pull triggers, press every button ({secs} s)")
    end = time.time() + secs
    while time.time() < end:
        for _, _, typ, code, val in read_events(fd, 0.2):
            if typ == EV_ABS and code in axes:
                lo, hi = seen[code]
                seen[code] = [min(lo, val), max(hi, val)]
                if code in small and val not in (0, -1) and (small[code] is None or abs(val) < small[code]):
                    small[code] = abs(val)
            elif typ == EV_KEY and val == 1:
                keys.add(code)
    os.close(fd)
    for c, n in axes.items():
        v, mn, mx, fuzz, flat, _ = info[c]
        lo, hi = seen[c]
        cov = 100.0 * (hi - lo) / (mx - mn) if mx > mn else 0
        extra = f"  smallest |value| off centre {small[c]} ({100.0 * small[c] / mx:.1f} %)" if c in small and small[c] else ""
        print(f"  {n:6} range {mn}..{mx} fuzz {fuzz} flat {flat}  seen {lo}..{hi} ({cov:.0f} %){extra}")
    print("  buttons seen (evdev codes):", sorted(keys))
    print("  Xbox Elite: 304 A 305 B 307 X 308 Y 310 LB 311 RB 314 View 315 Menu 316 Guide 317 L3 318 R3")


def cmd_touch(secs):
    a = irq_count("GDIX")
    print(f"drag one finger steadily for {secs} s")
    time.sleep(secs)
    b = irq_count("GDIX")
    print(f"Goodix interrupts: {b - a} in {secs} s = {(b - a) / secs:.0f} per second while touching")


def cmd_imu():
    name, path = find("Steam Deck Motion Sensors")
    if not path:
        sys.exit("no 'Steam Deck Motion Sensors': start Steam (or a Steam game) so the pad target is deck-uhid")
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
    poses = [
        ("flat on the table, screen up", 1, +1),
        ("standing on its bottom edge, screen facing you", 2, -1),
        ("lying on its left edge (right edge up), screen facing you", 0, +1),
    ]
    ok = True
    tty = open("/dev/tty")  # stdin is this script when run as `python3 -I - imu < drivers-check.py`
    for text, axis, sign in poses:
        print(f"pose: {text}; hold still, then press Enter ", end="", flush=True)
        tty.readline()
        acc = {0: [], 1: [], 2: []}
        end = time.time() + 1.0
        while time.time() < end:
            for _, _, typ, code, val in read_events(fd, 0.1):
                if typ == EV_ABS and code in acc:
                    acc[code].append(val)
        mean = {c: (sum(v) / len(v) if v else absinfo(fd, c)[0]) for c, v in acc.items()}
        big = max(mean, key=lambda c: abs(mean[c]))
        good = big == axis and (mean[big] > 0) == (sign > 0)
        ok &= good
        print(f"  x {mean[0]:8.0f}  y {mean[1]:8.0f}  z {mean[2]:8.0f}  -> {'OK' if good else 'WRONG'}"
              f" (expected {'+' if sign > 0 else '-'}{'xyz'[axis]})")
    os.close(fd)
    print("mount matrix", "verified" if ok else "WRONG: report the three lines above")


def cmd_latency(secs):
    mon = "/sys/kernel/debug/usb/usbmon/1u"
    if not os.path.exists(mon):
        sys.exit("usbmon not loaded: sudo modprobe usbmon (unload after with sudo modprobe -r usbmon)")
    name, path = find("Microsoft X-Box One Elite 2 pad", "Steam Deck")
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
    fcntl.ioctl(fd, EVIOCSCLOCKID, struct.pack("i", CLOCK_REALTIME))
    usb = open(mon, "rb", buffering=0)
    os.set_blocking(usb.fileno(), False)
    pending, deltas, prev, buf = [], [], None, b""
    print(f"press A about once a second for {secs} s ({name})")
    end = time.time() + secs
    while time.time() < end:
        r, _, _ = select.select([fd, usb], [], [], 0.2)
        if usb in r:
            buf += usb.read(65536) or b""
            *lines, buf = buf.split(b"\n")
            for l in lines:
                f = l.decode(errors="replace").split()
                # tag ts C Ii:1:dev:1 0:1 len = data...
                if len(f) > 7 and f[2] == "C" and f[3].startswith("Ii:1:") and f[3].endswith(":1") and "=" in f:
                    data = "".join(f[f.index("=") + 1:])
                    btn = data[4:8]  # report bytes 2-3: buttons
                    if prev is not None and btn != prev:
                        pending.append(int(f[1]))  # microseconds, realtime mod 4096 s
                    prev = btn
        if fd in r:
            for sec, usec, typ, code, val in read_events(fd, 0):
                if typ == EV_KEY and pending:
                    t_ev = (sec % 4096) * 1_000_000 + usec
                    d = (t_ev - pending.pop(0)) % (4096 * 1_000_000)
                    if d < 100_000:
                        deltas.append(d)
                    pending.clear()
    if not deltas:
        sys.exit("no pairs: was a face button pressed?")
    deltas.sort()
    n = len(deltas)
    print(f"{n} presses: USB completion to virtual pad EV_KEY  median {deltas[n // 2] / 1000:.2f} ms"
          f"  p90 {deltas[int(n * 0.9) - 1 if n > 1 else 0] / 1000:.2f} ms  max {deltas[-1] / 1000:.2f} ms")


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else ""
    secs = int(sys.argv[2]) if len(sys.argv) > 2 else 20
    {"buttons": lambda: cmd_buttons(secs), "pad": lambda: cmd_pad(secs), "touch": lambda: cmd_touch(secs),
     "imu": cmd_imu, "latency": lambda: cmd_latency(secs)}.get(mode, lambda: sys.exit(__doc__))()


if __name__ == "__main__":
    main()
