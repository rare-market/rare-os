#!/usr/bin/env python3
"""Drives a booted VM like a player with a pad (vpad.py) and times what the screen does.

  walk.py step NAME STEP...   send the pad steps, then watch the screen: response (first change),
                              settled (no change for --quiet s), the longest still gap while it was
                              changing; saves OUT/NAME.png and appends a line to OUT/timing.tsv
  walk.py shot NAME           one screenshot, OUT/NAME.png
  walk.py until NAME SECS     screenshots until the screen has been still for --quiet s (boot, install)

Env: RARE_VM_STATE (QMP socket, as harness.py), WALK_PAD (vpad socket), WALK_OUT (shots, timing).
A step that answers in more than 2 s, or not at all, is marked SLOW / NONE in timing.tsv.
"""
import hashlib, os, subprocess, sys, time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import harness  # noqa: E402

OUT = os.environ.get("WALK_OUT", "/var/tmp/rare-walk/out")
PAD = os.environ.get("WALK_PAD", "/var/tmp/rare-walk/pad.sock")
TMP = os.path.join(harness.STATE, "walk-frame.ppm")


def frame(q):
    """A hash of the screen without its bottom 8 % (the launcher's status cells tick on their own)."""
    q.cmd("screendump", filename=TMP)
    with open(TMP, "rb") as f:
        data = f.read()
    try:
        parts = data.split(b"\n", 3)  # P6, "w h", maxval, pixels
        w, h = (int(x) for x in parts[1].split())
        px = parts[3]
        data = px[: w * 3 * int(h * 0.92)]
    except Exception:  # noqa: BLE001
        pass
    return hashlib.blake2b(data, digest_size=16).digest()


def png(q, name):
    path = os.path.join(OUT, name + ".png")
    try:
        q.cmd("screendump", filename=path, format="png")
    except RuntimeError:
        q.cmd("screendump", filename=TMP)
        subprocess.run(["convert", TMP, path], check=False)
    return path


def watch(q, t0, quiet, limit):
    """(response s or None, settled s or None, longest still gap while moving)"""
    last = frame(q)
    first = None
    changed_at = t0
    gap = 0.0
    while True:
        time.sleep(0.05)
        now = time.time()
        f = frame(q)
        if f != last:
            if first is None:
                first = now - t0
            else:
                gap = max(gap, now - changed_at)
            changed_at = now
            last = f
        elif now - changed_at >= quiet and (first is not None or now - t0 >= quiet + 1.0):
            return first, (changed_at - t0) if first is not None else None, gap
        if now - t0 > limit:
            return first, None, gap


def send(steps):
    subprocess.run([sys.executable, os.path.join(os.path.dirname(os.path.abspath(__file__)), "vpad.py"), "send", PAD, *steps], check=True)


def main():
    os.makedirs(OUT, exist_ok=True)
    args = sys.argv[1:]
    quiet = 1.0
    if "--quiet" in args:
        k = args.index("--quiet")
        quiet = float(args[k + 1])
        del args[k:k + 2]
    if not args:
        print(__doc__)
        sys.exit(2)
    q = harness.qmp()
    act, name = args[0], args[1]
    if act == "shot":
        print(png(q, name))
        return
    if act == "until":
        limit = float(args[2])
        t0 = time.time()
        resp, settled, gap = watch(q, t0, quiet, limit)
        png(q, name)
        print(f"{name}: settled {settled if settled is None else round(settled, 2)} s")
        return
    steps = args[2:]
    t0 = time.time()
    send(steps)
    t1 = time.time()
    resp, settled, gap = watch(q, t1, quiet, float(os.environ.get("WALK_LIMIT", "20")))
    png(q, name)
    flag = "NONE" if resp is None else ("SLOW" if resp > 2.0 else ("STALL" if gap > 2.0 else ""))
    line = "\t".join([time.strftime("%H:%M:%S"), name, " ".join(steps), f"{t1 - t0:.2f}", "-" if resp is None else f"{resp:.2f}", "-" if settled is None else f"{settled:.2f}", f"{gap:.2f}", flag])
    with open(os.path.join(OUT, "timing.tsv"), "a") as f:
        f.write(line + "\n")
    print(line)


if __name__ == "__main__":
    main()
