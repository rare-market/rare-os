"""Markdown + HTML report from analyze() results."""
from __future__ import annotations

import base64
import html
import json
import os

from . import frames as F
from .analyze import Run


def clip_name(journey: str, idx: int) -> str:
    return f"{journey}__{idx:02d}.mp4"


def clip_windows(res: dict) -> list:
    """Clip windows for every failing step (and warn steps with frame faults)."""
    out = []
    for j in res["journeys"]:
        for s in j["steps"]:
            c = (s.get("metrics") or {}).get("clip")
            if not c:
                continue
            faulty = s["verdict"] == "fail" or any(True for _ in (s.get("metrics") or {}).get("faults", []))
            if faulty:
                out.append({"name": clip_name(j["name"], s["idx"]), "start_ns": c[0], "end_ns": c[1]})
    return out


def _fmt(v, unit=""):
    return "" if v is None else f"{v}{unit}"


def _row_metrics(s: dict) -> dict:
    m = s.get("metrics") or {}
    p = m.get("power") or {}
    return {
        "respond": _fmt(m.get("respond_ms"), " ms"),
        "settle": _fmt(m.get("settle_ms"), " ms"),
        "screen": _fmt(m.get("screen_ms"), " ms"),
        "gap": _fmt(m.get("max_gap_ms"), " ms"),
        "faults": ", ".join(f"{f['kind']}x{f['frames']}" for f in m.get("faults", [])),
        "watts": "" if p.get("socket_w") is None else f"{p['socket_w']} W" + (f" (cpu {p['cpu_w']}, gpu {p['gfx_w']})" if p.get("cpu_w") is not None and p.get("gfx_w") is not None else ""),
    }


def markdown(res: dict, clips_dir: str | None = None) -> str:
    meta = res["meta"]
    L = [f"# Rare OS journeys: {meta.get('run_id', '')}", ""]
    L.append(f"Launcher {meta.get('launcher_version', '?')}, OS {meta.get('os_version', '?')}, capture "
             f"{(meta.get('capture') or {}).get('pipeline_name', 'none')}, probe {'on' if res['probe'] else 'off'}, "
             f"{res['frames']} frames, nominal frame {res['nominal_frame_ms']} ms.")
    s = res["summary"]
    ss = res["steps_summary"]
    L.append(f"Journeys: {s['pass']} pass, {s['warn']} warn, {s['fail']} fail, {s['skip']} skipped. "
             f"Steps: {ss['pass']} pass, {ss['warn']} warn, {ss['fail']} fail, {ss['skip']} skipped.")
    if meta.get("config_changed"):
        L += ["", f"Launcher settings changed by the run: {', '.join(meta['config_changed'])} "
                  f"(`tools/journeys/run.sh restore-config {meta.get('run_id', '')}` puts them back)."]
    if res["crashes"]:
        L += ["", "## Crashes", ""]
        L += [f"- {c.get('source')}: {c.get('detail', '')}" for c in res["crashes"]]
    w = res["worst"]
    L += ["", "## Worst offenders", ""]
    if w["respond_ms"]:
        L.append("Slowest response (input to first changed frame): " + "; ".join(
            f"{x['journey']} / {x['step']} {x['respond_ms']} ms" for x in w["respond_ms"][:5]))
    if w["max_gap_ms"]:
        L.append("")
        L.append("Longest gap between moving frames: " + "; ".join(
            f"{x['journey']} / {x['step']} {x['max_gap_ms']} ms" for x in w["max_gap_ms"][:5]))
    if w["faults"]:
        L.append("")
        L.append("Frame faults: " + "; ".join(f"{x['journey']} / {x['step']} {x['kind']} x{x['frames']} ({x['ms']} ms)" for x in w["faults"][:15]))
    if w["power"]:
        L.append("")
        L.append("Highest APU power: " + "; ".join(f"{x['journey']} / {x['step']} {x['socket_w']} W" for x in w["power"][:5]))
    for j in res["journeys"]:
        L += ["", f"## {j['verdict'].upper()} {j['name']}: {j['title']}", ""]
        if j["status"] not in ("completed", "running"):
            L.append(f"Ended: {j['status']} {j.get('reason', '')}".rstrip())
            L.append("")
        for c in j.get("crashes", []):
            L.append(f"Crash: {c.get('source')} {c.get('detail', '')}")
        if not j["steps"]:
            continue
        L.append("| # | step | verdict | respond | settle | screen | worst gap | faults | power | notes |")
        L.append("|---|---|---|---|---|---|---|---|---|---|")
        for st in j["steps"]:
            r = _row_metrics(st)
            notes = "; ".join(st.get("reasons", []) + st.get("warns", []))
            clip = clip_name(j["name"], st["idx"])
            if clips_dir and os.path.exists(os.path.join(clips_dir, clip)):
                notes += f" [clip](clips/{clip})"
            L.append(f"| {st['idx'] + 1} | {st['name']} | {st['verdict']} | {r['respond']} | {r['settle']} | {r['screen']} | "
                     f"{r['gap']} | {r['faults']} | {r['watts']} | {notes.replace('|', '/')} |")
    return "\n".join(L) + "\n"


CSS = """
:root{--bg:#0d0f12;--fg:#e8eaed;--mut:#8b9098;--hair:#262a31;--ok:#5fb37a;--warn:#c9a43b;--bad:#d06060;--card:#15181d}
@media (prefers-color-scheme: light){:root:not([data-theme="dark"]){--bg:#fafafa;--fg:#16181c;--mut:#5d636c;--hair:#e2e4e8;--card:#fff}}
body{background:var(--bg);color:var(--fg);font:14px/1.45 system-ui,-apple-system,sans-serif;margin:0;padding:24px 16px;max-width:1200px;margin:auto}
h1{font-size:20px}h2{font-size:16px;margin-top:28px}table{border-collapse:collapse;width:100%;font-size:13px}
td,th{border-bottom:1px solid var(--hair);padding:6px 8px;text-align:left;vertical-align:top}th{color:var(--mut);font-weight:500}
.pass{color:var(--ok)}.warn{color:var(--warn)}.fail{color:var(--bad)}.skip{color:var(--mut)}.mut{color:var(--mut)}
.card{background:var(--card);border:1px solid var(--hair);border-radius:8px;padding:12px 14px;margin:12px 0}
img.film{image-rendering:pixelated;max-width:100%;border:1px solid var(--hair);margin-top:6px}
.wrap{overflow-x:auto}
"""


def html_report(res: dict, run: Run | None, clips_dir: str | None = None) -> str:
    e = html.escape
    meta = res["meta"]
    s, ss = res["summary"], res["steps_summary"]
    H = ["<!doctype html><html><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'>",
         f"<title>Rare journeys</title><style>{CSS}</style></head><body>",
         f"<h1>Rare OS journeys <span class=mut>{e(str(meta.get('run_id', '')))}</span></h1>",
         f"<p class=mut>Launcher {e(str(meta.get('launcher_version', '?')))} &middot; OS {e(str(meta.get('os_version', '?')))} &middot; "
         f"capture {e(str((meta.get('capture') or {}).get('pipeline_name', 'none')))} &middot; probe {'on' if res['probe'] else 'off'} &middot; "
         f"{res['frames']} frames &middot; nominal frame {res['nominal_frame_ms']} ms</p>",
         f"<div class=card><b>Journeys</b> <span class=pass>{s['pass']} pass</span> &middot; <span class=warn>{s['warn']} warn</span> &middot; "
         f"<span class=fail>{s['fail']} fail</span> &middot; <span class=skip>{s['skip']} skipped</span><br>"
         f"<b>Steps</b> <span class=pass>{ss['pass']} pass</span> &middot; <span class=warn>{ss['warn']} warn</span> &middot; "
         f"<span class=fail>{ss['fail']} fail</span> &middot; <span class=skip>{ss['skip']} skipped</span></div>"]
    if meta.get("config_changed"):
        H.append(f"<p class=warn>Launcher settings changed by the run: {e(', '.join(meta['config_changed']))} "
                 f"(<code>tools/journeys/run.sh restore-config {e(str(meta.get('run_id', '')))}</code> puts them back)</p>")
    if res["crashes"]:
        H.append("<h2 class=fail>Crashes</h2><ul>" + "".join(f"<li>{e(str(c.get('source')))}: {e(str(c.get('detail', '')))}</li>" for c in res["crashes"]) + "</ul>")
    w = res["worst"]
    H.append("<h2>Worst offenders</h2><div class=wrap><table><tr><th>what</th><th>where</th><th>value</th></tr>")
    for x in w["respond_ms"][:5]:
        H.append(f"<tr><td>response</td><td>{e(x['journey'])} / {e(x['step'])}</td><td>{x['respond_ms']} ms</td></tr>")
    for x in w["max_gap_ms"][:5]:
        H.append(f"<tr><td>gap between frames</td><td>{e(x['journey'])} / {e(x['step'])}</td><td>{x['max_gap_ms']} ms</td></tr>")
    for x in w["faults"][:15]:
        H.append(f"<tr><td class=fail>{e(x['kind'])}</td><td>{e(x['journey'])} / {e(x['step'])}</td><td>{x['frames']} frames, {x['ms']} ms</td></tr>")
    for x in w["power"][:5]:
        H.append(f"<tr><td>APU power</td><td>{e(x['journey'])} / {e(x['step'])}</td><td>{x['socket_w']} W</td></tr>")
    H.append("</table></div>")
    for j in res["journeys"]:
        H.append(f"<h2><span class={j['verdict']}>{j['verdict'].upper()}</span> {e(j['name'])} <span class=mut>{e(j['title'])}</span></h2>")
        if j["status"] not in ("completed", "running"):
            H.append(f"<p class=fail>Ended: {e(j['status'])} {e(j.get('reason', ''))}</p>")
        if not j["steps"]:
            continue
        H.append("<div class=wrap><table><tr><th>#</th><th>step</th><th>verdict</th><th>respond</th><th>settle</th><th>screen</th><th>worst gap</th><th>faults</th><th>power</th><th>notes</th></tr>")
        films = []
        for st in j["steps"]:
            r = _row_metrics(st)
            notes = "<br>".join([f"<span class=fail>{e(x)}</span>" for x in st.get("reasons", [])] + [f"<span class=warn>{e(x)}</span>" for x in st.get("warns", [])])
            clip = clip_name(j["name"], st["idx"])
            if clips_dir and os.path.exists(os.path.join(clips_dir, clip)):
                notes += f" <a href='clips/{clip}'>clip</a>"
            H.append(f"<tr><td>{st['idx'] + 1}</td><td>{e(st['name'])}</td><td class={st['verdict']}>{st['verdict']}</td><td>{r['respond']}</td>"
                     f"<td>{r['settle']}</td><td>{r['screen']}</td><td>{r['gap']}</td><td>{e(r['faults'])}</td><td>{e(r['watts'])}</td><td>{notes}</td></tr>")
            film = (st.get("metrics") or {}).get("film")
            if run is not None and run.thumbs.n and film and (st["verdict"] == "fail" or (st.get("metrics") or {}).get("faults")):
                px, tw, th = F.strip([run.thumbs[i] for i in film], run.thumbs.w, run.thumbs.h)
                b64 = base64.b64encode(F.png_gray(px, tw, th, 2)).decode()
                films.append(f"<div class=card><b>{st['idx'] + 1}. {e(st['name'])}</b> <span class=mut>frames {film[0]}-{film[-1]}</span><br>"
                             f"<img class=film alt='frames around the fault' src='data:image/png;base64,{b64}'></div>")
        H.append("</table></div>")
        H.extend(films)
    H.append("</body></html>")
    return "\n".join(H)


def write(run_dir: str, res: dict, clips_dir: str | None = None):
    clips_dir = clips_dir or os.path.join(run_dir, "clips")
    with open(os.path.join(run_dir, "results.json"), "w") as f:
        json.dump(res, f, indent=1, default=str)
    with open(os.path.join(run_dir, "report.md"), "w") as f:
        f.write(markdown(res, clips_dir))
    with open(os.path.join(run_dir, "report.html"), "w") as f:
        f.write(html_report(res, Run(run_dir), clips_dir))
    with open(os.path.join(run_dir, "clips.json"), "w") as f:
        json.dump(clip_windows(res), f, indent=1)
