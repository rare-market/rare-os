//! Rare Store shapes as rare.market's API sends them (store/types.ts, rare-store docs/OFFCHAIN.md):
//! amounts are strings in base units. The demo fixture is store/mock.ts's answers at one clock, so
//! the native screens and the web screenshots read the same games.

use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// u64 from a JSON string or number (the API sends amounts as strings).
fn de_u64<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(match v {
        Value::String(s) => s.trim().parse().unwrap_or(0),
        Value::Number(n) => n.as_u64().unwrap_or_else(|| n.as_f64().unwrap_or(0.0) as u64),
        _ => 0,
    })
}

fn de_opt_u64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    let v = Option::<Value>::deserialize(d)?;
    Ok(match v {
        Some(Value::String(s)) => s.trim().parse().ok(),
        Some(Value::Number(n)) => n.as_u64().or_else(|| n.as_f64().map(|f| f as u64)),
        _ => None,
    })
}

/// /v1/store/config, normalised (api.ts normalizeConfig). Every id is None until the package is deployed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StoreConfig {
    pub package: Option<String>,
    pub origin: Option<String>,
    pub store: Option<String>,
    pub store_version: Option<String>,
    pub policy: Option<String>,
    pub policy_version: Option<String>,
    pub usdc_type: Option<String>,
    pub uploads_open: bool,
}

/// Accepts snake_case, camelCase and the env names, flat or under `config`.
pub fn normalize_config(raw: &Value) -> StoreConfig {
    let o = raw.get("config").unwrap_or(raw);
    let pick = |keys: &[&str]| -> Option<String> {
        keys.iter().find_map(|k| match o.get(*k) {
            Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
            Some(Value::Number(n)) => Some(n.to_string()),
            _ => None,
        })
    };
    StoreConfig {
        package: pick(&["package", "package_id", "packageId", "RARE_STORE_PACKAGE"]),
        origin: pick(&["origin", "origin_id", "originId", "original_package", "RARE_STORE_ORIGIN"]),
        store: pick(&["store", "store_id", "storeId", "RARE_STORE_STORE"]),
        store_version: pick(&["store_version", "storeVersion", "store_initial_shared_version", "RARE_STORE_STORE_VERSION"]),
        policy: pick(&["policy", "policy_id", "policyId", "RARE_STORE_POLICY"]),
        policy_version: pick(&["policy_version", "policyVersion", "RARE_STORE_POLICY_VERSION"]),
        usdc_type: pick(&["usdc_type", "usdcType", "RARE_STORE_USDC_TYPE"]),
        uploads_open: o.get("uploads_open") == Some(&Value::Bool(true)) || o.get("uploadsOpen") == Some(&Value::Bool(true)),
    }
}

/// The store can sell: package, Store and USDC type are known. Until then there is no buy button.
pub fn can_sell(c: Option<&StoreConfig>) -> bool {
    c.is_some_and(|c| c.package.is_some() && c.store.is_some() && c.usdc_type.is_some())
}

pub fn can_resell(c: Option<&StoreConfig>) -> bool {
    can_sell(c) && c.is_some_and(|c| c.policy.is_some())
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct StorePrice {
    /// micro-USD
    #[serde(deserialize_with = "de_u64")]
    pub usd: u64,
    #[serde(default, deserialize_with = "de_opt_u64")]
    pub sale_usd: Option<u64>,
    #[serde(default)]
    pub sale_ends_ms: Option<f64>,
    #[serde(default)]
    pub accept_sui: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Studio {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Images {
    #[serde(default)]
    pub logo: Option<String>,
    #[serde(default)]
    pub banner: Option<String>,
    #[serde(default)]
    pub cover: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Links {
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub x: Option<String>,
    #[serde(default)]
    pub play: Option<String>,
    #[serde(default)]
    pub steam: Option<String>,
    #[serde(default)]
    pub epic: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Media {
    pub kind: String,
    pub url: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct GamePageInfo {
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub media: Vec<Media>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub requirements: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct ApiBuild {
    pub platform: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(deserialize_with = "de_u64")]
    pub size: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Latest {
    pub version: u64,
    #[serde(default)]
    pub label: String,
    pub edition: u64,
    #[serde(default)]
    pub published_ms: f64,
    #[serde(default)]
    pub builds: Vec<ApiBuild>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Resale {
    pub enabled: bool,
    pub royalty_bps: u64,
    #[serde(default)]
    pub min_hold_ms: f64,
    #[serde(default, deserialize_with = "de_u64")]
    pub min_royalty_mist: u64,
}

/// A store game with its page fields (StoreGameDetail; a shelf's StoreGame leaves them empty).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct StoreGame {
    pub slug: String,
    /// "link" | "uploaded"
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub studio: Studio,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub description: String,
    /// "live" | "beta"
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub images: Images,
    #[serde(default)]
    pub links: Links,
    #[serde(default)]
    pub price: Option<StorePrice>,
    #[serde(default)]
    pub game_id: Option<String>,
    #[serde(default)]
    pub featured: bool,
    #[serde(default)]
    pub sales: u64,
    #[serde(default)]
    pub created_ms: Option<f64>,
    // ---- StoreGameDetail
    #[serde(default)]
    pub page: Option<GamePageInfo>,
    #[serde(default)]
    pub latest: Option<Latest>,
    #[serde(default)]
    pub edition: Option<u64>,
    #[serde(default)]
    pub resale: Option<Resale>,
    #[serde(default)]
    pub launch_check: bool,
    #[serde(default)]
    pub listed: bool,
    #[serde(default)]
    pub blocked: bool,
}

impl StoreGame {
    pub fn is_link(&self) -> bool {
        self.kind == "link"
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Licence {
    pub id: String,
    /// "licence" | "resale"
    #[serde(rename = "type")]
    pub kind: String,
    pub game_id: String,
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub image_url: String,
    pub edition: u64,
    pub version: u64,
    #[serde(default)]
    pub purchased_ms: f64,
    pub number: u64,
    #[serde(default)]
    pub kiosk_id: Option<String>,
    #[serde(default)]
    pub refundable_until_ms: Option<f64>,
    #[serde(default)]
    pub tradable_from_ms: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct SuiPrice {
    #[serde(deserialize_with = "de_u64")]
    pub price_e8: u64,
    #[serde(default, deserialize_with = "de_u64")]
    pub conf_e8: u64,
    #[serde(default)]
    pub publish_time: f64,
    #[serde(default)]
    pub update_b64: String,
    #[serde(default)]
    pub fresh: bool,
}

/// One platform build as the chain records it (Game.editions[edition].builds).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChainBuild {
    pub platform: String,
    pub sha256: String,
    pub size: u64,
    pub path: String,
}

/// The release a licence may download, read from the chain (chain.ts releaseFor).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChainRelease {
    pub version: u64,
    pub label: String,
    pub edition: u64,
    pub builds: Vec<ChainBuild>,
    pub published_ms: f64,
}

impl ChainRelease {
    /// The demo chain's release for a game (mock.ts chainGame): the API's latest, builds under games/.
    pub fn from_latest(slug: &str, l: &Latest) -> ChainRelease {
        ChainRelease {
            version: l.version,
            label: l.label.clone(),
            edition: l.edition,
            published_ms: l.published_ms,
            builds: l.builds.iter().map(|b| ChainBuild { platform: b.platform.clone(), sha256: b.sha256.clone(), size: b.size, path: format!("games/{slug}/{}/{}.tar.zst", l.version, b.platform) }).collect(),
        }
    }
}

/// What the game page needs of an install (rare_store::Installed).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InstalledAt {
    pub edition: u64,
    pub version: u64,
}

/// Install progress (the Rust installer's events).
#[derive(Clone, Debug, PartialEq)]
pub enum Progress {
    Download { done: u64, total: u64 },
    Verify,
    Unpack,
}

/// The Rare Wallet account the store acts for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Account {
    #[default]
    None,
    /// Watch-only: cannot sign.
    Watch(String),
    Signer(String),
}

impl Account {
    pub fn can_sign(&self) -> bool {
        matches!(self, Account::Signer(_))
    }
}

/// The demo fixture: mock.ts's answers at `now` (UTC), with the three configs the screenshots use.
#[derive(Clone, Debug, Deserialize)]
pub struct Demo {
    pub now: f64,
    pub config_closed: Value,
    pub config_sell: Value,
    pub config_live: Value,
    pub shelves: Shelves,
    pub search: std::collections::HashMap<String, Vec<String>>,
    pub games: Vec<StoreGame>,
    pub licences: Vec<Licence>,
    pub sui_price: SuiPrice,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Shelves {
    pub featured: Vec<String>,
    pub new: Vec<String>,
    pub top: Vec<String>,
    pub sale: Vec<String>,
}

impl Demo {
    pub fn game(&self, slug: &str) -> Option<&StoreGame> {
        self.games.iter().find(|g| g.slug == slug)
    }
    pub fn list(&self, slugs: &[String]) -> Vec<StoreGame> {
        slugs.iter().filter_map(|s| self.game(s)).cloned().collect()
    }
}

pub fn demo() -> Demo {
    serde_json::from_str(include_str!("fixtures/demo.json")).expect("store demo fixture")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_reads() {
        let d = demo();
        assert_eq!(d.shelves.featured.first().map(String::as_str), Some("driftline"));
        let hk = d.game("hollow-keep").unwrap();
        assert_eq!(hk.price.as_ref().unwrap().sale_usd, Some(6_990_000));
        assert_eq!(hk.latest.as_ref().unwrap().builds[0].size, 734_003_200);
        assert!(d.game("act").unwrap().is_link());
        assert_eq!(d.licences[0].number, 413);
    }

    #[test]
    fn config_is_closed_until_deployed() {
        let c = normalize_config(&serde_json::json!({ "package": null, "store": null, "usdc_type": null, "uploads_open": false }));
        assert!(!can_sell(Some(&c)));
    }

    #[test]
    fn config_reads_snake_camel_and_env_names() {
        let a = normalize_config(&serde_json::json!({ "package": "0x1", "store": "0x2", "store_version": "7", "usdc_type": "0x3::usdc::USDC", "policy": "0x4" }));
        assert!(can_sell(Some(&a)) && can_resell(Some(&a)));
        assert_eq!(a.store_version.as_deref(), Some("7"));
        let b = normalize_config(&serde_json::json!({ "config": { "RARE_STORE_PACKAGE": "0x1", "RARE_STORE_STORE": "0x2", "RARE_STORE_USDC_TYPE": "0x3::usdc::USDC" } }));
        assert!(can_sell(Some(&b)));
        assert!(!can_resell(Some(&b)));
    }
}
