#!/usr/bin/env python3
"""Cuts short mp4 clips out of a run's video.mkv. Runs on the device (it has ffmpeg) or anywhere.
  clips.py RUN_DIR [clips.json]   -> RUN_DIR/clips/<journey>__<step>.mp4
clips.json: [{"name", "start_ns", "end_ns"}] in CLOCK_MONOTONIC ns (written by analyze.py).
"""
import json
import os
import subprocess
import sys


def main():
    run = sys.argv[1]
    windows = json.load(open(sys.argv[2] if len(sys.argv) > 2 else os.path.join(run, "clips.json")))
    meta = json.load(open(os.path.join(run, "meta.json")))
    cap = meta.get("capture") or {}
    video = os.path.join(run, "video.mkv")
    t0 = cap.get("video_t0_ns")
    if not os.path.exists(video) or not t0:
        print("no video in this run", file=sys.stderr)
        return 1
    ff = os.environ.get("FFMPEG", "ffmpeg")
    out = os.path.join(run, "clips")
    os.makedirs(out, exist_ok=True)
    n = 0
    for w in windows:
        ss = max(0.0, (w["start_ns"] - t0) / 1e9)
        dur = max(0.5, min(12.0, (w["end_ns"] - w["start_ns"]) / 1e9))
        dst = os.path.join(out, w["name"])
        # Re-encode so the clip starts exactly at the window, half size, small file.
        r = subprocess.run([ff, "-hide_banner", "-loglevel", "error", "-y", "-ss", "%.3f" % ss, "-i", video, "-t", "%.3f" % dur,
                            "-an", "-vf", "scale=iw/2:-2", "-c:v", "libx264", "-preset", "veryfast", "-crf", "26",
                            "-pix_fmt", "yuv420p", "-movflags", "+faststart", dst], capture_output=True, text=True)
        if r.returncode == 0:
            n += 1
        else:
            print(f"{w['name']}: {r.stderr.strip()[:300]}", file=sys.stderr)
    print(f"{n} clips in {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
