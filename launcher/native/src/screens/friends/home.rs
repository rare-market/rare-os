//! Home's friends piece (friends/ui/HomeFriends.tsx): the party (yours, or an invite) as one panel
//! with Join, and "Friends playing" as a shelf: each card is the friend's game art with their
//! picture, name and game under it. A opens the friend in Friends, X joins their game.
//!
//! Home places it with `home_friends(ui, data, base)` (tags `base..`) and maps presses on those
//! tags with `home_press`.

use super::bits::*;
use super::model::FriendsData;
use super::screen::avatar_stack;
use crate::kit::{button, group, nav, rail, tile, Btn};
use crate::screens::market::host::*;
use rare_friends::nav::FriendsTarget;
use rare_friends::types::{Friend, MemberState, Party, Person, PresenceState};
use rare_friends::FriendsStatus;
use rare_render::input::Pad;
use rare_render::view::{key, El};

/// Tag offsets from `base`.
pub const H_JOIN_PARTY: u32 = 0;
pub const H_DECLINE: u32 = 1;
pub const H_JOIN_GAME: u32 = 2;
pub const H_OPEN_PARTY: u32 = 3;
/// Friend cards: `base + H_CARD + i`.
pub const H_CARD: u32 = 100;
/// How many tags the piece may use from `base`.
pub const H_SPAN: u32 = 200;

/// Friends playing a game now (online or away, with a game), at most 12 on the shelf.
pub fn playing(d: &FriendsData) -> Vec<&Friend> {
    d.st.friends
        .iter()
        .filter(|f| f.presence.state != PresenceState::Offline && f.presence.game.is_some())
        .take(12)
        .collect()
}

/// The party the strip shows and whether it is an invite.
fn strip_party(d: &FriendsData) -> Option<(&Party, bool)> {
    match d.st.active_party() {
        Some(p) => Some((p, false)),
        None => d.st.party_invites().first().map(|p| (*p, true)),
    }
}

/// The piece as Home's sections, in order: the party strip, then the "Friends playing" rail.
/// Empty when there is nothing to show (not ready, no party, nobody playing). Home lays them out
/// with its own section spacing, as HomeFriends' fragment does.
pub fn home_friends(ui: &mut Ui, d: &FriendsData, base: u32) -> Vec<El> {
    let mut out = Vec::new();
    if d.st.status != FriendsStatus::Ready {
        return out;
    }
    if let Some((p, invite)) = strip_party(d) {
        out.push(party_strip(ui, d, p, invite, base));
    }
    let play = playing(d);
    if !play.is_empty() {
        let cards: Vec<El> = play
            .iter()
            .enumerate()
            .map(|(i, f)| friend_card(ui, d, f, base + H_CARD + i as u32))
            .collect();
        let n = play.len().to_string();
        out.push(rail(
            ui,
            s(),
            "friends-playing",
            Some("Friends playing"),
            Some(&n),
            cards,
        ));
    }
    out
}

fn friend_card(ui: &mut Ui, d: &FriendsData, f: &Friend, tag: u32) -> El {
    let g = f.presence.game.as_ref().expect("playing");
    let pic = game_art(ui, s().fill(), d, g, true, 15.0, 15.0 * 215.0 / 460.0);
    let card = ui
        .bx(s().wp(1.0).aspect(460.0 / 215.0))
        .bg(tok::L3)
        .radius(ui.r(0.5))
        .clip()
        .child(pic);
    let card = nav(
        tile(card, 1.06),
        tag,
        Some("Open"),
        Some("Join game"),
        false,
    );
    let av = friend_avatar(ui, s(), f, 1.75);
    let words = ui
        .bx(s().col().shrink().min0())
        .child(ui.text(
            label(Some(f)),
            T::new(Ty::Meta).w(500).c(tok::fg(0.85)).trunc(),
        ))
        .child(ui.text(g.title.clone(), T::new(Ty::Label).c(tok::MUTED).trunc()));
    ui.bx(s().col().w(15.0))
        .child(card)
        .child(
            ui.bx(s().row().items_center().mt(0.75).gap(0.6).px(0.1))
                .child(av)
                .child(words),
        )
        .key(key(&f.address))
}

fn party_strip(ui: &mut Ui, d: &FriendsData, p: &Party, invite: bool, base: u32) -> El {
    let me = d.st.address.as_deref();
    let host = p
        .members
        .iter()
        .find(|m| m.address == p.host)
        .map(|m| &m.person);
    let joined: Vec<&Person> = p
        .members
        .iter()
        .filter(|m| m.state == MemberState::Joined)
        .map(|m| &m.person)
        .collect();
    let g = p.game.as_ref();
    let playing_it = g.is_some_and(|g| d.running.contains(&g.id));
    let title = if invite {
        format!("{} invited you to a party", label(host))
    } else if Some(p.host.as_str()) == me {
        "Your party".into()
    } else {
        format!("{}'s party", label(host))
    };
    let names: Vec<String> = joined
        .iter()
        .map(|m| {
            if Some(m.address.as_str()) == me {
                "you".to_string()
            } else {
                label(Some(m))
            }
        })
        .collect();
    let sub = format!(
        "{}{}",
        g.map(|g| format!("{}, ", g.title)).unwrap_or_default(),
        names.join(", ")
    );
    let stack = avatar_stack(
        ui,
        &joined.iter().take(5).copied().collect::<Vec<_>>(),
        2.25,
        0.7,
    );
    let words = ui
        .bx(s().col().grow())
        .child(ui.text(title, T::new(Ty::Body).w(600).trunc()))
        .child(ui.text(sub, T::new(Ty::Small).c(tok::MUTED).trunc()));
    let mut acts = Vec::new();
    if invite {
        acts.push(button(
            ui,
            Btn::new("Join party", base + H_JOIN_PARTY).primary(),
        ));
        acts.push(button(ui, Btn::new("Decline", base + H_DECLINE)));
    } else {
        if g.is_some() && !playing_it {
            acts.push(button(
                ui,
                Btn::new("Join game", base + H_JOIN_GAME).primary(),
            ));
        }
        acts.push(button(ui, Btn::new("Open party", base + H_OPEN_PARTY)));
    }
    let buttons = ui
        .bx(s().row().gap(0.75))
        .group(group("home-party", false))
        .kids(acts);
    let panel = crate::kit::panel(ui, s().row().items_center().gap(1.25).px(1.4).py(1.0))
        .child(stack)
        .child(words)
        .child(buttons);
    ui.bx(s().col().px(2.5)).section().child(panel)
}

/// What the app does for a press on one of the piece's tags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HomeAct {
    JoinParty(String),
    LeaveParty(String),
    /// Join this game (its id) through rare-friends' join flow.
    JoinGame(rare_friends::types::GameRef),
    Open(FriendsTarget),
}

/// A on a card opens the friend in Friends, X on a card joins their game; the strip's buttons do
/// what they say.
pub fn home_press(d: &FriendsData, base: u32, tag: u32, pad: Pad) -> Option<HomeAct> {
    if !(base..base + H_SPAN).contains(&tag) {
        return None;
    }
    let off = tag - base;
    if off >= H_CARD {
        let f = playing(d).get((off - H_CARD) as usize).copied()?;
        return match pad {
            Pad::A => Some(HomeAct::Open(FriendsTarget::Friend {
                address: f.address.clone(),
            })),
            Pad::X => f.presence.game.clone().map(HomeAct::JoinGame),
            _ => None,
        };
    }
    if pad != Pad::A {
        return None;
    }
    let (p, _) = strip_party(d)?;
    Some(match off {
        H_JOIN_PARTY => HomeAct::JoinParty(p.id.clone()),
        H_DECLINE => HomeAct::LeaveParty(p.id.clone()),
        H_JOIN_GAME => HomeAct::JoinGame(p.game.clone()?),
        H_OPEN_PARTY => HomeAct::Open(FriendsTarget::Party { id: p.id.clone() }),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::super::model::{mock, mock_party};
    use super::*;

    #[test]
    fn cards_open_or_join_and_the_strip_opens_the_party() {
        let d = mock_party();
        assert_eq!(playing(&d).len(), 3);
        let mira = d.st.friends[0].address.clone();
        assert_eq!(
            home_press(&d, 500, 500 + H_CARD, Pad::A),
            Some(HomeAct::Open(FriendsTarget::Friend { address: mira }))
        );
        assert!(
            matches!(home_press(&d, 500, 500 + H_CARD, Pad::X), Some(HomeAct::JoinGame(g)) if g.id == "web:act")
        );
        assert_eq!(
            home_press(&d, 500, 500 + H_OPEN_PARTY, Pad::A),
            Some(HomeAct::Open(FriendsTarget::Party {
                id: "mockparty01".into()
            }))
        );
        assert_eq!(home_press(&d, 500, 499, Pad::A), None);
        assert_eq!(home_press(&mock(), 500, 500 + H_OPEN_PARTY, Pad::A), None);
    }
}
