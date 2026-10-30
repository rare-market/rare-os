//! The Trade area's page stack (trade/Trade.tsx, nav.ts): every page in the stack stays mounted
//! (the ones underneath hidden and inert), so B goes back instantly to the same scroll position with
//! focus on the element that opened the page. A page asks for another through `Out::Act`
//! ("token:<type>[:buy|sell]", "perp:<id>", "swap", "trader:<address>"); the tab pushes it with
//! data from its `Source` and hands everything else up to the app.

use super::perp::{PerpData, PerpPage};
use super::root::{RootData, RootPage, Section};
use super::swap::{SwapData, SwapPage};
use super::token::{Side, TokenData, TokenPage};
use super::trader::{TraderData, TraderPage};
use crate::screens::market::host::*;
use rare_render::chart::Textures;
use rare_render::input::Pad;
use rare_render::{DrawList, Id, Node, Scene, TextSys};

/// A page of the stack (nav.ts View).
#[derive(Clone, Debug, PartialEq)]
pub enum View {
    Root(Section),
    Token { coin: String, side: Option<Side> },
    Swap { from: Option<String>, to: Option<String> },
    Perp { market: String },
    Trader { address: String },
}

impl View {
    /// The view an `Out::Act` from a page asks for, if it is a Trade page.
    pub fn from_act(a: &str) -> Option<View> {
        let (kind, rest) = a.split_once(':').unwrap_or((a, ""));
        match kind {
            "swap" if rest.is_empty() => Some(View::Swap { from: None, to: None }),
            "perp" if !rest.is_empty() && !rest.starts_with("sign") => Some(View::Perp { market: rest.to_string() }),
            "trader" if rest.starts_with("0x") => Some(View::Trader { address: rest.to_ascii_lowercase() }),
            "token" if !rest.is_empty() => {
                // "token:<0x..::mod::NAME>[:buy|:sell]": the side is the last part when it is one.
                let (coin, side) = match rest.rsplit_once(':') {
                    Some((c, "buy")) => (c, Some(Side::Buy)),
                    Some((c, "sell")) => (c, Some(Side::Sell)),
                    _ => (rest, None),
                };
                Some(View::Token { coin: coin.to_string(), side })
            }
            _ => None,
        }
    }
}

/// Where the pages' data comes from (the app's trade store; fixtures in goldens and tests).
pub trait Source {
    fn root(&self) -> RootData;
    fn token(&self, coin: &str) -> TokenData;
    fn perp(&self, market: &str) -> PerpData;
    fn swap(&self) -> SwapData;
    fn trader(&self, address: &str) -> TraderData;
}

struct Frame {
    view: View,
    page: Box<dyn Page>,
    /// The frame's own layer under the tab (hidden while a page is on top of it).
    layer: Id,
}

/// At most this many pages deep; the oldest page above the root goes first.
const MAX_DEPTH: usize = 8;

pub struct TradeTab {
    parent: Id,
    stack: Vec<Frame>,
    source: Box<dyn Source>,
}

impl TradeTab {
    pub fn new(cx: &mut Cx, parent: Id, source: Box<dyn Source>, section: Section, now: f64) -> TradeTab {
        let mut t = TradeTab { parent, stack: Vec::new(), source };
        t.push_view(cx, View::Root(section), now, false);
        t
    }

    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// The front page's section.
    pub fn section(&self) -> Section {
        match self.stack[0].page.section() {
            Some(1) => Section::Perps,
            Some(2) => Section::Traders,
            Some(3) => Section::Positions,
            _ => Section::Home,
        }
    }

    /// Every page of the stack, bottom first, with its view and whether it is on top.
    pub fn each_page(&mut self, mut f: impl FnMut(&View, bool, &mut dyn Page)) {
        let n = self.stack.len();
        for (i, fr) in self.stack.iter_mut().enumerate() {
            f(&fr.view, i + 1 == n, fr.page.as_mut());
        }
    }

    /// Swaps the data source (live data replaces fixtures).
    pub fn set_source(&mut self, source: Box<dyn Source>) {
        self.source = source;
    }

    /// The coin on the front page when it is a token page: (coin type, symbol, price in USD).
    pub fn top_coin(&self) -> Option<(String, String, f64)> {
        let View::Token { coin, .. } = self.top_view() else { return None };
        let d = self.source.token(coin);
        let c = d.coin()?;
        Some((d.coin_type.clone(), c.symbol, c.price_usd?))
    }

    pub fn top_view(&self) -> &View {
        &self.stack.last().expect("root").view
    }

    fn layer(cx: &mut Cx, parent: Id) -> Id {
        let st = s().fill().to_style(cx.r());
        cx.scene.add(parent, st, Node::default())
    }

    /// Opens a page on top (nav.ts `push`), fading it in as `.fade-in` does.
    pub fn push_view(&mut self, cx: &mut Cx, view: View, now: f64, animate: bool) {
        if let Some(top) = self.stack.last() {
            cx.scene.n(top.layer).hidden = true;
        }
        let layer = Self::layer(cx, self.parent);
        let page: Box<dyn Page> = match &view {
            View::Root(sec) => Box::new(RootPage::new(cx, layer, self.source.root(), *sec, now)),
            View::Token { coin, side } => Box::new(TokenPage::new(cx, layer, self.source.token(coin), side.unwrap_or(Side::Buy), now)),
            View::Swap { from, to } => Box::new(SwapPage::new(cx, layer, self.source.swap(), from.as_deref(), to.as_deref(), now)),
            View::Perp { market } => Box::new(PerpPage::new(cx, layer, self.source.perp(market), now)),
            View::Trader { address } => Box::new(TraderPage::new(cx, layer, self.source.trader(address), now)),
        };
        if animate {
            let r = cx.r();
            let n = cx.scene.n(layer);
            n.opacity.restart(0.0, 1.0, now, 0.22);
            n.ty.restart(0.4 * r, 0.0, now, 0.22);
        }
        self.stack.push(Frame { view, page, layer });
        while self.stack.len() > MAX_DEPTH {
            let f = self.stack.remove(1);
            cx.scene.remove(f.layer);
        }
    }

    /// goSection: back to the front page, showing `sec` (the Market tab's LT/RT and deep links).
    pub fn set_section(&mut self, cx: &mut Cx, sec: Section, now: f64) {
        if self.stack.len() == 1 && self.section() == sec {
            return;
        }
        while self.pop(cx) {}
        if let Some(f) = self.stack.pop() {
            cx.scene.remove(f.layer);
        }
        self.push_view(cx, View::Root(sec), now, false);
    }

    /// B: back to the page underneath, with its focus where it was. False at the root.
    pub fn pop(&mut self, cx: &mut Cx) -> bool {
        if self.stack.len() <= 1 {
            return false;
        }
        let f = self.stack.pop().expect("frame");
        cx.scene.remove(f.layer);
        if let Some(top) = self.stack.last() {
            cx.scene.n(top.layer).hidden = false;
            cx.scene.mark_dirty();
        }
        true
    }

    /// A press goes to the page on top; what it asks for is handled here or handed up.
    pub fn press(&mut self, cx: &mut Cx, b: Pad, now: f64) -> Out {
        let Some(top) = self.stack.last_mut() else { return Out::None };
        match top.page.pad(cx, b, now) {
            Out::Close => {
                if self.pop(cx) {
                    Out::Moved
                } else {
                    Out::None
                }
            }
            Out::Act(a) => match View::from_act(&a) {
                Some(v) => {
                    self.push_view(cx, v, now, true);
                    Out::Moved
                }
                None => Out::Act(a),
            },
            o => o,
        }
    }
}

impl Page for TradeTab {
    fn prepare_paint(&mut self, scene: &Scene, ts: &mut TextSys, tex: &mut dyn Textures) {
        if let Some(top) = self.stack.last_mut() {
            top.page.prepare_paint(scene, ts, tex);
        }
    }
    fn paint_slot(&mut self, sp: &rare_render::scene::SlotPaint, ts: &mut TextSys, out: &mut DrawList) {
        if let Some(top) = self.stack.last_mut() {
            top.page.paint_slot(sp, ts, out);
        }
    }
    fn hints(&self) -> Vec<(&'static str, String)> {
        self.stack.last().map(|f| f.page.hints()).unwrap_or_default()
    }
}

impl Screen for TradeTab {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.stack.last_mut().is_some_and(|top| top.page.touch_focus(cx, x, y, now))
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        self.press(cx, pad, now)
    }
    fn tick(&mut self, cx: &mut Cx, now: f64) {
        if let Some(top) = self.stack.last_mut() {
            top.page.tick(cx, now);
        }
    }
}

/// Fixture data for every page (the captured web answers).
pub struct Fixtures;

impl Source for Fixtures {
    fn root(&self) -> RootData {
        let mut d = RootData::from_fixture(&super::model::trade_root(), Some(super::root::demo_wallet()));
        d.traders = super::traders::TradersData::from_fixture(&super::traders::traders_fixture());
        d
    }
    fn token(&self, coin: &str) -> TokenData {
        let f = super::model::token_deep();
        if super::fmt::normalize_type(coin) == super::fmt::normalize_type(&f.coin_type) {
            TokenData::from_fixture(&f, Some(super::DEMO))
        } else {
            TokenData { now_ms: f.now, coin_type: super::fmt::normalize_type(coin), wallet: Some(super::DEMO.into()), ..Default::default() }
        }
    }
    fn perp(&self, market: &str) -> PerpData {
        let f = super::model::perp_btc();
        if market.eq_ignore_ascii_case(&f.market_id) {
            PerpData::from_fixture(&f, Some(super::DEMO))
        } else {
            PerpData { now_ms: f.now, wallet: Some(super::DEMO.into()), ..Default::default() }
        }
    }
    fn swap(&self) -> SwapData {
        SwapData::from_fixture(&super::swap::swap_fixture(), Some(super::DEMO))
    }
    fn trader(&self, address: &str) -> TraderData {
        let f = super::trader::trader_fixture();
        if address.eq_ignore_ascii_case(&f.address) {
            TraderData::from_fixture(&f, Some(super::DEMO))
        } else {
            TraderData { now_ms: f.now, address: address.to_string(), wallet: Some(super::DEMO.into()), ..Default::default() }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acts_name_pages() {
        assert_eq!(View::from_act("swap"), Some(View::Swap { from: None, to: None }));
        assert_eq!(View::from_act("perp:0xabc"), Some(View::Perp { market: "0xabc".into() }));
        assert_eq!(View::from_act("perp:sign"), None);
        assert_eq!(View::from_act("token:0x2::sui::SUI:buy"), Some(View::Token { coin: "0x2::sui::SUI".into(), side: Some(Side::Buy) }));
        assert_eq!(View::from_act("token:0xdeeb::deep::DEEP"), Some(View::Token { coin: "0xdeeb::deep::DEEP".into(), side: None }));
        assert_eq!(View::from_act("trader:0xABC"), Some(View::Trader { address: "0xabc".into() }));
        assert_eq!(View::from_act("search"), None);
        assert_eq!(View::from_act("swap:sign"), None);
    }

    /// Opens DEEP from Home's trending rail, then B comes back to Home with focus on the card.
    #[test]
    fn push_and_pop_keep_focus() {
        use rare_render::{ImageLoader, Scene, TextSys};
        static FONTS: [&[u8]; 1] = [include_bytes!("../../../../render/assets/fonts/Inter-Regular.ttf")];
        let mut ts = TextSys::new(&FONTS, false, 1024);
        let mut img = ImageLoader::new(1, std::sync::Arc::new(|| {}), None);
        let mut scene = Scene::new(1920.0, 1200.0, 24.0);
        let mut cx = Cx { scene: &mut scene, t: &mut ts, img: &mut img };
        let main = super::super::main_area(&mut cx);
        let mut tab = TradeTab::new(&mut cx, main, Box::new(Fixtures), Section::Home, 0.0);
        assert_eq!(tab.depth(), 1);
        assert_eq!(tab.press(&mut cx, Pad::A, 0.1), Out::Moved);
        assert_eq!(tab.depth(), 2);
        assert!(matches!(tab.top_view(), View::Token { .. }));
        assert_eq!(tab.hints()[0].1, "Select");
        assert_eq!(tab.press(&mut cx, Pad::B, 0.2), Out::Moved);
        assert_eq!(tab.depth(), 1);
        assert_eq!(tab.hints()[0].1, "Open");
        assert_eq!(tab.press(&mut cx, Pad::B, 0.3), Out::None);
    }
}
