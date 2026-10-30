//! Friends state for the selected Rare Wallet account: the friend list with live presence, requests,
//! parties and their chat. Loaded from /v1/friends, kept live by the notify stream's friend events,
//! refreshed on any graph change.
//!
//! [`FriendsState`] is plain data with pure transitions (what the screens draw from). [`Friends`]
//! wraps it with the device key and the API: its methods block on the network, so the host calls
//! them off the UI thread, and it is shared (`Arc<Friends>`) so the UI can read a
//! [`Friends::snapshot`] (with `busy` set) while a call runs. [`Friends::rev`] changes on every
//! state change, so a screen knows when to redraw.

use crate::api::{Error, FriendsApi, Result};
use crate::display::label;
use crate::sign::{device_key, forget_key, mark_linked, now_ms, DeviceKey, LINK_MS};
use crate::storage::Storage;
use crate::types::*;
use crate::username::Mine;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

fn off_key(address: &str) -> String {
    format!("rare.friends.off.{address}")
}

/// How many chat lines are kept per party.
pub const CHAT_KEEP: usize = 200;
/// How many direct messages are kept per friend in memory (older pages load again on scroll).
pub const DM_KEEP: usize = 5000;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum FriendsStatus {
    /// no Rare Wallet account
    #[default]
    None,
    /// a watch-only account: it cannot sign, so no friends
    Watch,
    /// a signing account that has not turned friends on yet on this device
    Unlinked,
    Linking,
    Loading,
    Ready,
    Error,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct FriendsState {
    pub status: FriendsStatus,
    pub error: Option<String>,
    pub address: Option<String>,
    pub me: Option<Me>,
    pub friends: Vec<Friend>,
    pub received: Vec<Pending>,
    pub sent: Vec<Pending>,
    pub blocked: Vec<Pending>,
    pub parties: Vec<Party>,
    pub messages: HashMap<String, Vec<Message>>,
    pub unread: HashMap<String, u32>,
    /// The party whose chat is on screen (no unread count, no notices for it).
    pub open_party: Option<String>,
    /// Direct messages by friend address, oldest first (what was loaded or arrived).
    pub dms: HashMap<String, Vec<DirectMessage>>,
    /// Conversations, newest first: the last message and the unread count per friend.
    pub convos: Vec<Conversation>,
    /// The friend whose messages are on screen (read as they arrive, no notices).
    pub open_dm: Option<String>,
    /// Conversations whose whole history (30 days) is loaded.
    pub dm_full: std::collections::HashSet<String>,
    /// Friends played with recently, newest first.
    pub played_with: Vec<PlayedWith>,
    pub busy: bool,
    /// When the overview was last loaded (ms); 1 when it came from the cache, 0 never.
    pub at: i64,
}

/// What a stream event changed that a notice could be made from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Applied {
    Online { friend: Friend, game: Option<GameRef> },
    Request { from: Option<Person>, address: String },
    Accepted { who: Option<Person>, address: String },
    Invite { party: Party, from: Option<String> },
    Message { message: Message, party: Option<Party> },
    /// A direct message from a friend (not on screen).
    Dm { message: DirectMessage, from: Option<Person> },
}

/// The Friends screen's three lists.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FriendGroups<'a> {
    pub playing: Vec<&'a Friend>,
    pub online: Vec<&'a Friend>,
    pub offline: Vec<&'a Friend>,
}

impl FriendsState {
    fn me_is(&self, a: &str) -> bool {
        self.address.as_deref() == Some(a)
    }

    /// The party the player is in (joined), if any.
    pub fn active_party(&self) -> Option<&Party> {
        let me = self.address.as_deref()?;
        self.parties.iter().find(|p| p.has(me, MemberState::Joined))
    }

    /// Invites waiting for an answer.
    pub fn party_invites(&self) -> Vec<&Party> {
        let Some(me) = self.address.as_deref() else { return Vec::new() };
        self.parties.iter().filter(|p| p.has(me, MemberState::Invited)).collect()
    }

    /// Playing, online (no game) and offline friends, in list order.
    pub fn groups(&self) -> FriendGroups<'_> {
        let mut g = FriendGroups::default();
        for f in &self.friends {
            match (f.presence.state, &f.presence.game) {
                (PresenceState::Offline, _) => g.offline.push(f),
                (_, Some(_)) => g.playing.push(f),
                (_, None) => g.online.push(f),
            }
        }
        g
    }

    /// Name and picture of anyone we know about (friends, requests, party members).
    pub fn person_of(&self, address: &str) -> Option<&Person> {
        let a = address.to_lowercase();
        self.friends
            .iter()
            .map(|f| &f.person)
            .chain(self.received.iter().map(|f| &f.person))
            .chain(self.sent.iter().map(|f| &f.person))
            .chain(self.parties.iter().flat_map(|p| p.members.iter().map(|m| &m.person)))
            .find(|p| p.address == a)
    }

    /// Username first, else SuiNS name, else a short address.
    pub fn label_of(&self, address: &str) -> String {
        label(self.person_of(address))
    }

    /// A loaded overview replaces the lists.
    pub fn set_overview(&mut self, o: Overview, at: i64) {
        self.me = Some(o.me);
        self.friends = o.friends;
        self.received = o.received;
        self.sent = o.sent;
        self.blocked = o.blocked;
        self.parties = o.parties;
        self.convos = o.dms;
        if let Some(open) = &self.open_dm {
            for c in self.convos.iter_mut().filter(|c| &c.address == open) {
                c.unread = 0;
            }
        }
        self.played_with = o.played_with;
        self.status = FriendsStatus::Ready;
        self.error = None;
        self.at = at;
    }

    /// Another account (or none): the last one's lists, chat and error go. Status, the open chat,
    /// `busy` and `at` stay, as in the web launcher (the caller sets the status next).
    pub fn reset(&mut self, address: Option<String>) {
        *self = FriendsState { status: self.status, open_party: self.open_party.take(), busy: self.busy, at: self.at, address, ..FriendsState::default() };
    }

    /// Unread direct messages from one friend.
    pub fn dm_unread(&self, address: &str) -> u32 {
        self.convos.iter().find(|c| c.address == address).map(|c| c.unread).unwrap_or(0)
    }

    /// Unread direct messages from everyone.
    pub fn dm_unread_total(&self) -> u32 {
        self.convos.iter().map(|c| c.unread).sum()
    }

    /// What Friends' tab counts: unread messages, friend requests and party invites.
    pub fn badge(&self) -> u32 {
        self.dm_unread_total() + self.received.len() as u32 + self.party_invites().len() as u32
    }

    /// Adds one direct message (to the loaded list and the conversation line); false when it is
    /// already there. A message from the friend that is not on screen counts as unread.
    pub fn add_dm(&mut self, m: DirectMessage) -> bool {
        let me = self.address.clone().unwrap_or_default();
        let peer = m.peer(&me).to_string();
        let list = self.dms.entry(peer.clone()).or_default();
        if list.iter().any(|x| x.id == m.id) {
            return false;
        }
        list.push(m.clone());
        if list.len() > DM_KEEP {
            list.drain(..list.len() - DM_KEEP);
            self.dm_full.remove(&peer);
        }
        let unread_add = (m.from != me && self.open_dm.as_deref() != Some(peer.as_str())) as u32;
        let unread = self.convos.iter().find(|c| c.address == peer).map(|c| c.unread).unwrap_or(0) + unread_add;
        self.convos.retain(|c| c.address != peer);
        self.convos.insert(0, Conversation { address: peer, last: m, unread });
        true
    }

    /// The conversation with `address` is on screen (or none is): its count goes to zero.
    pub fn open_dm(&mut self, address: Option<&str>) {
        self.open_dm = address.map(str::to_string);
        if let Some(a) = address {
            for c in self.convos.iter_mut().filter(|c| c.address == a) {
                c.unread = 0;
            }
        }
    }

    fn clear_lists(&mut self) {
        self.me = None;
        self.friends.clear();
        self.received.clear();
        self.sent.clear();
        self.blocked.clear();
        self.parties.clear();
    }

    pub fn upsert_party(&mut self, p: Party) {
        self.parties.retain(|x| x.id != p.id);
        self.parties.insert(0, p);
    }

    pub fn drop_party(&mut self, id: &str) {
        self.parties.retain(|x| x.id != id);
        self.messages.remove(id);
        self.unread.remove(id);
    }

    /// A party made here: it leads, and any other party the player had joined goes.
    pub fn party_created(&mut self, p: Party) {
        let me = self.address.clone().unwrap_or_default();
        self.parties.retain(|x| !x.has(&me, MemberState::Joined));
        self.parties.insert(0, p);
    }

    /// Joining one party leaves any other.
    pub fn party_joined(&mut self, p: Party) {
        let me = self.address.clone().unwrap_or_default();
        let id = p.id.clone();
        self.parties.retain(|x| x.id == id || !x.has(&me, MemberState::Joined));
        self.upsert_party(p);
    }

    /// Adds one chat line; false when it is already there.
    pub fn add_message(&mut self, m: Message) -> bool {
        let cur = self.messages.entry(m.party_id.clone()).or_default();
        if cur.iter().any(|x| x.id == m.id) {
            return false;
        }
        cur.push(m);
        if cur.len() > CHAT_KEEP {
            cur.drain(..cur.len() - CHAT_KEEP);
        }
        true
    }

    pub fn open_chat(&mut self, id: Option<&str>) {
        self.open_party = id.map(str::to_string);
        if let Some(id) = id {
            self.unread.insert(id.to_string(), 0);
        }
    }

    pub fn set_presence(&mut self, address: &str, presence: &Presence) {
        for f in self.friends.iter_mut().filter(|f| f.address == address) {
            f.presence = presence.clone();
        }
        for p in &mut self.parties {
            for m in p.members.iter_mut().filter(|m| m.address == address) {
                m.presence = presence.clone();
            }
        }
    }

    /// Privacy changed here: shown at once, before the server answers.
    pub fn patch_privacy(&mut self, s: &PrivacyPatch) {
        if let Some(me) = &mut self.me {
            me.settings = me.settings.with(s);
        }
    }

    /// Applies one stream event; returns what a notice could be made from. Friend graph events
    /// change lists only the server knows, so they return None here: [`Friends::apply`] refreshes
    /// first and then reads who it was.
    pub fn apply(&mut self, e: &SocialEvent) -> Option<Applied> {
        if self.status != FriendsStatus::Ready {
            return None;
        }
        match e {
            SocialEvent::Presence(d) => {
                let was_offline = self.friends.iter().find(|f| f.address == d.address).map(|f| f.presence.state == PresenceState::Offline);
                self.set_presence(&d.address, &d.presence);
                let after = self.friends.iter().find(|f| f.address == d.address)?;
                if was_offline == Some(true) && d.presence.state != PresenceState::Offline {
                    return Some(Applied::Online { friend: after.clone(), game: d.presence.game.clone() });
                }
                None
            }
            SocialEvent::Friend(_) => None,
            SocialEvent::Party(d) => {
                if matches!(d.change, PartyChange::Closed | PartyChange::Left) {
                    if let Some(id) = &d.id {
                        self.drop_party(id);
                        return None;
                    }
                }
                let p = d.party.as_ref()?;
                // A party I am no longer in (left from another device).
                if !p.members.iter().any(|m| self.me_is(&m.address)) {
                    self.drop_party(&p.id);
                    return None;
                }
                self.upsert_party(p.clone());
                let me = self.address.clone().unwrap_or_default();
                if d.change == PartyChange::Invite && p.has(&me, MemberState::Invited) {
                    return Some(Applied::Invite { party: p.clone(), from: d.from.clone() });
                }
                None
            }
            SocialEvent::Chat(m) => {
                if !self.add_message(m.clone()) {
                    return None;
                }
                if self.me_is(&m.from) || self.open_party.as_deref() == Some(m.party_id.as_str()) {
                    return None;
                }
                *self.unread.entry(m.party_id.clone()).or_insert(0) += 1;
                Some(Applied::Message { message: m.clone(), party: self.parties.iter().find(|p| p.id == m.party_id).cloned() })
            }
            SocialEvent::Dm(m) => {
                if !self.add_dm(m.clone()) || self.me_is(&m.from) || self.open_dm.as_deref() == Some(m.from.as_str()) {
                    return None;
                }
                Some(Applied::Dm { message: m.clone(), from: self.person_of(&m.from).cloned() })
            }
            SocialEvent::DmRead(r) => {
                for c in self.convos.iter_mut().filter(|c| c.address == r.address) {
                    c.unread = 0;
                }
                None
            }
        }
    }
}

/// The selected Rare Wallet account, as friends needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub address: String,
    /// False for a watch-only account (unless the dev signer stands in for it).
    pub can_sign: bool,
}

fn cache_key(address: &str) -> String {
    format!("ui:friends:{}", address.to_lowercase())
}

/// Friends for the selected account: state, device key and API together.
pub struct Friends {
    state: RwLock<FriendsState>,
    key: Mutex<Option<DeviceKey>>,
    api: Arc<dyn FriendsApi>,
    keys: Arc<dyn Storage>,
    cache: Arc<dyn Storage>,
    clock: fn() -> i64,
    rev: AtomicU64,
    key_rev: AtomicU64,
    refresh_lock: Mutex<()>,
    refresh_gen: AtomicU64,
    /// A privacy change still on its way: an overview read meanwhile keeps showing it (the switch
    /// does not flip back while rare.market is slow to answer the save).
    privacy_pending: Mutex<Option<PrivacyPatch>>,
}

impl Friends {
    /// `keys` holds the device keys; `cache` keeps the last overview per account for the next start.
    pub fn new(api: Arc<dyn FriendsApi>, keys: Arc<dyn Storage>, cache: Arc<dyn Storage>) -> Friends {
        Friends {
            state: RwLock::new(FriendsState::default()),
            key: Mutex::new(None),
            api,
            keys,
            cache,
            clock: now_ms,
            rev: AtomicU64::new(0),
            key_rev: AtomicU64::new(0),
            refresh_lock: Mutex::new(()),
            refresh_gen: AtomicU64::new(0),
            privacy_pending: Mutex::new(None),
        }
    }

    /// A fixed clock (tests, golden images).
    pub fn with_clock(mut self, clock: fn() -> i64) -> Friends {
        self.clock = clock;
        self
    }

    pub fn now(&self) -> i64 {
        (self.clock)()
    }

    pub fn api(&self) -> &Arc<dyn FriendsApi> {
        &self.api
    }

    /// A copy of the state to draw from.
    pub fn snapshot(&self) -> FriendsState {
        self.state.read().unwrap().clone()
    }

    /// Reads the state without copying it.
    pub fn read<R>(&self, f: impl FnOnce(&FriendsState) -> R) -> R {
        f(&self.state.read().unwrap())
    }

    /// Changes the state (screens and tests); bumps [`Friends::rev`].
    pub fn update<R>(&self, f: impl FnOnce(&mut FriendsState) -> R) -> R {
        let r = f(&mut self.state.write().unwrap());
        self.rev.fetch_add(1, Ordering::SeqCst);
        r
    }

    /// Changes on every state change.
    pub fn rev(&self) -> u64 {
        self.rev.load(Ordering::SeqCst)
    }

    /// Changes when the linked key changes (the notify stream reconnects with a new token, and a
    /// heartbeat goes out).
    pub fn key_rev(&self) -> u64 {
        self.key_rev.load(Ordering::SeqCst)
    }

    fn set_key(&self, k: Option<DeviceKey>) {
        *self.key.lock().unwrap() = k;
        self.key_rev.fetch_add(1, Ordering::SeqCst);
    }

    /// The linked device key for the current account, or None.
    pub fn current_key(&self) -> Option<DeviceKey> {
        let now = self.now();
        self.key.lock().unwrap().clone().filter(|k| k.linked(now))
    }

    /// The notify stream's `social` token, when friends is on.
    pub fn stream_token(&self) -> Option<String> {
        self.current_key().map(|k| k.stream_token(self.now()))
    }

    /// Picks up the selected account: linked already, needs linking, or cannot sign.
    pub fn sync_account(&self, account: Option<&Account>) {
        let address = account.map(|a| a.address.to_lowercase());
        if self.read(|s| s.address == address && s.status != FriendsStatus::None) {
            return;
        }
        *self.key.lock().unwrap() = None;
        self.update(|s| s.reset(address.clone()));
        let (Some(a), Some(address)) = (account, address) else {
            self.update(|s| s.status = FriendsStatus::None);
            self.set_key(None);
            return;
        };
        if !a.can_sign && !self.api.is_mock() {
            self.update(|s| s.status = FriendsStatus::Watch);
            self.set_key(None);
            return;
        }
        let now = self.now();
        let mut k = device_key(&*self.keys, &address);
        if self.api.is_mock() && !k.linked(now) {
            mark_linked(&*self.keys, &mut k, now + LINK_MS);
        }
        if !k.linked(now) {
            self.update(|s| s.status = FriendsStatus::Unlinked);
            self.set_key(None);
            return;
        }
        // The last overview kept for this account is on screen at once; refresh() updates it (`at`
        // 1: shown from cache, so a failed refresh keeps it on screen rather than an error).
        let last = self.cache.get(&cache_key(&address)).and_then(|v| serde_json::from_str::<Overview>(&v).ok());
        self.update(|s| match last {
            Some(o) => s.set_overview(o, 1),
            None => s.status = FriendsStatus::Loading,
        });
        self.set_key(Some(k));
        self.refresh();
    }

    /// Turns friends on for this account on this device: one wallet signature. False when it did
    /// not happen (the error is in the state, except when the player cancelled).
    pub fn turn_on(&self) -> bool {
        let Some(address) = self.read(|s| s.address.clone()) else { return false };
        self.keys.set(&off_key(&address), None);
        self.update(|s| {
            s.status = FriendsStatus::Linking;
            s.error = None;
        });
        let now = self.now();
        let mut k = device_key(&*self.keys, &address);
        let linked = if self.api.is_mock() { Ok(now + LINK_MS) } else { self.api.link(&k, now) };
        match linked {
            Ok(until) => {
                mark_linked(&*self.keys, &mut k, until);
                self.update(|s| s.status = FriendsStatus::Loading);
                self.set_key(Some(k));
                self.refresh();
                true
            }
            Err(e) => {
                self.update(|s| {
                    s.status = FriendsStatus::Unlinked;
                    s.error = if e.is_rejected() { None } else { Some(e.to_string()) };
                });
                false
            }
        }
    }

    /// The long-lived sign-in renews itself: a live link with less than [`RENEW_MS`] left is
    /// extended with the device key's own signature (no wallet, no prompt). Best effort; a failure
    /// is tried again on a later call. True when it renewed.
    pub fn renew_if_due(&self) -> bool {
        let now = self.now();
        let Some(mut k) = self.current_key() else { return false };
        if self.api.is_mock() || k.linked_until - now > crate::sign::RENEW_MS {
            return false;
        }
        match self.api.renew(&k) {
            Ok(until) if until > k.linked_until => {
                mark_linked(&*self.keys, &mut k, until);
                self.set_key(Some(k));
                true
            }
            // the server's answer or the network: tried again on a later call
            Ok(_) | Err(_) => false,
        }
    }

    /// Signs one account out of rare.market on this device (Settings > Accounts): the key is
    /// revoked on the server and forgotten here. The account on screen goes back to "Turn on".
    pub fn sign_out_account(&self, address: &str) {
        let address = address.to_lowercase();
        self.keys.set(&off_key(&address), Some("1"));
        if self.read(|s| s.address.as_deref() == Some(address.as_str())) {
            self.turn_off();
            return;
        }
        if let Some(k) = crate::sign::stored_key(&*self.keys, &address) {
            if k.linked_until > self.now() && !self.api.is_mock() {
                let _ = self.api.unlink(&k);
            }
        }
        forget_key(&*self.keys, &address);
        self.rev.fetch_add(1, Ordering::SeqCst);
    }

    /// Whether the player turned friends off (or signed out) for `address` on this device: the
    /// launcher then never signs it in again by itself.
    pub fn turned_off(&self, address: &str) -> bool {
        self.keys.get(&off_key(&address.to_lowercase())).is_some()
    }

    /// The rare.market sign-in kept for `address` on this device.
    pub fn session_of(&self, address: &str) -> Option<crate::sign::Session> {
        crate::sign::session_of(&*self.keys, address)
    }

    /// Signs this device out of friends (the key is revoked on the server and forgotten here).
    pub fn turn_off(&self) {
        if let Some(k) = self.current_key() {
            if !self.api.is_mock() {
                let _ = self.api.unlink(&k);
            }
        }
        if let Some(a) = self.read(|s| s.address.clone()) {
            forget_key(&*self.keys, &a);
            self.keys.set(&off_key(&a), Some("1"));
        }
        self.update(|s| {
            s.status = FriendsStatus::Unlinked;
            s.clear_lists();
        });
        self.set_key(None);
    }

    /// Loads the overview. Concurrent calls share one request: a caller that waited for another's
    /// refresh returns with its result.
    pub fn refresh(&self) {
        let Some(k) = self.current_key() else { return };
        let gen = self.refresh_gen.load(Ordering::SeqCst);
        let _g = self.refresh_lock.lock().unwrap();
        if self.refresh_gen.load(Ordering::SeqCst) != gen {
            return;
        }
        let r = self.api.overview(&k);
        self.refresh_gen.fetch_add(1, Ordering::SeqCst);
        if self.current_key().map(|c| c.key) != Some(k.key.clone()) {
            return;
        }
        match r {
            Ok(o) => {
                if let Ok(v) = serde_json::to_string(&o) {
                    self.cache.set(&cache_key(&k.address), Some(&v));
                }
                let now = self.now();
                let pending = self.privacy_pending.lock().unwrap().clone();
                self.update(|s| {
                    s.set_overview(o, now);
                    if let Some(p) = &pending {
                        s.patch_privacy(p);
                    }
                });
            }
            Err(Error::Api(e)) if e.status == 401 && e.message.contains("not linked") => {
                // The server no longer knows this key (revoked or expired): link again.
                forget_key(&*self.keys, &k.address);
                self.update(|s| s.status = FriendsStatus::Unlinked);
                self.set_key(None);
            }
            Err(e) => self.update(|s| {
                s.status = if s.at != 0 { FriendsStatus::Ready } else { FriendsStatus::Error };
                s.error = Some(e.to_string());
            }),
        }
    }

    fn run<T>(&self, f: impl FnOnce(&DeviceKey) -> Result<T>) -> Result<T> {
        let k = self.current_key().ok_or_else(|| Error::Other("Turn on friends first.".into()))?;
        self.update(|s| s.busy = true);
        let r = f(&k);
        self.update(|s| s.busy = false);
        r
    }

    pub fn graph(&self, op: GraphOp, address: &str) -> Result<()> {
        self.run(|k| self.api.graph(k, op, address))?;
        self.refresh();
        Ok(())
    }

    pub fn save_privacy(&self, p: &PrivacyPatch) -> Result<()> {
        self.update(|s| s.patch_privacy(p));
        *self.privacy_pending.lock().unwrap() = Some(p.clone());
        let r = self.run(|k| self.api.save_settings(k, p));
        *self.privacy_pending.lock().unwrap() = None;
        let v = r?;
        self.update(|s| {
            if let Some(me) = &mut s.me {
                me.settings = v;
            }
        });
        Ok(())
    }

    pub fn make_code(&self) -> Result<FriendCode> {
        self.run(|k| self.api.make_code(k))
    }

    pub fn redeem_code(&self, code: &str) -> Result<Redeemed> {
        let r = self.run(|k| self.api.redeem_code(k, code))?;
        self.refresh();
        Ok(r)
    }

    // ---------- parties ----------

    pub fn create_party(&self, game: Option<&GameRef>, invite: &[String]) -> Result<Party> {
        let p = self.run(|k| self.api.create_party(k, game, invite))?;
        self.update(|s| s.party_created(p.clone()));
        Ok(p)
    }

    pub fn invite_to_party(&self, id: &str, addresses: &[String]) -> Result<()> {
        let p = self.run(|k| self.api.invite_to_party(k, id, addresses))?;
        self.update(|s| s.upsert_party(p));
        Ok(())
    }

    /// Joins a party (leaving any other) and loads its chat.
    pub fn join_party(&self, id: &str) -> Result<Party> {
        let p = self.run(|k| self.api.join_party(k, id))?;
        self.update(|s| s.party_joined(p.clone()));
        self.load_messages(id);
        Ok(p)
    }

    pub fn leave_party(&self, id: &str) -> Result<()> {
        self.run(|k| self.api.leave_party(k, id))?;
        self.update(|s| s.drop_party(id));
        Ok(())
    }

    pub fn set_party_game(&self, id: &str, game: Option<&GameRef>) -> Result<()> {
        let p = self.run(|k| self.api.set_party_game(k, id, game))?;
        self.update(|s| s.upsert_party(p));
        Ok(())
    }

    /// Loads a party's chat (quietly keeps what is there when it fails).
    pub fn load_messages(&self, id: &str) {
        let Some(k) = self.current_key() else { return };
        if let Ok(list) = self.api.messages(&k, id) {
            self.update(|s| {
                s.messages.insert(id.to_string(), list);
            });
        }
    }

    pub fn send_message(&self, id: &str, text: &str) -> Result<()> {
        let m = self.run(|k| self.api.post_message(k, id, text))?;
        self.update(|s| s.add_message(m));
        Ok(())
    }

    pub fn open_chat(&self, id: Option<&str>) {
        self.update(|s| s.open_chat(id));
    }

    // ---------- direct messages ----------

    /// Loads the messages with a friend (quietly keeps what is there when it fails).
    pub fn load_dms(&self, peer: &str) {
        let Some(k) = self.current_key() else { return };
        if let Ok(list) = self.api.dms(&k, peer) {
            self.update(|s| {
                if list.len() < crate::api::DM_PAGE {
                    s.dm_full.insert(peer.to_string());
                }
                s.dms.insert(peer.to_string(), list);
            });
        }
    }

    /// The page before the oldest loaded message, put in front (scrolling back through a
    /// conversation). Nothing when the whole history is here.
    pub fn load_older_dms(&self, peer: &str) {
        let Some(k) = self.current_key() else { return };
        let Some(first) = self.read(|s| (!s.dm_full.contains(peer)).then(|| s.dms.get(peer).and_then(|v| v.first()).map(|m| m.id.clone())).flatten()) else { return };
        if let Ok(older) = self.api.dms_before(&k, peer, &first) {
            self.update(|s| {
                if older.len() < crate::api::DM_PAGE {
                    s.dm_full.insert(peer.to_string());
                }
                let list = s.dms.entry(peer.to_string()).or_default();
                let mut v: Vec<DirectMessage> = older.into_iter().filter(|m| !list.iter().any(|x| x.id == m.id)).collect();
                v.append(list);
                *list = v;
            });
        }
    }

    pub fn send_dm(&self, peer: &str, text: &str, attach: Option<&Attach>) -> Result<()> {
        let m = self.run(|k| self.api.send_dm(k, peer, text, attach))?;
        self.update(|s| s.add_dm(m));
        Ok(())
    }

    /// The conversation with `peer` is on screen (None: closed). Opening one marks it read.
    pub fn open_dm(&self, peer: Option<&str>) {
        let had = peer.map(|p| self.read(|s| s.dm_unread(p))).unwrap_or(0);
        self.update(|s| s.open_dm(peer));
        if let (Some(p), Some(k)) = (peer, self.current_key()) {
            if had > 0 {
                let _ = self.api.read_dms(&k, p);
            }
        }
    }

    /// The heartbeat's answer: my own presence as friends see it.
    pub fn set_my_presence(&self, p: Presence) {
        self.update(|s| {
            if let Some(me) = &mut s.me {
                me.presence = p;
            }
        });
    }

    // ---------- people ----------

    pub fn person_of(&self, address: &str) -> Option<Person> {
        self.read(|s| s.person_of(address).cloned())
    }

    // ---------- usernames ----------

    /// Sets the signed-in account's username (with the linked key when it is this account's, else
    /// one wallet signature) and refreshes friends. The host updates the profile's username.
    pub fn claim_username(&self, account: Option<&str>, raw: &str) -> Result<Mine> {
        let address = account.ok_or_else(|| Error::Other("Add a Rare Wallet account first.".into()))?.to_lowercase();
        let key = self.current_key().filter(|k| k.address == address);
        let m = self.api.set_username(&address, raw, key.as_ref())?;
        self.refresh();
        Ok(m)
    }

    // ---------- live events ----------

    /// Applies one stream event; returns what a notice could be made from.
    pub fn apply(&self, e: &SocialEvent) -> Option<Applied> {
        if let SocialEvent::Friend(d) = e {
            if self.read(|s| s.status != FriendsStatus::Ready) {
                return None;
            }
            self.refresh();
            let who = self.person_of(&d.address);
            return match d.change {
                FriendChange::Request => Some(Applied::Request { from: who, address: d.address.clone() }),
                FriendChange::Accepted => Some(Applied::Accepted { who, address: d.address.clone() }),
                _ => None,
            };
        }
        // A message in the conversation on screen is read as it lands.
        if let SocialEvent::Dm(m) = e {
            let open = self.read(|s| s.open_dm.as_deref() == Some(m.from.as_str()) && s.address.as_deref() != Some(m.from.as_str()));
            let r = self.update(|s| s.apply(e));
            if open {
                if let Some(k) = self.current_key() {
                    let _ = self.api.read_dms(&k, &m.from);
                }
            }
            return r;
        }
        self.update(|s| s.apply(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Remote;
    use crate::storage::MemoryStorage;
    use rare_market::net::{Method, MockNet};

    fn me() -> String {
        format!("0x{}", "a".repeat(64))
    }
    fn mira() -> String {
        format!("0x{}", "b".repeat(64))
    }
    fn kenji() -> String {
        format!("0x{}", "c".repeat(64))
    }
    fn off() -> Presence {
        Presence { state: PresenceState::Offline, game: None, since: None, last_seen: Some(1) }
    }
    fn person(address: &str, name: Option<&str>) -> Person {
        Person { address: address.into(), name: name.map(Into::into), ..Person::default() }
    }
    fn friend(address: &str, name: &str) -> Friend {
        Friend { person: person(address, Some(name)), since: 1, presence: off() }
    }
    fn member(address: &str, name: Option<&str>, state: MemberState) -> PartyMember {
        PartyMember { person: person(address, name), state, presence: off() }
    }
    fn party(members: Vec<PartyMember>) -> Party {
        Party { id: "party0001".into(), host: mira(), game: None, created_at: 1, members }
    }
    #[test]
    fn direct_messages_count_unread_until_the_conversation_is_open() {
        let mut s = ready();
        let dm = |id: &str, from: String, to: String| DirectMessage { id: id.into(), from, to, text: "hi".into(), attach: None, at: 1, read: false };
        let r = s.apply(&SocialEvent::Dm(dm("1", mira(), me())));
        assert!(matches!(r, Some(Applied::Dm { .. })));
        assert_eq!(s.apply(&SocialEvent::Dm(dm("1", mira(), me()))), None, "the same message once");
        assert_eq!((s.dm_unread(&mira()), s.badge()), (1, 1));
        // Mine count for nobody, and land in the same conversation.
        assert_eq!(s.apply(&SocialEvent::Dm(dm("2", me(), mira()))), None);
        assert_eq!(s.dms[&mira()].len(), 2);
        assert_eq!(s.dm_unread(&mira()), 1);
        s.open_dm(Some(&mira()));
        assert_eq!(s.dm_unread(&mira()), 0);
        assert_eq!(s.apply(&SocialEvent::Dm(dm("3", mira(), me()))), None, "open: no notice");
        assert_eq!(s.dm_unread(&mira()), 0);
        s.open_dm(None);
        s.apply(&SocialEvent::Dm(dm("4", kenji(), me())));
        assert_eq!(s.convos[0].address, kenji());
        s.apply(&SocialEvent::DmRead(DmReadEvent { address: kenji(), at: 2 }));
        assert_eq!(s.dm_unread_total(), 0);
    }

    fn ready() -> FriendsState {
        FriendsState { status: FriendsStatus::Ready, address: Some(me()), friends: vec![friend(&mira(), "mira.sui"), friend(&kenji(), "kenji.sui")], ..FriendsState::default() }
    }

    #[test]
    fn a_friend_coming_online_is_news_once_a_heartbeat_while_online_is_not() {
        let mut s = ready();
        let on = Presence { state: PresenceState::Online, game: Some(GameRef { id: "web:act".into(), title: "ACT".into(), kind: GameKind::Web, ..GameRef::default() }), since: Some(2), last_seen: Some(2) };
        let r = s.apply(&SocialEvent::Presence(PresenceEvent { address: mira(), presence: on.clone() }));
        assert!(matches!(&r, Some(Applied::Online { game: Some(g), .. }) if g.id == "web:act"), "{r:?}");
        assert_eq!(s.friends[0].presence.state, PresenceState::Online);
        assert_eq!(s.apply(&SocialEvent::Presence(PresenceEvent { address: mira(), presence: Presence { game: None, ..on } })), None);
    }

    #[test]
    fn party_invites_land_in_the_list_and_are_told_once() {
        let mut s = ready();
        let p = party(vec![member(&mira(), Some("mira.sui"), MemberState::Joined), member(&me(), None, MemberState::Invited)]);
        let r = s.apply(&SocialEvent::Party(PartyEvent { change: PartyChange::Invite, party: Some(p.clone()), id: None, from: Some(mira()) }));
        assert!(matches!(&r, Some(Applied::Invite { from: Some(f), .. }) if *f == mira()), "{r:?}");
        assert_eq!(s.party_invites().len(), 1);
        assert_eq!(s.active_party(), None);
        s.apply(&SocialEvent::Party(PartyEvent { change: PartyChange::Closed, party: None, id: Some(p.id.clone()), from: None }));
        assert!(s.parties.is_empty());
    }

    #[test]
    fn chat_counts_unread_unless_that_chat_is_open_and_ignores_repeats_and_my_own_lines() {
        let mut s = ready();
        let m = Message { id: "1".into(), party_id: "party0001".into(), from: mira(), text: "ready?".into(), at: 3 };
        assert!(matches!(s.apply(&SocialEvent::Chat(m.clone())), Some(Applied::Message { .. })));
        assert_eq!(s.apply(&SocialEvent::Chat(m.clone())), None);
        assert_eq!(s.unread["party0001"], 1);
        s.open_party = Some("party0001".into());
        assert_eq!(s.apply(&SocialEvent::Chat(Message { id: "2".into(), ..m.clone() })), None);
        assert_eq!(s.apply(&SocialEvent::Chat(Message { id: "3".into(), from: me(), ..m })), None);
        assert_eq!(s.messages["party0001"].len(), 3);
    }

    #[test]
    fn nothing_applies_before_friends_are_ready() {
        let mut s = FriendsState { status: FriendsStatus::Unlinked, ..ready() };
        assert_eq!(s.apply(&SocialEvent::Chat(Message { id: "9".into(), party_id: "x".into(), from: mira(), text: "hi".into(), at: 1 })), None);
    }

    // ---------- the controller over a mock rare.market ----------

    const NOW: i64 = 1_800_000_000_000;
    fn clock() -> i64 {
        NOW
    }

    fn overview_json(friends: &str) -> String {
        format!(r#"{{"me":{{"address":"{}","settings":{{"showPlaying":true,"appearOffline":false,"visibility":"friends"}},"presence":{{"state":"online","game":null,"since":1,"lastSeen":1}}}},"friends":[{friends}],"received":[],"sent":[],"blocked":[],"parties":[]}}"#, me())
    }

    fn setup(net: &MockNet) -> (Friends, Arc<MemoryStorage>, Arc<MemoryStorage>) {
        let keys = Arc::new(MemoryStorage::new());
        let cache = Arc::new(MemoryStorage::new());
        let mut remote = Remote::new(Arc::new(net.clone()), Arc::new(crate::api::NoWallet));
        remote.clock = clock;
        let f = Friends::new(Arc::new(remote), keys.clone(), cache.clone()).with_clock(clock);
        (f, keys, cache)
    }

    fn linked(keys: &MemoryStorage) {
        let mut k = device_key(keys, &me());
        mark_linked(keys, &mut k, NOW + LINK_MS);
    }

    #[test]
    fn follows_the_account_unlinked_watch_and_linked() {
        let net = MockNet::new();
        let (f, keys, cache) = setup(&net);
        f.sync_account(None);
        assert_eq!(f.snapshot().status, FriendsStatus::None);
        f.sync_account(Some(&Account { address: "0xWATCH".into(), can_sign: false }));
        assert_eq!(f.snapshot().status, FriendsStatus::Watch);
        assert_eq!(f.snapshot().address.as_deref(), Some("0xwatch"));
        f.sync_account(Some(&Account { address: me(), can_sign: true }));
        assert_eq!(f.snapshot().status, FriendsStatus::Unlinked);
        assert!(net.calls().is_empty());

        linked(&keys);
        net.on_call(Method::Get, "https://rare.market/v1/friends", 200, overview_json(&format!(r#"{{"address":"{}","name":"mira","since":1,"presence":{{"state":"offline","game":null,"since":null,"lastSeen":1}},"avatarUrl":null}}"#, mira())));
        f.sync_account(None);
        let before = f.key_rev();
        f.sync_account(Some(&Account { address: me(), can_sign: true }));
        let s = f.snapshot();
        assert_eq!(s.status, FriendsStatus::Ready);
        assert_eq!(s.at, NOW);
        assert_eq!(s.friends[0].name.as_deref(), Some("mira"));
        assert!(f.key_rev() > before);
        assert!(cache.get(&cache_key(&me())).is_some());
        // The same account again: nothing to do.
        let n = net.calls().len();
        f.sync_account(Some(&Account { address: me(), can_sign: true }));
        assert_eq!(net.calls().len(), n);
    }

    #[test]
    fn shows_the_kept_overview_when_the_refresh_fails() {
        let net = MockNet::new();
        let (f, keys, cache) = setup(&net);
        linked(&keys);
        cache.set(&cache_key(&me()), Some(&overview_json("")));
        net.on_call(Method::Get, "https://rare.market/v1/friends", 503, "");
        f.sync_account(Some(&Account { address: me(), can_sign: true }));
        let s = f.snapshot();
        assert_eq!((s.status, s.at, s.error.as_deref()), (FriendsStatus::Ready, 1, Some("rare.market 503")));
    }

    #[test]
    fn a_key_the_server_forgot_asks_to_link_again() {
        let net = MockNet::new();
        let (f, keys, _) = setup(&net);
        linked(&keys);
        net.on_call(Method::Get, "https://rare.market/v1/friends", 401, r#"{"error":{"code":"auth","message":"device key not linked"}}"#);
        f.sync_account(Some(&Account { address: me(), can_sign: true }));
        assert_eq!(f.snapshot().status, FriendsStatus::Unlinked);
        assert_eq!(f.current_key(), None);
        assert!(!device_key(&*keys, &me()).linked(NOW));
    }

    #[test]
    fn actions_need_friends_on_and_mark_busy() {
        let net = MockNet::new();
        let (f, keys, _) = setup(&net);
        assert_eq!(f.make_code().unwrap_err().to_string(), "Turn on friends first.");
        linked(&keys);
        net.on_call(Method::Get, "https://rare.market/v1/friends", 200, overview_json(""));
        let p = format!(r#"{{"id":"p1","host":"{m}","game":null,"createdAt":1,"members":[{{"address":"{m}","name":null,"avatarUrl":null,"state":"joined","presence":{{"state":"online","game":null,"since":1,"lastSeen":1}}}}]}}"#, m = me());
        net.on_call(Method::Post, "https://rare.market/v1/friends/parties", 200, p.clone());
        net.on_call(Method::Post, "https://rare.market/v1/friends/parties/p1/leave", 200, r#"{"ok":true}"#);
        net.on_call(Method::Put, "https://rare.market/v1/friends/settings", 200, r#"{"showPlaying":false,"appearOffline":false,"visibility":"friends"}"#);
        f.sync_account(Some(&Account { address: me(), can_sign: true }));
        let created = f.create_party(None, &[mira()]).unwrap();
        assert_eq!(f.read(|s| s.active_party().map(|p| p.id.clone())), Some(created.id.clone()));
        assert!(!f.snapshot().busy);
        f.leave_party("p1").unwrap();
        assert!(f.snapshot().parties.is_empty());
        f.save_privacy(&PrivacyPatch { show_playing: Some(false), ..Default::default() }).unwrap();
        assert!(!f.snapshot().me.unwrap().settings.show_playing);
        let body = net.calls().iter().find(|c| c.method == Method::Put).unwrap().body.clone();
        assert_eq!(body.as_deref(), Some(r#"{"showPlaying":false}"#));
    }

    #[test]
    fn friend_events_refresh_then_say_who() {
        let net = MockNet::new();
        let (f, keys, _) = setup(&net);
        linked(&keys);
        net.on_call(Method::Get, "https://rare.market/v1/friends", 200, overview_json(""));
        f.sync_account(Some(&Account { address: me(), can_sign: true }));
        let n = net.calls().len();
        let r = f.apply(&SocialEvent::Friend(FriendEvent { change: FriendChange::Request, address: mira() }));
        assert_eq!(r, Some(Applied::Request { from: None, address: mira() }));
        assert_eq!(net.calls().len(), n + 1);
        assert_eq!(f.apply(&SocialEvent::Friend(FriendEvent { change: FriendChange::Removed, address: mira() })), None);
    }
}
