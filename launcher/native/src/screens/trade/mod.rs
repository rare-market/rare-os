//! Rare Trade (launcher/src/trade): the Market tab's Coins, Perps, Traders and Positions sections and
//! the coin, swap, perp and trader pages, as native scenes. Data comes from fixtures captured from
//! the web launcher until rare-trade lands.

pub mod cards;
pub mod chart_area;
pub mod fmt;
pub mod live;
pub mod model;
pub mod perp;
pub mod review;
pub mod root;
pub mod search;
pub mod swap;
pub mod tab;
pub mod ticket;
pub mod token;
pub mod trader;
pub mod traders;
pub mod ui;

use crate::screens::market::host::{Cx, Page, Screen};
use rare_render::{Id, Node};

/// The demo watch-only wallet the web screenshots use.
pub const DEMO: &str = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";

/// The shell's main area (between the 4.25 rem top bar and the 3 rem hint bar), for fixtures.
pub fn main_area(cx: &mut Cx) -> rare_render::Id {
    let r = cx.r();
    let root = cx.scene.root;
    let st = crate::kit::s().abs(Some(4.25), Some(0.0), Some(3.0), Some(0.0)).to_style(r);
    cx.scene.add(root, st, Node::default())
}

/// A Trade page by fixture name, built into `parent` (the shell's main area).
pub fn page(name: &str, cx: &mut Cx, parent: Id, now: f64) -> Option<Box<dyn Page>> {
    Some(match name {
        "trade-perp" => Box::new(perp::PerpPage::new(cx, parent, perp::PerpData::from_fixture(&model::perp_btc(), Some(DEMO)), now)),
        "trade-perp-nowallet" => Box::new(perp::PerpPage::new(cx, parent, perp::PerpData::from_fixture(&model::perp_btc(), None), now)),
        "trade-home" => Box::new(root::RootPage::new(cx, parent, root::RootData::from_fixture(&model::trade_root(), Some(root::demo_wallet())), root::Section::Home, now)),
        "trade-home-nowallet" => Box::new(root::RootPage::new(cx, parent, root::RootData::from_fixture(&model::trade_root(), None), root::Section::Home, now)),
        "trade-perps" => Box::new(root::RootPage::new(cx, parent, root::RootData::from_fixture(&model::trade_root(), Some(root::demo_wallet())), root::Section::Perps, now)),
        "trade-token" => Box::new(token::TokenPage::new(cx, parent, token::TokenData::from_fixture(&model::token_deep(), Some(DEMO)), token::Side::Buy, now)),
        "trade-token-sell" => Box::new(token::TokenPage::new(cx, parent, token::TokenData::from_fixture(&model::token_deep(), Some(DEMO)), token::Side::Sell, now)),
        "trade-traders" => {
            let mut d = root::RootData::from_fixture(&model::trade_root(), Some(root::demo_wallet()));
            d.traders = traders::TradersData::from_fixture(&traders::traders_fixture());
            Box::new(root::RootPage::new(cx, parent, d, root::Section::Traders, now))
        }
        "trade-trader" => Box::new(trader::TraderPage::new(cx, parent, trader::TraderData::from_fixture(&trader::trader_fixture(), Some(DEMO)), now)),
        "trade-tab" => Box::new(tab::TradeTab::new(cx, parent, Box::new(tab::Fixtures), root::Section::Home, now)),
        "trade-swap" => Box::new(swap::SwapPage::new(cx, parent, swap::SwapData::from_fixture(&swap::swap_fixture(), Some(DEMO)), None, None, now)),
        "trade-positions" => Box::new(root::RootPage::new(cx, parent, root::RootData::from_fixture(&model::trade_root(), Some(root::demo_wallet())), root::Section::Positions, now)),
        _ => return None,
    })
}

/// Builds a Trade screen by fixture name (the WS 13 golden harness and tests).
pub fn fixture(name: &str, cx: &mut Cx, now: f64) -> Option<Box<dyn Screen>> {
    if SHOTS.contains(&name) {
        let main = main_area(cx);
        return page(name, cx, main, now).map(|p| p as Box<dyn Screen>);
    }
    None
}

/// The Trade fixtures (the golden harness shoots these by default).
pub const SHOTS: &[&str] = &["trade-perp", "trade-perp-nowallet", "trade-home", "trade-home-nowallet", "trade-perps", "trade-positions", "trade-token", "trade-token-sell", "trade-swap", "trade-traders", "trade-trader", "trade-tab"];
