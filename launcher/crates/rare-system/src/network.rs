//! Which Sui network the launcher talks to (Settings > Developer). Mainnet by default. On testnet
//! the rare.market API and the gas sponsor answer with `?network=testnet`, and the UI points its Sui reads at the testnet RPC (src/lib/network.ts). The testnet API,
//! sponsor and RPC addresses can be overridden in the config for a studio's own test setup.

use serde::Deserialize;

/// The three config fields this module reads. The launcher's Config implements it; the native
/// UI can use `NetworkSettings` straight from the config file.
pub trait NetworkCfg {
    fn network(&self) -> Option<&str>;
    fn testnet_api(&self) -> &Option<String>;
    fn testnet_sponsor(&self) -> &Option<String>;
}

#[derive(Deserialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct NetworkSettings {
    pub network: Option<String>,
    pub testnet_api: Option<String>,
    pub testnet_sponsor: Option<String>,
}

impl NetworkCfg for NetworkSettings {
    fn network(&self) -> Option<&str> {
        self.network.as_deref()
    }
    fn testnet_api(&self) -> &Option<String> {
        &self.testnet_api
    }
    fn testnet_sponsor(&self) -> &Option<String> {
        &self.testnet_sponsor
    }
}

const MARKET: &str = "https://rare.market";

pub fn is_testnet(c: &impl NetworkCfg) -> bool {
    c.network() == Some("testnet")
}

/// https anywhere; plain http only on this machine (a local API under development).
fn base_ok(b: &str) -> bool {
    match b.parse::<url::Url>() {
        Ok(u) => crate::host::get().web_url_allowed(&u) && u.query().is_none() && u.fragment().is_none(),
        Err(_) => false,
    }
}

fn base<'a>(over: &'a Option<String>) -> &'a str {
    match over.as_deref().map(|s| s.trim().trim_end_matches('/')) {
        Some(b) if base_ok(b) => b,
        _ => MARKET,
    }
}

fn with_network(base: &str, path: &str, testnet: bool) -> String {
    let mut url = format!("{base}{path}");
    if testnet && !url.contains("network=") {
        url.push(if url.contains('?') { '&' } else { '?' });
        url.push_str("network=testnet");
    }
    url
}

/// Where a read goes: rare.market on mainnet; the testnet API (same host unless overridden) on testnet.
pub fn market_url(c: &impl NetworkCfg, path: &str) -> String {
    if is_testnet(c) {
        with_network(base(c.testnet_api()), path, true)
    } else {
        format!("{MARKET}{path}")
    }
}

/// Where a gas sponsor call goes: the testnet sponsor if set, else the testnet API base.
pub fn sponsor_url(c: &impl NetworkCfg, path: &str) -> String {
    if is_testnet(c) {
        let b = if c.testnet_sponsor().is_some() { base(c.testnet_sponsor()) } else { base(c.testnet_api()) };
        with_network(b, path, true)
    } else {
        format!("{MARKET}{path}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(j: &str) -> NetworkSettings {
        serde_json::from_str(j).unwrap()
    }

    #[test]
    fn mainnet_is_the_default_and_unchanged() {
        let c = cfg("{}");
        assert_eq!(market_url(&c, "/v1/items/0x1"), "https://rare.market/v1/items/0x1");
        assert_eq!(sponsor_url(&c, "/v1/sponsor"), "https://rare.market/v1/sponsor");
        // overrides mean nothing on mainnet
        let c = cfg(r#"{"testnetApi":"https://t.example.com"}"#);
        assert_eq!(market_url(&c, "/v1/status"), "https://rare.market/v1/status");
    }

    #[test]
    fn testnet_adds_the_network_query() {
        let c = cfg(r#"{"network":"testnet"}"#);
        assert_eq!(market_url(&c, "/v1/items/0x1"), "https://rare.market/v1/items/0x1?network=testnet");
        assert_eq!(market_url(&c, "/v1/offers?item=0x1"), "https://rare.market/v1/offers?item=0x1&network=testnet");
        assert_eq!(sponsor_url(&c, "/v1/sponsor/execute"), "https://rare.market/v1/sponsor/execute?network=testnet");
    }

    #[test]
    fn testnet_overrides_and_sponsor_falls_back_to_the_api() {
        let c = cfg(r#"{"network":"testnet","testnetApi":"http://localhost:3001/"}"#);
        assert_eq!(market_url(&c, "/v1/status"), "http://localhost:3001/v1/status?network=testnet");
        assert_eq!(sponsor_url(&c, "/v1/sponsor"), "http://localhost:3001/v1/sponsor?network=testnet");
        let c = cfg(r#"{"network":"testnet","testnetApi":"http://localhost:3001","testnetSponsor":"https://sponsor.example.com"}"#);
        assert_eq!(sponsor_url(&c, "/v1/sponsor"), "https://sponsor.example.com/v1/sponsor?network=testnet");
    }

    #[test]
    fn a_bad_override_is_ignored() {
        for bad in ["http://evil.example.com", "ftp://x", "not a url", "https://x.com/?a=1"] {
            let c = cfg(&format!(r#"{{"network":"testnet","testnetApi":"{bad}"}}"#));
            assert_eq!(market_url(&c, "/v1/status"), "https://rare.market/v1/status?network=testnet", "{bad}");
        }
    }
}
