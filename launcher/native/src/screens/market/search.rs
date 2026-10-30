//! Marketplace search (marketplace/SearchPage.tsx): the on-screen keyboard for the term (the app
//! runs it and calls `set_term`), then wallets and collections.

use super::data::Answers;
use super::host::*;
use super::parts::*;
use super::shell::{Act, MkPage};
use rare_market::api::{paths, SearchResult};
use rare_market::format::short_addr;
use rare_market::nav::View;
use rare_render::input::Pad;
use rare_render::view::{key, El};
use std::rc::Rc;

pub const T_FIELD: u32 = 1;
pub const T_WALLET: u32 = 100;
pub const T_COLL: u32 = 1000;

/// What the keyboard is asked for (askText's title and placeholder).
pub const ASK: &str = "osk:Search Rare Marketplace|Collection name, wallet or name.sui";

pub struct SearchPage {
    pub a: Rc<Answers>,
    pub q: String,
    pub frame: u64,
    pub host: Host,
}

impl SearchPage {
    pub fn new(cx: &mut Cx, parent: rare_render::Id, a: Rc<Answers>, q: &str, frame: u64, now: f64) -> SearchPage {
        let mut p = SearchPage { a, q: q.to_string(), frame, host: Host::new(cx, parent) };
        p.render(cx, now);
        p
    }

    /// The keyboard's answer: None (cancelled) keeps the term; the app pops the page when the
    /// keyboard was cancelled on a page opened without one.
    pub fn set_term(&mut self, cx: &mut Cx, q: Option<&str>, now: f64) {
        if let Some(q) = q {
            self.q = q.trim().to_string();
            self.render(cx, now);
        }
    }

    fn result(&self) -> Option<SearchResult> {
        if self.q.is_empty() {
            return None;
        }
        self.a.get(&paths::search(&self.q))
    }
}

pub fn view(ui: &mut Ui, a: &Answers, q: &str, frame: u64) -> El {
    let mut page = ui.bx(s().fill().col().px(2.5).pb(2.5).pt(1.0)).scrolls_y(key(&format!("mk-search-{frame}")));
    let field = search_field(ui, s().wp(1.0), 3.0, if q.is_empty() { "Search collections" } else { q }, q.is_empty(), BASE, 1.1, T_FIELD);
    page = page.child(field);
    let res: Option<SearchResult> = if q.is_empty() { None } else { a.get(&paths::search(q)) };
    let Some(r) = res else { return page };
    if r.collections.is_empty() && r.accounts.is_empty() {
        return page.child(empty(ui, &format!("No results for \"{q}\""), None, None, false));
    }
    let h2 = |ui: &Ui, txt: &str| ui.text_s(s().mb(0.5), txt, t(XS).c(tok::MUTED));
    if !r.accounts.is_empty() {
        let mut sec = ui.bx(s().col().mt(1.5)).child(h2(ui, "WALLETS"));
        for (i, w) in r.accounts.iter().enumerate() {
            let name = w.name.clone().unwrap_or_else(|| short_addr(Some(&w.address)));
            let av = wallet_avatar(ui, s(), &w.address, 2.0);
            let row = ui
                .bx(s().row().items_center().gap(0.75).h(3.5).wp(1.0).px(0.75))
                .radius(ui.r(0.5))
                .child(av)
                .child(ui.text(name, t(SM).w(600)))
                .child(ui.text(short_addr(Some(&w.address)), t(SM).mono().c(tok::MUTED)))
                .key(key(&w.address));
            sec = sec.child(fill_nav(row, T_WALLET + i as u32, None, None, false));
        }
        page = page.child(sec);
    }
    if !r.collections.is_empty() {
        let mut sec = ui.bx(s().col().mt(1.5)).child(h2(ui, "COLLECTIONS"));
        for (i, c) in r.collections.iter().enumerate() {
            let av = avatar(ui, s(), c.r#ref.image_url.as_deref(), &c.r#ref.name, 2.5, false);
            let mut name = ui.bx(s().row().items_center().gap(0.375).grow()).child(ui.text(c.r#ref.name.clone(), t(SM).w(600).trunc()));
            if c.r#ref.verified {
                name = name.child(kit::verified(ui, 0.95));
            }
            let fl = sui(ui, s(), c.floor_mist.as_deref(), 2, false, t(SM).w(600));
            let vol = sui(ui, s(), Some(&c.volume_mist), 2, true, t(SM).w(600));
            let floor = ui.bx(s().col().items_end().w(10.0)).child(ui.text("Floor", t(XS).c(tok::MUTED))).child(fl);
            let volume = ui.bx(s().col().items_end().w(10.0)).child(ui.text("Volume 7d", t(XS).c(tok::MUTED))).child(vol);
            let row = ui.bx(s().row().items_center().gap(0.75).h(4.0).wp(1.0).px(0.75)).radius(ui.r(0.5)).child(av).child(name).child(floor).child(volume).key(key(&c.r#ref.slug));
            sec = sec.child(fill_nav(row, T_COLL + i as u32, None, None, i == 0));
        }
        page = page.child(sec);
    }
    page
}

impl MkPage for SearchPage {
    fn host(&mut self) -> &mut Host {
        &mut self.host
    }

    fn render(&mut self, cx: &mut Cx, now: f64) {
        let (a, q, frame) = (&*self.a, self.q.as_str(), self.frame);
        self.host.render(cx, now, |ui| view(ui, a, q, frame));
    }

    fn is_search(&self) -> bool {
        true
    }

    fn press(&mut self, cx: &mut Cx, b: Pad, now: f64) -> Act {
        let _ = (cx, now);
        match b {
            Pad::Y => Act::Out(Out::Act(ASK.into())),
            Pad::A => match self.host.tag() {
                Some(T_FIELD) => Act::Out(Out::Act(ASK.into())),
                Some(t) if (T_WALLET..T_COLL).contains(&t) => {
                    let r = self.result().unwrap_or_default();
                    r.accounts.get((t - T_WALLET) as usize).map(|w| Act::Push(View::Account { address: w.address.clone(), label: w.name.clone().unwrap_or_else(|| short_addr(Some(&w.address))) })).unwrap_or(Act::None)
                }
                Some(t) if t >= T_COLL => {
                    let r = self.result().unwrap_or_default();
                    r.collections.get((t - T_COLL) as usize).map(|c| Act::Push(View::Collection { slug: c.r#ref.slug.clone() })).unwrap_or(Act::None)
                }
                _ => Act::None,
            },
            _ => Act::Pass,
        }
    }

    fn wants(&self) -> Vec<String> {
        if self.q.is_empty() {
            return vec![];
        }
        let mut w = vec![paths::search(&self.q)];
        // The wallets found (a name.sui, an address): their items are read now, so the wallet page
        // has them when it opens instead of starting a cold read on the tap.
        if let Some(r) = self.result() {
            w.extend(r.accounts.iter().take(2).map(|a| paths::account_items(&a.address)));
        }
        w
    }

    fn set_answers(&mut self, a: Rc<Answers>) {
        self.a = a;
    }

    fn set_query(&mut self, cx: &mut Cx, q: Option<&str>, now: f64) -> bool {
        self.set_term(cx, q, now);
        true
    }

    fn hints(&self) -> Vec<(&'static str, String)> {
        vec![("a", "Open".into()), ("y", "Edit search".into()), ("b", "Back".into())]
    }
}
