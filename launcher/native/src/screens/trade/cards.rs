//! the Trade API's coin row, trending card and perps card (trade/parts.tsx TokenListRow, TokenCard,
//! tokenMeta; RootPage's PerpCard) as view elements.

use super::fmt::{age, fmt_leverage, perp_px};
use super::model::{logo_url, PerpMarket, TokenRow};
use super::ui::{change, coin_logo};
use crate::kit::format::{num_parts, NumKind, NumOptions};
use crate::kit::{nav, num, row_look, tile};
use crate::screens::market::host::*;
use rare_render::view::El;

/// Market cap and age, the meta line under a coin's name ("MC $41.68M, 52m old").
pub fn token_meta(t: &TokenRow, with_age: bool, now_ms: f64) -> String {
    let mc = num_parts(NumKind::Market, t.mcap, NumOptions::default());
    let mut parts: Vec<String> = Vec::new();
    if let Some(p) = &mc {
        parts.push(format!("MC {}{}", p.sign, p.body));
    }
    if with_age && t.created_ms.is_some_and(|c| c != 0.0) {
        parts.push(format!("{} old", age(t.created_ms, now_ms)));
    }
    if mc.is_none() && !with_age {
        parts.push(t.name.clone());
    }
    parts.join(", ")
}

/// `TokenListRow`: avatar, name over one meta line, price and change on the right.
pub fn token_list_row(ui: &mut Ui, t: &TokenRow, meta: Option<String>, tag: u32, def: bool) -> El {
    let logo = logo_url(t.logo.as_deref());
    let names = ui
        .bx(s().col().flex(1.0).min0())
        .child(ui.text(t.symbol.clone(), T::new(Ty::Body).w(600).trunc()))
        .child(ui.text(meta.unwrap_or_else(|| t.name.clone()), T::new(Ty::Label).c(tok::MUTED).trunc()));
    let mut right = ui.bx(s().col().items_end());
    if let Some(p) = num(ui, s(), NumKind::Price, t.price_usd, NumOptions::default(), T::new(Ty::Body).w(500), true) {
        right = right.child(p);
    }
    if let Some(c) = change(ui, t.change24(), T::new(Ty::Label)) {
        right = right.child(c);
    }
    let el = ui.bx(s().row().items_center().gap(0.75).h(3.75).wp(1.0).px(0.75)).radius(ui.r(0.5)).child(coin_logo(ui, logo.as_deref(), &t.symbol, 2.5)).child(names).child(right);
    nav(row_look(el), tag, Some("Trade"), None, def)
}

/// `TokenCard`: a trending card for a rail.
pub fn token_card(ui: &mut Ui, t: &TokenRow, tag: u32, def: bool) -> El {
    let logo = logo_url(t.logo.as_deref());
    let names = ui.bx(s().col().min0()).child(ui.text(t.symbol.clone(), T::new(Ty::Body).w(600).trunc())).child(ui.text(t.name.clone(), T::new(Ty::Label).c(tok::MUTED).trunc()));
    let top = ui.bx(s().row().items_center().gap(0.75)).child(coin_logo(ui, logo.as_deref(), &t.symbol, 2.5)).child(names);
    let mut mid = ui.bx(s().row().items_end().justify_between().gap(0.5).mt(1.0));
    let price = num(ui, s(), NumKind::Price, t.price_usd, NumOptions::default(), T::new(Ty::Row).w(600), true);
    let ch = change(ui, t.change24(), T::new(Ty::Small));
    mid = baseline_pair(ui, mid, price, ch, Ty::Row, Ty::Small);
    let mc = num_parts(NumKind::Market, t.mcap, NumOptions::default()).map(|p| p.body).unwrap_or_default();
    let vol = num_parts(NumKind::Market, t.volume24h, NumOptions::default()).map(|p| p.body).unwrap_or_default();
    let bottom = ui.bx(s().row().justify_between().gap(0.5).mt(0.25)).child(ui.text(format!("MC {mc}"), T::new(Ty::Label).c(tok::MUTED))).child(ui.text(format!("Vol {vol}"), T::new(Ty::Label).c(tok::MUTED)));
    let el = ui.bx(s().col().w(15.0).p(1.0)).bg(tok::L1).radius(ui.r(0.5)).border(1.0, tok::HAIR).child(top).child(mid).child(bottom);
    nav(tile(el, 1.06), tag, Some("Trade"), None, def)
}

/// Two runs on one baseline in a justify-between row (`flex items-baseline justify-between`).
fn baseline_pair(ui: &Ui, row: El, a: Option<El>, b: Option<El>, ta: Ty, tb: Ty) -> El {
    let below = |ty: Ty| {
        let (size, line, _) = ty.metrics();
        let (size, line) = (size * ui.rem, line * ui.rem);
        let half = (line - 1.2109375 * size) / 2.0;
        line - (half + 0.96875 * size)
    };
    let (da, db) = (below(ta), below(tb));
    let m = da.max(db);
    let mut row = row;
    if let Some(a) = a {
        row = row.child(ui.bx(s().mb((m - da) / ui.rem)).child(a));
    }
    if let Some(b) = b {
        row = row.child(ui.bx(s().mb((m - db) / ui.rem)).child(b));
    }
    row
}

/// RootPage's `PerpCard`.
pub fn perp_card(ui: &mut Ui, m: &PerpMarket, logo: Option<&str>, tag: u32) -> El {
    let logo = logo_url(logo);
    let names = ui
        .bx(s().col().min0())
        .child(ui.text(m.symbol.clone(), T::new(Ty::Body).w(600).trunc()))
        .child(ui.text(format!("Up to {}", fmt_leverage(m.max_leverage.unwrap_or(1.0))), T::new(Ty::Label).c(tok::MUTED)));
    let top = ui.bx(s().row().items_center().gap(0.75)).child(coin_logo(ui, logo.as_deref(), &m.symbol, 2.0)).child(names);
    let price = Some(ui.text(perp_px(Some(m.price)), T::new(Ty::Row).w(600).tab()));
    let ch = change(ui, m.change_pct, T::new(Ty::Small));
    let mid = baseline_pair(ui, ui.bx(s().row().items_end().justify_between().gap(0.5).mt(1.0)), price, ch, Ty::Row, Ty::Small);
    let el = ui.bx(s().col().w(13.0).p(1.0)).bg(tok::L1).radius(ui.r(0.5)).border(1.0, tok::HAIR).child(top).child(mid);
    nav(tile(el, 1.06), tag, Some("Trade"), None, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_lines() {
        let t = TokenRow { name: "Moon".into(), mcap: Some(3_800.0), created_ms: Some(1_000.0), ..Default::default() };
        assert_eq!(token_meta(&t, false, 0.0), "MC $3.8K");
        assert_eq!(token_meta(&t, true, 1_000.0 + 52.0 * 60_000.0), "MC $3.8K, 52m old");
        let bare = TokenRow { name: "Bare".into(), ..Default::default() };
        assert_eq!(token_meta(&bare, false, 0.0), "Bare");
    }
}
