import csv, glob, json, os, re, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, "..", ".."))
# metric -> (label, better: +1 higher is better / -1 lower is better)
METRICS = {
 "rundown": {"hours_full_power": ("Hours, full battery (avg power)", 1), "hours_full_slope": ("Hours, full battery (curve slope)", 1), "avg_batt_w": ("Battery draw W", -1)},
 "game": {"fps_avg": ("Avg FPS", 1), "fps_1pct_low": ("1% low FPS", 1), "fps_01pct_low": ("0.1% low FPS", 1), "apu_w": ("APU W", -1),
          "fps_per_w": ("FPS per W", 1), "tctl_c": ("Tctl C", -1), "edge_c": ("Edge C", -1), "fan_rpm": ("Fan RPM", -1)},
 "timing": {"boot_to_first_frame_s": ("Boot to first launcher frame s", -1), "boot_to_userspace_s": ("Boot to userspace s", -1),
            "kernel_resume_s": ("Resume, kernel s", -1), "wake_to_userspace_ready_s": ("Resume, to userspace ready s", -1),
            "play_to_first_focus_change_s": ("Play to first focus change s", -1), "play_to_game_focus_s": ("Play to game focus s", -1),
            "drain_pct_per_h": ("Sleep drain %/h", -1), "drain_mw": ("Sleep drain mW", -1)},
}

def load(runs, label):
    cells = {}; skipped = 0
    for p in glob.glob(os.path.join(runs, "**", "summary.json"), recursive=True):
        s = json.load(open(p))
        if s.get("build") != label: continue
        kind, m = s["kind"], s["metrics"]
        if kind == "rundown" and not m.get("clean"): skipped += 1; continue
        for k, (name, d) in METRICS.get(kind, {}).items():
            v = m.get(k)
            if isinstance(v, (int, float)):
                scen = s["scenario"] + (" [%s]" % m["fps_source"] if kind == "game" and k.startswith("fps") and m.get("fps_source") else "")
                cells.setdefault((kind, scen, k), []).append(float(v))
    return cells, skipped

def fmt(v): return "%.2f" % v if abs(v) < 100 else "%.1f" % v

def main(a):
    if len(a) < 2: print(open(os.path.join(HERE, "report")).read().split("exec")[0]); return 2
    base, other = a[0], a[1]
    runs = a[a.index("--runs") + 1] if "--runs" in a else os.path.join(ROOT, "bench", "runs")
    out = a[a.index("--out") + 1] if "--out" in a else os.path.join(ROOT, "bench", "reports", "%s-vs-%s" % (re.sub(r"\W", "_", base), re.sub(r"\W", "_", other)))
    cb, sb = load(runs, base); co, so = load(runs, other)
    keys = sorted(set(cb) | set(co), key=lambda k: (k[0], k[1], list(METRICS[k[0]]).index(k[2])))
    rows = []
    for k in keys:
        kind, scen, met = k; name, better = METRICS[kind][met]
        x, y = cb.get(k, []), co.get(k, [])
        r = {"kind": kind, "scenario": scen, "metric": name, "baseline": base, "other": other, "n_base": len(x), "n_other": len(y),
             "mean_base": sum(x) / len(x) if x else None, "mean_other": sum(y) / len(y) if y else None,
             "min_base": min(x) if x else None, "max_base": max(x) if x else None, "min_other": min(y) if y else None, "max_other": max(y) if y else None}
        if x and y:
            r["delta"] = r["mean_other"] - r["mean_base"]
            r["delta_pct"] = r["delta"] / r["mean_base"] * 100 if r["mean_base"] else None
            overlap = max(r["min_base"], r["min_other"]) <= min(r["max_base"], r["max_other"])
            r["status"] = "headline" if len(x) >= 2 and len(y) >= 2 else "preliminary"
            if r["status"] == "headline" and overlap: r["verdict"] = "within run-to-run spread"
            elif better * r["delta"] > 0: r["verdict"] = "better"
            elif r["delta"] == 0: r["verdict"] = "same"
            else: r["verdict"] = "worse"
        else:
            r["status"] = "missing side"; r["verdict"] = ""
        rows.append(r)
    os.makedirs(os.path.dirname(out), exist_ok=True)
    fields = ["kind", "scenario", "metric", "baseline", "other", "n_base", "n_other", "mean_base", "mean_other", "min_base", "max_base", "min_other", "max_other", "delta", "delta_pct", "status", "verdict"]
    with open(out + ".csv", "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=fields); w.writeheader()
        for r in rows: w.writerow({k: ("%.4f" % v if isinstance(v, float) else v) for k, v in ((k, r.get(k)) for k in fields) if v is not None})
    def table(rs):
        L = ["| Cell | Metric | %s | %s | Delta | Runs (base/other) | Verdict |" % (base, other), "|---|---|---|---|---|---|---|"]
        for r in rs:
            d = "%+.2f (%+.1f%%)" % (r["delta"], r["delta_pct"] or 0) if "delta" in r else ""
            L.append("| %s/%s | %s | %s | %s | %s | %d/%d | %s |" % (r["kind"], r["scenario"], r["metric"],
                     fmt(r["mean_base"]) if r["mean_base"] is not None else "-", fmt(r["mean_other"]) if r["mean_other"] is not None else "-", d, r["n_base"], r["n_other"], r["verdict"]))
        return L
    md = ["# Rare OS bench: %s vs %s" % (base, other), "",
          "Delta is %s minus %s. A headline needs at least 2 runs on both sides; anything with 1 run is preliminary and must not be quoted as a result." % (other, base), ""]
    h = [r for r in rows if r["status"] == "headline"]; p = [r for r in rows if r["status"] == "preliminary"]; mi = [r for r in rows if r["status"] == "missing side"]
    md += ["## Headline numbers", ""] + (table(h) if h else ["None yet: no cell has 2 or more runs on both sides."]) + [""]
    if p: md += ["## Preliminary (n=1 on at least one side)", ""] + table(p) + [""]
    if mi: md += ["## Only one side measured", ""] + table(mi) + [""]
    if sb or so: md += ["Excluded %d unclean rundown(s) (charger on or too few discharge samples)." % (sb + so), ""]
    open(out + ".md", "w").write("\n".join(md))
    print("\n".join(md)); print("wrote %s.md and %s.csv" % (out, out)); return 0

if __name__ == "__main__": sys.exit(main(sys.argv[1:]))
