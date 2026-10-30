//! What the Friends screens read: the friends state (rare-friends' `FriendsState`), the clock, the
//! library bits Join and the game picker need, the player's own name, and the friend code. Fixtures
//! are the web launcher's `?friends=mock` data (friends/mock.ts) at a fixed clock, as JSON.

use rare_friends::types::{Friend, FriendCode, Me, Message, Party, Pending};
use rare_friends::{FriendsState, FriendsStatus};
use serde::Deserialize;
use std::collections::HashMap;

/// The demo account the web mock signs in as (mock.ts `ME`).
pub const MOCK_ME: &str = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";

/// A game on this device (the parts of the library Friends reads).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LibGame {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub last_played: Option<f64>,
    #[serde(default)]
    pub cover: Vec<String>,
    #[serde(default)]
    pub header: Vec<String>,
    #[serde(default)]
    pub hero: Vec<String>,
}

/// A collectible to send (rare.items: id, name, image, collection name).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ItemCard {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub collection: Option<String>,
}

/// A friend's public profile on rare.market (`GET /v1/profile/<address>`, the web's /u/<address>):
/// counts, trophies by game, games played, and the items they hold (`/v1/accounts/<a>/items`).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct FriendProfile {
    /// Their banner (rare.market's renditions; its pick, else a Rare default by address).
    pub banner: Option<ProfileBanner>,
    pub counts: ProfileCounts,
    pub trophies: Vec<TrophyGroup>,
    pub games: Vec<ProfileGame>,
    /// Their collectibles (None while loading).
    #[serde(skip)]
    pub items: Option<Vec<ItemCard>>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ProfileBanner {
    /// 1500 x 500
    pub url: String,
    /// 750 x 250
    pub url_small: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ProfileCounts {
    pub games: u32,
    pub trophies: u32,
    pub trophy_points: u32,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct TrophyGroup {
    pub game_name: String,
    pub count: u32,
    pub points: u32,
    pub data: Vec<TrophyLine>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct TrophyLine {
    pub name: String,
    pub image_url: String,
    pub tier: String,
    pub points: u32,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ProfileGame {
    pub name: String,
    pub logo_url: String,
    pub trophies: u32,
    pub points: u32,
    pub last_played_at: Option<String>,
}

/// Everything the Friends screens draw from.
#[derive(Clone, Debug, Default)]
pub struct FriendsData {
    pub st: FriendsState,
    /// The clock (ms) presence and dates are worded against.
    pub now_ms: i64,
    pub library: Vec<LibGame>,
    /// Ids of the games running on this device.
    pub running: Vec<String>,
    /// The player's Rare username (None: not picked yet).
    pub my_username: Option<String>,
    /// What the top bar calls this account ("Demo wallet", a SuiNS name, a short address).
    pub my_name: String,
    /// The friend code on the QR panel, once made.
    pub code: Option<FriendCode>,
    /// Collectibles for "Send an item" (None while loading).
    pub items: Option<Vec<ItemCard>>,
    /// The profile of the friend on screen (address, profile; None while loading).
    pub profile: Option<(String, Option<FriendProfile>)>,
    /// How this account approves turning friends on (shown before it is on).
    pub sign_method: Option<String>,
}

/// The fixture JSON: the mock overview plus the clock and chat.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    now: i64,
    address: String,
    me: Me,
    #[serde(default)]
    friends: Vec<Friend>,
    #[serde(default)]
    received: Vec<Pending>,
    #[serde(default)]
    sent: Vec<Pending>,
    #[serde(default)]
    blocked: Vec<Pending>,
    #[serde(default)]
    parties: Vec<Party>,
    #[serde(default)]
    messages: HashMap<String, Vec<Message>>,
}

fn from_json(json: &str) -> FriendsData {
    let f: Fixture = serde_json::from_str(json).expect("friends fixture");
    let st = FriendsState {
        status: FriendsStatus::Ready,
        address: Some(f.address),
        me: Some(f.me),
        friends: f.friends,
        received: f.received,
        sent: f.sent,
        blocked: f.blocked,
        parties: f.parties,
        messages: f.messages,
        at: f.now,
        ..Default::default()
    };
    FriendsData {
        st,
        now_ms: f.now,
        my_name: "Demo wallet".into(),
        ..Default::default()
    }
}

/// `?friends=mock`: seven friends, a request, a sent request.
pub fn mock() -> FriendsData {
    from_json(include_str!("fixtures/mock.json"))
}

/// `?friends=mock&party`: the same with mira's party (you joined, otter invited) and its chat.
pub fn mock_party() -> FriendsData {
    from_json(include_str!("fixtures/mock-party.json"))
}

const LONG_NAME: &str = "the_longest_username_anyone_ever_picked_on_rare_market_so_far";
const LONG_TEXT: &str = "That last run was wild, we should queue again after dinner. I found a route through the second boss that skips the whole bridge section and saves about four minutes if nobody dies, which honestly never happens with us. Bring snacks.";

fn dm(id: usize, from: &str, to: &str, text: String, at: i64) -> rare_friends::types::DirectMessage {
    rare_friends::types::DirectMessage { id: id.to_string(), from: from.into(), to: to.into(), text, attach: None, at, read: true }
}

fn convo(address: &str, last: rare_friends::types::DirectMessage, unread: u32) -> rare_friends::types::Conversation {
    rare_friends::types::Conversation { address: address.into(), last, unread }
}

/// Edge cases for the golden shots: "empty" (no friends), "many" (200 friends, long names, unread
/// up to 99+), "convo" (500 messages with long lines, a loaded profile, a very long name).
pub fn edge(which: &str) -> FriendsData {
    let mut d = mock();
    let me = d.st.address.clone().unwrap_or_default();
    let now = d.now_ms;
    match which {
        "empty" => {
            d.st.friends.clear();
            d.st.received.clear();
            d.st.sent.clear();
            d.st.blocked.clear();
        }
        "many" => {
            let base = d.st.friends.clone();
            d.st.friends = (0..200)
                .map(|i| {
                    let mut f = base[i % base.len()].clone();
                    f.person.address = format!("0x{:064x}", i + 1);
                    let name = if i % 9 == 4 { format!("{LONG_NAME}{i}") } else { format!("{}{i}", f.person.name.clone().unwrap_or_default()) };
                    f.person.name = Some(name.clone());
                    f.person.username = Some(name);
                    f
                })
                .collect();
            for (i, n) in [(0usize, 150u32), (1, 99), (2, 7), (4, 1)] {
                let a = d.st.friends[i].address.clone();
                d.st.convos.push(convo(&a, dm(i, &a, &me, "hey".into(), now), n));
            }
        }
        _ => {
            let mira = d.st.friends[0].address.clone();
            d.st.friends[1].person.name = Some(LONG_NAME.into());
            d.st.friends[1].person.username = Some(LONG_NAME.into());
            let list: Vec<_> = (0..500)
                .map(|i| {
                    let (from, to) = if i % 3 == 0 { (me.as_str(), mira.as_str()) } else { (mira.as_str(), me.as_str()) };
                    let text = match i % 4 {
                        0 => LONG_TEXT.to_string(),
                        1 => format!("message {i}"),
                        2 => "https://rare.market/s/0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".into(),
                        _ => "gg".into(),
                    };
                    dm(i, from, to, text, now - (500 - i as i64) * 60_000)
                })
                .collect();
            let last = list.last().cloned().unwrap();
            d.st.dms.insert(mira.clone(), list);
            d.st.dm_full.insert(mira.clone());
            d.st.convos.push(convo(&mira, last.clone(), 0));
            let k = d.st.friends[1].address.clone();
            d.st.convos.push(convo(&k, dm(9999, &k, &me, LONG_TEXT.into(), now), 120));
            d.st.played_with.push(rare_friends::types::PlayedWith { address: mira.clone(), game: rare_friends::types::GameRef::steam("steam:1145350", "Hades II", 1145350), at: now - 2 * 86_400_000 });
            let games = (0..8).map(|i| ProfileGame { name: if i == 0 { format!("A game with a very long title that keeps going {i}") } else { format!("Game {i}") }, trophies: 3 * i, points: 30 * i, ..Default::default() }).collect();
            let line = |n: &str, t: &str| TrophyLine { name: n.into(), tier: t.into(), points: 90, ..Default::default() };
            let trophies = vec![TrophyGroup { game_name: "Word Game".into(), count: 3, points: 120, data: vec![line("Perfect week of guesses without a single miss, every day", "platinum"), line("First win", "bronze")] }];
            let items = (0..30).map(|i| ItemCard { id: format!("0x{i:x}"), name: format!("Item {i}"), image_url: None, collection: None }).collect();
            // RR_BANNER=<url> shows that banner (shots of uploads) on every friend's profile, else
            // mira's default on hers
            let any = std::env::var("RR_BANNER").ok();
            let url = any.clone().unwrap_or_else(|| crate::banners::url(crate::banners::starter(&mira).0, false));
            let banner = Some(ProfileBanner { url_small: url.clone(), url });
            let mira = if any.is_some() { "*".to_string() } else { mira };
            d.profile = Some((mira, Some(FriendProfile { banner, counts: ProfileCounts { games: 8, trophies: 41, trophy_points: 1230 }, trophies, games, items: Some(items) })));
        }
    }
    d
}

/// Before the list: a status other than ready (no account, watch only, not turned on, ...).
pub fn gate(status: FriendsStatus, error: Option<&str>) -> FriendsData {
    let mut d = FriendsData {
        now_ms: mock().now_ms,
        my_name: "Demo wallet".into(),
        sign_method: Some(crate::screens::wallet::service::METHOD_PASSKEY_PHONE.into()),
        ..Default::default()
    };
    d.st.status = status;
    d.st.error = error.map(str::to_string);
    d
}

/// The mock friend code (mock.ts `code()`): valid 15 minutes from the clock.
pub fn mock_code(now_ms: i64) -> FriendCode {
    FriendCode {
        code: "K7Q4MX".into(),
        url: "https://rare.market/v1/friends/qr/K7Q4MX".into(),
        expires_at: now_ms + 15 * 60_000,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_parse_like_the_web_mock() {
        let d = mock();
        assert_eq!(d.st.friends.len(), 7);
        assert_eq!(d.st.received.len(), 1);
        assert_eq!(d.st.sent.len(), 1);
        assert!(d.st.parties.is_empty());
        let g = d.st.groups();
        assert_eq!(
            (g.playing.len(), g.online.len(), g.offline.len()),
            (3, 2, 2)
        );
        let p = mock_party();
        let party = p.st.active_party().expect("joined party");
        assert_eq!(party.id, "mockparty01");
        assert_eq!(p.st.messages["mockparty01"].len(), 3);
    }
}
