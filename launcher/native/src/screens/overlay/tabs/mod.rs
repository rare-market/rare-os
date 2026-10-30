//! The Rare menu's Rare, Friends and Notifications tabs (overlay/menuTabs.tsx). The menu shell
//! (overlay/menu) calls `rare_tab`, `friends_tab` and `notices_tab` to fill the tab body; they add
//! rows to `parent` and register focusables on `nav`.
//!
//! A on a row comes back from the shell as `Out::Act("<tab>:<sid>")` (the focused row's `sid`).
//! `parse_act` turns that into an `Effect` for the app. Tags and sids set here:
//!
//! Rare (`rare:`):  `resume`, `steam`, `share`, `quick`, `downloads`, `friends`, `profile`,
//!   `settings`, `power` (the first screen of the Rare button: the things people do most), plus
//!   `home`, `sleep`, `restart`, `poweroff`, `desktop` from older menus and the power sheet.
//! Friends (`friends:`):
//!   `invite-join`        Join a party you were invited to (the menu focuses it when an invite toast is up)
//!   `party-invite`       Invite friends to your party
//!   `party-leave`        Leave the party
//!   `party-start`        Start a party (then open the invite page)
//!   `join-game:<addr>`   Join the game a friend is playing (leaves a game for Home first)
//!   `friend:<addr>`      Open a friend's page
//!   `dm:<addr>`          Reply to a friend's messages (the on-screen keyboard; marks them read)
//!   `requests`           Open the friend requests
//!   `turn-on`            Turn on friends (the unlinked state's button)
//! Notifications (`news:`):  `notice:<id>` (tag = index in the list) and `mark-all-read`.
//!
//! Data comes from a per-thread `Backend` (fixtures by default). The app swaps in the real
//! friends and notify stores with `set_friends` / `set_notices` / `set_now_ms` and rebuilds the menu.

pub mod friends;
pub mod notices;

use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use friends::FriendsView;
use notices::NoticeStore;
use rare_render::theme::tok;
use rare_render::{Id, Node};
use std::cell::RefCell;

/// What the menu is over: a game or the launcher, and what closing does.
#[derive(Clone, Debug, Default)]
pub struct Where {
    pub over_game: bool,
    /// What runs under the launcher (its title), for the Resume row.
    pub resume: Option<String>,
}

// Nav group ids (data-nav-group): menu-rare, menu-party, menu-playing, menu-online, menu-news-head, menu-news.
pub const G_RARE: u32 = 1;
pub const G_PARTY: u32 = 2;
pub const G_PLAYING: u32 = 3;
pub const G_ONLINE: u32 = 4;
pub const G_NEWS_HEAD: u32 = 6;
pub const G_NEWS: u32 = 7;
/// The Requests row sits in no group on the web.
pub const G_REQUESTS: u32 = 5;

pub struct Backend {
    pub friends: FriendsView,
    pub notices: NoticeStore,
    pub now_ms: i64,
}

impl Default for Backend {
    fn default() -> Self {
        Backend { friends: friends::mock_view(), notices: notices::mock_store(), now_ms: friends::NOW_MS }
    }
}

thread_local! {
    static BACKEND: RefCell<Backend> = RefCell::new(Backend::default());
}

pub fn with_backend<R>(f: impl FnOnce(&mut Backend) -> R) -> R {
    BACKEND.with(|b| f(&mut b.borrow_mut()))
}
pub fn set_friends(v: FriendsView) {
    with_backend(|b| b.friends = v);
}
pub fn set_notices(v: NoticeStore) {
    with_backend(|b| b.notices = v);
}
pub fn set_now_ms(ms: i64) {
    with_backend(|b| b.now_ms = ms);
}

// ---- small pieces shared by the three tabs ----

/// Section: a heading (text-label, medium, secondary; an optional tabular note) over its rows.
pub fn section(cx: &mut Cx, parent: Id, title: &str, note: Option<&str>) -> Id {
    let r = cx.r();
    let col = cx.scene.add(parent, s().col().done(), Node::default());
    let head = cx.scene.add(col, s().row().items_center().gap(0.5 * r).pad(0.75 * r, 0.7 * r, 0.75 * r, 0.25 * r).done(), Node::default());
    text(cx, head, ty::LABEL, 500, tok::MUTED, title);
    if let Some(n) = note {
        let t = text(cx, head, ty::LABEL, 500, tok::MUTED, n);
        if let Some(ts_) = cx.scene.n(t).text.as_mut() {
            ts_.style.tabular = true;
        }
    }
    col
}

/// The kit Row: `h` rem high (3.25 or 3.5), rounded-card, px-[0.75rem], column gap 0.5rem (.trow).
pub fn mk_row(cx: &mut Cx, parent: Id, h: f32) -> Focusable {
    let r = cx.r();
    let id = cx.scene.add(parent, s().row().items_center().gap(0.5 * r).h(h * r).wp(1.0).px(0.75 * r).done(), Node::default().radius(0.5 * r));
    Focusable { id, kind: Kind::Row, group: 0, hint: String::new(), tag: 0, sid: String::new() }
}

/// Line: a font-medium title over an optional text-label secondary line, both truncated.
pub fn line(cx: &mut Cx, row: Id, title: &str, sub: Option<&str>) -> Id {
    let col = cx.scene.add(row, s().col().grow().done(), Node::default());
    text_tr(cx, col, ty::SMALL, 500, tok::FG, title);
    if let Some(sb) = sub {
        text_tr(cx, col, ty::LABEL, 400, tok::MUTED, sb);
    }
    col
}

/// The trailing "Join" / "Invite" word of a row.
pub fn right_label(cx: &mut Cx, row: Id, label: &str) -> Id {
    text(cx, row, ty::LABEL, 400, tok::MUTED, label)
}

/// Calm: a plain sentence, px-[0.75rem] py-[1.5rem] text-small secondary.
pub fn calm(cx: &mut Cx, parent: Id, msg: &str) {
    let r = cx.r();
    let c = cx.scene.add(parent, s().col().items_start().pad(0.75 * r, 1.5 * r, 0.75 * r, 1.5 * r).done(), Node::default());
    cx.scene.add_text(cx.t, c, s().done(), msg, ts(r, ty::SMALL.0, ty::SMALL.1, 400).wrap(), tok::MUTED);
}

fn simple(cx: &mut Cx, nav: &mut Nav, parent: Id, title: &str, hint: &str, sid: &str) {
    let fo = mk_row(cx, parent, 3.25);
    line(cx, fo.id, title, None);
    nav.add(fo.group(G_RARE).hint(hint).sid(sid));
}

// ---- Rare (over the launcher) ----

/// The Rare button's first screen over the launcher, most used first: Resume (while something
/// runs), Open Steam, Share (screenshots and clips), Quick settings, Downloads, Friends, your
/// profile, Settings, and Power last (Up from the top lands on it: the list wraps). Over a game
/// the Game tab has Screenshot under Resume.
pub fn rare_tab(cx: &mut Cx, nav: &mut Nav, parent: Id, w: &Where, _now: f64) {
    let col = cx.scene.add(parent, s().col().done(), Node::default());
    if let Some(t) = &w.resume {
        let fo = mk_row(cx, col, 3.5);
        line(cx, fo.id, &format!("Resume {t}"), Some("Back to your game"));
        nav.add(fo.group(G_RARE).hint("Resume").sid("resume"));
    }
    shortcut(cx, nav, col, "Open Steam", None, "Open Steam", "steam", Some(G::X));
    shortcut(cx, nav, col, "Share", Some("Your screenshots and clips"), "Open", "share", None);
    shortcut(cx, nav, col, "Quick settings", Some("Brightness, volume, Wi-Fi, performance"), "Open", "quick", Some(G::Quick));
    // The downloads line: the running one's progress, else that nothing is downloading.
    let sub = crate::features::downloads::menu_line().unwrap_or_else(|| "Nothing downloading".into());
    shortcut(cx, nav, col, "Downloads", Some(&sub), "Open", "downloads", None);
    shortcut(cx, nav, col, "Friends", None, "Open", "friends", None);
    simple(cx, nav, col, "Your profile", "Open", "profile");
    shortcut(cx, nav, col, "Settings", None, "Open", "settings", None);
    shortcut(cx, nav, col, "Power", Some("Sleep, restart, turn off, desktop mode"), "Open", "power", None);
}

/// A row with an optional second line and the button that does the same from here on its right.
fn shortcut(cx: &mut Cx, nav: &mut Nav, parent: Id, title: &str, sub: Option<&str>, hint: &str, sid: &str, key: Option<G>) {
    let r = cx.r();
    let fo = mk_row(cx, parent, if sub.is_some() { 3.5 } else { 3.25 });
    line(cx, fo.id, title, sub);
    if let Some(k) = key {
        let g = cx.scene.add(fo.id, s().row().items_center().no_shrink_().pad(0.0, 0.0, 0.1 * r, 0.0).done(), Node::default());
        glyph(cx, g, k, true);
    }
    nav.add(fo.group(G_RARE).hint(hint).sid(sid));
}

pub fn friends_tab(cx: &mut Cx, nav: &mut Nav, parent: Id, _w: &Where, _now: f64) {
    let (v, ms) = with_backend(|b| (b.friends.clone(), b.now_ms));
    friends::friends_tab(cx, nav, parent, &v, ms);
}

pub fn notices_tab(cx: &mut Cx, nav: &mut Nav, parent: Id, _w: &Where, _now: f64) {
    let (st, ms) = with_backend(|b| (b.notices.clone(), b.now_ms));
    notices::notices_tab(cx, nav, parent, &st, ms);
}

// ---- what A does ----

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PowerAct {
    Suspend,
    Reboot,
    Poweroff,
    Desktop,
}

/// What a row asks the app to do. Over a game, the ones that live in the launcher leave the game
/// for Home first (`inLauncher` on the web).
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Home,
    Profile,
    Settings,
    Steam,
    Power(PowerAct),
    /// The power sheet: Sleep, Restart, Turn off, Desktop mode.
    PowerSheet,
    JoinParty,
    InviteFriends,
    LeaveParty,
    StartParty,
    JoinGame(String),
    OpenFriend(String),
    Reply(String),
    OpenRequests,
    TurnOn,
    MarkAllRead,
    OpenNotice(String),
}

/// Maps the shell's `Out::Act` string (`rare:home`, `friends:join-game:0x..`, `news:notice:12`).
pub fn parse_act(act: &str) -> Option<Effect> {
    let (tab, what) = act.split_once(':')?;
    Some(match (tab, what) {
        ("rare", "home") => Effect::Home,
        ("rare", "power") => Effect::PowerSheet,
        ("rare", "profile") => Effect::Profile,
        ("rare", "settings") => Effect::Settings,
        ("rare", "steam") => Effect::Steam,
        ("rare", "sleep") => Effect::Power(PowerAct::Suspend),
        ("rare", "restart") => Effect::Power(PowerAct::Reboot),
        ("rare", "poweroff") => Effect::Power(PowerAct::Poweroff),
        ("rare", "desktop") => Effect::Power(PowerAct::Desktop),
        ("friends", "invite-join") => Effect::JoinParty,
        ("friends", "party-invite") => Effect::InviteFriends,
        ("friends", "party-leave") => Effect::LeaveParty,
        ("friends", "party-start") => Effect::StartParty,
        ("friends", "requests") => Effect::OpenRequests,
        ("friends", "turn-on") => Effect::TurnOn,
        ("friends", w) if w.starts_with("join-game:") => Effect::JoinGame(w["join-game:".len()..].into()),
        ("friends", w) if w.starts_with("friend:") => Effect::OpenFriend(w["friend:".len()..].into()),
        ("friends", w) if w.starts_with("dm:") => Effect::Reply(w["dm:".len()..].into()),
        ("news", "mark-all-read") => Effect::MarkAllRead,
        ("news", w) if w.starts_with("notice:") => Effect::OpenNotice(w["notice:".len()..].into()),
        _ => return None,
    })
}

/// Applies the parts that only touch the notices store (opening a notice reads it); the app does
/// the rest, then rebuilds the menu.
pub fn apply_local(e: &Effect) {
    with_backend(|b| match e {
        Effect::OpenNotice(id) => b.notices.mark_read(id),
        Effect::MarkAllRead => {
            let ms = b.now_ms;
            b.notices.mark_all_read(ms)
        }
        _ => {}
    });
}

// ---- fixtures (the menu shell's menu-rare / menu-friends / menu-news and the states below) ----

/// Which tab a fixture name opens ("rare", "friends", "news"), or None for names that are not mine.
pub fn fixture_tab(name: &str) -> Option<&'static str> {
    Some(match name {
        "menu-rare" => "rare",
        "menu-friends" | "menu-friends-invite" | "menu-friends-party" | "menu-friends-quiet" | "menu-friends-signin" | "menu-friends-passkey" | "menu-friends-error" | "menu-friends-unlinked" | "menu-friends-linking" | "menu-friends-requests" | "menu-friends-messages" => "friends",
        "menu-news" | "menu-news-empty" | "menu-news-read" => "news",
        _ => return None,
    })
}

/// Sets the backend for a fixture name (call before building the menu).
pub fn use_fixture(name: &str) {
    let mut b = Backend::default();
    match name {
        "menu-friends-invite" => friends::mock_invite(&mut b.friends),
        "menu-friends-messages" => friends::mock_messages(&mut b.friends),
        "menu-friends-party" => friends::mock_party(&mut b.friends),
        "menu-friends-quiet" => {
            b.friends.friends.truncate(0);
            b.friends.received.clear();
        }
        "menu-friends-requests" => {
            b.friends.friends.retain(|f| f.presence.state == friends::PState::Offline);
            let one = b.friends.received[0].clone();
            let mut two = one.clone();
            two.address.push('1');
            two.username = Some("pixel".into());
            two.name = Some("pixel".into());
            b.friends.received = vec![one, two];
        }
        "menu-friends-signin" => b.friends.status = friends::Status::None,
        "menu-friends-passkey" => b.friends.status = friends::Status::Watch,
        "menu-friends-error" => b.friends.status = friends::Status::Error,
        "menu-friends-unlinked" => b.friends.status = friends::Status::Unlinked,
        "menu-friends-linking" => b.friends.status = friends::Status::Linking,
        "menu-news-empty" => b.notices.notices.clear(),
        "menu-news-read" => b.notices.mark_all_read(friends::NOW_MS),
        _ => {}
    }
    BACKEND.with(|c| *c.borrow_mut() = b);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rare_render::{ImageLoader, Scene, TextSys};

    fn with_cx<R>(f: impl FnOnce(&mut Cx) -> R) -> R {
        let mut scene = Scene::new(1920.0, 1200.0, 18.0);
        let mut t = TextSys::new(&[include_bytes!("../../../../../render/assets/fonts/Inter-Regular.ttf"), include_bytes!("../../../../../render/assets/fonts/Inter-Medium.ttf"), include_bytes!("../../../../../render/assets/fonts/Inter-SemiBold.ttf")], false, 256);
        let mut img = ImageLoader::new(1, std::sync::Arc::new(|| {}), None);
        let mut cx = Cx { scene: &mut scene, t: &mut t, img: &mut img };
        f(&mut cx)
    }

    fn sids(name: &str, f: fn(&mut Cx, &mut Nav, Id, &Where, f64)) -> Vec<String> {
        use_fixture(name);
        with_cx(|cx| {
            let mut nav = Nav::default();
            let root = cx.scene.root;
            f(cx, &mut nav, root, &Where::default(), 0.0);
            cx.scene.layout(cx.t);
            nav.items.iter().map(|i| i.sid.clone()).collect()
        })
    }

    #[test]
    fn rare_tab_rows() {
        assert_eq!(sids("menu-rare", rare_tab), ["steam", "share", "quick", "downloads", "friends", "profile", "settings", "power"]);
    }

    #[test]
    fn friends_default_has_start_playing_online_requests() {
        let v = sids("menu-friends", friends_tab);
        assert_eq!(v.len(), 1 + 3 + 2 + 1);
        assert_eq!(v[0], "party-start");
        assert!(v[1].starts_with("join-game:") && v[4].starts_with("friend:"));
        assert_eq!(v.last().unwrap(), "requests");
    }

    #[test]
    fn friends_invite_and_party_states() {
        let v = sids("menu-friends-invite", friends_tab);
        assert_eq!(&v[..2], ["invite-join", "party-start"]);
        let v = sids("menu-friends-party", friends_tab);
        assert_eq!(&v[..2], ["party-invite", "party-leave"]);
    }

    #[test]
    fn friends_quiet_and_prompt_states() {
        assert_eq!(sids("menu-friends-quiet", friends_tab), ["party-start"]);
        for n in ["menu-friends-signin", "menu-friends-passkey", "menu-friends-error", "menu-friends-linking"] {
            assert!(sids(n, friends_tab).is_empty(), "{n}");
        }
        assert_eq!(sids("menu-friends-unlinked", friends_tab), ["turn-on"]);
    }

    #[test]
    fn notices_unread_and_empty() {
        let v = sids("menu-news", notices_tab);
        assert_eq!(v.len(), 1 + 8);
        assert_eq!(v[0], "mark-all-read");
        assert_eq!(v[1], "notice:1008");
        assert_eq!(sids("menu-news-read", notices_tab).len(), 8);
        assert!(sids("menu-news-empty", notices_tab).is_empty());
    }

    #[test]
    fn acts_parse_and_opening_a_notice_reads_it() {
        assert_eq!(parse_act("friends:join-game:0xab"), Some(Effect::JoinGame("0xab".into())));
        assert_eq!(parse_act("rare:sleep"), Some(Effect::Power(PowerAct::Suspend)));
        assert_eq!(parse_act("news:mark-all-read"), Some(Effect::MarkAllRead));
        assert_eq!(parse_act("items:3"), None);
        use_fixture("menu-news");
        assert_eq!(with_backend(|b| b.notices.unread()), 2);
        apply_local(&parse_act("news:notice:1008").unwrap());
        assert_eq!(with_backend(|b| b.notices.unread()), 1);
        apply_local(&Effect::MarkAllRead);
        assert_eq!(with_backend(|b| b.notices.unread()), 0);
    }
}
