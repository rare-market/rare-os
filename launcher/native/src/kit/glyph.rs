//! Controller glyphs and hints (ui/controls.tsx `Glyph`, `Hint`): the Xbox layout InputPlumber
//! presents, plus the SuiPlay's Rare and Quick keys and the LC / RC tabs. Keys are the engine's
//! hint keys (`rare_render::focus::hints::Key`), so the hint bar and the glyphs speak one language.

use super::icon::{svg_box, svg_tinted};
use super::tok;
use super::ui::{s, Ty, Ui, T};
use rare_render::view::El;

pub use rare_render::focus::hints::Key as Pb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GSize {
    Sm,
    Md,
}

const START_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12"><path d="M2 3h8M2 6h8M2 9h8" stroke="white" stroke-width="1.6" stroke-linecap="round"/></svg>"#;
const SELECT_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12"><rect x="1.5" y="1.5" width="6" height="6" rx="1" fill="none" stroke="white" stroke-width="1.3"/><rect x="4.5" y="4.5" width="6" height="6" rx="1" fill="white"/></svg>"#;
const QUICK_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12"><circle cx="2.4" cy="6" r="1.05" fill="white"/><circle cx="6" cy="6" r="1.05" fill="white"/><circle cx="9.6" cy="6" r="1.05" fill="white"/></svg>"#;
const RARE_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12"><path d="M6 1.2 10.2 4.6 6 10.8 1.8 4.6Z" fill="rgb(242,242,243)"/><path d="M1.8 4.6h8.4M4.2 4.6 6 1.2l1.8 3.4" fill="none" stroke="rgb(16,12,24)" stroke-width="0.7" stroke-linejoin="round"/></svg>"#;

/// `Glyph b size`.
pub fn glyph(ui: &mut Ui, b: Pb, size: GSize) -> El {
    let (d, fs) = if size == GSize::Sm { (1.2, 0.62) } else { (1.45, 0.72) };
    let round = |ui: &Ui| ui.bx(s().row().items_center().justify_center().size(d)).radius(ui.r(d / 2.0));
    match b {
        Pb::A | Pb::B | Pb::X | Pb::Y => {
            let fill = if b == Pb::A { tok::FG } else { tok::fg(0.85) };
            let l = match b {
                Pb::A => "A",
                Pb::B => "B",
                Pb::X => "X",
                _ => "Y",
            };
            round(ui).bg(fill).child(ui.text(l, T::new(Ty::Px(fs, fs)).w(700).c(tok::BG)))
        }
        Pb::Lb | Pb::Rb | Pb::Lt | Pb::Rt => {
            let (h, fs) = if size == GSize::Sm { (1.1, 0.58) } else { (1.3, 0.66) };
            let l = match b {
                Pb::Lb => "LB",
                Pb::Rb => "RB",
                Pb::Lt => "LT",
                _ => "RT",
            };
            ui.bx(s().row().items_center().h(h).px(0.4)).bg(tok::fg(0.12)).radius(ui.r(0.4)).child(ui.text(l, T::new(Ty::Px(fs, fs)).w(700).c(tok::fg(0.9)).track(0.025)))
        }
        Pb::Start | Pb::Select => {
            let (name, svg) = if b == Pb::Start { ("g-start", START_SVG) } else { ("g-select", SELECT_SVG) };
            let ic = svg_tinted(ui, name, svg.to_string(), 0.62, 0.62, tok::FG);
            round(ui).border(1.0, tok::fg(0.4)).child(ic)
        }
        Pb::Rare | Pb::Guide | Pb::GuideHold => {
            let ic = svg_box(ui, "g-rare", RARE_SVG.to_string(), 0.72, 0.72, s());
            round(ui).border(1.0, tok::fg(0.7)).child(ic)
        }
        Pb::Quick | Pb::Qam => {
            let ic = svg_tinted(ui, "g-quick", QUICK_SVG.to_string(), 0.72, 0.72, tok::FG);
            round(ui).border(1.0, tok::fg(0.7)).child(ic)
        }
        Pb::Lc | Pb::Rc => {
            // The small keys above each shoulder: a short tab with the outer corner cut.
            let left = b == Pb::Lc;
            let w = if size == GSize::Sm { 1.55 } else { 1.85 };
            let path = if left { "M9 1.2h19a1.3 1.3 0 0 1 1.3 1.3v13a1.3 1.3 0 0 1-1.3 1.3H2.2Z" } else { "M21 1.2H2a1.3 1.3 0 0 0-1.3 1.3v13A1.3 1.3 0 0 0 2 16.8h25.8Z" };
            let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 30 18"><path d="{path}" fill="none" stroke="rgba(242,242,243,0.7)" stroke-width="1.3" stroke-linejoin="round"/><text x="{}" y="12.2" text-anchor="middle" font-size="8.5" font-weight="700" font-family="Inter" fill="rgb(242,242,243)">{}</text></svg>"#, if left { 17 } else { 13 }, if left { "LC" } else { "RC" });
            svg_box(ui, if left { "g-lc" } else { "g-rc" }, svg, w, w * 0.6, s())
        }
        Pb::LbRb | Pb::LtRt | Pb::ExitHold => {
            let (a, c) = match b {
                Pb::LbRb => (Pb::Lb, Pb::Rb),
                Pb::LtRt => (Pb::Lt, Pb::Rt),
                _ => (Pb::Select, Pb::Start),
            };
            let (ga, gc) = (glyph(ui, a, size), glyph(ui, c, size));
            ui.bx(s().row().items_center().gap(0.2)).child(ga).child(gc)
        }
        Pb::Up | Pb::Down | Pb::Left | Pb::Right => {
            let l = b.name()[..1].to_uppercase();
            round(ui).border(1.0, tok::fg(0.4)).child(ui.text(l, T::new(Ty::Px(0.6, 0.6)).c(tok::FG)))
        }
    }
}

/// A glyph and its label, as in the hint bar (`t` is the label's look; it inherits the bar's).
pub fn hint(ui: &mut Ui, b: Pb, label: &str, t: T) -> El {
    let g = glyph(ui, b, GSize::Md);
    ui.bx(s().row().items_center().gap(0.45)).child(g).child(ui.text(label, t))
}
