#!/usr/bin/env python3
"""Finds flash frames in a capture from capture.sh (needs numpy and Pillow).

  analyze.py <dir> [label]      dir holds <label>.raw and <label>.pts (label defaults to the dir name)

A flash is a run of 1 or 2 composited frames that differs from the frames on both sides while those
two agree with each other: something vanished (or showed) for one or two frames and came back. That
is how the GPU-mode flash looked on the SuiPlay (every <img> gone for one frame, 2026-10-27).
Prints every flash with its time, size and region, writes <label>-flash-<n>.png (before, flash,
after) next to the capture, and exits 1 when any flash covers more than MIN_FLASH of the screen.
Also prints black frames and, per key in keys.log, how long the screen took to settle.
"""
import os, sys
import numpy as np
from PIL import Image, ImageDraw

W, H = 640, 400
PIX = 24            # luma step that counts as a changed pixel
MIN_FLASH = 0.005   # share of the screen a flash must cover to fail the check


def load(d, label):
    raw = np.fromfile(os.path.join(d, f"{label}.raw"), np.uint8)
    n = raw.size // (W * H * 3)
    frames = raw[: n * W * H * 3].reshape(n, H, W, 3)
    pts = [float(x) for x in open(os.path.join(d, f"{label}.pts")).read().split()][:n]
    return frames, np.array(pts)


def changed(a, b):
    return np.abs(a - b) > PIX


def main():
    d = sys.argv[1]
    label = sys.argv[2] if len(sys.argv) > 2 else os.path.basename(os.path.normpath(d))
    frames, pts = load(d, label)
    n = len(pts)
    luma = frames.astype(np.int16).mean(axis=3)
    t0 = pts[0] if n else 0
    dt = np.diff(pts) * 1000 if n > 1 else np.array([0.0])
    print(f"{label}: {n} frames over {pts[-1] - t0:.2f} s, interval p50 {np.median(dt):.1f} ms, max {dt.max():.0f} ms")

    flashes = []
    for k in (1, 2):
        for i in range(1, n - k):
            a, b = luma[i - 1], luma[i + k]
            side = changed(a, b).mean()
            mids = [changed(luma[j], a).mean() for j in range(i, i + k)] + [changed(luma[j], b).mean() for j in range(i, i + k)]
            m = min(mids)
            if m > 0.001 and side < m * 0.3:
                if any(f[0] <= i < f[0] + f[1] for f in flashes):
                    continue
                mask = changed(luma[i], a)
                ys, xs = np.nonzero(mask)
                before, during = luma[i - 1][mask].mean(), luma[i][mask].mean()
                flashes.append((i, k, m, (xs.min(), ys.min(), xs.max(), ys.max()), before, during))

    fail = False
    for no, (i, k, m, box, before, during) in enumerate(flashes, 1):
        kind = "darker (content vanished)" if during < before - 8 else "brighter" if during > before + 8 else "changed"
        big = m >= MIN_FLASH
        fail |= big
        print(f"{'FLASH' if big else 'flash'} #{no}: frame {i} at {(pts[i] - t0) * 1000:.0f} ms, {k} frame(s) "
              f"{(pts[i + k] - pts[i]) * 1000:.0f} ms, {m * 100:.1f}% of the screen {kind}, region x{box[0]}-{box[2]} y{box[1]}-{box[3]} (of 640x400)")
        strip = Image.new("RGB", (3 * W, H + 16), "white")
        dr = ImageDraw.Draw(strip)
        for c, j in enumerate((i - 1, i, i + k)):
            strip.paste(Image.fromarray(frames[j]), (c * W, 16))
            dr.text((c * W + 4, 2), f"#{j} {(pts[j] - t0) * 1000:.0f} ms", fill="black")
        strip.save(os.path.join(d, f"{label}-flash-{no}.png"))

    black = [i for i in range(n) if (luma[i] < 12).mean() > 0.9]
    if black:
        print(f"black frames: {len(black)} (first at {(pts[black[0]] - t0) * 1000:.0f} ms); expected only when a game or app takes the screen")

    keys = os.path.join(d, "keys.log")
    if os.path.exists(keys) and n:
        # pts are pipeline time; line the first key up with the first busy frame after 1.5 s of preroll.
        lines = [l.split() for l in open(keys) if l.strip()]
        k0 = float(lines[0][0])
        for no, (t, key) in enumerate(lines):
            rel = float(t) - k0 + 1.5
            end = float(lines[no + 1][0]) - k0 + 1.5 if no + 1 < len(lines) else rel + 1.0
            seg = [i for i in range(1, n) if rel <= pts[i] - t0 < end]
            moving = [i for i in seg if changed(luma[i], luma[i - 1]).mean() > 0.002]
            settle = (pts[moving[-1]] - t0 - rel) * 1000 if moving else 0
            print(f"key {key:6} at ~{rel * 1000:.0f} ms: {len(moving)} changing frames, settled after ~{settle:.0f} ms")

    print("RESULT:", "FAIL" if fail else "PASS", f"({sum(f[2] >= MIN_FLASH for f in flashes)} flash(es) over {MIN_FLASH * 100:.1f}% of the screen)")
    sys.exit(1 if fail else 0)


if __name__ == "__main__":
    main()
