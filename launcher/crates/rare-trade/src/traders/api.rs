//! The trader ledger behind Top traders and the trader page. Shapes follow the Trade API
//! (/leaderboard, /users/:address/*), which reads the ClickHouse trader ledger: realized
//! P/L per wallet and position, the 30 day and all time boards, funding links between wallets.
//! Every path here is GET: the boards are cached reads, the per-trader pages are uncached calls on
//! the host's GET-only allow-list.

use crate::http::{enc, get, Result};
use crate::jsnum::js_round;
use rare_market::net::Net;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoardWindow {
    #[default]
    #[serde(rename = "7d")]
    D7,
    #[serde(rename = "30d")]
    D30,
}

impl BoardWindow {
    pub fn as_str(&self) -> &'static str {
        match self {
            BoardWindow::D7 => "7d",
            BoardWindow::D30 => "30d",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            BoardWindow::D7 => "7 days",
            BoardWindow::D30 => "30 days",
        }
    }
    /// X switches between the two.
    pub fn other(&self) -> BoardWindow {
        match self {
            BoardWindow::D7 => BoardWindow::D30,
            BoardWindow::D30 => BoardWindow::D7,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UserChip {
    pub address: String,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TokenChip {
    pub coin_type: String,
    pub symbol: String,
    pub name: String,
    pub logo: Option<String>,
    pub price_usd: Option<f64>,
    pub change24h: Option<f64>,
    pub verified: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Team {
    pub members: u32,
    pub entity: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LeaderRow {
    pub rank: u32,
    pub user: UserChip,
    /// realized P/L in the window
    pub pnl_usd: f64,
    pub volume_usd: f64,
    pub trades: f64,
    /// profitable share of sells with a known cost
    pub win_rate: Option<f64>,
    pub top_tokens: Vec<TokenChip>,
    pub token_count: u32,
    /// team board rows: wallets linked by funding
    pub team: Option<Team>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BoardTag {
    pub key: String,
    pub label: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExtrasRow {
    pub tags: Vec<BoardTag>,
    pub curve30d: Vec<f64>,
    pub copy_pnl_usd: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BoardExtras {
    pub available: bool,
    pub rows: HashMap<String, ExtrasRow>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PnlPoint {
    pub time: f64,
    pub pnl_usd: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TraderHolding {
    pub coin: String,
    pub amount: f64,
    pub price_usd: Option<f64>,
    pub value_usd: Option<f64>,
    pub cost_usd: Option<f64>,
    pub unrealized: Option<f64>,
    pub token: Option<TokenChip>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TraderLabel {
    pub smart: bool,
    pub whale: bool,
    pub kol: bool,
}

impl TraderLabel {
    /// The ledger's labels as words: "Smart money", "Whale".
    pub fn words(&self) -> Vec<&'static str> {
        [(self.smart, "Smart money"), (self.whale, "Whale"), (self.kol, "KOL")].into_iter().filter(|(on, _)| *on).map(|(_, w)| w).collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TraderStats {
    pub address: String,
    pub window: String,
    pub realized: f64,
    pub unrealized: f64,
    pub pnl: f64,
    pub trades: f64,
    pub buys: f64,
    pub sells: f64,
    pub volume: f64,
    pub wins: f64,
    pub losses: f64,
    pub win_rate: Option<f64>,
    pub holdings_usd: Option<f64>,
    pub label: Option<TraderLabel>,
    /// the indexer reads this wallet as a bot
    pub bot: bool,
    pub first_trade_ms: Option<f64>,
    pub holdings: Vec<TraderHolding>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TraderTrade {
    pub id: String,
    pub digest: String,
    pub time_ms: f64,
    pub buy: bool,
    pub usd: f64,
    /// coin units
    pub amount: f64,
    pub price_usd: f64,
    pub trader: String,
    pub coin: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TradesPage {
    pub items: Vec<TraderTrade>,
    pub next_before: Option<f64>,
    pub tokens: HashMap<String, TokenChip>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LinkedWallet {
    pub address: String,
    pub username: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LinkedPage {
    pub total: u32,
    pub wallets: Vec<LinkedWallet>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PublicProfile {
    pub address: String,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub followers: Option<f64>,
    pub trades: Option<f64>,
}

pub mod trader_paths {
    use super::*;
    pub fn board(w: BoardWindow) -> String {
        format!("/leaderboard?window={}", w.as_str())
    }
    pub fn extras(w: BoardWindow) -> String {
        format!("/leaderboard/extras?window={}", w.as_str())
    }
    /// funding-linked wallet groups (30 days is the shortest window the team board has)
    pub fn teams() -> String {
        "/leaderboard?window=30d&view=teams".into()
    }
    pub fn profile(a: &str) -> String {
        format!("/users/{}", enc(a))
    }
    pub fn stats(a: &str, w: BoardWindow) -> String {
        format!("/users/{}/stats?window={}", enc(a), w.as_str())
    }
    pub fn pnl(a: &str, w: BoardWindow) -> String {
        format!("/users/{}/pnl?window={}", enc(a), w.as_str())
    }
    pub fn trades(a: &str, limit: u32) -> String {
        format!("/users/{}/trades?limit={limit}", enc(a))
    }
    pub fn linked(a: &str) -> String {
        format!("/users/{}/linked?limit=5", enc(a))
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Rows {
    rows: Vec<LeaderRow>,
}

pub fn board(net: &dyn Net, w: BoardWindow) -> Result<Vec<LeaderRow>> {
    Ok(get::<Rows>(net, &trader_paths::board(w))?.rows)
}

pub fn extras(net: &dyn Net, w: BoardWindow) -> Result<BoardExtras> {
    get(net, &trader_paths::extras(w))
}

pub fn teams(net: &dyn Net) -> Result<Vec<LeaderRow>> {
    Ok(get::<Rows>(net, &trader_paths::teams())?.rows)
}

pub fn profile(net: &dyn Net, address: &str) -> Result<PublicProfile> {
    get(net, &trader_paths::profile(address))
}

pub fn stats(net: &dyn Net, address: &str, w: BoardWindow) -> Result<TraderStats> {
    get(net, &trader_paths::stats(address, w))
}

pub fn pnl(net: &dyn Net, address: &str, w: BoardWindow) -> Result<Vec<PnlPoint>> {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Points {
        points: Vec<PnlPoint>,
    }
    Ok(get::<Points>(net, &trader_paths::pnl(address, w))?.points)
}

pub fn trades(net: &dyn Net, address: &str, limit: u32) -> Result<TradesPage> {
    get(net, &trader_paths::trades(address, limit))
}

pub fn linked(net: &dyn Net, address: &str) -> Result<LinkedPage> {
    get(net, &trader_paths::linked(address))
}

/// The name a trader goes by, Rare username first (`rare_primary`, from friends' names), else as
/// [`trader_name`].
pub fn trader_name_with(rare_primary: Option<&str>, u: &UserChip) -> String {
    match rare_primary {
        Some(p) => p.to_string(),
        None => trader_name(u),
    }
}

/// The name a trader goes by: their username, a display name, else a short address.
pub fn trader_name(u: &UserChip) -> String {
    if let Some(n) = u.username.as_deref().filter(|n| !n.is_empty()) {
        return format!("@{n}");
    }
    if let Some(n) = u.display_name.as_deref().filter(|n| !n.is_empty()) {
        return n.to_string();
    }
    let c: Vec<char> = u.address.chars().collect();
    let head: String = c.iter().take(6).collect();
    let tail: String = c[c.len().saturating_sub(4)..].iter().collect();
    format!("{head}...{tail}")
}

/// 0.534 -> "53%"; unknown is left out.
pub fn rate_pct(v: Option<f64>) -> String {
    match v.filter(|v| v.is_finite()) {
        Some(v) => format!("{}%", crate::jsnum::js_string(js_round(v * 100.0))),
        None => String::new(),
    }
}

/// "3m", "5h", "2d" since a time (ms).
pub fn ago(ms: f64, now_ms: f64) -> String {
    let s = js_round((now_ms - ms) / 1000.0).max(0.0) as u64;
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

#[cfg(test)]
mod tests {
    use super::*;
    use rare_market::net::{Method, MockNet};
    use serde_json::json;

    #[test]
    fn names_and_words() {
        let mut u = UserChip { address: format!("0x{}", "ab".repeat(32)), ..UserChip::default() };
        assert_eq!(trader_name(&u), "0xabab...abab");
        u.display_name = Some("Whale".into());
        assert_eq!(trader_name(&u), "Whale");
        u.username = Some("degen".into());
        assert_eq!(trader_name(&u), "@degen");
        assert_eq!(trader_name_with(Some("rare.sui"), &u), "rare.sui");
        assert_eq!(rate_pct(Some(0.534)), "53%");
        assert_eq!(rate_pct(None), "");
        assert_eq!(ago(0.0, 59_000.0), "59s");
        assert_eq!(ago(0.0, 3.0 * 86_400_000.0), "3d");
        assert_eq!(TraderLabel { smart: true, kol: true, ..TraderLabel::default() }.words(), ["Smart money", "KOL"]);
        assert_eq!(BoardWindow::D7.other().label(), "30 days");
    }

    #[test]
    fn boards_are_cached_and_trader_pages_are_calls() {
        let n = MockNet::new();
        n.on_read_json("trade:/leaderboard?window=7d", json!({ "rows": [{ "rank": 1, "user": { "address": "0x1", "username": null }, "pnlUsd": 5, "volumeUsd": 10, "trades": 3, "winRate": null, "topTokens": [], "tokenCount": 1 }] }));
        n.on_call(Method::Get, "https://rare.market/v1/trade/users/0x1/trades?limit=10", 200, json!({ "items": [], "nextBefore": null, "tokens": {} }).to_string());
        assert_eq!(board(&n, BoardWindow::D7).unwrap()[0].user.address, "0x1");
        assert!(trades(&n, "0x1", 10).unwrap().items.is_empty());
        assert_eq!(n.calls()[0].method, Method::Get);
    }
}
