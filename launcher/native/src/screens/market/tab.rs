//! The Market tab (market/Market.tsx): the marketplace (Collections) and Trade (Coins, Perps,
//! Traders) as sections of one tab. Each area keeps its own pages; LT / RT on a front page step
//! through the four sections (market/area.ts via rare_market::nav), crossing between the areas.

use super::host::*;
use super::shell::Marketplace;
use crate::screens::trade::root::Section as TradeSection;
use crate::screens::trade::tab::TradeTab;
use rare_market::nav::Area;
use rare_render::input::Pad;
use rare_render::{Id, Node};

pub struct MarketTab {
    pub area: Area,
    pub market: Marketplace,
    pub trade: TradeTab,
    market_node: Id,
    trade_node: Id,
}

/// Trade's section for an `area:trade:<section>` act (goSection's names).
pub fn trade_section(name: &str) -> TradeSection {
    match name {
        "perps" => TradeSection::Perps,
        "positions" => TradeSection::Positions,
        "traders" => TradeSection::Traders,
        _ => TradeSection::Home,
    }
}

impl MarketTab {
    /// `build_market` and `build_trade` mount each area into the node they are given.
    pub fn new(cx: &mut Cx, parent: Id, area: Area, build_market: impl FnOnce(&mut Cx, Id) -> Marketplace, build_trade: impl FnOnce(&mut Cx, Id) -> TradeTab) -> MarketTab {
        let st = s().fill().to_style(cx.r());
        let market_node = cx.scene.add(parent, st.clone(), Node::default());
        let trade_node = cx.scene.add(parent, st, Node::default());
        let market = build_market(cx, market_node);
        let trade = build_trade(cx, trade_node);
        let mut t = MarketTab { area, market, trade, market_node, trade_node };
        t.show(cx);
        t
    }

    fn show(&mut self, cx: &mut Cx) {
        cx.scene.n(self.market_node).hidden = self.area != Area::Collections;
        cx.scene.n(self.trade_node).hidden = self.area != Area::Trade;
        cx.scene.mark_dirty();
    }

    /// setArea / goSection: shows an area (and Trade's section).
    pub fn go(&mut self, cx: &mut Cx, area: Area, section: Option<TradeSection>, now: f64) {
        if let Some(sec) = section {
            self.trade.set_section(cx, sec, now);
        }
        if self.area != area {
            self.area = area;
            self.show(cx);
        }
    }

    pub fn press(&mut self, cx: &mut Cx, b: Pad, now: f64) -> Out {
        let out = match self.area {
            Area::Collections => self.market.press(cx, b, now),
            Area::Trade => self.trade.pad(cx, b, now),
        };
        match &out {
            Out::Act(a) if a == "area:collections" => {
                self.go(cx, Area::Collections, None, now);
                Out::Moved
            }
            Out::Act(a) if a.starts_with("area:trade:") => {
                let sec = trade_section(&a["area:trade:".len()..]);
                self.go(cx, Area::Trade, Some(sec), now);
                Out::Moved
            }
            _ => out,
        }
    }
}

impl Page for MarketTab {
    fn prepare_paint(&mut self, scene: &rare_render::Scene, ts: &mut rare_render::TextSys, tex: &mut dyn rare_render::chart::Textures) {
        if self.area == Area::Trade {
            self.trade.prepare_paint(scene, ts, tex);
        }
    }
    fn paint_slot(&mut self, sp: &rare_render::scene::SlotPaint, ts: &mut rare_render::TextSys, out: &mut rare_render::DrawList) {
        if self.area == Area::Trade {
            self.trade.paint_slot(sp, ts, out);
        }
    }
    fn hints(&self) -> Vec<(&'static str, String)> {
        match self.area {
            Area::Collections => self.market.hints(),
            Area::Trade => self.trade.hints(),
        }
    }
}

impl Screen for MarketTab {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        match self.area {
            Area::Collections => self.market.touch_focus(cx, x, y, now),
            Area::Trade => self.trade.touch_focus(cx, x, y, now),
        }
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        self.press(cx, pad, now)
    }
    fn tick(&mut self, cx: &mut Cx, now: f64) {
        match self.area {
            Area::Collections => self.market.tick(cx, now),
            Area::Trade => self.trade.tick(cx, now),
        }
    }
}
