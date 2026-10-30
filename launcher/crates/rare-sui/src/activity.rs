//! Recent activity for an address from the Trade API's indexer (Sui's gRPC API has no "transactions by
//! address" query; the Trade API only indexes mainnet). Ports `src/wallet/data/activity.ts`; the detail
//! page reads one transaction over gRPC ([`crate::txdetail`]).

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::market::encode_component;
use crate::network::Network;
use crate::prices::{HttpGet, ReqwestGet, TRADE_URL};
use crate::{Error, Result};

const PAGE: u32 = 30;

/// One balance change in the Trade API's /ext/activity answer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityChange {
    pub coin_type: String,
    /// Signed base units.
    pub amount: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Send,
    Receive,
    Swap,
    #[default]
    Other,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeActivityItem {
    pub digest: String,
    pub timestamp_ms: i64,
    pub kind: Kind,
    pub changes: Vec<ActivityChange>,
    #[serde(default)]
    pub counterparty: Option<String>,
    /// "success" | "failure"
    #[serde(default)]
    pub status: Option<String>,
    /// Transfers rolled into one row where history kept only their sum.
    #[serde(default)]
    pub count: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Success,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityItem {
    pub digest: String,
    /// ms since epoch
    pub timestamp: i64,
    pub kind: Kind,
    pub changes: Vec<ActivityChange>,
    /// "1.5 SUI", "215.195 SUI → 502.6154 MAGMA"
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterparty: Option<String>,
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityPage {
    pub items: Vec<ActivityItem>,
    /// Pass back for the next page; None at the end.
    pub cursor: Option<String>,
}

/// A number the way `toLocaleString(undefined, { maximumFractionDigits: 4 })` prints it (en-US).
pub fn locale4(v: f64) -> String {
    let s = format!("{:.4}", v.abs());
    let (int, frac) = s.split_once('.').unwrap_or((&s, ""));
    let frac = frac.trim_end_matches('0');
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let sign = if v < 0.0 && (int != "0" || !frac.is_empty()) { "-" } else { "" };
    if frac.is_empty() {
        format!("{sign}{grouped}")
    } else {
        format!("{sign}{grouped}.{frac}")
    }
}

/// "1.5 SUI": the size of a change, unsigned (the row's label says which way).
fn fmt(c: &ActivityChange) -> String {
    let dec = c.decimals.unwrap_or(9);
    let digits = c.amount.trim_start_matches('-');
    let sym = c.symbol.clone().unwrap_or_else(|| c.coin_type.rsplit("::").next().unwrap_or("").to_string());
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return sym;
    }
    // integer and fraction parts exactly, then one f64 like Number(raw / unit) + Number(raw % unit) / Number(unit)
    let (int, frac) = if digits.len() > dec as usize { digits.split_at(digits.len() - dec as usize) } else { ("0", digits) };
    let frac = format!("{frac:0>width$}", width = dec as usize);
    let v = int.parse::<f64>().unwrap_or(0.0) + if dec == 0 { 0.0 } else { frac.parse::<f64>().unwrap_or(0.0) / 10f64.powi(dec as i32) };
    format!("{} {sym}", locale4(v))
}

pub fn to_activity_item(y: &TradeActivityItem) -> ActivityItem {
    let out: Vec<&ActivityChange> = y.changes.iter().filter(|c| c.amount.starts_with('-')).collect();
    let inn: Vec<&ActivityChange> = y.changes.iter().filter(|c| !c.amount.starts_with('-')).collect();
    let kind = match y.kind {
        Kind::Other if !out.is_empty() && !inn.is_empty() => Kind::Swap,
        Kind::Other if !out.is_empty() => Kind::Send,
        Kind::Other => Kind::Receive,
        k => k,
    };
    let legs = |cs: &[&ActivityChange]| cs.iter().map(|c| fmt(c)).collect::<Vec<_>>().join(", ");
    let mut summary = if kind == Kind::Swap && !out.is_empty() && !inn.is_empty() {
        format!("{} → {}", legs(&out), legs(&inn))
    } else {
        let all: Vec<&ActivityChange> = y.changes.iter().collect();
        let l = legs(&all);
        if l.is_empty() {
            "Transaction".into()
        } else {
            l
        }
    };
    if let Some(n) = y.count.filter(|n| *n > 1) {
        summary.push_str(&format!(" in {n} transfers"));
    }
    ActivityItem {
        digest: y.digest.clone(),
        kind,
        changes: y.changes.clone(),
        summary,
        timestamp: y.timestamp_ms,
        status: if y.status.as_deref() == Some("failure") { Status::Failed } else { Status::Success },
        counterparty: y.counterparty.clone().filter(|_| kind != Kind::Swap),
        count: y.count,
    }
}

/// the Trade API's activity feed.
pub struct ActivityFeed {
    base: String,
    network: Network,
    http: Arc<dyn HttpGet>,
}

impl ActivityFeed {
    pub fn new(base_url: Option<&str>, network: Network, http: Option<Arc<dyn HttpGet>>) -> Self {
        ActivityFeed { base: base_url.unwrap_or(TRADE_URL).trim_end_matches('/').to_string(), network, http: http.unwrap_or_else(|| Arc::new(ReqwestGet::default())) }
    }

    /// One page of activity, newest first. Empty on testnet, where the Trade API has no history.
    pub async fn list(&self, address: &str, cursor: Option<&str>) -> Result<ActivityPage> {
        if self.network == Network::Testnet {
            return Ok(ActivityPage::default());
        }
        let url = format!("{}/ext/activity/{address}?limit={PAGE}{}", self.base, cursor.map(|c| format!("&cursor={}", encode_component(c))).unwrap_or_default());
        let body = self.http.get_json(&url).await.map_err(|e| match e {
            Error::Http { status, message } if !message.is_empty() => Error::Http { status, message },
            Error::Http { status: Some(s), .. } => Error::http(Some(s), format!("Activity unavailable ({s})")),
            Error::Http { status: None, .. } | Error::Decode(_) => Error::http(None, "Activity unavailable"),
            other => other,
        })?;
        let Some(items) = body.get("items").and_then(Value::as_array) else { return Err(Error::http(None, "Activity unavailable")) };
        let items = items.iter().filter_map(|i| serde_json::from_value::<TradeActivityItem>(i.clone()).ok()).map(|y| to_activity_item(&y)).collect();
        Ok(ActivityPage { items, cursor: body.get("cursor").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BoxFut;
    use serde_json::json;
    use std::sync::Mutex;

    fn me() -> String {
        format!("0x{}", "a".repeat(64))
    }
    fn bob() -> String {
        format!("0x{}", "b".repeat(64))
    }
    fn sui() -> String {
        format!("0x{}2::sui::SUI", "0".repeat(63))
    }
    fn tok() -> String {
        format!("0x{}::magma::MAGMA", "d".repeat(64))
    }
    fn ch(t: &str, amount: &str, symbol: Option<&str>, decimals: Option<u32>) -> ActivityChange {
        ActivityChange { coin_type: t.into(), amount: amount.into(), symbol: symbol.map(str::to_string), decimals, logo: None }
    }
    fn item() -> TradeActivityItem {
        TradeActivityItem { digest: "D1".into(), timestamp_ms: 1_700_000_000_000, kind: Kind::Receive, changes: vec![ch(&sui(), "1500000000", Some("SUI"), Some(9))], counterparty: Some(bob()), status: Some("success".into()), count: None }
    }

    // activity.test.ts "toActivityItem"
    #[test]
    fn maps_a_receive_with_its_sender() {
        assert_eq!(
            to_activity_item(&item()),
            ActivityItem { digest: "D1".into(), kind: Kind::Receive, changes: item().changes, summary: "1.5 SUI".into(), timestamp: 1_700_000_000_000, status: Status::Success, counterparty: Some(bob()), count: None }
        );
    }

    #[test]
    fn shows_both_legs_of_a_swap_without_a_counterparty() {
        let a = to_activity_item(&TradeActivityItem { kind: Kind::Swap, counterparty: None, changes: vec![ch(&sui(), "-215194996385", Some("SUI"), Some(9)), ch(&tok(), "502615361461", Some("MAGMA"), Some(9))], ..item() });
        assert_eq!(a.kind, Kind::Swap);
        assert_eq!(a.summary, "215.195 SUI → 502.6154 MAGMA");
        assert_eq!(a.counterparty, None);
    }

    #[test]
    fn shows_the_one_leg_a_fresh_swap_has_so_far() {
        let a = to_activity_item(&TradeActivityItem { kind: Kind::Swap, changes: vec![ch(&tok(), "2000000000", Some("MAGMA"), Some(9))], ..item() });
        assert_eq!(a.summary, "2 MAGMA");
    }

    #[test]
    fn maps_a_send_and_falls_back_to_the_coin_name_and_9_decimals() {
        let a = to_activity_item(&TradeActivityItem { kind: Kind::Send, changes: vec![ch(&tok(), "-3000000000", None, None)], ..item() });
        assert_eq!((a.kind, a.summary.as_str(), a.counterparty), (Kind::Send, "3 MAGMA", Some(bob())));
    }

    #[test]
    fn reads_other_activity_by_which_way_coins_moved() {
        let k = |c: Vec<ActivityChange>| to_activity_item(&TradeActivityItem { kind: Kind::Other, changes: c, ..item() }).kind;
        assert_eq!(k(vec![ch(&sui(), "-1", None, Some(9))]), Kind::Send);
        assert_eq!(k(vec![ch(&sui(), "1", None, Some(9))]), Kind::Receive);
        assert_eq!(k(vec![ch(&sui(), "-1", None, Some(9)), ch(&tok(), "1", None, Some(9))]), Kind::Swap);
    }

    #[test]
    fn says_how_many_transfers_a_rolled_up_row_holds_and_marks_failures() {
        let a = to_activity_item(&TradeActivityItem { count: Some(3), status: Some("failure".into()), ..item() });
        assert_eq!(a.summary, "1.5 SUI in 3 transfers");
        assert_eq!(a.count, Some(3));
        assert_eq!(a.status, Status::Failed);
    }

    #[test]
    fn keeps_precision_for_big_amounts() {
        let a = to_activity_item(&TradeActivityItem { changes: vec![ch(&tok(), "123456789012345678901234", Some("X"), Some(9))], ..item() });
        // (123456789012345.678901234).toLocaleString(undefined, { maximumFractionDigits: 4 }):
        // the double is 123456789012345.671875
        assert_eq!(a.summary, "123,456,789,012,345.6719 X");
    }

    struct Http {
        body: Result<Value>,
        urls: Mutex<Vec<String>>,
    }
    impl HttpGet for Http {
        fn get_json(&self, url: &str) -> BoxFut<'_, Value> {
            self.urls.lock().unwrap().push(url.to_string());
            let b = self.body.clone();
            Box::pin(async move { b })
        }
    }
    fn http(body: Result<Value>) -> Arc<Http> {
        Arc::new(Http { body, urls: Mutex::new(vec![]) })
    }
    fn to_json(y: &TradeActivityItem) -> Value {
        serde_json::to_value(y).unwrap()
    }

    // activity.test.ts "createActivityFeed"
    #[tokio::test]
    async fn reads_trade_apis_activity_endpoint_and_maps_every_item() {
        let h = http(Ok(json!({ "items": [to_json(&item()), to_json(&TradeActivityItem { digest: "D2".into(), kind: Kind::Send, ..item() })], "cursor": "next" })));
        let page = ActivityFeed::new(Some("https://y.test/"), Network::Mainnet, Some(h.clone())).list(&me(), None).await.unwrap();
        assert_eq!(h.urls.lock().unwrap()[0], format!("https://y.test/ext/activity/{}?limit=30", me()));
        assert_eq!(page.cursor.as_deref(), Some("next"));
        assert_eq!(page.items.iter().map(|a| (a.digest.as_str(), a.kind)).collect::<Vec<_>>(), [("D1", Kind::Receive), ("D2", Kind::Send)]);
    }

    #[tokio::test]
    async fn throws_trade_apis_error_or_a_plain_one() {
        let e = ActivityFeed::new(None, Network::Mainnet, Some(http(Err(Error::http(Some(502), "Activity temporarily unavailable"))))).list(&me(), None).await.unwrap_err();
        assert_eq!(e.to_string(), "http 502: Activity temporarily unavailable");
        let e = ActivityFeed::new(None, Network::Mainnet, Some(http(Err(Error::http(Some(504), ""))))).list(&me(), None).await.unwrap_err();
        assert_eq!(e.to_string(), "http 504: Activity unavailable (504)");
        let e = ActivityFeed::new(None, Network::Mainnet, Some(http(Ok(json!({ "nope": true }))))).list(&me(), None).await.unwrap_err();
        assert!(e.to_string().contains("unavailable"));
    }

    #[tokio::test]
    async fn passes_the_cursor_back_for_the_next_page() {
        let h = http(Ok(json!({ "items": [to_json(&item())], "cursor": null })));
        let page = ActivityFeed::new(Some("https://y.test"), Network::Mainnet, Some(h.clone())).list(&me(), Some("eyJiIjoxfQ")).await.unwrap();
        assert_eq!(h.urls.lock().unwrap()[0], format!("https://y.test/ext/activity/{}?limit=30&cursor=eyJiIjoxfQ", me()));
        assert_eq!(page.cursor, None);
    }

    #[tokio::test]
    async fn skips_the_network_on_testnet() {
        let h = http(Ok(json!({})));
        assert_eq!(ActivityFeed::new(None, Network::Testnet, Some(h.clone())).list(&me(), None).await.unwrap(), ActivityPage::default());
        assert!(h.urls.lock().unwrap().is_empty());
    }

    #[test]
    fn locale_numbers() {
        assert_eq!(locale4(1.5), "1.5");
        assert_eq!(locale4(1234.56789), "1,234.5679");
        assert_eq!(locale4(2.0), "2");
        assert_eq!(locale4(0.00001), "0");
    }
}
