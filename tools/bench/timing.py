import json, os, re, subprocess, sys, time

HOST = os.environ.get("BENCH_HOST", "suiplay")
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.environ.get("BENCH_OUT", os.path.join(HERE, "..", "..", "bench", "runs"))

def ssh(cmd, stdin=None, timeout=300):
    r = subprocess.run(["ssh", "-o", "ConnectTimeout=10", HOST, cmd], input=stdin, capture_output=True, text=True, timeout=timeout)
    return r.stdout

def secs(v, unit):
    return float(v) * {"ms": 0.001, "s": 1, "min": 60}[unit]

# ---- parsers (pure functions, tested by `selftest`) ----
def parse_analyze(text):
    out = {}
    for tot, name in re.findall(r"((?:[\d.]+(?:ms|min|s)\s*)+)\((firmware|loader|kernel|initrd|userspace)\)", text):
        out[name] = sum(secs(v, u) for v, u in re.findall(r"([\d.]+)(ms|min|s)", tot))
    return out

def parse_boot(analyze_text, mono_journal):
    """mono_journal: `journalctl -b -o short-monotonic` lines. Kernel monotonic zero = kernel start, i.e. after firmware+loader."""
    a = parse_analyze(analyze_text); m = {}; notes = []
    for k, v in a.items(): m[k + "_s"] = v
    pre = a.get("firmware", 0) + a.get("loader", 0)
    first = {}
    for line in mono_journal.splitlines():
        t = re.match(r"\[\s*([\d.]+)\]", line)
        if not t: continue
        ts = float(t.group(1))
        for key, pat in (("page", r"\[startup\] page is running: first config request at (\d+) ms"),
                         ("setup", r"\[startup\] setup (\d+) ms, at (\d+) ms since start"),
                         ("bg", r"\[startup\] background services started at (\d+) ms")):
            mm = re.search(pat, line)
            if mm and key not in first: first[key] = (ts, mm)
    if "page" in first:
        ts, mm = first["page"]
        m["launcher_start_s"] = pre + ts - int(mm.group(1)) / 1000     # process start, from power-on
        m["boot_to_first_frame_s"] = pre + ts
        m["launcher_page_ms"] = int(mm.group(1))
    else:
        notes.append("no '[startup]' lines in the journal: the running launcher predates commit 8fa77bc, or its stderr is not in the journal. Boot to first frame is n/a; only systemd-analyze figures are recorded.")
    if "setup" in first: m["launcher_setup_ms"] = int(first["setup"][1].group(1))
    if "bg" in first: m["launcher_bg_services_ms"] = int(first["bg"][1].group(1))
    if a:
        m["boot_to_userspace_s"] = sum(a.values())
    return m, notes

def parse_cycles(unix_journal):
    """short-unix lines. Returns suspend cycles: entry, exit, wake (end of the largest silent gap between), ready (userspace back)."""
    lines = []
    for l in unix_journal.splitlines():
        t = re.match(r"(\d{9,}\.\d+)\s+(.*)", l)
        if t: lines.append((float(t.group(1)), t.group(2)))
    cycles = []; i = 0
    while i < len(lines):
        ts, msg = lines[i]
        if "PM: suspend entry" in msg:
            ent = ts; j = i + 1; ex = None
            while j < len(lines):
                if "PM: suspend exit" in lines[j][1]: ex = lines[j][0]; break
                j += 1
            if ex is None: break
            gap, wake = 0, ent
            for k in range(i + 1, j + 1):
                g = lines[k][0] - lines[k - 1][0]
                if g > gap: gap, wake = g, lines[k][0]
            ready = None
            for k in range(j, min(j + 400, len(lines))):
                if re.search(r"System returned from sleep|Finished .*suspend|Stopped target .*[Ss]leep|Reached target .*[Ss]uspend", lines[k][1]):
                    ready = lines[k][0]; break
            cycles.append({"entry": ent, "exit": ex, "wake": wake, "ready": ready, "gap_s": gap})
            i = j
        i += 1
    return cycles

# ---- device probes ----
def now_json(name):
    return json.loads(ssh("cat /tmp/rare-bench/%s.json" % name) or "{}")

def bat():
    o = ssh("cd /sys/class/power_supply/BAT0 && echo $(cat capacity) $(cat energy_now) $(cat status) $(cat energy_full)").split()
    return {"t": float(ssh("date +%s.%N")), "cap": float(o[0]), "e_uwh": float(o[1]), "status": o[2], "full_uwh": float(o[3])}

LAUNCH_REMOTE = r'''
import os, subprocess, sys, threading, time, glob
watch = float(sys.argv[1]); t0 = time.time(); ev = []; spyp = None
def log(k, v): ev.append((time.time(), k, v)); print("%.3f %s %s" % (time.time(), k, v), flush=True)
def spy():
    global spyp
    p = spyp = subprocess.Popen(["xprop", "-root", "-spy", "GAMESCOPE_FOCUSED_APP"], env=dict(os.environ, DISPLAY=":0"), stdout=subprocess.PIPE, text=True)
    for l in p.stdout:
        log("focus", l.strip().split("=")[-1].strip())
threading.Thread(target=spy, daemon=True).start()
def procs():
    d = {}
    for p in os.listdir("/proc"):
        if p.isdigit():
            try:
                st = open("/proc/%s/stat" % p).read(); i = st.rfind(")"); f = st[i+2:].split()
                d[int(p)] = (int(f[1]), st[st.find("(")+1:i])
            except Exception: pass
    return d
base = procs(); seen = set(base)
while time.time() - t0 < watch:
    cur = procs()
    for pid, (pp, comm) in cur.items():
        if pid in seen: continue
        seen.add(pid)
        chain, x = [], pid
        for _ in range(6):
            x = cur.get(x, (0, ""))[0]; chain.append(cur.get(x, (0, ""))[1])
        try: cmd = open("/proc/%d/cmdline" % pid).read().replace("\0", " ")[:80]
        except Exception: cmd = ""
        if "rare-launcher" in chain[:2] or "steam" in cmd.lower() or "rungameid" in cmd:
            log("spawn", "%s %s" % (comm, cmd.replace(" ", "_")))
    time.sleep(0.05)
if spyp: spyp.kill()
os._exit(0)
'''

def parse_launch(text):
    ev = []
    for l in text.splitlines():
        p = l.split(" ", 2)
        if len(p) == 3:
            try: ev.append((float(p[0]), p[1], p[2]))
            except ValueError: pass
    return ev

def launch_metrics(ev):
    notes = []; m = {}
    foc = [e for e in ev if e[1] == "focus"]
    if not foc: return m, ["no focus data (xprop on :0 failed?)"]
    base = foc[0][2]; m["baseline_focus_appid"] = base
    spawns = [e for e in ev if e[1] == "spawn"]
    if not spawns: return m, ["nothing was launched during the watch window (dry run), so no Play-to-focus time"]
    press = spawns[0][0]
    after = [e for e in foc[1:] if e[0] >= press - 0.2 and e[2] != base]
    if not after: return m, ["a process started but focus never left the launcher (%s)" % base]
    m["play_to_first_focus_change_s"] = after[0][0] - press
    m["play_to_game_focus_s"] = after[-1][0] - press
    m["focus_changes"] = len(after); m["final_focus_appid"] = after[-1][2]
    if len(after) > 1: notes.append("focus changed %d times (Steam UI before the game is normal); game_focus is the last change" % len(after))
    return m, notes

def save(build, kind, scen, metrics, notes, extra=None):
    d = os.path.join(OUT, re.sub(r"[^A-Za-z0-9._-]", "_", build), "timing", scen, time.strftime("%Y%m%dT%H%M%S"))
    os.makedirs(d, exist_ok=True)
    s = {"kind": "timing", "build": build, "scenario": scen, "note": (extra or {}).get("note", ""), "start": time.time(), "metrics": metrics, "notes": notes}
    json.dump(s, open(d + "/summary.json", "w"), indent=1)
    print("timing %s  build=%s" % (scen, build))
    for k, v in metrics.items(): print("  %-30s %s" % (k, ("%.3f" % v) if isinstance(v, float) else v))
    for n in notes: print("  NOTE: " + n)
    print("saved: " + d)

def flag(args, name, default):
    return args[args.index(name) + 1] if name in args else default

def main(a):
    if not a: print(open(os.path.join(HERE, "timing")).read().split("exec")[0]); return 2
    c = a[0]
    if c == "selftest": return selftest()
    if c == "boot":
        b = flag(a, "--boot", "0"); build = a[1]
        an = ssh("systemd-analyze 2>&1")
        j = ssh("journalctl -b %s -o short-monotonic --no-pager 2>/dev/null | grep -E '\\[startup\\]'" % b)
        m, n = parse_boot(an, j); save(build, "timing", "boot", m, n)
    elif c == "resume":
        b = flag(a, "--boot", "0"); build = a[1]; last = int(flag(a, "--last", "1"))
        j = ssh("journalctl -b %s -o short-unix --no-pager 2>/dev/null | grep -E 'PM: suspend (entry|exit)|System returned from sleep|suspend\\.service|[Ss]leep|suspend\\.target' " % b + "; true")
        # the gap search needs every line between entry and exit, so refetch around each cycle
        cyc0 = parse_cycles(j)
        cyc = []
        for c0 in cyc0[-last:]:
            full = ssh("journalctl -b %s -o short-unix --no-pager --since @%d --until @%d 2>/dev/null" % (b, c0["entry"] - 1, (c0["ready"] or c0["exit"]) + 5))
            cyc += parse_cycles(full)
        if not cyc:
            save(build, "timing", "resume", {}, ["no suspend/resume cycle in boot %s journal. Suspend the device once (power button), wake it, then rerun." % b]); return 0
        for cy in cyc:
            m = {"slept_s": cy["exit"] - cy["entry"], "kernel_resume_s": cy["exit"] - cy["wake"]}
            if cy["ready"]: m["wake_to_userspace_ready_s"] = cy["ready"] - cy["wake"]
            save(build, "timing", "resume", m, ["wake = end of the longest silent gap in the journal; display-on is not in the journal, measure it with the stopwatch/phone video protocol if needed"])
    elif c == "launch":
        build, label = a[1], a[2]; w = float(flag(a, "--watch", "120"))
        print("watching %.0f s: press Play on the device now (focus is read from GAMESCOPE_FOCUSED_APP)" % w, flush=True)
        out = subprocess.run(["ssh", "-o", "ConnectTimeout=10", HOST, "python3 -c \"$(cat)\" %s" % w],
                             input=LAUNCH_REMOTE, capture_output=True, text=True, timeout=w + 60).stdout
        ev = parse_launch(out); m, n = launch_metrics(ev)
        m["events"] = len(ev)
        save(build, "timing", "launch/" + re.sub(r"[^A-Za-z0-9._-]", "_", label), m, n)
    elif c == "sleep":
        if a[1] == "start":
            s = bat(); ssh("mkdir -p /tmp/rare-bench && cat > /tmp/rare-bench/sleep.json", stdin=json.dumps(s))
            print("recorded: %s" % s)
            if s["status"] != "Discharging": print("WARNING: battery is %s, not Discharging; unplug the charger first" % s["status"])
            print("now suspend the device, wait at least 30 min, wake it, then run: timing sleep finish <build>")
        else:
            build = a[2]; s0 = now_json("sleep"); s1 = bat()
            cyc = parse_cycles(ssh("journalctl -o short-unix --no-pager --since @%d 2>/dev/null" % int(s0["t"]))) if s0 else []
            asleep = sum(c["exit"] - c["entry"] for c in cyc if c["entry"] >= s0["t"]) if s0 else 0
            notes = []
            if not s0: save(build, "timing", "sleep", {}, ["run `timing sleep start` first"]); return 1
            if not asleep:
                asleep = s1["t"] - s0["t"]; notes.append("no suspend cycle found in the journal: used wall time since start (includes any awake time)")
            h = asleep / 3600
            m = {"asleep_h": h, "cap_start": s0["cap"], "cap_end": s1["cap"], "cycles": len(cyc)}
            m["drain_pct_per_h"] = (s0["cap"] - s1["cap"]) / h if h else 0
            m["drain_mw"] = (s0["e_uwh"] - s1["e_uwh"]) / 1000 / h if h else 0
            if "Charging" in (s0["status"], s1["status"]) or s1["status"] == "Full": notes.append("charger was connected: result invalid, drain not recorded"); m.pop("drain_pct_per_h"); m.pop("drain_mw")
            if h < 0.5: notes.append("under 30 min asleep: 1 % capacity steps make this number coarse; energy_now based mW is more precise")
            save(build, "timing", "sleep", m, notes, {"note": flag(a, "--note", "")})
    else:
        print("unknown command"); return 2
    return 0

def selftest():
    an = "Startup finished in 5.565s (firmware) + 4.486s (loader) + 853ms (kernel) + 3.482s (initrd) + 52.014s (userspace) = 1min 6.401s"
    p = parse_analyze(an); assert abs(p["firmware"] - 5.565) < 1e-6 and abs(p["kernel"] - 0.853) < 1e-6 and abs(p["userspace"] - 52.014) < 1e-6, p
    p2 = parse_analyze("Startup finished in 1min 2.5s (userspace)"); assert abs(p2["userspace"] - 62.5) < 1e-6, p2
    j = "[   40.000000] bazzite gamescope-session-plus[3189]: [startup] page is running: first config request at 900 ms\n[   41.0] x y: [startup] setup 210 ms, at 1000 ms since start\n[   42.0] x y: [startup] background services started at 3400 ms (5 ms)\n"
    m, n = parse_boot(an, j); assert abs(m["boot_to_first_frame_s"] - (5.565 + 4.486 + 40.0)) < 1e-6 and m["launcher_setup_ms"] == 210, m
    m, n = parse_boot(an, ""); assert "boot_to_first_frame_s" not in m and n
    fx = open(os.path.join(HERE, "test", "suspend-fixture.txt")).read()
    cy = parse_cycles(fx); assert len(cy) == 1, cy
    c = cy[0]; assert abs(c["exit"] - c["wake"] - 0.9) < 1e-3 and abs(c["ready"] - c["wake"] - 1.5) < 1e-3 and abs(c["gap_s"] - 3590) < 1, c
    ev = parse_launch("100.0 focus 769\n105.0 spawn steam_-silent\n111.5 focus 5555\n118.0 focus 1234567\n")
    m, n = launch_metrics(ev); assert abs(m["play_to_first_focus_change_s"] - 6.5) < 1e-6 and abs(m["play_to_game_focus_s"] - 13.0) < 1e-6, m
    print("selftest ok"); return 0

if __name__ == "__main__": sys.exit(main(sys.argv[1:]))
