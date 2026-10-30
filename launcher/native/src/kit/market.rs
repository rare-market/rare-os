//! rare.market's display pieces (marketplace/parts.tsx, ui/Thumb.tsx) and the Trade API's StatLine: `Sui`
//! amounts, `Delta` with the triangle, `Avatar`, `Thumb`, `Verified`, `StatLine`.

use super::art::{art, ArtFit};
use super::format::{fmt_pct, fmt_sui, mist_to_sui};
use super::icon::{svg_box, svg_tinted, VERIFIED};
use super::tok;
use super::ui::{s, Ty, Ui, S, T};
use rare_render::view::El;

/// `Sui`: the amount in the text's look, "SUI" (ml-1, normal weight, secondary) after it; an
/// unknown amount is "--" in the secondary colour.
pub fn sui(ui: &mut Ui, st: S, mist: Option<u128>, dp: usize, t: T) -> El {
    let none = mist_to_sui(mist).is_none();
    let mut row = ui.bx(st.row().items_end()).child(ui.text(fmt_sui(mist, dp, false, false), if none { t.tab().c(tok::MUTED) } else { t.tab() }));
    if !none {
        row = row.child(ui.text_s(s().ml(0.25), "SUI", t.w(400).c(tok::MUTED)));
    }
    row
}

/// `Delta`: a small triangle and the percent, green up, red down, secondary flat; unknown is an
/// empty cell (never a dash).
pub fn delta(ui: &mut Ui, st: S, value: Option<f64>, dp: usize, t: T) -> El {
    let row = ui.bx(st.row().items_center());
    let Some(v) = value.filter(|v| v.is_finite()) else { return row };
    let c = if v > 0.0 {
        tok::POS
    } else if v < 0.0 {
        tok::NEG
    } else {
        tok::MUTED
    };
    let mut row = row;
    if v != 0.0 {
        let path = if v > 0.0 { "M4 1 7.5 7h-7z" } else { "M4 7 .5 1h7z" };
        let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><path d="{path}" fill="white"/></svg>"#);
        let m = ui.r(0.125);
        let tri = svg_tinted(ui, if v > 0.0 { "tri-up" } else { "tri-down" }, svg, 0.5, 0.5, c).style(move |st| st.margin.right = rare_render::taffy::prelude::length(m));
        row = row.child(tri);
    }
    row.child(ui.text(fmt_pct(Some(v), dp), t.tab().c(c)))
}

/// The verified badge.
pub fn verified(ui: &mut Ui, size: f32) -> El {
    svg_box(ui, "verified", VERIFIED.to_string(), size, size, s())
}

/// The first letter or digit of a name, upper-cased (Thumb's fallback).
pub fn initial(label: &str) -> String {
    label.trim().chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_default()
}

/// `Avatar`: round (or `square`, rounded-md) art on layer 03; the initial when it has none.
pub fn avatar(ui: &mut Ui, st: S, src: Option<&str>, label: &str, size: f32, square: bool) -> El {
    let r = if square { ui.r(0.375) } else { ui.r(size / 2.0) };
    let fb = ui.bx(s().fill().row().items_center().justify_center()).child(ui.text(initial(label), T::new(Ty::Px(size * 0.4, size * 0.5)).w(600).c(tok::fg(0.35))));
    let sources: Vec<String> = src.into_iter().map(String::from).collect();
    let img = art(ui, s().fill(), &sources, size, size, ArtFit::Cover(0.5), Some(fb)).radius(r);
    ui.bx(st.size(size)).bg(tok::L3).radius(r).clip().child(img)
}

/// `Thumb`: remote art that fades in, or a quiet tile with the name's initial (text-[1.4rem]
/// semibold fg/35). `w` x `h` rem, rounded `radius` rem.
#[allow(clippy::too_many_arguments)]
pub fn thumb(ui: &mut Ui, st: S, src: Option<&str>, label: &str, w: f32, h: f32, radius: f32, contain: bool) -> El {
    let fb = ui.bx(s().fill().row().items_center().justify_center()).child(ui.text(initial(label), T::new(Ty::px(1.4)).w(600).c(tok::fg(0.35))));
    let sources: Vec<String> = src.into_iter().map(String::from).collect();
    let fit = if contain { ArtFit::Contain(0.5, 0.5) } else { ArtFit::Cover(0.5) };
    let img = art(ui, s().fill(), &sources, w, h, fit, Some(fb)).radius(ui.r(radius));
    ui.bx(st.wh(w, h)).bg(tok::L3).radius(ui.r(radius)).clip().child(img)
}

pub struct StatItem {
    pub label: String,
    pub value: String,
}

/// A stat line: label over value, a hairline between items; empty values are left out,
/// and with none left it draws nothing.
pub fn stat_line(ui: &mut Ui, st: S, items: &[StatItem], fill: bool, lg: bool) -> Option<El> {
    let shown: Vec<&StatItem> = items.iter().filter(|i| !i.value.is_empty()).collect();
    if shown.is_empty() {
        return None;
    }
    let mut row = ui.bx(st.row());
    for (i, it) in shown.iter().enumerate() {
        let mut cs = s().col().min0();
        if fill {
            cs = cs.grow();
        }
        if i > 0 {
            cs = cs.pl(1.0);
        }
        if i + 1 < shown.len() {
            cs = cs.pr(1.0);
        }
        let mut c = ui.bx(cs);
        if i > 0 {
            c = c.child(ui.bx(s().abs(Some(0.0), None, Some(0.0), Some(0.0)).w_px(1.0)).bg(tok::HAIR));
        }
        c = c.child(ui.text(it.label.clone(), T::new(Ty::Label).c(tok::MUTED))).child(ui.text(it.value.clone(), T::new(if lg { Ty::Row } else { Ty::Body }).w(500).tab().trunc()));
        row = row.child(c);
    }
    Some(row)
}
