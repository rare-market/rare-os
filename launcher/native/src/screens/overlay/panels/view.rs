//! Drawing for the panels (overlay/panels.tsx ItemList, MarketList, Trophies, ItemDetail).

use super::money::{amount, fmt_sui, short_name, venue_name, Fmt};
use super::trade::{offer_net_mist, Offer};
use super::trophies::{sort_rows, RowFilter, RowState};
use super::{Account, OfferState, PanelData, Phase};
use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::theme::tok;
use rare_render::{Color, Fit, Id, ImgReq, Node, Src, TextStyle};

// Focus groups (data-nav-group).
const G_ITEMS: u32 = 10;
const G_MARKET: u32 = 11;
const G_RARE: u32 = 12;
const G_GAME: u32 = 13;
const G_PRICE: u32 = 14;
const G_ACT: u32 = 15;

pub const T_GAME_OBJECT: u32 = 1000;

fn tab(cx: &mut Cx, size: (f32, f32), weight: u16) -> TextStyle {
    ts(cx.r(), size.0, size.1, weight)
}

/// A text node with its own colour and optional tabular figures.
fn label(cx: &mut Cx, parent: Id, t: (f32, f32), weight: u16, color: Color, tabular: bool, v: &str) -> Id {
    let mut st = tab(cx, t, weight);
    if tabular {
        st = st.tabular();
    }
    cx.scene.add_text(cx.t, parent, s().done(), v, st, color)
}

fn wrapped(cx: &mut Cx, parent: Id, t: (f32, f32), weight: u16, color: Color, v: &str) -> Id {
    let st = tab(cx, t, weight).wrap();
    cx.scene.add_text(cx.t, parent, s().done(), v, st, color)
}

/// panels.tsx Calm.
fn calm(cx: &mut Cx, parent: Id, v: &str) {
    let r = cx.r();
    let b = cx.scene.add(parent, s().col().px(0.75 * r).py(1.5 * r).done(), Node::default());
    wrapped(cx, b, ty::SMALL, 400, tok::MUTED, v);
}

fn price_text(mist: Option<i128>) -> Option<String> {
    mist.map(|m| fmt_sui(Some(m), Fmt::compact(2)))
}

/// The art box: bg-l3, rounded-card, the image covering it, else the name's initial (MImg's fallback).
fn thumb(cx: &mut Cx, parent: Id, rem: f32, src: &Option<Src>, name: &str) -> Id {
    let r = cx.r();
    let w = rem * r;
    let b = cx.scene.add(parent, s().wh(w, w).done(), Node::bg(tok::L3).radius(0.5 * r).clip());
    thumb_fill(cx, b, src, name, w);
    b
}

fn thumb_fill(cx: &mut Cx, b: Id, src: &Option<Src>, name: &str, px: f32) {
    let r = cx.r();
    match src {
        Some(src) => {
            let k = cx.img.request(ImgReq { src: src.clone(), w: px.round() as u32, h: px.round() as u32, fit: Fit::Cover { fy: 0.5 } });
            cx.scene.add(b, s().fill().done(), Node::default().image(k));
        }
        None => {
            let c = name.trim().chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_default();
            let f = cx.scene.add(b, s().fill().items_center().justify_center().done(), Node::bg(tok::L3));
            if !c.is_empty() {
                cx.scene.add_text(cx.t, f, s().done(), &c, ts(r, 1.6, 2.0, 600), tok::MUTED.alpha(0.6));
            }
        }
    }
}

/// A `Row` (trow grid): 3.5rem high, rounded-card, px 0.75rem, 0.5rem between cells.
fn trow(cx: &mut Cx, parent: Id) -> Id {
    let r = cx.r();
    cx.scene.add(parent, s().row().items_center().gap(0.5 * r).h(3.5 * r).wp(1.0).px(0.75 * r).done(), Node::default().radius(0.5 * r))
}

fn row_focus(id: Id, group: u32, tag: u32, hint: &str) -> Focusable {
    Focusable { id, kind: Kind::Row, group, hint: hint.to_string(), tag, sid: String::new() }
}

/// Title (font-medium) over a muted label line, both truncated.
fn name_col(cx: &mut Cx, row: Id, title: &str, title_color: Color, sub: Option<&str>) {
    let col = cx.scene.add(row, s().col().grow().done(), Node::default());
    let st = tab(cx, ty::SMALL, 500).truncate();
    cx.scene.add_text(cx.t, col, s().done(), title, st, title_color);
    if let Some(sb) = sub {
        let st = tab(cx, ty::LABEL, 400).truncate();
        cx.scene.add_text(cx.t, col, s().done(), sb, st, tok::MUTED);
    }
}

fn price_cell(cx: &mut Cx, row: Id, v: Option<String>) {
    let r = cx.r();
    let c = cx.scene.add(row, s().row().justify_end().w(3.9 * r).done(), Node::default().clip());
    if let Some(v) = v {
        label(cx, c, ty::SMALL, 400, tok::FG, true, &v);
    }
}

fn head_row(cx: &mut Cx, parent: Id, cols: &[&str]) {
    let r = cx.r();
    let h = cx.scene.add(parent, s().row().items_center().gap(0.5 * r).h(2.0 * r).wp(1.0).px(0.75 * r).done(), Node::default());
    cx.scene.add(h, s().w(2.6 * r).done(), Node::default());
    let c = cx.scene.add(h, s().grow().done(), Node::default());
    label(cx, c, ty::LABEL, 500, tok::MUTED, false, cols[0]);
    for l in &cols[1..] {
        let c = cx.scene.add(h, s().row().justify_end().w(3.9 * r).done(), Node::default());
        label(cx, c, ty::LABEL, 500, tok::MUTED, false, l);
    }
}

pub fn items(cx: &mut Cx, nav: &mut Nav, parent: Id, d: &PanelData, _now: f64) {
    let r = cx.r();
    let Some(items) = &d.items else { return };
    if items.is_empty() {
        calm(cx, parent, "You don't hold any items from this game yet. The Market tab has some for sale.");
        return;
    }
    head_row(cx, parent, &["Item", "Floor", "Offer", "Last"]);
    let group = cx.scene.add(parent, s().col().done(), Node::default());
    for (i, it) in items.iter().enumerate() {
        let row = trow(cx, group);
        thumb(cx, row, 2.6, &it.image, &it.name);
        let sub = match &it.listing {
            Some(l) => format!("Listed for {} SUI", fmt_sui(Some(l.price_mist), Fmt::trim(2))),
            None => it.collection.clone().unwrap_or_default(),
        };
        name_col(cx, row, short_name(&it.name, it.collection.as_deref()), tok::FG, Some(&sub));
        price_cell(cx, row, price_text(it.floor_mist));
        price_cell(cx, row, price_text(it.top_offer_mist));
        price_cell(cx, row, price_text(it.last_sale_mist));
        nav.add(row_focus(row, G_ITEMS, i as u32, "Sell or list"));
    }
    let n = cx.scene.add(parent, s().col().px(0.75 * r).pt(0.6 * r).done(), Node::default());
    label(cx, n, ty::LABEL, 400, tok::MUTED, false, "Prices in SUI.");
}

/// A SUI amount: the number, then the unit muted at the same size.
fn sui_amount(cx: &mut Cx, parent: Id, mist: i128, t: (f32, f32), weight: u16, color: Color, f: Fmt) -> Id {
    let r = cx.r();
    let line = cx.scene.add(parent, s().row().items_center().gap(0.25 * r).done(), Node::default());
    let v = fmt_sui(Some(mist), f);
    label(cx, line, t, weight, color, true, &v);
    label(cx, line, t, 400, tok::MUTED, false, "SUI");
    line
}

pub fn market(cx: &mut Cx, nav: &mut Nav, parent: Id, d: &PanelData, _now: f64) {
    let Some(items) = &d.market else { return };
    if items.is_empty() {
        calm(cx, parent, "Nothing from this game is listed right now.");
        return;
    }
    let group = cx.scene.add(parent, s().col().done(), Node::default());
    for (i, it) in items.iter().enumerate() {
        let row = trow(cx, group);
        thumb(cx, row, 2.6, &it.image, &it.name);
        let mut sub = it.collection.clone().unwrap_or_default();
        if let Some(l) = &it.listing {
            if !l.venue.is_empty() {
                sub.push_str(&format!(" on {}", venue_name(&l.venue)));
            }
        }
        name_col(cx, row, short_name(&it.name, it.collection.as_deref()), tok::FG, Some(&sub));
        match it.buy_price() {
            Some(m) => {
                let slot = cx.scene.add(row, s().row().no_shrink_().done(), Node::default());
                sui_amount(cx, slot, m, ty::SMALL, 500, tok::FG, Fmt::dp(3));
            }
            None => {
                label(cx, row, ty::SMALL, 500, tok::MUTED, true, "--");
            }
        }
        nav.add(row_focus(row, G_MARKET, i as u32, "Buy"));
    }
}

pub fn trophies(cx: &mut Cx, nav: &mut Nav, parent: Id, d: &PanelData, _now: f64) {
    let r = cx.r();
    let rows = d.rare.as_ref().map(|g| sort_rows(&g.rows, RowFilter::All)).unwrap_or_default();
    if d.trophies_ready && d.trophies.is_empty() && rows.is_empty() {
        calm(cx, parent, &format!("No trophies from {} yet. Achievements and rewards the game gives you show up here.", d.game));
        return;
    }
    let col = cx.scene.add(parent, s().col().gap(1.0 * r).done(), Node::default());
    if let (false, Some(g)) = (rows.is_empty(), &d.rare) {
        let sec = cx.scene.add(col, s().col().done(), Node::default());
        let head = cx.scene.add(sec, s().col().px(0.75 * r).pb(0.3 * r).done(), Node::default());
        label(cx, head, ty::LABEL, 400, tok::MUTED, false, &trophy_head(g.sum.unlocked, g.sum.total, g.sum.minted));
        for (i, row_) in rows.iter().enumerate() {
            let row = trow(cx, sec);
            let locked = row_.state == RowState::Locked;
            let icon = if locked { row_.icon_gray_url.as_ref().or(row_.icon_url.as_ref()) } else { row_.icon_url.as_ref() };
            let art = icon.and_then(|_| g.art.get(&row_.key)).cloned();
            thumb(cx, row, 2.6, &art, &row_.name);
            let col_ = cx.scene.add(row, s().col().grow().done(), Node::default());
            let st = tab(cx, ty::SMALL, 500).truncate();
            let title = if row_.masked { "Hidden trophy" } else { row_.name.as_str() };
            cx.scene.add_text(cx.t, col_, s().done(), title, st, if locked { tok::MUTED } else { tok::FG });
            if !row_.masked && !row_.description.is_empty() {
                let st = tab(cx, ty::LABEL, 400).truncate();
                cx.scene.add_text(cx.t, col_, s().done(), &row_.description, st, tok::MUTED);
            }
            label(cx, row, ty::LABEL, 400, tok::MUTED, false, row_.state.label());
            nav.add(row_focus(row, G_RARE, i as u32, ""));
        }
    }
    if !d.trophies.is_empty() {
        let sec = cx.scene.add(col, s().col().done(), Node::default());
        if !rows.is_empty() {
            let h = cx.scene.add(sec, s().col().px(0.75 * r).pb(0.4 * r).done(), Node::default());
            label(cx, h, ty::LABEL, 400, tok::MUTED, false, "From the game");
        }
        let grid = cx.scene.add(sec, s().row().wrap().gap(0.75 * r).px(0.4 * r).done(), Node::default());
        // Three columns across the body: 32rem aside, 1px rule, 0.65rem padding each side, 0.4rem inset.
        let w = ((32.0 - 1.3 - 0.8 - 1.5) * r - 1.0) / 3.0;
        for (i, t) in d.trophies.iter().enumerate() {
            let tile = cx.scene.add(grid, s().col().w(w).done(), Node::bg(tok::L2).radius(0.5 * r).clip());
            let art = cx.scene.add(tile, s().wh(w, w).done(), Node::bg(tok::L3).clip());
            thumb_fill(cx, art, &t.art, &t.name, w);
            let nm = cx.scene.add(tile, s().col().px(0.6 * r).py(0.5 * r).done(), Node::default());
            let st = tab(cx, ty::META, 500).truncate();
            cx.scene.add_text(cx.t, nm, s().done(), &t.name, st, tok::FG);
            nav.add(Focusable { id: tile, kind: Kind::Tile { scale: 1.06 }, group: G_GAME, hint: String::new(), tag: T_GAME_OBJECT + i as u32, sid: String::new() });
        }
    }
}

/// "3 of 8 unlocked, 1 minted".
pub fn trophy_head(unlocked: usize, total: usize, minted: usize) -> String {
    format!("{unlocked} of {total} unlocked{}", if minted > 0 { format!(", {minted} minted") } else { String::new() })
}

/// The listing price choices: Floor, Floor +10%, and the current listing when it differs.
pub fn price_choices(floor: Option<i128>, listed: Option<i128>) -> Vec<(&'static str, &'static str, i128)> {
    let mut v = Vec::new();
    if let Some(f) = floor.filter(|f| *f != 0) {
        v.push(("Floor", "floor", f));
        v.push(("Floor +10%", "floor+10", f * 110 / 100));
    }
    if let Some(l) = listed.filter(|l| *l != 0 && Some(*l) != floor) {
        v.push(("Current", "current", l));
    }
    v
}

/// The line under the Sell button.
pub fn sell_note(o: &Offer, net: i128) -> String {
    if net < o.price_mist {
        format!("Best offer {} SUI on {}. You get {} SUI after fees and royalty.", amount(o.price_mist), venue_name(&o.venue), amount(net))
    } else {
        format!("Best offer on {}. You get the full price.", venue_name(&o.venue))
    }
}

/// StatLine: label over value with a hairline between items; items without a value are left out.
fn stat_line(cx: &mut Cx, parent: Id, items: &[(&str, Option<i128>)]) {
    let r = cx.r();
    let shown: Vec<_> = items.iter().filter(|(_, v)| v.is_some()).collect();
    if shown.is_empty() {
        return;
    }
    let dl = cx.scene.add(parent, s().row().done(), Node::default());
    let n = shown.len();
    for (i, (l, v)) in shown.iter().enumerate() {
        let pl = if i > 0 { 1.0 * r } else { 0.0 };
        let pr = if i < n - 1 { 1.0 * r } else { 0.0 };
        let cell = cx.scene.add(dl, s().col().grow().pad(pl, 0.0, pr, 0.0).done(), Node::default());
        if i > 0 {
            edge(cx, cell, 3, tok::HAIR);
        }
        label(cx, cell, ty::LABEL, 400, tok::MUTED, false, l);
        sui_amount(cx, cell, v.unwrap(), ty::BODY, 500, tok::FG, Fmt::trim(3));
    }
}

fn section_label(cx: &mut Cx, parent: Id, v: &str) {
    label(cx, parent, ty::LABEL, 500, tok::MUTED, false, v);
}

fn disabled(cx: &mut Cx, f: &Focusable) {
    cx.scene.n(f.id).opacity.snap(0.4);
}

pub fn detail(cx: &mut Cx, nav: &mut Nav, parent: Id, d: &PanelData, index: usize, now: f64) {
    let r = cx.r();
    let Some(it) = d.items.as_ref().and_then(|v| v.get(index)).or_else(|| d.market.as_ref().and_then(|v| v.get(index))) else { return };
    let v = &d.detail;
    let can_sign = v.account == Account::Signer;
    let floor = it.floor_mist;
    let listed = it.listing.as_ref().map(|l| l.price_mist);
    // Listed on Tradeport or Drip: their listing holds the item, so it has to come off there first.
    let elsewhere = it.listing.as_ref().filter(|l| l.venue != "kiosk" && l.venue != "market").map(|l| venue_name(&l.venue));
    let busy = matches!(d.phase, Phase::Building(_) | Phase::Signing);

    let page = cx.scene.add(parent, s().col().gap(1.1 * r).pad(0.75 * r, 0.4 * r, 0.75 * r, 0.0).done(), Node::default());
    fade_in(cx, page, now);

    // Head: art and names.
    let head = cx.scene.add(page, s().row().gap(1.0 * r).done(), Node::default());
    thumb(cx, head, 7.5, &it.image, &it.name);
    let names = cx.scene.add(head, s().col().justify_center().gap(0.25 * r).grow().done(), Node::default());
    label(cx, names, ty::ROW, 600, tok::FG, false, &it.name);
    if let Some(c) = &it.collection {
        let st = tab(cx, ty::SMALL, 400).truncate();
        cx.scene.add_text(cx.t, names, s().done(), c, st, tok::MUTED);
    }
    if let Some(l) = &it.listing {
        wrapped(cx, names, ty::SMALL, 400, tok::MUTED, &format!("Listed for {} SUI on {}", amount(l.price_mist), venue_name(&l.venue)));
    }
    let best = match &v.offer {
        OfferState::Ready(o) | OfferState::Blocked(o, _) => Some(o.price_mist),
        _ => it.top_offer_mist,
    };
    stat_line(cx, page, &[("Floor", floor), ("Best offer", best), ("Last sale", it.last_sale_mist)]);

    if !can_sign {
        let signed_out = v.account == Account::SignedOut;
        let c = cx.scene.add(page, s().col().items_start().gap(0.75 * r).done(), Node::default());
        wrapped(cx, c, ty::SMALL, 400, tok::MUTED, if signed_out { "Sign in with a passkey to sell." } else { "Add a passkey to sell." });
        let b = button(cx, c, if signed_out { "Sign in" } else { "Add a passkey" }, Variant::Primary, Sz::Md, false).tag(1).sid("add-passkey").hint(if signed_out { "Sign in" } else { "Add a passkey" });
        nav.add(b);
    }
    if can_sign {
        if let Some(e) = &elsewhere {
            wrapped(cx, page, ty::SMALL, 400, tok::MUTED, &format!("Listed on {e}. Delist it there to sell or relist it here."));
        }
    }
    if can_sign && elsewhere.is_none() {
        if let Phase::Done(text) = &d.phase {
            let row = cx.scene.add(page, s().row().items_center().gap(0.6 * r).done(), Node::default());
            dot(cx, row, tok::POS);
            label(cx, row, ty::SMALL, 400, tok::FG, false, text);
        } else {
            // Sell now.
            let sec = cx.scene.add(page, s().col().gap(0.5 * r).done(), Node::default());
            section_label(cx, sec, "Sell now");
            match &v.offer {
                OfferState::Looking => {
                    wrapped(cx, sec, ty::SMALL, 400, tok::MUTED, "Looking for offers...");
                }
                OfferState::Blocked(o, why) => {
                    wrapped(cx, sec, ty::SMALL, 400, tok::MUTED, &format!("Best offer {} SUI on {}. {}", amount(o.price_mist), venue_name(&o.venue), why));
                }
                OfferState::Ready(o) => {
                    let net = offer_net_mist(o);
                    let b = button(cx, sec, &format!("Sell for {} SUI", amount(net)), Variant::Primary, Sz::Md, true).tag(2).sid("sell").hint("Sell");
                    if busy {
                        disabled(cx, &b);
                    } else {
                        nav.add(b);
                    }
                    wrapped(cx, sec, ty::LABEL, 400, tok::MUTED, &sell_note(o, net));
                }
                OfferState::Nothing => {
                    wrapped(cx, sec, ty::SMALL, 400, tok::MUTED, "No offers on this item right now.");
                }
            }
            // List for sale / change the price.
            let sec = cx.scene.add(page, s().col().gap(0.5 * r).done(), Node::default());
            section_label(cx, sec, if it.listing.is_some() { "Change the price" } else { "List for sale" });
            let chips = cx.scene.add(sec, s().row().wrap().gap(0.5 * r).done(), Node::default());
            for (i, (lbl, sid, mist)) in price_choices(floor, listed).into_iter().enumerate() {
                let on = v.price == Some(mist);
                let (base, fg) = if on { (tok::FG, tok::BG) } else { (tok::L3, tok::FG) };
                let f = chip(cx, chips, &format!("{lbl} {}", fmt_sui(Some(mist), Fmt::trim(2))), base, fg).tag(10 + i as u32).sid(&format!("price:{sid}")).hint("Pick");
                nav.add(f);
            }
            let f = chip(cx, chips, "Other price", tok::L3, tok::FG).tag(20).sid("price:other").hint("Type a price");
            nav.add(f);
            let lbl = match v.price {
                Some(p) => format!("{} for {} SUI", if it.listing.is_some() { "Relist" } else { "List" }, amount(p)),
                None => "Pick a price".to_string(),
            };
            let b = button(cx, sec, &lbl, Variant::Secondary, Sz::Md, true).tag(30).sid("list").hint("List");
            if busy || v.price.is_none() {
                disabled(cx, &b);
            } else {
                nav.add(b);
            }
            wrapped(cx, sec, ty::LABEL, 400, tok::MUTED, "Listed in your own kiosk. Buyers pay the creator royalty on top.");
        }
    }
    let status = match &d.phase {
        Phase::Building(w) => Some((w.as_str(), false)),
        Phase::Signing => Some(("Review and approve in Rare Wallet.", false)),
        Phase::Error(m) => Some((m.as_str(), true)),
        _ => None,
    };
    if let Some((text, err)) = status {
        wrapped(cx, page, ty::SMALL, 400, if err { tok::NEG } else { tok::MUTED }, text);
    }
}

/// A price chip: h 2.25rem, rounded-chip, px 0.8rem, small medium text.
fn chip(cx: &mut Cx, parent: Id, text: &str, base: Color, fg: Color) -> Focusable {
    let r = cx.r();
    let id = cx.scene.add(parent, s().row().items_center().justify_center().h(2.25 * r).px(0.8 * r).done(), Node::bg(base).radius(0.25 * r));
    let st = tab(cx, ty::SMALL, 500).tabular();
    let tid = cx.scene.add_text(cx.t, id, s().done(), text, st, fg);
    Focusable { id, kind: Kind::Btn { base, fg, text: tid }, group: G_PRICE, hint: String::new(), tag: 0, sid: String::new() }
}

#[allow(dead_code)]
const _: u32 = G_ACT;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_choices_floor_plus_ten_and_current() {
        let g = 1_000_000_000i128;
        let c = price_choices(Some(10 * g), Some(12 * g));
        assert_eq!(c.iter().map(|x| (x.0, x.2)).collect::<Vec<_>>(), [("Floor", 10 * g), ("Floor +10%", 11 * g), ("Current", 12 * g)]);
        assert_eq!(price_choices(Some(10 * g), Some(10 * g)).len(), 2);
        assert!(price_choices(None, None).is_empty());
        assert_eq!(price_choices(None, Some(g)).len(), 1);
    }

    #[test]
    fn trophy_head_counts() {
        assert_eq!(trophy_head(3, 8, 0), "3 of 8 unlocked");
        assert_eq!(trophy_head(3, 8, 1), "3 of 8 unlocked, 1 minted");
    }
}
