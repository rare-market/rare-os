//! The Marketplace front page (marketplace/HomePage.tsx) in rare.market's home layout: the Market
//! sections and the search field, Trending (mover cards), Your NFTs (item cards) and the top
//! collections table with its time range. X steps the range, LT/RT the Market sections.

use super::data::Answers;
use super::host::*;
use super::parts::*;
use super::shell::{Act, MkPage};
use rare_market::api::{paths, CollectionRow, ItemCard};
use rare_market::format::{fmt_int, short_addr};
use rare_market::nav::{step_section, Area, Section, View};
use rare_render::input::Pad;
use rare_render::view::{key, El};
use std::rc::Rc;

pub const WINDOWS: [&str; 5] = ["1h", "6h", "24h", "7d", "30d"];

pub const T_SEARCH: u32 = 1;
pub const T_TREND: u32 = 1000;
pub const T_MINE: u32 = 2000;
pub const T_MINE_ALL: u32 = 2900;
pub const T_WIN: u32 = 3000;
pub const T_ROW: u32 = 4000;

/// LT / RT from the Collections front page: Trade's section to show ("home" is Coins, "perps",
/// "traders"), as the `area:trade:<section>` act the Market tab switches on.
pub fn section_step(dir: i32) -> String {
    let go = step_section(Area::Collections, "home", dir);
    format!("area:trade:{}", go.trade_section.unwrap_or("home"))
}

pub struct HomePage {
    pub a: Rc<Answers>,
    /// Index into WINDOWS (24h).
    pub win: usize,
    pub frame: u64,
    pub host: Host,
}

impl HomePage {
    pub fn new(cx: &mut Cx, parent: rare_render::Id, a: Rc<Answers>, frame: u64, now: f64) -> HomePage {
        let mut p = HomePage { a, win: 2, frame, host: Host::new(cx, parent) };
        p.render(cx, now);
        p
    }

    fn trending(&self) -> Vec<CollectionRow> {
        self.a.page::<CollectionRow>(&paths::collections("24h", "volume", 12)).map(|p| p.data).unwrap_or_default()
    }

    fn top(&self) -> Vec<CollectionRow> {
        self.a.rows::<CollectionRow>(&paths::collections(WINDOWS[self.win], "volume", 25)).items
    }
}

/// The page's tree.
pub fn view(ui: &mut Ui, a: &Answers, win: usize, frame: u64) -> El {
    let page = ui.bx(s().fill().col().pt(1.0).pb(2.5)).scrolls_y(key(&format!("mk-home-{frame}")));

    // ---- sections, search, SUI price
    let sections = market_sections(ui, Section::Collections);
    let search = search_field(ui, s().ml_auto().w(24.0), 2.75, "Search collections", true, SM, 1.0, T_SEARCH);
    let mut head = ui.bx(s().row().items_center().gap(1.0).px(2.5).pb(1.5).pt(0.5)).child(sections).child(search);
    if let Some(u) = a.sui_usd() {
        head = head.child(ui.text(format!("SUI ${u:.3}"), t(SM).c(tok::MUTED).tab()));
    }

    // ---- trending
    let trend_path = paths::collections("24h", "volume", 12);
    let rows: Vec<CollectionRow> = a.page::<CollectionRow>(&trend_path).map(|p| p.data).unwrap_or_default();
    let mut trending = ui.bx(s().col()).child(ui.bx(s().px(2.5)).child(ui.text("Trending", t(XL).w(700))));
    if !rows.is_empty() {
        let cards: Vec<El> = rows.iter().enumerate().map(|(i, c)| mover_card(ui, c, T_TREND + i as u32, i == 0)).collect();
        trending = trending.child(kit::rail(ui, s().mt(1.25), &format!("mk-trending-{frame}"), None, None, cards));
    } else if a.failed(&trend_path) {
        trending = trending.child(empty(ui, "Could not load Rare Marketplace", Some("Check the connection. This page refreshes on its own."), None, true));
    }

    let mut page = page.child(head).child(trending);

    // ---- your NFTs: only when the account holds something (no empty shelf while loading or empty).
    if let Some(acct) = &a.account {
        let items: Vec<ItemCard> = a.page::<ItemCard>(&paths::account_items(&acct.address)).map(|m| m.data).unwrap_or_default();
        if !items.is_empty() {
            let mine_total = a.page::<ItemCard>(&paths::account_items(&acct.address)).and_then(|m| m.total);
            let who = if acct.label.is_empty() { short_addr(Some(&acct.address)) } else { acct.label.clone() };
            let right = match mine_total {
                Some(n) => format!("{who}, {} items", fmt_int(Some(n as f64), false)),
                None => who,
            };
            let mut tiles: Vec<El> = items.iter().take(14).enumerate().map(|(i, it)| item_card(ui, s().w(11.0), it, 11.0, T_MINE + i as u32, false, true)).collect();
            let all = ui.bx(s().row().items_center().justify_center().w(11.0)).bg(tok::BUTTON_SECONDARY).radius(ui.r(0.5)).child(ui.text("See all", t(SM).w(500)));
            tiles.push(fill_nav(all, T_MINE_ALL, None, None, false));
            let sec = ui.bx(s().col().mt(2.5)).child(section_title(ui, s().px(2.5), "Your NFTs", Some(right))).child(kit::rail(ui, s(), &format!("mk-mine-{frame}"), None, None, tiles));
            page = page.child(sec);
        }
    }

    // ---- top collections
    let top_rows = a.rows::<CollectionRow>(&paths::collections(WINDOWS[win], "volume", 25));
    let top = &top_rows.items;
    let seg = segmented(ui, &WINDOWS, win, T_WIN);
    let thead = ui.bx(s().row().items_center().gap(0.5).pb(0.75)).child(ui.bx(s().pl(0.75)).child(ui.text("Top collections", t(XL).w(700)))).child(ui.bx(s().grow())).child(seg);
    let mut body = ui.bx(s().col()).group(kit::group(&format!("mk-top-{frame}"), false));
    for (i, c) in top.iter().take(100).enumerate() {
        body = body.child(collection_row(ui, c, T_ROW + i as u32));
    }
    let head_row = collection_head(ui, WINDOWS[win]);
    let mut table = ui.bx(s().col().mx(2.5).mt(2.5).p(1.0)).bg(tok::L1).radius(ui.r(1.0)).child(thead).child(head_row).child(body);
    if top_rows.ready && top.is_empty() {
        table = table.child(empty(ui, "No collections yet", None, None, true));
    } else if top.is_empty() && a.failed(&paths::collections(WINDOWS[win], "volume", 25)) {
        table = table.child(empty(ui, "Could not load collections", None, None, true));
    }
    page.child(table)
}

impl MkPage for HomePage {
    fn host(&mut self) -> &mut Host {
        &mut self.host
    }

    fn render(&mut self, cx: &mut Cx, now: f64) {
        let (a, win, frame) = (&*self.a, self.win, self.frame);
        self.host.render(cx, now, |ui| view(ui, a, win, frame));
    }

    fn press(&mut self, cx: &mut Cx, b: Pad, now: f64) -> Act {
        match b {
            Pad::X => {
                self.win = (self.win + 1) % WINDOWS.len();
                self.render(cx, now);
                Act::Moved
            }
            Pad::Lt | Pad::Rt => {
                Act::Out(Out::Act(section_step(if b == Pad::Rt { 1 } else { -1 })))
            }
            Pad::A => match self.host.tag() {
                Some(T_SEARCH) => Act::Push(View::Search { q: String::new() }),
                Some(t) if (T_TREND..T_TREND + 100).contains(&t) => {
                    let rows = self.trending();
                    rows.get((t - T_TREND) as usize).map(|c| Act::Push(View::Collection { slug: c.r#ref.slug.clone() })).unwrap_or(Act::None)
                }
                Some(T_MINE_ALL) => match &self.a.account {
                    Some(acct) => Act::Push(View::Account { address: acct.address.clone(), label: "Your NFTs".into() }),
                    None => Act::None,
                },
                Some(t) if (T_MINE..T_MINE + 100).contains(&t) => {
                    let Some(acct) = &self.a.account else { return Act::None };
                    let items = self.a.page::<ItemCard>(&paths::account_items(&acct.address)).map(|p| p.data).unwrap_or_default();
                    items.get((t - T_MINE) as usize).map(|it| Act::Push(View::Item { id: it.id.clone(), card: Some(Box::new(it.clone())), thumb: Some(400.0) })).unwrap_or(Act::None)
                }
                Some(t) if (T_WIN..T_WIN + 10).contains(&t) => {
                    self.win = (t - T_WIN) as usize;
                    self.render(cx, now);
                    Act::Moved
                }
                Some(t) if t >= T_ROW => {
                    let rows = self.top();
                    rows.get((t - T_ROW) as usize).map(|c| Act::Push(View::Collection { slug: c.r#ref.slug.clone() })).unwrap_or(Act::None)
                }
                _ => Act::None,
            },
            _ => Act::Pass,
        }
    }

    fn wants(&self) -> Vec<String> {
        let mut w = vec![paths::status(), paths::collections("24h", "volume", 12)];
        let base = paths::collections(WINDOWS[self.win], "volume", 25);
        let r = self.a.rows::<CollectionRow>(&base);
        w.push(base);
        // The table loads more as focus nears its end, up to 100 rows.
        let idx = self.host.tag().filter(|t| *t >= T_ROW).map(|t| (t - T_ROW) as usize);
        if let (Some(n), Some(i)) = (r.next, idx) {
            if i + 6 >= r.items.len() && r.items.len() < 100 {
                w.push(n);
            }
        }
        if let Some(acct) = &self.a.account {
            w.push(paths::account_items(&acct.address));
        }
        w
    }

    fn set_answers(&mut self, a: Rc<Answers>) {
        self.a = a;
    }

    fn hints(&self) -> Vec<(&'static str, String)> {
        vec![("a", "Open".into()), ("x", format!("Range {}", WINDOWS[self.win])), ("ltrt", "Sections".into()), ("y", "Search".into()), ("b", "Back".into()), ("qam", "Quick settings".into())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lt_rt_leave_for_the_trade_sections() {
        assert_eq!(section_step(1), "area:trade:home");
        assert_eq!(section_step(-1), "area:trade:traders");
    }
}
