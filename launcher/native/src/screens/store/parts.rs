//! Store pieces (store/parts.tsx): the logo-first store tile Sui Games uses, GameLogo, the price
//! line with the old price struck through, and a stat line whose values can be elements.

use super::logic::{card_status, on_sale, price_now, studio_line, usd_text};
use super::model::StoreGame;
use crate::kit::{self, tok, ArtFit, DotTone};
use crate::screens::market::host::*;
use rare_render::view::El;

/// `GameLogo`: the logo cover-fit on layer 03 (rounded-card), or the name's first letter
/// (text-[1.25rem] semibold fg/50) when there is none or it fails. `size` rem.
pub fn game_logo(ui: &mut Ui, g: &StoreGame, size: f32) -> El {
    let first: String = g.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    let fb = ui.bx(s().fill().row().items_center().justify_center()).child(ui.text(first, T::new(Ty::px(1.25)).w(600).c(tok::fg(0.5))));
    let sources: Vec<String> = g.images.logo.iter().filter(|l| !l.is_empty()).cloned().collect();
    let img = kit::art(ui, s().fill(), &sources, size, size, ArtFit::Cover(0.5), Some(fb));
    ui.bx(s().size(size)).bg(tok::L3).radius(ui.r(0.5)).clip().child(img)
}

/// Inter's line-through: about 0.37 em above the baseline, 0.075 em thick.
fn strike(ui: &Ui, text: String, t: T) -> El {
    const ASC: f32 = 0.96875;
    const DESC: f32 = 0.2421875;
    let (size, line, _) = t.ty.metrics();
    let (size, line) = (size * ui.rem, t.leading.unwrap_or(line) * ui.rem);
    let base = (line.round() - (ASC + DESC) * size) / 2.0 + ASC * size;
    let th = (size * 0.075).round().max(1.0);
    let y = (base - 0.37 * size - th / 2.0).round();
    let st = rare_render::taffy::prelude::length(y);
    let bar = ui.bx(s().abs(None, Some(0.0), None, Some(0.0)).h_px(th)).bg(t.color).style(move |s| s.inset.top = st);
    ui.bx(s()).child(ui.text(text, t)).child(bar)
}

/// `PriceText`: "$14.99", or the sale price with the old one struck through and muted. `ty` is
/// the inherited size. None without a price.
pub fn price_text(ui: &mut Ui, st: S, g: &StoreGame, ty: Ty, now: f64) -> Option<El> {
    let p = price_now(g, now)?;
    let t = T::new(ty).tab();
    let mut runs = vec![(t.w(500), usd_text(p))];
    let sale = on_sale(g, now);
    if sale {
        runs.push((t.c(tok::MUTED), usd_text(g.price.as_ref().map(|x| x.usd).unwrap_or(0))));
    }
    if !sale {
        return Some(baseline_row(ui, st.gap(0.45), runs));
    }
    // The struck price: the same row, its second run wrapped with the bar.
    let (t2, old) = runs.pop().unwrap();
    let mut row = baseline_row(ui, st.gap(0.45), runs);
    row = row.child(strike(ui, old, t2));
    Some(row)
}

/// `StoreCard`: logo, name, studio and genre; status and price (or "In your library") below.
/// `wide` is the featured grid's tile (w-full, scale 1.03), else the shelf's 20rem tile.
pub fn store_card(ui: &mut Ui, g: &StoreGame, owned: bool, wide: bool, def: bool, tag: u32, now: f64) -> El {
    let names = ui
        .bx(s().col().min0().shrink())
        .child(trunc_text(ui, s(), g.name.clone(), T::new(Ty::Row).w(600)))
        .child(trunc_text(ui, s(), studio_line(g), T::new(Ty::Small).c(tok::MUTED)));
    let top = ui.bx(s().row().items_center().gap(1.0).min0()).child(game_logo(ui, g, 3.5)).child(names);
    let mut left = ui.bx(s().row().items_center().gap(0.45).min0().shrink());
    if g.status.as_deref() == Some("live") {
        left = left.child(kit::dot(ui, s(), DotTone::Primary, false));
    }
    left = left.child(trunc_text(ui, s(), card_status(g), T::new(Ty::Meta).c(tok::MUTED)));
    let right = if owned { Some(ui.text("In your library", T::new(Ty::Meta).c(tok::MUTED))) } else { price_text(ui, s(), g, Ty::Meta, now) };
    let bottom = ui.bx(s().row().items_center().justify_between().gap(0.75).min0()).child(left).maybe(right);
    // The 1px border sits inside the box in CSS (border-box): it adds to the padding.
    let mut st = s().col().justify_between().h(8.0).p(1.0 + 1.0 / ui.rem);
    st = if wide { st.wp(1.0).min0() } else { st.w(20.0) };
    let card = ui.bx(st).bg(tok::L1).radius(ui.r(0.5)).border(1.0, tok::HAIR).child(top).child(bottom).key(rare_render::view::key(&g.slug));
    kit::nav(kit::tile(card, if wide { 1.03 } else { 1.06 }), tag, Some("Details"), None, def)
}

/// One fact of the game page's stat line: plain text, or an element (the price, `.num` version).
#[allow(clippy::large_enum_variant)]
pub enum Fact {
    Text(String),
    El(El),
}

/// StatLine size="lg": label over value, a hairline between items, each item flex-1.
pub fn fact_line(ui: &mut Ui, st: S, items: Vec<(&str, Option<Fact>)>) -> Option<El> {
    let shown: Vec<(&str, Fact)> = items.into_iter().filter_map(|(l, v)| v.map(|v| (l, v))).collect();
    if shown.is_empty() {
        return None;
    }
    let n = shown.len();
    let mut row = ui.bx(st.row());
    for (i, (label, v)) in shown.into_iter().enumerate() {
        let mut cs = s().col().min0().grow();
        if i > 0 {
            cs = cs.pl(1.0);
        }
        if i + 1 < n {
            cs = cs.pr(1.0);
        }
        let mut c = ui.bx(cs);
        if i > 0 {
            c = c.child(ui.bx(s().abs(Some(0.0), None, Some(0.0), Some(0.0)).w_px(1.0)).bg(tok::HAIR));
        }
        let value = match v {
            Fact::Text(t) => trunc_text(ui, s(), t, T::new(Ty::Row).w(500).tab()),
            Fact::El(e) => e,
        };
        c = c.child(ui.text(label, T::new(Ty::Label).c(tok::MUTED))).child(value);
        row = row.child(c);
    }
    Some(row)
}

/// A `truncate` text. Taffy rounds the final box to whole pixels, which can leave it a fraction
/// narrower than the text it measured, and the second layout pass then cuts it with an
/// ellipsis; 1px of right padding keeps a text that fits whole (engine request filed).
pub fn trunc_text(ui: &Ui, st: S, text: impl Into<String>, t: T) -> El {
    ui.text_s(st.pr(1.0 / ui.rem), text, t.trunc())
}
