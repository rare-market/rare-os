//! The HUD: the volume and brightness bar and toasts, drawn over whatever has the screen (a game,
//! Chrome, an emulator or the launcher) without taking focus or input from it.
//!
//! gamescope (steamcompmgr.cpp, `pick_primary_focus_and_override` and `paint_all`) treats an X
//! window on server 0 that carries `STEAM_OVERLAY` = 1 as Steam's overlay: it never becomes the
//! focus or input window (that needs `STEAM_INPUT_FOCUS`), and it is painted on the overlay plane
//! above the focused app at its `_NET_WM_WINDOW_OPACITY` (opaque when unset). A full root-size
//! window is picked as the overlay window and painted at scale 1. So the HUD is an
//! override-redirect, 32-bit (ARGB) window of the root's size, transparent but for what it shows.
//!
//! It stays mapped and hides with `_NET_WM_WINDOW_OPACITY` = 0, the way Steam hides its overlay.
//! Unmapping does not hide it: gamescope picks the overlay window without looking at the map state
//! (`DetermineAndApplyFocus`, `handle_property_notify`) and keeps painting the last frame it got
//! (`unmap_win` drops no commits), and an unmapped window's opacity changes are never seen. The
//! volume bar stayed on screen for good over games and the launcher that way. Hiding fades the
//! opacity out, clears the pixels, then sets it to 0; the window takes no input (empty input shape).
//!
//! Why not `GAMESCOPE_EXTERNAL_OVERLAY`: mangoapp runs in every session (it is the performance
//! overlay, hidden with `no_display`) and holds that slot, so a second external overlay would
//! show only when gamescope happens to pick it. The overlay plane has no such tenant while Steam
//! is closed; with Steam running, Steam's own overlay window competes for it (device check).
//!
//! Everything is drawn here, not in a webview: it must show while the launcher is busy or behind
//! a game, and in every WebKit mode (render.rs) including the Wayland one, where Rare's windows
//! carry no X atoms. Shapes are antialiased with signed distances, text with fontdue from the
//! system's sans font, and the pixels go out with PutImage.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::time::Duration;

/// How long the bar stays after the last change.
pub const BAR_FOR: Duration = Duration::from_millis(1500);
/// How long a toast stays.
pub const TOAST_FOR: Duration = Duration::from_millis(3000);
/// The fade-out once nothing is left to show.
pub const FADE: Duration = Duration::from_millis(180);
/// Opacity steps during the fade.
const FADE_STEP: Duration = Duration::from_millis(30);
/// Toasts on screen at once; the oldest goes first.
pub const MAX_TOASTS: usize = 3;

/// The level bar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bar {
    /// 0..=100.
    Volume { level: u8, muted: bool },
    /// 0..=100.
    Brightness { level: u8 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub title: String,
    pub body: Option<String>,
    /// A picture on the left (an achievement's badge).
    pub icon: Option<Icon>,
}

/// Straight (not premultiplied) RGBA pixels, `w` x `h`.
#[derive(Clone, Debug, PartialEq)]
pub struct Icon {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
}

/// Premultiplied RGBA, 0..=1.
type Rgba = [f32; 4];

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Rgba {
    [r as f32 / 255.0 * a, g as f32 / 255.0 * a, b as f32 / 255.0 * a, a]
}

const BG: (u8, u8, u8, f32) = (20, 21, 24, 0.94);
const FG: (u8, u8, u8) = (245, 245, 247);

/// Coverage of a pixel at signed distance `d` (negative inside).
fn cover(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

/// Signed distance to a rounded rectangle (centre cx, cy; half sizes hw, hh; radius r).
fn rrect(px: f32, py: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hw - r);
    let qy = (py - cy).abs() - (hh - r);
    let out = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    out + qx.max(qy).min(0.0) - r
}

/// Signed distance to a segment of thickness `t` (round caps).
fn capsule(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32, t: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let h = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    ((px - ax - dx * h).powi(2) + (py - ay - dy * h).powi(2)).sqrt() - t / 2.0
}

/// A premultiplied RGBA image with source-over drawing.
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<Rgba>,
}

impl Canvas {
    pub fn new(w: usize, h: usize) -> Self {
        Canvas { w, h, px: vec![[0.0; 4]; w * h] }
    }

    fn over(&mut self, x: usize, y: usize, c: Rgba, k: f32) {
        if k <= 0.0 || x >= self.w || y >= self.h {
            return;
        }
        let p = &mut self.px[y * self.w + x];
        for i in 0..4 {
            p[i] = c[i] * k + p[i] * (1.0 - c[3] * k);
        }
    }

    /// Fills the shape whose signed distance `d` gives, inside the box (x0, y0)-(x1, y1).
    fn fill(&mut self, bx: (f32, f32, f32, f32), c: Rgba, d: impl Fn(f32, f32) -> f32) {
        let x0 = (bx.0.floor().max(0.0)) as usize;
        let y0 = (bx.1.floor().max(0.0)) as usize;
        let x1 = (bx.2.ceil().max(0.0) as usize).min(self.w);
        let y1 = (bx.3.ceil().max(0.0) as usize).min(self.h);
        for y in y0..y1 {
            for x in x0..x1 {
                let k = cover(d(x as f32 + 0.5, y as f32 + 0.5));
                self.over(x, y, c, k);
            }
        }
    }

    fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgba) {
        self.fill((x - 1.0, y - 1.0, x + w + 1.0, y + h + 1.0), c, |px, py| rrect(px, py, x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0, r));
    }
}

/// The bar's pill, `w` x `h`.
pub fn render_bar(bar: Bar, w: usize, h: usize) -> Canvas {
    let mut cv = Canvas::new(w, h);
    let (wf, hf) = (w as f32, h as f32);
    let fg = rgba(FG.0, FG.1, FG.2, 1.0);
    let dim = rgba(FG.0, FG.1, FG.2, 0.22);
    cv.rrect(0.0, 0.0, wf, hf, hf / 2.0, rgba(BG.0, BG.1, BG.2, BG.3));
    let u = hf / 10.0;
    let (ix, iy) = (hf * 0.62, hf / 2.0);
    let level = match bar {
        Bar::Volume { muted: true, .. } => 0.0,
        Bar::Volume { level, .. } | Bar::Brightness { level } => level.min(100) as f32 / 100.0,
    };
    let icon = (ix - 3.0 * u, iy - 3.5 * u, ix + 4.0 * u, iy + 3.5 * u);
    match bar {
        Bar::Volume { muted, .. } => {
            // Speaker: body and a cone widening to the right.
            cv.fill(icon, fg, |fx, fy| {
                let body = rrect(fx, fy, ix - 1.6 * u, iy, 0.9 * u, 1.15 * u, 0.3 * u);
                let (x0, x1) = (ix - 0.9 * u, ix + 0.6 * u);
                let half = 1.15 * u + (fx.clamp(x0, x1) - x0) / (x1 - x0) * 1.35 * u;
                let cone = (x0 - fx).max(fx - x1).max(((fy - iy).abs() - half) * 0.8);
                body.min(cone)
            });
            if muted {
                cv.fill(icon, fg, |fx, fy| {
                    let a = capsule(fx, fy, ix + 1.4 * u, iy - 1.1 * u, ix + 3.4 * u, iy + 1.1 * u, 0.55 * u);
                    let b = capsule(fx, fy, ix + 1.4 * u, iy + 1.1 * u, ix + 3.4 * u, iy - 1.1 * u, 0.55 * u);
                    a.min(b)
                });
            } else {
                // Sound waves, more of them the louder it is.
                for (i, rad) in [1.6 * u, 2.9 * u].into_iter().enumerate() {
                    let on = level > 0.0 && (i == 0 || level > 0.5);
                    cv.fill(icon, if on { fg } else { dim }, |fx, fy| {
                        let (dx, dy) = (fx - (ix + 0.2 * u), fy - iy);
                        let r = (dx * dx + dy * dy).sqrt();
                        ((r - rad).abs() - 0.32 * u).max((dy.abs() - dx * 1.1) * 0.7)
                    });
                }
            }
        }
        Bar::Brightness { .. } => {
            // Sun: a disc and eight rays.
            let (cx, cy) = (ix + 0.2 * u, iy);
            cv.fill(icon, fg, |fx, fy| {
                let mut d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt() - 1.25 * u;
                for k in 0..8 {
                    let a = k as f32 * std::f32::consts::FRAC_PI_4;
                    let (s, c) = a.sin_cos();
                    d = d.min(capsule(fx, fy, cx + c * 2.1 * u, cy + s * 2.1 * u, cx + c * 2.9 * u, cy + s * 2.9 * u, 0.5 * u));
                }
                d
            });
        }
    }
    // Track, then the level.
    let (bx0, bx1) = (hf * 1.22, wf - hf * 0.5);
    let bh = (hf * 0.11).max(3.0);
    cv.rrect(bx0, (hf - bh) / 2.0, bx1 - bx0, bh, bh / 2.0, dim);
    if level > 0.0 {
        let fw = ((bx1 - bx0) * level).max(bh);
        cv.rrect(bx0, (hf - bh) / 2.0, fw, bh, bh / 2.0, fg);
    }
    cv
}

/// Text drawing with one font at a few sizes.
pub struct Text {
    font: fontdue::Font,
}

impl Text {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).ok().map(|font| Text { font })
    }

    /// The system's sans font (fontconfig's pick, then known paths).
    pub fn system() -> Option<Self> {
        let mut paths: Vec<String> = Vec::new();
        if let Ok(o) = std::process::Command::new("fc-match").args(["-f", "%{file}", "sans-serif"]).output() {
            let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !p.is_empty() {
                paths.push(p);
            }
        }
        paths.extend(
            [
                "/usr/share/fonts/google-noto-vf/NotoSans[wght].ttf",
                "/usr/share/fonts/abattis-cantarell-vf-fonts/Cantarell-VF.otf",
                "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/System/Library/Fonts/Supplemental/Arial.ttf",
            ]
            .map(String::from),
        );
        paths.iter().find_map(|p| std::fs::read(p).ok().and_then(|b| Self::from_bytes(&b)))
    }

    pub fn width(&self, s: &str, size: f32) -> f32 {
        s.chars().map(|c| self.font.metrics(c, size).advance_width).sum()
    }

    /// `s`, cut with an ellipsis to fit `max` pixels.
    pub fn fit(&self, s: &str, size: f32, max: f32) -> String {
        if self.width(s, size) <= max {
            return s.to_string();
        }
        let mut out: Vec<char> = s.chars().collect();
        while !out.is_empty() && self.width(&format!("{}…", out.iter().collect::<String>().trim_end()), size) > max {
            out.pop();
        }
        format!("{}…", out.iter().collect::<String>().trim_end())
    }

    /// Up to `lines` lines of `s` wrapped at words into `max` pixels; the last one cut to fit.
    pub fn wrap(&self, s: &str, size: f32, max: f32, lines: usize) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut cur = String::new();
        let words: Vec<&str> = s.split_whitespace().collect();
        for (i, w) in words.iter().enumerate() {
            let next = if cur.is_empty() { w.to_string() } else { format!("{cur} {w}") };
            if self.width(&next, size) <= max || cur.is_empty() {
                cur = next;
                continue;
            }
            out.push(std::mem::take(&mut cur));
            if out.len() == lines - 1 {
                // The rest goes on the last line, cut to fit.
                let rest = words[i..].join(" ");
                out.push(self.fit(&rest, size, max));
                return out;
            }
            cur = w.to_string();
        }
        if !cur.is_empty() {
            out.push(self.fit(&cur, size, max));
        }
        out.truncate(lines);
        out
    }

    /// Draws `s` with its baseline at `y`, from `x`.
    pub fn draw(&self, cv: &mut Canvas, s: &str, x: f32, y: f32, size: f32, c: Rgba) {
        let mut pen = x;
        for ch in s.chars() {
            let (m, bitmap) = self.font.rasterize(ch, size);
            let gx = (pen + m.xmin as f32).round() as i32;
            let gy = (y - m.ymin as f32 - m.height as f32).round() as i32;
            for row in 0..m.height {
                for col in 0..m.width {
                    let k = bitmap[row * m.width + col] as f32 / 255.0;
                    let (px, py) = (gx + col as i32, gy + row as i32);
                    if px >= 0 && py >= 0 {
                        cv.over(px as usize, py as usize, c, k);
                    }
                }
            }
            pen += m.advance_width;
        }
    }
}

/// Sizes of a toast card for a root whose shorter side is `short` pixels.
#[derive(Clone, Copy, Debug)]
pub struct ToastSizes {
    pub w: usize,
    pub pad: f32,
    pub title: f32,
    pub body: f32,
    pub gap: usize,
}

pub fn toast_sizes(rw: u16, rh: u16) -> ToastSizes {
    let short = rw.min(rh) as f32;
    ToastSizes { w: ((short * 0.54).round() as usize).min(rw as usize - 32), pad: (short * 0.022).round(), title: (short * 0.024).round(), body: (short * 0.021).round(), gap: (short * 0.012).round() as usize }
}

/// One toast card: its pixels (height follows the text).
pub fn render_toast(text: &Text, t: &Toast, s: ToastSizes) -> Canvas {
    let line = |size: f32| (size * 1.35).round();
    // The picture is a square two text lines tall, left of the text.
    let side = if t.icon.is_some() { (line(s.title) + line(s.body)).round() } else { 0.0 };
    let left = if side > 0.0 { s.pad + side + s.pad * 0.75 } else { s.pad };
    let inner = s.w as f32 - left - s.pad;
    let body = t.body.as_deref().map(|b| text.wrap(b, s.body, inner, 2)).unwrap_or_default();
    let h = (2.0 * s.pad + (line(s.title) + body.len() as f32 * line(s.body)).max(side)).round() as usize;
    let mut cv = Canvas::new(s.w, h);
    cv.rrect(0.0, 0.0, s.w as f32, h as f32, s.pad * 0.9, rgba(BG.0, BG.1, BG.2, BG.3));
    if let Some(icon) = &t.icon {
        let top = ((h as f32 - side) / 2.0).round();
        draw_icon(&mut cv, icon, s.pad, top, side, side * 0.16);
    }
    let text_h = line(s.title) + body.len() as f32 * line(s.body);
    let mut y = ((h as f32 - text_h) / 2.0).max(s.pad) + line(s.title) * 0.78;
    text.draw(&mut cv, &text.fit(&t.title, s.title, inner), left, y, s.title, rgba(FG.0, FG.1, FG.2, 1.0));
    for l in &body {
        y += line(s.body);
        text.draw(&mut cv, l, left, y, s.body, rgba(FG.0, FG.1, FG.2, 0.68));
    }
    cv
}

/// Where the toast column starts on a root of `rw` x `rh`: the top-right corner, just under where
/// the launcher's top bar ends, the same place as the launcher's own toasts (shell::toasts: the
/// bar is 4.25rem high with 2.5rem side padding; rem = min(w / 80, h / 50)). Cards stack downward.
/// `bar` is the volume bar's rect while it shows: the column moves under it rather than overlap.
pub fn toast_origin(rw: u16, rh: u16, w: usize, bar: Option<(u16, u16, i16, i16)>) -> (i16, i16) {
    let rem = (rw as f32 / 80.0).min(rh as f32 / 50.0);
    let x = (rw as f32 - 2.5 * rem - w as f32).max(16.0).round() as i16;
    let mut y = (4.75 * rem).round() as i16;
    if let Some((bw, bh, bx, by)) = bar {
        let overlaps_x = (x as i32) < bx as i32 + bw as i32;
        if overlaps_x && y < by + bh as i16 {
            y = by + bh as i16 + (0.5 * rem).round() as i16;
        }
    }
    (x, y)
}

/// Draws `icon` scaled into the `side` square at (x, y), corners rounded by `r` (bilinear).
fn draw_icon(cv: &mut Canvas, icon: &Icon, x: f32, y: f32, side: f32, r: f32) {
    let n = side.round().max(1.0) as usize;
    let at = |sx: usize, sy: usize| -> [f32; 4] {
        let i = (sy.min(icon.h - 1) * icon.w + sx.min(icon.w - 1)) * 4;
        let a = icon.rgba[i + 3] as f32 / 255.0;
        [icon.rgba[i] as f32 / 255.0 * a, icon.rgba[i + 1] as f32 / 255.0 * a, icon.rgba[i + 2] as f32 / 255.0 * a, a]
    };
    for py in 0..n {
        for px in 0..n {
            let fx = ((px as f32 + 0.5) / n as f32 * icon.w as f32 - 0.5).max(0.0);
            let fy = ((py as f32 + 0.5) / n as f32 * icon.h as f32 - 0.5).max(0.0);
            let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
            let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
            let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
            let mut p = [0.0f32; 4];
            for i in 0..4 {
                p[i] = (a[i] * (1.0 - tx) + b[i] * tx) * (1.0 - ty) + (c[i] * (1.0 - tx) + d[i] * tx) * ty;
            }
            let cx = x + px as f32 + 0.5;
            let cy = y + py as f32 + 0.5;
            let k = cover(rrect(cx, cy, x + side / 2.0, y + side / 2.0, side / 2.0, side / 2.0, r));
            cv.over((x as usize) + px, (y as usize) + py, p, k);
        }
    }
}

/// The bar's size and place on a root of `rw` x `rh`: top centre.
pub fn bar_layout(rw: u16, rh: u16) -> (u16, u16, i16, i16) {
    let short = rw.min(rh) as f32;
    let h = (short * 0.07).round().clamp(36.0, 120.0) as u16;
    let w = ((h as f32) * 5.4).round().min(rw as f32 - 32.0) as u16;
    let x = ((rw - w) / 2) as i16;
    let y = (short * 0.045).round() as i16;
    (w, h, x, y)
}

/// The pixels as the X server wants them for a 32-bit ZPixmap (B, G, R, A on LSB-first servers).
pub fn to_bytes(px: &[Rgba], lsb_first: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(px.len() * 4);
    for p in px {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let (r, g, b, a) = (c(p[0]), c(p[1]), c(p[2]), c(p[3]));
        if lsb_first {
            out.extend_from_slice(&[b, g, r, a]);
        } else {
            out.extend_from_slice(&[a, r, g, b]);
        }
    }
    out
}

#[cfg(target_os = "linux")]
mod x {
    use super::*;
    use x11rb::connection::Connection;
    use x11rb::protocol::shape::{self, ConnectionExt as _};
    use x11rb::protocol::xproto::*;
    use x11rb::rust_connection::RustConnection;
    use x11rb::wrapper::ConnectionExt as _;

    pub struct Window {
        conn: RustConnection,
        win: u32,
        gc: u32,
        pub root: (u16, u16),
        lsb: bool,
        opacity_atom: u32,
    }

    impl Window {
        pub fn new(display: &str) -> Result<Self, String> {
            let (conn, n) = x11rb::connect(Some(display)).map_err(|e| format!("x11 {display}: {e}"))?;
            let screen = conn.setup().roots[n].clone();
            let lsb = conn.setup().image_byte_order == ImageOrder::LSB_FIRST;
            let visual = screen
                .allowed_depths
                .iter()
                .filter(|d| d.depth == 32)
                .flat_map(|d| d.visuals.iter())
                .find(|v| v.class == VisualClass::TRUE_COLOR)
                .map(|v| v.visual_id)
                .ok_or("no 32-bit visual")?;
            let cmap = conn.generate_id().map_err(|e| e.to_string())?;
            conn.create_colormap(ColormapAlloc::NONE, cmap, screen.root, visual).map_err(|e| e.to_string())?;
            let (rw, rh) = (screen.width_in_pixels, screen.height_in_pixels);
            let win = conn.generate_id().map_err(|e| e.to_string())?;
            let aux = CreateWindowAux::new().background_pixel(0).border_pixel(0).colormap(cmap).override_redirect(1u32);
            conn.create_window(32, win, screen.root, 0, 0, rw, rh, 0, WindowClass::INPUT_OUTPUT, visual, &aux).map_err(|e| e.to_string())?;
            let atom = conn.intern_atom(false, b"STEAM_OVERLAY").map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?.atom;
            conn.change_property32(PropMode::REPLACE, win, atom, AtomEnum::CARDINAL, &[1]).map_err(|e| e.to_string())?;
            conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_NAME, AtomEnum::STRING, b"Rare HUD").map_err(|e| e.to_string())?;
            let opacity_atom = conn.intern_atom(false, b"_NET_WM_WINDOW_OPACITY").map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?.atom;
            // Hidden from the start; gamescope reads the opacity when the window maps.
            conn.change_property32(PropMode::REPLACE, win, opacity_atom, AtomEnum::CARDINAL, &[0]).map_err(|e| e.to_string())?;
            // No input ever lands on it (an empty input region), mapped or not.
            if let Err(e) = conn.shape_rectangles(shape::SO::SET, shape::SK::INPUT, ClipOrdering::UNSORTED, win, 0, 0, &[]) {
                crate::host::log(&format!("[hud] input shape: {e}"));
            }
            let gc = conn.generate_id().map_err(|e| e.to_string())?;
            conn.create_gc(gc, win, &CreateGCAux::new()).map_err(|e| e.to_string())?;
            conn.map_window(win).map_err(|e| e.to_string())?;
            conn.flush().map_err(|e| e.to_string())?;
            Ok(Window { conn, win, gc, root: (rw, rh), lsb, opacity_atom })
        }
    }

    impl Surface for Window {
        fn root(&self) -> (u16, u16) {
            self.root
        }

        /// Puts `cv` at (x, y), in strips well under the core request size limit.
        fn put(&mut self, cv: &Canvas, x: i16, y: i16) -> Result<(), String> {
            let bytes = to_bytes(&cv.px, self.lsb);
            let row = cv.w * 4;
            let rows = (128 * 1024 / row.max(1)).max(1);
            for (i, chunk) in bytes.chunks(row * rows).enumerate() {
                let h = (chunk.len() / row) as u16;
                let dy = y + (i * rows) as i16;
                self.conn.put_image(ImageFormat::Z_PIXMAP, self.win, self.gc, cv.w as u16, h, x, dy, 0, 32, chunk).map_err(|e| e.to_string())?;
            }
            Ok(())
        }

        fn clear(&mut self, x: i16, y: i16, w: u16, h: u16) -> Result<(), String> {
            self.conn.clear_area(false, self.win, x, y, w, h).map_err(|e| e.to_string())?;
            Ok(())
        }

        fn clear_all(&mut self) -> Result<(), String> {
            // 0 x 0 clears to the window's edges.
            self.conn.clear_area(false, self.win, 0, 0, 0, 0).map_err(|e| e.to_string())?;
            Ok(())
        }

        fn set_opacity(&mut self, k: f32) -> Result<(), String> {
            let v = opacity_value(k);
            if v == u32::MAX {
                // Back on top of other override-redirect windows when it shows.
                self.conn.configure_window(self.win, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE)).map_err(|e| e.to_string())?;
            }
            self.conn.change_property32(PropMode::REPLACE, self.win, self.opacity_atom, AtomEnum::CARDINAL, &[v]).map_err(|e| e.to_string())?;
            Ok(())
        }

        fn flush(&mut self) -> Result<(), String> {
            // Drain events (none asked for; errors arrive here) and send what is queued.
            while self.conn.poll_for_event().map_err(|e| e.to_string())?.is_some() {}
            self.conn.flush().map_err(|e| e.to_string())
        }
    }
}

enum Msg {
    Bar(Bar),
    Toast(Toast),
}

static TX: std::sync::OnceLock<std::sync::Mutex<std::sync::mpsc::Sender<Msg>>> = std::sync::OnceLock::new();

fn send(m: Msg) {
    let tx = TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Msg>();
        std::thread::spawn(move || run(rx));
        std::sync::Mutex::new(tx)
    });
    let _ = tx.lock().unwrap_or_else(|e| e.into_inner()).send(m);
}

/// Shows the volume or brightness bar over whatever has the screen (gamescope sessions only).
pub fn bar(b: Bar) {
    send(Msg::Bar(b));
}

/// Shows a toast over whatever has the screen (gamescope sessions only).
pub fn toast(title: &str, body: Option<&str>) {
    send(Msg::Toast(Toast { title: title.to_string(), body: body.map(str::to_string).filter(|b| !b.trim().is_empty()), icon: None }));
}

/// A toast with a picture on its left (an achievement's badge), drawn like `toast`.
pub fn toast_icon(title: &str, body: Option<&str>, icon: Option<Icon>) {
    send(Msg::Toast(Toast { title: title.to_string(), body: body.map(str::to_string).filter(|b| !b.trim().is_empty()), icon: icon.filter(|i| i.w > 0 && i.h > 0 && i.rgba.len() == i.w * i.h * 4) }));
}

/// A toast from the page. Drawn only while a game or app has the screen: with the launcher in
/// front the page shows its own.
pub fn hud_toast(title: String, body: Option<String>) {
    if crate::host::get().launcher_front() {
        return;
    }
    toast(&title, body.as_deref());
}

/// What is up and until when.
#[derive(Default)]
struct Shown {
    bar: Option<(Bar, std::time::Instant)>,
    toasts: Vec<(Toast, std::time::Instant)>,
}

impl Shown {
    fn next_change(&self) -> Option<std::time::Instant> {
        self.bar.iter().map(|(_, t)| *t).chain(self.toasts.iter().map(|(_, t)| *t)).min()
    }

    /// Drops what has run its time; true when something went.
    fn expire(&mut self, now: std::time::Instant) -> bool {
        let before = (self.bar.is_some(), self.toasts.len());
        if self.bar.is_some_and(|(_, t)| now >= t) {
            self.bar = None;
        }
        self.toasts.retain(|(_, t)| now < *t);
        before != (self.bar.is_some(), self.toasts.len())
    }

    fn add(&mut self, m: Msg, now: std::time::Instant) {
        match m {
            Msg::Bar(b) => self.bar = Some((b, now + BAR_FOR)),
            Msg::Toast(t) => {
                self.toasts.retain(|(x, _)| *x != t);
                self.toasts.push((t, now + TOAST_FOR));
                let n = self.toasts.len();
                if n > MAX_TOASTS {
                    self.toasts.drain(..n - MAX_TOASTS);
                }
            }
        }
    }
}

/// `_NET_WM_WINDOW_OPACITY` for `k` (0..=1): 0 is hidden, u32::MAX opaque.
fn opacity_value(k: f32) -> u32 {
    if k >= 1.0 {
        u32::MAX
    } else if k <= 0.0 {
        0
    } else {
        (k as f64 * u32::MAX as f64) as u32
    }
}

/// What the HUD draws on: the overlay window, or a recorder in tests.
trait Surface {
    fn root(&self) -> (u16, u16);
    fn put(&mut self, cv: &Canvas, x: i16, y: i16) -> Result<(), String>;
    fn clear(&mut self, x: i16, y: i16, w: u16, h: u16) -> Result<(), String>;
    fn clear_all(&mut self) -> Result<(), String>;
    /// 0 hides it (gamescope paints nothing), 1 is fully shown.
    fn set_opacity(&mut self, k: f32) -> Result<(), String>;
    fn flush(&mut self) -> Result<(), String>;
}

/// The HUD's state over time: what is up, the window's opacity, the fade-out.
#[derive(Default)]
struct Hud {
    shown: Shown,
    /// Something was added since the last draw.
    dirty: bool,
    /// The window's opacity as last set (a new window starts hidden).
    opacity: f32,
    /// When the fade-out started (nothing left to show, the window still visible).
    fade_from: Option<std::time::Instant>,
    /// The toast column drawn last (x, y, w, h), to clear before a redraw.
    column: Option<(i16, i16, u16, u16)>,
    text: Option<Option<Text>>,
}

impl Hud {
    fn add(&mut self, m: Msg, now: std::time::Instant) {
        self.shown.add(m, now);
        self.dirty = true;
    }

    /// When `tick` has work next: an expiry or the next fade step.
    fn next_wake(&self, now: std::time::Instant) -> Option<std::time::Instant> {
        let fading = self.opacity > 0.0 && self.shown.bar.is_none() && self.shown.toasts.is_empty();
        let step = (fading || self.fade_from.is_some()).then(|| now + FADE_STEP);
        [self.shown.next_change(), step].into_iter().flatten().min()
    }

    /// A window was lost (or never made): whatever comes next starts from a hidden window.
    fn reset_window(&mut self) {
        self.opacity = 0.0;
        self.fade_from = None;
        self.column = None;
    }

    /// Takes everything down at once (gamescope can't draw now); what is still in time shows again
    /// once it can.
    fn hide_now(&mut self, s: &mut impl Surface) -> Result<(), String> {
        if self.opacity > 0.0 {
            s.clear_all()?;
            s.set_opacity(0.0)?;
            s.flush()?;
        }
        self.reset_window();
        Ok(())
    }

    /// Expires what has run its time and brings the window up to date: draws what changed, or
    /// fades the window out once nothing is left (`FADE`), then clears it and sets it hidden.
    fn tick(&mut self, s: &mut impl Surface, now: std::time::Instant) -> Result<(), String> {
        let expired = self.shown.expire(now);
        let visible = self.shown.bar.is_some() || !self.shown.toasts.is_empty();
        if !visible {
            self.dirty = false;
            if self.opacity <= 0.0 {
                return Ok(());
            }
            let from = *self.fade_from.get_or_insert(now);
            let k = 1.0 - now.saturating_duration_since(from).as_secs_f32() / FADE.as_secs_f32();
            if k > 0.0 {
                if k < self.opacity {
                    s.set_opacity(k)?;
                    self.opacity = k;
                }
            } else {
                // Clear first: should anything still paint the window, it paints nothing.
                s.clear_all()?;
                s.set_opacity(0.0)?;
                self.reset_window();
            }
            return s.flush();
        }
        if !(expired || self.dirty || self.fade_from.is_some() || self.opacity < 1.0) {
            return Ok(());
        }
        self.dirty = false;
        self.fade_from = None;
        let (rw, rh) = s.root();
        let (bw, bh, bx, by) = bar_layout(rw, rh);
        match self.shown.bar {
            Some((b, _)) => s.put(&render_bar(b, bw as usize, bh as usize), bx, by)?,
            None => s.clear(bx, by, bw, bh)?,
        }
        if let Some((x, y, cw, ch)) = self.column.take() {
            s.clear(x, y, cw, ch)?;
        }
        if !self.shown.toasts.is_empty() {
            let font = self.text.get_or_insert_with(|| {
                let t = Text::system();
                if t.is_none() {
                    crate::host::log("[hud] no font for toasts");
                }
                t
            });
            if let Some(font) = font.as_ref() {
                let ts = toast_sizes(rw, rh);
                let (x, top) = toast_origin(rw, rh, ts.w, self.shown.bar.map(|_| (bw, bh, bx, by)));
                let mut y = top;
                // Newest on top.
                for (t, _) in self.shown.toasts.iter().rev() {
                    let cv = render_toast(font, t, ts);
                    s.put(&cv, x, y)?;
                    y += (cv.h + ts.gap) as i16;
                }
                self.column = Some((x, top, ts.w as u16, (y - top) as u16));
            }
        }
        // Shown after the pixels are in, so it never flashes what was there before.
        if self.opacity < 1.0 {
            s.set_opacity(1.0)?;
            self.opacity = 1.0;
        }
        s.flush()
    }
}

#[cfg(target_os = "linux")]
fn run(rx: std::sync::mpsc::Receiver<Msg>) {
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Instant;
    let mut win: Option<x::Window> = None;
    let mut hud = Hud::default();
    loop {
        let now = Instant::now();
        let wait = hud.next_wake(now).map(|t| t.saturating_duration_since(now)).unwrap_or(Duration::from_secs(3600));
        match rx.recv_timeout(wait.max(Duration::from_millis(1))) {
            Ok(m) => {
                hud.add(m, Instant::now());
                while let Ok(m) = rx.try_recv() {
                    hud.add(m, Instant::now());
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if !crate::host::get().gamescope_active() {
            // Nothing can be drawn now: take down what is up rather than leave it on screen.
            if let Some(w) = win.as_mut() {
                if hud.hide_now(w).is_err() {
                    win = None;
                }
            }
            hud.reset_window();
            continue;
        }
        if win.is_none() {
            if !hud.dirty {
                // Nothing new to show: no window is needed to expire what was up.
                hud.shown.expire(Instant::now());
                continue;
            }
            match x::Window::new(&crate::host::get().gamescope_base_display()) {
                Ok(w) => {
                    win = Some(w);
                    hud.reset_window();
                }
                Err(e) => {
                    crate::host::log(&format!("[hud] no overlay window: {e}"));
                    hud.shown = Shown::default();
                    hud.dirty = false;
                    continue;
                }
            }
        }
        let Some(w) = win.as_mut() else { continue };
        if let Err(e) = hud.tick(w, Instant::now()) {
            crate::host::log(&format!("[hud] {e}"));
            win = None;
            hud.reset_window();
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn run(rx: std::sync::mpsc::Receiver<Msg>) {
    while rx.recv().is_ok() {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn alpha_at(cv: &Canvas, x: usize, y: usize) -> f32 {
        cv.px[y * cv.w + x][3]
    }

    #[test]
    fn pill_is_transparent_outside_and_solid_inside() {
        let cv = render_bar(Bar::Volume { level: 50, muted: false }, 300, 56);
        assert_eq!(alpha_at(&cv, 0, 0), 0.0, "rounded corner");
        assert!(alpha_at(&cv, 150, 28) > 0.9);
        // Premultiplied: no channel above alpha.
        assert!(cv.px.iter().all(|p| p[0] <= p[3] + 1e-4 && p[1] <= p[3] + 1e-4 && p[2] <= p[3] + 1e-4));
    }

    #[test]
    fn bar_follows_the_level() {
        let (w, h) = (300, 56);
        let x = (h as f32 * 1.22 + (w as f32 - h as f32 * 1.72) * 0.75) as usize;
        let bright = |b: Bar| render_bar(b, w, h).px[(h / 2) * w + x][0];
        // At 75 % of the bar: lit at 100, dim at 50, dim when muted.
        assert!(bright(Bar::Volume { level: 100, muted: false }) > 0.8);
        assert!(bright(Bar::Volume { level: 50, muted: false }) < 0.5);
        assert!(bright(Bar::Volume { level: 100, muted: true }) < 0.5);
        assert!(bright(Bar::Brightness { level: 100 }) > 0.8);
    }

    #[test]
    fn layouts_fit_the_root() {
        for (rw, rh) in [(1280u16, 800u16), (1920, 1200), (800, 1280)] {
            let (w, h, x, y) = bar_layout(rw, rh);
            assert!(x >= 0 && y >= 0 && (x as u32 + w as u32) <= rw as u32 && (y as u32 + h as u32) <= rh as u32);
            assert!(h >= 36);
            let s = toast_sizes(rw, rh);
            assert!(s.w + 2 * s.gap <= rw as usize && s.title >= 12.0);
            // Top right, under the top bar's height, inside the screen, and under the volume bar
            // when that shows over the same columns.
            let (tx, ty) = toast_origin(rw, rh, s.w, None);
            assert!(tx >= 0 && tx as usize + s.w <= rw as usize && ty > 0 && (ty as u16) < rh / 4);
            let (_, ty2) = toast_origin(rw, rh, s.w, Some((w, h, x, y)));
            assert!(ty2 >= ty);
        }
    }

    #[test]
    fn byte_order() {
        let px = [[0.2f32, 0.4, 0.6, 1.0]];
        assert_eq!(to_bytes(&px, true), vec![153, 102, 51, 255]);
        assert_eq!(to_bytes(&px, false), vec![255, 51, 102, 153]);
    }

    #[test]
    fn toasts_queue_newest_and_expire() {
        let t0 = Instant::now();
        let mut s = Shown::default();
        for i in 0..5 {
            s.add(Msg::Toast(Toast { title: format!("t{i}"), body: None, icon: None }), t0);
        }
        assert_eq!(s.toasts.len(), MAX_TOASTS);
        assert_eq!(s.toasts[0].0.title, "t2");
        s.add(Msg::Bar(Bar::Volume { level: 10, muted: false }), t0);
        assert_eq!(s.next_change(), Some(t0 + BAR_FOR));
        assert!(s.expire(t0 + BAR_FOR));
        assert!(s.bar.is_none() && s.toasts.len() == 3);
        assert!(s.expire(t0 + TOAST_FOR));
        assert!(s.toasts.is_empty());
        assert!(!s.expire(t0 + TOAST_FOR));
    }

    /// Records what the HUD does to its window.
    #[derive(Default)]
    struct Rec {
        ops: Vec<String>,
        opacity: f32,
    }

    impl Surface for Rec {
        fn root(&self) -> (u16, u16) {
            (1920, 1200)
        }
        fn put(&mut self, _: &Canvas, _: i16, _: i16) -> Result<(), String> {
            self.ops.push("put".into());
            Ok(())
        }
        fn clear(&mut self, _: i16, _: i16, _: u16, _: u16) -> Result<(), String> {
            self.ops.push("clear".into());
            Ok(())
        }
        fn clear_all(&mut self) -> Result<(), String> {
            self.ops.push("clear_all".into());
            Ok(())
        }
        fn set_opacity(&mut self, k: f32) -> Result<(), String> {
            self.opacity = k;
            self.ops.push(format!("opacity {k:.2}"));
            Ok(())
        }
        fn flush(&mut self) -> Result<(), String> {
            Ok(())
        }
    }

    fn vol(level: u8) -> Msg {
        Msg::Bar(Bar::Volume { level, muted: false })
    }

    /// Runs the HUD's timers from `t` the way `run` does until nothing is due; returns when that was.
    fn run_out(hud: &mut Hud, rec: &mut Rec, mut t: Instant) -> Instant {
        for _ in 0..1000 {
            match hud.next_wake(t) {
                Some(w) => {
                    t = w.max(t);
                    hud.tick(rec, t).unwrap();
                }
                None => return t,
            }
        }
        panic!("the HUD never settles");
    }

    #[test]
    fn volume_bar_fades_out_after_the_last_change_and_stays_hidden() {
        let t0 = Instant::now();
        let mut hud = Hud::default();
        let mut rec = Rec::default();
        hud.add(vol(50), t0);
        hud.tick(&mut rec, t0).unwrap();
        assert_eq!(rec.opacity, 1.0);
        let shown_at = rec.ops.iter().position(|o| o == "opacity 1.00").unwrap();
        assert!(rec.ops[..shown_at].contains(&"put".to_string()), "pixels go in before it shows: {:?}", rec.ops);

        // Presses keep it up: it goes BAR_FOR after the last one, not the first.
        let last = t0 + Duration::from_millis(1000);
        hud.add(vol(55), last);
        hud.tick(&mut rec, last).unwrap();
        hud.tick(&mut rec, t0 + Duration::from_millis(2000)).unwrap();
        assert_eq!(rec.opacity, 1.0);
        assert_eq!(hud.next_wake(t0 + Duration::from_millis(2000)), Some(last + BAR_FOR));

        rec.ops.clear();
        let done = run_out(&mut hud, &mut rec, t0 + Duration::from_millis(2000));
        // Faded out right after BAR_FOR, then cleared and hidden by opacity (gamescope keeps
        // painting an unmapped overlay window, so it is never unmapped).
        assert_eq!(rec.opacity, 0.0);
        assert!(done >= last + BAR_FOR && done <= last + BAR_FOR + FADE + FADE_STEP, "{:?}", done - last);
        let n = rec.ops.len();
        assert_eq!(rec.ops[n - 2..], ["clear_all".to_string(), "opacity 0.00".to_string()]);
        let steps: Vec<f32> = rec.ops.iter().filter_map(|o| o.strip_prefix("opacity ")).map(|v| v.parse().unwrap()).collect();
        assert!(steps.len() >= 3 && steps.windows(2).all(|w| w[1] < w[0]), "a fade, not a cut: {steps:?}");
        assert!(hud.shown.bar.is_none() && hud.fade_from.is_none());

        // Nothing more happens however long it sits.
        rec.ops.clear();
        hud.tick(&mut rec, done + Duration::from_secs(3600)).unwrap();
        assert!(rec.ops.is_empty() && hud.next_wake(done).is_none());
    }

    #[test]
    fn a_press_during_the_fade_brings_it_back() {
        let t0 = Instant::now();
        let mut hud = Hud::default();
        let mut rec = Rec::default();
        hud.add(vol(50), t0);
        hud.tick(&mut rec, t0).unwrap();
        let t = t0 + BAR_FOR;
        hud.tick(&mut rec, t).unwrap();
        hud.tick(&mut rec, t + FADE / 2).unwrap();
        assert!(rec.opacity > 0.0 && rec.opacity < 1.0, "{}", rec.opacity);
        hud.add(vol(45), t + FADE / 2);
        hud.tick(&mut rec, t + FADE / 2).unwrap();
        assert_eq!(rec.opacity, 1.0);
        assert_eq!(hud.next_wake(t + FADE / 2), Some(t + FADE / 2 + BAR_FOR));
        run_out(&mut hud, &mut rec, t + FADE / 2);
        assert_eq!(rec.opacity, 0.0);
    }

    #[test]
    fn toasts_outlast_the_bar_then_it_all_hides() {
        let t0 = Instant::now();
        let mut hud = Hud::default();
        let mut rec = Rec::default();
        hud.add(vol(50), t0);
        hud.add(Msg::Toast(Toast { title: "Offer received".into(), body: None, icon: None }), t0);
        hud.tick(&mut rec, t0).unwrap();
        rec.ops.clear();
        hud.tick(&mut rec, t0 + BAR_FOR).unwrap();
        assert_eq!(rec.opacity, 1.0, "the toast is still up");
        assert!(rec.ops.contains(&"clear".to_string()), "the bar's place is cleared: {:?}", rec.ops);
        let done = run_out(&mut hud, &mut rec, t0 + BAR_FOR);
        assert_eq!(rec.opacity, 0.0);
        assert!(done <= t0 + TOAST_FOR + FADE + FADE_STEP);
    }

    #[test]
    fn hide_now_takes_it_down_at_once() {
        let t0 = Instant::now();
        let mut hud = Hud::default();
        let mut rec = Rec::default();
        hud.add(vol(50), t0);
        hud.tick(&mut rec, t0).unwrap();
        hud.hide_now(&mut rec).unwrap();
        assert_eq!(rec.opacity, 0.0);
        assert_eq!(rec.ops.last().map(String::as_str), Some("opacity 0.00"));
        // Still in time when gamescope can draw again: it shows again.
        hud.tick(&mut rec, t0 + Duration::from_millis(100)).unwrap();
        assert_eq!(rec.opacity, 1.0);
    }

    #[test]
    fn opacity_values() {
        assert_eq!(opacity_value(1.0), u32::MAX);
        assert_eq!(opacity_value(0.0), 0);
        assert_eq!(opacity_value(-1.0), 0);
        let half = opacity_value(0.5);
        assert!(half > u32::MAX / 2 - 2 && half < u32::MAX / 2 + 2);
    }

    #[test]
    fn text_wraps_and_fits() {
        let Some(t) = Text::system() else { return };
        let lines = t.wrap("Someone offered 12.5 SUI for your Warped Universe item, the best offer so far today", 16.0, 200.0, 2);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| t.width(l, 16.0) <= 200.5));
        assert!(lines[1].ends_with('…'));
        assert_eq!(t.wrap("Short", 16.0, 200.0, 2), vec!["Short"]);
        let cv = render_toast(&t, &Toast { title: "Offer received".into(), body: Some("12.5 SUI for Warped Universe #12".into()), icon: None }, toast_sizes(1280, 800));
        assert!(cv.h > 40 && cv.px.iter().any(|p| p[3] > 0.9));
        // A badge: a red square drawn on the left, the text moved right of it.
        let icon = Icon { w: 4, h: 4, rgba: [255u8, 0, 0, 255].repeat(16) };
        let s = toast_sizes(1280, 800);
        let cv = render_toast(&t, &Toast { title: "Four!".into(), body: Some("Get a 4! \u{b7} 1 point".into()), icon: Some(icon) }, s);
        let mid = cv.px[(cv.h / 2) * cv.w + (s.pad as usize + 10)];
        assert!(mid[0] > 0.9 && mid[1] < 0.1, "{mid:?}");
        if let Ok(dir) = std::env::var("HUD_SHOT") {
            let bytes: Vec<u8> = cv.px.iter().flat_map(|p| { let a = p[3].max(1e-6); [(p[0] / a * 255.0) as u8, (p[1] / a * 255.0) as u8, (p[2] / a * 255.0) as u8, (p[3] * 255.0) as u8] }).collect();
            std::fs::write(format!("{dir}/toast-{}x{}.rgba", cv.w, cv.h), bytes).unwrap();
        }
    }
}
