//! rare.market friends API types (contract: market api/src/friends, /v1/friends, /v1/presence,
//! /v1/players). A player is their Sui address, shown by username or SuiNS name and NFT picture.
//! Field names follow the JSON on the wire (camelCase), so these parse the server's answers as is.

use serde::{Deserialize, Serialize};
use std::ops::Deref;

pub const FRIENDS_BASE: &str = "https://rare.market/v1";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum GameKind {
    Web,
    Steam,
    #[default]
    Other,
}

/// What someone is playing: the launcher's game id, its title, and how to start it elsewhere.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct GameRef {
    pub id: String,
    pub title: String,
    pub kind: GameKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appid: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// An invite value a web game takes in its URL, when the game supports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join: Option<String>,
    /// A Steam game: the player's SteamID64, so friends can join their lobby through Steam.
    #[serde(default, rename = "steamId", skip_serializing_if = "Option::is_none")]
    pub steam_id: Option<String>,
    /// A Steam lobby friends can join (`steam://joinlobby/<appid>/<lobby>/<steamId>`), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lobby: Option<String>,
}

impl GameRef {
    pub fn web(id: &str, title: &str, url: &str) -> GameRef {
        GameRef { id: id.into(), title: title.into(), kind: GameKind::Web, url: Some(url.into()), ..GameRef::default() }
    }
    pub fn steam(id: &str, title: &str, appid: u64) -> GameRef {
        GameRef { id: id.into(), title: title.into(), kind: GameKind::Steam, appid: Some(appid), ..GameRef::default() }
    }
    pub fn other(id: &str, title: &str) -> GameRef {
        GameRef { id: id.into(), title: title.into(), kind: GameKind::Other, ..GameRef::default() }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum PresenceState {
    Online,
    Away,
    #[default]
    Offline,
}

impl PresenceState {
    pub fn as_str(&self) -> &'static str {
        match self {
            PresenceState::Online => "online",
            PresenceState::Away => "away",
            PresenceState::Offline => "offline",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Presence {
    pub state: PresenceState,
    #[serde(default)]
    pub game: Option<GameRef>,
    #[serde(default)]
    pub since: Option<i64>,
    #[serde(default)]
    pub last_seen: Option<i64>,
}

/// Anyone the friends API names: an address with what to show for it. Also the identity the
/// display rule works on (`display::shown`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub address: String,
    /// What to show: the Rare username, else the SuiNS name.
    #[serde(default)]
    pub name: Option<String>,
    /// The Rare username, picked by the player.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// The default SuiNS name, when the player also owns one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suins: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

impl Person {
    /// Just an address (what the server sends for someone with no name or picture).
    pub fn bare(address: &str) -> Person {
        Person { address: address.into(), ..Person::default() }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Friend {
    #[serde(flatten)]
    pub person: Person,
    pub since: i64,
    pub presence: Presence,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    #[serde(flatten)]
    pub person: Person,
    pub at: i64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    #[default]
    Friends,
    Everyone,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Privacy {
    pub show_playing: bool,
    pub appear_offline: bool,
    pub visibility: Visibility,
}

impl Default for Privacy {
    fn default() -> Self {
        Privacy { show_playing: true, appear_offline: false, visibility: Visibility::Friends }
    }
}

/// Some privacy settings to change (`Partial<Privacy>`); unset fields are left out of the body.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_playing: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appear_offline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,
}

impl Privacy {
    /// These settings with a patch laid over them.
    pub fn with(&self, p: &PrivacyPatch) -> Privacy {
        Privacy {
            show_playing: p.show_playing.unwrap_or(self.show_playing),
            appear_offline: p.appear_offline.unwrap_or(self.appear_offline),
            visibility: p.visibility.unwrap_or(self.visibility),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MemberState {
    Invited,
    Joined,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PartyMember {
    #[serde(flatten)]
    pub person: Person,
    pub state: MemberState,
    pub presence: Presence,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Party {
    pub id: String,
    pub host: String,
    #[serde(default)]
    pub game: Option<GameRef>,
    pub created_at: i64,
    pub members: Vec<PartyMember>,
}

impl Party {
    /// Whether `address` is in this party with the given state.
    pub fn has(&self, address: &str, state: MemberState) -> bool {
        self.members.iter().any(|m| m.address == address && m.state == state)
    }
    pub fn joined(&self) -> usize {
        self.members.iter().filter(|m| m.state == MemberState::Joined).count()
    }
    pub fn invited(&self) -> usize {
        self.members.iter().filter(|m| m.state == MemberState::Invited).count()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub party_id: String,
    pub from: String,
    pub text: String,
    pub at: i64,
}

/// A shared screenshot or clip sent in a message: its kind and the share link.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Attach {
    /// "screenshot" or "clip".
    pub kind: String,
    pub url: String,
}

/// A direct message between two friends (`/v1/friends/dms`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DirectMessage {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub attach: Option<Attach>,
    pub at: i64,
    #[serde(default)]
    pub read: bool,
}

impl DirectMessage {
    /// The other side of the conversation, seen from `me`.
    pub fn peer(&self, me: &str) -> &str {
        if self.from == me {
            &self.to
        } else {
            &self.from
        }
    }
}

/// One conversation: the friend, the last message and how many are unread.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub address: String,
    pub last: DirectMessage,
    #[serde(default)]
    pub unread: u32,
}

/// A friend seen in the same game at the same time.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlayedWith {
    pub address: String,
    pub game: GameRef,
    pub at: i64,
}

/// The other device read a conversation (`dm_read`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DmReadEvent {
    pub address: String,
    #[serde(default)]
    pub at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    pub address: String,
    pub settings: Privacy,
    pub presence: Presence,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub me: Me,
    #[serde(default)]
    pub friends: Vec<Friend>,
    #[serde(default)]
    pub received: Vec<Pending>,
    #[serde(default)]
    pub sent: Vec<Pending>,
    #[serde(default)]
    pub blocked: Vec<Pending>,
    #[serde(default)]
    pub parties: Vec<Party>,
    /// Conversations, newest first (servers before direct messages leave it out).
    #[serde(default)]
    pub dms: Vec<Conversation>,
    #[serde(default)]
    pub played_with: Vec<PlayedWith>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FriendChange {
    Request,
    Accepted,
    Cancelled,
    Removed,
    Sync,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PartyChange {
    Invite,
    Update,
    Game,
    Closed,
    Left,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PresenceEvent {
    pub address: String,
    pub presence: Presence,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct FriendEvent {
    pub change: FriendChange,
    pub address: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PartyEvent {
    pub change: PartyChange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub party: Option<Party>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

/// Live events on the notify stream (`?social=` token). On the wire each is an SSE event named
/// `presence`, `friend`, `party`, `chat`, `dm` or `dm_read` whose data is the JSON payload.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "event", content = "data", rename_all = "lowercase")]
pub enum SocialEvent {
    Presence(PresenceEvent),
    Friend(FriendEvent),
    Party(PartyEvent),
    Chat(Message),
    Dm(DirectMessage),
    #[serde(rename = "dm_read")]
    DmRead(DmReadEvent),
}

impl SocialEvent {
    /// The SSE event names that carry friend events (the notify stream hands these to friends).
    pub const NAMES: [&'static str; 6] = ["presence", "friend", "party", "chat", "dm", "dm_read"];

    /// One SSE event (name and data line) as a social event; None for other events or bad data.
    pub fn parse(event: &str, data: &str) -> Option<SocialEvent> {
        let data: serde_json::Value = serde_json::from_str(data).ok()?;
        serde_json::from_value(serde_json::json!({ "event": event, "data": data })).ok()
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GraphOp {
    Request,
    Accept,
    Decline,
    Cancel,
    Remove,
    Block,
    Unblock,
}

impl GraphOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            GraphOp::Request => "request",
            GraphOp::Accept => "accept",
            GraphOp::Decline => "decline",
            GraphOp::Cancel => "cancel",
            GraphOp::Remove => "remove",
            GraphOp::Block => "block",
            GraphOp::Unblock => "unblock",
        }
    }
}

/// The picture that goes up with a heartbeat (a Rare default or an NFT the player picked), with
/// when it was picked: rare.market keeps the newest pick (one made on the site later wins).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Avatar {
    pub id: String,
    pub image_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<i64>,
}

/// One heartbeat's body. `avatar` is left out when the picture has not changed, and is `null`
/// when the player has none.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Beat {
    pub state: PresenceState,
    pub game: Option<GameRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<Option<Avatar>>,
}

/// What `/v1/friends/<op>` answers.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct GraphState {
    #[serde(default)]
    pub state: Option<String>,
}

/// A friend code and its QR page (`POST /v1/friends/codes`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FriendCode {
    pub code: String,
    pub url: String,
    pub expires_at: i64,
}

/// Who a redeemed code belonged to.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Redeemed {
    pub address: String,
}

macro_rules! person_deref {
    ($($t:ty),*) => {$(
        impl Deref for $t {
            type Target = Person;
            fn deref(&self) -> &Person {
                &self.person
            }
        }
    )*};
}
person_deref!(Friend, Pending, PartyMember);
