//! Rare Marketplace (launcher/src/marketplace, market/) and the Phase 4 pages' host (`host`: the
//! View, focus and art every Phase 4 page runs on, over the kit).
//!
//! `shell::Marketplace` is the Marketplace tab's page stack; `home`, `collection`, `item`,
//! `search` and `account` are its pages, `buy` the purchase sheet, `parts` rare.market's pieces and
//! `data` what the pages read (rare.market answers by path, fixtures captured from the web).

pub mod account;
pub mod buy;
pub mod cart;
pub mod collection;
pub mod data;
pub mod feed;
pub mod home;
pub mod host;
pub mod item;
pub mod live;
pub mod parts;
pub mod search;
pub mod shell;
pub mod tab;

#[cfg(test)]
mod shots;
#[cfg(test)]
mod tests;

use host::{Cx, Page, Screen};
use rare_market::nav::View;
use rare_render::Id;
use std::rc::Rc;

/// A Marketplace page by fixture name, built into `parent` (the shell's main area).
pub fn page(name: &str, cx: &mut Cx, parent: Id, now: f64) -> Option<Box<dyn Page>> {
    let mk = |cx: &mut Cx, a: data::Answers, views: Vec<View>| -> Box<dyn Page> { Box::new(shell::Marketplace::new(cx, parent, Rc::new(a), views, now)) };
    let coll = || vec![View::Collection { slug: "popkins".into() }];
    Some(match name {
        "market-home" => mk(cx, data::home(), vec![]),
        "market-home-mine" => {
            // Your NFTs (a Rare Wallet account): the home answers plus the wallet's items. The web
            // shows the row only for an account it can sign with, so this one has no web shot.
            let mut a = data::home();
            let acct = data::account();
            let path = rare_market::api::paths::account_items(data::DEMO);
            if let Some(v) = acct.get::<serde_json::Value>(&path) {
                a.set(path, v);
            }
            a.account = Some(data::Account { address: data::DEMO.into(), label: "Main".into(), signer: true });
            mk(cx, a, vec![])
        }
        "market-collection" => mk(cx, data::collection(), coll()),
        "market-list" => {
            collection::set_list_view(true);
            let p = mk(cx, data::collection(), coll());
            collection::set_list_view(false);
            p
        }
        "market-activity" | "market-holders" | "market-filters" => {
            let mut m = shell::Marketplace::new(cx, parent, Rc::new(data::collection()), coll(), now);
            let script: &[rare_render::input::Pad] = match name {
                "market-activity" => &[rare_render::input::Pad::Rt],
                "market-holders" => &[rare_render::input::Pad::Rt, rare_render::input::Pad::Rt],
                _ => &[rare_render::input::Pad::X],
            };
            for p in script {
                m.press(cx, *p, now);
            }
            Box::new(m)
        }
        "market-item" => mk(cx, data::item(), vec![View::Item { id: data::ITEM.into(), card: None, thumb: None }]),
        "market-buy" => {
            let mut m = shell::Marketplace::new(cx, parent, Rc::new(data::item()), vec![View::Item { id: data::ITEM.into(), card: None, thumb: None }], now);
            m.press(cx, rare_render::input::Pad::A, now);
            Box::new(m)
        }
        "market-sweep" => Box::new(cart::SweepShot::new(cx, parent, false, now)),
        "market-sweep-budget" => Box::new(cart::SweepShot::new(cx, parent, true, now)),
        "market-search" => mk(cx, data::search(), vec![View::Search { q: "pop".into() }]),
        "market-account" => mk(cx, data::account(), vec![View::Account { address: data::DEMO.into(), label: "Wallet".into() }]),
        _ => return None,
    })
}

/// Builds a Marketplace screen by fixture name (the WS 13 golden harness and tests).
pub fn fixture(name: &str, cx: &mut Cx, now: f64) -> Option<Box<dyn Screen>> {
    let main = crate::screens::trade::main_area(cx);
    page(name, cx, main, now).map(|p| p as Box<dyn Screen>)
}

/// Every Phase 4 page by fixture name (Trade, Marketplace, Store, Friends, Share).
pub fn any_page(name: &str, cx: &mut Cx, parent: Id, now: f64) -> Option<Box<dyn Page>> {
    crate::screens::trade::page(name, cx, parent, now)
        .or_else(|| page(name, cx, parent, now))
        .or_else(|| crate::screens::store::page(name, cx, parent, now))
        .or_else(|| crate::screens::friends::page(name, cx, parent, now))
        .or_else(|| crate::screens::share::page(name, cx, parent, now))
}

/// The Marketplace fixtures the golden harness shoots by default.
pub const SHOTS: &[&str] = &["market-home", "market-home-mine", "market-collection", "market-list", "market-activity", "market-holders", "market-filters", "market-item", "market-buy", "market-sweep", "market-sweep-budget", "market-search", "market-account"];

/// Every Phase 4 fixture the golden harness shoots by default.
pub fn all_shots() -> Vec<&'static str> {
    [crate::screens::trade::SHOTS, SHOTS, crate::screens::store::SHOTS, crate::screens::friends::SHOTS, crate::screens::share::SHOTS].concat()
}
