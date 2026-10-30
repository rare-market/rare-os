//! The Rare menu shell and the Game tab (overlay/RareMenu.tsx, GameTab.tsx, menuModel.ts): a 32rem
//! aside over the dimmed frame of the game, a tab strip (LT/RT), the tab body, and footer hints.
//! Other tabs are filled by `overlay::tabs` and `overlay::panels`.
//!
//! Hooks the app wires when backends land: every action comes back as `Out::Act(name)`:
//! `resume`, `screenshot`, `save-clip`, `performance`, `steam-menu`, `steam-page:<page>`,
//! `emu:<save|load|menu>`, `details`, `quit`, and for the other tabs `<tab>:<sid or tag>`
//! (`items` rows open the item page here, with `Nav` tags = item index). B closes with `Out::Close`.

pub mod ask;
pub mod deal;
pub mod game;
pub mod live;
pub mod media;
pub mod model;

use super::common::*;
use super::fixtures::{self, FRAME};
use super::lay::s;
use super::panels::{self, item_detail, items_tab, market_tab, trophies_tab};
use super::tabs::{friends_tab, notices_tab, rare_tab, Where};
use model::*;
use rare_render::input::Pad;
use rare_render::theme::tok;
use rare_render::{Color, Id, Node, Src};

pub struct Perf {
    pub profile: String,
    pub tdp: Option<f32>,
}

pub struct GameInfo {
    pub title: String,
    pub banner: Option<Src>,
    pub logo: Option<Src>,
}

pub struct EmuAction {
    /// "save", "load", "menu"; RetroArch's "save:<slot>", "load:<slot>", "ff", "rewind", "disc".
    pub id: String,
    pub label: String,
    pub ready: bool,
    pub note: Option<String>,
    /// A state slot's picture.
    pub thumb: Option<Src>,
    /// A state slot's line ("5 min ago").
    pub sub: Option<String>,
}

/// Everything the menu shows (the web hooks' results): fixtures until the backends are wired.
pub struct MenuData {
    /// Over the native overlay session (a frozen frame) rather than the launcher's own sheet.
    pub session: bool,
    pub over_game: bool,
    pub rid: Option<String>,
    pub frame: Option<Src>,
    pub guest: bool,
    pub game: GameInfo,
    pub is_app: bool,
    pub in_library: bool,
    pub address: Option<String>,
    /// What the header calls the account: the Rare username, else its name, else None (the short address).
    pub name: Option<String>,
    pub signed_out: bool,
    pub collections: bool,
    pub trophies: bool,
    /// Your items from this game (None until loaded).
    pub items: Option<Vec<OverlayItem>>,
    pub unread: u32,
    pub trophies_count: u32,
    pub loading: bool,
    pub perf: Option<Perf>,
    pub power_saved: bool,
    pub started_ms: Option<f64>,
    pub now_ms: f64,
    pub emu: Vec<EmuAction>,
    pub running: bool,
    pub taking: Option<&'static str>,
    pub invite: bool,
    pub tab: Option<MenuTab>,
    /// The Sui game's collections (Move type, name): the Items and Market tabs load them live.
    /// Empty in fixtures and for games that are not on rare.market.
    pub colls: Vec<live::Coll>,
    /// Instant replay's length when it is on (None: off).
    pub replay: Option<u32>,
    /// Discord is open: (muted, deafened), for its rows.
    pub discord: Option<(bool, bool)>,
    /// Over the launcher with something running: its title (the Rare tab's Resume row).
    pub resume: Option<String>,
}

pub struct RareMenu {
    d: MenuData,
    nav: Nav,
    tabs: Vec<MenuTab>,
    /// The tab strip's cells as last drawn (touch: a tap on a tab opens it).
    tab_cells: Vec<(Id, MenuTab)>,
    tab: MenuTab,
    detail: Option<usize>,
    strip: Id,
    body: Id,
    bar: Id,
    footer: Id,
    refs: game::GameRefs,
    armed: Option<f64>,
    quitting: bool,
    load: Option<std::sync::mpsc::Receiver<live::Loaded>>,
    poll: Option<f64>,
    /// A Sui game's live collections: the item page looks up offers and can sell and list.
    live: bool,
    look: Option<std::sync::mpsc::Receiver<(String, panels::OfferState, Option<rare_market::inventory::trade::Quote>)>>,
    quote: Option<rare_market::inventory::trade::Quote>,
    deal: Option<std::sync::mpsc::Receiver<Result<deal::Did, String>>>,
    /// When a trade went out: the page says "Review and approve" once the build has had its moment.
    dealt_at: f64,
    /// Reload the tabs at this time (after a trade or a purchase).
    reload_at: Option<f64>,
    /// Now playing as last drawn (rare_system::media::version), and when to look again.
    media_ver: u64,
    media_at: Option<f64>,
    /// Ask Rare: the conversation as last drawn, when to look again, the picture switch.
    ask_ver: u64,
    ask_at: Option<f64>,
    shoot: bool,
}

/// The tabs that fit in `room` rem around `on` (estimated from the labels: text-small medium is
/// about half a rem a letter, a count adds about one, tabs are 1.25rem apart): (first, end).
pub fn tab_window(labels: &[(&str, Option<u32>)], on: usize, room: f32) -> (usize, usize) {
    let w = |i: usize| labels[i].0.chars().count() as f32 * 0.5 + if labels[i].1.is_some() { 1.0 } else { 0.0 };
    const GAP: f32 = 1.25;
    let (mut from, mut to) = (on, on + 1);
    let mut used = w(on);
    // Right first (the tabs after the current one are where RT goes), then left.
    loop {
        let mut grew = false;
        if to < labels.len() && used + GAP + w(to) <= room {
            used += GAP + w(to);
            to += 1;
            grew = true;
        }
        if from > 0 && used + GAP + w(from - 1) <= room {
            used += GAP + w(from - 1);
            from -= 1;
            grew = true;
        }
        if !grew {
            break;
        }
    }
    (from, to)
}

fn short(a: &str) -> String {
    if a.len() > 10 {
        format!("{}...{}", &a[..6], &a[a.len() - 4..])
    } else {
        a.to_string()
    }
}

/// A stand-in for the identicon: a round swatch from the address (the real art arrives with rare-images).
fn avatar_color(a: &str) -> Color {
    let h = a.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    let (r, g, b) = (60 + (h & 0x7f) as u8, 60 + ((h >> 8) & 0x7f) as u8, 90 + ((h >> 16) & 0x7f) as u8);
    Color::rgb(r, g, b)
}

impl RareMenu {
    pub fn build(cx: &mut Cx, mut d: MenuData, now: f64) -> RareMenu {
        let r = cx.r();
        // A Sui game's Items and Market: load them now, the answer lands in `tick`.
        let live = d.over_game && !d.colls.is_empty();
        let load = if live {
            d.collections = true;
            d.loading = true;
            let account = if d.address.is_some() { panels::Account::Signer } else { panels::Account::SignedOut };
            panels::set_data(panels::live_data(&d.game.title, account, None, None));
            Some(live::spawn(d.address.clone(), d.colls.clone()))
        } else {
            None
        };
        let tabs = menu_tabs(d.over_game, d.collections, d.trophies);
        let tab = d.tab.filter(|t| tabs.contains(t)).unwrap_or_else(|| if d.invite { MenuTab::Friends } else { opening_tab(d.over_game, d.unread) });
        let kind = game_kind(d.rid.as_deref());
        let root = cx.scene.root;
        let page = cx.scene.add(root, s().fill().row().done(), Node::default());
        if d.session {
            frozen(cx, page, d.frame.clone());
        } else {
            cx.scene.add(page, s().fill().done(), Node::bg(Color::BLACK.alpha(0.55)));
        }

        // Left: the game and the hints, bottom aligned.
        let left = cx.scene.add(page, s().col().grow().justify_end().pad(2.5 * r, 0.0, 2.5 * r, 2.25 * r).done(), Node::default());
        if d.over_game {
            let head = cx.scene.add(left, s().row().items_center().gap(0.9 * r).done(), Node::default());
            if let Some(src) = d.game.logo.clone() {
                let w = 2.75 * r;
                let b = cx.scene.add(head, s().wh(w, w).done(), Node::bg(tok::L3).radius(0.5 * r).clip());
                let k = cx.img.request(rare_render::ImgReq { src, w: w as u32, h: w as u32, fit: rare_render::Fit::Contain });
                cx.scene.add(b, s().fill().done(), Node::default().image(k).contain([0.5, 0.5]));
            }
            let names = cx.scene.add(head, s().col().shrink().done(), Node::default());
            text_tr(cx, names, ty::TITLE, 600, tok::FG, &d.game.title);
            text(cx, names, ty::SMALL, 400, tok::FG.alpha(0.75), if kind == Some(GameKind::App) { "Open under Rare" } else { "Running under Rare" });
        }
        if d.session {
            let hints = cx.scene.add(left, s().row().items_center().gap(1.25 * r).mt(1.0 * r).done(), Node::default());
            let back = if d.over_game {
                if kind == Some(GameKind::App) { "Back to the app" } else { "Back to game" }
            } else {
                "Close"
            };
            let dim = tok::FG.alpha(0.8);
            let item = |cx: &mut Cx, label_a: Option<&str>, g: G, label_b: Option<&str>| {
                let h = cx.scene.add(hints, s().row().items_center().gap(0.45 * r).done(), Node::default());
                if let Some(l) = label_a {
                    text(cx, h, ty::SMALL, 400, dim, l);
                }
                glyph(cx, h, g, false);
                if let Some(l) = label_b {
                    text(cx, h, ty::SMALL, 400, dim, l);
                }
            };
            item(cx, None, G::B, Some(back));
            if !d.guest {
                item(cx, None, G::Rare, Some("Home"));
                item(cx, Some("Hold"), G::Rare, Some("to switch games"));
            }
        }

        // The aside.
        let aside = cx.scene.add(page, s().col().w(32.0 * r).hp(1.0).done(), Node::bg(tok::L1).shadow(&[rare_render::scene::Shadow::rem(-2.0, 0.0, 4.0, 0.45, r)]));
        edge(cx, aside, 3, tok::HAIR);
        cx.scene.n(aside).tx.restart(32.0 * r, 0.0, now, 0.2);
        let header = cx.scene.add(aside, s().col().gap(0.9 * r).pad(1.4 * r, 1.3 * r, 1.4 * r, 0.0).done(), Node::default());
        let top = cx.scene.add(header, s().row().items_center().gap(0.6 * r).done(), Node::default());
        text(cx, top, ty::ROW, 600, tok::FG, "Rare");
        if d.over_game {
            text_tr(cx, top, ty::SMALL, 400, tok::MUTED, &format!("in {}", d.game.title));
        }
        if let Some(a) = &d.address {
            let w = cx.scene.add(top, s().row().items_center().gap(0.4 * r).ml_auto().no_shrink_().done(), Node::default());
            let pic = crate::data::live::picture(a);
            if crate::screens::overlay::common::player_avatar(cx, w, a, Some(&pic), 1.1 * r).is_none() {
                cx.scene.add(w, s().wh(1.1 * r, 1.1 * r).done(), Node::bg(avatar_color(a)).radius(0.55 * r));
            }
            match d.name.as_deref().filter(|n| !n.is_empty()) {
                Some(n) => {
                    text_tr(cx, w, ty::META, 500, tok::MUTED, n);
                }
                None => {
                    let t = text(cx, w, ty::META, 400, tok::MUTED, &short(a));
                    if let Some(ts_) = cx.scene.n(t).text.as_mut() {
                        ts_.style.mono = true;
                    }
                }
            }
        }
        let strip = cx.scene.add(header, s().col().gap(0.9 * r).done(), Node::default());

        let wrap = cx.scene.add(aside, s().grow().col().done(), Node::default().clip());
        let scroller = cx.scene.add(wrap, s().fill().col().done(), Node::default().scrolls());
        let inner = cx.scene.add(scroller, s().col().wp(1.0).pad(0.65 * r, 0.6 * r, 0.65 * r, 1.0 * r).done(), Node::default());
        let bar = load_bar(cx, wrap, false);

        let footer = cx.scene.add(aside, s().row().items_center().gap(1.1 * r).h(3.0 * r).px(1.4 * r).done(), Node::default());
        edge(cx, footer, 0, tok::HAIR);

        let mut m = RareMenu {
            d,
            nav: Nav::default(),
            tabs,
            tab_cells: Vec::new(),
            tab,
            detail: None,
            strip,
            body: inner,
            bar,
            footer,
            refs: game::GameRefs { quit_title: None, quit_sub: None, quit_what: String::new(), quit_label: String::new() },
            armed: None,
            quitting: false,
            load,
            poll: None,
            live,
            look: None,
            quote: None,
            deal: None,
            dealt_at: 0.0,
            reload_at: None,
            media_ver: rare_system::media::version(),
            media_at: None,
            ask_ver: 0,
            ask_at: None,
            shoot: crate::features::agent::setup().shots,
        };
        m.nav.scrollers.push(scroller);
        m.rebuild(cx, now);
        m
    }

    fn where_(&self) -> Where {
        Where { over_game: self.d.over_game, resume: self.d.resume.clone() }
    }

    /// The header's tab strip (and the Items stat line), the body and the footer for the current tab.
    fn rebuild(&mut self, cx: &mut Cx, now: f64) {
        let r = cx.r();
        let kind = game_kind(self.d.rid.as_deref());
        // A new body starts at the top.
        if let Some(sc) = cx.scene.parent(self.body) {
            cx.scene.n(sc).scroll_y.snap(0.0);
        }
        // Strip.
        cx.scene.clear_children(self.strip);
        let items: Vec<OverlayItem> = self.d.items.clone().unwrap_or_default();
        if self.tab == MenuTab::Items && self.detail.is_none() && self.d.address.is_some() && !items.is_empty() {
            let worth: u128 = items.iter().map(|x| x.floor_mist.unwrap_or(0)).sum();
            let offers: u128 = items.iter().map(|x| x.top_offer_mist.unwrap_or(0)).sum();
            let cells = [("Items", Some(items.len().to_string()), false), ("At floor", (worth > 0).then(|| fmt_sui2(worth)), true), ("Best offers", (offers > 0).then(|| fmt_sui2(offers)), true)];
            let dl = cx.scene.add(self.strip, s().row().done(), Node::default());
            for (i, (l, v, sui)) in cells.iter().enumerate() {
                let pr = if i < 2 { 1.0 * r } else { 0.0 };
                let pl = if i > 0 { 1.0 * r } else { 0.0 };
                let cell = cx.scene.add(dl, s().col().grow().pad(pl, 0.0, pr, 0.0).done(), Node::default());
                if i > 0 {
                    edge(cx, cell, 3, tok::HAIR);
                }
                text(cx, cell, ty::LABEL, 400, tok::MUTED, l);
                let line = cx.scene.add(cell, s().row().items_center().gap(0.25 * r).done(), Node::default());
                if let Some(v) = v {
                    text(cx, line, ty::BODY, 500, tok::FG, v);
                    if *sui {
                        text(cx, line, ty::BODY, 400, tok::MUTED, "SUI");
                    }
                }
            }
        }
        if self.detail.is_none() {
            let bar = cx.scene.add(self.strip, s().row().items_stretch().gap(0.8 * r).h(2.4 * r).done(), Node::default());
            edge(cx, bar, 2, tok::HAIR);
            let l = cx.scene.add(bar, s().row().items_center().done(), Node::default());
            glyph(cx, l, G::Lt, true);
            let labels: Vec<(&str, Option<u32>)> = self
                .tabs
                .iter()
                .map(|t| {
                    let n = match t {
                        MenuTab::News => self.d.unread,
                        MenuTab::Trophies => self.d.trophies_count,
                        _ => 0,
                    };
                    (t.label(), (n > 0).then_some(n))
                })
                .collect();
            let on = self.tabs.iter().position(|t| *t == self.tab).unwrap_or(0);
            let tr = cx.scene.add(bar, s().row().items_stretch().shrink().done(), Node::default().clip());
            // Seven tabs (a Sui game with items and trophies) are wider than the aside: the strip
            // shows the run of tabs around the current one that fits, LT and RT reach the rest.
            let (from, to) = tab_window(&labels, on, 24.0);
            let row = tabs(cx, tr, &labels[from..to], on - from, true);
            self.tab_cells = cx.scene.children(row).into_iter().zip(self.tabs[from..to].iter().copied()).collect();
            let rr = cx.scene.add(bar, s().row().items_center().ml_auto().done(), Node::default());
            glyph(cx, rr, G::Rt, true);
        }
        // Footer.
        cx.scene.clear_children(self.footer);
        edge(cx, self.footer, 0, tok::HAIR);
        // The first tab shows X and Y instead of Tabs (the tab strip has LT and RT on it), so the
        // footer fits the 32rem panel at every size.
        let shortcuts = matches!(self.tab, MenuTab::Game | MenuTab::Rare) && self.detail.is_none();
        if self.detail.is_none() && !shortcuts {
            let h = cx.scene.add(self.footer, s().row().items_center().gap(0.45 * r).done(), Node::default());
            glyph_pair(cx, h, G::Lt, G::Rt, false);
            text(cx, h, ty::SMALL, 400, tok::FG.alpha(0.75), "Tabs");
        }
        hint(cx, self.footer, G::A, "Select");
        if self.tab == MenuTab::Ask && self.detail.is_none() && ask::info().voice {
            hint(cx, self.footer, G::X, "Hold to talk");
        }
        if shortcuts {
            hint(cx, self.footer, G::X, "Steam");
            hint(cx, self.footer, G::Y, "Search settings");
        }
        let back = if self.detail.is_some() {
            "Back"
        } else if self.d.over_game {
            if kind == Some(GameKind::App) { "Back to the app" } else { "Back to game" }
        } else {
            "Close"
        };
        let bw = cx.scene.add(self.footer, s().ml_auto().done(), Node::default());
        hint(cx, bw, G::B, back);

        // Body.
        cx.scene.clear_children(self.body);
        self.nav.clear();
        self.refs = game::GameRefs { quit_title: None, quit_sub: None, quit_what: String::new(), quit_label: String::new() };
        let loading = self.d.loading && matches!(self.tab, MenuTab::Items | MenuTab::Market | MenuTab::Trophies | MenuTab::Game);
        cx.scene.n(self.bar).opacity.snap(if loading { 1.0 } else { 0.0 });
        let w = self.where_();
        let body = self.body;
        let signed_out = self.d.signed_out && matches!(self.tab, MenuTab::Items | MenuTab::Market);
        if signed_out {
            let c = cx.scene.add(body, s().col().items_start().gap(0.75 * r).pad(0.75 * r, 0.4 * r, 0.75 * r, 0.0).done(), Node::default());
            let t = cx.scene.add_text(cx.t, c, s().done(), "Sign in with a passkey to see your items.", ts(r, ty::SMALL.0, ty::SMALL.1, 400).wrap(), tok::MUTED);
            let _ = t;
            let b = button(cx, c, "Sign in", Variant::Primary, Sz::Md, false).tag(1).hint("Sign in");
            self.nav.add(b);
        } else {
            match (self.tab, self.detail) {
                (MenuTab::Game, _) => self.refs = game::build(cx, &mut self.nav, body, &self.d, now),
                (MenuTab::Rare, _) => {
                    let col = cx.scene.add(body, s().col().gap(0.6 * r).done(), Node::default());
                    media::build(cx, &mut self.nav, col);
                    rare_tab(cx, &mut self.nav, col, &w, now)
                }
                (MenuTab::Ask, _) => {
                    self.ask_ver = crate::features::agent::version();
                    let info = self.ask_info();
                    let land = ask::build(cx, &mut self.nav, body, &crate::features::agent::view(), &info);
                    if let Some(id) = self.nav.items.iter().find(|f| f.sid == land).map(|f| f.id) {
                        self.nav.focus(cx.scene, id, now, false);
                    }
                }
                (MenuTab::Friends, _) => friends_tab(cx, &mut self.nav, body, &w, now),
                (MenuTab::News, _) => notices_tab(cx, &mut self.nav, body, &w, now),
                (MenuTab::Items, Some(i)) | (MenuTab::Market, Some(i)) => item_detail(cx, &mut self.nav, body, i, now),
                (MenuTab::Items, None) => items_tab(cx, &mut self.nav, body, now),
                (MenuTab::Market, None) => market_tab(cx, &mut self.nav, body, now),
                (MenuTab::Trophies, _) => trophies_tab(cx, &mut self.nav, body, now),
            }
        }
        // Land on the first row: Resume on Game, Join on an invite, else the tab's first.
        let join = if self.d.invite && self.tab == MenuTab::Friends { self.nav.items.iter().find(|f| f.sid == "invite-join").map(|f| f.id) } else { None };
        // Now playing shown: look again every second (another song, paused elsewhere).
        let shown = self.nav.items.iter().any(|f| media::cmd_of(&f.sid).is_some());
        self.media_at = shown.then_some(now + 1.0);
        let first = self.nav.items.iter().find(|f| media::cmd_of(&f.sid).is_none()).or(self.nav.items.first()).map(|f| f.id);
        crate::features::agent::set_shown(self.tab == MenuTab::Ask);
        if self.tab == MenuTab::Ask && self.nav.cur.is_some() {
            // Ask landed on its own default (Talk or Type).
        } else if let Some(id) = join.or(first) {
            self.nav.focus(cx.scene, id, now, false);
        }
        cx.scene.mark_dirty();
    }

    /// The game's items and market arrived: the tabs and the Game tab's items line show them.
    fn loaded(&mut self, cx: &mut Cx, l: live::Loaded, now: f64) {
        let account = if self.d.address.is_some() { panels::Account::Signer } else { panels::Account::SignedOut };
        self.d.items = Some(live::overlay_items(&l.items));
        self.d.loading = false;
        panels::set_data(panels::live_data(&self.d.game.title, account, Some(l.items), Some(l.market)));
        self.detail = None;
        // Leave the Game tab's focus alone unless it is still on Resume.
        let resting = self.nav.items.first().is_some_and(|f| self.nav.current().is_some_and(|c| c.id == f.id));
        if self.tab != MenuTab::Game || resting {
            self.rebuild(cx, now);
        } else {
            cx.scene.n(self.bar).opacity.snap(0.0);
        }
    }

    /// The id of the item whose page is open.
    fn open_item_id(&self) -> Option<String> {
        let i = self.detail?;
        panels::data().items.and_then(|v| v.get(i).map(|x| x.id.clone()))
    }

    fn set_view(&self, f: impl FnOnce(&mut panels::DetailView, &mut panels::Phase)) {
        let mut d = panels::data();
        f(&mut d.detail, &mut d.phase);
        panels::set_data(d);
    }

    /// Rebuilds and puts focus back on the row with this sid (else the first).
    fn rebuild_keep(&mut self, cx: &mut Cx, sid: &str, now: f64) {
        self.rebuild(cx, now);
        if let Some(id) = self.nav.items.iter().find(|f| f.sid == sid).map(|f| f.id) {
            self.nav.focus(cx.scene, id, now, false);
        }
    }

    /// An Items row opens its page. For a Sui game, the best offer is looked up for it (ItemDetail's
    /// bestOffer) and the listing price starts at the floor.
    fn open_detail(&mut self, cx: &mut Cx, index: usize, now: f64) {
        self.detail = Some(index);
        self.quote = None;
        self.look = None;
        self.deal = None;
        if self.live {
            let account = if self.d.address.is_some() { panels::Account::Signer } else { panels::Account::SignedOut };
            let item = panels::data().items.and_then(|v| v.get(index).cloned());
            let looking = self.d.address.is_some() && item.is_some();
            let floor = item.as_ref().and_then(|i| i.floor_mist);
            self.set_view(|v, p| {
                *v = panels::DetailView { account, offer: if looking { panels::OfferState::Looking } else { panels::OfferState::Nothing }, price: floor };
                *p = panels::Phase::Idle;
            });
            if let (Some(it), Some(addr)) = (item, self.d.address.clone()) {
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let (state, q) = deal::lookup(&it.id, &addr);
                    let _ = tx.send((it.id, state, q));
                });
                self.look = Some(rx);
            }
        }
        self.rebuild(cx, now);
    }

    /// The item page's buttons: Sell, List and the price choices. None when it is not one of them.
    fn item_act(&mut self, cx: &mut Cx, sid: &str, now: f64) -> Option<Out> {
        let busy = self.deal.is_some() || matches!(panels::data().phase, panels::Phase::Building(_) | panels::Phase::Signing);
        match sid {
            "sell" | "list" => {
                if busy {
                    return Some(Out::None);
                }
                let (Some(id), Some(addr)) = (self.open_item_id(), self.d.address.clone()) else { return Some(Out::None) };
                let (tx, rx) = std::sync::mpsc::channel();
                if sid == "sell" {
                    let Some(q) = self.quote.clone() else { return Some(Out::None) };
                    std::thread::spawn(move || {
                        let _ = tx.send(deal::sell(&id, &addr, &q));
                    });
                    self.set_view(|_, p| *p = panels::Phase::Building("Getting the sale ready...".into()));
                } else {
                    let Some(price) = panels::data().detail.price else { return Some(Out::None) };
                    std::thread::spawn(move || {
                        let _ = tx.send(deal::list(&id, &addr, price));
                    });
                    self.set_view(|_, p| *p = panels::Phase::Building("Getting the listing ready...".into()));
                }
                self.deal = Some(rx);
                self.dealt_at = now;
                self.rebuild_keep(cx, sid, now);
                Some(Out::Moved)
            }
            "price:other" => Some(Out::Act("items:price-other".into())),
            s if s.starts_with("price:") => {
                let d = panels::data();
                let it = self.detail.and_then(|i| d.items.as_ref().and_then(|v| v.get(i)));
                let pick = it.and_then(|it| panels::price_choices(it.floor_mist, it.listing.as_ref().map(|l| l.price_mist)).into_iter().find(|c| format!("price:{}", c.1) == s).map(|c| c.2));
                if let Some(p) = pick {
                    self.set_view(|v, _| v.price = Some(p));
                    self.rebuild_keep(cx, s, now);
                }
                Some(Out::Moved)
            }
            _ => None,
        }
    }

    /// Reads the tabs again (after a sale, a listing or a purchase).
    fn reload(&mut self) {
        if self.live && self.load.is_none() {
            self.load = Some(live::spawn(self.d.address.clone(), self.d.colls.clone()));
        }
    }

    /// Answers of the lookup and the trade, the reload after one.
    fn deals(&mut self, cx: &mut Cx, now: f64) {
        use std::sync::mpsc::TryRecvError;
        if let Some(rx) = &self.look {
            match rx.try_recv() {
                Ok((id, state, q)) => {
                    self.look = None;
                    if self.open_item_id().as_deref() == Some(id.as_str()) {
                        self.quote = q;
                        self.set_view(|v, _| v.offer = state);
                        self.rebuild(cx, now);
                    }
                }
                Err(TryRecvError::Empty) => {}
                Err(_) => {
                    self.look = None;
                    self.set_view(|v, _| v.offer = panels::OfferState::Nothing);
                    self.rebuild(cx, now);
                }
            }
        }
        if let Some(rx) = &self.deal {
            let r = match rx.try_recv() {
                Ok(r) => Some(r),
                Err(TryRecvError::Empty) => None,
                Err(_) => Some(Err(String::new())),
            };
            if let Some(r) = r {
                self.deal = None;
                let phase = match r {
                    Ok(deal::Did::Sold(net)) => {
                        self.reload_at = Some(now + 4.0);
                        panels::Phase::Done(format!("Sold for {} SUI. It can take a minute to show.", panels::money::fmt_sui(Some(net), panels::money::Fmt::trim(3))))
                    }
                    Ok(deal::Did::Listed(p)) => {
                        self.reload_at = Some(now + 4.0);
                        panels::Phase::Done(format!("Listed for {} SUI.", panels::money::fmt_sui(Some(p), panels::money::Fmt::trim(3))))
                    }
                    Err(e) if e == rare_market::inventory::notice::SIGN_REJECTED => panels::Phase::Idle,
                    Err(e) => panels::Phase::Error(panels::trade::plain_error(&e)),
                };
                self.set_view(|_, p| *p = phase);
                self.rebuild(cx, now);
            }
        } else if self.reload_at.is_none() && live::take_reload() {
            // A purchase from the Market tab finished: the tabs read again once the chain has it.
            self.reload_at = Some(now + 3.0);
        }
        if self.deal.is_some() && self.dealt_at > 0.0 && now >= self.dealt_at + 1.5 && matches!(panels::data().phase, panels::Phase::Building(_)) {
            self.set_view(|_, p| *p = panels::Phase::Signing);
            self.rebuild_keep(cx, "sell", now);
        }
        if self.reload_at.is_some_and(|t| now >= t) && self.load.is_none() {
            self.reload_at = None;
            self.reload();
        }
    }

    /// Now playing changed (another song, paused, a player came or went): the Game and Rare tabs
    /// draw it again with focus where it was. Not while Quit is armed or an item page is open.
    fn media_tick(&mut self, cx: &mut Cx, now: f64) {
        if self.media_at.is_none_or(|t| now < t) {
            return;
        }
        self.media_at = Some(now + 1.0);
        let v = rare_system::media::version();
        if v == self.media_ver {
            return;
        }
        self.media_ver = v;
        if !matches!(self.tab, MenuTab::Game | MenuTab::Rare) || self.detail.is_some() || self.armed.is_some() || self.quitting {
            return;
        }
        let (tag, sid) = self.nav.current().map(|f| (f.tag, f.sid.clone())).unwrap_or_default();
        self.rebuild(cx, now);
        let again = self.nav.items.iter().find(|f| if sid.is_empty() { f.tag == tag && f.tag != 0 } else { f.sid == sid }).map(|f| f.id);
        if let Some(id) = again {
            self.nav.focus(cx.scene, id, now, false);
        }
    }

    fn ask_info(&self) -> ask::AskInfo {
        ask::AskInfo { shoot: self.shoot, over_game: self.d.over_game, ..ask::info() }
    }

    /// A press on the Ask tab.
    fn ask_act(&mut self, cx: &mut Cx, sid: &str, now: f64) -> Out {
        use crate::features::agent as ag;
        match sid {
            "ask-talk" => {
                if !ask::info().voice {
                    return Out::Act("ask:type".into());
                }
                if ag::listening() {
                    ag::stop_listen();
                } else {
                    ag::listen(ag::Source::Menu);
                }
                self.ask_refresh(cx, now);
                Out::Moved
            }
            "ask-type" => Out::Act("ask:type".into()),
            "ask-stop" => {
                ag::stop();
                Out::Moved
            }
            "ask-shot" => {
                self.shoot = !self.shoot;
                self.ask_refresh(cx, now);
                Out::Moved
            }
            "ask-clear" => {
                ag::clear();
                self.ask_refresh(cx, now);
                Out::Moved
            }
            "ask-setup" => Out::Act("ask:setup".into()),
            _ => Out::None,
        }
    }

    /// The conversation moved: the Ask tab draws again with focus and scroll where they were.
    fn ask_refresh(&mut self, cx: &mut Cx, now: f64) {
        if self.tab != MenuTab::Ask || self.detail.is_some() {
            return;
        }
        let keep = self.nav.current().map(|f| f.sid.clone());
        self.ask_ver = crate::features::agent::version();
        cx.scene.clear_children(self.body);
        self.nav.clear();
        let info = self.ask_info();
        let land = ask::build(cx, &mut self.nav, self.body, &crate::features::agent::view(), &info);
        let pick = keep.and_then(|k| self.nav.items.iter().find(|f| f.sid == k).map(|f| f.id)).or_else(|| self.nav.items.iter().find(|f| f.sid == land).map(|f| f.id)).or(self.nav.items.first().map(|f| f.id));
        if let Some(id) = pick {
            self.nav.focus(cx.scene, id, now, false);
        }
        // The footer's X hint follows the voice setting.
        cx.scene.mark_dirty();
    }

    fn ask_tick(&mut self, cx: &mut Cx, now: f64) {
        if self.tab != MenuTab::Ask {
            self.ask_at = None;
            return;
        }
        let v = crate::features::agent::version();
        if v != self.ask_ver && self.ask_at.is_none_or(|t| now >= t) {
            self.ask_refresh(cx, now);
            // At most about 12 redraws a second while words stream in.
            self.ask_at = Some(now + 0.08);
        } else if v == self.ask_ver {
            self.ask_at = None;
        }
    }

    fn switch_to(&mut self, cx: &mut Cx, tab: MenuTab, now: f64) {
        self.tab = tab;
        self.detail = None;
        self.rebuild(cx, now);
    }

    fn arm(&mut self, cx: &mut Cx, now: f64) {
        self.armed = Some(now + 4.0);
        self.quit_texts(cx, "Quit now", "Unsaved progress is lost. Press A again to quit.");
        self.set_quit_hint("Quit now");
    }

    fn disarm(&mut self, cx: &mut Cx) {
        self.armed = None;
        let (l, w) = (self.refs.quit_label.clone(), self.refs.quit_what.clone());
        self.quit_texts(cx, &l, &w);
        self.set_quit_hint(&l);
    }

    fn quit_texts(&mut self, cx: &mut Cx, title: &str, sub: &str) {
        if let Some(t) = self.refs.quit_title {
            cx.scene.set_text(cx.t, t, title);
        }
        if let Some(t) = self.refs.quit_sub {
            cx.scene.set_text(cx.t, t, sub);
        }
    }

    fn set_quit_hint(&mut self, h: &str) {
        if let Some(f) = self.nav.items.iter_mut().find(|f| f.tag == game::T_QUIT) {
            f.hint = h.to_string();
        }
    }

    fn act(&mut self, cx: &mut Cx, tag: u32, sid: String, now: f64) -> Out {
        if let Some(c) = media::cmd_of(&sid) {
            media::run(c);
            self.media_at = Some(now + 0.15);
            return Out::Moved;
        }
        if self.tab == MenuTab::Ask {
            return self.ask_act(cx, &sid, now);
        }
        if self.tab != MenuTab::Game {
            // A market row opens the Buy sheet (the trade screens), not the in-place item page.
            if self.tab == MenuTab::Market && sid.is_empty() {
                return Out::Act(format!("market:{tag}"));
            }
            if self.tab == MenuTab::Items && self.detail.is_none() && sid.is_empty() {
                self.open_detail(cx, tag as usize, now);
                return Out::Moved;
            }
            if self.live && self.tab == MenuTab::Items && self.detail.is_some() {
                if let Some(o) = self.item_act(cx, &sid, now) {
                    return o;
                }
            }
            let what = if sid.is_empty() { tag.to_string() } else { sid };
            let tab = match self.tab {
                MenuTab::Rare => "rare",
                MenuTab::Friends => "friends",
                MenuTab::News => "news",
                MenuTab::Trophies => "trophies",
                MenuTab::Items => "items",
                _ => "market",
            };
            return Out::Act(format!("{tab}:{what}"));
        }
        match tag {
            game::T_RESUME => Out::Act("resume".into()),
            game::T_ITEMS => {
                self.switch_to(cx, MenuTab::Items, now);
                Out::Moved
            }
            game::T_SCREENSHOT => Out::Act("screenshot".into()),
            game::T_CLIP => Out::Act("save-clip".into()),
            game::T_DISCORD_MUTE | game::T_DISCORD_DEAFEN => {
                let (m, d) = self.d.discord.unwrap_or((false, false));
                let mute = tag == game::T_DISCORD_MUTE;
                self.d.discord = Some(if mute { (!m, d) } else { (m, !d) });
                self.rebuild_keep(cx, if mute { "discord-mute" } else { "discord-deafen" }, now);
                Out::Act(if mute { "discord-mute" } else { "discord-deafen" }.into())
            }
            game::T_PERF => Out::Act("performance".into()),
            game::T_STEAM => Out::Act("steam-menu".into()),
            game::T_OPEN_STEAM => Out::Act("rare:steam".into()),
            game::T_DETAILS => Out::Act("details".into()),
            game::T_QUIT => {
                if self.quitting {
                    return Out::None;
                }
                if self.armed.is_none() {
                    self.arm(cx, now);
                    return Out::Moved;
                }
                self.armed = None;
                self.quitting = true;
                self.quit_texts(cx, "Quitting...", "");
                Out::Act("quit".into())
            }
            t if (game::T_STEAM_PAGE..game::T_STEAM_PAGE + 4).contains(&t) => Out::Act(format!("steam-page:{}", game::STEAM_PAGES[(t - game::T_STEAM_PAGE) as usize].0)),
            t if t >= game::T_EMU => match self.d.emu.get((t - game::T_EMU) as usize) {
                Some(a) if a.ready => Out::Act(a.act_name()),
                _ => Out::None,
            },
            _ => Out::None,
        }
    }
}

impl Screen for RareMenu {
    fn touch_outside(&self, cx: &mut Cx, x: f32, _y: f32, now: f64) -> bool {
        // Left of the aside: the dimmed launcher or game.
        let Some(aside) = cx.scene.parent(self.strip).and_then(|h| cx.scene.parent(h)) else { return false };
        x < cx.scene.window_rect(aside, now)[0]
    }
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        // A tab in the strip: open it (LT / RT's job); nothing more to press.
        if self.detail.is_none() {
            let ids: Vec<Id> = self.tab_cells.iter().map(|c| c.0).collect();
            if let Some(t) = cx.scene.hit_among(x, y, now, &ids).and_then(|id| self.tab_cells.iter().find(|c| c.0 == id).map(|c| c.1)) {
                if t != self.tab {
                    self.switch_to(cx, t, now);
                }
                return false;
            }
        }
        self.nav.focus_at(cx.scene, x, y, now)
    }
    fn focus_id(&self) -> Option<Id> {
        self.nav.current().map(|f| f.id)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        match pad {
            Pad::B => {
                if self.detail.is_some() {
                    self.detail = None;
                    self.rebuild(cx, now);
                    Out::Moved
                } else {
                    Out::Close
                }
            }
            Pad::Lt | Pad::Rt => {
                if self.detail.is_some() {
                    return Out::None;
                }
                let n = self.tabs.len();
                let i = self.tabs.iter().position(|t| *t == self.tab).unwrap_or(0);
                let j = (i + if pad == Pad::Rt { 1 } else { n - 1 }) % n;
                let t = self.tabs[j];
                self.switch_to(cx, t, now);
                Out::Moved
            }
            Pad::X if self.tab == MenuTab::Ask && self.detail.is_none() => self.ask_act(cx, "ask-talk", now),
            Pad::Y if self.tab == MenuTab::Ask && self.detail.is_none() => Out::Act("ask:type".into()),
            // The first tab's shortcuts, shown in the footer: X opens Steam, Y searches Settings.
            Pad::X if matches!(self.tab, MenuTab::Game | MenuTab::Rare) && self.detail.is_none() && self.armed.is_none() => Out::Act("rare:steam".into()),
            Pad::Y if matches!(self.tab, MenuTab::Game | MenuTab::Rare) && self.detail.is_none() && self.armed.is_none() => Out::Act("rare:search-settings".into()),
            Pad::A => {
                let Some(f) = self.nav.current() else { return Out::None };
                let (tag, sid) = (f.tag, f.sid.clone());
                self.act(cx, tag, sid, now)
            }
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                if self.nav.step(cx.scene, pad, now) {
                    Out::Moved
                } else if matches!(pad, Pad::Up | Pad::Down) && matches!(self.tab, MenuTab::Game | MenuTab::Rare) && self.detail.is_none() {
                    // The first tab's list wraps: Up on the top row lands on the last (Quit game,
                    // Power), Down on the last back on the top.
                    let pick = if pad == Pad::Up { self.nav.items.last() } else { self.nav.items.iter().find(|f| media::cmd_of(&f.sid).is_none()) };
                    match pick.map(|f| f.id) {
                        Some(id) if Some(id) != self.nav.cur => {
                            self.nav.focus(cx.scene, id, now, true);
                            Out::Moved
                        }
                        _ => Out::None,
                    }
                } else {
                    Out::None
                }
            }
            // Nothing underneath may act (search, Steam, the launcher's tabs).
            _ => Out::None,
        }
    }

    fn tick(&mut self, cx: &mut Cx, now: f64) {
        if let Some(rx) = &self.load {
            match rx.try_recv() {
                Ok(l) => {
                    self.load = None;
                    self.poll = None;
                    self.loaded(cx, l, now);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => self.poll = Some(now + 0.2),
                Err(_) => {
                    self.load = None;
                    self.poll = None;
                    self.loaded(cx, live::Loaded { items: vec![], market: vec![] }, now);
                }
            }
        }
        self.deals(cx, now);
        // A Steam game's achievements landed after the menu opened: the Trophies tab fills in.
        if !self.live && panels::pending() && panels::poll_pending() {
            self.d.trophies_count = panels::data().rare.as_ref().map(|r| r.sum.unlocked as u32).unwrap_or(0);
            if self.tab == MenuTab::Trophies && self.detail.is_none() {
                self.switch_to(cx, MenuTab::Trophies, now);
            }
        }
        self.poll = (self.load.is_some() || self.look.is_some() || self.deal.is_some() || (!self.live && panels::pending())).then_some(now + 0.2);
        if self.armed.is_some_and(|t| now >= t) {
            self.disarm(cx);
        }
        self.media_tick(cx, now);
        self.ask_tick(cx, now);
    }

    fn answer(&mut self, cx: &mut Cx, key: &str, value: Option<String>, now: f64) -> Out {
        if key == "ask:type" {
            if let Some(t) = value.filter(|t| !t.trim().is_empty()) {
                crate::features::agent::ask(&t, self.shoot && crate::features::agent::can_shoot());
                self.ask_refresh(cx, now);
            }
            return Out::None;
        }
        if key == "items:price-other" {
            if let Some(p) = value.as_deref().and_then(panels::money::parse_sui_input) {
                if self.detail.is_some() {
                    self.set_view(|v, _| v.price = Some(p));
                    self.rebuild_keep(cx, "price:other", now);
                }
            }
        }
        Out::None
    }

    fn next_deadline(&self) -> Option<f64> {
        let sign = (self.deal.is_some() && matches!(panels::data().phase, panels::Phase::Building(_))).then_some(self.dealt_at + 1.5);
        [self.armed, self.poll, self.reload_at, sign, self.media_at, self.ask_at].into_iter().flatten().reduce(f64::min)
    }
}

// ---- Fixtures ----

fn act_art() -> Src {
    Src::Static("fixtures/act-hero2.jpg", include_bytes!("../../../../../render/assets/games/act/hero.jpg"))
}
fn word_art() -> Src {
    Src::Static("fixtures/act-hero.jpg", include_bytes!("../../../../../render/assets/games/act/hero.jpg"))
}
fn mark() -> Src {
    Src::Static("fixtures/mark.png", include_bytes!("../../../../../render/assets/brand/mark.png"))
}

const ADDR: &str = "0x7ce77a1f5e9b41c8d3a0f2e6b9c4d1a8e35f0b72c6d91a4e8f3b05d2c761cb";
const NOW_MS: f64 = 1_800_000_000_000.0;

fn base(rid: &str, title: &str) -> MenuData {
    MenuData {
        session: true,
        over_game: true,
        rid: Some(rid.into()),
        frame: Some(Src::Static("fixtures/act-hero.jpg", FRAME)),
        guest: false,
        game: GameInfo { title: title.into(), banner: Some(act_art()), logo: Some(mark()) },
        is_app: false,
        in_library: true,
        address: Some(ADDR.into()),
        name: None,
        signed_out: false,
        collections: false,
        trophies: false,
        items: None,
        unread: 0,
        trophies_count: 0,
        loading: false,
        perf: Some(Perf { profile: "balanced".into(), tdp: Some(15.0) }),
        power_saved: false,
        started_ms: Some(NOW_MS - 42.0 * 60_000.0),
        now_ms: NOW_MS,
        emu: vec![],
        running: true,
        taking: None,
        invite: false,
        tab: None,
        colls: vec![],
        replay: Some(30),
        discord: None,
        resume: None,
    }
}

fn sui_items() -> Vec<OverlayItem> {
    let m = 1_000_000_000u128;
    let it = |n: &str, f: u128, o: u128| OverlayItem { name: n.into(), floor_mist: Some(f * m / 10), top_offer_mist: (o > 0).then_some(o * m / 10) };
    vec![it("Sword of Dawn #12", 42, 30), it("Sword of Dawn #77", 42, 0), it("Ember Shield #3", 130, 90), it("Ember Shield #9", 130, 0)]
}

pub fn fixture(name: &str, cx: &mut Cx, now: f64) -> Option<Box<dyn Screen>> {
    let d = fixture_data_opt(name)?;
    let mut m = RareMenu::build(cx, d, now);
    if name == "menu-game-quit-armed" {
        m.arm(cx, now);
        // The golden holds the armed state past the harness script.
        m.armed = Some(now + 3600.0);
    }
    let _ = fixtures::session;
    Some(Box::new(m))
}

#[cfg(test)]
fn fixture_data(name: &str) -> MenuData {
    fixture_data_opt(name).unwrap()
}

/// The menu's data for a fixture name.
fn fixture_data_opt(name: &str) -> Option<MenuData> {
    Some(match name {
        "menu-game" => {
            let mut d = base("steam:620", "Portal 2");
            d.trophies = true;
            d.trophies_count = 7;
            d
        }
        "menu-game-sui" => {
            let mut d = base("web:wordgame", "Word Game");
            d.game.banner = Some(word_art());
            d.collections = true;
            d.trophies = true;
            d.items = Some(sui_items());
            d.unread = 3;
            d.power_saved = true;
            d
        }
        "menu-game-app" => {
            let mut d = base("app:telegram", "Telegram");
            d.game.banner = None;
            d.is_app = true;
            d.started_ms = None;
            d
        }
        "menu-game-steamui" => {
            let mut d = base("app:steam", "Steam");
            d.game.banner = None;
            d.is_app = true;
            d.perf = None;
            d
        }
        n if super::panels::fixture_tab(n).is_some() => {
            let mut d = base("web:wordgame", "Word Game");
            d.game.banner = Some(word_art());
            super::panels::fixture_menu(n, &mut d);
            d
        }
        "menu-game-quit-armed" | "menu-game-quit" => base("steam:620", "Portal 2"),
        "menu-game-stream" => {
            let mut d = base("stream:desk:1", "Desktop PC");
            d.in_library = false;
            d
        }
        "menu-game-discord" => {
            let mut d = base("steam:1145350", "Hades II");
            d.discord = Some((true, false));
            d.replay = Some(120);
            d
        }
        "menu-game-emulator" => {
            let mut d = base("emu:dolphin:mario", "Super Mario Sunshine");
            d.emu = vec![
                EmuAction { id: "save".into(), label: "Save state".into(), ready: true, note: None, thumb: None, sub: None },
                EmuAction { id: "load".into(), label: "Load state".into(), ready: true, note: None, thumb: None, sub: None },
                EmuAction { id: "menu".into(), label: "Emulator menu".into(), ready: false, note: Some("Bind a hotkey for the menu in the emulator's settings.".into()), thumb: None, sub: None },
            ];
            d
        }
        // A ROM in RetroArch: state slots with their pictures, fast forward, rewind, discs.
        "menu-game-retroarch" | "menu-game-retroarch-empty" => {
            let mut d = base("emu:psx/Big Adventure (USA) (Disc 1).chd", "Big Adventure");
            d.game.banner = None;
            let row = |id: &str, label: &str| EmuAction { id: id.into(), label: label.into(), ready: true, note: None, thumb: None, sub: None };
            d.emu = vec![row("save:3", "Save state")];
            if name == "menu-game-retroarch" {
                d.emu.push(EmuAction { thumb: Some(act_art()), sub: Some("2 min ago".into()), ..row("load:1", "Slot 1") });
                d.emu.push(EmuAction { thumb: Some(word_art()), sub: Some("Yesterday".into()), ..row("load:2", "Slot 2") });
                d.emu.push(EmuAction { thumb: None, sub: Some("Oct 3".into()), ..row("load:4", "Slot 4") });
            }
            d.emu.extend([row("ff", "Fast forward"), row("rewind", "Rewind 5 seconds"), row("disc", "Next disc"), row("menu", "Emulator settings")]);
            d
        }
        n if super::tabs::fixture_tab(n).is_some() => {
            super::tabs::use_fixture(n);
            let mut d = base("", "");
            d.session = false;
            d.over_game = false;
            d.rid = None;
            d.frame = None;
            d.unread = 2;
            d.tab = Some(match super::tabs::fixture_tab(n) {
                Some("friends") => MenuTab::Friends,
                Some("news") => MenuTab::News,
                _ => MenuTab::Rare,
            });
            if n == "menu-rare" {
                d.unread = 0;
            }
            if n.contains("friends-invite") {
                d.invite = true;
            }
            d
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rare_render::{ImageLoader, Scene, TextSys};

    fn with_cx<R>(f: impl FnOnce(&mut Cx) -> R) -> R {
        let mut scene = Scene::new(1920.0, 1200.0, 18.0);
        let mut t = TextSys::new(&[include_bytes!("../../../../../render/assets/fonts/Inter-Regular.ttf"), include_bytes!("../../../../../render/assets/fonts/Inter-Medium.ttf"), include_bytes!("../../../../../render/assets/fonts/Inter-SemiBold.ttf")], false, 256);
        let mut img = ImageLoader::new(1, std::sync::Arc::new(|| {}), None);
        let mut cx = Cx { scene: &mut scene, t: &mut t, img: &mut img };
        f(&mut cx)
    }

    #[test]
    fn the_tab_strip_shows_what_fits_around_the_current_tab() {
        let all = [("Game", None), ("Ask Rare", None), ("Items", None), ("Market", None), ("Trophies", Some(7)), ("Friends", None), ("Notifications", Some(3))];
        let (a, b) = tab_window(&all, 1, 24.0);
        assert!(a == 0 && b >= 5 && b < 7, "{a}..{b}");
        let (a, b) = tab_window(&all, 6, 24.0);
        assert_eq!(b, 7);
        assert!(a > 0);
        // Four tabs fit whole.
        assert_eq!(tab_window(&all[..4], 3, 24.0), (0, 4));
    }

    #[test]
    fn lt_rt_cycle_tabs_and_b_closes() {
        with_cx(|cx| {
            let mut m = fixture("menu-game-sui", cx, 0.0).unwrap();
            cx.scene.layout(cx.t);
            assert_eq!(m.pad(cx, Pad::Rt, 0.1), Out::Moved);
            assert_eq!(m.pad(cx, Pad::Lt, 0.2), Out::Moved);
            assert_eq!(m.pad(cx, Pad::B, 0.3), Out::Close);
        });
    }

    #[test]
    fn resume_is_focused_and_quit_needs_a_second_press() {
        with_cx(|cx| {
            let mut m = fixture("menu-game", cx, 0.0).unwrap();
            cx.scene.layout(cx.t);
            assert_eq!(m.pad(cx, Pad::A, 0.1), Out::Act("resume".into()));
            // Screenshot, Save clip, Performance, Steam, Open Steam, Game details, then Quit.
            for _ in 0..7 {
                m.pad(cx, Pad::Down, 0.2);
            }
            assert_eq!(m.pad(cx, Pad::A, 1.0), Out::Moved);
            assert_eq!(m.next_deadline(), Some(5.0));
            m.tick(cx, 6.0);
            assert_eq!(m.next_deadline(), None);
            assert_eq!(m.pad(cx, Pad::A, 7.0), Out::Moved);
            assert_eq!(m.pad(cx, Pad::A, 8.0), Out::Act("quit".into()));
        });
    }

    #[test]
    fn game_tab_wraps_from_quit_to_resume() {
        with_cx(|cx| {
            let mut m = fixture("menu-game", cx, 0.0).unwrap();
            cx.scene.layout(cx.t);
            for _ in 0..8 {
                m.pad(cx, Pad::Down, 0.2);
            }
            assert_eq!(m.pad(cx, Pad::A, 1.0), Out::Act("resume".into()));
        });
    }
}

/// Presses A on every row of every tab of the Rare menu fixtures and prints what each asked for:
/// `cargo test -p rare-launcher-native --lib menu::crawl -- --nocapture --ignored`.
#[cfg(test)]
mod crawl {
    use super::*;
    use rare_render::{ImageLoader, Scene, TextSys};
    use std::sync::Arc;

    static FONTS: [&[u8]; 1] = [include_bytes!("../../../../../render/assets/fonts/Inter-Regular.ttf")];

    fn texts(scene: &rare_render::Scene, id: Id, out: &mut Vec<String>) {
        if let Some(t) = scene.get(id).text.as_ref() {
            out.push(t.text.clone());
        }
        for c in scene.children(id) {
            texts(scene, c, out);
        }
    }

    #[test]
    #[ignore]
    fn crawl_every_row() {
        let mut text = TextSys::new(&FONTS, false, 512);
        let mut images = ImageLoader::new(1, Arc::new(|| {}), None);
        for name in ["menu-game", "menu-game-sui", "menu-game-app", "menu-game-steamui", "menu-game-stream", "menu-game-discord", "menu-game-emulator", "menu-game-retroarch", "menu-rare"] {
            let mut scene = Scene::new(1280.0, 800.0, 16.0);
            let mut cx = Cx { scene: &mut scene, t: &mut text, img: &mut images };
            let Some(_) = fixture(name, &mut cx, 0.0) else { println!("== {name}: no fixture"); continue };
            // The tabs, by switching with RT from a fresh menu.
            let ntabs = {
                let mut scene = Scene::new(1280.0, 800.0, 16.0);
                let mut cx = Cx { scene: &mut scene, t: &mut text, img: &mut images };
                let mut m = fixture(name, &mut cx, 0.0).unwrap();
                let mut seen = vec![];
                for _ in 0..12 {
                    let mut w = vec![];
                    let root = cx.scene.root;
                    texts(cx.scene, root, &mut w);
                    let sig = w.join("|");
                    if seen.contains(&sig) {
                        break;
                    }
                    seen.push(sig);
                    m.pad(&mut cx, Pad::Rt, 1.0);
                }
                seen.len()
            };
            println!("== {name} ({ntabs} tab states)");
            for t in 0..ntabs {
                let count = {
                    let mut scene = Scene::new(1280.0, 800.0, 16.0);
                    let mut cx = Cx { scene: &mut scene, t: &mut text, img: &mut images };
                    let mut m = RareMenu::build(&mut cx, fixture_data(name), 0.0);
                    for _ in 0..t {
                        m.pad(&mut cx, Pad::Rt, 1.0);
                    }
                    println!("  -- tab {:?}", m.tab);
                    m.nav.len()
                };
                for i in 0..count {
                    let mut scene = Scene::new(1280.0, 800.0, 16.0);
                    let mut cx = Cx { scene: &mut scene, t: &mut text, img: &mut images };
                    let mut m = RareMenu::build(&mut cx, fixture_data(name), 0.0);
                    for _ in 0..t {
                        m.pad(&mut cx, Pad::Rt, 1.0);
                    }
                    let Some(f) = m.nav.items.get(i).cloned() else { continue };
                    m.nav.focus(cx.scene, f.id, 2.0, false);
                    let mut w = vec![];
                    texts(cx.scene, f.id, &mut w);
                    let out = m.pad(&mut cx, Pad::A, 3.0);
                    let label: String = w.join(" | ").chars().take(80).collect();
                    println!("    [{i}] sid={} tag={} -> {:?} detail={:?} :: {}", f.sid, f.tag, out, m.detail, label);
                }
            }
        }
    }
}
