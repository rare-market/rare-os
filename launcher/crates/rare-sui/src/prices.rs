//! the Trade API's public extension API (`/ext/*`): prices, 24 h change, token safety, portfolios and
//! perp candles. Ports `src/wallet/data/prices.ts` and the Trade API half of `src/lib/live.ts`.
//! Everything here is optional: callers carry on without prices when the Trade API is down.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ids::short_key;
use crate::json::finite;
use crate::market::encode_component;
use crate::{BoxFut, Result};

pub const TRADE_URL: &str = "https://rare.market/v1/trade";
/// the Trade API's SUI perp market: the hourly line behind the SUI price.
pub const SUI_PERP: &str = "0xa97dacdd972a0414363290ec5752cb93ac47443c3e7710df145cc7f337575002";

/// GET a URL's JSON (fakes in tests, [`ReqwestGet`] in the app).
pub trait HttpGet: Send + Sync {
    fn get_json(&self, url: &str) -> BoxFut<'_, Value>;
}

pub struct ReqwestGet(pub reqwest::Client);

impl Default for ReqwestGet {
    fn default() -> Self {
        ReqwestGet(reqwest::Client::builder().timeout(Duration::from_secs(8)).user_agent("RareLauncher/0.1").build().expect("http client"))
    }
}

impl HttpGet for ReqwestGet {
    fn get_json(&self, url: &str) -> BoxFut<'_, Value> {
        let url = url.to_string();
        Box::pin(async move { crate::market::get_json(&self.0, &url).await })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenMeta {
    pub coin_type: String,
    pub symbol: String,
    pub name: String,
    pub decimals: u32,
    pub logo: Option<String>,
    pub price_usd: Option<f64>,
    /// Percent over 24 h.
    pub change24h: Option<f64>,
    pub verified: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenSafety {
    pub spam: bool,
    pub honeypot: bool,
}

/// `/ext/token/<type>`: meta plus market and safety detail.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenDetail {
    #[serde(flatten)]
    pub meta: TokenMeta,
    pub mcap: Option<f64>,
    pub volume24h: Option<f64>,
    pub liquidity_usd: Option<f64>,
    pub holders: Option<f64>,
    pub safety: Option<TokenSafety>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeHolding {
    #[serde(flatten)]
    pub meta: TokenMeta,
    /// Whole tokens.
    pub amount: f64,
    pub base_units: String,
    pub value_usd: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradePortfolio {
    pub address: String,
    pub total_usd: f64,
    pub change24h_usd: f64,
    pub holdings: Vec<TradeHolding>,
}

/// One candle close (`{ time, close }` in the API; `LinePoint` in the UI).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinePoint {
    pub t: f64,
    pub v: f64,
}

/// Relative logo paths resolve against the Trade API origin.
fn logo_url(base: &str, logo: Option<&Value>) -> Option<String> {
    let l = logo?.as_str().filter(|s| !s.is_empty())?;
    if l.starts_with("http:") || l.starts_with("https:") || l.starts_with("data:") || l.starts_with("ipfs:") {
        return Some(l.to_string());
    }
    if let Some(rest) = l.strip_prefix("//") {
        return Some(format!("https://{rest}"));
    }
    Some(format!("{base}{}{l}", if l.starts_with('/') { "" } else { "/" }))
}

pub struct PriceClient {
    pub base_url: String,
    http: Arc<dyn HttpGet>,
    cache: Mutex<HashMap<String, TokenMeta>>,
}

impl Default for PriceClient {
    fn default() -> Self {
        PriceClient::new(TRADE_URL, Arc::new(ReqwestGet::default()))
    }
}

impl PriceClient {
    pub fn new(base_url: &str, http: Arc<dyn HttpGet>) -> Self {
        PriceClient { base_url: base_url.trim_end_matches('/').to_string(), http, cache: Mutex::new(HashMap::new()) }
    }

    pub async fn get_json(&self, path: &str) -> Result<Value> {
        self.http.get_json(&format!("{}{path}", self.base_url)).await
    }

    fn to_meta(&self, raw: &Value) -> TokenMeta {
        let coin_type = match raw.get("coinType") {
            Some(Value::String(s)) => s.clone(),
            Some(v) if !v.is_null() => v.to_string(),
            _ => String::new(),
        };
        let symbol = raw.get("symbol").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| coin_type.rsplit("::").next().unwrap_or("?").to_string());
        TokenMeta {
            symbol,
            name: raw.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
            decimals: finite(raw.get("decimals")).map(|d| d as u32).unwrap_or(9),
            logo: logo_url(&self.base_url, raw.get("logo")),
            price_usd: finite(raw.get("priceUsd")),
            change24h: finite(raw.get("change24h")),
            verified: raw.get("verified") == Some(&Value::Bool(true)),
            coin_type,
        }
    }

    fn remember(&self, key: &str, m: &TokenMeta) {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).insert(short_key(key), m.clone());
    }

    /// GET /ext/portfolio/<address>. Seeds the meta cache.
    pub async fn portfolio(&self, address: &str) -> Result<TradePortfolio> {
        let j = self.get_json(&format!("/ext/portfolio/{}", encode_component(address))).await?;
        let rows = j.get("holdings").and_then(Value::as_array).cloned().unwrap_or_default();
        let holdings = rows
            .iter()
            .map(|h| {
                let meta = self.to_meta(h);
                self.remember(&meta.coin_type, &meta);
                let base_units = h.get("baseUnits").and_then(Value::as_str).filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).unwrap_or("0").to_string();
                TradeHolding { amount: finite(h.get("amount")).unwrap_or(0.0), base_units, value_usd: finite(h.get("valueUsd")), meta }
            })
            .collect();
        Ok(TradePortfolio { address: address.to_string(), total_usd: finite(j.get("totalUsd")).unwrap_or(0.0), change24h_usd: finite(j.get("change24hUsd")).unwrap_or(0.0), holdings })
    }

    /// GET /ext/token/<type>: full detail incl. safety. None when not indexed or the Trade API is down.
    pub async fn token_detail(&self, coin_type: &str) -> Option<TokenDetail> {
        let j = self.get_json(&format!("/ext/token/{}", encode_component(coin_type))).await.ok()?;
        let mut raw = j.clone();
        if raw.get("coinType").is_none_or(Value::is_null) {
            if let Value::Object(o) = &mut raw {
                o.insert("coinType".into(), Value::String(coin_type.to_string()));
            }
        }
        let meta = self.to_meta(&raw);
        self.remember(coin_type, &meta);
        let safety = j.get("safety").and_then(Value::as_object).map(|s| TokenSafety { spam: s.get("spam") == Some(&Value::Bool(true)), honeypot: s.get("honeypot") == Some(&Value::Bool(true)) });
        Some(TokenDetail { meta, mcap: finite(j.get("mcap")), volume24h: finite(j.get("volume24h")), liquidity_usd: finite(j.get("liquidityUsd")), holders: finite(j.get("holders")), safety })
    }

    /// Cached meta, else /ext/token/<type>.
    pub async fn token_meta(&self, coin_type: &str) -> Option<TokenMeta> {
        if let Some(m) = self.cached(coin_type) {
            return Some(m);
        }
        self.token_detail(coin_type).await.map(|d| d.meta)
    }

    /// Batch token_meta; unknown coins are left out. Keys are the input coin types.
    pub async fn tokens(&self, coin_types: &[String]) -> HashMap<String, TokenMeta> {
        let mut seen = std::collections::HashSet::new();
        let uniq: Vec<&String> = coin_types.iter().filter(|t| seen.insert(t.as_str())).collect();
        let got = futures::future::join_all(uniq.iter().map(|t| async move { (t.to_string(), self.token_meta(t).await) })).await;
        got.into_iter().filter_map(|(t, m)| m.map(|m| (t, m))).collect()
    }

    /// Cache-only lookup, for synchronous renders.
    pub fn cached(&self, coin_type: &str) -> Option<TokenMeta> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).get(&short_key(coin_type)).cloned()
    }

    /// Hourly (resolution in minutes) closes of a perp market between two unix ms.
    pub async fn candles(&self, market: &str, resolution: u32, from_ms: f64, to_ms: f64) -> Result<Vec<LinePoint>> {
        let j = self.get_json(&format!("/ext/perps/candles?market={market}&resolution={resolution}&from={}&to={}", from_ms as i64, to_ms as i64)).await?;
        Ok(j.get("bars")
            .and_then(Value::as_array)
            .map(|b| b.iter().filter_map(|b| Some(LinePoint { t: finite(b.get("time"))?, v: finite(b.get("close"))? })).collect())
            .unwrap_or_default())
    }
}

/// Plain warning for a token's safety flags, or None when clean or unknown.
pub fn safety_warning(s: Option<&TokenSafety>) -> Option<&'static str> {
    let s = s?;
    if s.honeypot {
        return Some("Possible honeypot. You may not be able to sell this token.");
    }
    if s.spam {
        return Some("Flagged as spam.");
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::Error;
    use serde_json::json;

    /// Answers every GET with one body (or an HTTP error), counting calls.
    pub struct FakeHttp {
        pub body: Result<Value>,
        pub calls: Mutex<Vec<String>>,
    }
    impl FakeHttp {
        pub fn ok(v: Value) -> Arc<Self> {
            Arc::new(FakeHttp { body: Ok(v), calls: Mutex::new(vec![]) })
        }
        pub fn fail(status: u16) -> Arc<Self> {
            Arc::new(FakeHttp { body: Err(Error::http(Some(status), "down")), calls: Mutex::new(vec![]) })
        }
    }
    impl HttpGet for FakeHttp {
        fn get_json(&self, url: &str) -> BoxFut<'_, Value> {
            self.calls.lock().unwrap().push(url.to_string());
            let b = self.body.clone();
            Box::pin(async move { b })
        }
    }

    #[tokio::test]
    async fn portfolio_resolves_logos_and_seeds_the_cache() {
        let y = PriceClient::new("https://y.test/", FakeHttp::ok(json!({ "totalUsd": 5, "holdings": [{ "coinType": "0x0002::sui::SUI", "symbol": "SUI", "logo": "/l/sui.png", "priceUsd": 2, "amount": 2.5, "baseUnits": "2500000000" }] })));
        let p = y.portfolio("0x1").await.unwrap();
        assert_eq!(p.total_usd, 5.0);
        assert_eq!(p.change24h_usd, 0.0);
        assert_eq!(p.holdings[0].meta.logo.as_deref(), Some("https://y.test/l/sui.png"));
        assert_eq!(p.holdings[0].base_units, "2500000000");
        assert_eq!(y.cached("0x2::sui::SUI").unwrap().price_usd, Some(2.0));
    }

    #[tokio::test]
    async fn token_detail_reads_safety_and_is_none_when_down() {
        let y = PriceClient::new("https://y.test", FakeHttp::ok(json!({ "symbol": "", "priceUsd": 1, "safety": { "honeypot": true } })));
        let d = y.token_detail("0xab::coin::COIN").await.unwrap();
        assert_eq!(d.meta.symbol, "COIN");
        assert_eq!(d.meta.coin_type, "0xab::coin::COIN");
        assert_eq!(safety_warning(d.safety.as_ref()), Some("Possible honeypot. You may not be able to sell this token."));
        let down = PriceClient::new("https://y.test", FakeHttp::fail(502));
        assert!(down.token_detail("0xab::coin::COIN").await.is_none());
        assert_eq!(safety_warning(None), None);
        assert_eq!(safety_warning(Some(&TokenSafety { spam: true, honeypot: false })), Some("Flagged as spam."));
    }

    #[test]
    fn logos() {
        assert_eq!(logo_url("https://y", Some(&json!("//cdn/x.png"))).as_deref(), Some("https://cdn/x.png"));
        assert_eq!(logo_url("https://y", Some(&json!("x.png"))).as_deref(), Some("https://y/x.png"));
        assert_eq!(logo_url("https://y", Some(&json!("ipfs://q"))).as_deref(), Some("ipfs://q"));
        assert_eq!(logo_url("https://y", Some(&json!(""))), None);
    }

    #[tokio::test]
    async fn candles_to_line() {
        let y = PriceClient::new("https://y.test", FakeHttp::ok(json!({ "bars": [{ "time": 1, "close": 2.5 }, { "time": 2, "close": 2.6 }] })));
        let l = y.candles(SUI_PERP, 60, 0.0, 10.0).await.unwrap();
        assert_eq!(l, vec![LinePoint { t: 1.0, v: 2.5 }, LinePoint { t: 2.0, v: 2.6 }]);
    }
}
