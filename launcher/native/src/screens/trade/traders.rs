//! Top traders (trade/traders/TradersSection.tsx, board.ts, parts.tsx): the ledger's 7 and 30 day
//! boards by realized P/L or win rate, with a minimum of activity and wash or bot-like wallets left
//! out. Followed traders and their latest trades sit on the right. X switches 7 and 30 days; A
//! opens a trader.

use super::ui::{coin_logo, tf_chips};
use crate::kit::format::{NumKind, NumOptions};
use crate::kit::{cell, dot, head_row, num, panel, row, section_head, spark, DotTone};
use crate::screens::market::host::*;
use rare_render::view::El;
use serde::Deserialize;
use std::collections::HashMap;

/// At least this many trades in the window...
pub const MIN_TRADES: f64 = 10.0;
/// ...and this much volume, in USD.
pub const MIN_VOLUME_USD: f64 = 1_000.0;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct UserChip {
    pub address: String,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct TokenChip {
    pub coin_type: String,
    pub symbol: String,
    pub name: String,
    pub logo: Option<String>,
    pub price_usd: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Team {
    pub members: u32,
    pub entity: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct LeaderRow {
    pub rank: u32,
    pub user: UserChip,
    pub pnl_usd: f64,
    pub volume_usd: f64,
    pub trades: f64,
    pub win_rate: Option<f64>,
    pub top_tokens: Vec<TokenChip>,
    pub token_count: u32,
    pub team: Option<Team>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct BoardTag {
    pub key: String,
    pub label: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Extra {
    pub tags: Vec<BoardTag>,
    pub curve30d: Vec<f64>,
    pub copy_pnl_usd: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct BoardExtras {
    pub available: bool,
    pub rows: HashMap<String, Extra>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Rows<T> {
    pub rows: Vec<T>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window {
    D7,
    D30,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Pnl,
    WinRate,
}

/// One ranked trader.
#[derive(Clone, Debug, PartialEq)]
pub struct TopTrader {
    pub address: String,
    pub row: LeaderRow,
    pub tags: Vec<BoardTag>,
    /// Cumulative realized P/L by day over 30 days, when the board has it.
    pub curve: Vec<f64>,
    /// Wallets in this trader's funding-linked group, itself included.
    pub linked: Option<u32>,
}

const FLAGGED: [&str; 2] = ["bot", "wash"];

pub fn is_flagged(tags: &[BoardTag]) -> bool {
    tags.iter().any(|t| FLAGGED.contains(&t.key.as_str()))
}

pub fn is_active(r: &LeaderRow) -> bool {
    r.trades >= MIN_TRADES && r.volume_usd >= MIN_VOLUME_USD
}

/// Funding-linked group sizes by wallet, from the team board.
pub fn linked_sizes(teams: &[LeaderRow]) -> HashMap<String, u32> {
    teams.iter().filter_map(|t| t.team.as_ref().filter(|x| x.members > 1).map(|x| (t.user.address.to_ascii_lowercase(), x.members))).collect()
}

/// board.ts `topTraders`.
pub fn top_traders(rows: &[LeaderRow], extras: Option<&BoardExtras>, sort: Sort, teams: &HashMap<String, u32>, limit: usize) -> Vec<TopTrader> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for row in rows {
        let address = row.user.address.to_ascii_lowercase();
        if !seen.insert(address.clone()) {
            continue;
        }
        let x = extras.and_then(|e| e.rows.get(&row.user.address).or_else(|| e.rows.get(&address)));
        let tags = x.map(|x| x.tags.clone()).unwrap_or_default();
        if is_flagged(&tags) || !is_active(row) {
            continue;
        }
        out.push(TopTrader { linked: teams.get(&address).copied(), address, row: row.clone(), tags, curve: x.map(|x| x.curve30d.clone()).unwrap_or_default() });
    }
    match sort {
        Sort::WinRate => {
            out.retain(|t| t.row.win_rate.is_some() && t.row.pnl_usd > 0.0);
            out.sort_by(|a, b| b.row.win_rate.unwrap_or(0.0).partial_cmp(&a.row.win_rate.unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal).then(b.row.pnl_usd.partial_cmp(&a.row.pnl_usd).unwrap_or(std::cmp::Ordering::Equal)));
        }
        Sort::Pnl => out.sort_by(|a, b| b.row.pnl_usd.partial_cmp(&a.row.pnl_usd).unwrap_or(std::cmp::Ordering::Equal)),
    }
    out.truncate(limit);
    out
}

/// Style tags worth a word in a row (the flagged ones never reach a row).
pub fn style_words(tags: &[BoardTag]) -> Vec<String> {
    tags.iter().filter(|t| !FLAGGED.contains(&t.key.as_str())).map(|t| t.label.clone()).collect()
}

/// The name a trader goes by: their username, a display name, else a short address.
pub fn trader_name(u: &UserChip) -> String {
    if let Some(n) = u.username.as_deref().filter(|n| !n.is_empty()) {
        return format!("@{n}");
    }
    if let Some(n) = u.display_name.as_deref().filter(|n| !n.is_empty()) {
        return n.to_string();
    }
    let a = &u.address;
    format!("{}...{}", &a[..6.min(a.len())], &a[a.len().saturating_sub(4)..])
}

/// 0.534 -> "53%".
pub fn pct(v: Option<f64>) -> String {
    v.filter(|x| x.is_finite()).map(|x| format!("{}%", (x * 100.0).round() as i64)).unwrap_or_default()
}

/// "3m", "5h", "2d" since a time.
pub fn ago(ms: f64, now: f64) -> String {
    let s = ((now - ms) / 1000.0).round().max(0.0) as i64;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86_400 {
        format!("{}h", s / 3600)
    } else {
        format!("{}d", s / 86_400)
    }
}

/// A followed trader (follow.ts): kept on the device per wallet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Follow {
    pub address: String,
    pub name: String,
}

/// What the section reads.
#[derive(Clone, Debug, Default)]
pub struct TradersData {
    pub now_ms: f64,
    pub board: Vec<LeaderRow>,
    pub board_loaded: bool,
    pub board_error: bool,
    pub extras: Option<BoardExtras>,
    pub teams: Vec<LeaderRow>,
    pub follows: Vec<Follow>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TradersFixture {
    pub now: f64,
    pub board: Rows<LeaderRow>,
    pub extras: BoardExtras,
    pub teams: Rows<LeaderRow>,
}

pub fn traders_fixture() -> TradersFixture {
    serde_json::from_str(include_str!("fixtures/traders.json")).expect("traders fixture")
}

impl TradersData {
    pub fn from_fixture(f: &TradersFixture) -> TradersData {
        TradersData { now_ms: f.now, board: f.board.rows.clone(), board_loaded: true, board_error: false, extras: Some(f.extras.clone()), teams: f.teams.rows.clone(), follows: Vec::new() }
    }
    pub fn ranked(&self, sort: Sort) -> Vec<TopTrader> {
        top_traders(&self.board, self.extras.as_ref(), sort, &linked_sizes(&self.teams), 50)
    }
}

pub const T_WINDOW: u32 = 8100;
pub const T_SORT: u32 = 8200;
pub const T_TRADER: u32 = 8300;
pub const T_FOLLOWING: u32 = 8900;

const COLS: [Track; 7] = [Track::Rem(2.0), Track::Fr(1.0), Track::Rem(7.5), Track::Rem(5.0), Track::Rem(4.5), Track::Rem(6.0), Track::Rem(5.5)];

/// `TraderAvatar`: their picture, else the wallet's gem pattern.
pub fn trader_avatar(ui: &mut Ui, u: &UserChip, size: f32) -> El {
    match u.avatar_url.as_deref().filter(|a| !a.is_empty()) {
        Some(src) => crate::kit::avatar(ui, s(), Some(src), &trader_name(u), size, false),
        None => crate::screens::market::parts::wallet_avatar(ui, s(), &u.address, size),
    }
}

fn ct(ui: &Ui, text: impl Into<String>, t: T, end: bool) -> El {
    cell(ui, end).child(ui.text(text, t.tab()))
}

/// The section's page (TradersSection's JSX).
pub fn section(ui: &mut Ui, d: &TradersData, win: Window, sort: Sort) -> El {
    let rows = d.ranked(sort);
    let mut wins = tf_chips(ui, &["7 days", "30 days"], if win == Window::D7 { 0 } else { 1 }, "tr-traders-win", T_WINDOW);
    // useLandingFocus: the section's first focusable (the window chips), not the search above.
    if let Some(first) = wins.kids.first_mut() {
        *first = default_nav(std::mem::take(first));
    }
    let sorts = tf_chips(ui, &["Realized P/L", "Win rate"], if sort == Sort::Pnl { 0 } else { 1 }, "tr-traders-sort", T_SORT);
    let note = format!("At least {} trades and ${}K volume. Wash and bot-like wallets left out.", MIN_TRADES as i64, (MIN_VOLUME_USD / 1000.0) as i64);
    let bar = ui.bx(s().row().items_center().gap(1.0).mx(2.5).mb(1.0)).child(wins).child(sorts).child(ui.bx(s().ml_auto()).child(ui.text(note, T::new(Ty::Label).c(tok::MUTED))));

    let heads: Vec<El> = ["#", "Trader", "Realized P/L", "Win rate", "Trades", "Volume", "30 days"].iter().enumerate().map(|(i, l)| ct(ui, *l, T::new(Ty::Label).w(500).c(tok::MUTED), i > 1)).collect();
    let mut table = panel(ui, s().col().min0().p(1.0)).child(head_row(ui, &COLS, heads));
    let mut list = ui.bx(s().col()).group(crate::kit::group("tr-traders", false));
    let following: Vec<String> = d.follows.iter().map(|f| f.address.to_ascii_lowercase()).collect();
    for (i, t) in rows.iter().enumerate() {
        let r = &t.row;
        let mut words: Vec<String> = style_words(&t.tags).into_iter().take(2).collect();
        if let Some(l) = t.linked {
            words.push(format!("Linked to {} {}", l - 1, if l == 2 { "wallet" } else { "wallets" }));
        }
        let mut name_line = ui.bx(s().row().items_center().gap(0.5)).child(ui.text(trader_name(&r.user), T::new(Ty::Body).w(600).trunc()));
        if following.contains(&t.address) {
            name_line = name_line.child(dot(ui, s(), DotTone::Primary, false));
        }
        let sub = if !words.is_empty() {
            ui.text(words.join(", "), T::new(Ty::Label).c(tok::MUTED).trunc())
        } else {
            top_coins(ui, r)
        };
        let names = ui.bx(s().col().min0()).child(name_line).child(ui.bx(s().row().items_center().gap(0.75).min0()).child(sub));
        let avatar = trader_avatar(ui, &r.user, 2.25);
        let who = cell(ui, false).child(ui.bx(s().row().items_center().gap(0.75).min0()).child(avatar).child(names));
        let small = T::new(Ty::Small);
        let curve = cell(ui, true).maybe((t.curve.len() > 1).then(|| {
            let pts: Vec<crate::kit::charts::Point> = t.curve.iter().enumerate().map(|(k, v)| (k as f64, *v)).collect();
            spark(ui, s(), &pts, 5.0, 1.5)
        }));
        let cells = vec![
            ct(ui, (i + 1).to_string(), small.c(tok::MUTED), false),
            who,
            cell(ui, true).maybe(num(ui, s(), NumKind::Pnl, Some(r.pnl_usd), NumOptions::default(), small.w(600), true)),
            ct(ui, pct(r.win_rate), small, true),
            ct(ui, super::fmt::grouped(r.trades, 0, 0), small.c(tok::MUTED), true),
            cell(ui, true).maybe(num(ui, s(), NumKind::Market, Some(r.volume_usd), NumOptions::default(), small.c(tok::MUTED), true)),
            curve,
        ];
        let rw = row(ui, &COLS, T_TRADER + i as u32, Some("Open"), cells).with_style(s().grid(&COLS).items_center().h(3.75).wp(1.0).gap_xy(0.5, 0.0).px(0.75).to_style(ui.rem));
        list = list.child(rw.key(rare_render::view::key(&t.address)));
    }
    table = table.child(list);
    if rows.is_empty() {
        let msg = if d.board_error {
            "Top traders are unavailable right now. This page refreshes on its own."
        } else if d.board_loaded {
            "No trader passes the activity minimum yet."
        } else {
            ""
        };
        table = table.child(center_text(ui, s().py(2.0).wp(1.0), msg, T::new(Ty::Small).c(tok::MUTED)));
    }

    let note = (!d.follows.is_empty()).then(|| d.follows.len().to_string());
    let mut fol = panel(ui, s().col().p(1.0)).child(section_head(ui, s().mb(0.5).px(0.75).pt(0.25), "Following", note.as_deref(), None));
    if d.follows.is_empty() {
        fol = fol.child(ui.text_s(s().px(0.75).pb(0.5), "Follow a trader to get their trades as they happen.", T::new(Ty::Small).c(tok::MUTED).wrap()));
    } else {
        let mut l = ui.bx(s().col()).group(crate::kit::group("tr-following", false));
        for (i, f) in d.follows.iter().enumerate() {
            let av = trader_avatar(ui, &UserChip { address: f.address.clone(), ..Default::default() }, 1.75);
            let r = ui.bx(s().row().items_center().gap(0.75).h(3.0).wp(1.0).px(0.75)).radius(ui.r(0.5)).child(av).child(ui.text(f.name.clone(), T::new(Ty::Body).w(600).trunc()));
            l = l.child(crate::kit::nav(crate::kit::row_look(r), T_FOLLOWING + i as u32, Some("Open"), None, false));
        }
        fol = fol.child(l);
    }
    let right = ui.bx(s().col().gap(1.25).min0()).child(fol);
    let grid = ui.bx(s().grid(&[Track::Fr(1.0), Track::Rem(22.0)]).gap(1.25).mx(2.5).items_start()).child(table).child(right);
    ui.bx(s().fill().col().pt(1.0).pb(2.5)).scrolls_y(rare_render::view::key("tr-traders")).child(bar).child(grid)
}

/// The coins that made a trader the most, as small logos with their symbols.
fn top_coins(ui: &mut Ui, r: &LeaderRow) -> El {
    let mut row = ui.bx(s().row().items_center().gap(0.35));
    for t in r.top_tokens.iter().take(3) {
        let logo = super::model::logo_url(t.logo.as_deref());
        let chip = ui.bx(s().row().items_center().gap(0.25)).child(coin_logo(ui, logo.as_deref(), &t.symbol, 1.0)).child(ui.text(t.symbol.clone(), T::new(Ty::Label).c(tok::MUTED)));
        row = row.child(chip);
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lr(addr: &str, pnl: f64, trades: f64, vol: f64, win: Option<f64>) -> LeaderRow {
        LeaderRow { user: UserChip { address: addr.into(), ..Default::default() }, pnl_usd: pnl, trades, volume_usd: vol, win_rate: win, ..Default::default() }
    }

    #[test]
    fn board_rules() {
        let rows = vec![lr("0xA", 10.0, 50.0, 5_000.0, Some(0.4)), lr("0xb", 50.0, 5.0, 5_000.0, Some(0.9)), lr("0xc", 30.0, 20.0, 9_000.0, Some(0.8)), lr("0xa", 99.0, 50.0, 9_000.0, None), lr("0xd", 70.0, 30.0, 9_000.0, Some(0.5))];
        let mut extras = BoardExtras::default();
        extras.rows.insert("0xd".into(), Extra { tags: vec![BoardTag { key: "bot".into(), label: "Bot".into(), reason: String::new() }], ..Default::default() });
        let t = top_traders(&rows, Some(&extras), Sort::Pnl, &HashMap::new(), 50);
        assert_eq!(t.iter().map(|x| x.address.as_str()).collect::<Vec<_>>(), vec!["0xc", "0xa"]);
        let w = top_traders(&rows, Some(&extras), Sort::WinRate, &HashMap::new(), 50);
        assert_eq!(w[0].address, "0xc");
        assert_eq!(pct(Some(0.534)), "53%");
        assert_eq!(ago(0.0, 300_000.0), "5m");
        assert_eq!(trader_name(&UserChip { address: "0x12710cab3ccb5800181db8a48139b4a55ef6217786435c8b251daa2582a3dc3a".into(), ..Default::default() }), "0x1271...dc3a");
    }

    #[test]
    fn fixture_ranks() {
        let d = TradersData::from_fixture(&traders_fixture());
        let r = d.ranked(Sort::Pnl);
        assert!(!r.is_empty());
        assert!(r.windows(2).all(|w| w[0].row.pnl_usd >= w[1].row.pnl_usd));
    }
}
