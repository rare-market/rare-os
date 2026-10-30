//! Coin balances and the token portfolio. Ports `src/wallet/data/balances.ts`.
//!
//! Amounts are always from Sui gRPC (list_balances + coin metadata); USD prices and 24 h change
//! are layered on from the Trade API when it answers, so the wallet keeps working without prices.

use std::cmp::Ordering;
use std::collections::HashMap;

use futures::future::join_all;
use serde::{Deserialize, Serialize};

use crate::grpc::ChainRead;
use crate::ids::{canon_type, is_sui, short_key};
use crate::prices::{PriceClient, TradePortfolio};
use crate::Result;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Balance {
    pub coin_type: String,
    pub symbol: String,
    pub name: String,
    pub decimals: u32,
    pub icon_url: Option<String>,
    /// Base units.
    pub raw: u64,
    /// Whole tokens.
    pub amount: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioToken {
    #[serde(flatten)]
    pub balance: Balance,
    pub price_usd: Option<f64>,
    pub value_usd: Option<f64>,
    /// Percent over 24 h.
    pub change24h: Option<f64>,
    /// the Trade API lists it as a verified token.
    pub verified: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Portfolio {
    /// None when no token has a price (the Trade API down).
    pub total_usd: Option<f64>,
    pub change24h_usd: Option<f64>,
    pub tokens: Vec<PortfolioToken>,
    /// False when prices could not be loaded.
    pub priced: bool,
}

/// Base units to whole tokens without losing the integer part.
pub fn to_amount(raw: u64, decimals: u32) -> f64 {
    let unit = 10u128.pow(decimals.min(38));
    let raw = u128::from(raw);
    (raw / unit) as f64 + (raw % unit) as f64 / unit as f64
}

/// SUI first, then by the given key, larger first.
fn sui_first<T>(a: &T, b: &T, ty: impl Fn(&T) -> &str, rest: impl Fn(&T, &T) -> Ordering) -> Ordering {
    match (is_sui(ty(a)), is_sui(ty(b))) {
        (true, _) => Ordering::Less,
        (_, true) => Ordering::Greater,
        _ => rest(a, b),
    }
}

/// Every non-zero balance with its metadata: SUI first, then larger holdings.
pub async fn balances(chain: &dyn ChainRead, address: &str) -> Result<Vec<Balance>> {
    let nodes = chain.list_balances(address).await?;
    let metas = join_all(nodes.iter().map(|n| async move { chain.coin_metadata(&n.coin_type).await.ok().flatten() })).await;
    let mut out: Vec<Balance> = nodes
        .iter()
        .zip(metas)
        .map(|(n, m)| {
            let decimals = m.as_ref().map(|m| m.decimals).unwrap_or(9);
            Balance {
                coin_type: n.coin_type.clone(),
                symbol: m.as_ref().map(|m| m.symbol.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| n.coin_type.rsplit("::").next().unwrap_or("?").to_string()),
                name: m.as_ref().map(|m| m.name.clone()).unwrap_or_default(),
                decimals,
                icon_url: m.and_then(|m| m.icon_url),
                raw: n.balance,
                amount: to_amount(n.balance, decimals),
            }
        })
        .filter(|b| b.raw > 0)
        .collect();
    out.sort_by(|a, b| sui_first(a, b, |x| &x.coin_type, |a, b| b.amount.partial_cmp(&a.amount).unwrap_or(Ordering::Equal)));
    Ok(out)
}

/// Merge gRPC balances with the Trade API prices (pure).
pub fn merge_portfolio(list: &[Balance], y: Option<&TradePortfolio>) -> Portfolio {
    let prices: HashMap<String, _> = y.map(|y| y.holdings.iter().map(|h| (short_key(&canon_type(&h.meta.coin_type)), h)).collect()).unwrap_or_default();
    let mut total = 0.0;
    let mut change = 0.0;
    let mut any = false;
    let mut tokens: Vec<PortfolioToken> = list
        .iter()
        .map(|b| {
            let h = prices.get(&short_key(&canon_type(&b.coin_type)));
            let price_usd = h.and_then(|h| h.meta.price_usd);
            let value_usd = price_usd.map(|p| b.amount * p);
            let change24h = h.and_then(|h| h.meta.change24h);
            if let Some(v) = value_usd {
                any = true;
                total += v;
                // value 24 h ago = now / (1 + pct); the difference is today's move in USD
                if let Some(c) = change24h.filter(|c| *c > -100.0) {
                    change += v - v / (1.0 + c / 100.0);
                }
            }
            let mut balance = b.clone();
            if balance.icon_url.is_none() {
                balance.icon_url = h.and_then(|h| h.meta.logo.clone());
            }
            PortfolioToken { balance, price_usd, value_usd, change24h, verified: h.is_some_and(|h| h.meta.verified) }
        })
        .collect();
    // SUI first, then by USD value; unpriced tokens after priced ones, by amount
    tokens.sort_by(|a, b| {
        sui_first(a, b, |x| &x.balance.coin_type, |a, b| match (a.value_usd, b.value_usd) {
            (Some(x), Some(y)) => y.partial_cmp(&x).unwrap_or(Ordering::Equal),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => b.balance.amount.partial_cmp(&a.balance.amount).unwrap_or(Ordering::Equal),
        })
    });
    Portfolio { total_usd: any.then_some(total), change24h_usd: any.then_some(change), tokens, priced: y.is_some() }
}

/// Balances from gRPC with USD prices from the Trade API when it answers.
pub async fn portfolio(chain: &dyn ChainRead, trade: &PriceClient, address: &str) -> Result<Portfolio> {
    let (list, prices) = futures::join!(balances(chain, address), trade.portfolio(address));
    Ok(merge_portfolio(&list?, prices.ok().as_ref()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::grpc::{CoinBalance, CoinMeta};
    use crate::prices::tests::FakeHttp;
    use crate::{BoxFut, Error};
    use serde_json::json;

    pub const ME: &str = "0xa1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
    const SUI: &str = "0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI";
    const USDC: &str = "0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC";
    const DMC: &str = "0x4c981f3ff786cdb9e514da897ab8a953647dae2ace9679e8358eec1e3e8871ac::dmc::DMC";
    fn junk() -> String {
        format!("0x{}::junk::JUNK", "99".repeat(32))
    }

    pub struct Client;
    impl ChainRead for Client {
        fn list_balances(&self, _o: &str) -> BoxFut<'_, Vec<CoinBalance>> {
            let b = |t: &str, v: u64| CoinBalance { coin_type: t.into(), balance: v, ..Default::default() };
            let v = vec![b(DMC, 188_000_000_000), b(USDC, 2_500_000), b(SUI, 57_497_388_949), b(&junk(), 5_000_000_000_000), b(&format!("0x{}::zero::ZERO", "88".repeat(32)), 0)];
            Box::pin(async move { Ok(v) })
        }
        fn coin_metadata(&self, t: &str) -> BoxFut<'_, Option<CoinMeta>> {
            let m = |d: u32, s: &str, n: &str, i: Option<&str>| Ok(Some(CoinMeta { decimals: d, symbol: s.into(), name: n.into(), description: String::new(), icon_url: i.map(str::to_string) }));
            let r = if t == USDC {
                m(6, "USDC", "USDC", Some("https://usdc.png"))
            } else if t == SUI {
                m(9, "SUI", "Sui", None)
            } else if t == junk() {
                Err(Error::invalid("no metadata"))
            } else {
                m(9, "DMC", "DeLorean", None)
            };
            Box::pin(async move { r })
        }
    }

    fn trade_body() -> serde_json::Value {
        json!({
            "totalUsd": 55, "change24hUsd": -0.8,
            "holdings": [
                { "coinType": "0x2::sui::SUI", "symbol": "SUI", "decimals": 9, "logo": "https://sui.png", "priceUsd": 1, "change24h": -2, "verified": true, "amount": 57.49, "valueUsd": 57.49 },
                { "coinType": USDC, "symbol": "USDC", "decimals": 6, "priceUsd": 1, "change24h": 0, "verified": true, "amount": 2.5, "valueUsd": 2.5 },
                { "coinType": DMC, "symbol": "DMC", "decimals": 9, "logo": "/logos/dmc.png", "priceUsd": 0.0003, "change24h": 5, "verified": false, "amount": 188, "valueUsd": 0.0564 },
            ]
        })
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    // data.test.ts "balances and portfolio"
    #[tokio::test]
    async fn balances_grpc_amounts_sui_first_zero_dropped_metadata_fallback() {
        let list = balances(&Client, ME).await.unwrap();
        let got: Vec<(String, f64)> = list.iter().map(|b| (b.symbol.clone(), b.amount)).collect();
        assert_eq!(got, vec![("SUI".to_string(), 57.497388949), ("JUNK".into(), 5000.0), ("DMC".into(), 188.0), ("USDC".into(), 2.5)]);
    }

    #[tokio::test]
    async fn portfolio_trade_prices_values_and_24h_change() {
        let y = PriceClient::new("https://y.test", FakeHttp::ok(trade_body()));
        let p = portfolio(&Client, &y, ME).await.unwrap();
        assert!(p.priced);
        assert_eq!(p.tokens.iter().map(|t| t.balance.symbol.as_str()).collect::<Vec<_>>(), ["SUI", "USDC", "DMC", "JUNK"]);
        let sui = &p.tokens[0];
        assert_eq!((sui.price_usd, sui.change24h, sui.verified), (Some(1.0), Some(-2.0), true));
        assert_eq!(sui.balance.icon_url.as_deref(), Some("https://sui.png"));
        assert!(close(sui.value_usd.unwrap(), 57.497388949));
        assert_eq!(p.tokens[2].balance.icon_url.as_deref(), Some("https://y.test/logos/dmc.png"));
        assert_eq!((p.tokens[3].price_usd, p.tokens[3].value_usd, p.tokens[3].verified), (None, None, false));
        assert!(close(p.total_usd.unwrap(), 57.497388949 + 2.5 + 188.0 * 0.0003));
        let exp = 57.497388949 - 57.497388949 / 0.98 + (188.0 * 0.0003 - (188.0 * 0.0003) / 1.05);
        assert!(close(p.change24h_usd.unwrap(), exp));
    }

    #[tokio::test]
    async fn portfolio_still_works_when_trade_is_down() {
        let y = PriceClient::new("https://y.test", FakeHttp::fail(502));
        let p = portfolio(&Client, &y, ME).await.unwrap();
        assert!(!p.priced);
        assert_eq!((p.total_usd, p.change24h_usd), (None, None));
        assert_eq!(p.tokens.iter().map(|t| t.balance.symbol.as_str()).collect::<Vec<_>>(), ["SUI", "JUNK", "DMC", "USDC"]);
    }

    #[test]
    fn merge_portfolio_with_no_balances() {
        assert_eq!(merge_portfolio(&[], None), Portfolio { total_usd: None, change24h_usd: None, tokens: vec![], priced: false });
    }

    #[test]
    fn amounts_keep_the_integer_part() {
        assert_eq!(to_amount(57_497_388_949, 9), 57.497388949);
        assert_eq!(to_amount(u64::MAX, 0), u64::MAX as f64);
    }
}
