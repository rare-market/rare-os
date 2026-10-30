//! Text parity: draws text-parity/specimen.txt with the engine's text path and compares it with
//! the browser's rendering of the same specimen (text-parity/web.mjs).
//!
//!   node launcher/render/engine/text-parity/web.mjs OUT
//!   cargo run -p rare-render --example text_parity -- OUT [--fit]
//!
//! Glyph quads come from `TextSys::paint` and are composited on the CPU with the pipeline's own
//! math (shader.wgsl kind 3: premultiplied colour times atlas coverage, premultiplied-over blend in
//! sRGB-encoded space), so the result is what the GPU draws, without a GPU. Writes native.png,
//! diff-<engine>.png (absolute difference x4) and report.md; `--fit` searches the rasteriser's
//! gamma and contrast for the smallest error against each browser.

use rare_render::gpu::{DrawList, KIND_GLYPH};
use rare_render::text::{TextPaint, TextStyle, TextSys, Tune};
use rare_render::Color;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const X: f32 = 24.0;
const GAP: f32 = 12.0;
const WIDTH: u32 = 1600;
const BG: [u8; 3] = [16, 12, 24];

struct Line {
    style: TextStyle,
    color: Color,
    text: String,
    top: f32,
    /// Box width for truncated, wrapped and clamped lines.
    width: Option<f32>,
    /// Lines the box reserves.
    rows: u32,
}

fn spec(path: &Path) -> (Vec<Line>, u32) {
    let src = std::fs::read_to_string(path).expect("specimen");
    let mut out = Vec::new();
    let mut y = 16.0;
    for raw in src.lines() {
        if raw.trim().is_empty() || raw.starts_with('#') {
            continue;
        }
        let (head, text) = raw.split_once(" | ").expect("line format");
        let f: Vec<&str> = head.split_whitespace().collect();
        let (size, line): (f32, f32) = (f[2].parse().unwrap(), f[3].parse().unwrap());
        let mut st = TextStyle::new(size, f[1].parse().unwrap()).line(line).tracking(f[4].parse().unwrap());
        if f[0] == "fira" {
            st = st.mono();
        }
        if f[5] == "tnum" {
            st = st.tabular();
        }
        let color = match f[6] {
            "fg" => Color::rgb(242, 242, 243),
            "muted" => Color::rgb(133, 127, 148),
            "pos" => Color::rgb(42, 223, 114),
            "neg" => Color::rgb(234, 65, 65),
            _ => Color::WHITE,
        };
        let (mut text, mut width, mut rows) = (text.to_string(), None, 1);
        if let Some(rest) = text.strip_prefix('@') {
            let (spec, body) = rest.split_once(' ').expect("@box text");
            let at = spec.find(|c: char| !c.is_ascii_digit()).expect("box mode");
            width = Some(spec[..at].parse::<f32>().unwrap());
            let n: u32 = spec[at + 1..].parse().unwrap_or(1);
            match &spec[at..at + 1] {
                "t" => st = st.truncate(),
                "w" => st = st.wrap(),
                _ => st = st.clamp(n as u16),
            }
            rows = n;
            text = body.to_string();
        }
        out.push(Line { style: st, color, text, top: y, width, rows });
        y += line * rows as f32 + GAP;
    }
    (out, (y + 16.0) as u32)
}

struct Canvas {
    w: u32,
    h: u32,
    px: Vec<f32>,
}

impl Canvas {
    fn new(w: u32, h: u32) -> Canvas {
        let mut px = Vec::with_capacity((w * h * 3) as usize);
        for _ in 0..w * h {
            px.extend(BG.map(|c| c as f32 / 255.0));
        }
        Canvas { w, h, px }
    }

    fn rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity((self.w * self.h * 4) as usize);
        for p in self.px.chunks(3) {
            out.extend(p.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
            out.push(255);
        }
        out
    }
}

/// The atlas as the GPU holds it.
struct Atlas {
    size: u32,
    px: Vec<u8>,
}

impl Atlas {
    fn upload(&mut self, t: &mut TextSys) {
        for (s, data) in t.pending.drain(..) {
            for row in 0..s.h {
                let dst = ((s.y + row) * self.size + s.x) as usize;
                self.px[dst..dst + s.w as usize].copy_from_slice(&data[(row * s.w) as usize..((row + 1) * s.w) as usize]);
            }
        }
    }

    /// Bilinear sample at a texel-space point (wgpu linear filter, clamp).
    fn sample(&self, u: f32, v: f32) -> f32 {
        let (x, y) = (u - 0.5, v - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |xi: f32, yi: f32| {
            let xi = (xi as i64).clamp(0, self.size as i64 - 1) as u32;
            let yi = (yi as i64).clamp(0, self.size as i64 - 1) as u32;
            self.px[(yi * self.size + xi) as usize] as f32 / 255.0
        };
        let a = at(x0, y0) * (1.0 - fx) + at(x0 + 1.0, y0) * fx;
        let b = at(x0, y0 + 1.0) * (1.0 - fx) + at(x0 + 1.0, y0 + 1.0) * fx;
        a * (1.0 - fy) + b * fy
    }
}

fn composite(list: &DrawList, atlas: &Atlas, c: &mut Canvas) {
    for p in &list.prims {
        if p.p[2] != KIND_GLYPH {
            continue;
        }
        let [x, y, w, h] = p.rect;
        let s = atlas.size as f32;
        let (x0, y0) = (x.floor().max(0.0) as u32, y.floor().max(0.0) as u32);
        let (x1, y1) = (((x + w).ceil() as u32).min(c.w), ((y + h).ceil() as u32).min(c.h));
        for py in y0..y1 {
            for px in x0..x1 {
                let (cx, cy) = (px as f32 + 0.5, py as f32 + 0.5);
                if cx < x || cy < y || cx > x + w || cy > y + h {
                    continue;
                }
                let lu = ((cx - x) / w).clamp(0.0, 1.0);
                let lv = ((cy - y) / h).clamp(0.0, 1.0);
                let u = (p.uv[0] + (p.uv[2] - p.uv[0]) * lu) * s;
                let v = (p.uv[1] + (p.uv[3] - p.uv[1]) * lv) * s;
                let a = atlas.sample(u, v) * p.c0[3] * p.p[3];
                let i = ((py * c.w + px) * 3) as usize;
                for k in 0..3 {
                    c.px[i + k] = p.c0[k] * a + c.px[i + k] * (1.0 - a);
                }
            }
        }
    }
}

/// Unrounded layout width of each line (what flex layout would get).
fn widths(t: &mut TextSys, lines: &[Line]) -> Vec<f32> {
    lines
        .iter()
        .map(|l| {
            let mut buf = t.buffer(&l.text, &l.style);
            t.measure(&mut buf, l.width);
            l.width.unwrap_or_else(|| buf.layout_runs().map(|r| r.line_w).fold(0.0, f32::max))
        })
        .collect()
}

fn render(t: &mut TextSys, lines: &[Line], h: u32) -> Vec<u8> {
    let mut atlas = Atlas { size: t.atlas_size, px: vec![0; (t.atlas_size * t.atlas_size) as usize] };
    let mut canvas = Canvas::new(WIDTH, h);
    let mut list = DrawList::default();
    for l in lines {
        let mut buf = t.buffer(&l.text, &l.style);
        t.measure(&mut buf, l.width);
        list.clear();
        t.paint(&buf, &TextPaint::at(X, l.top, l.color), &mut list);
        atlas.upload(t);
        composite(&list, &atlas, &mut canvas);
    }
    canvas.rgba()
}

#[derive(Default, Clone, Copy)]
struct Stat {
    /// Mean absolute difference over the union of both inks, 0..255.
    mae: f64,
    /// Share of pixels in the line box off by more than 2 levels.
    off: f64,
    /// Native ink over browser ink.
    ink: f64,
    dx: f64,
    dy: f64,
    /// Right edge of the ink, native minus browser, px.
    dw: i64,
}

fn ink_of(p: &[u8]) -> f64 {
    (0..3).map(|k| (p[k] as f64 - BG[k] as f64).abs()).fold(0.0, f64::max)
}

fn line_stat(a: &[u8], b: &[u8], top: u32, hgt: u32) -> Stat {
    let (mut diff, mut n, mut off, mut tot) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let (mut ia, mut ib) = (0.0f64, 0.0f64);
    let (mut xa, mut ya, mut xb, mut yb) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let (mut ra, mut rb) = (0i64, 0i64);
    for y in top..top + hgt {
        for x in 0..WIDTH {
            let i = ((y * WIDTH + x) * 4) as usize;
            let (pa, pb) = (&a[i..i + 3], &b[i..i + 3]);
            let (ka, kb) = (ink_of(pa), ink_of(pb));
            let d = (0..3).map(|k| (pa[k] as f64 - pb[k] as f64).abs()).fold(0.0, f64::max);
            tot += 1.0;
            if d > 2.0 {
                off += 1.0;
            }
            if ka > 2.0 || kb > 2.0 {
                diff += d;
                n += 1.0;
            }
            ia += ka;
            ib += kb;
            xa += ka * x as f64;
            ya += ka * y as f64;
            xb += kb * x as f64;
            yb += kb * y as f64;
            if ka > 24.0 {
                ra = ra.max(x as i64);
            }
            if kb > 24.0 {
                rb = rb.max(x as i64);
            }
        }
    }
    Stat { mae: diff / n.max(1.0), off: off / tot, ink: ia / ib.max(1.0), dx: xa / ia.max(1.0) - xb / ib.max(1.0), dy: ya / ia.max(1.0) - yb / ib.max(1.0), dw: ra - rb }
}

fn stats(native: &[u8], web: &[u8], lines: &[Line]) -> Vec<Stat> {
    lines.iter().map(|l| line_stat(native, web, l.top as u32, (l.style.line * l.rows as f32) as u32)).collect()
}

fn mean(s: &[Stat], f: impl Fn(&Stat) -> f64) -> f64 {
    s.iter().map(f).sum::<f64>() / s.len().max(1) as f64
}

fn load(p: &Path, h: u32) -> Option<Vec<u8>> {
    let img = image::open(p).ok()?.to_rgba8();
    if img.width() != WIDTH || img.height() != h {
        eprintln!("{}: {}x{}, want {}x{}", p.display(), img.width(), img.height(), WIDTH, h);
        return None;
    }
    Some(img.into_raw())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = PathBuf::from(args.first().cloned().unwrap_or_else(|| "text-parity/out".into()));
    let fit = args.iter().any(|a| a == "--fit");
    let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("text-parity");
    let (lines, h) = spec(&here.join("specimen.txt"));
    let mut t = TextSys::new(&[], false, 2048);
    if let Ok(b) = std::env::var("RR_BASELINE") {
        t.baseline = if b == "blink" { rare_render::text::Baseline::Blink } else { rare_render::text::Baseline::WebKit };
    }
    if let (Ok(g), Ok(c)) = (std::env::var("RR_TEXT_GAMMA"), std::env::var("RR_TEXT_CONTRAST")) {
        t.set_tune(Tune { gamma: g.parse().unwrap(), contrast: c.parse().unwrap(), ..t.tune() });
    }
    if let Ok(m) = std::env::var("RR_HINT_MAX") {
        t.set_tune(Tune { hint: rare_render::text::Hint::Light, hint_max: m.parse().unwrap(), ..t.tune() });
    }
    if let Ok(h) = std::env::var("RR_HINT") {
        t.set_tune(Tune { hint: if h == "none" { rare_render::text::Hint::None } else { rare_render::text::Hint::Light }, ..t.tune() });
    }
    let webs: Vec<(String, Vec<u8>)> = ["chrome", "webkit"].iter().filter_map(|e| Some((e.to_string(), load(&out.join(format!("web-{e}.png")), h)?))).collect();

    if fit {
        for (name, web) in &webs {
            let mut best = (f64::MAX, Tune::default());
            for gi in 0..=16 {
                for ci in 0..=10 {
                    let tune = Tune { gamma: 1.0 + gi as f32 * 0.1, contrast: ci as f32 * 0.1, ..t.tune() };
                    t.set_tune(tune);
                    let native = render(&mut t, &lines, h);
                    let s = stats(&native, web, &lines);
                    let e = mean(&s, |s| s.mae);
                    if e < best.0 {
                        best = (e, tune);
                    }
                }
            }
            println!("{name}: best gamma {:.1} contrast {:.1}, mean error {:.2}", best.1.gamma, best.1.contrast, best.0);
        }
        return;
    }

    let mut native = render(&mut t, &lines, h);
    if args.iter().any(|a| a == "--gpu") {
        // The same quads through the real pipeline (shader.wgsl) on a headless adapter.
        let mut gpu = rare_render::Gpu::headless(WIDTH, h).expect("headless gpu");
        let mut t = TextSys::new(&[], false, gpu.atlas_size);
        t.set_look(rare_render::text::TextLook::from_env());
        let mut list = DrawList::default();
        for l in &lines {
            let mut buf = t.buffer(&l.text, &l.style);
            t.measure(&mut buf, l.width);
            t.paint(&buf, &TextPaint::at(X, l.top, l.color), &mut list);
        }
        t.upload(&mut gpu);
        let (_, _, px) = gpu.capture(&list, Color::rgb(BG[0], BG[1], BG[2]));
        let worst = native.iter().zip(&px).map(|(a, b)| (*a as i32 - *b as i32).abs()).max().unwrap_or(0);
        let off = native.chunks(4).zip(px.chunks(4)).filter(|(a, b)| (0..3).any(|k| (a[k] as i32 - b[k] as i32).abs() > 1)).count();
        println!("gpu vs cpu compositor: worst channel difference {worst}, {off} pixels off by more than 1");
        native = px;
    }
    image::save_buffer(out.join("native.png"), &native, WIDTH, h, image::ExtendedColorType::Rgba8).unwrap();
    let mut md = String::new();
    let tune = t.tune();
    let _ = writeln!(md, "Tune: gamma {:.2}, contrast {:.2}, hint {:?}, baseline {:?}\n", tune.gamma, tune.contrast, tune.hint, t.baseline);
    for (name, web) in &webs {
        let s = stats(&native, web, &lines);
        let mut diff = Vec::with_capacity(native.len());
        for (a, b) in native.chunks(4).zip(web.chunks(4)) {
            for k in 0..3 {
                diff.push(((a[k] as i32 - b[k] as i32).unsigned_abs() * 4).min(255) as u8);
            }
            diff.push(255);
        }
        image::save_buffer(out.join(format!("diff-{name}.png")), &diff, WIDTH, h, image::ExtendedColorType::Rgba8).unwrap();
        let _ = writeln!(md, "### Against {name}\n\n| Line | MAE | Off >2 | Ink | dx | dy | Right edge |\n|---|---|---|---|---|---|---|");
        for (l, s) in lines.iter().zip(&s) {
            let boxed = match (l.width, l.style.truncate, l.style.clamp) {
                (Some(w), true, _) => format!(" truncate {w}"),
                (Some(w), _, 0) => format!(" wrap {w}"),
                (Some(w), _, n) => format!(" clamp-{n} {w}"),
                _ => String::new(),
            };
            let label = format!("{} {} {}/{}{}{}", if l.style.mono { "Fira" } else { "Inter" }, l.style.weight, l.style.size, l.style.line, if l.style.tracking != 0.0 { format!(" {:+}em", l.style.tracking) } else { String::new() }, boxed);
            let _ = writeln!(md, "| {label} | {:.1} | {:.1} % | {:.3} | {:+.2} | {:+.2} | {:+} |", s.mae, s.off * 100.0, s.ink, s.dx, s.dy, s.dw);
        }
        let _ = writeln!(
            md,
            "| **mean** | **{:.1}** | **{:.1} %** | **{:.3}** | {:+.2} | {:+.2} | |\n",
            mean(&s, |s| s.mae),
            mean(&s, |s| s.off) * 100.0,
            mean(&s, |s| s.ink),
            mean(&s, |s| s.dx),
            mean(&s, |s| s.dy)
        );
    }
    let ours = widths(&mut t, &lines);
    for name in ["chrome", "webkit"] {
        let Ok(src) = std::fs::read_to_string(out.join(format!("widths-{name}.txt"))) else { continue };
        let web: Vec<f32> = src.lines().filter_map(|l| l.parse().ok()).collect();
        let d: Vec<f32> = ours.iter().zip(&web).map(|(a, b)| a - b).collect();
        let worst = d.iter().fold(0.0f32, |m, v| if v.abs() > m.abs() { *v } else { m });
        let mean = d.iter().map(|v| v.abs()).sum::<f32>() / d.len().max(1) as f32;
        let _ = writeln!(md, "Layout widths against {name}: mean |diff| {mean:.3} px, worst {worst:+.3} px");
        if std::env::var("RR_WIDTHS").is_ok() {
            for ((l, a), b) in lines.iter().zip(&ours).zip(&web) {
                let _ = writeln!(md, "  {:>9.3} {:>9.3} {:+.3}  {}", a, b, a - b, l.text);
            }
        }
    }
    std::fs::write(out.join("report.md"), &md).unwrap();
    print!("{md}");
}
