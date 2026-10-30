//! A collection (marketplace/CollectionPage.tsx) in rare.market's layout: the banner when it has
//! one, the header with the stat strip, then Items (a grid of cards or a list), Activity and
//! Holders. LT/RT switch the tabs, X opens Filters (sort and traits), View flips grid and list.

use super::data::{near_end, Answers};
use super::host::*;
use super::parts::*;
use super::shell::{Act, MkPage};
use rare_market::api::{paths, CollectionDetail, EventQuery, HoldersPage, ItemCard, MarketEvent, Page as ApiPage, Sort, TraitValue};
use rare_market::format::{self as mf, SuiOpts, DASH};
use rare_market::nav::View;
use rare_render::input::Pad;
use rare_render::scene::layer;
use rare_render::view::{key, El, Look, Motion, Transition};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Items,
    Activity,
    Holders,
}

pub const TABS: [(Tab, &str); 3] = [(Tab::Items, "Items"), (Tab::Activity, "Activity"), (Tab::Holders, "Holders")];
pub const SORTS: [(Sort, &str); 4] = [(Sort::PriceAsc, "Price: low to high"), (Sort::PriceDesc, "Price: high to low"), (Sort::RarityAsc, "Rarity: rarest first"), (Sort::ListedDesc, "Recently listed")];
pub const COLS: usize = 6;

pub const T_TAB: u32 = 10;
pub const T_SORT_BTN: u32 = 20;
pub const T_SWEEP_BTN: u32 = 21;
pub const T_CART_BTN: u32 = 22;
pub const T_FAIL_OPEN: u32 = 23;
pub const T_ITEM: u32 = 1000;
pub const T_EVENT: u32 = 5000;
pub const T_HOLDER: u32 = 8000;
pub const T_F_CLOSE: u32 = 20_000;
pub const T_F_SORT: u32 = 20_010;
pub const T_F_CLEAR: u32 = 20_020;
pub const T_F_GROUP: u32 = 20_100;
pub const T_F_VALUE: u32 = 21_000;
/// The filter panel's focus scope.
pub const SCOPE: u32 = 6;

/// View (Select) flips the items between a grid of cards and a list of rows; the choice is kept.
static LIST: AtomicBool = AtomicBool::new(false);

pub fn list_view() -> bool {
    LIST.load(Ordering::Relaxed)
}

pub fn set_list_view(on: bool) {
    LIST.store(on, Ordering::Relaxed)
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub tab: Tab,
    pub sort: Sort,
    /// "type:value" keys.
    pub traits: Vec<String>,
    pub filters: bool,
    /// The trait type open in the filter panel.
    pub open: Option<String>,
    /// Items as a list of rows (from the kept choice when the page opens).
    pub list: bool,
}

impl Default for State {
    fn default() -> State {
        State { tab: Tab::Items, sort: Sort::PriceAsc, traits: Vec::new(), filters: false, open: None, list: list_view() }
    }
}

pub struct CollectionPage {
    pub a: Rc<Answers>,
    pub slug: String,
    pub st: State,
    pub frame: u64,
    pub host: Host,
    /// The focused row and since when (secs): a holder focus resting 250 ms reads that wallet's
    /// items ahead, so opening it is instant.
    rest: std::cell::Cell<(Option<u32>, f64)>,
}

/// How long focus rests on a holder row before its wallet is read ahead.
const REST_S: f64 = 0.25;

/// The trait groups of the filter panel: types in answer order, values by count, at most 60.
pub fn trait_groups(vals: &[TraitValue]) -> Vec<(String, Vec<TraitValue>)> {
    let mut out: Vec<(String, Vec<TraitValue>)> = Vec::new();
    for v in vals {
        match out.iter_mut().find(|(t, _)| *t == v.kind) {
            Some((_, g)) => g.push(v.clone()),
            None => out.push((v.kind.clone(), vec![v.clone()])),
        }
    }
    for (_, g) in out.iter_mut() {
        g.sort_by(|a, b| b.count.cmp(&a.count));
        g.truncate(60);
    }
    out
}

/// "Royalty 5.0%, listed on Kiosk, Tradeport".
pub fn royalty_line(c: &CollectionDetail) -> String {
    let r = c.royalty_bps.map(|b| format!("{:.1}%", b / 100.0)).unwrap_or_else(|| DASH.into());
    let mut s = format!("Royalty {r}");
    if !c.venues.is_empty() {
        let names: Vec<String> = c
            .venues
            .iter()
            .map(|v| {
                if v == "market" {
                    "Rare".to_string()
                } else {
                    let mut ch = v.chars();
                    ch.next().map(|f| f.to_uppercase().collect::<String>() + ch.as_str()).unwrap_or_default()
                }
            })
            .collect();
        s.push_str(&format!(", listed on {}", names.join(", ")));
    }
    s
}

/// The sort button's words: the sort, and how many traits are on.
pub fn sort_label(sort: Sort, traits: usize) -> String {
    let mut s = SORTS.iter().find(|x| x.0 == sort).map(|x| x.1).unwrap_or("").to_string();
    if traits > 0 {
        s.push_str(&format!(", {traits} trait{}", if traits > 1 { "s" } else { "" }));
    }
    s
}

thread_local! {
    static PRESET: std::cell::RefCell<Option<(String, String)>> = const { std::cell::RefCell::new(None) };
}

/// The next page opened on this collection starts filtered to this trait ("<type>:<value>"): an
/// item page's trait card.
pub fn preset_trait(slug: &str, key: String) {
    PRESET.with(|p| *p.borrow_mut() = Some((slug.to_string(), key)));
}

impl CollectionPage {
    pub fn new(cx: &mut Cx, parent: rare_render::Id, a: Rc<Answers>, slug: &str, frame: u64, now: f64) -> CollectionPage {
        let mut st = State::default();
        if let Some((s, k)) = PRESET.with(|p| p.borrow_mut().take()) {
            if s == slug {
                st.traits.push(k);
            }
        }
        let mut p = CollectionPage { a, slug: slug.to_string(), st, frame, host: Host::new(cx, parent), rest: std::cell::Cell::new((None, 0.0)) };
        p.render(cx, now);
        p
    }

    /// rare.market did not answer for this collection (the page shows only Open on rare.market).
    fn failed(&self) -> bool {
        self.a.get::<CollectionDetail>(&paths::collection(&self.slug)).is_none() && self.a.failed(&paths::collection(&self.slug))
    }

    pub fn items_path(&self) -> String {
        paths::items(&self.slug, self.st.sort, &self.st.traits.join(","))
    }

    pub fn items(&self) -> Vec<ItemCard> {
        self.a.rows::<ItemCard>(&self.items_path()).items
    }

    fn events_path(&self) -> String {
        paths::events(&EventQuery { collection: Some(&self.slug), ..Default::default() }, 30)
    }

    fn switch_tab(&mut self, cx: &mut Cx, tab: Tab, now: f64) {
        self.st.tab = tab;
        self.render(cx, now);
        let i = TABS.iter().position(|t| t.0 == tab).unwrap_or(0) as u32;
        self.host.focus_tag(cx, T_TAB + i, now);
    }

    fn set_filters(&mut self, cx: &mut Cx, on: bool, now: f64) {
        self.st.filters = on;
        self.st.open = None;
        self.render(cx, now);
        if on {
            let i = SORTS.iter().position(|s| s.0 == self.st.sort).unwrap_or(0) as u32;
            self.host.focus_tag(cx, T_F_SORT + i, now);
        } else {
            self.host.focus_tag(cx, T_SORT_BTN, now);
        }
    }

    fn filter_press(&mut self, cx: &mut Cx, b: Pad, now: f64) -> Act {
        match b {
            Pad::B | Pad::X => {
                self.set_filters(cx, false, now);
                Act::Moved
            }
            Pad::Lt | Pad::Rt | Pad::Y => Act::None,
            Pad::A => {
                let Some(tag) = self.host.tag() else { return Act::None };
                let groups = trait_groups(&self.a.get::<ApiPage<TraitValue>>(&paths::traits(&self.slug)).map(|p| p.data).unwrap_or_default());
                match tag {
                    T_F_CLOSE => {
                        self.set_filters(cx, false, now);
                        return Act::Moved;
                    }
                    t if (T_F_SORT..T_F_SORT + 4).contains(&t) => self.st.sort = SORTS[(t - T_F_SORT) as usize].0,
                    T_F_CLEAR => self.st.traits.clear(),
                    t if (T_F_GROUP..T_F_VALUE).contains(&t) => {
                        let g = groups.get((t - T_F_GROUP) as usize).map(|g| g.0.clone());
                        self.st.open = if self.st.open == g { None } else { g };
                    }
                    t if t >= T_F_VALUE => {
                        let open = self.st.open.clone().unwrap_or_default();
                        if let Some(v) = groups.iter().find(|g| g.0 == open).and_then(|g| g.1.get((t - T_F_VALUE) as usize)) {
                            let k = format!("{}:{}", v.kind, v.value);
                            if let Some(i) = self.st.traits.iter().position(|x| *x == k) {
                                self.st.traits.remove(i);
                            } else {
                                self.st.traits.push(k);
                            }
                        }
                    }
                    _ => return Act::None,
                }
                self.render(cx, now);
                Act::Moved
            }
            _ => Act::Pass,
        }
    }
}

/// The page's tree.
pub fn view(ui: &mut Ui, a: &Answers, slug: &str, st: &State, frame: u64) -> El {
    let page = ui.bx(s().fill().col().pt(1.0).pb(2.5)).scrolls_y(key(&format!("mk-coll-{frame}")));
    let c: Option<CollectionDetail> = a.get(&paths::collection(slug));
    let mut page = page;

    if let Some(b) = c.as_ref().and_then(|c| c.banner_url.clone()) {
        let name = c.as_ref().map(|c| c.r#ref.name.clone()).unwrap_or_default();
        let art = mimg(ui, s().fill(), Some(&b), &name, ui.w / ui.rem, 11.0, Img { width: 1600.0, inside: true, contain: false });
        page = page.child(ui.bx(s().wp(1.0).h(11.0)).bg(tok::L2).clip().child(art));
    }

    // ---- header
    let name = c.as_ref().map(|c| c.r#ref.name.clone()).unwrap_or_default();
    let av = avatar(ui, s(), c.as_ref().and_then(|c| c.r#ref.image_url.as_deref()), c.as_ref().map(|c| c.r#ref.name.as_str()).unwrap_or(slug), 3.5, false);
    let mut title = ui.bx(s().row().items_center().gap(0.5)).child(ui.text(name, t(XL).w(600).trunc()));
    if c.as_ref().map(|c| c.r#ref.verified).unwrap_or(false) {
        title = title.child(kit::verified(ui, 1.0));
    }
    let mut names = ui.bx(s().col().min0().shrink()).child(title);
    if let Some(c) = &c {
        names = names.child(ui.text_s(s().mt(0.125), royalty_line(c), t(XS).c(tok::MUTED)));
    }
    let mut head = ui.bx(s().row().items_center().gap(0.75).px(2.5).pt(1.0)).child(av).child(names);
    if let Some(sx) = c.as_ref().and_then(|c| c.stats.clone()) {
        let semi = t(BASE).w(600).tab();
        let unit = t(BASE).w(400).c(tok::MUTED);
        let floor_delta = delta_el(ui, s(), sx.floor_change24h_pct, 2, t(BASE).w(400));
        let floor = stat(ui, "Floor price", vec![(semi, mf::fmt_sui(sx.floor_mist.as_deref(), SuiOpts { dp: 3, ..Default::default() })), (unit, "SUI".into())]);
        // The delta joins the floor's baseline row.
        let floor = add_to_line(floor, floor_delta);
        let offer = stat(ui, "Top offer", vec![(semi, mf::fmt_sui(sx.top_offer_mist.as_deref(), SuiOpts { dp: 3, ..Default::default() })), (unit, "SUI".into())]);
        let vol = stat(ui, "Volume 7d", vec![(semi, mf::fmt_sui(sx.volume7d_mist.as_deref(), SuiOpts { dp: 1, compact: true, trim: false }))]);
        let sales = stat(ui, "Sales 7d", vec![(semi, mf::fmt_int(Some(sx.sales7d as f64), false))]);
        let pct = if sx.supply > 0 { mf::fmt_pct(Some(sx.listed_count as f64 / sx.supply as f64 * 100.0), 0) } else { String::new() };
        let listed = stat(ui, "Listed / supply", vec![(semi, format!("{} / {}", mf::fmt_int(Some(sx.listed_count as f64), false), mf::fmt_int(Some(sx.supply as f64), false))), (unit, pct)]);
        let owners = if sx.owners > 0 {
            stat(ui, "Owners", vec![(semi, mf::fmt_int(Some(sx.owners as f64), true)), (unit, mf::fmt_pct(Some(sx.unique_owner_pct), 1))])
        } else {
            stat(ui, "Owners", vec![(semi, DASH.into())])
        };
        let strip = ui.bx(s().row().items_end().gap_xy(1.5, 0.0).ml(2.0).grow()).clip().child(floor).child(offer).child(vol).child(sales).child(listed).child(owners);
        head = head.child(strip);
    }
    page = page.child(head);
    if c.is_none() && a.failed(&paths::collection(slug)) {
        let open = btn(ui, s(), "Open on rare.market", T_FAIL_OPEN, V::Secondary, Sz::Lg, true, false);
        // No tabs, sort or Sweep for a collection that isn't there.
        return page.child(empty(ui, "Could not load this collection", None, Some(open), true));
    }

    // ---- tabs
    let ti = TABS.iter().position(|x| x.0 == st.tab).unwrap_or(0);
    let labels: Vec<&str> = TABS.iter().map(|x| x.1).collect();
    let tabs_el = tabs(ui, s(), &labels, ti, T_TAB);
    let mut bar = ui.bx(bw_side(s().row().items_center().gap(0.75).px(2.5).mt(1.5), 0.0, 1.0)).look(Look::default().bg(tok::BG).z(4)).sticky().name(PIN).group(kit::group(&format!("mk-bar-{frame}"), false)).child(ui.bx(s().abs(None, Some(0.0), Some(0.0), Some(0.0)).h_px(1.0)).style(|st| st.inset.bottom = rare_render::taffy::prelude::length(-1.0)).bg(BORDER)).child(tabs_el).child(ui.bx(s().grow()));
    let items_rows = a.rows::<ItemCard>(&paths::items(slug, st.sort, &st.traits.join(",")));
    if st.tab == Tab::Items {
        let total = items_rows.total.map(|n| format!("{} listed", mf::fmt_int(Some(n as f64), false))).unwrap_or_default();
        let sort_btn = btn(ui, s(), &sort_label(st.sort, st.traits.len()), T_SORT_BTN, V::Secondary, Sz::Sm, false, true);
        let sweep_btn = btn(ui, s(), "Sweep", T_SWEEP_BTN, V::Secondary, Sz::Sm, false, false);
        let mut tools = ui.bx(s().row().items_center().gap(0.5).pb(0.5)).child(ui.text(total, t(SM).c(tok::MUTED))).child(sort_btn).child(sweep_btn);
        if super::cart::count() > 0 {
            tools = tools.child(btn(ui, s(), &format!("Cart ({})", super::cart::count()), T_CART_BTN, V::Primary, Sz::Sm, false, false));
        }
        bar = bar.child(tools);
    }
    page = page.child(bar);

    // ---- content
    let mut content = ui.bx(s().col().px(2.5).pt(1.0));
    match st.tab {
        Tab::Items => {
            let items = &items_rows.items;
            let list = st.list;
            let mut g = if list {
                ui.bx(s().col().gap(0.25))
            } else {
                let cols = [Track::Fr(1.0); COLS];
                ui.bx(s().grid(&cols).gap(0.75))
            }
            .group(kit::group(&format!("mk-items-{frame}"), false));
            let cw = (ui.w / ui.rem - 5.0 - 0.75 * (COLS as f32 - 1.0)) / COLS as f32;
            for (i, it) in items.iter().enumerate() {
                let el = if list { item_row(ui, it, T_ITEM + i as u32, i == 0) } else { item_card(ui, s().min0(), it, cw, T_ITEM + i as u32, i == 0, false) };
                g = g.child(el);
            }
            content = content.child(g);
            if items_rows.ready && items.is_empty() {
                let title = if st.traits.is_empty() { "Nothing listed right now" } else { "Nothing listed with these traits" };
                content = content.child(empty(ui, title, None, None, false));
            }
        }
        Tab::Activity => {
            let ev = a.rows::<MarketEvent>(&paths::events(&EventQuery { collection: Some(slug), ..Default::default() }, 30));
            match ev {
                r if r.ready && r.items.is_empty() => content = content.child(empty(ui, "No activity yet", None, None, false)),
                r => {
                    let mut tbl = ui.bx(bw(s().col().p(0.5), 1.0)).bg(tok::L2).radius(ui.r(0.5)).border(1.0, BORDER).child(event_head(ui, true));
                    for (i, e) in r.items.iter().enumerate() {
                        tbl = tbl.child(event_row(ui, e, true, a.now_ms, T_EVENT + i as u32));
                    }
                    content = content.child(tbl);
                }
            }
        }
        Tab::Holders => content = content.child(holders(ui, a, slug)),
    }
    page = page.child(content);

    if st.filters {
        page = page.child(filter_sheet(ui, a, slug, st));
    }
    page
}

/// Appends `el` to a Stat's value line (the floor's delta).
fn add_to_line(stat: El, el: El) -> El {
    let mut stat = stat;
    if let Some(line) = stat.kids.last_mut() {
        let l = std::mem::take(line);
        *line = l.child(el);
    }
    stat
}

/// ItemRow (the list view): thumbnail, short name, price, last sale. It grows 3 % on focus, not the
/// tiles' 6 %: a full-width row at 6 % ran past the 2.5 rem gutter and off the screen.
pub fn item_row(ui: &mut Ui, c: &ItemCard, tag: u32, def: bool) -> El {
    let label = c.collection.as_ref().map(|x| x.name.clone()).unwrap_or_else(|| c.name.clone());
    let art = mimg(ui, s().fill(), c.image_url.as_deref(), &label, 2.5, 2.5, Img::w(96.0));
    let thumb = ui.bx(s().size(2.5)).bg(tok::L3).radius(ui.r(0.25)).clip().child(art);
    let coll = c.collection.as_ref().map(|x| x.name.as_str());
    let name = ui.text_s(s().grow(), mf::short_name(&c.name, coll).to_string(), t(SM).w(600).trunc());
    let price = match &c.listing {
        Some(l) => {
            let m = l.all_in_mist.clone().unwrap_or_else(|| l.price_mist.clone());
            sui(ui, s(), Some(&m), 3, false, t(SM).w(600))
        }
        None => ui.text("Not listed", t(SM).c(tok::MUTED)),
    };
    let last = c.last_sale_mist.as_deref().map(|m| mf::fmt_sui(Some(m), SuiOpts { dp: 3, compact: false, trim: true })).unwrap_or_else(|| DASH.into());
    let look = Look::default().bg(tok::L1).radius(ui.r(0.5)).border(1.0, rare_render::Color::TRANSPARENT);
    let focus = look.clone().bg(tok::L2).border(1.0, BORDER_HOVER).scale(1.03);
    let el = ui
        .bx(bw(s().row().items_center().h(3.5).wp(1.0).gap(0.75).px(0.75), 1.0))
        .look(look)
        .focus_look(focus)
        .transition(Transition::FOCUS)
        .child(thumb)
        .child(name)
        .child(ui.bx(s().row().justify_end().w(9.0)).child(price))
        .child(right_text(ui, s().w(7.0), last, t(SM).c(tok::MUTED).tab()))
        .key(key(&c.id));
    kit::nav(el, tag, None, None, def)
}

/// The Holders tab: the header figures, staked or locked, the holder rows.
/// Every holders page that has arrived (the first has the header), and the path of the next one.
fn holder_rows(a: &Answers, slug: &str) -> (Option<HoldersPage>, Vec<rare_market::api::Holder>, Option<String>) {
    let base = paths::holders(slug);
    let mut path = rare_market::api::page_path(&base, None);
    let mut first: Option<HoldersPage> = None;
    let mut rows = Vec::new();
    while let Some(p) = a.get::<HoldersPage>(&path) {
        rows.extend(p.data.iter().cloned());
        let next = p.next_cursor.clone();
        first.get_or_insert(p);
        match next {
            Some(c) => path = rare_market::api::page_path(&base, Some(&c)),
            None => return (first, rows, None),
        }
    }
    let next = first.as_ref().map(|_| path);
    (first, rows, next)
}

fn holders(ui: &mut Ui, a: &Answers, slug: &str) -> El {
    let (h, rows, _) = holder_rows(a, slug);
    let mut b = ui.bx(bw(s().col(), 1.0)).bg(tok::L2).radius(ui.r(0.5)).border(1.0, BORDER).clip();
    let line = |ui: &Ui| bottom_line(ui);
    if let Some(hd) = h.as_ref().map(|h| h.header.clone()) {
        let figs = [
            ("Owners", mf::fmt_int(Some(hd.owners as f64), false)),
            ("Unique owners", mf::fmt_pct(Some(hd.unique_owners_pct), 1)),
            ("Top 10 hold", mf::fmt_pct(Some(hd.top10_pct), 1)),
            ("Top 100 hold", mf::fmt_pct(Some(hd.top100_pct), 1)),
            ("Items held", format!("{} / {}", mf::fmt_int(Some(hd.items_held as f64), false), mf::fmt_int(Some(hd.supply as f64), false))),
        ];
        let mut g = ui.bx(bw_side(s().grid(&[Track::Fr(1.0); 5]).gap_xy(1.0, 0.75).px(1.0).py(0.75), 0.0, 1.0)).child(line(ui));
        for (k, v) in figs {
            g = g.child(ui.bx(s().col().min0()).child(ui.text(k, t(XS).c(tok::MUTED))).child(ui.text_s(s().mt(0.125), v, t(SM).w(600).tab().trunc())));
        }
        b = b.child(g);
        if !hd.staked_or_locked.is_empty() {
            let mut sl = ui.bx(bw_side(s().col().px(1.0).py(0.75), 0.0, 1.0)).child(line(ui)).child(ui.text("Staked or locked", t(XS).c(tok::MUTED)));
            let mut ul = ui.bx(s().col().mt(0.25).gap(0.125));
            for r in &hd.staked_or_locked {
                ul = ul.child(ui.text(format!("{} in {}", mf::fmt_int(Some(r.count as f64), false), r.name), t(Ty::Px(0.8125, 1.25)).tab()));
            }
            sl = sl.child(ul);
            b = b.child(sl);
        }
    } else if !a.has(&paths::holders(slug)) {
        // still loading: the table head alone
    }
    b = b.child(holder_head(ui));
    for (i, r) in rows.iter().enumerate() {
        b = b.child(holder_row(ui, r, T_HOLDER + i as u32));
    }
    b
}

/// ChevronDown turned over (the open trait group).
fn chevron(ui: &mut Ui, up: bool) -> El {
    let path = if up { "m18 15-6-6-6 6" } else { "m6 9 6 6 6-6" };
    let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><path d="{path}"/></svg>"#);
    crate::kit::icon::svg_tinted(ui, if up { "chev-up" } else { "chev-down" }, svg, 1.125, 1.125, tok::ICON2)
}

/// The Filters panel (collection/filters.tsx): sort, then trait types that open to their values.
fn filter_sheet(ui: &mut Ui, a: &Answers, slug: &str, st: &State) -> El {
    let vals = a.get::<ApiPage<TraitValue>>(&paths::traits(slug));
    let groups = trait_groups(&vals.as_ref().map(|p| p.data.clone()).unwrap_or_default());
    let line = |ui: &Ui| bottom_line(ui);

    let close_ic = icon_el(ui, "x", 1.25);
    let close = fill_nav(ui.bx(s().row().items_center().justify_center().size(2.5)).radius(ui.r(0.25)).child(close_ic), T_F_CLOSE, None, None, false);
    let head = ui.bx(s().row().items_center().justify_between().h(4.0).px(1.0)).child(ui.text("Filters", t(LG).w(600))).child(close);

    let mut sort = ui.bx(bw_side(s().col().pb(0.75), 0.0, 1.0)).child(line(ui)).child(ui.bx(s().row().items_center().h(3.0)).child(ui.text("Sort", t(BASE).w(500))));
    for (i, (v, label)) in SORTS.iter().enumerate() {
        let on = *v == st.sort;
        let dot = if on { ui.bx(s().size(1.25)).radius(ui.r(0.625)).border(6.0, tok::FG) } else { ui.bx(s().size(1.25)).radius(ui.r(0.625)).bg(tok::L4) };
        let row = ui.bx(s().row().items_center().h(2.75).wp(1.0).gap(0.75).px(0.25)).radius(ui.r(0.25)).child(dot).child(ui.text(*label, t(SM)));
        sort = sort.child(fill_nav(row, T_F_SORT + i as u32, None, None, on));
    }

    let mut th = ui.bx(s().row().items_center().justify_between().h(3.0)).child(ui.text("Traits", t(BASE).w(500)));
    if !st.traits.is_empty() {
        let clear = ui.bx(s().row().items_center().h(2.0).px(0.75)).bg(tok::BUTTON_SECONDARY).radius(ui.r(0.25)).child(ui.text("Clear", t(SM)));
        th = th.child(fill_nav(clear, T_F_CLEAR, None, None, false));
    }
    let mut traits = ui.bx(s().col().pt(0.25)).child(th);
    if groups.is_empty() && vals.is_some() {
        traits = traits.child(ui.bx(s().py(0.75)).child(ui.text("No traits for this collection", t(SM).c(tok::MUTED))));
    }
    let n = groups.len();
    for (gi, (ty, vs)) in groups.iter().enumerate() {
        let sel = st.traits.iter().filter(|k| k.starts_with(&format!("{ty}:"))).count();
        let shown = st.open.as_deref() == Some(ty.as_str());
        let mut b = ui.bx(s().row().items_center().h(2.75).wp(1.0).gap(0.5).px(0.25)).radius(ui.r(0.25)).child(ui.text_s(s().grow(), ty.clone(), t(SM).trunc()));
        if sel > 0 {
            b = b.child(ui.bx(s().row().items_center().justify_center().h(1.25).min_w(1.25).px(0.25)).bg(tok::L4).radius(ui.r(0.625)).child(ui.text(sel.to_string(), t(XS).tab())));
        }
        b = b.child(right_text(ui, s().w(2.0), vs.len().to_string(), t(SM).c(tok::MUTED).tab())).child(chevron(ui, shown));
        let last = gi + 1 == n;
        let mut grp = ui.bx(bw_side(s().col(), 0.0, if last { 0.0 } else { 1.0 })).child(fill_nav(b, T_F_GROUP + gi as u32, None, None, false));
        if !last {
            grp = grp.child(line(ui));
        }
        if shown {
            let mut ul = ui.bx(s().col().pb(0.5));
            for (vi, v) in vs.iter().enumerate() {
                let k = format!("{}:{}", v.kind, v.value);
                let on = st.traits.contains(&k);
                let mut cb = ui.bx(s().row().items_center().justify_center().size(1.0)).radius(ui.r(0.125)).border(1.0, if on { tok::FG } else { BORDER_HOVER });
                if on {
                    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><path d="M2 5.2 4 7l4-4.2" fill="none" stroke="white" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
                    cb = cb.bg(tok::FG).child(crate::kit::icon::svg_tinted(ui, "trait-check", svg.into(), 0.625, 0.625, tok::BG));
                }
                let floor = v.floor_mist.as_deref().map(|m| format!("{} SUI floor", mf::fmt_sui(Some(m), SuiOpts { dp: 3, ..Default::default() }))).unwrap_or_else(|| "No listings".into());
                let mid = ui.bx(s().col().grow()).child(ui.text(v.value.clone(), t(Ty::Px(0.875, 1.125)).trunc())).child(ui.text(floor, t(Ty::Px(0.75, 1.0)).c(tok::MUTED).trunc()));
                let right = ui
                    .bx(s().col().items_end().w(3.25))
                    .child(ui.text(mf::fmt_int(Some(v.count as f64), false), t(Ty::Px(0.875, 1.125)).tab()))
                    .child(ui.text(mf::fmt_pct(Some(v.pct), if v.pct < 1.0 { 1 } else { 0 }), t(Ty::Px(0.75, 1.0)).c(tok::MUTED).tab()));
                let row = ui.bx(s().row().items_center().h(3.0).wp(1.0).min0().gap(0.75).px(0.25)).radius(ui.r(0.25)).child(cb).child(mid).child(right);
                ul = ul.child(fill_nav(row, T_F_VALUE + vi as u32, None, None, false));
            }
            grp = grp.child(ul);
        }
        traits = traits.child(grp);
    }

    let body = ui.bx(s().col().px(1.0).pb(1.5)).child(sort).child(traits);
    let panel = ui
        .bx(bw_side(s().col().hp(1.0).w(26.0).pt(1.0), 1.0, 0.0))
        .bg(tok::BG)
        .child(ui.bx(s().abs(Some(0.0), None, Some(0.0), Some(0.0)).w_px(1.0)).style(|st| st.inset.left = rare_render::taffy::prelude::length(-1.0)).bg(tok::BORDER_LIGHT))
        .child(head)
        .child(body)
        .scrolls_y(key(&format!("mk-filters-{slug}")))
        .enter(Motion::SLIDE_IN_RIGHT);
    // In flow inside the layer (see buy_sheet).
    ui.bx(s().full().row().justify_end().pb(3.0))
        .portal(layer::SHEET)
        .scope(SCOPE)
        .child(ui.bx(s().abs(Some(0.0), Some(0.0), Some(3.0), Some(0.0))).bg(tok::BG.alpha(0.5)).enter(Motion::FADE))
        .child(panel)
}

fn icon_el(ui: &mut Ui, name: &str, size: f32) -> El {
    kit::focus_icon(ui, name, size, false, tok::FG, tok::FG)
}

impl CollectionPage {
    /// True once focus has stayed on `tag` for [`REST_S`] (the clock starts when it first sees it).
    fn rested(&self, tag: Option<u32>) -> bool {
        let now = rare_render::clock::secs();
        let (t, since) = self.rest.get();
        if t != tag {
            self.rest.set((tag, now));
            return false;
        }
        now - since >= REST_S
    }
}

impl MkPage for CollectionPage {
    fn host(&mut self) -> &mut Host {
        &mut self.host
    }

    fn render(&mut self, cx: &mut Cx, now: f64) {
        let (a, slug, st, frame) = (&*self.a, self.slug.as_str(), &self.st, self.frame);
        self.host.render(cx, now, |ui| view(ui, a, slug, st, frame));
    }

    fn press(&mut self, cx: &mut Cx, b: Pad, now: f64) -> Act {
        if self.st.filters {
            return self.filter_press(cx, b, now);
        }
        // A collection that failed to load has only Open on rare.market.
        if self.failed() && matches!(b, Pad::Lt | Pad::Rt | Pad::X | Pad::Select) {
            return Act::None;
        }
        match b {
            Pad::Lt | Pad::Rt => {
                let i = TABS.iter().position(|t| t.0 == self.st.tab).unwrap_or(0);
                let n = TABS.len();
                let j = if b == Pad::Rt { (i + 1) % n } else { (i + n - 1) % n };
                self.switch_tab(cx, TABS[j].0, now);
                Act::Moved
            }
            Pad::Select if self.st.tab == Tab::Items => {
                self.st.list = !self.st.list;
                set_list_view(self.st.list);
                self.render(cx, now);
                Act::Moved
            }
            Pad::X if self.st.tab == Tab::Items => {
                self.set_filters(cx, true, now);
                Act::Moved
            }
            Pad::A => {
                let Some(tag) = self.host.tag() else { return Act::None };
                match tag {
                    t if (T_TAB..T_TAB + 3).contains(&t) => {
                        self.switch_tab(cx, TABS[(t - T_TAB) as usize].0, now);
                        Act::Moved
                    }
                    T_FAIL_OPEN => Act::Out(Out::Act(format!("open:{}", mf::collection_url(&self.slug)))),
                    T_SORT_BTN => {
                        self.set_filters(cx, true, now);
                        Act::Moved
                    }
                    T_SWEEP_BTN => Act::Out(Out::Act(format!("market:sweep:{}", self.slug))),
                    T_CART_BTN => Act::Out(Out::Act("market:cart:open".into())),
                    t if (T_ITEM..T_EVENT).contains(&t) => {
                        let thumb = if self.st.list { 96.0 } else { 400.0 };
                        self.items().get((t - T_ITEM) as usize).map(|it| Act::Push(View::Item { id: it.id.clone(), card: Some(Box::new(it.clone())), thumb: Some(thumb) })).unwrap_or(Act::None)
                    }
                    t if (T_EVENT..T_HOLDER).contains(&t) => {
                        let ev = self.a.rows::<MarketEvent>(&self.events_path()).items;
                        ev.get((t - T_EVENT) as usize).and_then(|e| e.item.as_ref()).map(|it| Act::Push(View::Item { id: it.id.clone(), card: None, thumb: None })).unwrap_or(Act::None)
                    }
                    t if t >= T_HOLDER && t < T_F_CLOSE => {
                        let (_, rows, _) = holder_rows(&self.a, &self.slug);
                        rows.get((t - T_HOLDER) as usize).cloned()
                            .map(|r| Act::Push(View::Account { address: r.address.clone(), label: r.name.clone().unwrap_or_else(|| "Holder".into()) }))
                            .unwrap_or(Act::None)
                    }
                    _ => Act::None,
                }
            }
            _ => Act::Pass,
        }
    }

    fn wants(&self) -> Vec<String> {
        let mut w = vec![paths::collection(&self.slug)];
        let tag = self.host.tag();
        match self.st.tab {
            Tab::Items => {
                let r = self.a.rows::<ItemCard>(&self.items_path());
                w.push(self.items_path());
                let idx = tag.filter(|t| (T_ITEM..T_EVENT).contains(t)).map(|t| (t - T_ITEM) as usize);
                if let Some(n) = r.next.filter(|_| near_end(idx, r.items.len(), if self.st.list { 1 } else { COLS })) {
                    w.push(n);
                }
            }
            Tab::Activity => {
                let r = self.a.rows::<MarketEvent>(&self.events_path());
                w.push(self.events_path());
                let idx = tag.filter(|t| (T_EVENT..T_HOLDER).contains(t)).map(|t| (t - T_EVENT) as usize);
                if let Some(n) = r.next.filter(|_| near_end(idx, r.items.len(), 3)) {
                    w.push(n);
                }
            }
            Tab::Holders => {
                let (_, rows, next) = holder_rows(&self.a, &self.slug);
                w.push(paths::holders(&self.slug));
                let idx = tag.filter(|t| (T_HOLDER..T_F_CLOSE).contains(t)).map(|t| (t - T_HOLDER) as usize);
                if let Some(n) = next.filter(|_| near_end(idx, rows.len(), 3)) {
                    w.push(n);
                }
                // Focus resting on a holder: that wallet's items (what its page reads), kept in
                // the shared answers, so A opens it filled.
                if let Some(r) = idx.filter(|_| self.rested(tag)).and_then(|i| rows.get(i)) {
                    w.push(paths::account_items(&r.address));
                }
            }
        }
        if self.st.filters {
            w.push(paths::traits(&self.slug));
        }
        w
    }

    fn set_answers(&mut self, a: Rc<Answers>) {
        self.a = a;
    }

    fn deadline(&self) -> Option<f64> {
        // Wake when a holder focus has rested long enough to read its wallet ahead.
        let (t, since) = self.rest.get();
        let holder = self.st.tab == Tab::Holders && t.is_some_and(|t| (T_HOLDER..T_F_CLOSE).contains(&t));
        (holder && rare_render::clock::secs() - since < REST_S).then_some(since + REST_S)
    }

    fn hints(&self) -> Vec<(&'static str, String)> {
        if self.st.filters {
            return vec![("a", "Select".into()), ("b", "Done".into())];
        }
        if self.failed() {
            return vec![("a", "Select".into()), ("y", "Search".into()), ("b", "Back".into())];
        }
        let mut h = vec![("a", "Open".to_string())];
        if self.st.tab == Tab::Items {
            h.push(("x", "Filters".into()));
            h.push(("select", if self.st.list { "Grid view" } else { "List view" }.into()));
        }
        if super::cart::count() > 0 {
            h.push(("start", format!("Cart ({})", super::cart::count())));
        }
        h.extend([("ltrt", "Sections".to_string()), ("y", "Search".into()), ("b", "Back".into())]);
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words() {
        assert_eq!(sort_label(Sort::PriceAsc, 0), "Price: low to high");
        assert_eq!(sort_label(Sort::RarityAsc, 2), "Rarity: rarest first, 2 traits");
        assert_eq!(sort_label(Sort::ListedDesc, 1), "Recently listed, 1 trait");
        let c = CollectionDetail { royalty_bps: Some(500.0), venues: vec!["kiosk".into(), "tradeport".into(), "market".into()], ..Default::default() };
        assert_eq!(royalty_line(&c), "Royalty 5.0%, listed on Kiosk, Tradeport, Rare");
        assert_eq!(royalty_line(&CollectionDetail::default()), "Royalty --");
    }

    #[test]
    fn groups_keep_type_order_and_sort_values() {
        let v = |k: &str, val: &str, n: u64| TraitValue { kind: k.into(), value: val.into(), count: n, ..Default::default() };
        let g = trait_groups(&[v("B", "x", 1), v("A", "y", 5), v("B", "z", 9)]);
        assert_eq!(g.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["B", "A"]);
        assert_eq!(g[0].1[0].value, "z");
    }
}
