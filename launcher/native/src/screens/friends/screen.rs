//! Friends (friends/ui/FriendsScreen.tsx): the list on the left (add, party, requests, playing,
//! online, offline, sent, blocked, privacy), the focused entry on the right. Rows are 3.25rem on the
//! page; the right side is one panel with the player's picture, presence and actions. A picker
//! (invite friends, pick a game, pick an item to send) takes the right side over until B.
//!
//! Focusing a row selects it (the right side follows the d-pad); A on a row moves focus to the
//! right side (its default button, else its first). Side effects leave as `Out::Act("friends:..")`
//! for the app to run against rare-friends (and the wallet, the OSK, the launcher).

use super::add::{add_pane, AddState};
use super::bits::*;
use super::model::FriendsData;
use super::party::party_pane;
use super::pickers::{game_picker, invite_picker, invite_room, item_picker, picker_games};
use crate::kit::{button, group, stat_line, toggle, Btn, StatItem, Variant};
use crate::screens::market::host::*;
use rare_friends::types::{
    Friend, GameKind, GameRef, Party, Pending, Person, PresenceState, Privacy, Visibility,
};
use rare_friends::FriendsStatus;
use rare_render::view::{key, El};

/// What the right side shows (FriendsScreen's `Sel`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sel {
    Add,
    Privacy,
    Blocked,
    NewParty,
    Party(String),
    Friend(String),
    Request(String),
    Sent(String),
    /// Recently played with.
    Recent,
}

impl Sel {
    /// selKey
    pub fn key(&self) -> String {
        match self {
            Sel::Add => "add".into(),
            Sel::Privacy => "privacy".into(),
            Sel::Blocked => "blocked".into(),
            Sel::NewParty => "new-party".into(),
            Sel::Party(id) => format!("party:{id}"),
            Sel::Friend(a) => format!("friend:{a}"),
            Sel::Request(a) => format!("request:{a}"),
            Sel::Sent(a) => format!("sent:{a}"),
            Sel::Recent => "recent".into(),
        }
    }
}

/// A picker over the right side (`Picker`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Picker {
    /// Invite friends to this party, or to a new one.
    Invite(Option<String>),
    /// Pick the party's game.
    Game(String),
    /// Pick an item to send to this friend (address).
    Item(String),
    /// Pick the friend a shared capture goes to (its kind and link).
    Send { kind: String, url: String },
}

/// Remove or block, asked once more.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sure {
    Remove,
    Block,
}

/// The screen's own state (what React keeps in useState).
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub sel: Sel,
    pub picker: Option<Picker>,
    pub sure: Option<Sure>,
    /// A friend's Remove and Block, swapped into the action row by X.
    pub more: bool,
    /// How many of the newest messages the conversation box is scrolled back past (0: the newest).
    pub dm_back: usize,
    /// Friends picked in the invite picker (addresses).
    pub picked: Vec<String>,
    pub add: AddState,
}

impl Default for View {
    fn default() -> View {
        View {
            sel: Sel::Add,
            picker: None,
            sure: None,
            picked: Vec::new(),
            add: AddState::default(),
            more: false,
            dm_back: 0,
        }
    }
}

// Focus tags. List rows are T_ROW + their index in `list_rows`.
pub const T_ROW: u32 = 1000;
pub const T_JOIN_GAME: u32 = 2001;
pub const T_INVITE_PARTY: u32 = 2002;
pub const T_SEND_COINS: u32 = 2003;
pub const T_SEND_ITEM: u32 = 2004;
pub const T_REMOVE: u32 = 2005;
pub const T_BLOCK: u32 = 2006;
pub const T_YES: u32 = 2007;
pub const T_KEEP: u32 = 2008;
pub const T_ACCEPT: u32 = 2010;
pub const T_DECLINE: u32 = 2011;
pub const T_BLOCK_REQ: u32 = 2012;
pub const T_CANCEL_REQ: u32 = 2013;
pub const T_MESSAGE: u32 = 2014;
pub const T_SEND_CAPTURE: u32 = 2015;
/// The conversation box (A writes) and the profile panel: focusable so the d-pad scrolls to them.
pub const T_DM_PANEL: u32 = 2016;
pub const T_PROFILE: u32 = 2017;
pub const T_LESS: u32 = 2018;
pub const T_UNBLOCK: u32 = 2100;
pub const T_PRIVACY: u32 = 2200;
pub const T_TURN_OFF: u32 = 2210;
pub const T_INVITE_FRIENDS: u32 = 2300;
pub const T_START_EMPTY: u32 = 2301;
pub const T_ADD_FIELD: u32 = 2400;
pub const T_PICK_USERNAME: u32 = 2401;
pub const T_MORE: u32 = 2402;
pub const T_ENTER_CODE: u32 = 2403;
pub const T_NEW_CODE: u32 = 2404;
pub const T_JOIN_PARTY: u32 = 2500;
pub const T_DECLINE_PARTY: u32 = 2501;
pub const T_PARTY_JOIN_GAME: u32 = 2502;
pub const T_WRITE: u32 = 2503;
pub const T_PARTY_INVITE: u32 = 2504;
pub const T_PICK_GAME: u32 = 2505;
pub const T_LEAVE: u32 = 2506;
pub const T_PICK_FRIEND: u32 = 3000;
pub const T_INVITE_GO: u32 = 3100;
pub const T_INVITE_CANCEL: u32 = 3101;
pub const T_GAME: u32 = 3200;
pub const T_ITEM: u32 = 3400;
pub const T_RECENT: u32 = 3500;
pub const T_SEND_PICK: u32 = 3600;
pub const T_SEND_CANCEL: u32 = 3800;
pub const T_GATE: u32 = 4000;

/// The list's rows in order (what A and the d-pad select).
pub fn list_rows(d: &FriendsData) -> Vec<Sel> {
    let fs = &d.st;
    let mut v = vec![Sel::Add];
    v.push(match fs.active_party() {
        Some(p) => Sel::Party(p.id.clone()),
        None => Sel::NewParty,
    });
    v.extend(fs.party_invites().iter().map(|p| Sel::Party(p.id.clone())));
    v.extend(fs.received.iter().map(|f| Sel::Request(f.address.clone())));
    let g = fs.groups();
    v.extend(
        g.playing
            .iter()
            .chain(&g.online)
            .chain(&g.offline)
            .map(|f| Sel::Friend(f.address.clone())),
    );
    v.extend(fs.sent.iter().map(|f| Sel::Sent(f.address.clone())));
    if !super::profile::recent_list(d).is_empty() {
        v.push(Sel::Recent);
    }
    if !fs.blocked.is_empty() {
        v.push(Sel::Blocked);
    }
    v.push(Sel::Privacy);
    v
}

/// Whether `sel` still points at something (an accepted request, an ended party go away).
pub fn still_there(d: &FriendsData, sel: &Sel) -> bool {
    let fs = &d.st;
    match sel {
        Sel::Friend(a) => fs.friends.iter().any(|f| &f.address == a),
        Sel::Request(a) => fs.received.iter().any(|f| &f.address == a),
        Sel::Sent(a) => fs.sent.iter().any(|f| &f.address == a),
        Sel::Party(id) => fs.parties.iter().any(|p| &p.id == id),
        Sel::Recent => !super::profile::recent_list(d).is_empty(),
        _ => true,
    }
}

/// privacyLine
pub fn privacy_line(p: Option<&Privacy>) -> &'static str {
    match p {
        None => "",
        Some(p) if p.appear_offline => "Appearing offline",
        Some(p) if p.visibility == Visibility::Everyone => "Status shown to everyone",
        Some(p) if p.show_playing => "Friends see what you play",
        Some(_) => "Friends see you online",
    }
}

/// joinNote: how Join works for this game, in one line.
pub fn join_note(d: &FriendsData, g: &GameRef) -> &'static str {
    let own = d
        .library
        .iter()
        .any(|x| x.id == g.id || g.appid.is_some_and(|a| x.id == format!("steam:{a}")));
    match g.kind {
        GameKind::Steam if own => "Starts it through Steam, where their invite lands",
        GameKind::Steam => "Not on this device: opens it in Steam",
        GameKind::Web if g.join.is_some() => "Opens their game with the party invite",
        GameKind::Web => "Opens the same game; group up in its party screen",
        _ if own => "Starts the same game here",
        _ => "Not on this device",
    }
}

/// currentGame: the game this device is running, for a new party.
pub fn current_game(d: &FriendsData) -> Option<GameRef> {
    let id = d.running.first()?;
    let g = d.library.iter().find(|x| &x.id == id)?;
    Some(
        if let Some(appid) = g.id.strip_prefix("steam:").and_then(|a| a.parse().ok()) {
            GameRef::steam(&g.id, &g.title, appid)
        } else if g.id.starts_with("web:") {
            GameRef {
                id: g.id.clone(),
                title: g.title.clone(),
                kind: GameKind::Web,
                ..GameRef::default()
            }
        } else {
            GameRef::other(&g.id, &g.title)
        },
    )
}

// ---------------------------------------------------------------- the page

/// The whole screen: the gate before the list, else the list and the right side.
pub fn view(ui: &mut Ui, d: &FriendsData, v: &View) -> El {
    if d.st.status != FriendsStatus::Ready {
        return gate(ui, d);
    }
    let fs = &d.st;
    let g = fs.groups();
    let rows = list_rows(d);
    let tag_of = |sel: &Sel| T_ROW + rows.iter().position(|r| r == sel).unwrap_or(0) as u32;

    // ---- left: the list
    let online = if fs.friends.is_empty() {
        String::new()
    } else {
        format!("{} online", g.playing.len() + g.online.len())
    };
    let head = baseline_row(
        ui,
        s().mb(0.6).gap(0.75).px(1.0),
        vec![
            (T::new(Ty::Title).w(600), "Friends".into()),
            (T::new(Ty::Small).c(tok::MUTED), online),
        ],
    );
    let mut list = ui
        .bx(s()
            .col()
            .w(24.0)
            .ml(-1.0)
            .gap(0.15)
            .pt(1.0)
            .pb(1.0)
            .pl(0.25)
            .pr(0.5))
        .scrolls_y(key("friends-list"))
        .group(group("friends-list", true))
        .child(head);
    let on = |sel: &Sel| *sel == v.sel;
    let add = icon_line(
        ui,
        Some("user-plus"),
        "Add a friend",
        "By username, SuiNS name or address",
    );
    list = list.child(list_row(
        ui,
        &Sel::Add,
        on(&Sel::Add),
        tag_of(&Sel::Add),
        "Open",
        add,
    ));
    match fs.active_party() {
        Some(p) => {
            let sel = Sel::Party(p.id.clone());
            let line = party_line(ui, p, fs.address.as_deref(), false);
            list = list.child(list_row(
                ui,
                &sel,
                on(&sel),
                tag_of(&sel),
                "Open party",
                line,
            ));
        }
        None => {
            let line = icon_line(ui, Some("users"), "Start a party", "Play together and chat");
            list = list.child(list_row(
                ui,
                &Sel::NewParty,
                on(&Sel::NewParty),
                tag_of(&Sel::NewParty),
                "Open",
                line,
            ));
        }
    }
    for p in fs.party_invites() {
        let sel = Sel::Party(p.id.clone());
        let line = party_line(ui, p, fs.address.as_deref(), true);
        list = list.child(list_row(
            ui,
            &sel,
            on(&sel),
            tag_of(&sel),
            "Open invite",
            line,
        ));
    }
    list = list.maybe(group_head(ui, "Requests", fs.received.len()));
    for f in &fs.received {
        let sel = Sel::Request(f.address.clone());
        let line = person_line(ui, d, f, None, "Wants to be your friend");
        list = list.child(list_row(ui, &sel, on(&sel), tag_of(&sel), "Open", line));
    }
    for (title, fs) in [
        ("Playing", &g.playing),
        ("Online", &g.online),
        ("Offline", &g.offline),
    ] {
        list = list.maybe(group_head(ui, title, fs.len()));
        for f in fs.iter() {
            let sel = Sel::Friend(f.address.clone());
            let mut line = person_line(ui, d, f, Some(f), "");
            let unread = d.st.dm_unread(&f.address);
            if unread > 0 {
                line.push(unread_mark(ui, unread));
            }
            list = list.child(list_row(ui, &sel, on(&sel), tag_of(&sel), "Open", line));
        }
    }
    list = list.maybe(group_head(ui, "Sent", fs.sent.len()));
    for f in &fs.sent {
        let sel = Sel::Sent(f.address.clone());
        let line = person_line(ui, d, f, None, "Request sent");
        list = list.child(list_row(ui, &sel, on(&sel), tag_of(&sel), "Open", line));
    }
    if fs.friends.is_empty() && fs.received.is_empty() && fs.sent.is_empty() {
        list = list.child(ui.text_s(
            s().px(1.0).pt(0.75),
            "No friends yet. Add one by username, SuiNS name or address.",
            T::new(Ty::Small).c(tok::MUTED).wrap(),
        ));
    }
    list = list.child(ui.bx(s().mt(1.0)));
    let recent = super::profile::recent_list(d);
    if !recent.is_empty() {
        let n = recent.len();
        let line = icon_line(ui, Some("history"), "Recently played with", &format!("{n} {}", if n == 1 { "friend" } else { "friends" }));
        list = list.child(list_row(ui, &Sel::Recent, on(&Sel::Recent), tag_of(&Sel::Recent), "Open", line));
    }
    if !fs.blocked.is_empty() {
        let n = fs.blocked.len();
        let line = icon_line(
            ui,
            None,
            "Blocked",
            &format!("{n} {}", if n == 1 { "player" } else { "players" }),
        );
        list = list.child(list_row(
            ui,
            &Sel::Blocked,
            on(&Sel::Blocked),
            tag_of(&Sel::Blocked),
            "Open",
            line,
        ));
    }
    let line = icon_line(
        ui,
        Some("shield"),
        "Privacy",
        privacy_line(fs.me.as_ref().map(|m| &m.settings)),
    );
    list = list.child(list_row(
        ui,
        &Sel::Privacy,
        on(&Sel::Privacy),
        tag_of(&Sel::Privacy),
        "Open",
        line,
    ));

    // ---- right: the entry or a picker
    let body = match &v.picker {
        Some(Picker::Invite(pid)) => invite_picker(ui, d, pid.as_deref(), &v.picked),
        Some(Picker::Game(_)) => game_picker(ui, d),
        Some(Picker::Item(a)) => item_picker(ui, d, a),
        Some(Picker::Send { kind, .. }) => super::profile::send_picker(ui, d, kind),
        None => detail(ui, d, v),
    };
    let right = ui
        .bx(s().col().grow().pt(1.0).pb(1.0))
        .scrolls_y(key("friends-detail"))
        .group(group("friends-detail", false))
        .child(body.key(key(&(v.sel.key(), format!("{:?}", v.picker)))));

    ui.bx(s().fill().row().gap(2.0).px(2.5).pb(1.5).pt(0.5))
        .child(list)
        .child(right)
}

/// `ListRow`: 3.25rem, layer 02 and a 3 px bar when it is the one shown on the right.
fn list_row(ui: &mut Ui, sel: &Sel, on: bool, tag: u32, hint: &str, kids: Vec<El>) -> El {
    // The bar is always there (hidden when off) so selecting a row never changes the tree's shape.
    let bar = ui
        .bx(s().abs(Some(0.7), None, Some(0.7), Some(0.0)).w_px(3.0))
        .bg(tok::FG)
        .radius(1.5)
        .opacity(if on { 1.0 } else { 0.0 });
    // shrink: the list overflows, so (as flex items of the web's scrolling column) rows give up
    // their padding down to min-h 3.25rem.
    let el = ui
        .bx(s()
            .row()
            .items_center()
            .min_h(3.25)
            .wp(1.0)
            .gap(0.8)
            .px(1.0)
            .py(0.45)
            .shrink())
        .radius(ui.r(0.5))
        .bg(if on {
            tok::L2
        } else {
            rare_render::Color::TRANSPARENT
        })
        .child(bar);
    fill_nav(el.kids(kids), tag, Some(hint), None, false).key(key(&sel.key()))
}

/// Unread messages on a friend's row: a dot and the count, at the row's end.
fn unread_mark(ui: &mut Ui, n: u32) -> El {
    let d = crate::kit::dot(ui, s(), crate::kit::DotTone::Primary, false);
    // A fixed width, the count at its end: dots line up down the list.
    ui.bx(s().row().items_center().justify_end().gap(0.4).min_w(2.6)).child(d).child(ui.text(super::profile::count_text(n), T::new(Ty::Meta).w(600).tab()))
}

/// `IconLine`: an icon in a 2.25rem box, a title and a muted line.
fn icon_line(ui: &mut Ui, icon: Option<&str>, title: &str, body: &str) -> Vec<El> {
    let mut v = Vec::new();
    if let Some(i) = icon {
        v.push(
            ui.bx(s().size(2.25).row().items_center().justify_center())
                .child(lucide(ui, i, 1.15, tok::fg(0.8))),
        );
    }
    v.push(
        ui.bx(s().col().grow())
            .child(ui.text(title, T::new(Ty::Body).w(500).trunc()))
            .child(ui.text(body, T::new(Ty::Meta).c(tok::MUTED).trunc())),
    );
    v
}

/// The name, the SuiNS line beside it, and the presence (or a line of text).
fn person_line(
    ui: &mut Ui,
    d: &FriendsData,
    p: &Person,
    friend: Option<&Friend>,
    line: &str,
) -> Vec<El> {
    let shown = rare_friends::display::shown(Some(p), "Someone");
    let mut runs = vec![(T::new(Ty::Body).w(500).trunc(), label(Some(p)))];
    if let Some(sec) = shown.secondary {
        runs.push((T::new(Ty::Meta).c(tok::MUTED).trunc(), sec));
    }
    let top = baseline_row(ui, s().gap(0.6).min0(), runs);
    let second = match friend {
        Some(f) => presence_line(ui, s(), &f.presence, false, d.now_ms, Ty::Meta),
        None => ui.text(line, T::new(Ty::Meta).c(tok::MUTED).trunc()),
    };
    vec![
        friend_avatar(ui, s(), p, 2.25),
        ui.bx(s().col().grow()).child(top).child(second),
    ]
}

/// Overlapping pictures (`-space-x`), each ringed in the panel colour (`ring-2 ring-layer-01`).
pub fn avatar_stack(ui: &mut Ui, people: &[&Person], size: f32, overlap: f32) -> El {
    let ring = 2.0 / ui.rem;
    let mut row = ui.bx(s().row());
    for (i, p) in people.iter().enumerate() {
        let mut st = s()
            .size(size + 2.0 * ring)
            .m(-ring)
            .row()
            .items_center()
            .justify_center();
        if i > 0 {
            st = st.ml(-overlap - ring);
        }
        let av = friend_avatar(ui, s(), p, size);
        row = row.child(
            ui.bx(st)
                .bg(tok::L1)
                .radius(ui.r(size / 2.0 + ring))
                .child(av),
        );
    }
    row
}

/// `PartyLine`: up to three joined members' pictures, whose party, its game and size.
fn party_line(ui: &mut Ui, p: &Party, me: Option<&str>, invite: bool) -> Vec<El> {
    let host = p
        .members
        .iter()
        .find(|m| m.address == p.host)
        .map(|m| &m.person);
    let joined: Vec<&Person> = p
        .members
        .iter()
        .filter(|m| m.state == rare_friends::types::MemberState::Joined)
        .map(|m| &m.person)
        .collect();
    let title = if invite {
        format!("Party invite from {}", label(host))
    } else if host.map(|h| h.address.as_str()) == me {
        "Your party".to_string()
    } else {
        format!("{}'s party", label(host))
    };
    let n = joined.len();
    let sub = format!(
        "{}{n} {}",
        p.game
            .as_ref()
            .map(|g| format!("{}, ", g.title))
            .unwrap_or_default(),
        if n == 1 { "player" } else { "players" }
    );
    let stack = avatar_stack(
        ui,
        &joined.iter().take(3).copied().collect::<Vec<_>>(),
        1.75,
        0.6,
    );
    vec![
        stack,
        ui.bx(s().col().grow())
            .child(ui.text(title, T::new(Ty::Body).w(500).trunc()))
            .child(ui.text(sub, T::new(Ty::Meta).c(tok::MUTED).trunc())),
    ]
}

/// `Group`: a small heading with its count; nothing when the group is empty.
fn group_head(ui: &mut Ui, title: &str, n: usize) -> Option<El> {
    (n > 0).then(|| {
        baseline_row(
            ui,
            s().gap(0.5).px(1.0).pb(0.25).pt(1.0),
            vec![
                (T::new(Ty::Label).w(500).c(tok::MUTED), title.into()),
                (T::new(Ty::Label).w(500).tab().c(tok::MUTED), n.to_string()),
            ],
        )
    })
}

/// Before the list: no account, a watch account, friends not turned on yet, loading, an error.
fn gate(ui: &mut Ui, d: &FriendsData) -> El {
    let fs = &d.st;
    let body = match fs.status {
        FriendsStatus::None => Some(("Add a Rare Wallet account to use friends", "Your friends know you by your Sui address, shown with your SuiNS name and NFT picture.".to_string(), Btn::new("Open Rare Wallet", T_GATE).primary().def(true))),
        FriendsStatus::Watch => Some(("This account is watch only", "Friends need an account that can sign. Pick or add one in Rare Wallet.".to_string(), Btn::new("Open Rare Wallet", T_GATE).primary().def(true))),
        FriendsStatus::Unlinked | FriendsStatus::Linking => {
            let linking = fs.status == FriendsStatus::Linking;
            Some((
                "Turn on friends",
                "Rare Wallet signs one message that lets this device handle your friends, parties and presence. It does not move funds or cost gas. Your friend list stays private on rare.market, never on chain.".to_string(),
                Btn::new(if linking { "Signing in" } else { "Turn on friends" }, T_GATE).primary().def(true).disabled(linking),
            ))
        }
        FriendsStatus::Error => Some(("Friends could not load", fs.error.clone().unwrap_or_else(|| "Check your connection.".into()), Btn::new("Try again", T_GATE).def(true))),
        _ => None,
    };
    let mut page = ui.bx(s().fill().col().px(2.5).pt(0.5)).child(ui.text_s(
        s().mb(1.25),
        "Friends",
        T::new(Ty::Title).w(600),
    ));
    if let Some((title, text, btn)) = body {
        let b = button(ui, btn);
        let mut words = ui
            .bx(s().col().max_w(34.0))
            .child(ui.text(text, T::new(Ty::Small).c(tok::MUTED).wrap()));
        if matches!(fs.status, FriendsStatus::Unlinked | FriendsStatus::Linking) {
            // how this account approves it, so the prompt is never a mystery
            if let Some(m) = &d.sign_method {
                words = words.child(ui.text_s(s().mt(0.5), m.clone(), T::new(Ty::Small).c(tok::FG).wrap()));
            }
            if let Some(err) = &fs.error {
                words = words.child(ui.text_s(
                    s().mt(0.5),
                    err.clone(),
                    T::new(Ty::Small).c(tok::NEG).wrap(),
                ));
            }
        }
        let e = ui
            .bx(s().col().items_start().gap(0.5).px(1.5).py(1.3))
            .bg(tok::L1)
            .radius(ui.r(0.5))
            .border(1.0, tok::HAIR)
            .child(ui.text(title, T::new(Ty::Row).w(600)))
            .child(words)
            .child(ui.bx(s().row().gap(0.75).mt(0.5)).child(b));
        page = page.child(ui.bx(s().col().max_w(44.0)).child(e));
    }
    page
}

// ---------------------------------------------------------------- the right side

fn detail(ui: &mut Ui, d: &FriendsData, v: &View) -> El {
    let fs = &d.st;
    let none = || ui.bx(s());
    match &v.sel {
        Sel::Add => add_pane(ui, d, &v.add),
        Sel::Privacy => match fs.me.as_ref() {
            Some(me) => privacy(ui, &me.settings),
            None => none(),
        },
        Sel::Blocked => blocked(ui, &fs.blocked),
        Sel::NewParty => new_party(ui, d),
        Sel::Party(id) => match fs.parties.iter().find(|p| &p.id == id) {
            Some(p) => party_pane(ui, d, p),
            None => none(),
        },
        Sel::Friend(a) => match fs.friends.iter().find(|f| &f.address == a) {
            Some(f) => friend_pane(ui, d, f, v),
            None => none(),
        },
        Sel::Request(a) => match fs.received.iter().find(|f| &f.address == a) {
            Some(f) => request_pane(ui, f),
            None => none(),
        },
        Sel::Sent(a) => match fs.sent.iter().find(|f| &f.address == a) {
            Some(f) => sent_pane(ui, f),
            None => none(),
        },
        Sel::Recent => super::profile::recent_pane(ui, d),
    }
}

/// A 1 px hairline along the top of a box (`border-t border-hair`).
pub fn hair_top(ui: &Ui) -> El {
    ui.bx(s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h_px(1.0))
        .bg(tok::HAIR)
}

/// `PersonHead`: the 5rem picture, the name, the SuiNS line, the address, and a line under it.
pub fn person_head(ui: &mut Ui, p: &Person, extra: Option<El>) -> El {
    let shown = rare_friends::display::shown(Some(p), "Someone");
    let mut col = ui
        .bx(s().col().shrink().min0())
        .child(ui.text(label(Some(p)), T::new(Ty::Title).w(600).trunc()));
    if let Some(sec) = shown.secondary {
        col = col.child(ui.text(sec, T::new(Ty::Body).c(tok::MUTED).trunc()));
    }
    col = col.child(ui.text(
        p.address.clone(),
        T::new(Ty::Meta).c(tok::MUTED).mono().trunc(),
    ));
    if let Some(x) = extra {
        col = col.child(ui.bx(s().mt(0.35)).child(x));
    }
    ui.bx(s().row().items_center().gap(1.25))
        .child(friend_avatar(ui, s(), p, 5.0))
        .child(col)
}

/// The panel every entry sits on (`Panel className="fade flex flex-col gap-[1.4rem] px-[1.6rem] py-[1.5rem]"`).
fn entry_panel(ui: &Ui) -> El {
    crate::kit::panel(ui, s().col().gap(1.4).px(1.6).py(1.5))
}

fn friend_pane(ui: &mut Ui, d: &FriendsData, f: &Friend, v: &View) -> El {
    let sure = v.sure;
    let fs = &d.st;
    let party = fs.active_party();
    let g = if f.presence.state != PresenceState::Offline {
        f.presence.game.as_ref()
    } else {
        None
    };
    let in_party = party.is_some_and(|p| p.members.iter().any(|m| m.address == f.address));
    let pres = presence_line(ui, s(), &f.presence, true, d.now_ms, Ty::Small);
    let head = person_head(ui, f, Some(pres));
    let mut panel = entry_panel(ui).child(head);
    if let Some(g) = g {
        let cover = ui
            .bx(s().w(5.5).aspect(2.0 / 3.0))
            .bg(tok::L3)
            .radius(ui.r(0.5))
            .clip()
            .child(game_art(ui, s().fill(), d, g, false, 5.5, 8.25));
        let words = ui
            .bx(s().col().grow())
            .child(ui.text("Playing", T::new(Ty::Label).c(tok::MUTED)))
            .child(ui.text(g.title.clone(), T::new(Ty::Row).w(600).trunc()))
            .child(ui.text(join_note(d, g), T::new(Ty::Meta).c(tok::MUTED)));
        let join = button(
            ui,
            Btn::new("Join game", T_JOIN_GAME)
                .primary()
                .def(true)
                .hint("Join"),
        );
        panel = panel.child(
            ui.bx(s().row().items_center().gap(1.1).pt(1.2))
                .child(hair_top(ui))
                .child(cover)
                .child(words)
                .child(join),
        );
    }
    // One row at every size: Remove and Block live behind X ("Remove or block"), which swaps them
    // into this row in place of the everyday actions.
    let mut acts = Vec::new();
    const X: &str = "Remove or block";
    match sure {
        None if !v.more => {
            acts.push(button(ui, Btn::new("Message", T_MESSAGE).def(g.is_none()).x(X)));
            if !in_party {
                acts.push(button(ui, Btn::new(if party.is_some() { "Invite to party" } else { "Start a party" }, T_INVITE_PARTY).x(X)));
            }
            acts.push(button(ui, Btn::new("Send a capture", T_SEND_CAPTURE).x(X)));
            acts.push(button(ui, Btn::new("Send coins", T_SEND_COINS).x(X)));
            acts.push(button(ui, Btn::new("Send an item", T_SEND_ITEM).x(X)));
        }
        None => {
            acts.push(button(ui, Btn::new("Remove friend", T_REMOVE).def(true)));
            acts.push(button(ui, Btn::new("Block", T_BLOCK)));
            acts.push(button(ui, Btn::new("Cancel", T_LESS).ghost()));
        }
        Some(k) => {
            acts.push(button(
                ui,
                Btn::new(
                    if k == Sure::Remove {
                        "Yes, remove"
                    } else {
                        "Yes, block"
                    },
                    T_YES,
                )
                .v(Variant::Outline),
            ));
            acts.push(button(ui, Btn::new("Keep", T_KEEP).ghost().def(true)));
        }
    }
    panel = panel.child(
        ui.bx(s().row().gap(0.75))
            .group(group("friend-actions", true))
            .kids(acts),
    );
    let mut items = vec![StatItem {
        label: "Friends since".into(),
        value: date_long(f.since),
    }];
    if f.presence.state == PresenceState::Offline && f.presence.last_seen.is_some_and(|t| t != 0) {
        let t = rare_friends::format::presence_text(&f.presence, false, d.now_ms);
        items.push(StatItem {
            label: "Last seen".into(),
            value: t.trim_start_matches("Last seen ").to_string(),
        });
    }
    let sl = stat_line(ui, s(), &items, true, false);
    let panel = panel.child(ui.bx(s().col().pt(1.2)).child(hair_top(ui)).maybe(sl));
    let msgs = super::profile::messages_panel(ui, d, f, v.dm_back);
    let prof = super::profile::profile_panel(ui, d, f);
    ui.bx(s().col().gap(1.25)).child(panel).child(msgs).child(prof)
}

fn request_pane(ui: &mut Ui, f: &Pending) -> El {
    let line = ui.text("Wants to be your friend", T::new(Ty::Small).c(tok::MUTED));
    let head = person_head(ui, f, Some(line));
    let acts = vec![
        button(ui, Btn::new("Accept", T_ACCEPT).primary().def(true)),
        button(ui, Btn::new("Decline", T_DECLINE)),
        button(ui, Btn::new("Block", T_BLOCK_REQ).ghost()),
    ];
    entry_panel(ui)
        .child(head)
        .child(
            ui.bx(s().row().gap(0.75))
                .group(group("request-actions", false))
                .kids(acts),
        )
        .child(ui.text(
            "They will not be told if you decline or block.",
            T::new(Ty::Small).c(tok::MUTED),
        ))
}

fn sent_pane(ui: &mut Ui, f: &Pending) -> El {
    let line = ui.text(
        format!("Request sent {}", date_short(f.at)),
        T::new(Ty::Small).c(tok::MUTED),
    );
    let head = person_head(ui, f, Some(line));
    let cancel = button(ui, Btn::new("Cancel request", T_CANCEL_REQ).def(true));
    entry_panel(ui)
        .child(head)
        .child(ui.bx(s().row().gap(0.75)).child(cancel))
}

fn blocked(ui: &mut Ui, list: &[Pending]) -> El {
    let mut panel = crate::kit::panel(ui, s().col()).child(ui.text_s(
        s().px(1.4).pb(0.4).pt(1.2),
        "Blocked players cannot send you requests or invites, and never see your status.",
        T::new(Ty::Small).c(tok::MUTED).wrap(),
    ));
    for (i, f) in list.iter().enumerate() {
        let av = friend_avatar(ui, s(), f, 2.25);
        let words = ui
            .bx(s().col().grow())
            .child(ui.text(label(Some(f)), T::new(Ty::Body).w(500).trunc()))
            .child(ui.text(
                short_address(&f.address),
                T::new(Ty::Meta).c(tok::MUTED).mono().trunc(),
            ));
        let b = button(ui, Btn::new("Unblock", T_UNBLOCK + i as u32).sm());
        panel = panel.child(
            ui.bx(s()
                .row()
                .items_center()
                .min_h(4.25)
                .gap(1.0)
                .px(1.4)
                .py(0.6))
                .child(hair_top(ui))
                .child(av)
                .child(words)
                .child(b)
                .key(key(&f.address)),
        );
    }
    panel
}

/// Settings' `SwitchRow`: title, line, the switch; A flips it.
pub fn switch_row(ui: &mut Ui, title: &str, body: &str, on: bool, first: bool, tag: u32) -> El {
    let mut el = ui.bx(s()
        .row()
        .items_center()
        .min_h(4.25)
        .wp(1.0)
        .gap(1.25)
        .px(1.4)
        .py(0.85));
    if !first {
        el = el.child(hair_top(ui));
    }
    let words = ui
        .bx(s().col().grow())
        .child(ui.text(title, T::new(Ty::Body).w(600)))
        .child(ui.text(body, T::new(Ty::Small).c(tok::MUTED).wrap()));
    let el = el.child(words).child(toggle(ui, on));
    fill_nav(
        el,
        tag,
        Some(if on { "Turn off" } else { "Turn on" }),
        None,
        false,
    )
}

/// The privacy switches, in order: (title, body, on).
pub fn privacy_rows(p: &Privacy) -> [(&'static str, &'static str, bool); 3] {
    [
        ("Show what I'm playing", "Friends see the game you are in. Off: they only see that you are online.", p.show_playing),
        ("Appear offline", "Friends see you as offline. You still see them, and get their invites.", p.appear_offline),
        ("Show my status to everyone", "Off: only friends see your status. On: anyone can look up your name, picture and status on rare.market.", p.visibility == Visibility::Everyone),
    ]
}

fn privacy(ui: &mut Ui, p: &Privacy) -> El {
    let mut sw = crate::kit::panel(ui, s().col());
    for (i, (t, b, on)) in privacy_rows(p).into_iter().enumerate() {
        sw = sw.child(switch_row(ui, t, b, on, i == 0, T_PRIVACY + i as u32));
    }
    let words = ui
        .bx(s().col().grow())
        .child(ui.text("Friends on this device", T::new(Ty::Body).w(600)))
        .child(ui.text(
            "Turning off signs this device out. Your friends and requests stay on rare.market.",
            T::new(Ty::Small).c(tok::MUTED).wrap(),
        ));
    let off = button(ui, Btn::new("Turn off", T_TURN_OFF));
    let device = crate::kit::panel(ui, s().row().items_center().gap(1.25).px(1.4).py(1.0))
        .child(words)
        .child(off);
    ui.bx(s().col().gap(1.25)).child(sw).child(device)
}

fn new_party(ui: &mut Ui, d: &FriendsData) -> El {
    let g = current_game(d);
    let acts = vec![
        button(
            ui,
            Btn::new("Invite friends", T_INVITE_FRIENDS)
                .primary()
                .def(true)
                .disabled(d.st.friends.is_empty()),
        ),
        button(ui, Btn::new("Start without invites", T_START_EMPTY)),
    ];
    let mut panel = crate::kit::panel(ui, s().col().gap(1.0).px(1.6).py(1.5))
        .child(ui.text("Start a party", T::new(Ty::Row).w(600)))
        .child(ui.text_s(
            s().max_w(38.0),
            "Invite friends, pick a game and chat. The party shows on everyone's Home, and Join opens the same game for each of them. Chat is kept for 24 hours.",
            T::new(Ty::Small).c(tok::MUTED).wrap(),
        ))
        .child(ui.bx(s().row().gap(0.75)).kids(acts));
    if let Some(g) = g {
        panel = panel.child(ui.text(
            format!(
                "The party starts with {}, the game you are playing.",
                g.title
            ),
            T::new(Ty::Meta).c(tok::MUTED),
        ));
    }
    panel
}

// ---------------------------------------------------------------- what A does

/// What a press on `tag` means, given the data and the screen state. `None`: nothing.
#[derive(Clone, Debug, PartialEq)]
pub enum Press {
    /// Only the screen's state changed (render again).
    Local,
    /// Tell the app (`Out::Act`), after any local change.
    Act(String),
    /// A on a list row: select it and move to the right side.
    Row(Sel),
}

pub fn press_tag(d: &FriendsData, v: &mut View, tag: u32) -> Option<Press> {
    let fs = &d.st;
    if (T_ROW..T_ROW + 1000).contains(&tag) {
        return list_rows(d)
            .get((tag - T_ROW) as usize)
            .cloned()
            .map(Press::Row);
    }
    if tag == T_GATE {
        return Some(Press::Act(
            match fs.status {
                FriendsStatus::None | FriendsStatus::Watch => "friends:open-wallet",
                FriendsStatus::Unlinked => "friends:turn-on",
                FriendsStatus::Error => "friends:refresh",
                _ => return None,
            }
            .into(),
        ));
    }
    let party_id = |v: &View| match &v.sel {
        Sel::Party(id) => Some(id.clone()),
        _ => None,
    };
    let sel_addr = match &v.sel {
        Sel::Friend(a) | Sel::Request(a) | Sel::Sent(a) => Some(a.clone()),
        _ => None,
    };
    Some(match tag {
        T_JOIN_GAME => Press::Act(format!("friends:join-game:{}", sel_addr?)),
        T_INVITE_PARTY => {
            let a = sel_addr?;
            match fs.active_party() {
                Some(p) => Press::Act(format!("friends:invite:{}:{a}", p.id)),
                None => Press::Act(format!("friends:start-party:{a}")),
            }
        }
        T_SEND_COINS => Press::Act(format!("friends:send-coins:{}", sel_addr?)),
        T_MESSAGE | T_DM_PANEL => Press::Act(format!("friends:dm-write:{}", sel_addr?)),
        T_PROFILE => return None,
        T_SEND_CAPTURE => Press::Act(format!("friends:send-capture:{}", sel_addr?)),
        t if (T_RECENT..T_RECENT + 100).contains(&t) => {
            let a = super::profile::recent_list(d).get((t - T_RECENT) as usize)?.1.address.clone();
            Press::Row(Sel::Friend(a))
        }
        t if (T_SEND_PICK..T_SEND_PICK + 200).contains(&t) => {
            let (kind, url) = match &v.picker {
                Some(Picker::Send { kind, url }) => (kind.clone(), url.clone()),
                _ => return None,
            };
            let a = fs.friends.get((t - T_SEND_PICK) as usize)?.address.clone();
            v.picker = None;
            Press::Act(format!("friends:send-to:{a}:{kind}:{url}"))
        }
        T_SEND_CANCEL => {
            v.picker = None;
            Press::Local
        }
        T_SEND_ITEM => {
            v.picker = Some(Picker::Item(sel_addr?));
            Press::Act("friends:load-items".into())
        }
        T_REMOVE => {
            v.sure = Some(Sure::Remove);
            v.more = false;
            Press::Local
        }
        T_BLOCK => {
            v.sure = Some(Sure::Block);
            v.more = false;
            Press::Local
        }
        T_KEEP | T_LESS => {
            v.sure = None;
            v.more = false;
            Press::Local
        }
        T_YES => {
            let op = if v.sure? == Sure::Remove {
                "remove"
            } else {
                "block"
            };
            v.sure = None;
            Press::Act(format!("friends:graph:{op}:{}", sel_addr?))
        }
        T_ACCEPT => Press::Act(format!("friends:graph:accept:{}", sel_addr?)),
        T_DECLINE => Press::Act(format!("friends:graph:decline:{}", sel_addr?)),
        T_BLOCK_REQ => Press::Act(format!("friends:graph:block:{}", sel_addr?)),
        T_CANCEL_REQ => Press::Act(format!("friends:graph:cancel:{}", sel_addr?)),
        t if (T_UNBLOCK..T_UNBLOCK + 100).contains(&t) => Press::Act(format!(
            "friends:graph:unblock:{}",
            fs.blocked.get((t - T_UNBLOCK) as usize)?.address
        )),
        t if (T_PRIVACY..T_PRIVACY + 3).contains(&t) => {
            let p = &fs.me.as_ref()?.settings;
            let (field, val) = match t - T_PRIVACY {
                0 => ("showPlaying", !p.show_playing),
                1 => ("appearOffline", !p.appear_offline),
                _ => ("visibility", p.visibility != Visibility::Everyone),
            };
            Press::Act(format!("friends:privacy:{field}:{val}"))
        }
        T_TURN_OFF => Press::Act("friends:turn-off".into()),
        T_INVITE_FRIENDS => {
            if fs.friends.is_empty() {
                return None;
            }
            v.picker = Some(Picker::Invite(None));
            v.picked.clear();
            Press::Local
        }
        T_START_EMPTY => Press::Act("friends:start-party".into()),
        T_ADD_FIELD => Press::Act("friends:add-by-name".into()),
        T_PICK_USERNAME => Press::Act("friends:pick-username".into()),
        T_MORE => {
            v.add.more = !v.add.more;
            if v.add.more && d.code.is_none() {
                return Some(Press::Act("friends:new-code".into()));
            }
            Press::Local
        }
        T_ENTER_CODE => Press::Act("friends:enter-code".into()),
        T_NEW_CODE => Press::Act("friends:new-code".into()),
        T_JOIN_PARTY => Press::Act(format!("friends:join-party:{}", party_id(v)?)),
        T_DECLINE_PARTY | T_LEAVE => Press::Act(format!("friends:leave-party:{}", party_id(v)?)),
        T_PARTY_JOIN_GAME => Press::Act(format!("friends:party-join-game:{}", party_id(v)?)),
        T_WRITE => Press::Act(format!("friends:write:{}", party_id(v)?)),
        T_PARTY_INVITE => {
            v.picker = Some(Picker::Invite(Some(party_id(v)?)));
            v.picked.clear();
            Press::Local
        }
        T_PICK_GAME => {
            v.picker = Some(Picker::Game(party_id(v)?));
            Press::Local
        }
        t if (T_PICK_FRIEND..T_PICK_FRIEND + 100).contains(&t) => {
            let pid = match &v.picker {
                Some(Picker::Invite(p)) => p.clone(),
                _ => return None,
            };
            let party = pid
                .as_ref()
                .and_then(|id| fs.parties.iter().find(|p| &p.id == id));
            let list = super::pickers::invite_list(d, party);
            let a = list.get((t - T_PICK_FRIEND) as usize)?.address.clone();
            if let Some(i) = v.picked.iter().position(|x| *x == a) {
                v.picked.remove(i);
            } else if v.picked.len() < invite_room(party) {
                v.picked.push(a);
            }
            Press::Local
        }
        T_INVITE_GO => {
            if v.picked.is_empty() {
                return None;
            }
            let to = v.picked.join(",");
            let pid = match &v.picker {
                Some(Picker::Invite(p)) => p.clone(),
                _ => return None,
            };
            v.picker = None;
            v.picked.clear();
            Press::Act(match pid {
                Some(id) => format!("friends:invite:{id}:{to}"),
                None => format!("friends:start-party:{to}"),
            })
        }
        T_INVITE_CANCEL => {
            v.picker = None;
            v.picked.clear();
            Press::Local
        }
        t if (T_GAME..T_GAME + 100).contains(&t) => {
            let pid = match &v.picker {
                Some(Picker::Game(p)) => p.clone(),
                _ => return None,
            };
            let g = picker_games(d).get((t - T_GAME) as usize)?.id.clone();
            v.picker = None;
            Press::Act(format!("friends:set-game:{pid}:{g}"))
        }
        t if (T_ITEM..T_ITEM + 100).contains(&t) => {
            let a = match &v.picker {
                Some(Picker::Item(a)) => a.clone(),
                _ => return None,
            };
            let it = d.items.as_ref()?.get((t - T_ITEM) as usize)?;
            v.picker = None;
            Press::Act(format!("friends:send-item:{a}:{}", it.id))
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::super::model::{mock, mock_party};
    use super::*;

    #[test]
    fn rows_follow_the_web_list() {
        let d = mock();
        let rows = list_rows(&d);
        let keys: Vec<String> = rows.iter().map(|r| r.key()).collect();
        assert_eq!(keys[0], "add");
        assert_eq!(keys[1], "new-party");
        assert!(keys[2].starts_with("request:"));
        assert_eq!(keys.last().unwrap(), "privacy");
        assert_eq!(rows.len(), 1 + 1 + 1 + 7 + 1 + 1);
        let p = mock_party();
        assert_eq!(list_rows(&p)[1], Sel::Party("mockparty01".into()));
    }

    #[test]
    fn words_match_the_web() {
        let d = mock();
        let p = &d.st.me.as_ref().unwrap().settings;
        assert_eq!(privacy_line(Some(p)), "Friends see what you play");
        assert_eq!(
            privacy_line(Some(&Privacy {
                appear_offline: true,
                ..p.clone()
            })),
            "Appearing offline"
        );
        assert_eq!(
            join_note(&d, &GameRef::web("web:act", "ACT", "u")),
            "Opens the same game; group up in its party screen"
        );
        assert_eq!(
            join_note(&d, &GameRef::steam("steam:1", "B", 1)),
            "Not on this device: opens it in Steam"
        );
    }

    #[test]
    fn a_on_remove_asks_once_more_then_acts() {
        let d = mock();
        let mira = d.st.friends[0].address.clone();
        let mut v = View {
            sel: Sel::Friend(mira.clone()),
            ..View::default()
        };
        assert_eq!(press_tag(&d, &mut v, T_REMOVE), Some(Press::Local));
        assert_eq!(v.sure, Some(Sure::Remove));
        assert_eq!(
            press_tag(&d, &mut v, T_YES),
            Some(Press::Act(format!("friends:graph:remove:{mira}")))
        );
        assert_eq!(v.sure, None);
    }

    #[test]
    fn the_invite_picker_toggles_up_to_the_room_left() {
        let d = mock_party();
        let mut v = View {
            sel: Sel::Party("mockparty01".into()),
            ..View::default()
        };
        assert_eq!(press_tag(&d, &mut v, T_PARTY_INVITE), Some(Press::Local));
        assert_eq!(v.picker, Some(Picker::Invite(Some("mockparty01".into()))));
        press_tag(&d, &mut v, T_PICK_FRIEND + 1);
        assert_eq!(v.picked.len(), 1);
        press_tag(&d, &mut v, T_PICK_FRIEND + 1);
        assert!(v.picked.is_empty());
        press_tag(&d, &mut v, T_PICK_FRIEND);
        let out = press_tag(&d, &mut v, T_INVITE_GO).unwrap();
        assert!(matches!(out, Press::Act(ref a) if a.starts_with("friends:invite:mockparty01:0x")));
        assert_eq!(v.picker, None);
    }
}
