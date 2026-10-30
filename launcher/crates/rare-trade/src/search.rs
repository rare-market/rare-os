//! Coin search for the Trade tab: the Trade API's own token search (/search), with the chain's coin
//! metadata as a fallback so a pasted full coin type opens even when the coin is not indexed.
//! Recent searches are kept on the device.

use crate::amount::{is_coin_type, normalize_type};
use crate::api::{token_paths, TokenRow};
use crate::chain::ChainReader;
use crate::http::{enc, get};
use crate::kv::Kv;
use rare_market::net::Net;
use serde::{Deserialize, Serialize};

pub fn search_path(q: &str) -> String {
    let q: String = q.trim().chars().take(160).collect();
    format!("/search?q={}", enc(&q))
}

/// Pasted text that names a coin: a full type, with or without the short 0x2 form, quotes allowed.
pub fn pasted_type(q: &str) -> Option<String> {
    let t = q.trim();
    let t = t.strip_prefix(['"', '\'', '`']).unwrap_or(t);
    let t = t.strip_suffix(['"', '\'', '`']).unwrap_or(t);
    is_coin_type(t).then(|| normalize_type(t))
}

/// Coins matching `q` by name, ticker, address or full type. Never fails: no answer is no coins.
pub fn search_coins(net: &dyn Net, chain: &dyn ChainReader, q: &str) -> Vec<TokenRow> {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Found {
        items: Vec<TokenRow>,
    }
    let text = q.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let ty = pasted_type(text);
    let found = get::<Found>(net, &search_path(ty.as_deref().unwrap_or(text))).map(|f| f.items).unwrap_or_default();
    let Some(ty) = ty else { return found };
    if found.iter().any(|t| normalize_type(&t.coin_type) == ty) {
        return found;
    }
    // Not indexed: the Trade API's lean token answer, then the chain's own coin metadata.
    if let Ok(lean) = get::<TokenRow>(net, &token_paths::lean(&ty)) {
        if !lean.coin_type.is_empty() {
            return std::iter::once(lean).chain(found).collect();
        }
    }
    let Ok(Some(meta)) = chain.coin_metadata(&ty) else { return found };
    let row = TokenRow { coin_type: ty, symbol: meta.symbol, name: meta.name, decimals: meta.decimals, logo: meta.icon_url, price_usd: None, ..TokenRow::default() };
    std::iter::once(row).chain(found).collect()
}

// ---------- recents ----------

const KEY: &str = "rare:trade:recent-coins";
const MAX_RECENT: usize = 8;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recent {
    pub coin_type: String,
    #[serde(default)]
    pub symbol: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub logo: Option<String>,
}

/// The last coins opened from search, newest first.
pub fn recent_coins(kv: &dyn Kv) -> Vec<Recent> {
    let Some(raw) = kv.get(KEY) else { return Vec::new() };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&raw) else { return Vec::new() };
    items.into_iter().filter(|r| r.get("coinType").is_some_and(|t| t.is_string())).filter_map(|r| serde_json::from_value(r).ok()).take(MAX_RECENT).collect()
}

pub fn remember_coin(kv: &dyn Kv, t: &TokenRow) {
    let ty = normalize_type(&t.coin_type);
    let mut list = vec![Recent { coin_type: ty.clone(), symbol: t.symbol.clone(), name: t.name.clone(), logo: t.logo.clone() }];
    list.extend(recent_coins(kv).into_iter().filter(|r| normalize_type(&r.coin_type) != ty));
    list.truncate(MAX_RECENT);
    if let Ok(s) = serde_json::to_string(&list) {
        kv.set(KEY, &s);
    }
}

#[cfg(test)]
mod tests {
    //! search.test.ts
    use super::*;
    use crate::chain::{CoinMetadata, FakeChain};
    use crate::kv::MemKv;
    use rare_market::net::MockNet;
    use serde_json::json;

    fn ty() -> String {
        format!("0x{}::meme::MEME", "ab".repeat(32))
    }
    fn row(coin_type: &str, symbol: &str) -> serde_json::Value {
        json!({ "coinType": coin_type, "symbol": symbol, "name": symbol, "decimals": 9, "logo": null, "priceUsd": 1 })
    }

    /// A chain that must not be asked.
    struct NoChain;
    impl ChainReader for NoChain {
        fn balance(&self, _: &str, _: &str) -> crate::http::Result<u128> {
            panic!("balance")
        }
        fn object_types(&self, _: &[String]) -> crate::http::Result<Vec<Option<String>>> {
            panic!("object_types")
        }
        fn coin_metadata(&self, _: &str) -> crate::http::Result<Option<CoinMetadata>> {
            panic!("metadata")
        }
    }

    #[test]
    fn asks_trade_apis_search_with_the_text() {
        let n = MockNet::new();
        n.on_read_json(&format!("trade:{}", search_path("sui")), json!({ "items": [row("0x2::sui::SUI", "SUI")] }));
        let r = search_coins(&n, &NoChain, "sui");
        assert_eq!(n.reads(), vec![format!("trade:{}", search_path("sui"))]);
        assert_eq!(r.iter().map(|t| t.symbol.as_str()).collect::<Vec<_>>(), ["SUI"]);
    }

    #[test]
    fn opens_a_pasted_type_that_is_not_indexed_from_the_chains_metadata() {
        let n = MockNet::new();
        n.on_read_json(&format!("trade:{}", search_path(&ty())), json!({ "items": [] }));
        let mut c = FakeChain::default();
        c.metadata.insert(ty(), CoinMetadata { symbol: "MEME".into(), name: "Meme Coin".into(), decimals: 6, icon_url: Some("https://x/y.png".into()) });
        let r = search_coins(&n, &c, &format!(" {} ", ty()));
        assert_eq!(r.len(), 1);
        assert_eq!((r[0].coin_type.as_str(), r[0].symbol.as_str(), r[0].decimals, r[0].logo.as_deref(), r[0].price_usd), (ty().as_str(), "MEME", 6, Some("https://x/y.png"), None));
    }

    #[test]
    fn prefers_trade_apis_lean_answer_to_the_chain() {
        let n = MockNet::new();
        n.on_read_json(&format!("trade:{}", search_path(&ty())), json!({ "items": [] }));
        n.on_read_json(&format!("trade:{}", token_paths::lean(&ty())), row(&ty(), "MEME"));
        let r = search_coins(&n, &NoChain, &ty());
        assert_eq!(r[0].symbol, "MEME");
    }

    #[test]
    fn a_failing_search_still_resolves_a_pasted_type() {
        let n = MockNet::new();
        let mut c = FakeChain::default();
        c.metadata.insert(ty(), CoinMetadata { symbol: "M".into(), name: "M".into(), decimals: 9, icon_url: None });
        assert_eq!(search_coins(&n, &c, &ty())[0].symbol, "M");
    }

    #[test]
    fn tells_types_from_words() {
        assert_eq!(pasted_type("0x2::sui::SUI"), Some(format!("0x{}2::sui::SUI", "0".repeat(63))));
        assert_eq!(pasted_type("sui"), None);
        assert_eq!(pasted_type("'0x2::sui::SUI'").as_deref(), Some(crate::amount::SUI_TYPE));
    }

    #[test]
    fn recents_newest_first_without_repeats() {
        let kv = MemKv::new();
        let t = |c: &str, s: &str| TokenRow { coin_type: c.into(), symbol: s.into(), ..TokenRow::default() };
        remember_coin(&kv, &t("0x2::sui::SUI", "SUI"));
        remember_coin(&kv, &t(&ty(), "MEME"));
        remember_coin(&kv, &t(crate::amount::SUI_TYPE, "SUI"));
        assert_eq!(recent_coins(&kv).iter().map(|r| r.symbol.as_str()).collect::<Vec<_>>(), ["SUI", "MEME"]);
        for i in 0..10 {
            remember_coin(&kv, &t(&format!("0x{i}::c::C"), "C"));
        }
        assert_eq!(recent_coins(&kv).len(), 8);
        kv.set(KEY, "{broken");
        assert!(recent_coins(&kv).is_empty());
    }
}
