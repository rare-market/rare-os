//! Rare Render's `engine/src/theme.rs`, rendered from app.css. The engine keeps a checked-in copy
//! (it must build without this crate); `tests/theme.rs` fails when the copy is stale and rewrites it
//! with `RARE_TW_BLESS=1`.

use crate::appcss::{sheet, Sheet};
use crate::model::{Len, LineHeight, Rgba};
use crate::tokens;
use crate::value;
use std::fmt::Write;

/// Where the engine's copy lives, relative to launcher/crates/rare-tw/core.
pub const ENGINE_THEME: &str = "../../../render/engine/src/theme.rs";

fn upper(name: &str) -> String {
    name.replace('-', "_").to_uppercase()
}

fn f(v: f32) -> String {
    let s = format!("{v:?}");
    if s.contains('.') {
        s
    } else {
        format!("{s}.0")
    }
}

fn color_expr(c: Rgba) -> String {
    let [r, g, b, _] = c.to_u8();
    if (c.a - 1.0).abs() < 1e-6 {
        format!("Color::rgb({r}, {g}, {b})")
    } else {
        format!("Color::rgb({r}, {g}, {b}).alpha({})", f((c.a * 1000.0).round() / 1000.0))
    }
}

fn rem(l: Len) -> f32 {
    match l {
        Len::Rem(v) => v,
        Len::Px(v) => v / 16.0,
        _ => 0.0,
    }
}

/// A `scale(x)` in a rule's transform.
fn scale_of(sh: &Sheet, sel: &str) -> Option<f32> {
    let t = sh.decl(sel, "transform")?;
    value::num(t.strip_prefix("scale(")?.strip_suffix(')')?)
}

fn first_ms(v: &str) -> Option<f32> {
    value::words(v).iter().find_map(|w| value::time_ms(w.trim_end_matches(',')))
}

/// The engine theme module source.
pub fn engine_theme_rs() -> Result<String, String> {
    let sh = sheet();
    let need = |what: &str, v: Option<f32>| v.ok_or_else(|| format!("app.css: {what} not found"));
    let col = |n: &str| tokens::color(n).ok_or_else(|| format!("app.css: --color-{n} not found"));

    // rem: :root { font-size: calc(min(calc(100vw / 80), calc(100vh / 50)) * ...) }
    let fs = sh.root_plain.iter().rev().find(|(k, _)| k == "font-size").map(|(_, v)| v.clone()).ok_or("app.css: :root font-size not found")?;
    let div = |unit: &str| -> Option<f32> {
        let i = fs.find(unit)?;
        value::num(fs[i + unit.len()..].trim_start().strip_prefix('/')?.trim_start().split(')').next()?)
    };
    let (wdiv, hdiv) = (need("100vw / N", div("100vw"))?, need("100vh / N", div("100vh"))?);

    let mut s = String::new();
    let w = &mut s;
    writeln!(w, "//! GENERATED from launcher/src/styles/app.css by rare-tw (launcher/crates/rare-tw/core/src/theme.rs).").unwrap();
    writeln!(w, "//! Do not edit: change app.css, then run").unwrap();
    writeln!(w, "//!   RARE_TW_BLESS=1 cargo test --manifest-path launcher/crates/Cargo.toml -p rare-tw-core --test theme").unwrap();
    writeln!(w, "//! (that test fails while this copy is stale, so a token change reaches both front ends).").unwrap();
    writeln!(w, "//!").unwrap();
    writeln!(w, "//! Layout is in rem against a 1280 x 800 design: rem = min(width / {wdiv}, height / {hdiv}), so the").unwrap();
    writeln!(w, "//! SuiPlay's 1920 x 1200 is 24 px.").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "use crate::color::Color;").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "pub fn rem_for(w: f32, h: f32) -> f32 {{\n    (w / {}).min(h / {})\n}}", f(wdiv), f(hdiv)).unwrap();
    writeln!(w).unwrap();

    // The POC's short names, kept stable for the engine and screens.
    writeln!(w, "/// The core palette (DESIGN.md names).").unwrap();
    writeln!(w, "pub mod tok {{\n    use super::Color;").unwrap();
    for (k, n) in [("BG", "bg"), ("L1", "l1"), ("L2", "l2"), ("L3", "l3"), ("L4", "l4"), ("BRAND", "brand"), ("FG", "fg"), ("MUTED", "muted"), ("POS", "pos"), ("NEG", "neg"), ("ATTN", "warn"), ("HAIR", "hair")] {
        writeln!(w, "    pub const {k}: Color = {};", color_expr(col(n)?)).unwrap();
    }
    writeln!(w, "    /// The SUI mark's blue (launcher/src/ui/sui.tsx).").unwrap();
    writeln!(w, "    pub const SUI: Color = Color::rgb(77, 162, 255);").unwrap();
    writeln!(w, "}}\n").unwrap();

    writeln!(w, "/// Every `--color-*` in app.css's @theme (`bg-l3` is `color::L3`).").unwrap();
    writeln!(w, "pub mod color {{\n    use super::Color;").unwrap();
    for (n, c) in tokens::colors() {
        writeln!(w, "    pub const {}: Color = {};", upper(&n), color_expr(c)).unwrap();
    }
    writeln!(w, "}}\n").unwrap();

    writeln!(w, "/// Type scale in rem (DESIGN.md \"Type\").").unwrap();
    writeln!(w, "pub mod size {{").unwrap();
    for (n, t) in tokens::texts() {
        writeln!(w, "    pub const {}: f32 = {};", upper(&n), f(rem(t.size))).unwrap();
    }
    writeln!(w, "}}\n").unwrap();
    writeln!(w, "/// Line height per type step, in rem.").unwrap();
    writeln!(w, "pub mod line {{").unwrap();
    for (n, t) in tokens::texts() {
        let v = match t.line {
            Some(LineHeight::Len(l)) => rem(l),
            Some(LineHeight::Num(k)) => k * rem(t.size),
            None => continue,
        };
        writeln!(w, "    pub const {}: f32 = {};", upper(&n), f(v)).unwrap();
    }
    writeln!(w, "}}\n").unwrap();
    writeln!(w, "/// Letter spacing per type step, in em (0 where the step has none).").unwrap();
    writeln!(w, "pub mod tracking {{").unwrap();
    for (n, t) in tokens::texts() {
        let v = match t.tracking {
            Some(Len::Em(e)) => e,
            _ => 0.0,
        };
        writeln!(w, "    pub const {}: f32 = {};", upper(&n), f(v)).unwrap();
    }
    writeln!(w, "}}\n").unwrap();

    writeln!(w, "/// Radii in rem.").unwrap();
    writeln!(w, "pub mod radius {{").unwrap();
    for (n, l) in tokens::radii() {
        writeln!(w, "    pub const {}: f32 = {};", upper(&n), f(rem(l))).unwrap();
    }
    writeln!(w, "}}\n").unwrap();

    let focus_ms = need("[data-nav] transition", sh.decl("[data-nav]", "transition").as_deref().and_then(first_ms))?;
    let strip_ms = need(".strip-tile transition", sh.decl(".strip-tile", "transition").as_deref().and_then(first_ms))?;
    let label_ms = need(".strip-label transition", sh.decl(".strip-label", "transition").as_deref().and_then(first_ms))?;
    let dur = |n: &str| need(&format!("--dur-{n}"), tokens::duration(n));
    writeln!(w, "/// Motion tokens in seconds; one curve (`Ease::PRO`, app.css --ease).").unwrap();
    writeln!(w, "pub mod motion {{").unwrap();
    writeln!(w, "    /// Focus ring and focus scale ([data-nav] transition).").unwrap();
    writeln!(w, "    pub const FOCUS: f32 = {};", f(focus_ms / 1000.0)).unwrap();
    writeln!(w, "    pub const FAST: f32 = {};", f(dur("fast")? / 1000.0)).unwrap();
    writeln!(w, "    /// Home's strip tile growth (.strip-tile).").unwrap();
    writeln!(w, "    pub const STRIP: f32 = {};", f(strip_ms / 1000.0)).unwrap();
    writeln!(w, "    /// The strip tile's label fade (.strip-label).").unwrap();
    writeln!(w, "    pub const LABEL: f32 = {};", f(label_ms / 1000.0)).unwrap();
    writeln!(w, "    pub const BASE: f32 = {};", f(dur("base")? / 1000.0)).unwrap();
    writeln!(w, "    pub const SLOW: f32 = {};", f(dur("slow")? / 1000.0)).unwrap();
    writeln!(w, "    pub const TINT: f32 = {};", f(dur("tint")? / 1000.0)).unwrap();
    let ease = sh.root.get("ease").and_then(|e| value::ease(e)).ok_or("app.css: --ease not found")?;
    writeln!(w, "    /// cubic-bezier control points of the one curve.").unwrap();
    writeln!(w, "    pub const EASE: [f32; 4] = [{}, {}, {}, {}];", f(ease[0]), f(ease[1]), f(ease[2]), f(ease[3])).unwrap();
    writeln!(w, "}}\n").unwrap();

    // Focus ring: `0 0 0 GAP rgb(bg), 0 0 0 OUTER rgb(fg)`.
    let ring = sh.decl("[data-nav]:focus", "box-shadow").ok_or("app.css: [data-nav]:focus box-shadow not found")?;
    let layers = value::shadows(&ring).ok_or("app.css: focus ring box-shadow")?;
    let spread = |i: usize| layers.get(i).map(|l| rem(l.spread)).ok_or("app.css: focus ring layers");
    writeln!(w, "/// The focus ring: a {} rem gap in the page colour, then white out to {} rem.", f(spread(0)?), f(spread(1)?)).unwrap();
    writeln!(w, "pub mod ring {{").unwrap();
    writeln!(w, "    pub const GAP: f32 = {};", f(spread(0)?)).unwrap();
    writeln!(w, "    pub const OUTER: f32 = {};", f(spread(1)?)).unwrap();
    writeln!(w, "}}\n").unwrap();

    let shift = sh
        .decl(".strip-tile[data-nav]:focus ~ .strip-tile", "transform")
        .and_then(|t| value::len(t.strip_prefix("translateX(")?.strip_suffix(')')?))
        .ok_or("app.css: strip shift not found")?;
    writeln!(w, "/// What focus does to tiles and buttons (app.css [data-nav] rules).").unwrap();
    writeln!(w, "pub mod focus {{").unwrap();
    writeln!(w, "    /// `.tile`").unwrap();
    writeln!(w, "    pub const TILE_SCALE: f32 = {};", f(need(".tile scale", scale_of(sh, ".tile[data-nav]:focus"))?)).unwrap();
    writeln!(w, "    /// `.tile.tile-wide`").unwrap();
    writeln!(w, "    pub const TILE_WIDE_SCALE: f32 = {};", f(need(".tile-wide scale", scale_of(sh, ".tile.tile-wide[data-nav]:focus"))?)).unwrap();
    writeln!(w, "    /// `.btn` (also fills fg with bg text)").unwrap();
    writeln!(w, "    pub const BTN_SCALE: f32 = {};", f(need(".btn scale", scale_of(sh, ".btn[data-nav]:focus"))?)).unwrap();
    writeln!(w, "    /// `.strip-tile` grows from its top-left corner").unwrap();
    writeln!(w, "    pub const STRIP_SCALE: f32 = {};", f(need(".strip-tile scale", scale_of(sh, ".strip-tile[data-nav]:focus"))?)).unwrap();
    writeln!(w, "    /// and every later strip tile slides right by this, in rem").unwrap();
    writeln!(w, "    pub const STRIP_SHIFT: f32 = {};", f(rem(shift))).unwrap();
    writeln!(w, "}}").unwrap();
    Ok(s)
}
