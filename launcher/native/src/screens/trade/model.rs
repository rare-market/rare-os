//! What the Trade screens show, as the trade API answers it (trade/api.ts, trade/perps.ts types,
//! camelCase JSON). The screens read these; fixtures are real answers captured from the web
//! launcher, so the native and web screens show the same data. When rare-trade lands these become
//! its types (or `From` its types).

use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PerpMarket {
    pub market_id: String,
    pub symbol: String,
    pub pair: String,
    pub price: f64,
    pub change_pct: Option<f64>,
    pub volume_usd: Option<f64>,
    pub max_leverage: Option<f64>,
    pub funding_rate: Option<f64>,
    pub next_funding_ms: Option<f64>,
    pub open_interest_usd: Option<f64>,
    pub taker_fee: Option<f64>,
    pub min_order_usd: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PerpSide {
    #[default]
    Long,
    Short,
}

impl PerpSide {
    pub fn word(self) -> &'static str {
        match self {
            PerpSide::Long => "Long",
            PerpSide::Short => "Short",
        }
    }
    pub fn lower(self) -> &'static str {
        match self {
            PerpSide::Long => "long",
            PerpSide::Short => "short",
        }
    }
    pub fn flip(self) -> PerpSide {
        match self {
            PerpSide::Long => PerpSide::Short,
            PerpSide::Short => PerpSide::Long,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PerpPosition {
    pub market_id: String,
    pub symbol: String,
    pub side: PerpSide,
    pub size: f64,
    pub notional_usd: f64,
    pub margin_usd: f64,
    pub leverage: f64,
    pub entry_price: f64,
    pub mark_price: Option<f64>,
    pub liquidation_price: Option<f64>,
    pub pnl_usd: f64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PerpsAccount {
    pub account_id: String,
    pub equity_usd: f64,
    pub available_usd: f64,
    pub unrealized_pnl_usd: f64,
    pub positions: Vec<PerpPosition>,
    pub builder_approved: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct OrderPreview {
    pub size: String,
    pub size_units: f64,
    pub entry_price: f64,
    pub liquidation_price: Option<f64>,
    pub notional_usd: f64,
    pub margin_usd: f64,
    pub fee_usd: f64,
    pub builder_fee_usd: f64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct PerpBar {
    /// Milliseconds.
    pub time: f64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Items<T> {
    pub items: Vec<T>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct PerpLogo {
    pub id: String,
    pub symbol: Option<String>,
    pub logo: Option<String>,
}

/// usePerpLogos: market picture by id, then by symbol.
pub fn perp_logo<'a>(logos: &'a [PerpLogo], market_id: &str, symbol: &str) -> Option<&'a str> {
    let id = market_id.to_ascii_lowercase();
    logos
        .iter()
        .find(|l| l.id.to_ascii_lowercase() == id && l.logo.is_some())
        .or_else(|| logos.iter().find(|l| l.symbol.as_deref().map(|s| s.eq_ignore_ascii_case(symbol)).unwrap_or(false) && l.logo.is_some()))
        .and_then(|l| l.logo.as_deref())
}

/// trade/api.ts `logoUrl`: protocol-relative and site-relative logos made absolute.
pub fn logo_url(u: Option<&str>) -> Option<String> {
    let u = u?.trim();
    if u.is_empty() {
        return None;
    }
    Some(if let Some(rest) = u.strip_prefix("//") {
        format!("https://{rest}")
    } else if u.starts_with('/') {
        format!("{}{u}", rare_market::net::TRADE_ASSETS)
    } else {
        u.to_string()
    })
}

/// A token's price changes over windows, in percent (the Trade API's `change`).
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Change {
    pub m5: Option<f64>,
    pub h1: Option<f64>,
    pub h6: Option<f64>,
    pub h24: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
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

impl TokenRow {
    /// api.ts `change24`.
    pub fn change24(&self) -> Option<f64> {
        self.change.and_then(|c| c.h24).or(self.change24h)
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Holding {
    pub coin_type: String,
    pub symbol: String,
    pub name: String,
    pub decimals: u32,
    pub logo: Option<String>,
    pub price_usd: Option<f64>,
    pub change24h: Option<f64>,
    pub amount: f64,
    pub value_usd: f64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Portfolio {
    pub total_usd: f64,
    pub change24h_usd: f64,
    pub change24h_pct: f64,
    pub holdings: Vec<Holding>,
}

/// The perps page fixture: the answers the web page read, plus the wallet's USDC (read from chain).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerpFixture {
    pub now: f64,
    pub market_id: String,
    pub markets: Items<PerpMarket>,
    pub logos: Items<PerpLogo>,
    pub bars: Vec<PerpBar>,
    pub account: AccountAnswer,
    pub preview: Option<OrderPreview>,
    pub usdc: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AccountAnswer {
    pub account: Option<PerpsAccount>,
}

/// A trade on a coin's page (api.ts Trade).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct TradeRow {
    pub id: String,
    pub time_ms: f64,
    pub buy: bool,
    pub usd: f64,
    pub amount: f64,
    pub price_usd: f64,
    pub trader: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Holder {
    pub address: String,
    pub amount: f64,
    pub value_usd: Option<f64>,
    pub pct_supply: Option<f64>,
    pub kind: String,
    pub venue: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct HoldersPage {
    pub count: Option<f64>,
    pub holders: Vec<Holder>,
    pub top10_pct: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct CandleRow {
    /// Seconds.
    pub time: f64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct QuoteAnswer {
    pub amount_in: String,
    pub amount_out: String,
    pub fee_bps: u32,
    pub hops: u32,
    pub venues: Vec<String>,
    pub expires_at: f64,
}

/// A coin page's answers: the token, SUI's lean row, candles, trades, the live quote for the
/// default ticket and the wallet's balances (base units, from chain).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenFixture {
    pub now: f64,
    pub coin_type: String,
    pub token: TokenRow,
    pub sui: TokenRow,
    pub candles: Vec<CandleRow>,
    pub trades: Items<TradeRow>,
    #[serde(default)]
    pub holders: Option<HoldersPage>,
    pub quote: Option<QuoteAnswer>,
    pub balances: std::collections::HashMap<String, String>,
}

pub fn token_deep() -> TokenFixture {
    serde_json::from_str(include_str!("fixtures/token-deep.json")).expect("token-deep fixture")
}

/// The Trade root's answers: token lists, perps markets and logos, the wallet's portfolio and
/// perps account.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootFixture {
    pub now: f64,
    pub trending: Items<TokenRow>,
    pub gainers: Items<TokenRow>,
    pub fresh: Items<TokenRow>,
    pub markets: Items<PerpMarket>,
    pub logos: Items<PerpLogo>,
    pub portfolio: Option<Portfolio>,
    pub account: AccountAnswer,
}

pub fn trade_root() -> RootFixture {
    serde_json::from_str(include_str!("fixtures/trade-root.json")).expect("trade-root fixture")
}

pub fn perp_btc() -> PerpFixture {
    serde_json::from_str(include_str!("fixtures/perp-btc.json")).expect("perp-btc fixture")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_parses() {
        let f = perp_btc();
        let m = f.markets.items.iter().find(|m| m.market_id == f.market_id).unwrap();
        assert_eq!(m.symbol, "BTC");
        assert!(f.bars.len() > 100);
        assert!(f.account.account.is_none());
        assert!(perp_logo(&f.logos.items, &f.market_id, "BTC").unwrap().ends_with("btc.svg"));
        assert_eq!(logo_url(Some("//x.y/z.png")).unwrap(), "https://x.y/z.png");
        assert_eq!(logo_url(Some("/a.png")).unwrap(), "https://rare.market/v1/trade/asset/a.png");
        let r = trade_root();
        assert_eq!(r.trending.items[0].symbol, "DEEP");
        assert!(r.trending.items[0].change24().unwrap() > 2.0);
        assert_eq!(r.portfolio.unwrap().holdings.len(), 11);
        let t = token_deep();
        assert_eq!(t.token.symbol, "DEEP");
        assert_eq!(t.trades.items.len(), 30);
        assert_eq!(t.quote.unwrap().amount_out, "276426792");
    }
}
