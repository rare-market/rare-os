//! Search (components/Search.tsx): installed games from every store, Sui games, Rare Marketplace
//! collections, in one page. Y opens it with the keyboard; the field reopens the keyboard.

use crate::act::Act;
use crate::data::model::source_label;
use crate::kit::{self, s, tok, Ty, Ui, T};
use crate::page::{hash, Acts, Page};
use crate::screens::game::parts::{self, Variant};
use crate::screens::home::game_logo;
use crate::screens::trade::cards::token_list_row;
use crate::screens::trade::model::TokenRow;
use crate::state::{OskFor, OskReq, State};
use rare_market::api::SearchResult;
use rare_render::focus::Group;
use rare_render::scene::{GradDir, Gradient};
use rare_render::view::{key, El, Look, Motion};

/// A Rare Marketplace collection that matches the search.
#[derive(Clone, Debug, PartialEq)]
pub struct ColHit {
    pub slug: String,
    pub name: String,
    pub image: Option<String>,
}

/// What the network answered for a search term: collections (first 8) and coins (first 12).
#[derive(Clone, Debug, Default)]
pub struct Hits {
    pub q: String,
    pub cols: Vec<ColHit>,
    pub coins: Option<Vec<TokenRow>>,
}

/// The search page: it asks the network for collections and coins once the term has settled.
#[derive(Default)]
pub struct Search {
    asked: String,
    changed: f64,
}

fn group(ui: &mut Ui, title: &str, empty: &str, items: Vec<El>) -> El {
    let note = (!items.is_empty()).then(|| items.len().to_string());
    let head = kit::section_head(ui, s().mb(0.8), title, note.as_deref(), None);
    let body = if items.is_empty() {
        ui.text(empty, T::new(Ty::Small).c(tok::MUTED))
    } else {
        ui.bx(s().row().gap(0.9).py(1.4).my(-1.4)).scrolls_x(key(&format!("search-{title}"))).group(Group::new(&format!("search-{title}"), [0.0; 4]).rail()).kids(items)
    };
    ui.bx(s().col()).section().child(head).child(body).key(key(title))
}

impl Page for Search {
    fn stamp(&self, st: &State) -> u64 {
        hash((st.ver.search, &st.search, st.ver.library, st.ver.catalog, &st.search_hits.q, st.search_hits.cols.len(), st.search_hits.coins.as_ref().map(|c| c.len())))
    }

    fn view(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El {
        let q = st.search.clone();
        let edit = acts.tag(Act::Local(0));
        let mut field = ui.bx(s().row().items_center().gap(0.8).h(3.25).wp(1.0).px(1.1).mb(1.75)).bg(tok::L1).radius(ui.r(0.5)).border(1.0, tok::BORDER_LIGHT).child(kit::icon(ui, "search", 1.3, tok::ICON2));
        field = if q.is_empty() { field.child(ui.text("Search games, Sui games, collections and coins", T::new(Ty::Row).c(tok::MUTED))) } else { field.child(ui.text(q.clone(), T::new(Ty::Row))) };
        let field = kit::row_look(kit::nav(field, edit, Some("Edit search"), None, false));
        let mut page = ui.bx(s().col().fill().px(2.5).py(2.0)).bg(tok::BG).enter(Motion::FADE).scrolls_y(key("search")).child(field);
        if !q.is_empty() {
            let needle = q.to_lowercase();
            // One per game: copies of one game (data/groups.rs) show once.
            let games: Vec<El> = st
                .one_each(st.games().iter().chain(st.owned().iter()).chain(st.cloud().iter()).filter(|g| g.title.to_lowercase().contains(&needle)))
                .into_iter()
                .take(12)
                .map(|g| {
                    let art = parts::art(ui, &g.art.cover, &g.title, Variant::Cover, 8.0, 12.0, 0.5);
                    let shade = ui
                        .bx(s().abs(None, Some(0.0), Some(0.0), Some(0.0)).px(0.5).pb(0.4).pt(1.2))
                        .look(Look::default().grad(Gradient { dir: GradDir::ToTop, stops: [tok::BLACK.alpha(0.85), tok::BLACK.alpha(0.42), tok::BLACK.alpha(0.0)], mid: 0.5 }))
                        .child(ui.text(if g.installed { source_label(&g.source) } else { "Not installed" }, T::new(Ty::Label).c(tok::WHITE.alpha(0.85))));
                    let tag = acts.game(Act::OpenGame(g.id.clone()), &g.id);
                    let b = ui.bx(s().wh(8.0, 12.0)).bg(tok::L3).radius(ui.r(0.5)).clip().child(art).child(shade);
                    kit::nav(kit::tile(b, 1.06), tag, None, None, false).key(key(&g.id))
                })
                .collect();
            let sui: Vec<El> = st
                .sui_games
                .iter()
                .filter(|g| format!("{} {} {}", g.name, g.studio, g.genre).to_lowercase().contains(&needle))
                .take(8)
                .map(|g| {
                    let tag = acts.tag(Act::SuiGame(g.slug.clone()));
                    let logo = game_logo(ui, &g.logo_url, &g.name, 2.75);
                    let t = ui
                        .bx(s().row().items_center().gap(0.75).py(0.6).pl(0.6).pr(1.1))
                        .bg(tok::L1)
                        .radius(ui.r(0.5))
                        .border(1.0, tok::HAIR)
                        .child(logo)
                        .child(ui.bx(s().col()).child(ui.text(g.name.clone(), T::new(Ty::Body).w(600))).child(ui.text([g.studio.as_str(), g.genre.as_str()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" \u{b7} "), T::new(Ty::Meta).c(tok::MUTED))));
                    kit::nav(kit::tile(t, 1.06), tag, None, None, false).key(key(&g.slug))
                })
                .collect();
            let hits = (st.search_hits.q == q.trim()).then_some(&st.search_hits);
            let cols: Vec<El> = hits
                .map(|h| h.cols.as_slice())
                .unwrap_or(&[])
                .iter()
                .map(|c| {
                    let tag = acts.tag(Act::Market(format!("collection:{}", c.slug)));
                    let av = crate::screens::market::parts::avatar(ui, s(), c.image.as_deref(), &c.name, 2.75, false);
                    let t = ui.bx(s().row().items_center().gap(0.75).py(0.6).pl(0.6).pr(1.1)).bg(tok::L1).radius(ui.r(0.5)).border(1.0, tok::HAIR).child(av).child(ui.text(c.name.clone(), T::new(Ty::Body).w(600)));
                    kit::nav(kit::tile(t, 1.06), tag, None, None, false).key(key(&c.slug))
                })
                .collect();
            let mut col = ui.bx(s().col().gap(1.6)).child(group(ui, "Your games", "No game matches", games)).child(group(ui, "Sui Games", "No Sui game matches", sui)).child(group(ui, "Rare Marketplace", if hits.is_some() { "No collection matches" } else { "Searching" }, cols));
            // Coins (trade/CoinSearch.tsx CoinResults): rows of the swap picker's kind.
            let coins: Vec<TokenRow> = hits.and_then(|h| h.coins.clone()).unwrap_or_default();
            let note = (!coins.is_empty()).then(|| coins.len().min(12).to_string());
            let mut sec = ui.bx(s().col()).section().child(kit::section_head(ui, s().mb(0.5), "Coins", note.as_deref(), None));
            if coins.is_empty() {
                let msg = if hits.is_some_and(|h| h.coins.is_some()) { "No coin matches. Paste a full coin type to open one that is not listed." } else { "Searching" };
                sec = sec.child(ui.text_s(s().py(1.0), msg, T::new(Ty::Small).c(tok::MUTED)));
            } else {
                let mut rows = ui.bx(s().col()).group(Group::new("search-coins", [0.0; 4]));
                for t in coins.iter().take(12) {
                    let tag = acts.tag(Act::Market(format!("token:{}", t.coin_type)));
                    rows = rows.child(token_list_row(ui, t, None, tag, false).key(key(&t.coin_type)));
                }
                sec = sec.child(rows);
            }
            col = col.child(sec.key(key("Coins")));
            page = page.child(col);
        }
        page
    }

    fn tick(&mut self, st: &mut State, fx: &mut crate::page::Fx, now: f64) -> Option<f64> {
        let q = st.search.trim().to_string();
        if q == self.asked {
            return None;
        }
        if self.changed == 0.0 || now < self.changed {
            self.changed = now;
        }
        // The term has to hold still for a moment (the keyboard types it live).
        if now - self.changed < 0.35 {
            return Some(self.changed + 0.36);
        }
        self.asked = q.clone();
        self.changed = 0.0;
        if q.is_empty() {
            return None;
        }
        fx.spawn(move |_| {
            let net = crate::screens::market::feed::net();
            let cols: Vec<ColHit> = rare_market::api::get::<SearchResult>(&*net, &rare_market::api::paths::search(&q))
                .map(|r| r.collections.into_iter().take(8).map(|c| ColHit { slug: c.r#ref.slug, name: c.r#ref.name, image: c.r#ref.image_url }).collect())
                .unwrap_or_default();
            let coins = rare_trade::search::search_coins(&*net, &crate::screens::trade::live::SuiReader, &q);
            let coins: Vec<TokenRow> = coins.into_iter().filter_map(|r| serde_json::to_value(r).ok().and_then(|v| serde_json::from_value(v).ok())).collect();
            Box::new(move |st: &mut State, _| {
                if st.search.trim() == q {
                    st.search_hits = Hits { q, cols, coins: Some(coins) };
                    st.ver.search += 1;
                }
            })
        });
        None
    }

    fn act(&mut self, st: &mut State, _fx: &mut crate::page::Fx, code: u32) {
        if code == 0 {
            st.osk = Some(OskReq::new("Search", st.search.clone(), None, OskFor::Search));
        }
    }
}
