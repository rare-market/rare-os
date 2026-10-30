//! The Trade tab's page stack and its controller rules, without the drawing: every page in the
//! stack stays alive (the ones underneath are hidden), so B returns instantly to the same place with
//! focus on what opened the page. Each frame keeps whatever focus mark the screen gives it.

use rare_core::Pad;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    Home,
    Perps,
    Traders,
    Positions,
}

pub const SECTIONS: [(Section, &str); 4] = [(Section::Home, "Home"), (Section::Perps, "Perps"), (Section::Traders, "Traders"), (Section::Positions, "Positions")];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    Root,
    Token { coin_type: String, side: Option<Side> },
    Swap { from: Option<String>, to: Option<String> },
    Perp { market_id: String },
    Trader { address: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame<F> {
    pub key: u64,
    pub view: View,
    /// Where focus was when the next page opened, restored on B.
    pub focus: Option<F>,
}

pub const MAX_DEPTH: usize = 8;

/// The stack and the root's section (the web launcher's `useTradeNav`). `F` is the screen's focus
/// mark (a node id and scroll, say).
#[derive(Clone, Debug)]
pub struct TradeNav<F> {
    pub stack: Vec<Frame<F>>,
    pub section: Section,
    seq: u64,
}

impl<F: Clone> Default for TradeNav<F> {
    fn default() -> Self {
        TradeNav { stack: vec![Frame { key: 1, view: View::Root, focus: None }], section: Section::Home, seq: 1 }
    }
}

impl<F: Clone> TradeNav<F> {
    pub fn new() -> Self {
        Self::default()
    }

    fn frame(&mut self, view: View) -> Frame<F> {
        self.seq += 1;
        Frame { key: self.seq, view, focus: None }
    }

    pub fn top(&self) -> &Frame<F> {
        self.stack.last().expect("the root frame is never popped")
    }

    pub fn set_section(&mut self, s: Section) {
        self.section = s;
    }

    /// Opens a page over the current one, remembering where focus was on it. The stack never grows
    /// past MAX_DEPTH: the oldest page above the root goes.
    pub fn push(&mut self, view: View, focus: Option<F>) {
        if let Some(top) = self.stack.last_mut() {
            top.focus = focus;
        }
        let f = self.frame(view);
        self.stack.push(f);
        while self.stack.len() > MAX_DEPTH {
            self.stack.remove(1);
        }
    }

    /// B: back one page. False on the root.
    pub fn pop(&mut self) -> bool {
        if self.stack.len() <= 1 {
            return false;
        }
        self.stack.pop();
        true
    }

    /// The root with `views` over it.
    pub fn reset(&mut self, views: Vec<View>) {
        let mut stack = vec![self.frame(View::Root)];
        for v in views {
            let f = self.frame(v);
            stack.push(f);
        }
        self.stack = stack;
    }

    /// Opens a Trade page from anywhere in the launcher (wallet, search): pushed when Trade is
    /// already showing, else the stack starts over with it (the caller then switches to Trade).
    /// True when it was pushed.
    pub fn open(&mut self, view: View, trade_showing: bool, focus: Option<F>) -> bool {
        if trade_showing {
            self.push(view, focus);
            true
        } else {
            self.reset(vec![view]);
            false
        }
    }

    /// After the top changed from `prev_top`: the focus to restore when that was a step back.
    pub fn restore_after(&self, prev_top: u64) -> Option<&F> {
        let top = self.top();
        if top.key == prev_top || self.stack.iter().any(|f| f.key == prev_top) {
            return None;
        }
        top.focus.as_ref()
    }

    /// Dev and screenshots: `perps | traders | positions | token/<type> | sell/<type> | swap |
    /// perp/<id> | trader/<address>[,more pages]`. Applied only on a fresh stack.
    pub fn apply_deep_link(&mut self, tr: &str) {
        if tr.is_empty() || self.stack.len() > 1 {
            return;
        }
        let (section, views) = parse_deep_link(tr);
        if let Some(s) = section {
            self.section = s;
        }
        if !views.is_empty() {
            self.reset(views);
        }
    }
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && b[i + 1].is_ascii_hexdigit() && b[i + 2].is_ascii_hexdigit() {
            let hex = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
            out.push(hex(b[i + 1]) * 16 + hex(b[i + 2]));
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The section and pages a `tr` link names (the last section named wins).
pub fn parse_deep_link(tr: &str) -> (Option<Section>, Vec<View>) {
    let mut section = None;
    let mut views = Vec::new();
    for p in tr.split(',') {
        let (kind, arg) = p.split_once('/').unwrap_or((p, ""));
        match kind {
            "home" => section = Some(Section::Home),
            "perps" => section = Some(Section::Perps),
            "traders" => section = Some(Section::Traders),
            "positions" => section = Some(Section::Positions),
            "token" if !arg.is_empty() => views.push(View::Token { coin_type: decode(arg), side: None }),
            "sell" if !arg.is_empty() => views.push(View::Token { coin_type: decode(arg), side: Some(Side::Sell) }),
            "perp" if !arg.is_empty() => views.push(View::Perp { market_id: arg.to_string() }),
            "swap" => views.push(View::Swap { from: None, to: None }),
            "trader" if arg.len() == 66 && arg.starts_with("0x") && arg[2..].bytes().all(|b| b.is_ascii_hexdigit()) => views.push(View::Trader { address: arg.to_lowercase() }),
            _ => {}
        }
    }
    (section, views)
}

/// A bottom-bar hint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hint {
    pub b: Pad,
    pub label: &'static str,
}

pub const fn hint(b: Pad, label: &'static str) -> Hint {
    Hint { b, label }
}

pub mod h {
    use super::*;
    pub const OPEN: Hint = hint(Pad::A, "Open");
    pub const SELECT: Hint = hint(Pad::A, "Select");
    pub const BACK: Hint = hint(Pad::B, "Back");
    pub const CLOSE: Hint = hint(Pad::B, "Close");
    pub const SEARCH: Hint = hint(Pad::Y, "Search");
    pub const QS: Hint = hint(Pad::Qam, "Quick settings");
}

/// What a sheet does with a button: B closes it, the page's own shortcuts are held while it is up,
/// everything else goes on to the sheet's controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SheetPad {
    Close,
    Hold,
    Pass,
}

pub fn sheet_pad(b: Pad) -> SheetPad {
    match b {
        Pad::B => SheetPad::Close,
        Pad::Lb | Pad::Rb | Pad::Lt | Pad::Rt | Pad::X | Pad::Y => SheetPad::Hold,
        _ => SheetPad::Pass,
    }
}

/// The hints while a sheet is up but inactive (another sheet over it).
pub const SHEET_IDLE_HINTS: [Hint; 2] = [hint(Pad::A, "Select"), hint(Pad::B, "Back")];

/// Which part of a page holds focus: the chart, the order panel, or anything else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Area {
    Chart,
    Order,
    #[default]
    Other,
}

/// The perps page's hints for where focus is (`flip` is "Short" on a long ticket, "Long" on a short).
pub fn perp_hints(area: Area, flip: &'static str) -> Vec<Hint> {
    let x = hint(Pad::X, flip);
    match area {
        Area::Order => vec![h::SELECT, hint(Pad::Lt, "Less leverage"), hint(Pad::Rt, "More leverage"), x, h::SEARCH, h::BACK],
        Area::Chart => vec![hint(Pad::A, "Control chart"), hint(Pad::Select, "Chart mode"), x, h::SEARCH, h::BACK],
        Area::Other => vec![h::SELECT, x, h::SEARCH, h::BACK],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(t: &str) -> View {
        View::Token { coin_type: t.into(), side: None }
    }

    #[test]
    fn stack_push_pop_reset() {
        let mut n: TradeNav<u32> = TradeNav::new();
        assert!(!n.pop());
        let root = n.top().key;
        n.push(token("a"), Some(7));
        assert_eq!(n.stack[0].focus, Some(7));
        let a = n.top().key;
        n.push(View::Perp { market_id: "m".into() }, Some(3));
        assert!(n.pop());
        assert_eq!(n.restore_after(a + 1), Some(&3));
        assert_eq!(n.restore_after(n.top().key), None);
        assert!(n.pop());
        assert_eq!(n.top().key, root);
        for i in 0..20 {
            n.push(token(&i.to_string()), None);
        }
        assert_eq!(n.stack.len(), MAX_DEPTH);
        assert_eq!(n.stack[0].view, View::Root);
        assert_eq!(n.top().view, token("19"));
        assert!(!n.open(token("x"), false, None));
        assert_eq!(n.stack.len(), 2);
        assert!(n.open(token("y"), true, None));
        assert_eq!(n.stack.len(), 3);
    }

    #[test]
    fn deep_links() {
        let addr = format!("0x{}", "AB".repeat(32));
        let (s, v) = parse_deep_link(&format!("traders,token/0x2%3A%3Asui%3A%3ASUI,sell/0xa::b::C,perp/0x1,swap,trader/{addr},trader/0x1,bogus"));
        assert_eq!(s, Some(Section::Traders));
        assert_eq!(
            v,
            vec![
                token("0x2::sui::SUI"),
                View::Token { coin_type: "0xa::b::C".into(), side: Some(Side::Sell) },
                View::Perp { market_id: "0x1".into() },
                View::Swap { from: None, to: None },
                View::Trader { address: addr.to_lowercase() }
            ]
        );
        let mut n: TradeNav<()> = TradeNav::new();
        n.apply_deep_link("positions");
        assert_eq!((n.section, n.stack.len()), (Section::Positions, 1));
        n.apply_deep_link("swap,perp/0x2");
        assert_eq!(n.stack.len(), 3);
        n.apply_deep_link("swap");
        assert_eq!(n.stack.len(), 3);
    }

    #[test]
    fn sheets_and_hints() {
        assert_eq!(sheet_pad(Pad::B), SheetPad::Close);
        assert_eq!(sheet_pad(Pad::Rt), SheetPad::Hold);
        assert_eq!(sheet_pad(Pad::A), SheetPad::Pass);
        assert_eq!(perp_hints(Area::Order, "Short")[3], hint(Pad::X, "Short"));
        assert_eq!(perp_hints(Area::Other, "Long").len(), 4);
    }
}
