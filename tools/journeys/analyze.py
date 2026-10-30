#!/usr/bin/env python3
"""Analyse a journey run, or scan any video for black/blank/flash frames.
  analyze.py RUN_DIR               -> RUN_DIR/results.json, report.md, report.html, clips.json
  analyze.py --scan VIDEO [OUT]    -> decode (ffmpeg) and list every black/blank/flash/stale frame
  analyze.py --scan-raw DIR [LABEL] [OUT]  -> the same for a tools/journeys/flash capture (needs numpy)
  analyze.py --lint [JOURNEYS_DIR] -> load and check every journey file
"""
import json
import os
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from jlib import analyze as A  # noqa: E402
from jlib import frames as F  # noqa: E402
from jlib import report as R  # noqa: E402
from jlib import spec  # noqa: E402


def main(argv):
    if not argv or argv[0] in ("-h", "--help"):
        print(__doc__)
        return 0
    if argv[0] == "--lint":
        root = argv[1] if len(argv) > 1 else os.path.join(HERE, "journeys")
        js = spec.load_all(root)
        for j in js:
            print(f"{j['name']:<22} {len(j['steps']):>3} steps  needs={','.join(j['needs']) or '-':<22} {j['title']}")
        print(f"{len(js)} journeys OK")
        return 0
    if argv[0] == "--scan-raw":
        # A tools/journeys/flash capture: --scan-raw DIR [LABEL] [OUT]
        d = argv[1]
        label = argv[2] if len(argv) > 2 else os.path.basename(os.path.normpath(d))
        out = argv[3] if len(argv) > 3 else tempfile.mkdtemp(prefix="scan-")
        n = F.ingest_raw(d, label, out)
        res = A.scan(out)
        print(json.dumps({k: v for k, v in res.items() if k != "faults"}))
        for f in res["faults"]:
            area = f" {round(f['area'] * 100, 2)}% of the screen" if f.get("area") is not None else ""
            print(f"  {f['kind']:<6} frame {f['first']:>5} at {f['at_s']:>8.3f} s  {f['frames']} frame(s) {f['ms']} ms{area}{' (minor)' if f.get('minor') else ''}")
        print(f"{n} frames from {label}.raw into {out}")
        return 0
    if argv[0] == "--scan":
        out = argv[2] if len(argv) > 2 else tempfile.mkdtemp(prefix="scan-")
        n = F.ingest_video(argv[1], out)
        res = A.scan(out)
        print(json.dumps({k: v for k, v in res.items() if k != "faults"}))
        for f in res["faults"]:
            print(f"  {f['kind']:<6} at {f['at_s']:>8.3f} s  {f['frames']} frame(s) {f['ms']} ms")
        print(f"{n} frames decoded into {out}")
        return 0
    run = argv[0]
    res = A.analyze(run)
    R.write(run, res)
    s, ss = res["summary"], res["steps_summary"]
    print(f"journeys: {s['pass']} pass, {s['warn']} warn, {s['fail']} fail, {s['skip']} skipped; "
          f"steps: {ss['pass']} pass, {ss['warn']} warn, {ss['fail']} fail, {ss['skip']} skipped")
    for j in res["journeys"]:
        bad = [st for st in j["steps"] if st["verdict"] == "fail"]
        line = f"  {j['verdict']:<4} {j['name']}"
        if j["status"] not in ("completed",):
            line += f" ({j['status']}: {j['reason']})"
        if bad:
            line += " | " + "; ".join(f"{st['idx'] + 1}. {st['name']}: {', '.join(st['reasons'])}" for st in bad[:3])
        print(line)
    print(f"report: {os.path.join(run, 'report.html')}")
    return 1 if s["fail"] else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
