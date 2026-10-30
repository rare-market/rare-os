//! Friends running in the background: the account follows Rare Wallet's selected account, presence
//! goes out as a heartbeat (every 30 s and on every game change), friend events from the notify
//! stream update the store and become notices (friend request, came online, party invite, party
//! message), and a host's party follows the game they are playing.
//!
//! No timers here. The host calls [`Live::tick`] with the time (on a timer of its own, at
//! [`Live::next_wake`] or sooner) and does what [`Due`] says; stream events go through
//! [`Live::on_social`], which returns the notice or toast to show. Wallet account changes call
//! `Friends::sync_account` directly, as the web launcher's wallet and storage listeners do.

use crate::api::goodbye_call;
use crate::display::label;
use crate::join::{running_ref, Invites, LibraryGame};
use crate::store::{Applied, Friends, FriendsStatus};
use crate::types::*;
use rare_market::net::Call;
use serde::Serialize;
use std::collections::HashMap;

pub const BEAT_MS: i64 = 30_000;
/// No input for this long with the launcher in the background: away.
pub const AWAY_MS: i64 = 5 * 60_000;
/// One "came online" per friend per 15 minutes.
pub const ONLINE_NOTICE_MS: i64 = 15 * 60_000;
/// A busy chat makes one notice a minute, not one per line.
pub const CHAT_NOTICE_MS: i64 = 60_000;
/// The stream token lasts 12 hours on the server: reconnect with a fresh one every 6.
pub const STREAM_REFRESH_MS: i64 = 6 * 3_600_000;
/// A slow safety net for anything the stream missed while offline.
pub const SAFETY_REFRESH_MS: i64 = 5 * 60_000;
/// Accounts added without the wallet store (first run): check again soon after start, then slowly.
pub const SYNC_SOON_MS: [i64; 2] = [800, 2500];

/// How busy the launcher is (lib/idle.ts): the account poll slows when idle and away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Active,
    Idle,
    Away,
}

impl Activity {
    /// The account poll's interval in this state.
    pub fn sync_every(&self) -> i64 {
        match self {
            Activity::Active => 15_000,
            Activity::Idle => 120_000,
            Activity::Away => 300_000,
        }
    }
}

/// What the host should do now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Due {
    /// Call [`Live::beat`].
    pub beat: bool,
    /// Call `Friends::sync_account` with the wallet's selected account.
    pub sync_account: bool,
    /// Call `Friends::refresh`.
    pub refresh: bool,
    /// Reconnect the notify stream with a fresh `social` token (`Friends::stream_token`).
    pub refresh_stream: bool,
}

/// What the heartbeat needs to know about the launcher.
#[derive(Clone, Copy, Debug)]
pub struct BeatInput<'a> {
    /// Run ids of what is running, first first.
    pub running: &'a [String],
    pub library: &'a [LibraryGame],
    pub invites: &'a Invites,
    /// Whether the launcher window has focus.
    pub focused: bool,
    /// The profile picture (NFT object id and image URL), if one is picked.
    pub avatar: Option<(&'a str, &'a str)>,
    /// When it was picked (epoch ms), so an older pick never replaces a newer one made elsewhere.
    pub avatar_at: Option<i64>,
    /// The Steam account signed in here (SteamID64): a Steam game carries it so friends can join.
    pub steam_id: Option<&'a str>,
}

/// Where a notice leads when opened.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum NoticeAction {
    Friends { address: String },
    Party {
        #[serde(rename = "partyId")]
        party_id: String,
    },
}

/// A local notice for the notice system (`notifyLocal`).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalNotice {
    /// `friend_request`, `friend_online`, `party_invite` or `party_message`.
    pub kind: &'static str,
    pub title: String,
    pub body: Option<String>,
    pub image_url: Option<String>,
    pub action: NoticeAction,
}

/// What a stream event shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Notice(LocalNotice),
    /// A plain toast in the launcher.
    Toast(String),
}

/// The last picture sent, compared the way the web launcher does (`av?.imageUrl !== lastAvatar`,
/// where nothing sent yet is `undefined` and no picture is `null`).
#[derive(Clone, Debug, PartialEq, Eq)]
enum LastAvatar {
    Unsent,
    Sent(Option<String>),
}

/// "online" while a game runs; "away" when the launcher is in the background and untouched for
/// five minutes.
pub fn presence_state(running: bool, focused: bool, last_active: i64, now: i64) -> PresenceState {
    if running {
        return PresenceState::Online;
    }
    if !focused && now - last_active > AWAY_MS {
        PresenceState::Away
    } else {
        PresenceState::Online
    }
}

/// The picture that may go up: an NFT (object id, https image) or a Rare default (its id, the
/// rare.market URL; rare.market keeps only the id and serves its own copy).
pub fn avatar_now(avatar: Option<(&str, &str)>) -> Option<Avatar> {
    let (id, url) = avatar?;
    let nft = id.strip_prefix("0x").or_else(|| id.strip_prefix("0X")).is_some_and(|hex| hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()));
    let default = (2..=24).contains(&id.len()) && id.starts_with(|c: char| c.is_ascii_lowercase()) && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && url.starts_with("https://rare.market/avatars/");
    ((nft || default) && url.starts_with("https://")).then(|| Avatar { id: id.to_string(), image_url: url.to_string(), at: None })
}

/// The background side of friends: heartbeat, polls and notice throttles.
pub struct Live {
    started_at: i64,
    next_beat: i64,
    last_active: i64,
    last_sent_game: Option<String>,
    last_avatar: LastAvatar,
    online_at: HashMap<String, i64>,
    chat_at: HashMap<String, i64>,
    last_sync: i64,
    soon_done: usize,
    last_stream: i64,
    last_safety: i64,
    seen_key_rev: Option<u64>,
    seen_ready: bool,
    seen_running: Option<Vec<String>>,
}

impl Live {
    /// Starts friends (the web launcher's `startFriends`): the account sync and a heartbeat are due
    /// at once.
    pub fn new(now: i64) -> Live {
        Live {
            started_at: now,
            next_beat: now,
            last_active: now,
            last_sent_game: None,
            last_avatar: LastAvatar::Unsent,
            online_at: HashMap::new(),
            chat_at: HashMap::new(),
            last_sync: i64::MIN,
            soon_done: 0,
            last_stream: now,
            last_safety: now,
            seen_key_rev: None,
            seen_ready: false,
            seen_running: None,
        }
    }

    /// A key, pointer press or focus: the player is here (not away).
    pub fn input(&mut self, now: i64) {
        self.last_active = now;
    }

    /// What is due at `now`. Call it on a timer and whenever something may have changed (a game
    /// started or stopped, friends turned on). Each true flag is consumed: the next tick is false
    /// until it is due again.
    pub fn tick(&mut self, f: &Friends, running: &[String], activity: Activity, now: i64) -> Due {
        let mut d = Due::default();
        // The linked key changed: reconnect the stream with the new token and beat now.
        let key_rev = f.key_rev();
        if self.seen_key_rev != Some(key_rev) {
            if self.seen_key_rev.is_some() {
                d.refresh_stream = true;
                self.last_stream = now;
                d.beat = true;
            }
            self.seen_key_rev = Some(key_rev);
        }
        // Friends came ready: beat.
        let ready = f.read(|s| s.status == FriendsStatus::Ready);
        if ready && !self.seen_ready {
            d.beat = true;
        }
        self.seen_ready = ready;
        // A game started or stopped: tell friends now, not at the next beat.
        if self.seen_running.as_deref().is_some_and(|r| r != running) {
            d.beat = true;
        }
        self.seen_running = Some(running.to_vec());
        if now >= self.next_beat {
            d.beat = true;
        }
        if d.beat {
            // The beat schedules the next one; until the host runs it, do not ask twice.
            self.next_beat = now + BEAT_MS;
        }
        // The account: at start, twice soon after, then on the activity's cadence.
        if self.last_sync == i64::MIN {
            d.sync_account = true;
        }
        while self.soon_done < SYNC_SOON_MS.len() && now >= self.started_at + SYNC_SOON_MS[self.soon_done] {
            self.soon_done += 1;
            d.sync_account = true;
        }
        if now.saturating_sub(self.last_sync) >= activity.sync_every() {
            d.sync_account = true;
        }
        if d.sync_account {
            self.last_sync = now;
        }
        if now - self.last_stream >= STREAM_REFRESH_MS {
            self.last_stream = now;
            if f.current_key().is_some() {
                d.refresh_stream = true;
            }
        }
        if now - self.last_safety >= SAFETY_REFRESH_MS {
            self.last_safety = now;
            d.refresh = true;
        }
        d
    }

    /// When the next tick has something to do (ms), at the latest.
    pub fn next_wake(&self, activity: Activity) -> i64 {
        let mut t = self.next_beat.min(self.last_safety + SAFETY_REFRESH_MS).min(self.last_stream + STREAM_REFRESH_MS);
        if self.last_sync != i64::MIN {
            t = t.min(self.last_sync + activity.sync_every());
        }
        if let Some(ms) = SYNC_SOON_MS.get(self.soon_done) {
            t = t.min(self.started_at + ms);
        }
        t
    }

    /// Sends one heartbeat now (blocking) and schedules the next. Returns my presence as friends
    /// see it, when the beat went through.
    pub fn beat(&mut self, f: &Friends, input: &BeatInput, now: i64) -> Option<Presence> {
        self.next_beat = now + BEAT_MS;
        let k = f.current_key()?;
        if f.read(|s| s.status != FriendsStatus::Ready) {
            return None;
        }
        let party = f.read(|s| s.active_party().map(|p| p.id.clone()));
        let mut game = running_ref(input.running, input.library, party.as_deref(), input.invites);
        if let Some(g) = game.as_mut().filter(|g| g.kind == GameKind::Steam) {
            g.steam_id = input.steam_id.map(str::to_string);
        }
        let av = avatar_now(input.avatar).map(|a| Avatar { at: input.avatar_at, ..a });
        let av_url = av.as_ref().map(|a| a.image_url.clone());
        // The picture goes up only when it changes.
        let same = match (&av_url, &self.last_avatar) {
            (None, LastAvatar::Unsent) => true,
            (Some(u), LastAvatar::Sent(Some(l))) => u == l,
            _ => false,
        };
        let beat = Beat { state: presence_state(!input.running.is_empty(), input.focused, self.last_active, now), game: game.clone(), avatar: if same { None } else { Some(av) } };
        let me = match f.api().heartbeat(&k, &beat) {
            Ok(me) => {
                self.last_avatar = LastAvatar::Sent(av_url);
                f.set_my_presence(me.clone());
                Some(me)
            }
            // offline: the next beat tries again
            Err(_) => None,
        };
        self.follow_game(f, game.as_ref());
        me
    }

    /// The host's party shows the game the host is playing.
    fn follow_game(&mut self, f: &Friends, game: Option<&GameRef>) {
        let id = game.map(|g| g.id.clone());
        if id == self.last_sent_game {
            return;
        }
        self.last_sent_game = id;
        let Some(game) = game else { return };
        let party = f.read(|s| s.active_party().filter(|p| s.address.as_deref() == Some(p.host.as_str())).map(|p| (p.id.clone(), p.game.as_ref().map(|g| g.id.clone()))));
        let Some((party_id, party_game)) = party else { return };
        if party_game.as_deref() == Some(game.id.as_str()) {
            return;
        }
        let _ = f.set_party_game(&party_id, Some(game));
    }

    /// One friend event from the notify stream: updates the store and returns what to show.
    pub fn on_social(&mut self, f: &Friends, e: &SocialEvent, now: i64) -> Option<Outcome> {
        let r = f.apply(e)?;
        self.outcome(&r, now)
    }

    /// The notice or toast for an applied event (with the per-friend and per-chat throttles).
    pub fn outcome(&mut self, r: &Applied, now: i64) -> Option<Outcome> {
        match r {
            Applied::Request { from, address } => Some(Outcome::Notice(LocalNotice {
                kind: "friend_request",
                title: format!("{} wants to be friends", label(from.as_ref())),
                body: Some("Accept or decline in Friends.".into()),
                image_url: from.as_ref().and_then(|p| p.avatar_url.clone()),
                action: NoticeAction::Friends { address: address.clone() },
            })),
            Applied::Accepted { who, .. } => Some(Outcome::Toast(format!("{} is now your friend", label(who.as_ref())))),
            Applied::Online { friend, game } => {
                if now - self.online_at.get(&friend.address).copied().unwrap_or(0) < ONLINE_NOTICE_MS {
                    return None;
                }
                self.online_at.insert(friend.address.clone(), now);
                let name = label(Some(friend));
                Some(Outcome::Notice(LocalNotice {
                    kind: "friend_online",
                    title: match game {
                        Some(g) => format!("{name} is playing {}", g.title),
                        None => format!("{name} is online"),
                    },
                    body: None,
                    image_url: friend.avatar_url.clone(),
                    action: NoticeAction::Friends { address: friend.address.clone() },
                }))
            }
            Applied::Invite { party, from } => {
                let by = party.members.iter().find(|m| Some(&m.address) == from.as_ref()).or_else(|| party.members.iter().find(|m| m.address == party.host));
                Some(Outcome::Notice(LocalNotice {
                    kind: "party_invite",
                    title: format!("{} invited you to a party", label(by.map(|m| &m.person))),
                    body: Some(match &party.game {
                        Some(g) => format!("Playing {}", g.title),
                        None => format!("{} players", party.members.len()),
                    }),
                    image_url: by.and_then(|m| m.avatar_url.clone()),
                    action: NoticeAction::Party { party_id: party.id.clone() },
                }))
            }
            Applied::Dm { message, from } => {
                if now - self.chat_at.get(&message.from).copied().unwrap_or(0) < CHAT_NOTICE_MS {
                    return None;
                }
                self.chat_at.insert(message.from.clone(), now);
                let name = label(from.as_ref());
                let title = match (&message.attach, message.text.trim()) {
                    (Some(a), "") => format!("{name} sent you a {}", if a.kind == "clip" { "clip" } else { "screenshot" }),
                    (_, t) => format!("{name}: {t}"),
                };
                Some(Outcome::Notice(LocalNotice {
                    kind: "direct_message",
                    title,
                    body: Some("Message".into()),
                    image_url: from.as_ref().and_then(|p| p.avatar_url.clone()),
                    action: NoticeAction::Friends { address: message.from.clone() },
                }))
            }
            Applied::Message { message, party } => {
                if now - self.chat_at.get(&message.party_id).copied().unwrap_or(0) < CHAT_NOTICE_MS {
                    return None;
                }
                self.chat_at.insert(message.party_id.clone(), now);
                let by = party.as_ref().and_then(|p| p.members.iter().find(|m| m.address == message.from));
                Some(Outcome::Notice(LocalNotice {
                    kind: "party_message",
                    title: format!("{}: {}", label(by.map(|m| &m.person)), message.text),
                    body: Some(match party.as_ref().and_then(|p| p.game.as_ref()) {
                        Some(g) => format!("Party, {}", g.title),
                        None => "Party chat".into(),
                    }),
                    image_url: by.and_then(|m| m.avatar_url.clone()),
                    action: NoticeAction::Party { party_id: message.party_id.clone() },
                }))
            }
        }
    }
}

/// Tells friends we left (best effort, also on window close): the signed call to fire and forget,
/// or None when friends is off or on sample data.
pub fn goodbye(f: &Friends, now: i64, origin: &str) -> Option<Call> {
    if f.api().is_mock() {
        return None;
    }
    f.current_key().map(|k| goodbye_call(origin, &k, now))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Remote;
    use crate::sign::{device_key, mark_linked, LINK_MS};
    use crate::storage::MemoryStorage;
    use crate::store::Account;
    use rare_market::net::{Method, MockNet};
    use std::sync::Arc;

    const NOW: i64 = 1_800_000_000_000;
    fn clock() -> i64 {
        NOW
    }
    fn me() -> String {
        format!("0x{}", "a".repeat(64))
    }

    fn ready(net: &MockNet) -> Friends {
        let keys = Arc::new(MemoryStorage::new());
        let mut k = device_key(&*keys, &me());
        mark_linked(&*keys, &mut k, NOW + LINK_MS);
        let mut remote = Remote::new(Arc::new(net.clone()), Arc::new(crate::api::NoWallet));
        remote.clock = clock;
        net.on_call(Method::Get, "https://rare.market/v1/friends", 200, format!(r#"{{"me":{{"address":"{}","settings":{{"showPlaying":true,"appearOffline":false,"visibility":"friends"}},"presence":{{"state":"online","game":null,"since":1,"lastSeen":1}}}}}}"#, me()));
        let f = Friends::new(Arc::new(remote), keys, Arc::new(MemoryStorage::new())).with_clock(clock);
        f.sync_account(Some(&Account { address: me(), can_sign: true }));
        assert_eq!(f.snapshot().status, FriendsStatus::Ready);
        f
    }

    #[test]
    fn presence_goes_away_only_in_the_background_and_untouched() {
        assert_eq!(presence_state(true, false, 0, NOW), PresenceState::Online);
        assert_eq!(presence_state(false, true, 0, NOW), PresenceState::Online);
        assert_eq!(presence_state(false, false, NOW - AWAY_MS - 1, NOW), PresenceState::Away);
        assert_eq!(presence_state(false, false, NOW - 1000, NOW), PresenceState::Online);
        assert!(avatar_now(Some((&format!("0x{}", "1".repeat(64)), "https://x/y.png"))).is_some());
        assert!(avatar_now(Some(("0x12", "https://x/y.png"))).is_none());
        assert!(avatar_now(Some((&format!("0x{}", "1".repeat(64)), "http://x/y.png"))).is_none());
        assert!(avatar_now(Some(("fox", "https://rare.market/avatars/512/fox.png"))).is_some());
        assert!(avatar_now(Some(("fox", "https://x/fox.png"))).is_none());
    }

    #[test]
    fn ticks_say_what_is_due() {
        let net = MockNet::new();
        let f = ready(&net);
        let mut l = Live::new(NOW);
        let d = l.tick(&f, &[], Activity::Active, NOW);
        assert!(d.beat && d.sync_account && !d.refresh && !d.refresh_stream);
        assert_eq!(l.tick(&f, &[], Activity::Active, NOW + 10), Due::default());
        assert!(l.tick(&f, &[], Activity::Active, NOW + 800).sync_account);
        // A game started: beat now.
        assert!(l.tick(&f, &["web:act".into()], Activity::Active, NOW + 900).beat);
        assert!(l.tick(&f, &["web:act".into()], Activity::Active, NOW + 2500).sync_account);
        assert!(!l.tick(&f, &["web:act".into()], Activity::Away, NOW + 20_000).sync_account);
        assert!(l.tick(&f, &["web:act".into()], Activity::Away, NOW + 300_000).refresh);
        // The key changed (turned off): the stream reconnects.
        f.turn_off();
        assert!(l.tick(&f, &["web:act".into()], Activity::Active, NOW + 300_001).refresh_stream);
        assert!(l.next_wake(Activity::Active) > NOW + 300_001);
    }

    #[test]
    fn beats_with_the_running_game_and_the_picture_once() {
        let net = MockNet::new();
        let f = ready(&net);
        net.on_call(Method::Post, "https://rare.market/v1/presence", 200, r#"{"state":"online","game":null,"since":5,"lastSeen":5}"#);
        let lib = vec![LibraryGame { id: "web:act".into(), source: "web".into(), title: "ACT".into(), launch: Some(crate::join::Launch { kind: "web".into(), url: Some("https://play.actuniverse.com/".into()), appid: None }) }];
        let inv = Invites::default();
        let id = format!("0x{}", "1".repeat(64));
        let running = vec!["web:act".to_string()];
        let input = BeatInput { running: &running, library: &lib, invites: &inv, focused: false, avatar: Some((&id, "https://img/1.png")), avatar_at: Some(1), steam_id: None };
        let mut l = Live::new(NOW);
        assert_eq!(l.beat(&f, &input, NOW).unwrap().since, Some(5));
        assert_eq!(f.snapshot().me.unwrap().presence.since, Some(5));
        l.beat(&f, &input, NOW + BEAT_MS);
        let bodies: Vec<serde_json::Value> = net.calls().iter().filter(|c| c.url.ends_with("/v1/presence")).map(|c| serde_json::from_str(c.body.as_deref().unwrap()).unwrap()).collect();
        assert_eq!(bodies[0]["game"]["id"], "web:act");
        assert_eq!(bodies[0]["avatar"]["imageUrl"], "https://img/1.png");
        assert!(bodies[1].get("avatar").is_none());
        assert_eq!(goodbye(&f, NOW, "https://rare.market").unwrap().body.as_deref(), Some(r#"{"state":"offline","game":null}"#));
    }

    #[test]
    fn notices_are_throttled_per_friend_and_per_chat() {
        let mut l = Live::new(NOW);
        let friend = Friend { person: Person { address: "0xb".into(), name: Some("mira".into()), ..Person::default() }, since: 1, presence: Presence::default() };
        let on = Applied::Online { friend, game: Some(GameRef::web("web:act", "ACT", "https://a/")) };
        match l.outcome(&on, NOW) {
            Some(Outcome::Notice(n)) => assert_eq!((n.kind, n.title.as_str()), ("friend_online", "mira is playing ACT")),
            o => panic!("{o:?}"),
        }
        assert_eq!(l.outcome(&on, NOW + 60_000), None);
        assert!(l.outcome(&on, NOW + ONLINE_NOTICE_MS).is_some());
        let m = Applied::Message { message: Message { id: "1".into(), party_id: "p".into(), from: "0xb".into(), text: "ready?".into(), at: 1 }, party: None };
        match l.outcome(&m, NOW) {
            Some(Outcome::Notice(n)) => {
                assert_eq!((n.title.as_str(), n.body.as_deref()), ("Someone: ready?", Some("Party chat")));
                assert_eq!(serde_json::to_value(&n.action).unwrap(), serde_json::json!({ "kind": "party", "partyId": "p" }));
            }
            o => panic!("{o:?}"),
        }
        assert_eq!(l.outcome(&m, NOW + 59_000), None);
        assert_eq!(l.outcome(&Applied::Accepted { who: None, address: "0xb".into() }, NOW), Some(Outcome::Toast("Someone is now your friend".into())));
    }
}
