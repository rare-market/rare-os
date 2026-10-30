//! Market data for the Trade tab: token lists, a token's page (chart, holders, trades), the
//! wallet's priced portfolio and the perps logos. Shapes follow the Trade API routes
//! (/tokens, /token/:type/*, /ext/*). Every one of these paths is a cached read
//! (`trade:<path>` through rare-state), so going back is instant and nothing flashes; the web
//! launcher's in-memory stale-while-revalidate cache is rare-state's freshness policy now.

use crate::amount::normalize_type;
use crate::http::{enc, get, Result};
use rare_market::net::Net;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Change {
    pub m5: Option<f64>,
    pub h1: Option<f64>,
    pub h6: Option<f64>,
    pub h24: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TokenRow {
    pub coin_type: String,
    pub symbol: String,
    pub name: String,
    pub decimals: u32,
    pub logo: Option<String>,
    pub price_usd: Option<f64>,
    pub change: Option<Change>,
    /// Lean rows (ext) carry the 24h change flat.
    pub change24h: Option<f64>,
    pub volume24h: Option<f64>,
    pub liquidity: Option<f64>,
    pub mcap: Option<f64>,
    pub holders: Option<f64>,
    pub buys24h: Option<f64>,
    pub sells24h: Option<f64>,
    pub created_ms: Option<f64>,
    pub verified: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Link {
    pub label: String,
    pub url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TokenDetail {
    #[serde(flatten)]
    pub row: TokenRow,
    pub fdv: Option<f64>,
    pub supply: Option<f64>,
    pub description: Option<String>,
    pub links: Vec<Link>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Holder {
    pub address: String,
    pub amount: f64,
    pub value_usd: Option<f64>,
    pub pct_supply: Option<f64>,
    pub kind: String,
    pub venue: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HoldersPage {
    pub count: Option<f64>,
    pub holders: Vec<Holder>,
    pub top10_pct: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Trade {
    pub id: String,
    pub time_ms: f64,
    pub buy: bool,
    pub usd: f64,
    pub amount: f64,
    pub price_usd: f64,
    pub trader: String,
}

/// A chart bar; `time` in unix seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Candle {
    pub time: f64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Holding {
    pub coin_type: String,
    pub symbol: String,
    pub name: String,
    pub decimals: u32,
    pub logo: Option<String>,
    pub price_usd: Option<f64>,
    pub change24h: Option<f64>,
    pub verified: bool,
    pub amount: f64,
    pub base_units: String,
    pub value_usd: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Portfolio {
    pub total_usd: f64,
    pub change24h_usd: f64,
    pub change24h_pct: f64,
    pub holdings: Vec<Holding>,
}

/// A logo as an absolute URL: the Trade API serves some from its own paths (/coins/usdc.png).
pub fn logo_url(u: Option<&str>) -> Option<String> {
    let u = u.filter(|u| !u.is_empty())?;
    Some(if let Some(rest) = u.strip_prefix("//") {
        format!("https://{rest}")
    } else if u.starts_with('/') {
        format!("{}{u}", rare_market::net::TRADE_ASSETS)
    } else {
        u.to_string()
    })
}

/// The 24h change of a full or a lean row.
pub fn change24(t: &TokenRow) -> Option<f64> {
    t.change.as_ref().and_then(|c| c.h24).or(t.change24h)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenList {
    Trending,
    New,
    Gainers,
    Top,
}

impl TokenList {
    pub fn as_str(&self) -> &'static str {
        match self {
            TokenList::Trending => "trending",
            TokenList::New => "new",
            TokenList::Gainers => "gainers",
            TokenList::Top => "top",
        }
    }
}

/// The paths behind each read (the cache keys are `trade:` + these).
pub mod token_paths {
    use super::*;
    pub fn list(list: TokenList, limit: u32) -> String {
        format!("/tokens?list={}&limit={limit}", list.as_str())
    }
    pub fn pick_list(limit: u32) -> String {
        format!("/ext/tokens?limit={limit}")
    }
    pub fn token(t: &str) -> String {
        format!("/token/{}", enc(&normalize_type(t)))
    }
    pub fn lean(t: &str) -> String {
        format!("/ext/token/{}", enc(&normalize_type(t)))
    }
    pub fn holders(t: &str, limit: u32) -> String {
        format!("/token/{}/holders?limit={limit}", enc(&normalize_type(t)))
    }
    pub fn trades(t: &str) -> String {
        format!("/token/{}/trades", enc(&normalize_type(t)))
    }
    pub fn candles(t: &str, tf: &str, limit: u32) -> String {
        format!("/token/{}/candles?tf={tf}&limit={limit}", enc(&normalize_type(t)))
    }
    pub fn portfolio(address: &str) -> String {
        format!("/ext/portfolio/{address}")
    }
    pub fn search(q: &str) -> String {
        format!("/ext/search?q={}", enc(q))
    }
}

/// the Trade API's full perps list, read only for each market's picture (the ext list has none).
pub const PERP_LOGO_PATH: &str = "/perps?l=1";

#[derive(Deserialize, Default)]
#[serde(default)]
struct Items<T> {
    items: Vec<T>,
}

pub fn tokens(net: &dyn Net, list: TokenList, limit: u32) -> Result<Vec<TokenRow>> {
    Ok(get::<Items<TokenRow>>(net, &token_paths::list(list, limit))?.items)
}

/// The swap screen's coin list.
pub fn pick_list(net: &dyn Net, limit: u32) -> Result<Vec<TokenRow>> {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Tokens {
        tokens: Vec<TokenRow>,
    }
    Ok(get::<Tokens>(net, &token_paths::pick_list(limit))?.tokens)
}

pub fn token(net: &dyn Net, coin_type: &str) -> Result<TokenDetail> {
    get(net, &token_paths::token(coin_type))
}

pub fn lean(net: &dyn Net, coin_type: &str) -> Result<TokenRow> {
    get(net, &token_paths::lean(coin_type))
}

pub fn holders(net: &dyn Net, coin_type: &str, limit: u32) -> Result<HoldersPage> {
    get(net, &token_paths::holders(coin_type, limit))
}

pub fn trades(net: &dyn Net, coin_type: &str) -> Result<Vec<Trade>> {
    Ok(get::<Items<Trade>>(net, &token_paths::trades(coin_type))?.items)
}

pub fn candles(net: &dyn Net, coin_type: &str, tf: &str, limit: u32) -> Result<Vec<Candle>> {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Bars {
        candles: Vec<Candle>,
    }
    Ok(get::<Bars>(net, &token_paths::candles(coin_type, tf, limit))?.candles)
}

pub fn portfolio(net: &dyn Net, address: &str) -> Result<Portfolio> {
    get(net, &token_paths::portfolio(address))
}

/// Market pictures by id (and by symbol as a fallback), shared by every perps screen.
#[derive(Clone, Debug, Default)]
pub struct PerpLogos {
    by_id: HashMap<String, String>,
    by_sym: HashMap<String, String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LogoItem {
    id: String,
    logo: Option<String>,
    symbol: Option<String>,
}

impl PerpLogos {
    /// From the Trade API's perps list (`{ items: [{ id, logo, symbol }] }`).
    pub fn from_body(body: &serde_json::Value) -> PerpLogos {
        let items: Items<LogoItem> = serde_json::from_value(body.clone()).unwrap_or_default();
        let mut out = PerpLogos::default();
        for it in items.items {
            let Some(logo) = it.logo.filter(|l| !l.is_empty()) else { continue };
            if let Some(s) = it.symbol.filter(|s| !s.is_empty()) {
                out.by_sym.insert(s.to_uppercase(), logo.clone());
            }
            out.by_id.insert(it.id.to_lowercase(), logo);
        }
        out
    }

    /// Reads the list through the cache.
    pub fn load(net: &dyn Net) -> Result<PerpLogos> {
        Ok(PerpLogos::from_body(&crate::http::trade_get(net, PERP_LOGO_PATH)?))
    }

    pub fn get(&self, market_id: &str, symbol: Option<&str>) -> Option<&str> {
        self.by_id.get(&market_id.to_lowercase()).or_else(|| symbol.and_then(|s| self.by_sym.get(&s.to_uppercase()))).map(String::as_str)
    }
}

/// How many Trade reads are in flight, for the thin loading bar.
#[derive(Debug, Default)]
pub struct Loading(AtomicUsize);

impl Loading {
    /// Counts `f` as loading while it runs.
    pub fn track<T>(&self, f: impl FnOnce() -> T) -> T {
        self.0.fetch_add(1, Ordering::SeqCst);
        let out = f();
        self.0.fetch_sub(1, Ordering::SeqCst);
        out
    }
    pub fn busy(&self) -> bool {
        self.0.load(Ordering::SeqCst) > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rare_market::net::MockNet;
    use serde_json::json;

    #[test]
    fn logos_become_absolute() {
        assert_eq!(logo_url(Some("/coins/usdc.png")).as_deref(), Some("https://rare.market/v1/trade/asset/coins/usdc.png"));
        assert_eq!(logo_url(Some("//cdn.x/y.png")).as_deref(), Some("https://cdn.x/y.png"));
        assert_eq!(logo_url(Some("https://a/b.png")).as_deref(), Some("https://a/b.png"));
        assert_eq!(logo_url(None), None);
    }

    #[test]
    fn paths_use_long_encoded_types() {
        let sui = "0x0000000000000000000000000000000000000000000000000000000000000002%3A%3Asui%3A%3ASUI";
        assert_eq!(token_paths::token("0x2::sui::SUI"), format!("/token/{sui}"));
        assert_eq!(token_paths::candles("0x2::sui::SUI", "5m", 300), format!("/token/{sui}/candles?tf=5m&limit=300"));
        assert_eq!(token_paths::list(TokenList::Gainers, 6), "/tokens?list=gainers&limit=6");
    }

    #[test]
    fn reads_rows_and_perp_logos() {
        let n = MockNet::new();
        n.on_read_json("trade:/tokens?list=trending&limit=16", json!({"items": [{"coinType": "0x2::sui::SUI", "symbol": "SUI", "name": "Sui", "decimals": 9, "logo": null, "priceUsd": 3.1, "change": {"h24": 2.5}}]}));
        n.on_read_json("trade:/perps?l=1", json!({"items": [{"id": "0xAB", "logo": "https://l/btc.png", "symbol": "btc"}, {"id": "0xcd", "logo": null}]}));
        let rows = tokens(&n, TokenList::Trending, 16).unwrap();
        assert_eq!(rows[0].symbol, "SUI");
        assert_eq!(change24(&rows[0]), Some(2.5));
        let logos = PerpLogos::load(&n).unwrap();
        assert_eq!(logos.get("0xab", None), Some("https://l/btc.png"));
        assert_eq!(logos.get("0xzz", Some("BTC")), Some("https://l/btc.png"));
        assert_eq!(logos.get("0xcd", None), None);
    }
}
