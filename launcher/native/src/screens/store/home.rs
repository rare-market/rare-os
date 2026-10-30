//! The Store tab (store/Store.tsx): featured, new, top and on-sale shelves from rare.market's
//! /v1/store/games, and search. A tile opens the game page (`store:game:<slug>`). X searches (the
//! app asks with the on-screen keyboard and calls `set_query`); B leaves the results.

use super::model::StoreGame;
use super::parts::{store_card, trunc_text};
use crate::kit::{self, tok, Btn};
use crate::screens::market::host::*;
use rare_render::input::Pad;
use rare_render::view::{key, El, Look};

/// One shelf read: None while loading, Err when it failed.
pub type Load = Option<Result<Vec<StoreGame>, ()>>;

#[derive(Clone, Debug, Default)]
pub struct StoreData {
    pub now_ms: f64,
    pub featured: Load,
    pub fresh: Load,
    pub top: Load,
    pub sale: Load,
    /// The search's answer (for `query`).
    pub results: Load,
    /// Slugs the current account holds a licence for.
    pub owned: Vec<String>,
}

impl StoreData {
    fn ok(l: &Load) -> Option<&Vec<StoreGame>> {
        l.as_ref().and_then(|r| r.as_ref().ok())
    }
    fn failed(l: &Load) -> bool {
        matches!(l, Some(Err(())))
    }
    pub fn loading(&self, q: bool) -> bool {
        [&self.featured, &self.fresh, &self.top, &self.sale].iter().any(|l| l.is_none()) || (q && self.results.is_none())
    }
}

// Focus tags.
pub const T_SEARCH: u32 = 10;
pub const T_RETRY: u32 = 11;
pub const T_FEATURED: u32 = 1_000;
pub const T_NEW: u32 = 2_000;
pub const T_TOP: u32 = 3_000;
pub const T_SALE: u32 = 4_000;
pub const T_RESULTS: u32 = 5_000;

pub struct StorePage {
    pub data: StoreData,
    pub query: Option<String>,
    pub host: Host,
}

impl StorePage {
    pub fn new(cx: &mut Cx, parent: rare_render::Id, data: StoreData, query: Option<String>, now: f64) -> StorePage {
        let mut p = StorePage { data, query, host: Host::new(cx, parent) };
        p.render(cx, now);
        p
    }

    pub fn render(&mut self, cx: &mut Cx, now: f64) {
        let (d, q) = (&self.data, self.query.as_deref());
        self.host.render(cx, now, |ui| view(ui, d, q));
    }

    /// A search came back from the keyboard: show its results (the app fills `data.results`).
    pub fn set_query(&mut self, cx: &mut Cx, q: Option<String>, now: f64) {
        self.query = q.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        self.data.results = None;
        self.render(cx, now);
        self.land(cx, now);
    }

    /// Results or shelves arriving: focus lands on the first tile.
    pub fn land(&mut self, cx: &mut Cx, now: f64) {
        // focusFirst: the first tile, else the empty state's button, else the search field.
        let first = if self.query.is_some() { T_RESULTS } else { T_FEATURED };
        for t in [first, T_RETRY, T_SEARCH] {
            if self.host.focus_tag(cx, t, now) {
                break;
            }
        }
    }

    /// The game a tag points at.
    fn game_of(&self, tag: u32) -> Option<&StoreGame> {
        let (list, base) = match tag {
            t if t >= T_RESULTS => (&self.data.results, T_RESULTS),
            t if t >= T_SALE => (&self.data.sale, T_SALE),
            t if t >= T_TOP => (&self.data.top, T_TOP),
            t if t >= T_NEW => (&self.data.fresh, T_NEW),
            t if t >= T_FEATURED => (&self.data.featured, T_FEATURED),
            _ => return None,
        };
        StoreData::ok(list)?.get((tag - base) as usize)
    }

    pub fn page_hints(&self) -> Vec<(&'static str, String)> {
        let a = match self.host.tag() {
            Some(T_SEARCH) => Some("Search"),
            Some(T_RETRY) => Some("Select"),
            Some(_) => Some("Details"),
            None => None,
        };
        let mut h: Vec<(&'static str, String)> = a.map(|a| ("a", a.to_string())).into_iter().collect();
        h.push(("b", "Back".into()));
        h.push(("x", "Search store".into()));
        h.push(("y", "Search".into()));
        h
    }

    pub fn press(&mut self, cx: &mut Cx, b: Pad, now: f64) -> Out {
        match b {
            Pad::X => Out::Act("store:search".into()),
            Pad::B if self.query.is_some() => {
                self.query = None;
                self.data.results = None;
                self.render(cx, now);
                self.land(cx, now);
                Out::Moved
            }
            Pad::B => Out::Close,
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                if self.host.step(cx, b, now) {
                    Out::Moved
                } else {
                    Out::None
                }
            }
            Pad::A => match self.host.tag() {
                Some(T_SEARCH) => Out::Act("store:search".into()),
                Some(T_RETRY) if self.query.is_some() => Out::Act("store:search".into()),
                Some(T_RETRY) => Out::Act("store:retry".into()),
                Some(t) => self.game_of(t).map(|g| Out::Act(format!("store:game:{}", g.slug))).unwrap_or(Out::None),
                None => Out::None,
            },
            _ => Out::None,
        }
    }
}

/// `Grid`: grid-cols-3 gap-[1rem] of wide tiles, the first one focus's default.
fn grid(ui: &mut Ui, id: &str, games: &[StoreGame], owned: &[String], base: u32, now: f64) -> El {
    let mut g = ui.bx(s().grid(&[Track::Fr(1.0), Track::Fr(1.0), Track::Fr(1.0)]).gap(1.0)).group(kit::group(id, false));
    for (i, game) in games.iter().enumerate() {
        g = g.child(store_card(ui, game, owned.contains(&game.slug), true, i == 0, base + i as u32, now));
    }
    g
}

/// `Shelf`: a rail that shows only when it has games.
#[allow(clippy::too_many_arguments)]
fn shelf(ui: &mut Ui, id: &str, title: &str, note: Option<&str>, games: Option<&Vec<StoreGame>>, owned: &[String], base: u32, now: f64) -> Option<El> {
    let games = games.filter(|g| !g.is_empty())?;
    let tiles: Vec<El> = games.iter().enumerate().map(|(i, g)| store_card(ui, g, owned.contains(&g.slug), false, false, base + i as u32, now)).collect();
    Some(kit::rail(ui, s().mt(2.25), id, Some(title), note, tiles))
}

/// The page (Store.tsx's JSX).
pub fn view(ui: &mut Ui, d: &StoreData, q: Option<&str>) -> El {
    let now = d.now_ms;
    // data-scroll: padding-top max(1rem, ...) wins over pt-[0.5rem].
    let mut page = ui.bx(s().fill().col().pt(1.0).pb(2.5)).scrolls_y(key("store-scroll"));
    if d.loading(q.is_some()) {
        page = page.child(ui.bx(s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h_px(2.0)).bg(tok::BRAND.alpha(0.6)).key(key("s-load")));
    }

    // Header: title and line, the search field on the right.
    let titles = ui
        .bx(s().col())
        .child(ui.text("Store", T::new(Ty::Title).w(600)));
    // The field's text and icon are currentColor: muted, primary while focused.
    let icon = kit::focus_icon(ui, "search", 1.0, false, tok::MUTED, tok::FG);
    let field = ui
        .bx(s().row().items_center().gap(0.5).h(2.75).w(22.0).px(0.9 + 1.0 / ui.rem).ml_auto())
        .look(Look::default().radius(ui.r(0.25)).border(1.0, crate::kit::tok::BORDER_LIGHT))
        .child(icon)
        .child(trunc_text(ui, s(), q.unwrap_or("Search the store").to_string(), T::new(Ty::Small).c(tok::MUTED)).focus_color(tok::FG));
    let field = fill_nav(field, T_SEARCH, Some("Search"), None, false);
    page = page.child(ui.bx(s().row().items_end().gap(1.0).px(2.5).mb(1.25)).child(titles).child(field).key(key("s-head")));

    let owned = &d.owned;
    if let Some(q) = q {
        let res = StoreData::ok(&d.results);
        let note = res.map(|r| r.len().to_string());
        let mut body = ui.bx(s().col().px(2.5)).child(kit::section_head(ui, s().mb(0.9), &format!("Results for \"{q}\""), note.as_deref(), None));
        body = match (&d.results, res) {
            (_, Some(r)) if r.is_empty() => {
                let b = kit::button(ui, Btn::new("Search again", T_RETRY).primary().def(true));
                body.child(empty(ui, s(), "No games match that search", Some("Try a studio name or a genre."), vec![b]))
            }
            (Some(Err(())), _) => body.child(empty(ui, s(), "Couldn't reach the store", Some("Check the connection and try again."), vec![])),
            _ => body.child(grid(ui, "store-results", res.map(|v| v.as_slice()).unwrap_or(&[]), owned, T_RESULTS, now)),
        };
        return page.child(body.key(key("s-results")));
    }

    if StoreData::failed(&d.featured) && StoreData::failed(&d.fresh) && StoreData::failed(&d.top) {
        let b = kit::button(ui, Btn::new("Try again", T_RETRY).primary().def(true));
        let e = empty(ui, s(), "Couldn't reach the store", Some("Check the connection. The store needs rare.market to show games."), vec![b]);
        return page.child(ui.bx(s().col().px(2.5)).child(e).key(key("s-failed")));
    }

    if let Some(feat) = StoreData::ok(&d.featured).filter(|f| !f.is_empty()) {
        let six: Vec<StoreGame> = feat.iter().take(6).cloned().collect();
        let head = kit::section_head(ui, s().mb(0.9), "Featured", None, None);
        let g = grid(ui, "store-featured", &six, owned, T_FEATURED, now);
        page = page.child(ui.bx(s().col().px(2.5)).child(head).child(g).key(key("s-featured")));
    }
    let new = shelf(ui, "store-new", "New", None, StoreData::ok(&d.fresh), owned, T_NEW, now);
    let top = shelf(ui, "store-top", "Top sellers", Some("30 days"), StoreData::ok(&d.top), owned, T_TOP, now);
    let sale = shelf(ui, "store-sale", "On sale", None, StoreData::ok(&d.sale), owned, T_SALE, now);
    page.maybe(new.map(|e| e.key(key("s-new")))).maybe(top.map(|e| e.key(key("s-top")))).maybe(sale.map(|e| e.key(key("s-sale"))))
}

impl Page for StorePage {
    fn hints(&self) -> Vec<(&'static str, String)> {
        self.page_hints()
    }
}

impl Screen for StorePage {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.host.touch_focus(cx, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        self.press(cx, pad, now)
    }
    fn tick(&mut self, cx: &mut Cx, now: f64) {
        if self.host.tick(cx, now) {
            self.render(cx, now);
        }
    }
}

/// `Empty`: kit::empty with a one-line body. The kit's wrapping body measures at min-content
/// inside `items-start` and breaks every line (request filed); the store's lines are short.
fn empty(ui: &mut Ui, st: S, title: &str, body: Option<&str>, actions: Vec<El>) -> El {
    let mut b = ui
        .bx(st.col().items_start().gap(0.5).px(1.5 + 1.0 / ui.rem).py(1.3 + 1.0 / ui.rem))
        .look(Look::default().bg(tok::L1).radius(ui.r(0.5)).border(1.0, tok::HAIR))
        .child(ui.text(title, T::new(Ty::Row).w(600)));
    if let Some(body) = body {
        b = b.child(ui.text_s(s().max_w(34.0), body, T::new(Ty::Small).c(tok::MUTED)));
    }
    if !actions.is_empty() {
        b = b.child(ui.bx(s().row().gap(0.75).mt(0.5)).kids(actions));
    }
    b
}
