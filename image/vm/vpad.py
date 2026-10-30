#!/usr/bin/env python3
"""A virtual Xbox 360 pad on the VM host, handed to the guest whole with
`-device virtio-input-host-pci,evdev=<path>` (QEMU grabs it, so the host never sees its presses).
The guest gets the pad's name and USB ids (045e:028e), so udev, InputPlumber and the launcher
(gilrs) treat it like the real thing: no SSH, no guest software, the install and first boot included.

  vpad.py serve SOCK          create the pad, print its /dev/input/eventN, take commands on SOCK
  vpad.py send SOCK STEP...   send steps: a b x y lb rb lt rt select start guide l3 r3
                              up down left right (d-pad), ls-up ... (left stick), wait:S,
                              hold:BTN:S (held S seconds), chord:BTN+BTN:S, quit

Needs write access to /dev/uinput (run as root, like the rest of image/vm).
"""
import fcntl, os, socket, struct, sys, time

EV_SYN, EV_KEY, EV_ABS = 0, 1, 3
UI_SET_EVBIT, UI_SET_KEYBIT, UI_SET_ABSBIT = 0x40045564, 0x40045565, 0x40045567
UI_DEV_CREATE, UI_DEV_DESTROY = 0x5501, 0x5502
UI_GET_SYSNAME = 0x8040552C  # _IOC(_IOC_READ, 'U', 44, 64)
BTN = {"a": 304, "b": 305, "x": 307, "y": 308, "lb": 310, "rb": 311, "select": 314, "start": 315, "guide": 316, "l3": 317, "r3": 318}
HAT = {"left": (16, -1), "right": (16, 1), "up": (17, -1), "down": (17, 1)}
STICK = {"ls-left": (0, -32767), "ls-right": (0, 32767), "ls-up": (1, -32767), "ls-down": (1, 32767)}
TRIG = {"lt": 2, "rt": 5}
NAME = b"Microsoft X-Box 360 pad"


def create():
    fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
    for ev in (EV_KEY, EV_ABS, EV_SYN):
        fcntl.ioctl(fd, UI_SET_EVBIT, ev)
    for code in BTN.values():
        fcntl.ioctl(fd, UI_SET_KEYBIT, code)
    axes = [0, 1, 2, 3, 4, 5, 16, 17]
    for a in axes:
        fcntl.ioctl(fd, UI_SET_ABSBIT, a)
    absmax, absmin, fuzz, flat = [0] * 64, [0] * 64, [0] * 64, [0] * 64
    for a in (0, 1, 3, 4):
        absmin[a], absmax[a], fuzz[a], flat[a] = -32768, 32767, 16, 128
    for a in (2, 5):
        absmin[a], absmax[a] = 0, 255
    for a in (16, 17):
        absmin[a], absmax[a] = -1, 1
    # BUS_USB, Microsoft, Xbox 360 pad, version 0x114 (what xpad reports)
    dev = struct.pack("80sHHHHi", NAME, 0x03, 0x045E, 0x028E, 0x0114, 0)
    dev += b"".join(struct.pack("64i", *v) for v in (absmax, absmin, fuzz, flat))
    os.write(fd, dev)
    fcntl.ioctl(fd, UI_DEV_CREATE)
    buf = bytearray(64)
    fcntl.ioctl(fd, UI_GET_SYSNAME, buf)
    sysname = buf.split(b"\0")[0].decode()
    time.sleep(0.5)
    ev = next(n for n in os.listdir(f"/sys/devices/virtual/input/{sysname}") if n.startswith("event"))
    return fd, f"/dev/input/{ev}"


def emit(fd, typ, code, val):
    t = time.time()
    os.write(fd, struct.pack("llHHi", int(t), int((t % 1) * 1e6), typ, code, val))


def sync(fd):
    emit(fd, EV_SYN, 0, 0)


def set_btn(fd, name, down):
    if name in BTN:
        emit(fd, EV_KEY, BTN[name], 1 if down else 0)
    elif name in HAT:
        emit(fd, EV_ABS, HAT[name][0], HAT[name][1] if down else 0)
    elif name in STICK:
        emit(fd, EV_ABS, STICK[name][0], STICK[name][1] if down else 0)
    elif name in TRIG:
        emit(fd, EV_ABS, TRIG[name], 255 if down else 0)
    else:
        raise ValueError(f"unknown button {name}")


def press(fd, names, hold):
    for n in names:
        set_btn(fd, n, True)
    sync(fd)
    time.sleep(hold)
    for n in names:
        set_btn(fd, n, False)
    sync(fd)


def neutral(fd):
    for n in list(BTN) + list(HAT) + list(STICK) + list(TRIG):
        set_btn(fd, n, False)
    sync(fd)


def run_step(fd, s):
    if s.startswith("wait:"):
        time.sleep(float(s[5:]))
    elif s.startswith("hold:"):
        _, b, secs = s.split(":")
        press(fd, [b], float(secs))
    elif s.startswith("chord:"):
        _, bs, secs = s.split(":")
        press(fd, bs.split("+"), float(secs))
    else:
        press(fd, [s], 0.08)
        time.sleep(0.05)


def serve(path):
    fd, ev = create()
    print(ev, flush=True)
    if os.path.exists(path):
        os.remove(path)
    srv = socket.socket(socket.AF_UNIX)
    srv.bind(path)
    os.chmod(path, 0o660)
    srv.listen(4)
    try:
        while True:
            c, _ = srv.accept()
            with c:
                data = c.makefile().read().split()
                if "quit" in data:
                    return
                try:
                    for s in data:
                        run_step(fd, s)
                    c.sendall(b"ok\n")
                except Exception as e:  # noqa: BLE001
                    neutral(fd)
                    c.sendall(f"error {e}\n".encode())
    finally:
        neutral(fd)
        fcntl.ioctl(fd, UI_DEV_DESTROY)
        os.close(fd)
        os.path.exists(path) and os.remove(path)


def send(path, steps):
    c = socket.socket(socket.AF_UNIX)
    c.connect(path)
    c.sendall(" ".join(steps).encode())
    c.shutdown(socket.SHUT_WR)
    out = c.makefile().read().strip()
    if out and out != "ok":
        print(out, file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    if len(sys.argv) >= 3 and sys.argv[1] == "serve":
        serve(sys.argv[2])
    elif len(sys.argv) >= 3 and sys.argv[1] == "send":
        send(sys.argv[2], sys.argv[3:])
    else:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
