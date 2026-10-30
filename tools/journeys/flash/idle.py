#!/usr/bin/env python3
"""Idle guard for device tests: is anyone touching the SuiPlay?

  idle.py <seconds> [flagfile] [--beat FILE --parent PID]

Watches, for <seconds>, every readable evdev node that has keys (InputPlumber's pad and keyboard, the
power and steamos-manager keys) for key presses and stick/trigger moves past a deadzone, and the
interrupt counts of the touch screen (GDIX) and the AT keyboard (i8042). Keys injected through
gamescope's libei socket (eikey.py) never pass through evdev, and pad.py's "Rare Test Pad" is
skipped, so the test's own input does not count.
Prints one line and exits 0 when nothing moved, 1 at the first foreign input (and touches <flagfile>,
so a test running next to it can stop and restore at once).
--beat: writes "<pid> <CLOCK_MONOTONIC ns>" to FILE every 200 ms (beat.py supervise runs it this way;
the injectors send nothing while that heartbeat is 1 s old). --parent: exits as soon as PID (the
supervisor) is gone, so a guard nobody watches stops vouching for the test.
"""
import os, select, struct, sys, time

EV_KEY, EV_ABS = 1, 3
FMT = "llHHi"
SZ = struct.calcsize(FMT)


def irqs():
    out = {}
    try:
        lines = open("/proc/interrupts").read().splitlines()
    except OSError:
        lines = []
    for line in lines:
        parts = line.split()
        if not parts or not parts[0].endswith(":"):
            continue
        name = " ".join(parts[-2:])
        if "i8042" in name or "GDIX" in name or "goodix" in name.lower():
            out[name] = sum(int(p) for p in parts[1:] if p.isdigit())
    return out


def write_beat(p):
    tmp = f"{p}.{os.getpid()}"
    with open(tmp, "w") as f:
        f.write(f"{os.getpid()} {time.monotonic_ns()}\n")
    os.replace(tmp, p)


def main():
    args = sys.argv[1:]
    beat = parent = None
    if "--beat" in args:
        i = args.index("--beat"); beat = args[i + 1]; del args[i:i + 2]
    if "--parent" in args:
        i = args.index("--parent"); parent = int(args[i + 1]); del args[i:i + 2]
    secs = float(args[0]) if args else 60
    flag = args[1] if len(args) > 1 else None
    indir = os.environ.get("RARE_INPUT_DIR", "/dev/input")
    fds = {}
    for n in sorted(os.listdir(indir)):
        if n.startswith("event"):
            try:
                if open(f"/sys/class/input/{n}/device/name").read().strip() == "Rare Test Pad":
                    continue  # pad.py, the test's own controller
            except OSError:
                pass
            try:
                fds[os.open(f"{indir}/{n}", os.O_RDONLY | os.O_NONBLOCK)] = n
            except OSError:
                pass
    base = {}
    i0 = irqs()
    t_end = time.time() + secs
    hit = None
    while time.time() < t_end and not hit:
        if parent and os.getppid() != parent:
            print(f"{time.strftime('%H:%M:%S')} supervisor {parent} gone, guard stops", flush=True)
            sys.exit(2)
        if beat:
            write_beat(beat)
        try:
            r, _, _ = select.select(list(fds), [], [], 0.2)
        except OSError:
            # A node went away (a session restart re-creates virtual devices): drop the dead ones.
            for fd in list(fds):
                try:
                    os.fstat(fd)
                except OSError:
                    fds.pop(fd)
            continue
        for fd in r:
            try:
                data = os.read(fd, SZ * 64)
            except BlockingIOError:
                continue
            except OSError:
                fds.pop(fd, None)  # ENODEV: the device is gone
                continue
            for off in range(0, len(data) - SZ + 1, SZ):
                _, _, typ, code, val = struct.unpack_from(FMT, data, off)
                if typ == EV_KEY:
                    hit = f"{fds[fd]} key {code}={val}"
                elif typ == EV_ABS:
                    b = base.setdefault((fd, code), val)
                    if (code in (16, 17) and val != b) or (code in (2, 5) and abs(val - b) > 40) or abs(val - b) > 6000:
                        hit = f"{fds[fd]} abs {code} {b}->{val}"
        if not hit:
            i1 = irqs()
            moved = {k: i1[k] - i0.get(k, 0) for k in i1 if i1[k] != i0.get(k, 0)}
            if moved:
                hit = f"irq {moved}"
    if hit:
        print(f"{time.strftime('%H:%M:%S')} INPUT {hit}", flush=True)
        if flag:
            open(flag, "w").write(hit + "\n")
        sys.exit(1)
    print(f"{time.strftime('%H:%M:%S')} idle {secs:.0f} s ({', '.join(sorted(fds.values()))})", flush=True)


if __name__ == "__main__":
    main()
