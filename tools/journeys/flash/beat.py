#!/usr/bin/env python3
"""The idle guard as a hard dependency of the device tests' input.

  beat.py supervise [--secs S]   run idle.py as the guard (guard-run.sh does this). idle.py writes a
                                 heartbeat (GUARD.beat: "<pid> <CLOCK_MONOTONIC ns>") every 200 ms.
                                 When the guard exits for any reason (foreign input, its time is up,
                                 a crash, a kill), its heartbeat goes 1 s stale, or this supervisor is
                                 signalled, it calls lost(): stop every injector at once and, while a
                                 test is armed (ARMED), restore the stock session (restore.sh).
  beat.py check [--lost]         exit 0 while a live guard's heartbeat is under 1 s old, else print
                                 why and exit 1 (--lost: and call lost(), as an injector does)
  beat.py stop                   end the guard on purpose after a finished test (no restore)

eikey.py, pad.py and capture.sh send nothing unless check() passes, before every key or press, so
input stops within 1 s of the guard going away even if nothing else notices. Files live in $FLASHCAP
(default ~/flashcap): GUARD.beat, GUARD.pid (supervisor), INPUT (why the test stopped), ARMED (the
test changed the session), STOPGUARD, guard.log. RARE_RESTORE overrides restore.sh (selftest).
"""
import os, signal, subprocess, sys, time

HERE = os.path.dirname(os.path.realpath(__file__))
MAX_AGE_NS = 1_000_000_000
# What lost() stops: the scripts in this directory that send input or change the session.
INJECTORS = ("eikey.py", "pad.py", "capture.sh", "variant.sh", "mode.sh", "bench.py", "rr-run.sh")


def cap_dir():
    return os.environ.get("FLASHCAP") or os.path.expanduser("~/flashcap")


def path(name, d=None):
    return os.path.join(d or cap_dir(), name)


def say(msg, d=None):
    line = f"{time.strftime('%H:%M:%S')} {msg}\n"
    try:
        with open(path("guard.log", d), "a") as f:
            f.write(line)
    except OSError:
        sys.stderr.write(line)


def rm(p):
    """unlink; False when the file was not there (some sandboxes answer EPERM for a missing file)."""
    try:
        os.unlink(p)
        return True
    except FileNotFoundError:
        return False
    except PermissionError:
        if os.path.lexists(p):
            raise
        return False


def write_beat(p, pid=None):
    """Atomic heartbeat: '<pid> <monotonic ns>'."""
    tmp = f"{p}.{os.getpid()}"
    with open(tmp, "w") as f:
        f.write(f"{pid or os.getpid()} {time.monotonic_ns()}\n")
    os.replace(tmp, p)


def alive(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    try:  # a zombie is not a live guard
        return open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()[0] != "Z"
    except OSError:
        return True


def check(d=None):
    """None while a live guard's heartbeat is fresh, else why not."""
    try:
        pid, ns = open(path("GUARD.beat", d)).read().split()
        pid, ns = int(pid), int(ns)
    except FileNotFoundError:
        return "no idle guard heartbeat (start guard-run.sh first)"
    except (OSError, ValueError) as e:
        return f"unreadable guard heartbeat ({e})"
    if not alive(pid):
        return f"the idle guard (pid {pid}) is gone"
    age = time.monotonic_ns() - ns
    if age >= MAX_AGE_NS:
        return f"the idle guard's heartbeat is {age / 1e6:.0f} ms old"
    return None


def stop_injectors():
    """Kill this directory's injectors and session changers (never the caller), xdotool and the capture."""
    me = {os.getpid()}
    targets = {os.path.join(HERE, n) for n in INJECTORS}
    try:
        out = subprocess.run(["ps", "-eo", "pid=,args="], capture_output=True, text=True, timeout=5).stdout
    except Exception:
        out = ""
    killed = []
    for line in out.splitlines():
        parts = line.split()
        if len(parts) < 2 or not parts[0].isdigit():
            continue
        pid = int(parts[0])
        if pid in me:
            continue
        hit = any(os.path.realpath(a) in targets for a in parts[1:4] if "/" in a)
        hit = hit or os.path.basename(parts[1]) == "xdotool"
        hit = hit or ("gst-launch-1.0" in parts[1] and "target-object=gamescope" in line and "flashcap" in line)
        if hit:
            try:
                os.kill(pid, signal.SIGTERM)
                killed.append(pid)
            except OSError:
                pass
    if killed:
        time.sleep(0.3)
        for pid in killed:
            try:
                os.kill(pid, signal.SIGKILL)
            except OSError:
                pass
    return killed


def lost(why, d=None):
    """The guard is gone: no more input, and the stock session back if a test changed it."""
    d = d or cap_dir()
    try:
        fd = os.open(path("INPUT", d), os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
        os.write(fd, (why + "\n").encode())
        os.close(fd)
    except FileExistsError:
        pass  # the first reason stays (idle.py writes the foreign input it saw)
    killed = stop_injectors()
    say(f"guard lost: {why}; stopped {len(killed)} injector(s)", d)
    if not rm(path("ARMED", d)):  # whoever removes ARMED restores, once
        return
    say("restoring the stock session", d)
    restore = os.environ.get("RARE_RESTORE") or os.path.join(HERE, "restore.sh")
    with open(path("guard.log", d), "a") as log:
        subprocess.run([restore], stdout=log, stderr=log, timeout=120)


def supervise(secs):
    d = cap_dir()
    os.makedirs(d, exist_ok=True)
    beat = path("GUARD.beat", d)
    for n in ("GUARD.beat", "STOPGUARD", "INPUT"):  # a new guard starts clean
        rm(path(n, d))
    open(path("GUARD.pid", d), "w").write(f"{os.getpid()}\n")
    got = []
    for s in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        signal.signal(s, lambda n, _: got.append(n))
    log = open(path("guard.log", d), "a")
    p = subprocess.Popen([sys.executable, "-I", os.path.join(HERE, "idle.py"), str(secs), path("INPUT", d),
                          "--beat", beat, "--parent", str(os.getpid())], stdout=log, stderr=log)
    say(f"guard {p.pid} up for {secs:.0f} s", d)
    t0 = time.monotonic()
    why = None
    while why is None:
        time.sleep(0.05)
        if os.path.exists(path("STOPGUARD", d)):
            p.kill(); p.wait()
            for n in ("GUARD.beat", "STOPGUARD", "GUARD.pid"):
                rm(path(n, d))
            say("guard stopped on purpose", d)
            return 0
        rc = p.poll()
        if rc is not None:
            why = (f"foreign input: {open(path('INPUT', d)).read().strip()}" if rc == 1 and os.path.exists(path("INPUT", d))
                   else f"the idle guard exited (rc {rc})")
        elif got:
            p.kill(); p.wait()
            why = f"the guard supervisor got signal {got[0]}"
        elif time.monotonic() - t0 > 3:
            stale = check(d)
            if stale:
                p.kill(); p.wait()
                why = f"guard stalled: {stale}"
    rm(beat)  # injectors stop at their next key even before lost() kills them
    lost(why, d)
    return 1


def main():
    args = sys.argv[1:]
    cmd = args[0] if args else ""
    if cmd == "supervise":
        secs = float(args[args.index("--secs") + 1]) if "--secs" in args else 7200
        return supervise(secs)
    if cmd == "check":
        why = check()
        if why:
            print(why)
            if "--lost" in args:
                lost(why)
            return 1
        return 0
    if cmd == "stop":
        open(path("STOPGUARD"), "w").close()
        for _ in range(40):
            if not os.path.exists(path("GUARD.pid")):
                return 0
            time.sleep(0.05)
        return 1
    sys.exit(__doc__)


if __name__ == "__main__":
    sys.exit(main())
