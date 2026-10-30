//! A wallet's NFTs (marketplace/AccountPage.tsx): Your NFTs, a holder, a search result, in
//! rare.market's item card grid.

use super::collection::COLS;
use super::data::{near_end, Answers};
use super::host::*;
use super::parts::*;
use super::shell::{Act, MkPage};
use rare_market::api::{paths, ItemCard};
use rare_market::format::{fmt_int, short_addr};
use rare_market::nav::View;
use rare_render::input::Pad;
use rare_render::view::{key, El};
use std::rc::Rc;

pub const T_OPEN: u32 = 1;
pub const T_ITEM: u32 = 1000;

pub struct AccountPage {
    pub a: Rc<Answers>,
    pub address: String,
    pub label: String,
    pub frame: u64,
    pub host: Host,
}

impl AccountPage {
    pub fn new(cx: &mut Cx, parent: rare_render::Id, a: Rc<Answers>, address: &str, label: &str, frame: u64, now: f64) -> AccountPage {
        let mut p = AccountPage { a, address: address.to_string(), label: label.to_string(), frame, host: Host::new(cx, parent) };
        p.render(cx, now);
        p
    }

    fn items(&self) -> Vec<ItemCard> {
        self.a.rows::<ItemCard>(&paths::account_items(&self.address)).items
    }
}

pub fn view(ui: &mut Ui, a: &Answers, address: &str, label: &str, frame: u64) -> El {
    let page = ui.bx(s().fill().col().px(2.5).pb(2.5).pt(1.0)).scrolls_y(key(&format!("mk-acct-{frame}")));
    let items = a.rows::<ItemCard>(&paths::account_items(address));
    let av = wallet_avatar(ui, s(), address, 3.5);
    let mut runs = vec![(t(SM).mono().c(tok::MUTED), short_addr(Some(address)))];
    if let Some(n) = items.total {
        runs.push((t(SM).c(tok::MUTED), format!(", {} items", fmt_int(Some(n as f64), false))));
    }
    let sub = baseline_row(ui, s(), runs);
    let names = ui.bx(s().col().min0().shrink()).child(ui.text(label, t(XL).w(600).trunc())).child(sub);
    let open = btn(ui, s(), "Open on rare.market", T_OPEN, V::Secondary, Sz::Md, false, false);
    let head = ui.bx(s().row().items_center().gap(0.75).mb(1.5)).child(av).child(names).child(ui.bx(s().ml_auto()).child(open));
    let cw = (ui.w / ui.rem - 5.0 - 0.75 * (COLS as f32 - 1.0)) / COLS as f32;
    let mut g = ui.bx(s().grid(&[Track::Fr(1.0); COLS]).gap(0.75)).group(kit::group(&format!("mk-acct-{frame}"), false));
    let list = &items.items;
    for (i, it) in list.iter().enumerate() {
        g = g.child(item_card(ui, s().min0(), it, cw, T_ITEM + i as u32, i == 0, true));
    }
    let mut page = page.child(head).child(g);
    if items.ready && list.is_empty() {
        page = page.child(empty(ui, "No NFTs in this wallet", None, None, false));
    } else if list.is_empty() && a.failed(&paths::account_items(address)) {
        page = page.child(empty(ui, "Could not load this wallet", None, None, false));
    }
    page
}

impl MkPage for AccountPage {
    fn host(&mut self) -> &mut Host {
        &mut self.host
    }

    fn render(&mut self, cx: &mut Cx, now: f64) {
        let (a, address, label, frame) = (&*self.a, self.address.as_str(), self.label.as_str(), self.frame);
        self.host.render(cx, now, |ui| view(ui, a, address, label, frame));
    }

    fn press(&mut self, _cx: &mut Cx, b: Pad, _now: f64) -> Act {
        match b {
            Pad::A => match self.host.tag() {
                Some(T_OPEN) => Act::Out(Out::Act(format!("open:https://rare.market/u/{}", self.address))),
                Some(t) if t >= T_ITEM => self.items().get((t - T_ITEM) as usize).map(|it| Act::Push(View::Item { id: it.id.clone(), card: Some(Box::new(it.clone())), thumb: Some(400.0) })).unwrap_or(Act::None),
                _ => Act::None,
            },
            _ => Act::Pass,
        }
    }

    fn wants(&self) -> Vec<String> {
        let base = paths::account_items(&self.address);
        let r = self.a.rows::<ItemCard>(&base);
        let idx = self.host.tag().filter(|t| *t >= T_ITEM).map(|t| (t - T_ITEM) as usize);
        let mut w = vec![base];
        if let Some(n) = r.next.filter(|_| near_end(idx, r.items.len(), COLS)) {
            w.push(n);
        }
        w
    }

    fn set_answers(&mut self, a: Rc<Answers>) {
        self.a = a;
    }

    fn hints(&self) -> Vec<(&'static str, String)> {
        vec![("a", "Open".into()), ("y", "Search".into()), ("b", "Back".into())]
    }
}
