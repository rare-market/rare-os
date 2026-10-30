"""Frame metrics on small grayscale thumbnails. Pure Python (the device has no numpy).

Every captured frame is scaled to W x H gray (160 x 100 by default) and described by:
  mean  luma 0..255          std   spread (uniform frames have ~0)
  dark  fraction of pixels < 24
  diff  mean |delta| vs the previous frame, 0..255
  chg   fraction of pixels whose |delta| > 16
The same functions run live on the device (pacing: "has the screen settled") and in the analysis.
"""
from __future__ import annotations

import operator
import os
import re
import struct
import subprocess
import threading
import zlib

W, H = 160, 100
_SQ = [i * i for i in range(256)]
_DARK = bytes(1 if i < 24 else 0 for i in range(256))

# Classification thresholds (analysis and selftest share them).
BLACK_DARK = 0.985   # nearly every pixel dark ...
BLACK_MEAN = 16.0    # ... and a dark mean
BLANK_STD = 3.0      # one flat colour (white flash, solid fill)
CHG_PIXEL = 16       # a pixel counts as changed past this delta
CHG_MIN = 0.002      # a frame counts as changed when this fraction of pixels moved (32 of 16000) ...
DIFF_MIN = 0.6       # ... or the mean delta passes this


def stats(px: bytes) -> tuple:
    n = len(px)
    s = sum(px)
    mean = s / n
    var = max(0.0, sum(map(_SQ.__getitem__, px)) / n - mean * mean)
    dark = sum(px.translate(_DARK)) / n
    return mean, var ** 0.5, dark


def delta(a: bytes, b: bytes) -> tuple:
    """(mean abs diff, changed fraction) between two thumbnails."""
    if not a or not b:
        return 0.0, 0.0
    d = list(map(operator.sub, a, b))
    n = len(d)
    diff = sum(map(abs, d)) / n
    chg = sum(1 for x in d if x > CHG_PIXEL or x < -CHG_PIXEL) / n
    return diff, chg


def chg_frac(a: bytes, b: bytes, pix: int = CHG_PIXEL) -> float:
    """Share of pixels whose value moved more than pix."""
    if not a or not b:
        return 0.0
    return sum(1 for x in map(operator.sub, a, b) if x > pix or x < -pix) / len(a)


def mad(a: bytes, b: bytes) -> float:
    return sum(map(abs, map(operator.sub, a, b))) / len(a)


def is_black(mean, dark) -> bool:
    return dark >= BLACK_DARK and mean < BLACK_MEAN


def is_blank(mean, std, dark) -> bool:
    return std < BLANK_STD and not is_black(mean, dark)


def is_changed(diff, chg) -> bool:
    return chg >= CHG_MIN or diff >= DIFF_MIN


class Metric:
    """Live per-frame metrics, fed one thumbnail at a time."""

    def __init__(self):
        self.prev = None

    def feed(self, px: bytes) -> tuple:
        mean, std, dark = stats(px)
        diff, chg = delta(px, self.prev) if self.prev is not None else (0.0, 0.0)
        self.prev = px
        return mean, std, dark, diff, chg


FRAMES_HEADER = "idx,t_ns,arrival_ns,mean,std,dark,diff,chg\n"


def frame_row(idx, t_ns, arrival_ns, m) -> str:
    return "%d,%d,%d,%.2f,%.2f,%.4f,%.3f,%.4f\n" % (idx, t_ns, arrival_ns, m[0], m[1], m[2], m[3], m[4])


def read_frames(path: str) -> list:
    out = []
    with open(path) as f:
        f.readline()
        for line in f:
            p = line.rstrip("\n").split(",")
            if len(p) < 8:
                continue
            out.append((int(p[0]), int(p[1]), int(p[2]), float(p[3]), float(p[4]), float(p[5]), float(p[6]), float(p[7])))
    return out


class Thumbs:
    """Random access to thumbs.bin (W*H bytes per frame, in frames.csv order)."""

    def __init__(self, path: str, w: int = W, h: int = H):
        self.size = w * h
        self.w, self.h = w, h
        self.f = open(path, "rb") if path and os.path.exists(path) else None
        self.n = (os.path.getsize(path) // self.size) if self.f else 0

    def __getitem__(self, i: int) -> bytes:
        if not self.f or i < 0 or i >= self.n:
            return b""
        self.f.seek(i * self.size)
        return self.f.read(self.size)


# ---- video files (selftest, existing captures, re-analysis): needs an ffmpeg binary ----

def find_ffmpeg() -> str | None:
    for c in (os.environ.get("FFMPEG"), "ffmpeg", "/usr/bin/ffmpeg"):
        if not c:
            continue
        try:
            subprocess.run([c, "-version"], capture_output=True, check=True)
            return c
        except Exception:
            pass
    try:
        import imageio_ffmpeg  # type: ignore

        return imageio_ffmpeg.get_ffmpeg_exe()
    except Exception:
        return None


def ingest_video(path: str, out_dir: str, ffmpeg: str | None = None, w: int = W, h: int = H, t0_ns: int = 0) -> int:
    """Decodes every frame of a video to thumbs.bin + frames.csv (t_ns from the stream's pts)."""
    ffmpeg = ffmpeg or find_ffmpeg()
    if not ffmpeg:
        raise RuntimeError("no ffmpeg (set FFMPEG or pip install imageio-ffmpeg)")
    os.makedirs(out_dir, exist_ok=True)
    cmd = [ffmpeg, "-hide_banner", "-nostdin", "-i", path, "-an", "-fps_mode", "passthrough",
           "-vf", f"scale={w}:{h}:flags=area,format=gray,showinfo", "-f", "rawvideo", "-pix_fmt", "gray", "-"]
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    pts = []

    def errs():
        rx = re.compile(rb"pts_time:\s*(-?[0-9.]+)")
        for line in p.stderr:
            m = rx.search(line)
            if m and b"Parsed_showinfo" in line:
                pts.append(float(m.group(1)))

    th = threading.Thread(target=errs, daemon=True)
    th.start()
    size = w * h
    raw = []
    while True:
        b = p.stdout.read(size)
        if len(b) < size:
            break
        raw.append(b)
    p.wait()
    th.join(5)
    if not raw:
        raise RuntimeError(f"ffmpeg decoded no frames from {path}")
    m = Metric()
    with open(os.path.join(out_dir, "thumbs.bin"), "wb") as tb, open(os.path.join(out_dir, "frames.csv"), "w") as fc:
        fc.write(FRAMES_HEADER)
        for i, b in enumerate(raw):
            t = t0_ns + int(round((pts[i] if i < len(pts) else (pts[-1] if pts else 0) + (i - len(pts) + 1) / 60) * 1e9))
            tb.write(b)
            fc.write(frame_row(i, t, t, m.feed(b)))
    return len(raw)


def ingest_raw(src_dir: str, label: str, out_dir: str, sw: int = 640, sh: int = 400, w: int = W, h: int = H) -> int:
    """tools/journeys/flash captures (<label>.raw rgb24 sw x sh, <label>.pts seconds) to thumbs.
    Needs numpy (an analysis machine, not the device)."""
    import numpy as np  # type: ignore

    raw = np.memmap(os.path.join(src_dir, f"{label}.raw"), np.uint8, "r")
    n = raw.size // (sw * sh * 3)
    pts = [float(x) for x in open(os.path.join(src_dir, f"{label}.pts")).read().split()][:n]
    n = min(n, len(pts))
    fx, fy = sw // w, sh // h
    os.makedirs(out_dir, exist_ok=True)
    m = Metric()
    with open(os.path.join(out_dir, "thumbs.bin"), "wb") as tb, open(os.path.join(out_dir, "frames.csv"), "w") as fc:
        fc.write(FRAMES_HEADER)
        for i in range(n):
            fr = raw[i * sw * sh * 3:(i + 1) * sw * sh * 3].reshape(sh, sw, 3).astype(np.float32)
            luma = fr @ np.array([0.299, 0.587, 0.114], np.float32)
            small = luma[:h * fy, :w * fx].reshape(h, fy, w, fx).mean(axis=(1, 3))
            px = np.clip(small + 0.5, 0, 255).astype(np.uint8).tobytes()
            t = int(round(pts[i] * 1e9))
            tb.write(px)
            fc.write(frame_row(i, t, t, m.feed(px)))
    return n


def encode_video(frames: list, fps_or_times, path: str, ffmpeg: str | None = None, w: int = W, h: int = H):
    """Writes gray frames to a video. fps_or_times: an fps number, or per-frame times in seconds (VFR)."""
    ffmpeg = ffmpeg or find_ffmpeg()
    if not ffmpeg:
        raise RuntimeError("no ffmpeg")
    d = os.path.dirname(os.path.abspath(path))
    if isinstance(fps_or_times, (int, float)):
        cmd = [ffmpeg, "-hide_banner", "-loglevel", "error", "-y", "-f", "rawvideo", "-pix_fmt", "gray", "-s", f"{w}x{h}",
               "-r", str(fps_or_times), "-i", "-", "-c:v", "libx264", "-qp", "0", "-pix_fmt", "yuv444p", path]
        subprocess.run(cmd, input=b"".join(frames), check=True)
        return
    # VFR: an ffconcat list of PGM stills with durations.
    tmp = os.path.join(d, ".vfr")
    os.makedirs(tmp, exist_ok=True)
    lines = ["ffconcat version 1.0"]
    for i, fr in enumerate(frames):
        p = os.path.join(tmp, "%06d.pgm" % i)
        with open(p, "wb") as f:
            f.write(b"P5 %d %d 255\n" % (w, h) + fr)
        dur = (fps_or_times[i + 1] - fps_or_times[i]) if i + 1 < len(frames) else 1 / 60
        lines += [f"file '{p}'", "option framerate 1000", f"duration {dur:.6f}"]
    lst = os.path.join(tmp, "list.txt")
    open(lst, "w").write("\n".join(lines) + "\n")
    subprocess.run([ffmpeg, "-hide_banner", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i", lst,
                    "-fps_mode", "vfr", "-enc_time_base", "1/1000", "-c:v", "libx264", "-qp", "0", "-pix_fmt", "yuv444p", path], check=True)
    for f in os.listdir(tmp):
        os.unlink(os.path.join(tmp, f))
    os.rmdir(tmp)


# ---- PNG (filmstrips in the report), no PIL ----

def png_gray(px: bytes, w: int, h: int, scale: int = 1) -> bytes:
    if scale > 1:
        rows = []
        for y in range(h):
            row = px[y * w:(y + 1) * w]
            big = bytes(v for v in row for _ in range(scale))
            rows.extend([big] * scale)
        w, h = w * scale, h * scale
        px = b"".join(rows)
    raw = b"".join(b"\x00" + px[y * w:(y + 1) * w] for y in range(h))

    def chunk(t, d):
        c = struct.pack(">I", len(d)) + t + d
        return c + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 0, 0, 0, 0)) + \
        chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


def strip(frames: list, w: int = W, h: int = H, gap: int = 2) -> tuple:
    """Frames side by side with a white gap: (pixels, width, height)."""
    if not frames:
        return b"", 0, 0
    tw = len(frames) * w + (len(frames) - 1) * gap
    rows = []
    for y in range(h):
        parts = []
        for i, fr in enumerate(frames):
            parts.append(fr[y * w:(y + 1) * w] if fr else bytes(w))
            if i < len(frames) - 1:
                parts.append(b"\xff" * gap)
        rows.append(b"".join(parts))
    return b"".join(rows), tw, h
