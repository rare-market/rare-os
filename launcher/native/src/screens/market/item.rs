//! An item (marketplace/ItemPage.tsx) in rare.market's layout: the art on the left; the name,
//! collection, rarity and owner, the buy box, traits and recent activity on the right. Opened from
//! a card it paints at once from that card (name, price, the card's art at the width it was drawn)
//! and the full item fills in when it arrives.

use super::buy::{buy_sheet, BuyState, Phase, T_BUY, T_CLOSE, T_DONE, T_PASSKEY, T_SECOND};
use super::data::Answers;
use super::host::*;
use super::parts::*;
use super::shell::{Act, MkPage};
use rare_market::api::{paths, CollectionDetail, EventQuery, ItemCard, ItemDetail, MarketEvent};
use rare_market::format::{self as mf, SuiOpts, DASH};
use rare_market::nav::View;
use rare_render::input::Pad;
use rare_render::view::{key, El};
use std::rc::Rc;

pub const T_COLL: u32 = 1;
pub const T_BUY_NOW: u32 = 2;
pub const T_OPEN: u32 = 3;
pub const T_CART: u32 = 4;
pub const T_MANAGE: u32 = 5;

/// The account holds this item (its owner, or the seller of its listing).
pub fn owned(a: &Answers, c: &ItemCard) -> bool {
    let Some(acct) = &a.account else { return false };
    let me = acct.address.to_ascii_lowercase();
    c.owner.as_deref().is_some_and(|o| o.to_ascii_lowercase() == me) || c.listing.as_ref().and_then(|l| l.seller.as_deref()).is_some_and(|s| s.to_ascii_lowercase() == me)
}
pub const T_TRAIT: u32 = 100;
pub const T_EVENT: u32 = 500;

pub struct ItemPage {
    pub a: Rc<Answers>,
    pub id: String,
    /// What the opener showed, painted while the item loads.
    pub card: Option<ItemCard>,
    /// The width the opener drew its art at.
    pub thumb: Option<f64>,
    pub buying: bool,
    pub qr_asked: bool,
    pub phase: Phase,
    pub frame: u64,
    pub host: Host,
}

/// The item to show: the full answer, else the card it was opened from.
pub fn item_of(a: &Answers, id: &str, card: Option<&ItemCard>) -> Option<(ItemDetail, bool)> {
    match a.get::<ItemDetail>(&paths::item(id)) {
        Some(d) => Some((d, false)),
        None => card.map(|c| (ItemDetail { card: c.clone(), ..Default::default() }, true)),
    }
}

/// Floor diff of the all-in price, in percent ("+27.07%").
pub fn floor_diff(total: Option<&str>, floor: Option<&str>) -> Option<String> {
    let t: f64 = total?.parse().ok()?;
    let f: f64 = floor?.parse().ok()?;
    if f <= 0.0 {
        return None;
    }
    let d = (t - f) / f * 100.0;
    let sign = if d > 0.0 {
        "+"
    } else if d < 0.0 {
        "-"
    } else {
        ""
    };
    Some(format!("{sign}{}", mf::fmt_pct(Some(d), 2)))
}

impl ItemPage {
    #[allow(clippy::too_many_arguments)]
    pub fn new(cx: &mut Cx, parent: rare_render::Id, a: Rc<Answers>, id: &str, card: Option<ItemCard>, thumb: Option<f64>, frame: u64, now: f64) -> ItemPage {
        let mut p = ItemPage { a, id: id.to_string(), card, thumb, buying: false, qr_asked: false, phase: Phase::Idle, frame, host: Host::new(cx, parent) };
        p.render(cx, now);
        p
    }

    pub fn buy_state(&self) -> BuyState {
        let acct = self.a.account.as_ref();
        BuyState { phase: self.phase.clone(), qr_asked: self.qr_asked, in_app: acct.map(|a| a.signer).unwrap_or(false), signed_in: acct.is_some() }
    }

    fn item(&self) -> Option<ItemDetail> {
        item_of(&self.a, &self.id, self.card.as_ref()).map(|x| x.0)
    }

    pub fn open_buy(&mut self, cx: &mut Cx, now: f64) {
        self.buying = true;
        self.render(cx, now);
        // The sheet's default (Buy for / Done).
        let tag = if self.buy_state().in_app { T_BUY } else { T_DONE };
        self.host.focus_tag(cx, tag, now);
    }

    fn close_buy(&mut self, cx: &mut Cx, now: f64) {
        self.buying = false;
        self.qr_asked = false;
        self.phase = Phase::Idle;
        self.render(cx, now);
        self.host.focus_tag(cx, T_BUY_NOW, now);
    }
}

/// The page's tree.
#[allow(clippy::too_many_arguments)]
pub fn view(ui: &mut Ui, a: &Answers, id: &str, card: Option<&ItemCard>, thumb: Option<f64>, buying: Option<&BuyState>, frame: u64) -> El {
    let page = ui.bx(s().fill().col().pt(1.0)).scrolls_y(key(&format!("mk-item-{frame}")));
    let Some((item, pending)) = item_of(a, id, card) else {
        if a.failed(&paths::item(id)) {
            let open = btn(ui, s(), "Open on rare.market", T_OPEN, V::Secondary, Sz::Lg, true, false);
            return page.child(empty(ui, "Could not load this item", None, Some(open), false));
        }
        return page;
    };
    let c = &item.card;
    let coll: Option<CollectionDetail> = c.collection.as_ref().and_then(|x| a.get(&paths::collection(&x.slug)));
    let listing = c.listing.clone();
    let total = listing.as_ref().map(|l| l.all_in_mist.clone().unwrap_or_else(|| l.price_mist.clone()));
    let floor = coll.as_ref().and_then(|x| x.stats.as_ref()).and_then(|x| x.floor_mist.clone());
    let label = c.collection.as_ref().map(|x| x.name.clone()).unwrap_or_else(|| c.name.clone());

    // ---- art: the left column is 1fr of (1fr + 1.15fr) after the gutter.
    let inner = ui.w / ui.rem - 5.0 - 1.5;
    let side = inner / 2.15;
    let mut pic = ui.bx(s().wp(1.0).aspect(1.0)).bg(tok::L1).radius(ui.r(0.75)).clip();
    if let Some(w) = thumb {
        pic = pic.child(mimg(ui, s().fill(), c.image_url.as_deref(), &label, side, side, Img::w(w)));
    }
    let big = mimg(ui, s().fill(), c.image_url.as_deref(), &label, side, side, Img { width: 1024.0, inside: true, contain: true });
    pic = pic.child(if thumb.is_some() { big } else { big.bg(tok::L2) });
    let left = ui.bx(s().col().self_start().min0()).child(pic);

    // ---- name, collection, rarity, owner
    let mut meta = ui.bx(s().row().wrap().items_center().gap_xy(0.75, 0.5).mt(0.5));
    if let Some(cr) = &c.collection {
        let mut b = ui.bx(s().row().items_center().gap(0.375).h(2.0).px(0.25)).radius(ui.r(0.25)).child(ui.text(cr.name.clone(), t(SM)));
        if cr.verified {
            b = b.child(kit::verified(ui, 0.95));
        }
        meta = meta.child(fill_nav(b, T_COLL, None, None, false));
    }
    if let Some(r) = &c.rarity {
        meta = meta.child(rarity_badge(ui, s(), r, false));
    }
    meta = meta.child(ui.bx(s().h(1.25).w_px(1.0)).bg(tok::BORDER_LIGHT));
    let owner = baseline_row(ui, s().ml_auto().gap(0.3), vec![(t(SM).c(tok::MUTED), "Owned by".into()), (t(SM).mono(), mf::short_addr(c.owner.as_deref()))]);
    meta = meta.child(owner);

    // ---- the buy box
    let mut boxx = ui.bx(bw(s().col().mt(1.0).p(1.0), 1.0)).bg(tok::L2).radius(ui.r(0.75)).border(1.0, BORDER).child(ui.text(if listing.is_some() { "Total price" } else { "Not listed" }, t(BASE)));
    if let Some(l) = &listing {
        let price = baseline_row(ui, s().mt(0.25).gap(0.375), vec![(t(XL4).w(700).tab(), mf::fmt_sui(total.as_deref(), SuiOpts { dp: 4, ..Default::default() })), (t(XL).w(600), "SUI".into())]);
        let usd = mf::fmt_usd(mf::mist_to_usd(total.as_deref(), a.sui_usd()));
        let mut on = ui.bx(s().row().items_center().gap(0.375).mt(0.25)).child(ui.text("Listed on", t(SM).c(tok::MUTED)));
        if let Some(v) = venue_mark(ui, &l.venue, 0.875) {
            on = on.child(v);
        }
        on = on.child(ui.text(mf::venue_name(Some(&l.venue)), t(SM).c(tok::MUTED)));
        boxx = boxx.child(price).child(ui.text(format!("({usd})"), t(BASE).c(tok::MUTED).tab())).child(on);
    } else {
        boxx = boxx.child(ui.text_s(s().mt(0.25), "The owner has not listed this item.", t(SM).c(tok::MUTED)));
    }
    let mut btns = ui.bx(s().row().gap(0.75).mt(0.75));
    if owned(a, c) {
        btns = btns.child(btn(ui, s().grow(), if listing.is_some() { "Change price or cancel" } else { "List or sell" }, T_MANAGE, V::Primary, Sz::Lg, true, false));
        btns = btns.child(btn(ui, s(), "Open on rare.market", T_OPEN, V::Secondary, Sz::Lg, false, false));
    } else if listing.is_some() {
        btns = btns.child(btn(ui, s().grow(), "Buy now", T_BUY_NOW, V::Primary, Sz::Lg, true, false));
        let cart_label = if super::cart::contains(&c.id) { format!("In cart ({})", super::cart::count()) } else { "Add to cart".to_string() };
        btns = btns.child(btn(ui, s(), &cart_label, T_CART, V::Secondary, Sz::Lg, false, false));
        btns = btns.child(btn(ui, s(), "Open on rare.market", T_OPEN, V::Secondary, Sz::Lg, false, false));
    } else {
        btns = btns.child(btn(ui, s().grow(), "Open on rare.market", T_OPEN, V::Secondary, Sz::Lg, true, false));
    }
    boxx = boxx.child(btns);
    let semi = t(Ty::Body).w(500);
    let mut stats: Vec<(&str, El)> = Vec::new();
    if listing.is_some() {
        stats.push(("All-in price", sui(ui, s(), total.as_deref(), 3, false, semi)));
    }
    if floor.is_some() {
        stats.push(("Floor price", sui(ui, s(), floor.as_deref(), 3, false, semi)));
    }
    if let Some(d) = floor_diff(total.as_deref(), floor.as_deref()) {
        stats.push(("Floor diff", ui.text(d, semi.tab())));
    }
    if let Some(o) = c.top_offer_mist.as_deref() {
        stats.push(("Top offer", sui(ui, s(), Some(o), 3, false, semi)));
    }
    if !stats.is_empty() {
        let n = stats.len();
        let mut line = ui.bx(bw_top(s().row().mt(1.0).pt(1.0), 1.0)).child(top_line(ui));
        for (i, (l, v)) in stats.into_iter().enumerate() {
            let mut cs = s().col().min0().grow();
            if i > 0 {
                cs = cs.pl(1.0);
            }
            if i + 1 < n {
                cs = cs.pr(1.0);
            }
            let mut cell = ui.bx(cs);
            if i > 0 {
                cell = cell.child(ui.bx(s().abs(Some(0.0), None, Some(0.0), Some(0.0)).w_px(1.0)).bg(tok::HAIR));
            }
            line = line.child(cell.child(ui.text(l, T::new(Ty::Label).c(tok::MUTED))).child(v));
        }
        boxx = boxx.child(line);
    }

    // ---- traits
    let mut traits = ui.bx(bw(s().col().p(1.0), 1.0)).bg(tok::L2).radius(ui.r(0.5)).border(1.0, BORDER);
    match item.attributes.as_ref().filter(|x| !x.is_empty()) {
        Some(attrs) => {
            let mut g = ui.bx(s().grid(&[Track::Fr(1.0); 3]).gap(0.5)).group(kit::group(&format!("mk-traits-{frame}"), false));
            for (i, at) in attrs.iter().enumerate() {
                let pct = at.pct.map(|p| mf::fmt_pct(Some(p), if p < 1.0 { 1 } else { 0 })).unwrap_or_else(|| DASH.into());
                let lab = ui.bx(s().px(0.25)).bg(LABELS_NEUTRAL).radius(ui.r(0.125)).child(ui.text(pct, t(XS).c(LABEL_TEXT).tab()));
                let fl = sui(ui, s(), at.floor_mist.as_deref(), 3, false, t(SM));
                let foot = ui.bx(s().row().items_center().justify_between().mt(0.5)).child(lab).child(fl);
                let b = ui
                    .bx(s().col().min_h(5.0).p(0.5).min0())
                    .bg(tok::L4)
                    .radius(ui.r(0.375))
                    .child(ui.text(at.kind.clone(), t(XS).c(tok::MUTED).trunc()))
                    .child(ui.text(at.value.clone(), t(SM).w(600).trunc()))
                    .child(foot)
                    .key(key(&at.kind));
                g = g.child(fill_nav(b, T_TRAIT + i as u32, None, None, false));
            }
            traits = traits.child(g);
        }
        None if pending => traits = traits.child(ui.bx(s().h(5.0))),
        None => traits = traits.child(empty(ui, "No traits", None, None, true)),
    }

    // ---- activity
    let ev = a.page::<MarketEvent>(&paths::events(&EventQuery { item: Some(id), ..Default::default() }, 12));
    let mut act = ui.bx(bw(s().col().p(0.5), 1.0)).bg(tok::L2).radius(ui.r(0.5)).border(1.0, BORDER);
    match ev {
        Some(p) if p.data.is_empty() => act = act.child(empty(ui, "No activity yet", None, None, true)),
        p => {
            act = act.child(event_head(ui, false));
            for (i, e) in p.map(|p| p.data).unwrap_or_default().iter().enumerate() {
                act = act.child(event_row(ui, e, false, a.now_ms, T_EVENT + i as u32));
            }
        }
    }

    let h2 = |ui: &Ui, txt: &str| ui.text_s(s().mt(1.5).mb(0.75), txt, t(BASE).w(500));
    let mut right = ui
        .bx(s().col().min0())
        .child(ui.text(c.name.clone(), t(XL2).w(700)))
        .child(meta)
        .child(boxx)
        .child(h2(ui, "Traits"))
        .child(traits)
        .child(h2(ui, "Activity"))
        .child(act);
    if let Some(d) = item.description.as_deref().filter(|d| !d.is_empty()) {
        right = right.child(ui.text_s(s().pt(1.0), d.to_string(), t(SM).c(tok::MUTED).wrap()));
    }

    let grid = ui.bx(s().grid(&[Track::Fr(1.0), Track::Rem(1.5), Track::Fr(1.15)]).px(2.5).pb(2.5).pt(0.5)).child(left).child(ui.bx(s())).child(right);
    let mut page = page.child(grid);
    if let (Some(st), Some(l)) = (buying, listing.as_ref()) {
        page = page.child(buy_sheet(ui, c, l, st, a.sui_usd()));
    }
    page
}

impl MkPage for ItemPage {
    fn host(&mut self) -> &mut Host {
        &mut self.host
    }

    fn render(&mut self, cx: &mut Cx, now: f64) {
        let bs = self.buying.then(|| self.buy_state());
        let (a, id, card, thumb, frame) = (&*self.a, self.id.as_str(), self.card.as_ref(), self.thumb, self.frame);
        self.host.render(cx, now, |ui| view(ui, a, id, card, thumb, bs.as_ref(), frame));
    }

    fn press(&mut self, cx: &mut Cx, b: Pad, now: f64) -> Act {
        if self.buying {
            return match b {
                Pad::B => {
                    self.close_buy(cx, now);
                    Act::Moved
                }
                Pad::Lb | Pad::Rb | Pad::Y | Pad::X => Act::None,
                Pad::A => match self.host.tag() {
                    Some(T_CLOSE) | Some(T_DONE) => {
                        self.close_buy(cx, now);
                        Act::Moved
                    }
                    Some(T_BUY) => {
                        if matches!(self.phase, Phase::Building | Phase::Signing | Phase::Done(_)) {
                            return Act::None;
                        }
                        // The app builds the purchase (rare_market::buy) and hands it to Rare Wallet;
                        // it sets the phase back through `set_phase`.
                        self.phase = Phase::Building;
                        self.render(cx, now);
                        Act::Out(Out::Act(format!("market:buy:{}", self.id)))
                    }
                    Some(T_SECOND) => {
                        if self.buy_state().qr() {
                            Act::Out(Out::Act(format!("open:{}", mf::item_url(&self.id))))
                        } else {
                            self.qr_asked = true;
                            self.render(cx, now);
                            Act::Moved
                        }
                    }
                    Some(T_PASSKEY) => Act::Out(Out::Act("wallet:passkey:buy".into())),
                    _ => Act::None,
                },
                _ => Act::Pass,
            };
        }
        match b {
            Pad::A => match self.host.tag() {
                Some(T_COLL) => self.item().and_then(|i| i.card.collection.map(|c| Act::Push(View::Collection { slug: c.slug }))).unwrap_or(Act::None),
                Some(T_BUY_NOW) => {
                    self.open_buy(cx, now);
                    Act::Moved
                }
                Some(T_OPEN) => Act::Out(Out::Act(format!("open:{}", mf::item_url(&self.id)))),
                Some(T_CART) => {
                    let Some(i) = self.item() else { return Act::None };
                    super::cart::toggle(&i.card);
                    self.render(cx, now);
                    Act::Moved
                }
                Some(T_MANAGE) => Act::Out(Out::Act(format!("market:own:{}", self.id))),
                // A trait card: the collection's items with that trait.
                Some(t) if (T_TRAIT..T_EVENT).contains(&t) => self
                    .item()
                    .and_then(|i| {
                        let at = i.attributes.as_ref().and_then(|a| a.get((t - T_TRAIT) as usize)).map(|a| format!("{}:{}", a.kind, a.value));
                        i.card.collection.map(|c| {
                            if let Some(k) = at {
                                super::collection::preset_trait(&c.slug, k);
                            }
                            Act::Push(View::Collection { slug: c.slug })
                        })
                    })
                    .unwrap_or(Act::None),
                _ => Act::None,
            },
            _ => Act::Pass,
        }
    }

    fn wants(&self) -> Vec<String> {
        let mut w = vec![paths::item(&self.id), paths::events(&EventQuery { item: Some(&self.id), ..Default::default() }, 12), paths::status()];
        if let Some(c) = self.item().and_then(|i| i.card.collection) {
            w.push(paths::collection(&c.slug));
        }
        w
    }

    fn set_answers(&mut self, a: Rc<Answers>) {
        self.a = a;
    }

    fn set_buy_phase(&mut self, cx: &mut Cx, phase: Phase, now: f64) -> bool {
        self.set_phase(cx, phase, now);
        true
    }

    fn hints(&self) -> Vec<(&'static str, String)> {
        if self.buying {
            vec![("a", "Select".into()), ("b", "Close".into())]
        } else {
            let mut h = vec![("a", "Select".to_string())];
            if super::cart::count() > 0 {
                h.push(("start", format!("Cart ({})", super::cart::count())));
            }
            h.extend([("y", "Search".to_string()), ("b", "Back".into())]);
            h
        }
    }
}

impl ItemPage {
    /// The purchase moved on (the app's build, the wallet's answer).
    pub fn set_phase(&mut self, cx: &mut Cx, phase: Phase, now: f64) {
        self.phase = phase;
        self.render(cx, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_diff_signs() {
        assert_eq!(floor_diff(Some("75479000000"), Some("59400000000")).as_deref(), Some("+27.07%"));
        assert_eq!(floor_diff(Some("50"), Some("100")).as_deref(), Some("-50.00%"));
        assert_eq!(floor_diff(Some("100"), Some("100")).as_deref(), Some("0.00%"));
        assert_eq!(floor_diff(None, Some("1")), None);
        assert_eq!(floor_diff(Some("1"), Some("0")), None);
    }

    #[test]
    fn paints_from_the_card_until_the_item_arrives() {
        let a = Answers::default();
        let card = ItemCard { id: "0x1".into(), name: "Popkins #1".into(), ..Default::default() };
        let (d, pending) = item_of(&a, "0x1", Some(&card)).unwrap();
        assert!(pending);
        assert_eq!(d.card.name, "Popkins #1");
        assert!(item_of(&a, "0x1", None).is_none());
        let full = super::super::data::item();
        let (d, pending) = item_of(&full, super::super::data::ITEM, Some(&card)).unwrap();
        assert!(!pending);
        assert_ne!(d.card.name, "Popkins #1");
    }
}
