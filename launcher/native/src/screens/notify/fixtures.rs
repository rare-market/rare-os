//! Sample data for the notify screens: the web mock's notices (notify/mock.ts) and friends.

use super::data::downloads::Download;
use super::data::types::*;
use super::ui::ArtSource;
use rare_render::Src;

pub static ART_A: &[u8] = include_bytes!("../../../../render/assets/games/act/cover.jpg");

const POPKIN: &str = "0x89d249c6a43ff4490902247a950e2e69ba61b74089151166169cfe3fbfea8611";
const ME: &str = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
pub const SUI: &str = "0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI";
const DIGEST: &str = "5QDVv7YPKSPiLA959P8T1nW4phnfHHLg9XuXMP6umQRP";
const SUI_PERP: &str = "0xa97dacdd972a0414363290ec5752cb93ac47443c3e7710df145cc7f337575002";
const POPKIN_ART: &str = "https://storage.claynosaurz.com/popkins/images/popkin";

fn base(id: u64, kind: NoticeKind, title: &str, body: Option<&str>, action: NoticeAction, ago_ms: i64, now: i64) -> Notice {
    let t = now - ago_ms;
    Notice {
        id: id.to_string(),
        kind,
        addr: Some(ME.into()),
        title: title.into(),
        body: body.map(Into::into),
        image_url: None,
        amount: None,
        item: None,
        action,
        digest: Some(DIGEST.into()),
        cp_ts: Some(t - 380),
        created_at: t - 120,
        sent_at: t - 40,
    }
}

fn popkin(n: u32) -> NoticeItem {
    NoticeItem { id: POPKIN.into(), name: format!("Popkins #{n}"), image_url: Some(POPKIN_ART.into()), collection: Some(CollectionRef { slug: "popkins".into(), name: "Popkins".into() }) }
}

fn sui_amount(raw: &str, usd: f64) -> Amount {
    Amount { raw: raw.into(), decimals: 9, symbol: "SUI".into(), coin_type: SUI.into(), usd: Some(usd) }
}

pub fn sold(now: i64, ago: i64) -> Notice {
    let mut n = base(1001, NoticeKind::Sold, "Popkins #412 sold for 70 SUI", Some("Bought by 0x12ab...9f3c on Tradeport"), NoticeAction::Item { item_id: POPKIN.into(), slug: Some("popkins".into()) }, ago, now);
    n.image_url = Some(POPKIN_ART.into());
    n.amount = Some(sui_amount("70000000000", 63.42));
    n.item = Some(popkin(412));
    n
}

pub fn offer(now: i64, ago: i64) -> Notice {
    let mut n = base(1002, NoticeKind::OfferReceived, "New offer of 64 SUI on Popkins #412", Some("From 0x7c1d...a210, ends in 3 days"), NoticeAction::Item { item_id: POPKIN.into(), slug: Some("popkins".into()) }, ago, now);
    n.image_url = Some(POPKIN_ART.into());
    n.item = Some(popkin(412));
    n
}

pub fn price_alert(now: i64, ago: i64) -> Notice {
    let mut n = base(1003, NoticeKind::PriceAlert, "SUI is above $1.00", Some("Your alert at $1.00 was hit. Now $1.01."), NoticeAction::Coin { coin_type: SUI.into() }, ago, now);
    n.addr = None;
    n.digest = None;
    n
}

pub fn liquidation(now: i64, ago: i64) -> Notice {
    let mut n = base(1004, NoticeKind::PerpLiquidation, "SUI 5x long is near liquidation", Some("Mark $0.87, liquidation at $0.81"), NoticeAction::Position { market_id: SUI_PERP.into(), symbol: "SUI".into() }, ago, now);
    n.digest = None;
    n
}

pub fn coin_received(id: u64, now: i64, ago: i64) -> Notice {
    let mut n = base(id, NoticeKind::CoinReceived, "You received 25 SUI", Some("From 0x3f9e...11c0"), NoticeAction::Tx { digest: DIGEST.into() }, ago, now);
    n.amount = Some(sui_amount("25000000000", 22.65));
    n
}

pub fn bought(now: i64, ago: i64) -> Notice {
    let mut n = base(996, NoticeKind::Bought, "You bought Popkins #1187 for 58 SUI", Some("From a listing on rare.market"), NoticeAction::Item { item_id: POPKIN.into(), slug: Some("popkins".into()) }, ago, now);
    n.image_url = Some(POPKIN_ART.into());
    n.item = Some(popkin(1187));
    n
}

pub fn airdrop(now: i64, ago: i64) -> Notice {
    let mut n = base(995, NoticeKind::Airdrop, "Airdrop: Popkins Pass", Some("Minted to you by 0x55aa...0e12"), NoticeAction::Collection { slug: "popkins".into() }, ago, now);
    n.image_url = Some(POPKIN_ART.into());
    n
}

pub fn price_alert_below(now: i64, ago: i64) -> Notice {
    let mut n = base(994, NoticeKind::PriceAlert, "SUI is below $0.90", Some("Your alert at $0.90 was hit. Now $0.89."), NoticeAction::Coin { coin_type: SUI.into() }, ago, now);
    n.addr = None;
    n.digest = None;
    n
}

pub fn party_invite(now: i64) -> Notice {
    let mut n = base(1010, NoticeKind::PartyInvite, "Mira invited you to a party", Some("Playing Word Game"), NoticeAction::Party { party_id: "p1".into() }, 0, now);
    n.digest = None;
    n.addr = None;
    n
}

pub fn friend_request(now: i64, ago: i64) -> Notice {
    let mut n = base(1011, NoticeKind::FriendRequest, "Mira wants to be friends", Some("Accept or decline in Friends."), NoticeAction::Friends { address: None }, ago, now);
    n.digest = None;
    n.addr = None;
    n
}

pub fn battery(level: u8, now: i64) -> Notice {
    let mut n = base(1020 + level as u64, NoticeKind::Battery, super::data::battery::low_battery_text(level), None, NoticeAction::Battery, 0, now);
    n.digest = None;
    n.addr = None;
    n
}

pub fn os_update(now: i64) -> Notice {
    let l = super::data::downloads::os_update_notice(Some("43.2"));
    let mut n = base(1030, l.kind, &l.title, l.body.as_deref(), l.action, 0, now);
    n.digest = None;
    n.addr = None;
    n
}

/// The older notices of the mock (newest first), then the live ones; all in the order the stream
/// would have delivered them (oldest first).
pub fn history(now: i64) -> Vec<Notice> {
    vec![
        price_alert_below(now, 2 * 86_400_000),
        airdrop(now, 26 * 3_600_000),
        bought(now, 3 * 3_600_000),
        coin_received(993, now, 25 * 60_000),
    ]
}

pub fn live(now: i64) -> Vec<Notice> {
    vec![coin_received(1005, now, 9 * 60_000), liquidation(now, 6 * 60_000), price_alert(now, 3 * 60_000), offer(now, 90_000), sold(now, 20_000)]
}

pub fn downloads() -> Vec<Download> {
    vec![Download { id: "1".into(), title: "Hades".into(), done: 4.2e9, total: 9.0e9, paused: false }, Download { id: "2".into(), title: "Celeste".into(), done: 0.0, total: 0.0, paused: false }]
}

/// Bundled art in place of the Popkins image and avatars.
pub struct FixtureArt;
impl ArtSource for FixtureArt {
    fn art(&self, n: &Notice) -> Option<Src> {
        let url = n.image_url.as_deref().or_else(|| n.item.as_ref().and_then(|i| i.image_url.as_deref()))?;
        let _ = url;
        Some(Src::Static("fixtures/act-cover.jpg", ART_A))
    }
}
