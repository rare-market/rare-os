//! Which Sui network the launcher reads (Settings > Developer). Mainnet by default.
//!
//! Ports `src/lib/network.ts` (the Sui RPC the reads use) and the URL half of
//! `src-tauri/src/network.rs` (where rare.market and the gas sponsor answer on testnet), so the
//! native UI has one place for the switch. The deployed package ids come from the same JSON files
//! the web launcher bundles (`src/trophies/deployment.json`, `src/localapi/items.json`).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    #[default]
    Mainnet,
    Testnet,
}

impl Network {
    pub fn as_str(self) -> &'static str {
        match self {
            Network::Mainnet => "mainnet",
            Network::Testnet => "testnet",
        }
    }
    /// The public fullnode's gRPC endpoint.
    pub fn default_rpc(self) -> &'static str {
        match self {
            Network::Mainnet => DEFAULT_RPC_MAINNET,
            Network::Testnet => DEFAULT_RPC_TESTNET,
        }
    }
}

pub const DEFAULT_RPC_MAINNET: &str = "https://fullnode.mainnet.sui.io:443";
pub const DEFAULT_RPC_TESTNET: &str = "https://fullnode.testnet.sui.io:443";
pub const MARKET: &str = "https://rare.market";

/// The launcher config's network fields (`config.json`: `network`, `testnetRpc`, `testnetApi`,
/// `testnetSponsor`), as Settings > Developer writes them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NetworkConfig {
    pub network: Option<String>,
    pub testnet_rpc: Option<String>,
    pub testnet_api: Option<String>,
    pub testnet_sponsor: Option<String>,
}

/// https anywhere; plain http only on this machine (a local node or API under test).
pub fn url_ok(u: &str) -> bool {
    match reqwest::Url::parse(u) {
        Ok(p) => match p.scheme() {
            "https" => true,
            "http" => matches!(p.host_str(), Some("localhost") | Some("127.0.0.1") | Some("[::1]")),
            _ => false,
        },
        Err(_) => false,
    }
}

/// `rpcOk` in network.ts.
pub fn rpc_ok(u: &str) -> bool {
    url_ok(u)
}

fn base_ok(b: &str) -> bool {
    url_ok(b) && reqwest::Url::parse(b).map(|u| u.query().is_none() && u.fragment().is_none()).unwrap_or(false)
}

fn trimmed(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(|s| s.trim().trim_end_matches('/')).filter(|s| !s.is_empty())
}

impl NetworkConfig {
    pub fn mainnet() -> Self {
        Self::default()
    }
    pub fn testnet() -> Self {
        Self { network: Some("testnet".into()), ..Self::default() }
    }

    /// The network the launcher is on.
    pub fn current(&self) -> Network {
        if self.network.as_deref() == Some("testnet") {
            Network::Testnet
        } else {
            Network::Mainnet
        }
    }

    pub fn is_testnet(&self) -> bool {
        self.current() == Network::Testnet
    }

    /// The gRPC endpoint for a network (the testnet override applies to testnet only).
    pub fn rpc_url(&self, network: Network) -> String {
        if network == Network::Testnet {
            if let Some(r) = trimmed(&self.testnet_rpc) {
                if rpc_ok(r) {
                    return r.to_string();
                }
            }
        }
        network.default_rpc().to_string()
    }

    fn base(over: &Option<String>) -> &str {
        match trimmed(over) {
            Some(b) if base_ok(b) => b,
            _ => MARKET,
        }
    }

    /// Where a rare.market read goes: rare.market on mainnet; the testnet API (same host unless
    /// overridden) with `network=testnet` on testnet.
    pub fn market_url(&self, path: &str) -> String {
        if self.is_testnet() {
            with_network(Self::base(&self.testnet_api), path)
        } else {
            format!("{MARKET}{path}")
        }
    }

    /// Where a gas sponsor call goes: the testnet sponsor if set, else the testnet API base.
    pub fn sponsor_url(&self, path: &str) -> String {
        if self.is_testnet() {
            let b = if self.testnet_sponsor.is_some() { Self::base(&self.testnet_sponsor) } else { Self::base(&self.testnet_api) };
            with_network(b, path)
        } else {
            format!("{MARKET}{path}")
        }
    }
}

fn with_network(base: &str, path: &str) -> String {
    let mut url = format!("{base}{path}");
    if !url.contains("network=") {
        url.push(if url.contains('?') { '&' } else { '?' });
        url.push_str("network=testnet");
    }
    url
}

// ---------- deployed packages ----------

#[derive(Clone, Debug, Default, Deserialize)]
struct PerNet<T> {
    mainnet: T,
    testnet: T,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct TrophyEntry {
    #[serde(default)]
    package: String,
    #[serde(default)]
    registry: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct ItemsEntry {
    #[serde(default)]
    package: String,
}

const TROPHIES_JSON: &str = include_str!("../../../src/trophies/deployment.json");
const ITEMS_JSON: &str = include_str!("../../../src/localapi/items.json");

/// Where Rare Trophies lives on chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrophyConfig {
    pub network: Network,
    pub package: String,
    pub registry: String,
}

fn trophies() -> PerNet<TrophyEntry> {
    serde_json::from_str(TROPHIES_JSON).expect("trophies/deployment.json")
}

/// Mainnet when it is deployed there, else testnet, else None (claiming not open yet).
/// `trophyConfig` in trophies/config.ts.
pub fn trophy_config() -> Option<TrophyConfig> {
    let d = trophies();
    for (network, e) in [(Network::Mainnet, &d.mainnet), (Network::Testnet, &d.testnet)] {
        if !e.package.is_empty() && !e.registry.is_empty() {
            return Some(TrophyConfig { network, package: e.package.clone(), registry: e.registry.clone() });
        }
    }
    None
}

/// The trophy deployment on one network, if any.
pub fn trophy_deployment(network: Network) -> Option<TrophyConfig> {
    let d = trophies();
    let e = if network == Network::Mainnet { d.mainnet } else { d.testnet };
    (!e.package.is_empty() && !e.registry.is_empty()).then_some(TrophyConfig { network, package: e.package, registry: e.registry })
}

pub fn trophy_type(pkg: &str) -> String {
    format!("{pkg}::trophy::Trophy")
}

/// Where the Rare Device Pass lives on a network (market contracts/device/deployment.json), None
/// until it is published there. The package's types are `{package}::device_pass::DevicePass`.
pub fn device_pass_deployment(network: Network) -> Option<TrophyConfig> {
    let d: PerNet<TrophyEntry> = serde_json::from_str(include_str!("device_pass.json")).expect("device_pass.json");
    let e = if network == Network::Mainnet { d.mainnet } else { d.testnet };
    (!e.package.is_empty() && !e.registry.is_empty()).then_some(TrophyConfig { network, package: e.package, registry: e.registry })
}

/// The rare_items package on a network (`itemsPackage` in localapi/voucher.ts), None until deployed.
pub fn items_package(network: Network) -> Option<String> {
    let d: PerNet<ItemsEntry> = serde_json::from_str(ITEMS_JSON).expect("localapi/items.json");
    let p = if network == Network::Mainnet { d.mainnet.package } else { d.testnet.package };
    (!p.is_empty()).then_some(p)
}

/// The Rare Store's package (the one its types were first published at, rare.market's
/// /v1/store/config `origin`). None until it is published: every licence check then answers
/// "unavailable" (licence.ts `RARE_STORE`).
pub const RARE_STORE_PACKAGE: Option<&str> = None;
pub const RARE_STORE_NETWORK: Network = Network::Mainnet;

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(j: &str) -> NetworkConfig {
        serde_json::from_str(j).unwrap()
    }

    // network.test.ts
    #[test]
    fn is_mainnet_until_set() {
        let c = cfg("{}");
        assert_eq!(c.current(), Network::Mainnet);
        assert_eq!(c.rpc_url(Network::Mainnet), DEFAULT_RPC_MAINNET);
    }

    #[test]
    fn switches_to_testnet_with_the_public_rpc() {
        let c = cfg(r#"{"network":"testnet"}"#);
        assert_eq!(c.current(), Network::Testnet);
        assert_eq!(c.rpc_url(Network::Testnet), DEFAULT_RPC_TESTNET);
    }

    #[test]
    fn takes_a_testnet_rpc_override_for_testnet_only() {
        let c = cfg(r#"{"network":"testnet","testnetRpc":"http://localhost:9000/"}"#);
        assert_eq!(c.rpc_url(Network::Testnet), "http://localhost:9000");
        assert_eq!(c.rpc_url(Network::Mainnet), DEFAULT_RPC_MAINNET);
    }

    #[test]
    fn ignores_an_unsafe_override() {
        let c = cfg(r#"{"network":"testnet","testnetRpc":"http://example.com"}"#);
        assert_eq!(c.rpc_url(Network::Testnet), DEFAULT_RPC_TESTNET);
        assert!(!rpc_ok("not a url"));
    }

    // src-tauri/src/network.rs tests
    #[test]
    fn mainnet_is_the_default_and_unchanged() {
        let c = cfg("{}");
        assert_eq!(c.market_url("/v1/items/0x1"), "https://rare.market/v1/items/0x1");
        assert_eq!(c.sponsor_url("/v1/sponsor"), "https://rare.market/v1/sponsor");
        let c = cfg(r#"{"testnetApi":"https://t.example.com"}"#);
        assert_eq!(c.market_url("/v1/status"), "https://rare.market/v1/status");
    }

    #[test]
    fn testnet_adds_the_network_query() {
        let c = cfg(r#"{"network":"testnet"}"#);
        assert_eq!(c.market_url("/v1/items/0x1"), "https://rare.market/v1/items/0x1?network=testnet");
        assert_eq!(c.market_url("/v1/offers?item=0x1"), "https://rare.market/v1/offers?item=0x1&network=testnet");
        assert_eq!(c.sponsor_url("/v1/sponsor/execute"), "https://rare.market/v1/sponsor/execute?network=testnet");
    }

    #[test]
    fn testnet_overrides_and_sponsor_falls_back_to_the_api() {
        let c = cfg(r#"{"network":"testnet","testnetApi":"http://localhost:3001/"}"#);
        assert_eq!(c.market_url("/v1/status"), "http://localhost:3001/v1/status?network=testnet");
        assert_eq!(c.sponsor_url("/v1/sponsor"), "http://localhost:3001/v1/sponsor?network=testnet");
        let c = cfg(r#"{"network":"testnet","testnetApi":"http://localhost:3001","testnetSponsor":"https://sponsor.example.com"}"#);
        assert_eq!(c.sponsor_url("/v1/sponsor"), "https://sponsor.example.com/v1/sponsor?network=testnet");
    }

    #[test]
    fn a_bad_override_is_ignored() {
        for bad in ["http://evil.example.com", "ftp://x", "not a url", "https://x.com/?a=1"] {
            let c = cfg(&format!(r#"{{"network":"testnet","testnetApi":"{bad}"}}"#));
            assert_eq!(c.market_url("/v1/status"), "https://rare.market/v1/status?network=testnet", "{bad}");
        }
    }

    // trophies/config.ts + localapi/voucher.ts: the mainnet deploy ids
    #[test]
    fn mainnet_package_ids_from_the_deploy() {
        let t = trophy_config().expect("trophies are deployed on mainnet");
        assert_eq!(t.network, Network::Mainnet);
        assert_eq!(t.package, "0x4b8d384ca5b99280138687a6211298184ff9a34c3193eb4ca61540a364cf69a5");
        assert_eq!(t.registry, "0xa9ff67e1d28a912e96ad66736eab996f4fb8f530eb959fbf914d4efeec06742b");
        assert_eq!(items_package(Network::Mainnet).as_deref(), Some("0xda09fa9db4a54d4b0c337fb7ee41311f4679a7f6049e58372a4951968a32f73c"));
        assert_eq!(items_package(Network::Testnet), None);
        assert_eq!(trophy_deployment(Network::Testnet), None);
        assert!(device_pass_deployment(Network::Testnet).is_some());
        assert_eq!(RARE_STORE_PACKAGE, None);
    }
}
