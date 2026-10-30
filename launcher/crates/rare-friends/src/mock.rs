//! Sample friends (the web launcher's `?friends=mock`): screen work and golden images without a
//! wallet signature. State lives in memory and changes with the actions, like the real API. The
//! web version answers after a short delay; this one answers at once so images are stable.

use crate::api::{Error, FriendsApi, Result};
use crate::sign::{DeviceKey, LINK_MS};
use crate::store::{FriendsState, FriendsStatus};
use crate::types::*;
use crate::username::{Availability, Mine, Unavailable};
use crate::username_rules::normalize_username;
use std::collections::HashSet;
use std::sync::Mutex;

pub const ME: &str = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const MIN: i64 = 60_000;
const DAY: i64 = 86_400_000;

/// A stable fake address for a name (the web mock's FNV-style hash, so both show the same).
pub fn hex(seed: &str) -> String {
    let units: Vec<u16> = seed.encode_utf16().collect();
    let mut h: u32 = 2_166_136_261;
    let mut out = String::new();
    let mut i: u32 = 0;
    while out.len() < 64 {
        let c = units[i as usize % units.len()] as u32;
        h = (h ^ c ^ i).wrapping_mul(16_777_619);
        out.push_str(&format!("{h:08x}"));
        i += 1;
    }
    format!("0x{}", &out[..64])
}

pub fn act() -> GameRef {
    GameRef::web("web:act", "ACT", "https://play.actuniverse.com/")
}
pub fn balatro() -> GameRef {
    GameRef::steam("steam:2379780", "Balatro", 2379780)
}
pub fn hades() -> GameRef {
    GameRef::steam("steam:1145360", "Hades II", 1145360)
}

fn on(game: Option<GameRef>, since: i64, now: i64) -> Presence {
    Presence { state: PresenceState::Online, game, since: Some(now - since), last_seen: Some(now) }
}

/// Names ending in .sui are SuiNS names; a name with a username is shown as that username with
/// the SuiNS line under it.
fn friend(name: &str, avatar: Option<&str>, presence: Presence, days: i64, username: Option<&str>, now: i64) -> Friend {
    let suins = name.ends_with(".sui");
    let user = username.map(str::to_string).or_else(|| (!suins).then(|| name.to_string()));
    Friend {
        person: Person { address: hex(name), name: Some(user.clone().unwrap_or_else(|| name.to_string())), username: user, suins: suins.then(|| name.to_string()), avatar_url: avatar.map(str::to_string) },
        since: now - days * DAY,
        presence,
    }
}

fn member(f: &Friend, state: MemberState) -> PartyMember {
    PartyMember { person: f.person.clone(), state, presence: f.presence.clone() }
}

fn me_member(now: i64) -> PartyMember {
    PartyMember { person: Person { address: ME.into(), name: Some("you".into()), username: Some("you".into()), suins: None, avatar_url: None }, state: MemberState::Joined, presence: on(None, 30 * MIN, now) }
}

fn pending(p: &Person, at: i64) -> Pending {
    Pending { person: p.clone(), at }
}

struct Data {
    settings: Privacy,
    friends: Vec<Friend>,
    received: Vec<Pending>,
    sent: Vec<Pending>,
    blocked: Vec<Pending>,
    parties: Vec<Party>,
    chat: Vec<Message>,
    taken: HashSet<String>,
    next_party: u32,
}

/// The sample API. `now` fixes every time in it, so golden images do not drift.
pub struct MockApi {
    now: i64,
    data: Mutex<Data>,
}

/// The sample overview's lists, built at `now`. `with_party` adds a party with chat (the web
/// launcher's `&party`).
fn sample(now: i64, with_party: bool) -> Data {
    let friends = vec![
        friend("mira.sui", Some("https://storage.claynosaurz.com/popkins/images/0x817dac94fff3a567b80e23a3b55fca2fd108641e0dd06fc614e5ecc0acbb4e03"), on(Some(act()), 42 * MIN, now), 20, Some("mira"), now),
        friend("kenji.sui", Some("https://img.pawtato.app/hero/7842.png"), on(Some(balatro()), 18 * MIN, now), 20, Some("kenji"), now),
        friend("lowkey.sui", Some("https://byzantion.mypinata.cloud/ipfs/QmbUWeu1ryJX5aKz8wF3bnoYyabw9A2abCfjDsJVDdJtio/5822.png"), on(Some(hades()), 75 * MIN, now), 20, Some("lowkey"), now),
        friend("otter", None, on(None, 9 * MIN, now), 20, None, now),
        friend("vex.sui", Some("https://indexer.mypinata.cloud/ipfs/bafkreid6owvnhp2v2muf4yrk4v3hjonhposfgsnd5zirfzgbyutxoasxay"), Presence { state: PresenceState::Away, game: None, since: Some(now - 3 * 3_600_000), last_seen: Some(now) }, 20, None, now),
        friend("sable.sui", None, Presence { state: PresenceState::Offline, game: None, since: None, last_seen: Some(now - 3 * 3_600_000) }, 20, None, now),
        friend("quill.sui", Some("https://img.pawtato.app/land/15439.png"), Presence { state: PresenceState::Offline, game: None, since: None, last_seen: Some(now - 2 * DAY) }, 20, None, now),
    ];
    let received = vec![Pending { person: Person { address: hex("dune.sui"), name: Some("dune".into()), username: Some("dune".into()), suins: None, avatar_url: None }, at: now - 6 * MIN }];
    let sent = vec![Pending { person: Person { address: hex("pixel.sui"), name: Some("pixel.sui".into()), username: None, suins: Some("pixel.sui".into()), avatar_url: None }, at: now - 2 * DAY }];
    let (parties, chat) = if with_party {
        let p = Party { id: "mockparty01".into(), host: friends[0].address.clone(), game: Some(act()), created_at: now - 20 * MIN, members: vec![member(&friends[0], MemberState::Joined), me_member(now), member(&friends[3], MemberState::Invited)] };
        let line = |id: &str, from: &str, text: &str, ago: i64| Message { id: id.into(), party_id: "mockparty01".into(), from: from.into(), text: text.into(), at: now - ago * MIN };
        let chat = vec![
            line("1", &friends[0].address, "squad up in the hub, I'm at the west gate", 6),
            line("2", ME, "on my way", 5),
            line("3", &friends[0].address, "otter you coming?", 2),
        ];
        (vec![p], chat)
    } else {
        (Vec::new(), Vec::new())
    };
    Data {
        settings: Privacy::default(),
        friends,
        received,
        sent,
        blocked: Vec::new(),
        parties,
        chat,
        taken: ["mira", "kenji", "lowkey", "otter", "vex", "sable", "quill", "rare_fan"].iter().map(|s| s.to_string()).collect(),
        next_party: 0,
    }
}

fn mock_suins(u: &str) -> Option<String> {
    matches!(u, "mira" | "kenji" | "lowkey").then(|| format!("{u}.sui"))
}

impl MockApi {
    pub fn new(now: i64, with_party: bool) -> MockApi {
        MockApi { now, data: Mutex::new(sample(now, with_party)) }
    }

    fn party_mut<'a>(d: &'a mut Data, id: &str) -> Result<&'a mut Party> {
        d.parties.iter_mut().find(|p| p.id == id).ok_or_else(|| Error::Other("That party has ended.".into()))
    }
}

impl FriendsApi for MockApi {
    fn is_mock(&self) -> bool {
        true
    }

    fn link(&self, _: &DeviceKey, now: i64) -> Result<i64> {
        Ok(now + LINK_MS)
    }

    fn unlink(&self, _: &DeviceKey) -> Result<()> {
        Ok(())
    }

    fn overview(&self, _: &DeviceKey) -> Result<Overview> {
        let d = self.data.lock().unwrap();
        Ok(Overview {
            me: Me { address: ME.into(), settings: d.settings.clone(), presence: on(None, 30 * MIN, self.now) },
            friends: d.friends.clone(),
            received: d.received.clone(),
            sent: d.sent.clone(),
            blocked: d.blocked.clone(),
            parties: d.parties.clone(),
            dms: Vec::new(),
            played_with: Vec::new(),
        })
    }

    fn graph(&self, _: &DeviceKey, op: GraphOp, address: &str) -> Result<GraphState> {
        let mut d = self.data.lock().unwrap();
        let now = self.now;
        let p = d
            .friends
            .iter()
            .map(|x| &x.person)
            .chain(d.received.iter().map(|x| &x.person))
            .chain(d.sent.iter().map(|x| &x.person))
            .chain(d.blocked.iter().map(|x| &x.person))
            .find(|x| x.address == address)
            .cloned()
            .unwrap_or_else(|| Person { address: address.into(), name: None, username: None, suins: None, avatar_url: None });
        let drop = |d: &mut Data| {
            d.friends.retain(|x| x.address != address);
            d.received.retain(|x| x.address != address);
            d.sent.retain(|x| x.address != address);
        };
        match op {
            GraphOp::Request => {
                drop(&mut d);
                d.sent.insert(0, pending(&p, now));
            }
            GraphOp::Accept => {
                drop(&mut d);
                d.friends.insert(0, Friend { person: p, since: now, presence: on(None, MIN, now) });
            }
            GraphOp::Decline | GraphOp::Cancel | GraphOp::Remove => drop(&mut d),
            GraphOp::Block => {
                drop(&mut d);
                d.blocked.insert(0, pending(&p, now));
            }
            GraphOp::Unblock => d.blocked.retain(|x| x.address != address),
        }
        Ok(GraphState { state: Some(op.as_str().into()) })
    }

    fn save_settings(&self, _: &DeviceKey, s: &PrivacyPatch) -> Result<Privacy> {
        let mut d = self.data.lock().unwrap();
        d.settings = d.settings.with(s);
        Ok(d.settings.clone())
    }

    fn make_code(&self, _: &DeviceKey) -> Result<FriendCode> {
        Ok(FriendCode { code: "K7Q4MX".into(), url: "https://rare.market/v1/friends/qr/K7Q4MX".into(), expires_at: self.now + 15 * MIN })
    }

    fn redeem_code(&self, _: &DeviceKey, code: &str) -> Result<Redeemed> {
        if code.chars().filter(|c| !c.is_whitespace()).count() != 6 {
            return Err(Error::Other("That code is not valid or has expired.".into()));
        }
        let mut d = self.data.lock().unwrap();
        let f = friend("nova", None, on(None, MIN, self.now), 0, None, self.now);
        let address = f.address.clone();
        d.friends.insert(0, f);
        Ok(Redeemed { address })
    }

    fn heartbeat(&self, _: &DeviceKey, _: &Beat) -> Result<Presence> {
        Ok(on(None, 30 * MIN, self.now))
    }

    fn create_party(&self, _: &DeviceKey, game: Option<&GameRef>, invite: &[String]) -> Result<Party> {
        let mut d = self.data.lock().unwrap();
        d.next_party += 1;
        let mut members = vec![me_member(self.now)];
        members.extend(d.friends.iter().filter(|f| invite.contains(&f.address)).map(|f| member(f, MemberState::Invited)));
        let p = Party { id: format!("mock{}", &hex(&format!("party{}", d.next_party))[2..10]), host: ME.into(), game: game.cloned(), created_at: self.now, members };
        d.parties = vec![p.clone()];
        d.chat.clear();
        Ok(p)
    }

    fn invite_to_party(&self, _: &DeviceKey, id: &str, addresses: &[String]) -> Result<Party> {
        let mut d = self.data.lock().unwrap();
        let add: Vec<PartyMember> = d.friends.iter().filter(|f| addresses.contains(&f.address)).map(|f| member(f, MemberState::Invited)).collect();
        let p = Self::party_mut(&mut d, id)?;
        for m in add {
            if !p.members.iter().any(|x| x.address == m.address) {
                p.members.push(m);
            }
        }
        Ok(p.clone())
    }

    fn join_party(&self, _: &DeviceKey, id: &str) -> Result<Party> {
        let mut d = self.data.lock().unwrap();
        let p = Self::party_mut(&mut d, id)?;
        if let Some(m) = p.members.iter_mut().find(|m| m.address == ME) {
            m.state = MemberState::Joined;
        }
        Ok(p.clone())
    }

    fn leave_party(&self, _: &DeviceKey, id: &str) -> Result<()> {
        self.data.lock().unwrap().parties.retain(|p| p.id != id);
        Ok(())
    }

    fn set_party_game(&self, _: &DeviceKey, id: &str, game: Option<&GameRef>) -> Result<Party> {
        let mut d = self.data.lock().unwrap();
        let p = Self::party_mut(&mut d, id)?;
        p.game = game.cloned();
        Ok(p.clone())
    }

    fn messages(&self, _: &DeviceKey, id: &str) -> Result<Vec<Message>> {
        Ok(self.data.lock().unwrap().chat.iter().filter(|m| m.party_id == id).cloned().collect())
    }

    fn post_message(&self, _: &DeviceKey, id: &str, text: &str) -> Result<Message> {
        let mut d = self.data.lock().unwrap();
        let m = Message { id: (d.chat.len() + 10).to_string(), party_id: id.into(), from: ME.into(), text: text.into(), at: self.now };
        d.chat.push(m.clone());
        Ok(m)
    }

    fn check_username(&self, raw: &str, _: Option<&str>) -> Result<Availability> {
        let u = normalize_username(raw);
        if ["admin", "rare", "support", "mysten", "sui"].contains(&u.as_str()) {
            return Ok(Availability { username: u, available: false, reason: Some(Unavailable::Reserved), message: Some("That name is not available.".into()), yours: None });
        }
        if self.data.lock().unwrap().taken.contains(&u) {
            return Ok(Availability { username: u, available: false, reason: Some(Unavailable::Taken), message: Some("Someone already has that name.".into()), yours: None });
        }
        Ok(Availability { username: u, available: true, reason: None, message: None, yours: None })
    }

    fn set_username(&self, _: &str, raw: &str, _: Option<&DeviceKey>) -> Result<Mine> {
        let username = normalize_username(raw);
        self.data.lock().unwrap().taken.insert(username.clone());
        Ok(Mine { username, claimed_at: self.now, changed_at: self.now, next_change_at: None })
    }

    fn lookup_username(&self, raw: &str) -> Result<Option<Person>> {
        let u = normalize_username(raw);
        if !self.data.lock().unwrap().taken.contains(&u) {
            return Ok(None);
        }
        Ok(Some(Person { address: format!("0x{}", "ab".repeat(32)), name: Some(u.clone()), suins: mock_suins(&u), username: Some(u), avatar_url: None }))
    }
}

/// A ready friends state with the sample data, for a screen's golden image without a [`crate::store::Friends`].
pub fn ready_state(now: i64, with_party: bool) -> FriendsState {
    let d = sample(now, with_party);
    let mut messages = std::collections::HashMap::new();
    if with_party {
        messages.insert("mockparty01".to_string(), d.chat.clone());
    }
    FriendsState {
        status: FriendsStatus::Ready,
        address: Some(ME.into()),
        me: Some(Me { address: ME.into(), settings: d.settings, presence: on(None, 30 * MIN, now) }),
        friends: d.friends,
        received: d.received,
        sent: d.sent,
        blocked: d.blocked,
        parties: d.parties,
        messages,
        at: now,
        ..FriendsState::default()
    }
}

/// `&friendsping`: a party invite from the first friend arrives (for screenshots of the notice).
pub fn invite_event(s: &FriendsState, now: i64) -> Option<SocialEvent> {
    let mira = s.friends.first()?;
    let me = s.address.as_ref()?;
    let you = PartyMember { person: Person { address: me.clone(), name: None, username: None, suins: None, avatar_url: None }, state: MemberState::Invited, presence: mira.presence.clone() };
    Some(SocialEvent::Party(PartyEvent {
        change: PartyChange::Invite,
        from: Some(mira.address.clone()),
        id: None,
        party: Some(Party { id: "mockinvite1".into(), host: mira.address.clone(), game: mira.presence.game.clone(), created_at: now, members: vec![member(mira, MemberState::Joined), you] }),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;
    use crate::store::{Account, Applied, Friends};
    use std::sync::Arc;

    const NOW: i64 = 1_800_000_000_000;
    fn clock() -> i64 {
        NOW
    }

    #[test]
    fn addresses_match_the_web_mock() {
        // From the web mock's hex("mira.sui").
        assert_eq!(hex("mira.sui"), "0xe80c2f785b2e1630c98d06c0eb0171061f4528446bde46b693e9681f496f69e3");
    }

    #[test]
    fn plays_through_without_a_wallet() {
        let f = Friends::new(Arc::new(MockApi::new(NOW, false)), Arc::new(MemoryStorage::new()), Arc::new(MemoryStorage::new())).with_clock(clock);
        // Watch-only accounts may use the sample data.
        f.sync_account(Some(&Account { address: ME.into(), can_sign: false }));
        let s = f.snapshot();
        assert_eq!(s.status, FriendsStatus::Ready);
        assert_eq!(s.friends.len(), 7);
        assert_eq!(s.groups().playing.len(), 3);
        let p = f.create_party(Some(&act()), &[s.friends[0].address.clone()]).unwrap();
        f.send_message(&p.id, "hi").unwrap();
        assert_eq!(f.snapshot().messages[&p.id].len(), 1);
        f.graph(GraphOp::Accept, &hex("dune.sui")).unwrap();
        assert_eq!(f.snapshot().friends[0].name.as_deref(), Some("dune"));
        assert!(f.redeem_code("K7 Q4").is_err());
        f.redeem_code("K7Q4MX").unwrap();
        assert_eq!(f.snapshot().friends[0].name.as_deref(), Some("nova"));
        assert!(!f.api().check_username("mira", None).unwrap().available);
        let r = f.apply(&invite_event(&f.snapshot(), NOW).unwrap());
        assert!(matches!(r, Some(Applied::Invite { .. })));
    }

    #[test]
    fn golden_state_has_the_party_and_its_chat() {
        let s = ready_state(NOW, true);
        assert_eq!(s.active_party().map(|p| p.id.as_str()), Some("mockparty01"));
        assert_eq!(s.messages["mockparty01"].len(), 3);
    }
}
