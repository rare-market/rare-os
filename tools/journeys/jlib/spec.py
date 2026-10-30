"""Journey files: loading, defaults and validation. Python 3.9+, stdlib (+ PyYAML when present).

A journey is a YAML or JSON document (see tools/journeys/README.md):

    name: tabs
    title: Every top tab
    tags: [core, launcher]
    needs: [probe]
    timeout_s: 90
    steps:
      - press: rb
        expect: {screen: {tab: library}, respond_ms: 150}

YAML is read with PyYAML (on the device it is installed; on a Mac without it, Ruby's YAML is used).
"""
from __future__ import annotations

import json
import os
import re
import subprocess

# Player buttons -> InputPlumber capability. Triggers are floats, everything else a bool.
BUTTONS = {
    "a": "Gamepad:Button:South",
    "b": "Gamepad:Button:East",
    "x": "Gamepad:Button:West",
    "y": "Gamepad:Button:North",
    "lb": "Gamepad:Button:LeftBumper",
    "rb": "Gamepad:Button:RightBumper",
    "lt": "Gamepad:Trigger:LeftTrigger",
    "rt": "Gamepad:Trigger:RightTrigger",
    "up": "Gamepad:Button:DPadUp",
    "down": "Gamepad:Button:DPadDown",
    "left": "Gamepad:Button:DPadLeft",
    "right": "Gamepad:Button:DPadRight",
    "guide": "Gamepad:Button:Guide",
    "select": "Gamepad:Button:Select",
    "start": "Gamepad:Button:Start",
    "l3": "Gamepad:Button:LeftStick",
    "r3": "Gamepad:Button:RightStick",
    "volume_up": "Keyboard:KeyVolumeUp",
    "volume_down": "Keyboard:KeyVolumeDown",
}
# Named chords and holds, as a player does them.
MACROS = {
    # Quick access: Guide + A, the chord InputPlumber emits for the QAM key on an Xbox target.
    "qam": {"press": ["guide", "a"], "hold_ms": 120},
    # The SuiPlay's Quick button (Quick settings): the same quick access chord on the Xbox target.
    "quick": {"press": ["guide", "a"], "hold_ms": 120},
    # The way out of any game or app (launcher needs 0.6 s).
    "exit": {"press": ["select", "start"], "hold_ms": 900},
    "guide_hold": {"press": ["guide"], "hold_ms": 900},
    # The in-game Rare panel.
    "panel": {"press": ["select"], "hold_ms": 900},
}
# L3 + R3 held is the abort chord; journeys may not use it.
ABORT_CHORD = ("l3", "r3")
# Never pressed alone without allow_unsafe: Select opens Steam Big Picture in the launcher.
UNSAFE_ALONE = {"select"}
# A is refused while the probe shows one of these focused (never power off, sign, buy...).
DENY_A_FOCUS = re.compile(
    r'"[^"]*\b(Power off|Restart|Desktop mode|Reset|Erase|Delete|Remove|Uninstall|Sign out|Forget|'
    r'Get |Install|Update|Send|Buy|Sell|Place order|Confirm|Approve|Sign|Swap|Withdraw|Deposit|List for sale|Accept offer|Make offer|Quit now)\b',
    re.I,
)
FRAME_FAULTS = ("black", "blank", "flash", "stale")
STEP_KINDS = ("press", "stick", "key", "type", "wait", "wait_for", "nav", "goto", "sleep_wake", "restart_launcher", "mark")
NEEDS = ("probe", "root_or_attended", "steam", "network", "wallet", "video", "session_client")
DEFAULT_EXPECT = {"no": list(FRAME_FAULTS), "hitch_ms": 100}


class SpecError(ValueError):
    pass


def _load_text(path: str):
    text = open(path, encoding="utf-8").read()
    if path.endswith(".json"):
        return json.loads(text)
    try:
        import yaml  # type: ignore

        return yaml.safe_load(text)
    except ImportError:
        out = subprocess.run(
            ["ruby", "-ryaml", "-rjson", "-e", "puts JSON.dump(YAML.safe_load(STDIN.read))"],
            input=text, capture_output=True, text=True, check=False,
        )
        if out.returncode != 0:
            raise SpecError(f"{path}: no PyYAML and ruby failed: {out.stderr.strip()}")
        return json.loads(out.stdout)


def journey_files(root: str):
    out = []
    for d, _, files in os.walk(root):
        for f in sorted(files):
            if f.endswith((".yaml", ".yml", ".json")) and not f.startswith("_"):
                out.append(os.path.join(d, f))
    return sorted(out)


def load_vars(root: str) -> dict:
    """journeys/_targets.yaml: names for the device's games and apps (${steam_game} etc.)."""
    for n in ("_targets.yaml", "_targets.yml", "_targets.json"):
        p = os.path.join(root, n)
        if os.path.exists(p):
            return _load_text(p) or {}
    return {}


def _subst(v, vars_: dict):
    if isinstance(v, str):
        def rep(m):
            k = m.group(1)
            if k not in vars_:
                raise SpecError(f"unknown variable ${{{k}}}")
            return str(vars_[k])
        return re.sub(r"\$\{([a-zA-Z0-9_]+)\}", rep, v)
    if isinstance(v, list):
        return [_subst(x, vars_) for x in v]
    if isinstance(v, dict):
        return {k: _subst(x, vars_) for k, x in v.items()}
    return v


def _expand_step(raw, where: str) -> dict:
    if isinstance(raw, str):
        # Shorthand: "rb", "wait 500", "qam"
        parts = raw.split()
        if parts[0] == "wait" and len(parts) == 2:
            raw = {"wait": int(parts[1])}
        else:
            raw = {"press": raw}
    if not isinstance(raw, dict):
        raise SpecError(f"{where}: a step is a mapping or a button name")
    st = dict(raw)
    kinds = [k for k in STEP_KINDS if k in st]
    # wait_for next to an action is the action's post-condition.
    if len(kinds) == 2 and "wait_for" in kinds:
        kinds.remove("wait_for")
        if not isinstance(st["wait_for"], dict) or not st["wait_for"]:
            raise SpecError(f"{where}: wait_for needs conditions")
        st["wait_for"].setdefault("timeout_ms", 5000)
    if len(kinds) != 1:
        raise SpecError(f"{where}: a step needs exactly one of {', '.join(STEP_KINDS)} (has {kinds or 'none'})")
    kind = kinds[0]
    st["kind"] = kind
    if kind == "press":
        p = st["press"]
        if isinstance(p, str) and p in MACROS:
            m = MACROS[p]
            st.setdefault("label", p)
            st["press"] = list(m["press"])
            st.setdefault("hold_ms", m["hold_ms"])
        elif isinstance(p, str):
            st["press"] = [p]
        for b in st["press"]:
            if b not in BUTTONS:
                raise SpecError(f"{where}: unknown button {b!r} (buttons: {', '.join(BUTTONS)}; macros: {', '.join(MACROS)})")
        if set(st["press"]) >= set(ABORT_CHORD):
            raise SpecError(f"{where}: L3 + R3 is the abort chord")
        if len(st["press"]) == 1 and st["press"][0] in UNSAFE_ALONE and not st.get("allow_unsafe"):
            raise SpecError(f"{where}: {st['press'][0]} alone opens Steam; set allow_unsafe: true if that is the point")
        st.setdefault("hold_ms", 80)
        st.setdefault("repeat", 1)
        st.setdefault("gap_ms", 250)
    elif kind == "nav":
        n = st["nav"]
        if not isinstance(n, dict) or "until" not in n:
            raise SpecError(f"{where}: nav needs {{press: <dir>, until: {{focus: <regex>}}}}")
        n.setdefault("press", "down")
        n.setdefault("max", 12)
        n.setdefault("gap_ms", 220)
        if n["press"] not in BUTTONS:
            raise SpecError(f"{where}: nav press {n['press']!r} is not a button")
    elif kind == "goto":
        g = str(st["goto"])
        # Bare tab names (goto: settings) mean the tab.
        st["goto"] = g if ":" in g else f"tab:{g}"
    elif kind == "wait_for":
        w = st["wait_for"]
        if not isinstance(w, dict) or not w:
            raise SpecError(f"{where}: wait_for needs conditions")
        w.setdefault("timeout_ms", 5000)
    elif kind == "sleep_wake":
        s = st["sleep_wake"] if isinstance(st["sleep_wake"], dict) else {}
        s.setdefault("seconds", 20)
        st["sleep_wake"] = s
    elif kind == "stick":
        s = st["stick"]
        if not isinstance(s, dict) or not ({"left", "right"} & set(s)):
            raise SpecError(f"{where}: stick needs left: [x, y] or right: [x, y]")
        s.setdefault("ms", 300)
    exp = st.get("expect") or {}
    if not isinstance(exp, dict):
        raise SpecError(f"{where}: expect must be a mapping")
    if kind in ("press", "nav", "stick", "key", "sleep_wake", "restart_launcher"):
        merged = dict(DEFAULT_EXPECT)
        merged.update(exp)
        allow = st.get("allow") or []
        merged["no"] = [f for f in merged.get("no", []) if f not in allow]
        exp = merged
    for f in exp.get("no", []):
        if f not in FRAME_FAULTS:
            raise SpecError(f"{where}: expect.no {f!r} is not one of {FRAME_FAULTS}")
    st["expect"] = exp
    st.setdefault("name", st.get("label") or _auto_name(st))
    return st


def _auto_name(st: dict) -> str:
    k = st["kind"]
    if k == "press":
        return "+".join(st["press"])
    if k == "wait":
        return f"wait {st['wait']} ms"
    if k == "goto":
        return f"goto {st['goto']}"
    if k == "nav":
        return f"nav {st['nav']['press']} to {st['nav']['until']}"
    if k == "wait_for":
        return "wait for " + ", ".join(f"{a}={b}" for a, b in st["wait_for"].items() if a != "timeout_ms")
    return k


def normalize(doc, path: str, vars_: dict | None = None) -> dict:
    if not isinstance(doc, dict):
        raise SpecError(f"{path}: a journey is a mapping")
    doc = _subst(doc, vars_ or {})
    name = doc.get("name") or os.path.splitext(os.path.basename(path))[0]
    if not re.fullmatch(r"[a-z0-9][a-z0-9_.-]*", name):
        raise SpecError(f"{path}: name {name!r} must be lowercase letters, digits, _ . -")
    steps = doc.get("steps")
    if not isinstance(steps, list) or not steps:
        raise SpecError(f"{path}: steps must be a non-empty list")
    for n in doc.get("needs", []) or []:
        if n not in NEEDS:
            raise SpecError(f"{path}: unknown need {n!r} (known: {', '.join(NEEDS)})")
    j = {
        "name": name,
        "title": doc.get("title", name),
        "file": path,
        "tags": list(doc.get("tags", []) or []),
        "needs": list(doc.get("needs", []) or []),
        "timeout_s": float(doc.get("timeout_s", 120)),
        # Where the journey starts: home (cleanup first) or as-is.
        "start": doc.get("start", "home"),
        "cleanup": doc.get("cleanup", "home"),
        "steps": [_expand_step(s, f"{path} step {i + 1}") for i, s in enumerate(steps)],
    }
    if j["timeout_s"] <= 0 or j["timeout_s"] > 900:
        raise SpecError(f"{path}: timeout_s must be in (0, 900]")
    return j


def load_all(root: str, only=None, skip=None, tags=None) -> list:
    vars_ = load_vars(root)
    out, seen = [], set()
    for f in journey_files(root):
        j = normalize(_load_text(f), f, vars_)
        if j["name"] in seen:
            raise SpecError(f"{f}: duplicate journey name {j['name']}")
        seen.add(j["name"])
        if only and j["name"] not in only:
            continue
        if skip and j["name"] in skip:
            continue
        if tags and not (set(tags) & set(j["tags"])):
            continue
        out.append(j)
    if only:
        missing = set(only) - {j["name"] for j in out}
        if missing:
            raise SpecError(f"no journey named {', '.join(sorted(missing))}")
        order = {n: i for i, n in enumerate(only)}
        out.sort(key=lambda j: order[j["name"]])
    return out
