#!/usr/bin/env python3
"""Dead-control crawler: presses every focusable control on every screen of the native launcher.

Runs the real launcher headless (RR_SHOT + RR_DRIVE: steps from stdin, `state` back on stdout) with
the device's real library and config copied read only (ux.sh pull-device) and the fixture backend
for the rest, inside a sandbox: no network (user namespace), no privilege gain (no_new_privs, so no
sudo or pkexec), and every system command the launcher may run (systemctl, nmcli, steam, xdg-open...)
replaced by a stub that only logs it. Nothing on the build machine or the device changes.

Per screen (a seed below, or one a press opened): walk the focus graph with the d-pad, then per
control start fresh, walk to it, press A and record what happened: navigated, opened a sheet or
keyboard, toast, act, backend job, process, visible change, or nothing (DEAD). A press that opened
something stacked is followed by B: if B does nothing it is a dead end. Panics and ERROR lines in
the launcher's log are reported too.

  crawl.py run [--bin DIR] [--device DIR] [--out DIR] [--jobs N] [--res WxH] [--only ws,ws] [--depth N]
  crawl.py report --out DIR [--md FILE] [--shots DIR]     dead-controls.md from a run

Output: <out>/results.json, <out>/shots/*.png, <out>/dead-controls.md.
"""
from __future__ import annotations

import argparse
import collections
import concurrent.futures as cf
import datetime as dt
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_BIN = os.path.expanduser("~/rare-dev/uxj/bin")
DEFAULT_DEVICE = os.path.expanduser("~/rare-dev/uxj/device")
DEFAULT_OUT = os.path.expanduser("~/rare-dev/crawl/runs")
# Moves while walking a screen: the d-pad, then the shoulder buttons (tabs and sections inside a
# screen: the Rare menu's tabs, Market's sections, Cloud saves' tabs). A move that leaves the screen
# (LB/RB on a top tab) is undone.
DIRS = ["down", "right", "up", "left", "rt", "lt", "rb", "lb"]
OPP = {"down": "up", "up": "down", "left": "right", "right": "left", "lb": "rb", "rb": "lb", "lt": "rt", "rt": "lt"}
STUBS = ["xprop", "flatpak", "nmcli", "systemd-creds", "systemctl", "kill", "gdbus", "systemd-run", "sh", "bash", "script", "pgrep",
         "curl", "xdg-open", "vainfo", "tpm2_flushcontext", "tpm2_createek", "timeout", "tar", "pw-record", "pkill", "pkexec", "pactl",
         "mangoapp", "steam", "sudo", "reboot", "poweroff", "shutdown", "loginctl", "busctl", "bluetoothctl", "rfkill", "brightnessctl",
         "wpctl", "ydotool", "inputplumber", "gamescopectl", "dbus-send", "notify-send", "rare-session-client", "rare-session-select",
         "steamos-session-select", "flatpak-spawn", "gio", "wget", "ssh", "scp", "rsync", "nmtui", "iwctl", "upower", "powerprofilesctl",
         "tuned-adm", "fwupdmgr", "rpm-ostree", "ujust", "bootc", "journalctl", "coredumpctl", "dmesg", "lsblk", "udisksctl", "mount", "umount"]
PASS = ["fc-match", "fc-list"]
HANDOFF = re.compile(r"ingame:|SteamBigPicture|Launch\(|OpenApp\(|Legacy\(\"rare:steam|PcSheet")
OSK_NOOP = re.compile(r"^(Paste|X Delete|Delete|Backspace)$")
FIXTURE_TOAST = re.compile(r"fixture|Runs on the device", re.I)
SYSTEM = re.compile(r"^(Sleep|Restart|Turn off|Power off|Shut down|Reboot|Restart Steam|Log out|Sign out)\b", re.I)

# Seeds: (name, workstream, env, start steps, probe command). The game ids come from the device's
# library (and the fixture library for sources the device has none of).
GAME_SRC_WS = {}


def seeds(device):
    s = [
        ("Home", "B", {"RR_TAB": "home"}, [], None),
        ("Library", "B", {"RR_TAB": "library"}, [], None),
        ("Sui Games", "A", {"RR_TAB": "games"}, [], None),
        ("Market", "A", {"RR_TAB": "market"}, [], None),
        ("Wallet", "C", {"RR_TAB": "wallet"}, [], None),
        ("Friends", "C", {"RR_TAB": "friends"}, [], None),
        ("Settings", "D", {"RR_TAB": "settings"}, [], None),
        ("Search", "B", {"RR_OPEN": "search"}, [], None),
        ("Quick settings", "D", {"RR_OPEN": "qam"}, [], None),
        ("Rare menu over Rare", "D", {"RR_TAB": "home", "RR_MENU_ALLTABS": "1"}, ["guide"], None),
        ("Profile", "C", {"RR_OPEN": "profile"}, [], None),
        ("Downloads", "B", {"RR_OPEN": "downloads"}, [], None),
        ("First run", "D", {"RR_OPEN": "firstrun"}, [], None),
        ("Sign in", "C", {"RR_OPEN": "signin"}, [], None),
        ("Trophies", "C", {"RR_OPEN": "trophies"}, [], None),
        ("Cloud saves", "B", {"RR_OPEN": "saves"}, [], None),
        ("Wishlist", "B", {"RR_OPEN": "wishlist"}, [], None),
        ("Your stats", "B", {"RR_OPEN": "stats"}, [], None),
        ("Fan curve", "D", {"RR_OPEN": "fan-curve"}, [], None),
        ("Emulator setup", "B", {"RR_OPEN": "emusetup"}, [], None),
        ("Sui game page AresRPG", "A", {"RR_OPEN": "sui:aresrpg"}, [], None),
        ("Sui game page ACT", "A", {"RR_OPEN": "sui:act"}, [], None),
        ("Market collection Popkins", "A", {"RR_OPEN": "market:collection:popkins"}, [], None),
    ]
    for f in ("steam", "cloud", "emulation", "apps", "web", "installed"):
        s.append((f"Library: {f}", "B", {"RR_TAB": "library"}, [], f"tab:library:{f}"))
    game = "steam:1145350"
    for mode, label in (("menu", "Rare menu over a game"), ("quick", "Quick settings over a game"), ("switcher", "Switcher over a game"), ("power", "Power over a game")):
        s.append((label, "D", {"RR_RUNNING": game, "RR_OPEN": f"ingame:{game}|{mode}|fixture", "RR_MENU_ALLTABS": "1"}, [], None))
    # One game page per source.
    try:
        lib = json.load(open(os.path.join(device, ".cache/rare/library.json")))["library"]["games"]
    except Exception:
        lib = []
    seen = set()
    for g in lib:
        src = g.get("source") or g["id"].split(":")[0]
        kind = gkind(g["id"])
        if kind in seen:
            continue
        seen.add(kind)
        s.append((f"Game page {kind} ({g.get('title', '')[:30]})", "B", {"RR_OPEN": "game:" + g["id"]}, [], None))
    for gid in ("epic:1", "gog:2", "local:a"):
        s.append((f"Game page {gid.split(':')[0]} (fixture library)", "B", {"RR_OPEN": "game:" + gid, "_nolib": "1"}, [], None))
    return s


def gkind(gid):
    """A game page's type: its source, plus the platform for cloud and emulated games."""
    p = gid.split(":")
    if p[0] == "cloud" and len(p) > 1:
        return p[0] + ":" + p[1].split("/")[0]
    return p[0]


def ws_of(sig, fallback):
    t = sig.lower()
    rules = [
        ("D", r"firstrun|raremenu|qam|quick|power|running|settings|ingame"),
        ("C", r"wallet|friend|profile|share|troph|devicepass|perk|notif|message|sign"),
        ("A", r"tab=games|tab=market|trade|item|collection|sui"),
        ("B", r"home|library|game|store|apps|download|storage|search|launchfail|options|uninstall"),
    ]
    for ws, rx in rules:
        if re.search(rx, t):
            return ws
    return fallback


class Dead(Exception):
    pass


class Launcher:
    """One launcher process in drive mode."""

    def __init__(self, a, env_extra, workdir):
        self.a = a
        self.dir = workdir
        shutil.rmtree(workdir, ignore_errors=True)
        os.makedirs(os.path.join(workdir, "run"))
        home = os.path.join(workdir, "home")
        os.makedirs(home)
        open(os.path.join(workdir, "run", "rare-journey"), "w").close()
        for rel in (".config/rare", ".local/state/rare-native", ".cache/rare"):
            src = os.path.join(a.device, rel)
            if os.path.isdir(src):
                shutil.copytree(src, os.path.join(home, rel), dirs_exist_ok=True)
        for root, ds, fs in os.walk(home):
            for x in ds + fs:
                os.chmod(os.path.join(root, x), 0o755 if x in ds else 0o644)
        self.execlog = os.path.join(workdir, "exec.log")
        open(self.execlog, "w").close()
        env = {
            "PATH": a.stubs + ":/usr/bin:/bin",
            "HOME": home,
            "XDG_RUNTIME_DIR": os.path.join(workdir, "run"),
            "TZ": "America/New_York",
            "LANG": "C.UTF-8",
            "RR_SILENT": "1",
            "RR_NO_IP": "1",
            "RR_FIXTURE": "1",
            "RR_WINDOWED": a.res,
            "RR_CLOCK": "9:41 AM",
            "RR_SHOT": os.path.join(workdir, "end.png"),
            "RR_DRIVE": "1",
            "RR_SETTLE_MS": "150",
            "RR_CRAWL_EXEC": self.execlog,
            "RUST_BACKTRACE": "1",
        }
        lib = os.path.join(a.device, ".cache/rare/library.json")
        if os.path.exists(lib) and not env_extra.get("_nolib"):
            env["RR_LIBRARY"] = lib
        for k, v in env_extra.items():
            if not k.startswith("_"):
                env[k] = str(v)
        self.err = open(os.path.join(workdir, "err.log"), "w")
        cmd = [os.path.join(a.bin, "rare-launcher-native")]
        if a.sandbox:
            cmd = ["unshare", "-Un", "setpriv", "--no-new-privs"] + cmd
        self.p = subprocess.Popen(cmd, cwd=a.bin, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.err, text=True, bufsize=1)
        self.presses = 0

    def _cmd(self, line, want):
        try:
            self.p.stdin.write(line + "\n")
            self.p.stdin.flush()
        except (BrokenPipeError, OSError):
            raise Dead("launcher exited")
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            out = self.p.stdout.readline()
            if not out:
                raise Dead(f"launcher exited (code {self.p.wait()})")
            if out.startswith(want):
                return out[len(want):].strip()
            if out.startswith("@@bad"):
                raise RuntimeError(out)
        raise Dead("no answer in 60 s")

    def step(self, steps):
        """Plays steps (a list or 'a,wait:500') and returns the state once settled."""
        if isinstance(steps, list):
            steps = ",".join(steps)
        if steps:
            self.p.stdin.write(steps + "\n")
            self.presses += steps.count(",") + 1
        return self.state()

    def state(self):
        s = json.loads(self._cmd("state", "@@state "))
        s["execs"] = sum(1 for _ in open(self.execlog))
        return s

    def shot(self, path):
        self._cmd("shot " + path, "@@shot")

    def close(self):
        try:
            self.p.stdin.write("quit\n")
            self.p.stdin.flush()
        except Exception:
            pass
        try:
            self.p.wait(5)
        except subprocess.TimeoutExpired:
            self.p.kill()
            self.p.wait()
        self.err.close()

    def log_problems(self):
        out = []
        try:
            for line in open(os.path.join(self.dir, "err.log"), errors="replace"):
                if "panicked" in line or " ERROR " in line or "RUST_BACKTRACE" in line:
                    out.append(line.strip()[:300])
        except FileNotFoundError:
            pass
        return out

    def execs(self):
        try:
            return [l.strip() for l in open(self.execlog)]
        except FileNotFoundError:
            return []


def sig_of(s):
    a = s["app"]
    ov = a.get("overlay") or "-"
    oid = a.get("overlay_id") or ""
    if ov == "game" and oid:
        oid = gkind(oid)
    else:
        oid = ""
    # An overlay is the same screen over any tab.
    bits = [f"tab={a['tab']}"] if ov == "-" else []
    bits.append(f"overlay={ov}{(':' + oid) if oid else ''}")
    for k in ("legacy", "options", "share"):
        if a.get(k):
            bits.append(k)
    if a.get("osk"):
        # One keyboard: the same component wherever it opens, crawled once.
        return "osk"
    return " ".join(bits)


def fkey(s):
    f = s["app"]["focus"]
    r = f.get("rect") or [0, 0, 0, 0]
    # Digits folded: a slider's or a choice's value is part of its text and moves with left/right.
    text = re.sub(r"\d+([.,]\d+)?", "#", (f.get("text") or "")[:60])
    return f"{f.get('group') or ''}|{text}|{int(r[0] // 4)}|{int(r[2] // 4)}"


def label_of(s):
    f = s["app"]["focus"]
    t = (f.get("text") or "").strip()
    return t[:70] or "(no text)"


def stacked(s):
    a = s["app"]
    return bool(a.get("overlay") or a.get("legacy") or a.get("options") or a.get("osk") or a.get("share"))


class Crawl:
    def __init__(self, a):
        self.a = a
        self.lock = threading.Lock()
        self.screens = {}  # sig key -> screen dict
        self.results = []
        self.n = 0

    def wd(self, tag):
        with self.lock:
            self.n += 1
            n = self.n
        return os.path.join(self.a.work, f"{n:05d}-{tag}")

    def start(self, scr, tag):
        # The headless GPU can fail to come up when the build machine is busy: try again, a little later.
        for i in range(4):
            L = Launcher(self.a, scr["env"], self.wd(tag))
            try:
                st = L.step([])
                break
            except Dead:
                L.close()
                if i == 3 or not any("headless gpu" in x for x in open(os.path.join(L.dir, "err.log"), errors="replace")):
                    raise
                time.sleep(2 + 3 * i)
        if scr.get("cmd"):
            open(os.path.join(L.dir, "run", "rare-journey.cmd"), "w").write(scr["cmd"] + "\n")
            st = L.step(["wait:900"])
        if scr["prefix"]:
            st = L.step(scr["prefix"])
        return L, st

    def safe_explore(self, scr):
        try:
            self.explore(scr)
        except Exception as e:  # one screen never stops the crawl
            scr["crash"] = f"crawler error: {e!r}"
            scr.setdefault("nodes", {}), scr.setdefault("edges", {}), scr.setdefault("paths", {})

    def safe_press(self, scr, k):
        try:
            return self.press(scr, k)
        except Exception as e:
            return {"screen": scr["name"], "sig": scr.get("sig"), "ws": scr["ws"], "label": k, "key": k, "path": [], "prefix": scr["prefix"], "kind": "unreached", "what": f"crawler error: {e!r}"}

    # ---------------------------------------------------------------- explore
    def explore(self, scr):
        """Walks the screen's focus graph with the d-pad; fills scr['nodes'] and scr['edges']."""
        nodes, edges, untried = {}, {}, {}
        budget = self.a.budget
        restarts = 0
        try:
            L, st = self.start(scr, "explore")
        except Dead as e:
            scr["crash"] = f"could not start: {e}"
            scr["nodes"], scr["edges"], scr["paths"] = {}, {}, {}
            return
        try:
            scr["sig"] = sig_of(st)
            home = fkey(st)
            scr["home"] = home
            nodes[home] = {"label": label_of(st), "state": st}
            untried[home] = list(DIRS)
            cur = home
            presses = 0
            while presses < budget:
                if untried.get(cur):
                    d = untried[cur].pop(0)
                    st = L.step([d])
                    presses += 1
                    if sig_of(st) != scr["sig"]:
                        edges[(cur, d)] = "EXIT"
                        st = L.step([OPP[d]])
                        presses += 1
                        if sig_of(st) != scr["sig"]:
                            st = L.step(["b"])
                            presses += 1
                        if sig_of(st) != scr["sig"] or fkey(st) not in nodes:
                            L.close()
                            restarts += 1
                            if restarts > 6:
                                break
                            L, st = self.start(scr, "explore")
                        cur = fkey(st)
                        continue
                    k = fkey(st)
                    edges[(cur, d)] = k
                    if k not in nodes:
                        if len(nodes) >= self.a.max_nodes:
                            continue
                        nodes[k] = {"label": label_of(st), "state": st}
                        untried[k] = list(DIRS)
                    cur = k
                    continue
                # Route to the nearest node with untried directions over known edges.
                route = self.route(edges, cur, lambda k: bool(untried.get(k)))
                if route is None:
                    break
                for d in route:
                    st = L.step([d])
                    presses += 1
                    k = fkey(st)
                    exp = edges.get((cur, d))
                    if sig_of(st) != scr["sig"]:
                        L.close()
                        restarts += 1
                        if restarts > 6:
                            break
                        L, st = self.start(scr, "explore")
                        cur = fkey(st)
                        break
                    if k not in nodes:
                        nodes[k] = {"label": label_of(st), "state": st}
                        untried[k] = list(DIRS)
                    edges[(cur, d)] = k
                    cur = k
                    if k != exp:
                        break
                if restarts > 6:
                    break
            scr["presses"] = presses
        except Dead as e:
            scr["crash"] = str(e)
        finally:
            scr["log"] = L.log_problems()
            L.close()
        scr["nodes"], scr["edges"] = nodes, edges
        # Only controls that were seen (an edge may point past the node cap).
        scr["paths"] = {k: v for k, v in self.paths(edges, scr.get("home")).items() if k in nodes}

    @staticmethod
    def route(edges, frm, want):
        adj = collections.defaultdict(list)
        for (k, d), k2 in edges.items():
            if k2 != "EXIT":
                adj[k].append((d, k2))
        q = collections.deque([(frm, [])])
        seen = {frm}
        while q:
            k, p = q.popleft()
            if want(k) and p:
                return p
            for d, k2 in adj[k]:
                if k2 not in seen:
                    seen.add(k2)
                    q.append((k2, p + [d]))
        return None

    @staticmethod
    def paths(edges, home):
        if home is None:
            return {}
        adj = collections.defaultdict(list)
        for (k, d), k2 in edges.items():
            if k2 != "EXIT":
                adj[k].append((d, k2))
        out = {home: []}
        q = collections.deque([home])
        while q:
            k = q.popleft()
            for d, k2 in adj[k]:
                if k2 not in out:
                    out[k2] = out[k] + [d]
                    q.append(k2)
        return out

    # ---------------------------------------------------------------- press
    def targets(self, scr):
        """The controls to press: all, except long runs of look-alike tiles (first 3 and the last)."""
        groups = collections.defaultdict(list)
        for k in scr["paths"]:
            g = k.split("|", 1)[0]
            st = scr["nodes"][k]["state"]["app"]["focus"]
            r = st.get("rect") or [0, 0, 0, 0]
            groups[(g, round(r[2] / 8), round(r[3] / 8))].append(k)
        out, skipped = [], 0
        for g, ks in groups.items():
            if g[0] and len(ks) > 6:
                ks = sorted(ks, key=lambda k: len(scr["paths"][k]))
                skipped += len(ks) - 4
                ks = ks[:3] + ks[-1:]
            out += ks
        scr["similar_skipped"] = skipped
        return out

    def press(self, scr, k):
        path = scr["paths"][k]
        label = scr["nodes"][k]["label"]
        r = {"screen": scr["name"], "sig": scr["sig"], "ws": scr["ws"], "label": label, "key": k, "path": path, "prefix": scr["prefix"]}
        tag = hashlib.sha1((scr["name"] + k).encode()).hexdigest()[:10]
        L = None
        try:
            L, st = self.start(scr, "press")
            if path:
                st = L.step(path)
            if fkey(st) != k:
                # One retry along known edges from wherever the walk ended.
                rt = self.route(scr["edges"], fkey(st), lambda x: x == k)
                if rt:
                    st = L.step(rt)
            if fkey(st) != k:
                r.update(kind="unreached", what=f"focus ended on {label_of(st)!r}")
                return r
            before = st  # also the baseline frame for the diff
            after = L.step(["a", "wait:700"])
            shot = os.path.join(self.a.out, "shots", tag + ".png")
            L.shot(shot)
            r["shot"] = shot
            r.update(self.judge(before, after, L))
            acts_run = after["app"]["acts"][len(before["app"]["acts"]):]
            if r["kind"] in ("dead", "silent") and SYSTEM.search(label):
                r["kind"] = "system"
                r["what"] = "a power action: the fixture backend does nothing visible here; " + r["what"]
            elif r["kind"] == "silent" and any(HANDOFF.match(x) for x in acts_run):
                r["kind"] = "system"
                r["what"] = "hands off to the system or a game (shown by gamescope on the device, not here); " + r["what"]
            if r["kind"] in ("dead", "silent"):
                # A slider or a value row: A may do nothing while left/right adjusts it.
                for d in ("right", "left"):
                    adj = L.step([d])
                    if sig_of(adj) == sig_of(after) and fkey(adj) == fkey(after) and (adj["diff"] > self.a.px or label_of(adj) != label_of(after)):
                        r["kind"] = "adjusts"
                        r["what"] = f"A does nothing; {d} changes it ({label_of(after)!r} -> {label_of(adj)!r})"
                        break
                    if fkey(adj) != fkey(after) or sig_of(adj) != sig_of(after):
                        break
            if r["kind"] in ("dead", "silent") and sig_of(before) == "osk" and OSK_NOOP.match(label):
                r["kind"] = "expected"
                r["what"] = "keyboard key on an empty field (nothing to delete, an empty clipboard in the sandbox); " + r["what"]
            elif r["kind"] in ("dead", "silent") and self.selected_check(scr, k):
                r["kind"] = "selected"
                r["what"] = "already selected: A on a neighbour picks that one, then A here picks this one again; " + r["what"]
            if r["kind"] == "toast" and FIXTURE_TOAST.search(r["what"]):
                r["kind"] = "expected"
            if r["kind"] in ("navigated", "opened") and stacked(after):
                b = L.step(["b", "wait:400"])
                if sig_of(b) == sig_of(after) and b["diff"] <= self.a.px and fkey(b) == fkey(after):
                    r["deadend"] = f"B does nothing on {sig_of(after)}"
                    L.shot(shot.replace(".png", "-b.png"))
                    r["deadend_shot"] = shot.replace(".png", "-b.png")
            # A press that led somewhere new: crawl that screen too.
            r["opened_sig"] = None
            if r["kind"] in ("navigated", "opened") or (r["kind"] == "changed" and after["diff"] > self.a.view_px):
                r["opened_sig"] = sig_of(after) + ("" if r["kind"] != "changed" else " view=" + re.sub(r"\d+", "#", (after["app"]["focus"].get("group") or "") + ":" + label_of(after)[:24]))
        except Dead as e:
            r.update(kind="crash", what=str(e))
        finally:
            if L:
                r["log"] = L.log_problems()
                r["execs"] = L.execs()
                L.close()
        return r

    def selected_check(self, scr, k):
        """True when the control is the selected one of a row of choices: pressing a neighbour in the
        same row changes the screen, and pressing this one afterwards changes it back."""
        L = None
        try:
            L, st = self.start(scr, "selected")
            if scr["paths"][k]:
                st = L.step(scr["paths"][k])
            if fkey(st) != k:
                return False
            y = (st["app"]["focus"].get("rect") or [0, 0, 0, 0])[1]
            sig = sig_of(st)
            for d in ("right", "left"):
                n = L.step([d])
                ny = (n["app"]["focus"].get("rect") or [0, 0, 0, 0])[1]
                if sig_of(n) != sig or fkey(n) == k or abs(ny - y) > 4:
                    if fkey(n) != k:
                        n = L.step([OPP[d]])
                        if fkey(n) != k:
                            return False
                    continue
                picked = L.step(["a", "wait:700"])
                if sig_of(picked) != sig or picked["diff"] <= self.a.px:
                    return False
                back = L.step([OPP[d]])
                if fkey(back) != k:
                    return False
                again = L.step(["a", "wait:700"])
                return sig_of(again) == sig and again["diff"] > self.a.px
            return False
        except Dead:
            return False
        finally:
            if L:
                L.close()

    def judge(self, before, after, L):
        b, a = before["app"], after["app"]
        sb, sa = sig_of(before), sig_of(after)
        toasts = [t["text"] for t in a["toasts"] if t["id"] not in {x["id"] for x in b["toasts"]}]
        acts = a["acts"][len(b["acts"]):]
        jobs = a["jobs"] - b["jobs"]
        execs = after["execs"] - before["execs"]
        bits = []
        if toasts:
            bits.append("toast " + "; ".join(repr(t) for t in toasts))
        if acts:
            bits.append("act " + ", ".join(acts[:3]))
        if execs:
            bits.append("process " + "; ".join(L.execs()[-execs:][:3]))
        if jobs:
            bits.append(f"{jobs} backend job(s)")
        if sa != sb:
            kind = "opened" if (stacked(after) and not stacked(before)) or (a.get("osk") and not b.get("osk")) or (a.get("options") and not b.get("options")) or (a.get("legacy") and not b.get("legacy")) else "navigated"
            return {"kind": kind, "what": f"{sb} -> {sa}" + (("; " + "; ".join(bits)) if bits else "")}
        if toasts:
            return {"kind": "toast", "what": "; ".join(bits)}
        visible = after["diff"] > self.a.px or fkey(after) != fkey(before)
        if visible:
            what = f"{after['diff']} px changed" + (f", focus to {label_of(after)!r}" if fkey(after) != fkey(before) else "")
            return {"kind": "changed", "what": what + (("; " + "; ".join(bits)) if bits else "")}
        if bits:
            return {"kind": "silent", "what": "nothing visible; " + "; ".join(bits)}
        return {"kind": "dead", "what": "nothing: no navigation, state, sheet, toast, act, job or process" + (f" ({after['diff']} px)" if after["diff"] > 0 else "")}

    # ---------------------------------------------------------------- run
    def run(self):
        a = self.a
        queue = []
        for name, ws, env, prefix, cmd in seeds(a.device):
            if a.only and ws not in a.only:
                continue
            if a.seed and not re.search(a.seed, name):
                continue
            queue.append({"name": name, "ws": ws, "env": env, "prefix": list(prefix), "cmd": cmd, "depth": 0})
        known = set()
        with cf.ThreadPoolExecutor(max_workers=a.jobs) as ex:
            while queue:
                t0 = time.monotonic()
                batch, queue = queue, []
                list(ex.map(self.safe_explore, batch))
                fresh = []
                for scr in batch:
                    # A view inside a page (same signature as its page) is known by what opened it.
                    ident = scr["_want"] if " view=" in (scr.get("_want") or "") else scr.get("sig")
                    k = scr.get("sig", "?") + "|" + scr["name"] if scr["depth"] == 0 else ident
                    if scr["depth"] > 0 and ident in known and " view=" not in ident:
                        scr["dupe"] = True
                        continue
                    known.add(ident)
                    scr["ws"] = ws_of(scr.get("sig", ""), scr["ws"]) if scr["depth"] > 0 else scr["ws"]
                    fresh.append(scr)
                    self.screens[k] = scr
                print(f"explored {len(batch)} screens ({len(fresh)} new) in {time.monotonic() - t0:.0f} s: " + ", ".join(f"{s['name']} [{len(s.get('nodes', {}))}]" for s in fresh), flush=True)
                jobs = [(s, k) for s in fresh for k in self.targets(s)]
                t0 = time.monotonic()
                res = list(ex.map(lambda t: self.safe_press(*t), jobs))
                self.results += res
                c = collections.Counter(r["kind"] for r in res)
                print(f"pressed {len(res)} controls in {time.monotonic() - t0:.0f} s: {dict(c)}", flush=True)
                # Screens the presses opened, one crawl each (by signature).
                per_parent = collections.Counter()
                for (s, k), r in zip(jobs, res):
                    o = r.get("opened_sig")
                    if not o or s["depth"] + 1 > a.depth or o in known or o in {q.get("_want") for q in queue}:
                        continue
                    if per_parent[s["name"]] >= a.per_parent or len(self.screens) + len(queue) >= a.max_screens:
                        continue
                    per_parent[s["name"]] += 1
                    nm = f"{s['name']} > {r['label'][:40]}"
                    queue.append({"name": nm, "ws": ws_of(o, s["ws"]), "env": s["env"], "cmd": s["cmd"], "prefix": s["prefix"] + r["path"] + ["a", "wait:700"], "depth": s["depth"] + 1, "_want": o})
                    if " view=" in o:
                        known.add(o)


def write_report(out, md, shots_dir=None, run_label=""):
    data = json.load(open(os.path.join(out, "results.json")))
    res, screens = data["results"], data["screens"]
    if shots_dir:
        os.makedirs(shots_dir, exist_ok=True)

    def shot(p):
        if not p:
            return "-"
        if shots_dir and os.path.exists(p):
            dst = os.path.join(shots_dir, os.path.basename(p))
            shutil.copyfile(p, dst)
            return dst
        return p

    bad = [r for r in res if r["kind"] in ("dead", "silent", "crash") or r.get("deadend") or r.get("log")]
    names = {"A": "A: Sui Games, Market, Trade", "B": "B: Home, Library, game pages, Store, Apps, Downloads, Storage", "C": "C: Wallet, Friends, Profile, Share, trophies, Device Pass", "D": "D: Settings, Quick settings, Rare menu, first run"}
    by = collections.defaultdict(list)
    for r in bad:
        by[r["ws"]].append(r)
    pressed = collections.Counter(r["ws"] for r in res if r["kind"] != "unreached")
    fine = collections.Counter(r["ws"] for r in res if r["kind"] in ("selected", "expected"))
    L = [
        "# Dead controls (native launcher crawler)",
        "",
        f"Run {data['run']} ({run_label}), {data.get('commit', '?')}, {data.get('res')}, {round(data['secs'] / 60, 1)} min. Written by `tools/journeys/crawl.py` (CI `Dead-control crawler` on the build machine runner).",
        "The real launcher headless on the build machine, the SuiPlay's library and config copied read only, fixture backend for the rest (no network: images do not load). Every focusable control reached with the d-pad on every screen was pressed with A from a fresh start.",
        "DEAD = no navigation, no state change, no sheet, no toast, no act, no job, no process (and left/right do not adjust it either). SILENT = something ran in the background (a backend job or an act) but nothing visible changed: on the device it may still work, but there is no visible feedback. Power actions (Sleep, Restart, Turn off) are not judged. DEAD END = it opened something that B does not close. Look-alike tiles past the first 3 in a row are not pressed. SELECTED = the choice already picked in its row (proved: a neighbour picks, then this one picks again); EXPECTED = keyboard Paste/Delete on an empty field and device-only fixture toasts. Neither is listed below.",
        "",
        "| Workstream | Screens | Controls pressed | Dead | Silent | Dead ends | Crashes/log errors | Selected or expected (not listed) |",
        "|---|---|---|---|---|---|---|---|",
    ]
    for ws in "ABCD":
        rs = by.get(ws, [])
        L.append(f"| {ws} | {sum(1 for s in screens if s['ws'] == ws)} | {pressed[ws]} | {sum(r['kind'] == 'dead' for r in rs)} | {sum(r['kind'] == 'silent' for r in rs)} | {sum(bool(r.get('deadend')) for r in rs)} | {sum(r['kind'] == 'crash' or bool(r.get('log')) for r in rs)} | {fine[ws]} |")
    L.append("")
    for ws in "ABCD":
        L.append(f"## {names[ws]}")
        L.append("")
        rs = sorted(by.get(ws, []), key=lambda r: (r["screen"], r["label"]))
        if not rs:
            L.append("Nothing dead found.")
            L.append("")
        cur = None
        for r in rs:
            if r["screen"] != cur:
                cur = r["screen"]
                L.append(f"### {cur}")
                L.append("")
                L.append(f"How to get there: {r['sig']}; start steps {','.join(r['prefix']) or '(none)'}")
                L.append("")
            if r["kind"] in ("dead", "silent", "crash"):
                L.append(f"- **{r['kind'].upper()}** `{r['label']}`: {r['what']}. Path from screen start: {','.join(r['path']) or '(focused first)'},a. Shot: {shot(r.get('shot'))}")
            if r.get("deadend"):
                L.append(f"- **DEAD END** `{r['label']}`: opens, then {r['deadend']}. Shot: {shot(r.get('deadend_shot'))}")
            for line in (r.get("log") or [])[:3]:
                L.append(f"- **LOG** after `{r['label']}`: `{line[:200]}`")
        L.append("")
    crash_screens = [s for s in screens if s.get("crash") or s.get("log")]
    if crash_screens:
        L.append("## Screens with a crash or log errors while walking")
        L.append("")
        for s in crash_screens:
            L.append(f"- {s['name']} ({s.get('sig')}): {s.get('crash') or ''} {'; '.join(x[:200] for x in s.get('log', [])[:3])}")
        L.append("")
    L.append("## Screens crawled")
    L.append("")
    for s in sorted(screens, key=lambda s: (s["ws"], s["name"])):
        L.append(f"- {s['ws']} {s['name']}: {s['controls']} controls ({s.get('similar_skipped', 0)} look-alikes not pressed), {s.get('unreached', 0)} not reproducible; {s.get('sig')}")
    L.append("")
    open(md, "w").write("\n".join(L) + "\n")
    return {ws: len(by.get(ws, [])) for ws in "ABCD"}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["run", "report"])
    ap.add_argument("--bin", default=DEFAULT_BIN)
    ap.add_argument("--device", default=DEFAULT_DEVICE)
    ap.add_argument("--out", default=None)
    ap.add_argument("--md", default=None)
    ap.add_argument("--shots", default=None)
    ap.add_argument("--jobs", type=int, default=20)
    ap.add_argument("--res", default="1280x800")
    ap.add_argument("--only", default="")
    ap.add_argument("--seed", default="", help="regex on seed names")
    ap.add_argument("--depth", type=int, default=2)
    ap.add_argument("--budget", type=int, default=500, help="d-pad presses per screen walk")
    ap.add_argument("--max-nodes", type=int, default=250)
    ap.add_argument("--max-screens", type=int, default=160)
    ap.add_argument("--per-parent", type=int, default=12)
    ap.add_argument("--px", type=int, default=40, help="pixels that must change to count as visible")
    ap.add_argument("--view-px", type=int, default=200000)
    ap.add_argument("--commit", default="")
    ap.add_argument("--label", default="")
    ap.add_argument("--no-sandbox", dest="sandbox", action="store_false")
    a = ap.parse_args()
    a.only = set(x for x in a.only.split(",") if x)
    run_id = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    a.out = a.out or os.path.join(DEFAULT_OUT, run_id)
    if a.cmd == "report":
        print(write_report(a.out, a.md or os.path.join(a.out, "dead-controls.md"), a.shots, a.label))
        return 0
    os.makedirs(os.path.join(a.out, "shots"), exist_ok=True)
    a.work = os.path.join(a.out, "work")
    a.stubs = os.path.join(a.out, "stubs")
    os.makedirs(a.stubs, exist_ok=True)
    for n in STUBS:
        p = os.path.join(a.stubs, n)
        open(p, "w").write(f'#!/bin/dash\necho "{n} $*" >> "$RR_CRAWL_EXEC"\nexit 1\n')
        os.chmod(p, 0o755)
    for n in PASS:
        if shutil.which(n):
            os.symlink(shutil.which(n), os.path.join(a.stubs, n))
    t0 = time.monotonic()
    c = Crawl(a)
    c.run()
    screens = []
    for k, s in c.screens.items():
        rs = [r for r in c.results if r["screen"] == s["name"]]
        screens.append({"name": s["name"], "ws": s["ws"], "sig": s.get("sig"), "prefix": s["prefix"], "controls": len(s.get("nodes", {})), "similar_skipped": s.get("similar_skipped", 0), "unreached": sum(r["kind"] == "unreached" for r in rs), "presses": s.get("presses"), "crash": s.get("crash"), "log": s.get("log", [])})
    json.dump({"run": run_id, "commit": a.commit, "res": a.res, "secs": time.monotonic() - t0, "screens": screens, "results": c.results}, open(os.path.join(a.out, "results.json"), "w"), indent=1)
    counts = write_report(a.out, a.md or os.path.join(a.out, "dead-controls.md"), a.shots, a.label)
    shutil.rmtree(a.work, ignore_errors=True)
    print(f"done in {(time.monotonic() - t0) / 60:.1f} min; problems per workstream {counts}; {a.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
