//! rare.market reads for the Sui features. Ports `src/sui/data.ts`.
//!
//! Every read goes through a [`MarketApi`]: in the native launcher the host's cached `rare_get`
//! (rare-state), else [`HttpMarket`] straight to rare.market (testnet aware). Shapes follow
//! market/api/src/routes; unknown fields are kept in `extra` so nothing is lost on the way to a
//! screen.

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::network::NetworkConfig;
use crate::{BoxFut, Error, Result};

/// GET a rare.market path (`/v1/status`) and answer its JSON.
pub trait MarketApi: Send + Sync {
    fn get(&self, path: &str) -> BoxFut<'_, Value>;
}

/// Typed GET.
pub async fn get_as<T: DeserializeOwned>(api: &dyn MarketApi, path: &str) -> Result<T> {
    Ok(serde_json::from_value(api.get(path).await?)?)
}

/// rare.market over HTTP: `/v1/...` on mainnet, the testnet API with `network=testnet` on testnet.
#[derive(Clone)]
pub struct HttpMarket {
    cfg: NetworkConfig,
    http: reqwest::Client,
}

impl HttpMarket {
    pub fn new(cfg: NetworkConfig) -> Self {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(8)).user_agent("RareLauncher/0.1").build().expect("http client");
        HttpMarket { cfg, http }
    }
}

impl MarketApi for HttpMarket {
    fn get(&self, path: &str) -> BoxFut<'_, Value> {
        let url = self.cfg.market_url(path);
        Box::pin(async move { get_json(&self.http, &url).await })
    }
}

/// GET JSON. On non-2xx the error carries the body's `error` text when it has one (the Trade API and
/// rare.market say what went wrong there), else an empty message.
pub(crate) async fn get_json(http: &reqwest::Client, url: &str) -> Result<Value> {
    let r = http.get(url).header("accept", "application/json").send().await?;
    let status = r.status();
    let text = r.text().await?;
    if !status.is_success() {
        let msg = serde_json::from_str::<Value>(&text).ok().and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string)).unwrap_or_default();
        return Err(Error::http(Some(status.as_u16()), msg));
    }
    Ok(serde_json::from_str(&text)?)
}

fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `encodeURIComponent`.
pub fn encode_component(s: &str) -> String {
    enc(s)
}

// ---------- shapes ----------

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RareStatus {
    pub sui_usd: f64,
    pub tps: f64,
    pub checkpoint: f64,
    pub degraded: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RareAccountInfo {
    pub address: String,
    /// What rare.market shows: the Rare username, else the SuiNS name.
    pub name: Option<String>,
    pub username: Option<String>,
    pub suins: Option<String>,
    pub avatar_url: Option<String>,
    /// The picture (its pick, else its default) and when it was picked.
    pub avatar: Option<AccountAvatar>,
    /// The profile banner (its pick, else a Rare default by address).
    pub banner: Option<AccountBanner>,
    pub total_items: f64,
    pub listed_count: f64,
}

/// rare.market's `AvatarRef`: a Rare default or an NFT; `at` is when it was picked (None: never,
/// the account's default).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountAvatar {
    pub kind: String,
    pub id: String,
    pub url: String,
    pub at: Option<f64>,
}

/// rare.market's `BannerRef`: a Rare default, an upload or an NFT, as our own 3:1 renditions
/// (`url` 1500 x 500, `url_small` 750 x 250).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountBanner {
    pub kind: String,
    pub id: String,
    pub url: String,
    pub url_small: String,
    pub at: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CollectionRef {
    pub slug: String,
    pub name: String,
    pub image_url: Option<String>,
    pub verified: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PortfolioCollection {
    pub collection: CollectionRef,
    pub items: f64,
    pub value_mist: String,
}

/// `/v1/accounts/<a>/portfolio`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MarketPortfolio {
    pub value_mist: String,
    pub cost_mist: String,
    pub unrealized_pnl_mist: String,
    pub pnl_pct: Option<f64>,
    pub items: f64,
    pub collections: Vec<PortfolioCollection>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Rarity {
    pub rank: f64,
    pub tier: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Listing {
    pub price_mist: String,
    pub all_in_mist: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One item as rare.market lists it (marketplace/api.ts `ItemCard`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ItemCard {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub image_url: Option<String>,
    pub collection: Option<CollectionRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rarity: Option<Rarity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing: Option<Listing>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kiosk_id: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    #[serde(default = "Vec::new")]
    pub data: Vec<T>,
    #[serde(default)]
    pub next_cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
}

impl<T> Default for Page<T> {
    fn default() -> Self {
        Page { data: vec![], next_cursor: None, total: None }
    }
}

/// One collection the account holds (kiosks included).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HeldCollection {
    pub slug: String,
    pub name: String,
    pub image_url: Option<String>,
    pub verified: bool,
    #[serde(rename = "type")]
    pub collection_type: String,
    pub count: f64,
    pub floor_mist: Option<String>,
    pub value_mist: Option<String>,
    pub items: Vec<ItemCard>,
}

/// A time as rare.market sends it: unix seconds or ms, or an ISO string.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum When {
    Num(f64),
    Text(String),
}

impl When {
    /// Unix ms (`ms` in data.ts: numbers below 1e12 are seconds).
    pub fn ms(&self) -> Option<f64> {
        match self {
            When::Num(v) => Some(if *v < 1e12 { v * 1000.0 } else { *v }),
            When::Text(s) => parse_iso_ms(s),
        }
    }
}

/// ISO 8601 (`2026-10-27T10:00:00Z`, `...+02:00`, fractional seconds) to unix ms.
pub fn parse_iso_ms(s: &str) -> Option<f64> {
    let s = s.trim();
    let (date, rest) = s.split_once(['T', ' ']).unwrap_or((s, ""));
    let mut d = date.split('-');
    let y: i64 = d.next()?.parse().ok()?;
    let mo: i64 = d.next()?.parse().ok()?;
    let da: i64 = d.next()?.parse().ok()?;
    let (time, off_ms) = if rest.is_empty() {
        ("00:00:00", 0i64)
    } else if let Some(t) = rest.strip_suffix('Z').or_else(|| rest.strip_suffix('z')) {
        (t, 0)
    } else if let Some(i) = rest.rfind(['+', '-']) {
        let (t, off) = rest.split_at(i);
        let sign = if off.starts_with('-') { -1 } else { 1 };
        let mut o = off[1..].split(':');
        let oh: i64 = o.next()?.parse().ok()?;
        let om: i64 = o.next().unwrap_or("0").parse().ok()?;
        (t, sign * (oh * 60 + om) * 60_000)
    } else {
        // no zone: Date.parse reads a date-time without one as local time; treat as UTC here
        (rest, 0)
    };
    let mut t = time.split(':');
    let h: i64 = t.next()?.parse().ok()?;
    let mi: i64 = t.next().unwrap_or("0").parse().ok()?;
    let sec: f64 = t.next().unwrap_or("0").parse().ok()?;
    // days from civil (Howard Hinnant)
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + da - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86_400_000 + h * 3_600_000 + mi * 60_000 - off_ms) as f64 + sec * 1000.0)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Phase {
    pub name: Option<String>,
    pub starts_at: Option<When>,
    pub ends_at: Option<When>,
    pub price_mist: Option<String>,
}

/// One launchpad launch, as /launchpad/launches.json lists it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Launch {
    pub slug: String,
    pub name: String,
    pub image_url: Option<String>,
    pub minted: Option<f64>,
    pub supply: Option<f64>,
    pub starts_at: Option<When>,
    pub ends_at: Option<When>,
    pub phases: Option<Vec<Phase>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MintItem {
    pub id: String,
    pub name: String,
    pub image_url: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MintEvent {
    pub id: String,
    pub item: Option<MintItem>,
    pub collection: Option<CollectionRef>,
    pub price_mist: Option<String>,
    pub at: String,
}

/// marketplace/api.ts `MarketEvent` (sales on Home and the shell).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MarketEvent {
    pub id: String,
    pub kind: String,
    pub venue: Option<String>,
    pub item: Option<Value>,
    pub collection: Option<CollectionRef>,
    pub price_mist: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub from_name: Option<String>,
    pub to_name: Option<String>,
    pub at: String,
}

/// marketplace/api.ts `CollectionRow` (top collections).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CollectionRow {
    pub slug: String,
    pub name: String,
    pub image_url: Option<String>,
    pub verified: bool,
    pub rank: f64,
    pub supply: f64,
    pub floor_mist: Option<String>,
    pub floor_change_pct: Option<f64>,
    pub top_offer_mist: Option<String>,
    pub volume_mist: String,
    pub volume_change_pct: Option<f64>,
    pub sales: f64,
    pub listed_count: f64,
    pub owners: f64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

// ---------- reads (`rareSui` in data.ts) ----------

pub async fn status(api: &dyn MarketApi) -> Result<RareStatus> {
    get_as(api, "/v1/status").await
}

pub async fn account(api: &dyn MarketApi, a: &str) -> Result<RareAccountInfo> {
    get_as(api, &format!("/v1/accounts/{}", enc(a))).await
}

pub async fn portfolio(api: &dyn MarketApi, a: &str) -> Result<MarketPortfolio> {
    get_as(api, &format!("/v1/accounts/{}/portfolio?limit=1", enc(a))).await
}

/// Every collection the address holds (kiosks included), all type classes so game items count.
pub async fn collections(api: &dyn MarketApi, a: &str) -> Result<Vec<HeldCollection>> {
    let p: Page<HeldCollection> = get_as(api, &format!("/v1/accounts/{}/collections?class=all", enc(a))).await?;
    Ok(p.data)
}

pub async fn items(api: &dyn MarketApi, a: &str, limit: u32, class: &str) -> Result<Page<ItemCard>> {
    get_as(api, &format!("/v1/accounts/{}/items?limit={limit}&class={class}", enc(a))).await
}

pub async fn launches(api: &dyn MarketApi) -> Result<Vec<Launch>> {
    let v = api.get("/launchpad/launches.json").await?;
    Ok(match v {
        Value::Array(a) => a.into_iter().filter_map(|x| serde_json::from_value(x).ok()).collect(),
        _ => vec![],
    })
}

pub async fn drops(api: &dyn MarketApi) -> Result<Page<Launch>> {
    get_as(api, "/v1/drops").await
}

pub async fn mints(api: &dyn MarketApi) -> Result<Page<MintEvent>> {
    get_as(api, "/v1/events?kind=mint&limit=12").await
}

/// Launches minting now (inside a phase or the launch window, and not sold out).
pub fn live_launches(list: &[Launch], now_ms: f64) -> Vec<Launch> {
    list.iter()
        .filter(|l| {
            if let (Some(s), Some(m)) = (l.supply, l.minted) {
                if s != 0.0 && m >= s {
                    return false;
                }
            }
            let ms = |w: &Option<When>| w.as_ref().and_then(When::ms);
            let windows: Vec<(Option<f64>, Option<f64>)> = match &l.phases {
                Some(p) if !p.is_empty() => p.iter().map(|p| (ms(&p.starts_at), ms(&p.ends_at))).collect(),
                _ => vec![(ms(&l.starts_at), ms(&l.ends_at))],
            };
            windows.iter().any(|(s, e)| s.is_none_or(|s| s <= now_ms) && e.is_none_or(|e| now_ms < e) && (s.is_some() || e.is_some()))
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // live.test.ts "liveLaunches"
    #[test]
    fn keeps_launches_inside_a_phase_window_and_not_sold_out() {
        let now = 1_800_000_000_000f64;
        let iso = {
            // new Date(now - 1000).toISOString()
            let ms = now as i64 - 1000;
            let secs = ms / 1000;
            let days = secs / 86_400;
            let rem = secs % 86_400;
            // civil from days
            let z = days + 719_468;
            let era = z / 146_097;
            let doe = z - era * 146_097;
            let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = doy - (153 * mp + 2) / 5 + 1;
            let m = if mp < 10 { mp + 3 } else { mp - 9 };
            let y = yoe + era * 400 + i64::from(m <= 2);
            format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem % 3600 / 60, rem % 60, ms % 1000)
        };
        let list: Vec<Launch> = serde_json::from_value(json!([
            { "slug": "a", "name": "A", "phases": [{ "startsAt": now - 1000.0, "endsAt": now + 1000.0 }] },
            { "slug": "b", "name": "B", "phases": [{ "startsAt": now + 1000.0, "endsAt": now + 2000.0 }] },
            { "slug": "c", "name": "C", "minted": 10, "supply": 10, "startsAt": now - 1000.0 },
            { "slug": "d", "name": "D", "startsAt": iso },
            { "slug": "e", "name": "E" },
        ]))
        .unwrap();
        let live: Vec<String> = live_launches(&list, now).into_iter().map(|l| l.slug).collect();
        assert_eq!(live, ["a", "d"]);
    }

    #[test]
    fn seconds_and_iso_times() {
        assert_eq!(When::Num(1_700_000_000.0).ms(), Some(1_700_000_000_000.0));
        assert_eq!(parse_iso_ms("1970-01-02T00:00:00Z"), Some(86_400_000.0));
        assert_eq!(parse_iso_ms("2026-10-27T12:00:00+02:00"), parse_iso_ms("2026-10-27T10:00:00Z"));
        assert_eq!(parse_iso_ms("nope"), None);
    }

    #[test]
    fn shapes_parse_and_keep_extras() {
        let c: ItemCard = serde_json::from_value(json!({ "id": "0x1", "name": "N", "type": "0x2::a::B", "imageUrl": null, "collection": null, "staked": { "protocol": "x" } })).unwrap();
        assert_eq!(c.item_type, "0x2::a::B");
        assert!(c.extra.contains_key("staked"));
        let back = serde_json::to_value(&c).unwrap();
        assert_eq!(back["staked"]["protocol"], "x");
        assert_eq!(encode_component("0x1/ a"), "0x1%2F%20a");
    }
}
