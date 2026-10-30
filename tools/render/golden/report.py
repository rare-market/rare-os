"""HTML for a golden run: index.html is the parity dashboard (every screen state of the web
launcher and every native state, with native, web and golden status), then one section per native
state with its images, diffs against the approved PNGs, the side-by-side against the web shot,
and its frame-budget gates. Everything is relative to the run directory, so the directory is the
CI artifact."""
from __future__ import annotations

import html
import json
import os
import time

import imgdiff

CSS = """
:root { color-scheme: dark; --bg:#0d0e10; --panel:#16181b; --line:#2a2d32; --fg:#e8e9eb; --dim:#9a9ea6;
  --pass:#7fd1a0; --fail:#ff6b81; --warn:#f2c46d; --none:#5d6168; }
* { box-sizing: border-box; }
body { margin:0; background:var(--bg); color:var(--fg); font:14px/1.45 system-ui, -apple-system, "Segoe UI", sans-serif; }
main { max-width:1500px; margin:0 auto; padding:24px 16px 64px; }
h1 { font-size:22px; margin:0 0 4px; } h2 { font-size:17px; margin:40px 0 8px; } h3 { font-size:15px; margin:24px 0 6px; }
.meta { color:var(--dim); margin-bottom:20px; }
.cards { display:flex; flex-wrap:wrap; gap:12px; margin:16px 0 24px; }
.card { background:var(--panel); border:1px solid var(--line); border-radius:10px; padding:12px 16px; min-width:150px; }
.card b { display:block; font-size:22px; } .card span { color:var(--dim); }
.wrap { overflow-x:auto; border:1px solid var(--line); border-radius:10px; }
table { border-collapse:collapse; width:100%; font-variant-numeric:tabular-nums; }
th, td { text-align:left; padding:6px 10px; border-bottom:1px solid var(--line); white-space:nowrap; }
th { position:sticky; top:0; background:var(--panel); font-weight:600; color:var(--dim); }
tr:hover td { background:#1b1d21; }
.s { display:inline-flex; align-items:center; gap:6px; }
.s::before { content:""; width:7px; height:7px; border-radius:50%; background:var(--none); }
.pass::before { background:var(--pass); } .fail::before { background:var(--fail); } .warn::before { background:var(--warn); }
.dim { color:var(--dim); }
a { color:#9cc3ff; text-decoration:none; } a:hover { text-decoration:underline; }
.imgs { display:flex; flex-wrap:wrap; gap:10px; } .imgs figure { margin:0; } .imgs img { width:360px; max-width:100%; border:1px solid var(--line); border-radius:6px; display:block; }
.imgs figcaption { color:var(--dim); font-size:12px; margin-top:3px; }
.sbs img { width:100%; max-width:1960px; border:1px solid var(--line); border-radius:6px; }
pre { background:var(--panel); border:1px solid var(--line); border-radius:8px; padding:10px; overflow:auto; font-size:12px; }
input { background:var(--panel); color:var(--fg); border:1px solid var(--line); border-radius:8px; padding:7px 10px; width:280px; }
"""

JS = """
const q = document.getElementById('q');
q && q.addEventListener('input', () => { const v = q.value.toLowerCase();
  document.querySelectorAll('#parity tbody tr').forEach(r => r.style.display = r.textContent.toLowerCase().includes(v) ? '' : 'none'); });
"""


def e(s):
    return html.escape(str(s))


def badge(text, cls):
    return f'<span class="s {cls}">{e(text)}</span>'


def golden_cell(r):
    g = (r or {}).get("golden")
    if r and r.get("render") == "error":
        return badge("render error", "fail")
    if g == "pass":
        return badge("pass", "pass")
    if g == "fail":
        return badge(f"fail {r['golden_detail'].get('frac', 0) * 100:.2f} %", "fail")
    if g == "unapproved":
        return badge("unapproved", "warn")
    if g == "no render":
        return badge("no render", "fail")
    return '<span class="dim">-</span>'


def find_web(web_dir, name, res):
    if not web_dir:
        return None
    p = os.path.join(web_dir, f"{name}-{res}.png")
    return p if os.path.exists(p) else None


def write(out, states, results, web_names, idx, web_dir, resolutions, repo, approved_dir):
    os.makedirs(os.path.join(out, "thumbs"), exist_ok=True)
    os.makedirs(os.path.join(out, "sbs"), exist_ok=True)
    by_web = {}
    for s in states:
        if s.get("web"):
            by_web.setdefault(s["web"], []).append(s)
    env = {}
    try:
        env = json.load(open(os.path.join(out, "env.json")))
    except Exception:
        pass

    # side by side against the web shots (at the web sizes: 1920x1200 and 1280x800 by default)
    parity = {}
    for s in states:
        if not s.get("web"):
            continue
        for res in resolutions:
            png = os.path.join(out, "weblook", res, f"{s['id']}.png")
            if not os.path.exists(png):
                png = os.path.join(out, res, f"{s['id']}.png")
            wp = find_web(web_dir, s["web"], res)
            if os.path.exists(png) and wp:
                sbs = os.path.join(out, "sbs", f"{s['id']}-{res}.jpg")
                try:
                    frac = imgdiff.side_by_side(png, wp, sbs)
                    parity[(s["id"], res)] = (round(frac * 100, 1), os.path.relpath(sbs, out))
                except Exception as ex:  # noqa
                    parity[(s["id"], res)] = (None, str(ex))

    # ---- dashboard rows: every web state, then native states with no web counterpart
    rows = []
    n_ported = 0
    for name in web_names:
        ns = by_web.get(name, [])
        n_ported += bool(ns)
        if not ns:
            rows.append((name, None))
        for s in ns:
            rows.append((name, s))
    for s in states:
        if not s.get("web") or s["web"] not in web_names:
            rows.append((s.get("web") or "", s))

    gold = [r.get("golden") for rs in results.values() for r in rs.values()]
    n_pass, n_fail, n_unap = gold.count("pass"), sum(1 for g in gold if g in ("fail", "no render")), gold.count("unapproved")
    n_err = sum(1 for rs in results.values() for r in rs.values() if r.get("render") == "error")
    budget_bad = sum(1 for rs in results.values() for r in rs.values() if r.get("budget") and not r["budget"]["ok"])
    budget_missing = sum(1 for rs in results.values() for r in rs.values() if r.get("budget", {}).get("missing"))

    h = [f"<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'>"
         f"<title>Rare Render parity</title><style>{CSS}</style></head><body><main>"]
    h.append("<h1>Rare Render parity</h1>")
    run = os.environ.get("GITHUB_RUN_ID")
    sha = os.environ.get("GITHUB_SHA", "")[:9]
    h.append(f"<div class=meta>{e(time.strftime('%Y-%m-%d %H:%M UTC', time.gmtime()))}"
             + (f" · commit {e(sha)}" if sha else "") + (f" · run {e(run)}" if run else "")
             + f" · {e(env.get('os', ''))} · {e(env.get('mesa', ''))} · fixtures pinned at {e(idx.get('fixedIso', ''))}"
             + f" · tolerance: more than {imgdiff.MAX_FRAC * 100:.1f} % of pixels off by more than {imgdiff.TOL} levels fails</div>")
    h.append("<div class=cards>"
             f"<div class=card><b>{n_ported} / {len(web_names)}</b><span>web states with a native state</span></div>"
             f"<div class=card><b>{len(states)}</b><span>native states</span></div>"
             f"<div class=card><b>{n_pass}</b><span>golden images pass</span></div>"
             f"<div class=card><b>{n_fail}</b><span>golden images fail</span></div>"
             f"<div class=card><b>{n_unap}</b><span>unapproved</span></div>"
             f"<div class=card><b>{n_err}</b><span>render errors</span></div>"
             f"<div class=card><b>{budget_bad}</b><span>budget gates failing</span></div>"
             + (f"<div class=card><b>{budget_missing}</b><span>budget runs without a frame log</span></div>" if budget_missing else "") +
             "</div>")
    h.append("<h2>Every screen state</h2><p><input id=q placeholder='Filter states'></p>")
    h.append("<div class=wrap><table id=parity><thead><tr><th>Web state</th><th>Native state</th><th>Phase</th><th>Fixture</th><th>Web shot</th>"
             + "".join(f"<th>Golden {r}</th>" for r in resolutions)
             + "<th>vs web 1920x1200</th><th>vs web 1280x800</th><th>Budget</th></tr></thead><tbody>")
    for web, s in rows:
        web_cell = e(web) if web else '<span class="dim">native only</span>'
        has_web = bool(web) and (find_web(web_dir, web, "1920x1200") or find_web(web_dir, web, "1280x800"))
        shot = badge("shot", "pass") if has_web else ('<span class="dim">not shot</span>' if web else '<span class="dim">-</span>')
        if s is None:
            h.append(f"<tr><td>{web_cell}</td><td>{badge('not ported', 'none')}</td><td></td><td class=dim>{e(idx['shots'].get(web, ''))}</td><td>{shot}</td>"
                     + "<td></td>" * len(resolutions) + "<td></td><td></td><td></td></tr>")
            continue
        rs = results.get(s["id"], {})
        cells = "".join(f"<td>{golden_cell(rs.get(r)) if r in (s.get('res') or resolutions) else '<span class=dim>n/a</span>'}</td>" for r in resolutions)
        pv = []
        for r in ("1920x1200", "1280x800"):
            p = parity.get((s["id"], r))
            pv.append(f"<td><a href='{e(p[1])}'>{p[0]} % differ</a></td>" if p and p[0] is not None else "<td class=dim>-</td>")
        b = next((x["budget"] for x in rs.values() if x.get("budget")), None)
        bcell = (badge("no log", "warn") if b and b.get("missing") else badge("pass", "pass") if b and b["ok"]
                 else badge("fail", "fail") if b else '<span class="dim">-</span>')
        h.append(f"<tr><td>{web_cell}</td><td><a href='#{e(s['id'])}'>{e(s['id'])}</a></td><td>{e(s.get('phase', ''))}</td>"
                 f"<td class=dim>{e(rs and next(iter(rs.values())).get('fixture', '') or '')}</td><td>{shot}</td>{cells}{''.join(pv)}<td>{bcell}</td></tr>")
    h.append("</tbody></table></div>")

    # ---- per native state
    h.append("<h2>Native states</h2>")
    for s in states:
        rs = results.get(s["id"], {})
        h.append(f"<h3 id='{e(s['id'])}'>{e(s['id'])} <span class=dim>{e(s.get('title', ''))} · {e(s['program'])}"
                 + (f" · RR_SCREEN={e(s['screen'])}" if s.get("screen") else "") + (f" · RR_SCRIPT={e(s['script'])}" if s.get("script") else "")
                 + f" · {e(s['file'])}</span></h3><div class=imgs>")
        for r in resolutions:
            png = os.path.join(out, r, f"{s['id']}.png")
            x = rs.get(r, {})
            if os.path.exists(png):
                th = os.path.join(out, "thumbs", f"{s['id']}-{r}.jpg")
                imgdiff.thumb(png, th)
                h.append(f"<figure><a href='{e(os.path.relpath(png, out))}'><img loading=lazy src='{e(os.path.relpath(th, out))}'></a><figcaption>{r} · {golden_cell(x)}</figcaption></figure>")
                dp = os.path.join(out, "diff", r, f"{s['id']}.png")
                if x.get("golden") == "fail" and os.path.exists(dp):
                    ap = os.path.join(out, "approved", r, f"{s['id']}.png")
                    tha = os.path.join(out, "thumbs", f"{s['id']}-{r}-approved.jpg")
                    thd = os.path.join(out, "thumbs", f"{s['id']}-{r}-diff.jpg")
                    imgdiff.thumb(ap, tha)
                    imgdiff.thumb(dp, thd)
                    h.append(f"<figure><a href='{e(os.path.relpath(ap, out))}'><img loading=lazy src='{e(os.path.relpath(tha, out))}'></a><figcaption>{r} approved</figcaption></figure>")
                    h.append(f"<figure><a href='{e(os.path.relpath(dp, out))}'><img loading=lazy src='{e(os.path.relpath(thd, out))}'></a><figcaption>{r} diff: {e(x['golden_detail'].get('reason', ''))}</figcaption></figure>")
            elif x.get("render") == "error":
                h.append(f"<figure><figcaption>{r}: render error (exit {e(x.get('exit'))})</figcaption><pre>{e(x.get('render_error', ''))}</pre></figure>")
        h.append("</div>")
        for r in ("1920x1200", "1280x800"):
            p = parity.get((s["id"], r))
            if p and p[0] is not None:
                h.append(f"<div class=sbs><p class=dim>Against the web shot {e(s['web'])}-{r}.png (native with RR_TEXT_LOOK=chrome): native, web, difference</p><img loading=lazy src='{e(p[1])}'></div>")
        b = next((x["budget"] for x in rs.values() if x.get("budget")), None)
        if b:
            h.append("<pre>" + e("\n".join(f"{'ok  ' if g['ok'] else ('FAIL' if g['gating'] else 'warn')} {k}: {g['detail']}" for k, g in b["gates"].items())
                                   + f"\ncpu ms {b.get('cpu_ms')}  interval ms {b.get('interval_ms')}  windows {b.get('windows')} by {b.get('window_method')}") + "</pre>")
    if env.get("vulkan"):
        h.append(f"<h2>Renderer</h2><pre>{e(env['vulkan'])}</pre>")
    h.append(f"</main><script>{JS}</script></body></html>")
    open(os.path.join(out, "index.html"), "w").write("\n".join(h))
    # The dashboard alone, for a quick look and for whoever links it.
    json.dump({"states": len(states), "web_states": len(web_names), "ported": n_ported, "golden_pass": n_pass, "golden_fail": n_fail,
               "unapproved": n_unap, "render_errors": n_err, "budget_failing": budget_bad}, open(os.path.join(out, "summary.json"), "w"), indent=1)
