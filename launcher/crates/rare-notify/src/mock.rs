//! Sample notices in the contract's shape, for UI work and golden images (the `mock` feature): a
//! few older ones for `recent`, live ones for the stream, and a price alert. Times are relative to a
//! `now` the caller gives, and nothing is random, so screenshots are stable. [`net`] wires them into
//! a `MockNet` so the whole notify stack runs on them.

use rare_market::net::{Method, MockNet};
use serde_json::json;

use crate::types::{AlertDir, Amount, CollectionRef, Notice, NoticeAction, NoticeItem, NoticeKind, PriceAlert};

pub const POPKIN: &str = "0x89d249c6a43ff4490902247a950e2e69ba61b74089151166169cfe3fbfea8611";
pub const ME: &str = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
pub const SUI: &str = "0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI";
pub const DIGEST: &str = "5QDVv7YPKSPiLA959P8T1nW4phnfHHLg9XuXMP6umQRP";
pub const SUI_PERP: &str = "0xa97dacdd972a0414363290ec5752cb93ac47443c3e7710df145cc7f337575002";

fn art(id: &str) -> String {
    format!("https://storage.claynosaurz.com/popkins/images/{id}")
}

fn popkin(n: u32) -> NoticeItem {
    NoticeItem { id: POPKIN.into(), name: format!("Popkins #{n}"), image_url: Some(art(POPKIN)), collection: Some(CollectionRef { slug: "popkins".into(), name: "Popkins".into() }) }
}

fn base(kind: NoticeKind, title: &str, body: Option<&str>, action: NoticeAction) -> Notice {
    Notice {
        kind,
        addr: Some(ME.into()),
        title: title.into(),
        body: body.map(str::to_string),
        action,
        digest: Some(DIGEST.into()),
        ..Default::default()
    }
}

fn sui(raw: &str, usd: f64) -> Option<Amount> {
    Some(Amount { raw: raw.into(), decimals: 9, symbol: "SUI".into(), coin_type: SUI.into(), usd: Some(usd) })
}

fn item_action() -> NoticeAction {
    NoticeAction::Item { item_id: POPKIN.into(), slug: Some("popkins".into()) }
}

fn coin_action() -> NoticeAction {
    NoticeAction::Coin { coin_type: SUI.into() }
}

/// The live samples, in the order the mock stream sends them (without ids or times).
pub fn live_samples() -> Vec<Notice> {
    vec![
        Notice {
            image_url: Some(art(POPKIN)),
            amount: sui("70000000000", 63.42),
            item: Some(popkin(412)),
            ..base(NoticeKind::Sold, "Popkins #412 sold for 70 SUI", Some("Bought by 0x12ab...9f3c on Tradeport"), item_action())
        },
        Notice {
            image_url: Some(art(POPKIN)),
            item: Some(popkin(412)),
            ..base(NoticeKind::OfferReceived, "New offer of 64 SUI on Popkins #412", Some("From 0x7c1d...a210, ends in 3 days"), item_action())
        },
        Notice { addr: None, digest: None, ..base(NoticeKind::PriceAlert, "SUI is above $1.00", Some("Your alert at $1.00 was hit. Now $1.01."), coin_action()) },
        Notice {
            digest: None,
            ..base(
                NoticeKind::PerpLiquidation,
                "SUI 5x long is near liquidation",
                Some("Mark $0.87, liquidation at $0.81"),
                NoticeAction::Position { market_id: SUI_PERP.into(), symbol: "SUI".into() },
            )
        },
        Notice { amount: sui("25000000000", 22.65), ..base(NoticeKind::CoinReceived, "You received 25 SUI", Some("From 0x3f9e...11c0"), NoticeAction::Tx { digest: DIGEST.into() }) },
    ]
}

/// The older samples with how long ago each happened, ms, newest first.
fn old_samples() -> Vec<(Notice, i64)> {
    let live = live_samples();
    vec![
        (live[4].clone(), 25 * 60_000),
        (
            Notice { image_url: Some(art(POPKIN)), item: Some(popkin(1187)), ..base(NoticeKind::Bought, "You bought Popkins #1187 for 58 SUI", Some("From a listing on rare.market"), item_action()) },
            3 * 3_600_000,
        ),
        (
            Notice {
                image_url: Some(art(POPKIN)),
                ..base(NoticeKind::Airdrop, "Airdrop: Popkins Pass", Some("Minted to you by 0x55aa...0e12"), NoticeAction::Collection { slug: "popkins".into() })
            },
            26 * 3_600_000,
        ),
        (Notice { addr: None, digest: None, ..base(NoticeKind::PriceAlert, "SUI is below $0.90", Some("Your alert at $0.90 was hit. Now $0.89."), coin_action()) }, 2 * 86_400_000),
    ]
}

/// A sample with its id and times: recorded `ago` ms before `now`.
pub fn stamp(mut n: Notice, id: u64, now: i64, ago: i64) -> Notice {
    let t = now - ago;
    n.id = id.to_string();
    n.cp_ts = Some(t - 480);
    n.created_at = t - 120;
    n.sent_at = t - 40;
    n
}

/// What `recent` answers: newest first, ids growing with time (1001 is the oldest).
pub fn recent(now: i64) -> Vec<Notice> {
    let old = old_samples();
    let len = old.len() as u64;
    old.into_iter().enumerate().map(|(i, (n, ago))| stamp(n, 1000 + len - i as u64, now, ago)).collect()
}

/// The `i`th live notice the mock stream sends, stamped at `now` (ids after `recent`'s).
pub fn live(i: usize, now: i64) -> Notice {
    let s = live_samples();
    stamp(s[i % s.len()].clone(), 1005 + i as u64, now, 0)
}

pub fn alerts(now: i64) -> Vec<PriceAlert> {
    vec![PriceAlert { id: "a1".into(), coin_type: SUI.into(), symbol: "SUI".into(), dir: AlertDir::Above, target: 1.1, created_at: now - 3_600_000, fired_at: None, active: true }]
}

/// The SSE lines of a mock stream: hello, then `count` live notices.
pub fn stream_lines(now: i64, count: usize) -> Vec<String> {
    let mut out = vec!["retry: 3000".to_string(), String::new(), "event: hello".into(), format!("data: {}", json!({ "now": now, "addrs": [ME] })), String::new()];
    for i in 0..count {
        let n = live(i, now);
        out.push(format!("id: {}", n.id));
        out.push("event: notice".into());
        out.push(format!("data: {}", serde_json::to_string(&n).unwrap_or_default()));
        out.push(String::new());
    }
    out
}

/// A `MockNet` answering the notify API with these samples: `recent`, the alerts (adding answers
/// a new id, deleting succeeds) and one stream connection with every live sample.
pub fn net(now: i64) -> MockNet {
    let net = MockNet::new();
    let base = "https://rare.market/v1/notify";
    net.on_call(Method::Get, &format!("{base}/recent"), 200, json!({ "data": recent(now) }).to_string());
    net.on_call(Method::Get, &format!("{base}/alerts"), 200, json!({ "data": alerts(now) }).to_string());
    net.on_call(Method::Post, &format!("{base}/alerts"), 200, json!({ "id": format!("a{now}") }).to_string());
    net.on_call(Method::Delete, &format!("{base}/alerts/"), 200, json!({ "ok": true }).to_string());
    let lines = stream_lines(now, live_samples().len());
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    net.on_stream(&format!("{base}/stream"), &refs);
    net
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api;

    #[test]
    fn samples_round_trip_through_the_api() {
        let now = 1_800_000_000_000;
        let n = net(now);
        let r = api::recent(&n, &[ME.to_string()], "d", 50).unwrap();
        assert_eq!(r.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["1004", "1003", "1002", "1001"]);
        assert_eq!(r[0].created_at, now - 25 * 60_000 - 120);
        assert_eq!(api::list_alerts(&n, "d").unwrap(), alerts(now));
        assert_eq!(live(0, now).kind, NoticeKind::Sold);
        assert_eq!(live(5, now).id, "1010");
    }
}
