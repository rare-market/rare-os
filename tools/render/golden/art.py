"""Placeholder art for fixture runs: no network, same pixels every time.

Every http(s) image URL in a fixture is replaced with a local file drawn here from the URL alone
(colours from its hash, size from the kind of art the URL names). Steam logos carry the game's
title in Inter Bold so the lockup still reads. Links that are not images stay as they are.
"""
from __future__ import annotations

import hashlib
import json
import os
import re

IMG_EXT = re.compile(r"\.(jpe?g|png|webp|gif|svg|avif)$", re.I)
IMG_HINT = re.compile(r"/(img|image|images|ipfs)/|mypinata|claynosaurz|img\.pawtato", re.I)


def is_image_url(u: str) -> bool:
    if not u.startswith(("http://", "https://")):
        return False
    path = u.split("?", 1)[0]
    return bool(IMG_EXT.search(path) or IMG_HINT.search(u))


def kind_of(u: str):
    """(kind, width, height) for a URL."""
    p = u.split("?", 1)[0].lower()
    if "library_600x900" in p:
        return "cover", 600, 900
    if "library_hero" in p:
        return "hero", 1920, 620
    if p.endswith("/logo.png"):
        return "logo", 640, 240
    if p.endswith("/header.jpg"):
        return "header", 460, 215
    if "community_assets" in p:
        return "icon", 64, 64
    return "square", 512, 512


def _colors(u: str):
    h = hashlib.sha256(u.encode()).digest()
    # Muted, dark-leaning colours: placeholder art must not look like a design choice.
    a = tuple(40 + h[i] % 120 for i in range(3))
    b = tuple(15 + h[3 + i] % 70 for i in range(3))
    return a, b


def draw(u: str, path: str, label: str | None, font_path: str | None):
    from PIL import Image, ImageDraw, ImageFont

    kind, w, h = kind_of(u)
    if kind == "logo":
        im = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        d = ImageDraw.Draw(im)
        text = (label or "Game").upper()
        size = 96
        font = ImageFont.truetype(font_path, size) if font_path else ImageFont.load_default()
        while size > 20 and d.textlength(text, font=font) > w - 20:
            size -= 6
            font = ImageFont.truetype(font_path, size) if font_path else font
        tw = d.textlength(text, font=font)
        d.text(((w - tw) / 2, (h - size) / 2 - size * 0.1), text, font=font, fill=(255, 255, 255, 255))
        im.save(path, "PNG", optimize=True)
        return
    import numpy as np

    a, b = _colors(u)
    k = (np.arange(w)[None, :] / max(w - 1, 1) * 0.35 + np.arange(h)[:, None] / max(h - 1, 1) * 0.65)[..., None]
    rgb = np.array(a, np.float64) + (np.array(b, np.float64) - np.array(a, np.float64)) * k
    im = Image.fromarray(rgb.astype(np.uint8), "RGB")
    # A hairline frame and a centred block so focus scaling and cropping are visible.
    d = ImageDraw.Draw(im)
    d.rectangle([w * 0.3, h * 0.38, w * 0.7, h * 0.62], fill=tuple(min(255, c + 40) for c in a))
    im.save(path, "JPEG" if path.endswith(".jpg") else "PNG", quality=90)


def titles_from(fixture: dict) -> dict:
    """Steam appid -> title, from the fixture's library, for the logos."""
    out = {}
    for g in ((fixture.get("lib") or {}).get("library") or {}).get("games") or []:
        m = re.match(r"steam:(\d+)$", g.get("id", ""))
        if m:
            out[m.group(1)] = g.get("title")
    return out


def localize(obj, art_dir: str, font_path: str | None, titles: dict | None = None):
    """Returns obj with every image URL replaced by a placeholder file in art_dir (drawn once)."""
    os.makedirs(art_dir, exist_ok=True)
    titles = titles or {}

    def one(u: str) -> str:
        if not is_image_url(u):
            return u
        kind, _, _ = kind_of(u)
        ext = ".png" if kind == "logo" else ".jpg"
        name = hashlib.sha256(u.encode()).hexdigest()[:20] + ext
        p = os.path.join(art_dir, name)
        if not os.path.exists(p):
            m = re.search(r"/apps/(\d+)/", u)
            draw(u, p + ".tmp" + ext, titles.get(m.group(1)) if m else None, font_path)
            os.replace(p + ".tmp" + ext, p)
        return os.path.abspath(p)

    def walk(v):
        if isinstance(v, str):
            return one(v)
        if isinstance(v, list):
            return [walk(x) for x in v]
        if isinstance(v, dict):
            return {k: walk(x) for k, x in v.items()}
        return v

    return walk(obj)


if __name__ == "__main__":
    import sys

    fx = json.load(open(sys.argv[1]))
    print(json.dumps(localize(fx, sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else None, titles_from(fx)))[:2000])
