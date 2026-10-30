#!/usr/bin/env python3
"""Summarise a pulled run directory (rundown or game) into summary.json and print it.
usage: summarize.py <kind: rundown|game> <run-dir> <build> <scenario> [note]
"""
import csv, glob, json, os, sys

def f(x):
    try: return float(x)
    except (TypeError, ValueError): return None
def col(rows, k): return [v for v in (f(r.get(k)) for r in rows) if v is not None]
def mean(v): return sum(v) / len(v) if v else None

def regress(xs, ys):
    n = len(xs)
    if n < 3: return None
    mx, my = sum(xs) / n, sum(ys) / n
    den = sum((x - mx) ** 2 for x in xs)
    return sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / den if den else None

def lows(ft_ms):
    """avg FPS, 1% low, 0.1% low from frame times in ms. A low = 1000 / mean of the slowest 1% (0.1%) of frames."""
    ft = [x for x in ft_ms if x > 0]
    if len(ft) < 100: return None
    out = {"fps_avg": len(ft) * 1000.0 / sum(ft)}
    s = sorted(ft, reverse=True)
    for key, frac in (("fps_1pct_low", 0.01), ("fps_01pct_low", 0.001)):
        k = max(1, int(len(s) * frac)); out[key] = 1000.0 / (sum(s[:k]) / k)
    out["frames"] = len(ft)
    return out

def mango_frametimes(path):
    """MangoHud log csv: system-info header (2 lines), then a header containing fps,frametime, then rows. frametime in ms."""
    lines = open(path, errors="replace").read().splitlines()
    for i, l in enumerate(lines):
        cols = [c.strip().lower() for c in l.split(",")]
        if "fps" in cols and "frametime" in cols:
            j = cols.index("frametime")
            out = []
            for r in lines[i + 1:]:
                p = r.split(",")
                if len(p) > j and f(p[j]) is not None: out.append(f(p[j]))
            return out
    return []

def read_samples(d):
    return list(csv.DictReader(open(os.path.join(d, "samples.csv"))))

def rundown(d):
    rows = read_samples(d); meta = json.load(open(os.path.join(d, "meta.json")))
    dis = [r for r in rows if r.get("status") == "Discharging" and f(r["energy_uwh"]) and f(r["energy_uwh"]) > 0]
    m = {"samples": len(rows), "discharging_samples": len(dis)}
    full_wh = (f(meta.get("bat_energy_full_uwh")) or 0) / 1e6
    m["battery_full_wh"] = full_wh
    br, mx = col(rows, "brightness"), f(meta.get("max_brightness"))
    if br and mx: m["brightness_pct"] = mean(br) / mx * 100
    m["wifi"] = sorted({r["wifi"].split(":")[0] for r in rows})
    notes = []
    if len(dis) >= 3:
        t = [f(r["t"]) for r in dis]; e = [f(r["energy_uwh"]) / 1e6 for r in dis]; p = [f(r["power_uw"]) / 1e6 for r in dis]
        m["duration_h"] = (t[-1] - t[0]) / 3600
        m["cap_start"], m["cap_end"] = f(dis[0]["cap_pct"]), f(dis[-1]["cap_pct"])
        m["avg_batt_w"] = mean(p)
        slope = regress([x / 3600 for x in t], e)           # Wh per hour (negative)
        if slope and slope < 0:
            m["slope_w"] = -slope
            m["hours_to_empty_from_end"] = e[-1] / -slope
            m["hours_full_slope"] = full_wh / -slope
        if m["avg_batt_w"]: m["hours_full_power"] = full_wh / m["avg_batt_w"]
        if m["duration_h"] < 0.25: notes.append("run shorter than 15 min: extrapolation is rough; use a longer run for publishable numbers")
        if len(dis) < len(rows) * 0.95: notes.append("%d of %d samples were not Discharging (charger on?): run is not clean" % (len(rows) - len(dis), len(rows)))
    else:
        notes.append("no Discharging samples (charger connected?): no rundown numbers. Unplug the charger and rerun.")
    m["clean"] = len(dis) >= 3 and len(dis) >= len(rows) * 0.95
    return m, notes

def game(d):
    rows = read_samples(d); notes = []
    m = {"samples": len(rows)}
    for k, name in (("apu_w", "apu_w"), ("tctl_c", "tctl_c"), ("edge_c", "edge_c"), ("fan_rpm", "fan_rpm")):
        v = col(rows, k)
        if v: m[name] = mean(v); m[name + "_max"] = max(v)
    ft, src = [], None
    for p in sorted(glob.glob(os.path.join(d, "mango", "*.csv"))):
        if p.endswith("_summary.csv"): continue
        ft += mango_frametimes(p); src = "mangohud-frametime"
    if ft:
        r = lows(ft)
        if r: m.update(r)
        else: notes.append("mangohud log has fewer than 100 frames")
    if "fps_avg" not in m:
        pf = os.path.join(d, "fps.csv")
        fp = col(list(csv.DictReader(open(pf))), "fps") if os.path.exists(pf) else []
        if fp:
            src = "gamescope-stats-pipe-1hz"
            m["fps_avg"] = mean(fp)
            s = sorted(fp); k = max(1, int(len(s) * 0.01))
            m["fps_1pct_low"] = mean(s[:k]); m["fps_01pct_low"] = s[0]
            m["frames"] = len(fp)
            notes.append("no frame times: FPS values are gamescope's once-a-second counter, lows are lows of those 1 s samples (an approximation, not comparable to frame-time lows)")
        else:
            notes.append("no FPS data: nothing was drawing, or the stats pipe was empty")
    m["fps_source"] = src
    if m.get("fps_avg") and m.get("apu_w"): m["fps_per_w"] = m["fps_avg"] / m["apu_w"]
    if "fan_rpm" not in m: notes.append("fan RPM not exposed by this kernel (no hwmon fan*_input)")
    return m, notes

if __name__ == "__main__":
    kind, d, build, scen = sys.argv[1:5]; note = sys.argv[5] if len(sys.argv) > 5 else ""
    m, notes = (rundown if kind == "rundown" else game)(d)
    meta = json.load(open(os.path.join(d, "meta.json")))
    out = {"kind": kind, "build": build, "scenario": scen, "note": note, "start": meta["start"], "metrics": m, "notes": notes,
           "conditions": {k: meta.get(k) for k in ("host_kernel", "brightness", "max_brightness", "wifi", "ac_online", "bat_status", "bat_capacity", "bat_cycle_count", "bat_temp_dc", "acpitz_c", "epp", "platform_profile", "governor")}}
    json.dump(out, open(os.path.join(d, "summary.json"), "w"), indent=1)
    print("%s  build=%s  scenario=%s %s" % (kind, build, scen, note))
    for k, v in m.items(): print("  %-24s %s" % (k, ("%.3f" % v) if isinstance(v, float) else v))
    for n in notes: print("  NOTE: " + n)
    print("  conditions: " + ", ".join("%s=%s" % kv for kv in out["conditions"].items() if kv[1] not in (None, "", [])))
