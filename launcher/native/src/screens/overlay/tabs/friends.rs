//! Friends data and pure logic (friends/{types,display,format,mock}.ts, the selectors of store.ts)
//! and the Friends tab body (overlay/menuTabs.tsx FriendsTab).

use super::{calm, line, mk_row, right_label, section, G_ONLINE, G_PARTY, G_PLAYING, G_REQUESTS};
use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::theme::tok;
use rare_render::{Color, Id, Node};

// ---- types.ts ----

#[derive(Clone, Debug, PartialEq)]
pub struct GameRef {
    pub id: String,
    pub title: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PState {
    Online,
    Away,
    Offline,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Presence {
    pub state: PState,
    pub game: Option<GameRef>,
    pub since: Option<i64>,
    pub last_seen: Option<i64>,
}

/// display.ts Identity: what rare.market sends to name a player.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Identity {
    pub address: String,
    pub name: Option<String>,
    pub username: Option<String>,
    pub suins: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Friend {
    pub who: Identity,
    /// their picture (rare.market's avatarUrl)
    pub avatar: Option<String>,
    pub presence: Presence,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MState {
    Invited,
    Joined,
}

#[derive(Clone, Debug)]
pub struct PartyMember {
    pub who: Identity,
    pub state: MState,
}

#[derive(Clone, Debug)]
pub struct Party {
    pub id: String,
    pub host: String,
    pub game: Option<GameRef>,
    pub members: Vec<PartyMember>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Status {
    /// Not signed in.
    None,
    /// A watch-only account.
    Watch,
    Error,
    Unlinked,
    Linking,
    /// Loading: the tab shows nothing yet.
    Loading,
    Ready,
}

/// A conversation with a friend: the last lines (oldest first, `true` when mine) and the unread count.
#[derive(Clone, Debug)]
pub struct Convo {
    pub who: Identity,
    pub avatar: Option<String>,
    pub lines: Vec<(bool, String)>,
    pub unread: u32,
}

/// What the web `useFriends()` holds, as far as the tab reads it.
#[derive(Clone, Debug)]
pub struct FriendsView {
    pub status: Status,
    pub error: Option<String>,
    pub address: String,
    pub friends: Vec<Friend>,
    pub received: Vec<Identity>,
    pub parties: Vec<Party>,
    /// Conversations, newest first.
    pub convos: Vec<Convo>,
}

// ---- display.ts ----

pub fn short_addr(a: &str) -> String {
    if a.len() < 10 {
        return a.to_string();
    }
    format!("{}...{}", &a[..6], &a[a.len() - 4..])
}

#[derive(Debug, PartialEq)]
pub struct Shown {
    pub primary: String,
    pub secondary: Option<String>,
    pub kind: &'static str,
}

pub fn shown(p: Option<&Identity>, fallback: &str) -> Shown {
    let Some(p) = p else { return Shown { primary: fallback.into(), secondary: None, kind: "address" } };
    let username = p.username.clone().filter(|u| !u.is_empty());
    let suins = p.suins.clone().filter(|u| !u.is_empty());
    if let Some(u) = username {
        let secondary = suins.filter(|x| *x != u);
        return Shown { primary: u, secondary, kind: "username" };
    }
    let name = p.name.clone().filter(|u| !u.is_empty());
    if let Some(n) = &name {
        if suins.is_none() && !n.to_lowercase().ends_with(".sui") {
            return Shown { primary: n.clone(), secondary: None, kind: "username" };
        }
    }
    if let Some(x) = suins.or(name) {
        return Shown { primary: x, secondary: None, kind: "suins" };
    }
    Shown { primary: short_addr(&p.address), secondary: None, kind: "address" }
}

pub fn name_of(p: Option<&Identity>, fallback: &str) -> String {
    shown(p, fallback).primary
}

/// store.ts `label`.
pub fn label(p: Option<&Identity>) -> String {
    name_of(p, "Someone")
}

// ---- format.ts ----

fn round(x: f64) -> i64 {
    (x + 0.5).floor() as i64
}

fn mins(ms: i64) -> String {
    let m = round(ms as f64 / 60_000.0).max(1);
    if m < 60 {
        format!("{m} min")
    } else {
        format!("{} h {}", m / 60, if m % 60 != 0 { format!("{} min", m % 60) } else { String::new() }).trim().to_string()
    }
}

pub fn ago(t: i64, now: i64) -> String {
    let s = ((now - t) as f64 / 1000.0).max(0.0);
    if s < 120.0 {
        "just now".into()
    } else if s < 3600.0 {
        format!("{} min ago", round(s / 60.0))
    } else if s < 86400.0 {
        format!("{} h ago", round(s / 3600.0))
    } else if s < 86400.0 * 2.0 {
        "yesterday".into()
    } else {
        format!("{} days ago", round(s / 86400.0))
    }
}

/// "Playing ACT", "Online", "Away", "Last seen 3 h ago", "Offline". `long` adds how long.
pub fn presence_text(p: &Presence, long: bool, now: i64) -> String {
    match p.state {
        PState::Offline => match p.last_seen.filter(|t| *t != 0) {
            Some(t) => format!("Last seen {}", ago(t, now)),
            None => "Offline".into(),
        },
        PState::Away => match &p.game {
            Some(g) => format!("Away, in {}", g.title),
            None => "Away".into(),
        },
        PState::Online => match &p.game {
            Some(g) => match p.since.filter(|t| *t != 0) {
                Some(since) if long => format!("Playing {} for {}", g.title, mins(now - since)),
                _ => format!("Playing {}", g.title),
            },
            None => "Online".into(),
        },
    }
}

// ---- store.ts selectors ----

fn mine<'a>(v: &'a FriendsView, p: &'a Party, state: MState) -> bool {
    p.members.iter().any(|m| m.who.address == v.address && m.state == state)
}

/// The party the player is in (joined), if any.
pub fn active_party(v: &FriendsView) -> Option<&Party> {
    v.parties.iter().find(|p| mine(v, p, MState::Joined))
}

/// Invites waiting for an answer.
pub fn party_invites(v: &FriendsView) -> Vec<&Party> {
    v.parties.iter().filter(|p| mine(v, p, MState::Invited)).collect()
}

fn host_label(p: &Party) -> String {
    label(p.members.iter().find(|m| m.who.address == p.host).map(|m| &m.who))
}

// ---- mock.ts ----

pub const NOW_MS: i64 = 1_800_000_000_000;
pub const ME: &str = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const MIN: i64 = 60_000;

fn hex(seed: &str) -> String {
    let b = seed.as_bytes();
    let mut h: u32 = 2166136261;
    let mut out = String::new();
    let mut i = 0usize;
    while out.len() < 64 {
        h = (h ^ b[i % b.len()] as u32 ^ i as u32).wrapping_mul(16777619);
        out.push_str(&format!("{h:08x}"));
        i += 1;
    }
    format!("0x{}", &out[..64])
}

fn game(id: &str, title: &str) -> GameRef {
    GameRef { id: id.into(), title: title.into() }
}

fn on(g: Option<GameRef>, since_min: i64) -> Presence {
    Presence { state: PState::Online, game: g, since: Some(NOW_MS - since_min * MIN), last_seen: Some(NOW_MS) }
}

fn friend(name: &str, avatar: bool, presence: Presence, username: Option<&str>) -> Friend {
    let sui = name.ends_with(".sui");
    let user = username.map(|u| u.to_string()).or_else(|| (!sui).then(|| name.to_string()));
    Friend {
        who: Identity { address: hex(name), name: Some(user.clone().unwrap_or_else(|| name.to_string())), username: user, suins: sui.then(|| name.to_string()) },
        // a friend with a picture shows an NFT; the rest their default
        avatar: avatar.then(|| format!("https://storage.claynosaurz.com/popkins/images/{}", crate::screens::inventory::fixtures::POPKIN)),
        presence,
    }
}

pub fn mock_friends() -> Vec<Friend> {
    let off = |ago_ms: i64| Presence { state: PState::Offline, game: None, since: None, last_seen: Some(NOW_MS - ago_ms) };
    vec![
        friend("mira.sui", true, on(Some(game("web:act", "ACT")), 42), Some("mira")),
        friend("kenji.sui", true, on(Some(game("steam:2379780", "Balatro")), 18), Some("kenji")),
        friend("lowkey.sui", true, on(Some(game("steam:1145360", "Hades II")), 75), Some("lowkey")),
        friend("otter", false, on(None, 9), None),
        friend("vex.sui", true, Presence { state: PState::Away, game: None, since: Some(NOW_MS - 180 * MIN), last_seen: Some(NOW_MS) }, None),
        friend("sable.sui", false, off(3 * 3_600_000), None),
        friend("quill.sui", true, off(2 * 86_400_000), None),
    ]
}

fn member(f: &Friend, state: MState) -> PartyMember {
    PartyMember { who: f.who.clone(), state }
}

fn me_member() -> PartyMember {
    PartyMember { who: Identity { address: ME.into(), name: Some("you".into()), username: Some("you".into()), suins: None }, state: MState::Joined }
}

pub fn mock_view() -> FriendsView {
    FriendsView {
        status: Status::Ready,
        error: None,
        address: ME.into(),
        friends: mock_friends(),
        received: vec![Identity { address: hex("dune.sui"), name: Some("dune".into()), username: Some("dune".into()), suins: None }],
        parties: vec![],
        convos: vec![],
    }
}

/// Two conversations, one with unread messages and a long last line.
pub fn mock_messages(v: &mut FriendsView) {
    let f = &v.friends;
    v.convos = vec![
        Convo {
            who: f[0].who.clone(),
            avatar: f[0].avatar.clone(),
            lines: vec![(true, "you up for another run?".into()), (false, "yes, after this match. I found a route through the second boss that skips the whole bridge".into())],
            unread: 2,
        },
        Convo { who: f[1].who.clone(), avatar: f[1].avatar.clone(), lines: vec![(false, "gg".into()), (true, "Sent a screenshot: rare.market/s/abc".into())], unread: 0 },
    ];
}

/// `?party`: mira hosts a party in ACT, you joined, otter is invited.
pub fn mock_party(v: &mut FriendsView) {
    let f = &v.friends;
    v.parties = vec![Party { id: "mockparty01".into(), host: f[0].who.address.clone(), game: f[0].presence.game.clone(), members: vec![member(&f[0], MState::Joined), me_member(), member(&f[3], MState::Invited)] }];
}

/// An invite waiting for an answer: mira's party, you invited.
pub fn mock_invite(v: &mut FriendsView) {
    let f = &v.friends;
    let mut me = me_member();
    me.state = MState::Invited;
    v.parties = vec![Party { id: "mockparty01".into(), host: f[0].who.address.clone(), game: f[0].presence.game.clone(), members: vec![member(&f[0], MState::Joined), me, member(&f[3], MState::Joined)] }];
}

// ---- the tab ----

/// A player's picture (their Rare default or their NFT); a round swatch from the address when it
/// cannot be fetched.
fn avatar(cx: &mut Cx, parent: Id, address: &str, url: Option<&str>, size: f32) -> Id {
    if let Some(id) = crate::screens::overlay::common::player_avatar(cx, parent, address, url, size) {
        return id;
    }
    let h = address.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    let c = Color::rgb(60 + (h & 0x7f) as u8, 60 + ((h >> 8) & 0x7f) as u8, 90 + ((h >> 16) & 0x7f) as u8);
    cx.scene.add(parent, s().wh(size, size).no_shrink_().done(), Node::bg(c).radius(size / 2.0))
}

/// PresenceLine: a Dot (online white, away muted; none offline) and the words.
fn presence_line(cx: &mut Cx, parent: Id, p: &Presence, now: i64) -> Id {
    let r = cx.r();
    let row = cx.scene.add(parent, s().row().items_center().gap(0.45 * r).done(), Node::default());
    if p.state != PState::Offline {
        dot(cx, row, if p.state == PState::Online { tok::FG } else { tok::MUTED });
    }
    let c = if p.state == PState::Online && p.game.is_some() { tok::FG.alpha(0.85) } else { tok::MUTED };
    text_tr(cx, row, ty::LABEL, 400, c, &presence_text(p, false, now))
}

fn friend_row(cx: &mut Cx, nav: &mut Nav, parent: Id, f: &Friend, group: u32, hint: &str, sid: String, now: i64) {
    let r = cx.r();
    let fo = mk_row(cx, parent, 3.5);
    avatar(cx, fo.id, &f.who.address, f.avatar.as_deref(), 2.25 * r);
    let col = cx.scene.add(fo.id, s().col().grow().done(), Node::default());
    text_tr(cx, col, ty::SMALL, 500, tok::FG, &label(Some(&f.who)));
    presence_line(cx, col, &f.presence, now);
    nav.add(fo.group(group).hint(hint).sid(&sid));
}

pub fn friends_tab(cx: &mut Cx, nav: &mut Nav, parent: Id, v: &FriendsView, now_ms: i64) {
    let r = cx.r();
    match v.status {
        Status::None => return calm(cx, parent, "Sign in with a passkey to see your friends."),
        Status::Watch => return calm(cx, parent, "Add a passkey to see your friends."),
        Status::Error => return calm(cx, parent, v.error.as_deref().unwrap_or("Friends didn't load. Try again in a moment.")),
        Status::Unlinked | Status::Linking => {
            let c = cx.scene.add(parent, s().col().items_start().gap(0.8 * r).pad(0.75 * r, 1.2 * r, 0.75 * r, 1.2 * r).done(), Node::default());
            cx.scene.add_text(cx.t, c, s().done(), "Turn on friends to see who is playing and join their games.", ts(r, ty::SMALL.0, ty::SMALL.1, 400).wrap(), tok::MUTED);
            let linking = v.status == Status::Linking;
            let b = button(cx, c, if linking { "Turning on..." } else { "Turn on friends" }, Variant::Secondary, Sz::Md, false);
            if linking {
                // disabled:opacity-40, not focusable.
                cx.scene.n(b.id).opacity.snap(0.4);
            } else {
                nav.add(b.group(G_ONLINE + 10).hint("Turn on").sid("turn-on"));
            }
            return;
        }
        Status::Loading => return,
        Status::Ready => {}
    }
    let col = cx.scene.add(parent, s().col().done(), Node::default());
    let party = active_party(v);
    let invites = party_invites(v);
    let invite = invites.first().copied();
    let playing: Vec<&Friend> = v.friends.iter().filter(|f| f.presence.state != PState::Offline && f.presence.game.is_some()).collect();
    let online: Vec<&Friend> = v.friends.iter().filter(|f| f.presence.state != PState::Offline && f.presence.game.is_none()).collect();
    let requests = v.received.len();

    let sec = section(cx, col, "Party", None);
    if let (Some(inv), None) = (invite, party) {
        let fo = mk_row(cx, sec, 3.25);
        let sub = match &inv.game {
            Some(g) => g.title.clone(),
            None => format!("{} in the party", inv.members.iter().filter(|m| m.state == MState::Joined).count()),
        };
        line(cx, fo.id, &format!("{} invited you", host_label(inv)), Some(&sub));
        right_label(cx, fo.id, "Join");
        nav.add(fo.group(G_PARTY).hint("Join party").sid("invite-join"));
    }
    if let Some(p) = party {
        let fo = mk_row(cx, sec, 3.25);
        let title = if p.host == v.address { "Your party".to_string() } else { format!("{}'s party", host_label(p)) };
        let names: Vec<String> = p.members.iter().filter(|m| m.state == MState::Joined).map(|m| if m.who.address == v.address { "you".to_string() } else { label(Some(&m.who)) }).collect();
        line(cx, fo.id, &title, Some(&names.join(", ")));
        right_label(cx, fo.id, "Invite");
        nav.add(fo.group(G_PARTY).hint("Invite friends").sid("party-invite"));
        let fo = mk_row(cx, sec, 3.25);
        line(cx, fo.id, "Leave party", None);
        nav.add(fo.group(G_PARTY).hint("Leave party").sid("party-leave"));
    } else {
        let fo = mk_row(cx, sec, 3.25);
        line(cx, fo.id, "Start a party", Some("Then invite friends to this game"));
        nav.add(fo.group(G_PARTY).hint("Start a party").sid("party-start"));
    }
    if !v.convos.is_empty() {
        let unread: u32 = v.convos.iter().map(|c| c.unread).sum();
        let note = (unread > 0).then(|| format!("{unread} new"));
        let sec = section(cx, col, "Messages", note.as_deref());
        for c in v.convos.iter().take(4) {
            let fo = mk_row(cx, sec, 4.75);
            avatar(cx, fo.id, &c.who.address, c.avatar.as_deref(), 2.25 * r);
            let lc = cx.scene.add(fo.id, s().col().grow().done(), Node::default());
            text_tr(cx, lc, ty::SMALL, 500, tok::FG, &label(Some(&c.who)));
            for (mine, t) in c.lines.iter().rev().take(2).rev() {
                let who = if *mine { "You".to_string() } else { label(Some(&c.who)) };
                text_tr(cx, lc, ty::LABEL, 400, if *mine { tok::MUTED } else { tok::FG.alpha(0.85) }, &format!("{who}: {t}"));
            }
            if c.unread > 0 {
                dot(cx, fo.id, tok::FG);
                right_label(cx, fo.id, &if c.unread > 99 { "99+".to_string() } else { c.unread.to_string() });
            }
            nav.add(fo.group(G_PARTY + 20).hint("Reply").sid(&format!("dm:{}", c.who.address)));
        }
    }
    if !playing.is_empty() {
        let sec = section(cx, col, "Playing", Some(&playing.len().to_string()));
        for f in &playing {
            friend_row(cx, nav, sec, f, G_PLAYING, "Join game", format!("join-game:{}", f.who.address), now_ms);
        }
    }
    if !online.is_empty() {
        let sec = section(cx, col, "Online", Some(&online.len().to_string()));
        for f in &online {
            friend_row(cx, nav, sec, f, G_ONLINE, "Open", format!("friend:{}", f.who.address), now_ms);
        }
    }
    if requests > 0 {
        let sec = section(cx, col, "Requests", None);
        let fo = mk_row(cx, sec, 3.25);
        line(cx, fo.id, &if requests == 1 { "1 friend request".to_string() } else { format!("{requests} friend requests") }, None);
        dot(cx, fo.id, tok::FG);
        nav.add(fo.group(G_REQUESTS).hint("Open").sid("requests"));
    }
    if playing.is_empty() && online.is_empty() && requests == 0 {
        calm(cx, col, "None of your friends are online right now.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
    fn id(name: Option<&str>, username: Option<&str>, suins: Option<&str>) -> Identity {
        Identity { address: A.into(), name: name.map(Into::into), username: username.map(Into::into), suins: suins.map(Into::into) }
    }

    // display.test.ts
    #[test]
    fn username_leads_suins_is_the_quiet_second_line() {
        let s = shown(Some(&id(Some("mira"), Some("mira"), Some("mira.sui"))), "Someone");
        assert_eq!((s.primary.as_str(), s.secondary.as_deref(), s.kind), ("mira", Some("mira.sui"), "username"));
    }
    #[test]
    fn a_username_alone_has_no_second_line() {
        let s = shown(Some(&id(None, Some("mira"), None)), "Someone");
        assert_eq!((s.primary.as_str(), s.secondary, s.kind), ("mira", None, "username"));
    }
    #[test]
    fn suins_alone_leads_when_there_is_no_username() {
        let s = shown(Some(&id(Some("mira.sui"), None, Some("mira.sui"))), "Someone");
        assert_eq!((s.primary.as_str(), s.secondary, s.kind), ("mira.sui", None, "suins"));
    }
    #[test]
    fn falls_back_to_a_short_address() {
        let s = shown(Some(&id(None, None, None)), "Someone");
        assert_eq!((s.primary.as_str(), s.secondary, s.kind), ("0xa049...887e", None, "address"));
        assert_eq!(name_of(None, "Someone"), "Someone");
    }
    #[test]
    fn answers_that_only_carry_name_still_work() {
        assert_eq!(shown(Some(&id(Some("mira"), None, None)), "Someone").kind, "username");
        assert_eq!(shown(Some(&id(Some("mira.sui"), None, None)), "Someone").kind, "suins");
    }

    // format.test.ts
    fn pres(state: PState, g: Option<GameRef>, since: Option<i64>, last_seen: Option<i64>) -> Presence {
        Presence { state, game: g, since, last_seen }
    }
    #[test]
    fn presence_says_what_a_friend_is_doing() {
        let act = Some(game("web:act", "ACT"));
        let n = NOW_MS;
        assert_eq!(presence_text(&pres(PState::Online, act.clone(), Some(n - 42 * MIN), Some(n)), false, n), "Playing ACT");
        assert_eq!(presence_text(&pres(PState::Online, act.clone(), Some(n - 42 * MIN), Some(n)), true, n), "Playing ACT for 42 min");
        assert_eq!(presence_text(&pres(PState::Online, act, Some(n - 75 * MIN), Some(n)), true, n), "Playing ACT for 1 h 15 min");
        assert_eq!(presence_text(&pres(PState::Online, None, Some(n), Some(n)), false, n), "Online");
        assert_eq!(presence_text(&pres(PState::Away, None, Some(n), Some(n)), false, n), "Away");
    }
    #[test]
    fn offline_says_when_last_seen_if_shared() {
        let n = NOW_MS;
        assert_eq!(presence_text(&pres(PState::Offline, None, None, Some(n - 3 * 3_600_000)), false, n), "Last seen 3 h ago");
        assert_eq!(presence_text(&pres(PState::Offline, None, None, Some(n - 30 * 86_400_000)), false, n), "Last seen 30 days ago");
        assert_eq!(presence_text(&pres(PState::Offline, None, None, None), false, n), "Offline");
    }

    // store.test.ts (party selectors)
    #[test]
    fn invites_are_not_an_active_party() {
        let mut v = mock_view();
        assert!(active_party(&v).is_none() && party_invites(&v).is_empty());
        mock_invite(&mut v);
        assert_eq!(party_invites(&v).len(), 1);
        assert!(active_party(&v).is_none());
        mock_party(&mut v);
        assert!(active_party(&v).is_some());
        assert!(party_invites(&v).is_empty());
    }

    #[test]
    fn mock_addresses_are_stable_64_hex() {
        let h = hex("mira.sui");
        assert_eq!(h.len(), 66);
        assert_eq!(h, hex("mira.sui"));
    }
}
