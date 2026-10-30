//! Where the Market tab is: its two areas and four sections (launcher/src/market/area.ts), and the
//! marketplace's own page stack (marketplace/nav.ts). State and transitions only; the screens own
//! focus and drawing.
//!
//! Every page in the stack stays alive (the ones underneath are hidden), so B returns instantly to
//! the same scroll position, and focus goes back to the element that opened the page: each frame
//! keeps the screen's focus key for that.

use crate::api::ItemCard;

// ---------- market/area.ts ----------

/// The Market tab holds two page stacks: Collections (the marketplace) and Trade (coins, perps,
/// traders).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Area {
    #[default]
    Collections,
    Trade,
}

/// The sections along the top of each front page (LT / RT move between them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Collections,
    Coins,
    Perps,
    Traders,
}

pub const MARKET_SECTIONS: [(Section, &str); 4] = [(Section::Collections, "Collections"), (Section::Coins, "Coins"), (Section::Perps, "Perps"), (Section::Traders, "Traders")];

/// Which of the four sections is showing, given Trade's own section ("home", "perps", "positions",
/// "traders", ...). Trade's Positions page counts as Perps.
pub fn current_section(area: Area, trade: &str) -> Section {
    match area {
        Area::Collections => Section::Collections,
        Area::Trade => match trade {
            "perps" | "positions" => Section::Perps,
            "traders" => Section::Traders,
            _ => Section::Coins,
        },
    }
}

/// What showing a section changes: the area, and Trade's section when it goes to Trade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Go {
    pub area: Area,
    pub trade_section: Option<&'static str>,
}

pub fn go_section(s: Section) -> Go {
    match s {
        Section::Collections => Go { area: Area::Collections, trade_section: None },
        Section::Coins => Go { area: Area::Trade, trade_section: Some("home") },
        Section::Perps => Go { area: Area::Trade, trade_section: Some("perps") },
        Section::Traders => Go { area: Area::Trade, trade_section: Some("traders") },
    }
}

/// LT / RT from a front page: the next (`dir` 1) or previous (-1) section, wrapping.
pub fn step_section(area: Area, trade: &str, dir: i32) -> Go {
    let cur = current_section(area, trade);
    let n = MARKET_SECTIONS.len() as i32;
    let i = MARKET_SECTIONS.iter().position(|(s, _)| *s == cur).unwrap_or(0) as i32;
    go_section(MARKET_SECTIONS[((i + dir).rem_euclid(n)) as usize].0)
}

// ---------- marketplace/nav.ts ----------

#[derive(Clone, Debug, PartialEq)]
pub enum View {
    Home,
    Collection { slug: String },
    /// `card`/`thumb`: what the opener already showed (and the width its art was drawn at), painted
    /// at once while the item loads.
    Item { id: String, card: Option<Box<ItemCard>>, thumb: Option<f64> },
    Search { q: String },
    Account { address: String, label: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub key: u64,
    pub view: View,
    /// The screen's key of the element focused when the next page opened, restored on B.
    pub focus: Option<String>,
}

const MAX_DEPTH: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct MarketNav {
    pub stack: Vec<Frame>,
    seq: u64,
}

impl Default for MarketNav {
    fn default() -> Self {
        MarketNav { stack: vec![Frame { key: 1, view: View::Home, focus: None }], seq: 1 }
    }
}

impl MarketNav {
    fn next(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    pub fn top(&self) -> &Frame {
        self.stack.last().expect("home is always there")
    }

    /// Opens a page over the current one, remembering what had focus there.
    pub fn push(&mut self, view: View, focused: Option<String>) {
        if let Some(top) = self.stack.last_mut() {
            top.focus = focused;
        }
        let key = self.next();
        self.stack.push(Frame { key, view, focus: None });
        // Deep chains (item, collection, item, ...) drop the oldest pages above home.
        while self.stack.len() > MAX_DEPTH {
            self.stack.remove(1);
        }
    }

    /// Back one page; false on home.
    pub fn pop(&mut self) -> bool {
        if self.stack.len() <= 1 {
            return false;
        }
        self.stack.pop();
        true
    }

    /// Home, then `views` on top.
    pub fn reset(&mut self, views: Vec<View>) {
        let home = self.next();
        let mut stack = vec![Frame { key: home, view: View::Home, focus: None }];
        for view in views {
            let key = self.next();
            stack.push(Frame { key, view, focus: None });
        }
        self.stack = stack;
    }
}

/// How to open a marketplace page from anywhere in the launcher (search, wallet): pushed onto the
/// marketplace when it is already showing, else the stack is reset to it and the tab and area
/// switch. The caller closes overlays first (`app.close()`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenIn {
    Push,
    ResetAndShow,
}

pub fn open_in_market(nav: &mut MarketNav, view: View, on_market_tab: bool, area: Area, focused: Option<String>) -> OpenIn {
    if on_market_tab && area == Area::Collections {
        nav.push(view, focused);
        OpenIn::Push
    } else {
        nav.reset(vec![view]);
        OpenIn::ResetAndShow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// market/nav.test.ts "market sections" (the top-tab cases belong to the shell).
    #[test]
    fn sections_are_collections_coins_perps_traders() {
        assert_eq!(MARKET_SECTIONS.map(|(_, l)| l), ["Collections", "Coins", "Perps", "Traders"]);
    }

    #[test]
    fn sections_map_to_marketplace_and_trade() {
        assert_eq!(go_section(Section::Perps), Go { area: Area::Trade, trade_section: Some("perps") });
        assert_eq!(go_section(Section::Coins).trade_section, Some("home"));
        assert_eq!(go_section(Section::Collections).area, Area::Collections);
    }

    #[test]
    fn positions_count_as_perps() {
        assert_eq!(current_section(Area::Trade, "positions"), Section::Perps);
    }

    #[test]
    fn lt_rt_step_through_all_four_and_wrap() {
        let (mut area, mut trade) = (Area::Collections, "home");
        let mut seen = Vec::new();
        for _ in 0..4 {
            let g = step_section(area, trade, 1);
            area = g.area;
            if let Some(t) = g.trade_section {
                trade = t;
            }
            seen.push(current_section(area, trade));
        }
        assert_eq!(seen, [Section::Coins, Section::Perps, Section::Traders, Section::Collections]);
        let g = step_section(area, trade, -1);
        assert_eq!(current_section(g.area, g.trade_section.unwrap_or(trade)), Section::Traders);
    }

    #[test]
    fn page_stack_keeps_focus_caps_depth_and_resets() {
        let mut n = MarketNav::default();
        assert!(!n.pop());
        n.push(View::Collection { slug: "a".into() }, Some("tile-3".into()));
        assert_eq!(n.stack[0].focus.as_deref(), Some("tile-3"));
        for i in 0..10 {
            n.push(View::Search { q: i.to_string() }, None);
        }
        assert_eq!(n.stack.len(), 8);
        assert_eq!(n.stack[0].view, View::Home);
        assert_eq!(n.top().view, View::Search { q: "9".into() });
        assert!(n.pop());
        let keys: Vec<u64> = n.stack.iter().map(|f| f.key).collect();
        assert!(keys.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(open_in_market(&mut n, View::Home, false, Area::Collections, None), OpenIn::ResetAndShow);
        assert_eq!(n.stack.len(), 2);
        assert_eq!(open_in_market(&mut n, View::Collection { slug: "b".into() }, true, Area::Collections, None), OpenIn::Push);
        assert_eq!(n.stack.len(), 3);
    }
}
