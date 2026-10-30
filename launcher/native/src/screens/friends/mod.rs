//! Rare Friends (launcher/src/friends/ui): the Friends screen with its panes (add, party, a friend,
//! a request, privacy, the pickers), and Home's friends piece (`home::home_friends`). Logic, types
//! and the UI-side helpers (display, format, names, nav, username rules) are rare-friends'.

pub mod add;
pub mod bits;
pub mod home;
pub mod model;
pub mod page;
pub mod party;
pub mod pickers;
pub mod profile;
pub mod screen;
pub mod tab;

use crate::screens::market::host::s;
use crate::screens::market::host::{Cx, Host, Out, Page, Screen, Ui};
use page::FriendsPage;
use rare_friends::FriendsStatus;
use rare_render::input::Pad;
use rare_render::view::El;
use rare_render::Id;

/// This area's fixtures (the golden harness shoots these by default).
pub const SHOTS: &[&str] = &[
    "friends-add",
    "friends-list",
    "friends-steam",
    "friends-request",
    "friends-sent",
    "friends-newparty",
    "friends-offline",
    "friends-privacy",
    "friends-addmore",
    "friends-party",
    "friends-invite",
    "friends-watch",
    "friends-home",
    "friends-edge-empty",
    "friends-edge-many",
    "friends-edge-convo",
    "friends-edge-convotop",
];

/// A page of this area by fixture name, built into `parent` (the shell's main area).
pub fn page(name: &str, cx: &mut Cx, parent: Id, now: f64) -> Option<Box<dyn Page>> {
    let gate = |st: FriendsStatus, err: Option<&str>| model::gate(st, err);
    let data = match name {
        "friends-party" | "friends-invite" | "friends-home" => model::mock_party(),
        "friends-watch" => gate(FriendsStatus::Watch, None),
        "friends-none" => gate(FriendsStatus::None, None),
        "friends-unlinked" => gate(
            FriendsStatus::Unlinked,
            Some("Rare Wallet could not sign the message."),
        ),
        "friends-error" => gate(FriendsStatus::Error, None),
        n if n.starts_with("friends-edge-") => model::edge(&n["friends-edge-".len()..]),
        n if n.starts_with("friends-") => model::mock(),
        _ => return None,
    };
    if name == "friends-home" {
        return Some(Box::new(HomePage::new(cx, parent, data, now)));
    }
    let mut p = FriendsPage::new(cx, parent, data, now);
    // The web references: a row focused (and A pressed on it, which moves to the right side).
    let (row, right) = match name {
        "friends-list" => (Some(3), true),
        "friends-steam" => (Some(4), true),
        "friends-request" => (Some(2), true),
        "friends-sent" => (Some(10), true),
        "friends-newparty" => (Some(1), true),
        "friends-offline" => (Some(8), true),
        "friends-privacy" => (Some(11), false),
        "friends-party" | "friends-invite" => (Some(1), true),
        "friends-add" | "friends-addmore" => (Some(0), false),
        "friends-edge-empty" => (Some(0), false),
        "friends-edge-many" => (Some(2), false),
        "friends-edge-convo" => (Some(3), false),
        "friends-edge-convotop" => (Some(3), true),
        _ => (None, false),
    };
    if let Some(i) = row {
        p.focus_row(cx, i, right, now);
    }
    if name == "friends-edge-convotop" {
        // Scrolled all the way back: the first message and "Start of your messages".
        p.v.dm_back = 499;
        p.render(cx, now);
        p.focus(cx, screen::T_DM_PANEL, now);
    }
    if name == "friends-addmore" {
        p.v.add.more = true;
        p.data.code = Some(model::mock_code(p.data.now_ms));
        p.render(cx, now);
    }
    if name == "friends-invite" {
        // Invite friends, lowkey picked, focus on kenji (scripts/shots-friends.mjs).
        p.focus(cx, screen::T_PARTY_INVITE, now);
        p.press(cx, Pad::A, now);
        p.focus(cx, screen::T_PICK_FRIEND + 1, now);
        p.press(cx, Pad::A, now);
        p.focus(cx, screen::T_PICK_FRIEND, now);
    }
    Some(Box::new(p))
}

/// Builds a screen by fixture name (the WS 13 golden harness and tests).
pub fn fixture(name: &str, cx: &mut Cx, now: f64) -> Option<Box<dyn Screen>> {
    let main = crate::screens::trade::main_area(cx);
    page(name, cx, main, now).map(|p| p as Box<dyn Screen>)
}

/// Home's friends piece on its own page (the party strip and "Friends playing"), for golden images.
pub struct HomePage {
    pub data: model::FriendsData,
    pub host: Host,
}

const HOME_BASE: u32 = 500;

impl HomePage {
    pub fn new(cx: &mut Cx, parent: Id, data: model::FriendsData, now: f64) -> HomePage {
        let mut p = HomePage {
            data,
            host: Host::new(cx, parent),
        };
        p.render(cx, now);
        p.host.focus_tag(cx, HOME_BASE + home::H_CARD, now);
        p
    }
    fn render(&mut self, cx: &mut Cx, now: f64) {
        let d = &self.data;
        self.host.render(cx, now, |ui: &mut Ui| -> El {
            let parts = home::home_friends(ui, d, HOME_BASE);
            bits::trunc_slack(ui.bx(s().fill().col().pt(1.0).gap(3.0)).kids(parts))
        });
    }
}

impl Page for HomePage {
    fn hints(&self) -> Vec<(&'static str, String)> {
        let it = self.host.nav.current_item();
        let mut v = vec![(
            "a",
            it.and_then(|f| f.hint_a.clone())
                .unwrap_or_else(|| "Select".into()),
        )];
        if let Some(x) = it.and_then(|f| f.hint_x.clone()) {
            v.push(("x", x));
        }
        v
    }
}

impl Screen for HomePage {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.host.touch_focus(cx, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        match pad {
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                if self.host.step(cx, pad, now) {
                    Out::Moved
                } else {
                    Out::None
                }
            }
            Pad::A | Pad::X => match self
                .host
                .tag()
                .and_then(|t| home::home_press(&self.data, HOME_BASE, t, pad))
            {
                Some(a) => Out::Act(format!("friends-home:{a:?}")),
                None => Out::None,
            },
            _ => Out::None,
        }
    }
    fn tick(&mut self, cx: &mut Cx, now: f64) {
        if self.host.tick(cx, now) {
            self.render(cx, now);
        }
    }
}
