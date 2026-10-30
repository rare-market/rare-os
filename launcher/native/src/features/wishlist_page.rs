//! The Wishlist page (overlay `wishlist`): games added in Rare and the Steam account's wishlist,
//! each with the lowest price across stores right now (rare.market, from CheapShark) and the
//! lowest ever. A opens the best deal in the browser, X removes, Y adds a game (the keyboard, then
//! a search). Prices refresh every 6 hours; a drop of 5 % or more raises a notice.

use crate::act::Act;
use crate::data::knowledge::{self, Hit, Price};
use crate::data::wishlist::{self, money, Item};
use crate::kit::{self, s, tok, Btn as B, BtnIcon, Track, Ty, Ui, T};
use crate::page::{hash, Acts, Btn, Feedback, Fx, Page};
use crate::screens::game::parts::{self, Variant};
use crate::state::{State, Tone};
use rare_render::focus::hints::{hint, Hint, Key};
use rare_render::focus::{Focusable, Group};
use rare_render::view::{key, El, Motion};
use std::sync::{Arc, Mutex};

pub const KIND: &str = "wishlist";
const ASK: &str = "wishlist:search";

const ACT_ADD: u32 = 1;
const ACT_ROW: u32 = 1000;
const ACT_HIT: u32 = 5000;

#[derive(Default)]
struct Found {
    q: String,
    hits: Option<Result<Vec<Hit>, String>>,
}

#[derive(Default)]
pub struct WishlistPage {
    /// The search the keyboard asked for (None: the list).
    found: Option<Arc<Mutex<Found>>>,
    ver: u64,
    /// Rows by tag order this build (id), for X.
    ids: Vec<String>,
}

const COLS: [Track; 4] = [Track::Rem(8.0), Track::Fr(1.0), Track::Rem(11.0), Track::Rem(8.5)];

/// A wishlist game's art: Steam's header for Steam apps, else CheapShark's capsule.
fn art_srcs(steam: Option<u32>, thumb: Option<&str>) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(a) = steam {
        v.push(format!("https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/{a}/header.jpg"));
    }
    if let Some(t) = thumb {
        v.push(t.to_string());
    }
    v
}

impl WishlistPage {
    fn search(&mut self, fx: &mut Fx, q: String) {
        let f = Arc::new(Mutex::new(Found { q: q.clone(), hits: None }));
        self.found = Some(f.clone());
        self.ver += 1;
        fx.spawn(move |_| {
            let r = knowledge::search(&q);
            f.lock().unwrap().hits = Some(r);
            Box::new(|_, _| {})
        });
    }

    fn row(&self, ui: &mut Ui, st: &State, acts: &mut Acts, i: usize, it: &Item, p: Option<&Price>) -> El {
        let srcs = art_srcs(it.steam.or(p.and_then(|p| p.steam_app_id)), p.and_then(|p| p.thumb.as_deref()));
        let pic = parts::art(ui, &srcs, &it.title, Variant::Hero, 8.0, 3.75, 0.5);
        let art = ui.bx(s().wh(8.0, 3.75)).bg(tok::L3).radius(ui.r(0.35)).clip().child(pic);
        let owned = st.games().iter().any(|g| g.id == it.id || g.title.eq_ignore_ascii_case(&it.title));
        let mut sub: Vec<String> = Vec::new();
        if owned {
            sub.push("In your library".into());
        }
        sub.push(if it.steam_list && it.rare { "Steam and Rare".into() } else if it.steam_list { "From your Steam wishlist".into() } else { "Added in Rare".into() });
        if it.added > 0 {
            sub.push(format!("added {}", knowledge::day_label(it.added, false)));
        }
        let title = ui.bx(s().col().min0().gap(0.2)).child(ui.text(it.title.clone(), T::new(Ty::Body).w(600).trunc())).child(ui.text(sub.join(" \u{b7} "), T::new(Ty::Meta).c(tok::MUTED).trunc()));
        let (store, low) = match p {
            Some(p) if p.price.is_some() => (
                p.store.clone().map(|s| format!("at {s}")).unwrap_or_default(),
                p.lowest.as_ref().map(|l| format!("Lowest ever {}", money(l.price))).unwrap_or_default(),
            ),
            Some(_) => ("Not in any store yet".to_string(), String::new()),
            None => ("Checking prices".to_string(), String::new()),
        };
        let where_ = ui.bx(s().col().min0().gap(0.2)).child(ui.text(store, T::new(Ty::Small).c(tok::fg(0.8)).trunc())).child(ui.text(low, T::new(Ty::Meta).c(tok::MUTED).trunc()));
        let mut price = ui.bx(s().col().items_end().min0().gap(0.2));
        match p.and_then(|p| p.price.map(|v| (v, p))) {
            Some((v, p)) => {
                price = price.child(ui.text(money(v), T::new(Ty::Row).w(700).tab()));
                let line = if p.cut > 0 { format!("-{}% \u{b7} was {}", p.cut, money(p.retail.unwrap_or(0.0))) } else { String::new() };
                price = price.child(ui.text(line, T::new(Ty::Meta).c(tok::MUTED).tab().trunc()));
            }
            None => price = price.child(ui.text("--", T::new(Ty::Row).c(tok::MUTED))),
        }
        let el = ui.bx(s().grid(&COLS).items_center().gap_xy(1.0, 0.0).h(5.0).px(0.75).wp(1.0)).radius(ui.r(0.5)).child(art).child(title).child(where_).child(kit::cell(ui, true).child(price));
        let has_deal = p.and_then(|p| p.url.as_ref()).is_some();
        let tag = acts.tag(Act::Local(ACT_ROW + i as u32));
        kit::nav(kit::row_look(el), tag, Some(if has_deal { "Open deal" } else { "Details" }), Some("Remove"), i == 0).key(key(&it.id))
    }
}

impl Page for WishlistPage {
    fn stamp(&self, st: &State) -> u64 {
        let found = self.found.as_ref().map(|f| {
            let f = f.lock().unwrap();
            (f.q.clone(), f.hits.as_ref().map(|h| h.as_ref().map(|v| v.len()).unwrap_or(usize::MAX)))
        });
        hash((self.ver, wishlist::ver(), found, st.ver.library))
    }

    fn tick(&mut self, st: &mut State, fx: &mut Fx, now: f64) -> Option<f64> {
        if let Some(a) = st.take_answer(ASK) {
            match a.map(|q| q.trim().to_string()).filter(|q| q.len() >= 2) {
                Some(q) => self.search(fx, q),
                None => {}
            }
        }
        // Waiting on a search.
        self.found.as_ref().filter(|f| f.lock().unwrap().hits.is_none()).map(|_| now + 0.2)
    }

    fn view(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El {
        let (shown, prices, sale, steam_ok, prices_at): (Vec<Item>, Vec<Option<Price>>, usize, bool, i64) = wishlist::with(|w| {
            let v: Vec<Item> = w.shown().into_iter().cloned().collect();
            let p = v.iter().map(|i| w.prices.get(&i.id).cloned()).collect();
            (v, p, w.on_sale(), w.steam_ok, w.prices_at)
        });
        let note = if shown.is_empty() { String::new() } else if sale > 0 { format!("{} games, {sale} on sale", shown.len()) } else { format!("{} {}", shown.len(), if shown.len() == 1 { "game" } else { "games" }) };
        let head = ui
            .bx(s().row().items_center().gap(0.75).h(2.5).mb(1.0))
            .child(kit::icon(ui, "heart", 1.5, tok::ICON2))
            .child(ui.text("Wishlist", T::new(Ty::Title).w(700)))
            .child(ui.text_s(s().ml_auto(), note, T::new(Ty::Small).c(tok::MUTED).tab()));
        let mut page = ui.bx(s().col().fill().px(2.5).pt(2.0).pb(5.0)).bg(tok::BG).enter(Motion::FADE).scrolls_y(key("wishlist-page")).child(head);
        // A search the keyboard asked for.
        if let Some(f) = self.found.clone() {
            let f = f.lock().unwrap();
            page = page.child(ui.text_s(s().mb(0.75), format!("Results for \u{201c}{}\u{201d}", f.q), T::new(Ty::Row).w(600).trunc()));
            match &f.hits {
                None => page = page.child(ui.text("Searching the stores", T::new(Ty::Small).c(tok::MUTED))),
                Some(Err(_)) => {
                    let again = kit::button(ui, B::new("Search again", acts.tag(Act::Local(ACT_ADD))).primary().def(true));
                    page = page.child(kit::empty(ui, s(), "Couldn't search right now", Some("Check the connection and try again."), vec![again]));
                }
                Some(Ok(hits)) if hits.is_empty() => {
                    let again = kit::button(ui, B::new("Search again", acts.tag(Act::Local(ACT_ADD))).primary().def(true));
                    page = page.child(kit::empty(ui, s(), "No games found", Some("Try a shorter name, like the first word of the title."), vec![again]));
                }
                Some(Ok(hits)) => {
                    let rows: Vec<El> = hits
                        .iter()
                        .take(20)
                        .enumerate()
                        .map(|(i, h)| {
                            let srcs = art_srcs(h.steam_app_id, h.thumb.as_deref());
                            let pic = parts::art(ui, &srcs, &h.title, Variant::Hero, 8.0, 3.75, 0.5);
                            let art = ui.bx(s().wh(8.0, 3.75)).bg(tok::L3).radius(ui.r(0.35)).clip().child(pic);
                            let on = wishlist::with(|w| w.has(&h.id));
                            let el = ui
                                .bx(s().row().items_center().gap(1.0).h(5.0).px(0.75).wp(1.0))
                                .radius(ui.r(0.5))
                                .child(art)
                                .child(ui.bx(s().col().grow().gap(0.2)).child(ui.text(h.title.clone(), T::new(Ty::Body).w(600).trunc())).child(ui.text(if on { "On your wishlist" } else { "" }, T::new(Ty::Meta).c(tok::MUTED))))
                                .child(ui.text_s(s().w(7.0), h.price.map(money).unwrap_or_default(), T::new(Ty::Row).w(700).tab().right()));
                            let tag = acts.tag(Act::Local(ACT_HIT + i as u32));
                            kit::nav(kit::row_look(el), tag, Some(if on { "On your wishlist" } else { "Add" }), None, i == 0).key(key(&h.id))
                        })
                        .collect();
                    page = page.child(ui.bx(s().col().gap(0.25)).group(Group::new("wishlist-hits", [0.0; 4])).kids(rows));
                }
            }
            return page;
        }
        let add = kit::button(ui, B::new("Add a game", acts.tag(Act::Local(ACT_ADD))).icon(BtnIcon::Lucide("plus", 1.0, false)).hint("Add a game").def(shown.is_empty()));
        if shown.is_empty() {
            self.ids.clear();
            let body = if steam_ok { "Add games you want to keep an eye on. Their lowest price across stores shows here, and Rare tells you when one drops." } else { "Add games you want to keep an eye on, or sign in to Steam on this device: your Steam wishlist shows here when your profile's game details are public." };
            return page.child(kit::empty(ui, s(), "Your wishlist is empty", Some(body), vec![add]));
        }
        page = page.child(ui.bx(s().row().items_center().gap(1.0).mb(1.0)).child(add).child(ui.text_s(s().ml_auto().shrink().min0(), format!("Lowest price now across stores, in USD{}", if prices_at > 0 { format!(", checked {}", crate::data::model::fmt_ago(Some(prices_at)).trim_start_matches("Played ").to_lowercase()) } else { String::new() }), T::new(Ty::Small).c(tok::MUTED).trunc())));
        self.ids = shown.iter().map(|i| i.id.clone()).collect();
        let rows: Vec<El> = shown.iter().zip(prices.iter()).enumerate().map(|(i, (it, p))| self.row(ui, st, acts, i, it, p.as_ref())).collect();
        page = page.child(ui.bx(s().col().gap(0.25)).group(Group::new("wishlist-rows", [0.0; 4]).remember()).kids(rows));
        if !steam_ok {
            page = page.child(ui.text_s(s().mt(1.25).max_w(52.0), "Your Steam wishlist shows here too when this device is signed in to Steam and your profile's game details are public.", T::new(Ty::Small).c(tok::MUTED).wrap()));
        }
        page
    }

    fn act(&mut self, st: &mut State, fx: &mut Fx, code: u32) {
        let now = rare_render::clock::secs();
        if code == ACT_ADD {
            st.ask_text(ASK, "Add a game to your wishlist", "", Some("Game name"));
            return;
        }
        if let Some(i) = code.checked_sub(ACT_HIT) {
            let Some(h) = self.found.as_ref().and_then(|f| f.lock().unwrap().hits.as_ref().and_then(|r| r.as_ref().ok()).and_then(|v| v.get(i as usize).cloned())) else { return };
            if wishlist::with(|w| w.has(&h.id)) {
                return;
            }
            wishlist::add(&h.id, &h.title, h.steam_app_id);
            fx.fb(Feedback::Toggle);
            st.toast(format!("Added {} to your wishlist", h.title), Tone::Info, now);
            self.found = None;
            self.ver += 1;
            return;
        }
        if let Some(i) = code.checked_sub(ACT_ROW) {
            let Some(id) = self.ids.get(i as usize).cloned() else { return };
            let url = wishlist::with(|w| w.prices.get(&id).and_then(|p| p.url.clone()));
            let steam = id.strip_prefix("steam:").and_then(|a| a.parse::<u32>().ok());
            match (url, steam) {
                (Some(u), _) => {
                    st.toast("Opening the deal", Tone::Info, now);
                    fx.run(Act::OpenWeb(u));
                }
                (None, Some(a)) => fx.run(Act::SteamStore(a)),
                _ => st.toast("No store sells this one yet", Tone::Info, now),
            }
        }
    }

    fn pad(&mut self, st: &mut State, fx: &mut Fx, b: Btn, cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        let now = rare_render::clock::secs();
        match b {
            Btn::B if self.found.is_some() => {
                self.found = None;
                self.ver += 1;
                fx.fb(Feedback::Back);
                true
            }
            Btn::Y if self.found.is_none() => {
                self.act(st, fx, ACT_ADD);
                true
            }
            Btn::X if self.found.is_none() => {
                let Some((_, Some(Act::Local(c)))) = cur else { return false };
                let Some(id) = c.checked_sub(ACT_ROW).and_then(|i| self.ids.get(i as usize).cloned()) else { return false };
                let title = wishlist::with(|w| w.items.iter().find(|i| i.id == id).map(|i| i.title.clone())).unwrap_or_default();
                wishlist::remove(&id);
                fx.fb(Feedback::Toggle);
                st.toast(format!("Removed {title}"), Tone::Info, now);
                true
            }
            _ => false,
        }
    }

    fn hints(&self, _st: &State) -> Vec<Hint> {
        if self.found.is_some() {
            return vec![hint(Key::B, "Back to wishlist")];
        }
        vec![hint(Key::Y, "Add a game"), hint(Key::B, "Back")]
    }
}
