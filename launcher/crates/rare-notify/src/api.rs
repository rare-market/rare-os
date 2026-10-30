//! rare.market's notify API: recent notices and price alerts, uncached (they are per device and
//! per account and must be current). The live stream is in `stream.rs`; its URL is built here.

use std::time::Duration;

use rare_market::net::{Call, Method, Net, Reply};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::logic::{form_query, uri_component};
use crate::types::{notify_base, AlertDir, Notice, PriceAlert};
use crate::{Error, Result};

/// The web launcher's fetch timeout for these calls.
const TIMEOUT: Duration = Duration::from_secs(8);

/// The stream's URL for a query built by `logic::stream_query`.
pub fn stream_url(origin: &str, query: &str) -> String {
    format!("{}/stream?{query}", notify_base(origin))
}

fn send(net: &dyn Net, call: Call) -> Result<Value> {
    let r: Reply = net.call(&call.timeout(TIMEOUT))?;
    if !r.ok() {
        return Err(Error(format!("rare.market {}", r.status)));
    }
    Ok(r.json())
}

/// `data` as a list of `T`, skipping entries that do not parse (one bad notice must not hide the
/// rest).
fn data_list<T: for<'de> Deserialize<'de>>(v: &Value) -> Vec<T> {
    v.get("data").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| serde_json::from_value(x.clone()).ok()).collect()).unwrap_or_default()
}

/// The latest notices for these accounts and this device, newest first.
pub fn recent(net: &dyn Net, addrs: &[String], device: &str, limit: usize) -> Result<Vec<Notice>> {
    let mut q = vec![("device", device.to_string()), ("limit", limit.to_string())];
    if !addrs.is_empty() {
        q.push(("addr", addrs.join(",")));
    }
    let url = format!("{}/recent?{}", notify_base(&net.market_origin()), form_query(&q));
    Ok(data_list(&send(net, Call::get(url))?))
}

/// The default `limit` for [`recent`].
pub const RECENT_LIMIT: usize = 50;

pub fn list_alerts(net: &dyn Net, device: &str) -> Result<Vec<PriceAlert>> {
    let url = format!("{}/alerts?device={}", notify_base(&net.market_origin()), uri_component(device));
    Ok(data_list(&send(net, Call::get(url))?))
}

/// A price alert to set. `symbol` is for display only; the server does not take it.
#[derive(Clone, Debug, PartialEq)]
pub struct NewAlert {
    pub coin_type: String,
    pub symbol: String,
    pub dir: AlertDir,
    pub target: f64,
    pub addr: Option<String>,
}

/// Sets an alert for this device; the new alert's id.
pub fn add_alert(net: &dyn Net, device: &str, a: &NewAlert) -> Result<String> {
    let mut body = json!({ "device": device, "coinType": a.coin_type, "dir": a.dir.as_str(), "target": a.target });
    if let Some(addr) = &a.addr {
        body["addr"] = json!(addr);
    }
    let url = format!("{}/alerts", notify_base(&net.market_origin()));
    let v = send(net, Call::post(url, body.to_string()).header("content-type", "application/json"))?;
    Ok(v.get("id").and_then(Value::as_str).unwrap_or_default().to_string())
}

pub fn delete_alert(net: &dyn Net, id: &str, device: &str) -> Result<()> {
    let url = format!("{}/alerts/{}?device={}", notify_base(&net.market_origin()), uri_component(id), uri_component(device));
    send(net, Call::new(Method::Delete, url, None)).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rare_market::net::MockNet;

    #[test]
    fn recent_reads_data_and_skips_bad_entries() {
        let net = MockNet::new();
        net.on_call(Method::Get, "https://rare.market/v1/notify/recent", 200, r#"{"data":[{"id":"2","kind":"sold","title":"a"},7,{"id":"1","kind":"bought","title":"b"}]}"#);
        let list = recent(&net, &["0xa".into(), "0xb".into()], "dev", 50).unwrap();
        assert_eq!(list.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), ["2", "1"]);
        assert_eq!(net.calls()[0].url, "https://rare.market/v1/notify/recent?device=dev&limit=50&addr=0xa%2C0xb");
    }

    #[test]
    fn errors_carry_the_status() {
        let net = MockNet::new();
        net.on_call(Method::Get, "https://rare.market/v1/notify/alerts", 503, "");
        assert_eq!(list_alerts(&net, "d").unwrap_err().0, "rare.market 503");
    }

    #[test]
    fn alerts_post_without_symbol_and_delete_by_id() {
        let net = MockNet::new();
        net.on_call(Method::Post, "https://rare.market/v1/notify/alerts", 200, r#"{"id":"a9"}"#);
        net.on_call(Method::Delete, "https://rare.market/v1/notify/alerts/", 200, r#"{"ok":true}"#);
        let a = NewAlert { coin_type: "0x2::sui::SUI".into(), symbol: "SUI".into(), dir: AlertDir::Below, target: 0.9, addr: None };
        assert_eq!(add_alert(&net, "dev", &a).unwrap(), "a9");
        delete_alert(&net, "a 9", "dev").unwrap();
        let calls = net.calls();
        let body: Value = serde_json::from_str(calls[0].body.as_deref().unwrap()).unwrap();
        assert_eq!(body, json!({ "device": "dev", "coinType": "0x2::sui::SUI", "dir": "below", "target": 0.9 }));
        assert_eq!(calls[1].url, "https://rare.market/v1/notify/alerts/a%209?device=dev");
    }
}
