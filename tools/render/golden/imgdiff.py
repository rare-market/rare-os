"""Pixel comparison for golden images: a state fails when more than 0.1 % of its pixels differ by
more than 2 levels in any channel from the approved PNG (the plan's rule). Also draws the diff
image and the native/web side-by-side panels for the reports."""
from __future__ import annotations

import numpy as np
from PIL import Image

TOL = 2          # levels per channel a pixel may move and still count as equal
MAX_FRAC = 0.001  # share of pixels allowed past TOL


def load(path):
    return np.asarray(Image.open(path).convert("RGB"))


def compare(new_path, approved_path, diff_path=None, tol=TOL, max_frac=MAX_FRAC) -> dict:
    a, b = load(new_path), load(approved_path)
    if a.shape != b.shape:
        return {"status": "fail", "reason": f"size {a.shape[1]}x{a.shape[0]}, approved {b.shape[1]}x{b.shape[0]}", "frac": 1.0}
    d = np.abs(a.astype(np.int16) - b.astype(np.int16)).max(axis=2)
    bad = d > tol
    n = int(bad.sum())
    frac = n / bad.size
    out = {"status": "pass" if frac <= max_frac else "fail", "frac": round(frac, 6), "pixels": n, "max_delta": int(d.max())}
    if n:
        ys, xs = np.nonzero(bad)
        out["bbox"] = [int(xs.min()), int(ys.min()), int(xs.max()) + 1, int(ys.max()) + 1]
        if diff_path:
            heat(a, bad, diff_path)
    if out["status"] == "fail":
        out["reason"] = f"{frac * 100:.3f} % of pixels off by more than {tol} (limit {max_frac * 100:.1f} %), max delta {out['max_delta']}"
    return out


def heat(a, bad, path):
    """The new image dimmed to a third, failing pixels in red."""
    g = (a.astype(np.float32).mean(axis=2) * 0.33).astype(np.uint8)
    rgb = np.stack([g, g, g], axis=2)
    rgb[bad] = (255, 40, 90)
    Image.fromarray(rgb).save(path, optimize=True)


def side_by_side(native_path, web_path, out_path, label_native="native", label_web="web"):
    """native | web | difference, at the native image's size (web scaled if it differs).
    Returns the share of pixels that differ by more than 24 levels (a rough parity number)."""
    from PIL import ImageDraw

    n = Image.open(native_path).convert("RGB")
    w = Image.open(web_path).convert("RGB")
    if w.size != n.size:
        w = w.resize(n.size, Image.LANCZOS)
    na, wa = np.asarray(n).astype(np.int16), np.asarray(w).astype(np.int16)
    d = np.abs(na - wa).max(axis=2)
    frac = float((d > 24).mean())
    dim = np.clip(d * 3, 0, 255).astype(np.uint8)
    diff = Image.fromarray(np.stack([dim, dim // 3, dim // 2], axis=2))
    W, H = n.size
    s = 640 / W
    tw, th = int(W * s), int(H * s)
    out = Image.new("RGB", (tw * 3 + 16, th + 28), (12, 12, 14))
    dr = ImageDraw.Draw(out)
    for i, (im, lab) in enumerate(((n, label_native), (w, label_web), (diff, f"difference ({frac * 100:.1f} % past 24 levels)"))):
        out.paste(im.resize((tw, th), Image.LANCZOS), (i * (tw + 8), 28))
        dr.text((i * (tw + 8) + 4, 8), lab, fill=(220, 220, 225))
    out.save(out_path, quality=85) if out_path.endswith(".jpg") else out.save(out_path)
    return frac


def thumb(path, out_path, width=480):
    im = Image.open(path).convert("RGB")
    h = int(im.height * width / im.width)
    im.resize((width, h), Image.LANCZOS).save(out_path, quality=82)
