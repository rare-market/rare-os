//! A party (friends/ui/PartyPane.tsx): its game (Join opens it here), the members with their
//! presence, and the text chat (short messages over the notify stream, kept 24 hours). An invite
//! shows Join party and Decline.

use super::bits::*;
use super::model::FriendsData;
use super::screen::{
    hair_top, T_DECLINE_PARTY, T_JOIN_PARTY, T_LEAVE, T_PARTY_INVITE, T_PARTY_JOIN_GAME,
    T_PICK_GAME, T_WRITE,
};
use crate::kit::{button, group, Btn};
use crate::screens::market::host::*;
use rare_friends::types::{MemberState, Party};
use rare_render::view::{key, El};

/// The line over the game: who invited you, or whose party it is.
pub fn party_kicker(p: &Party, me: Option<&str>) -> String {
    let host = p
        .members
        .iter()
        .find(|m| m.address == p.host)
        .map(|m| &m.person);
    let invited = p
        .members
        .iter()
        .any(|m| Some(m.address.as_str()) == me && m.state == MemberState::Invited);
    if invited {
        format!("{} invited you", label(host))
    } else if Some(p.host.as_str()) == me {
        "Your party".into()
    } else {
        format!("{}'s party", label(host))
    }
}

/// "2 in the party, 1 invited"
pub fn party_count(p: &Party) -> String {
    let inv = p.invited();
    format!(
        "{} in the party{}",
        p.joined(),
        if inv > 0 {
            format!(", {inv} invited")
        } else {
            String::new()
        }
    )
}

pub fn party_pane(ui: &mut Ui, d: &FriendsData, p: &Party) -> El {
    let me = d.st.address.as_deref();
    let invited = p
        .members
        .iter()
        .any(|m| Some(m.address.as_str()) == me && m.state == MemberState::Invited);
    let is_host = Some(p.host.as_str()) == me;
    let g = p.game.as_ref();
    let playing_it = g.is_some_and(|g| d.running.contains(&g.id));

    // ---- head: cover, whose party, the game, the count
    let mut head = ui.bx(s().row().items_center().gap(1.25));
    if let Some(g) = g {
        head = head.child(
            ui.bx(s().w(5.5).aspect(2.0 / 3.0))
                .bg(tok::L3)
                .radius(ui.r(0.5))
                .clip()
                .child(game_art(ui, s().fill(), d, g, false, 5.5, 8.25)),
        );
    }
    head = head.child(
        ui.bx(s().col().grow())
            .child(ui.text(party_kicker(p, me), T::new(Ty::Label).c(tok::MUTED)))
            .child(
                ui.text(
                    g.map(|g| g.title.clone())
                        .unwrap_or_else(|| "No game picked yet".into()),
                    T::new(Ty::Title).w(600).trunc(),
                ),
            )
            .child(ui.text(party_count(p), T::new(Ty::Meta).c(tok::MUTED))),
    );

    // ---- actions
    let mut acts = Vec::new();
    if invited {
        acts.push(button(
            ui,
            Btn::new("Join party", T_JOIN_PARTY).primary().def(true),
        ));
        acts.push(button(ui, Btn::new("Decline", T_DECLINE_PARTY)));
    } else {
        if g.is_some() && !playing_it {
            acts.push(button(
                ui,
                Btn::new("Join game", T_PARTY_JOIN_GAME).primary().def(true),
            ));
        }
        acts.push(button(
            ui,
            Btn::new("Write a message", T_WRITE).def(g.is_none() || playing_it),
        ));
        acts.push(button(ui, Btn::new("Invite friends", T_PARTY_INVITE)));
        if is_host {
            acts.push(button(
                ui,
                Btn::new(
                    if g.is_some() {
                        "Change game"
                    } else {
                        "Pick a game"
                    },
                    T_PICK_GAME,
                ),
            ));
        }
        acts.push(button(ui, Btn::new("Leave party", T_LEAVE).ghost()));
    }
    let actions = ui
        .bx(s().row().wrap().gap(0.75))
        .group(group("party-actions", true))
        .kids(acts);

    // ---- members
    let mut members = ui
        .bx(s().row().wrap().gap_xy(1.75, 0.9).pt(1.2))
        .child(hair_top(ui));
    for m in &p.members {
        let name = if Some(m.address.as_str()) == me {
            "You".to_string()
        } else {
            label(Some(&m.person))
        };
        let mut runs = vec![(T::new(Ty::Body).w(500).trunc(), name)];
        if m.address == p.host {
            runs.push((T::new(Ty::Meta).c(tok::MUTED), "Host".into()));
        }
        let top = baseline_row(ui, s().gap(0.5).min0(), runs);
        let second = if m.state == MemberState::Invited {
            ui.text("Invited", T::new(Ty::Meta).c(tok::MUTED))
        } else {
            presence_line(ui, s(), &m.presence, false, d.now_ms, Ty::Meta)
        };
        let av = friend_avatar(ui, s(), &m.person, 2.25);
        members = members.child(
            ui.bx(s().row().items_center().min_w(13.0).gap(0.7))
                .child(av)
                .child(ui.bx(s().col().shrink().min0()).child(top).child(second))
                .key(key(&m.address)),
        );
    }
    let top = crate::kit::panel(ui, s().col().gap(1.3).px(1.6).py(1.5))
        .child(head)
        .child(actions)
        .child(members);
    let mut col = ui.bx(s().col().gap(1.25)).child(top);

    // ---- chat
    if !invited {
        let head = baseline_row(
            ui,
            s().mb(0.6).gap(0.75),
            vec![
                (T::new(Ty::Body).w(600), "Party chat".into()),
                (T::new(Ty::Meta).c(tok::MUTED), "Kept for 24 hours".into()),
            ],
        );
        let msgs = d.st.messages.get(&p.id);
        let mut list = ui
            .bx(s().col().gap(0.7).max_h(17.0))
            .scrolls_y(key(&format!("party-chat:{}", p.id)));
        for m in msgs.map(|v| v.as_slice()).unwrap_or_default() {
            let who = p
                .members
                .iter()
                .find(|x| x.address == m.from)
                .map(|x| x.person.clone())
                .or_else(|| d.st.person_of(&m.from).cloned());
            let name = if Some(m.from.as_str()) == me {
                "You".to_string()
            } else {
                label(who.as_ref())
            };
            let av = match &who {
                Some(w) => friend_avatar(ui, s(), w, 1.75),
                None => ui.bx(s().size(1.75)),
            };
            let line = baseline_row(
                ui,
                s().gap(0.5),
                vec![
                    (T::new(Ty::Small).w(600), name),
                    (T::new(Ty::Label).c(tok::MUTED), time_of(m.at)),
                ],
            );
            let words = ui
                .bx(s().col().shrink().min0())
                .child(line)
                .child(ui.text(m.text.clone(), T::new(Ty::Small).c(tok::fg(0.9)).wrap()));
            list = list.child(
                ui.bx(s().row().items_start().gap(0.7))
                    .child(av)
                    .child(words)
                    .key(key(&m.id)),
            );
        }
        if msgs.is_some_and(|v| v.is_empty()) {
            list = list.child(ui.text("No messages yet.", T::new(Ty::Small).c(tok::MUTED)).key(key("party-chat-empty")));
        }
        col = col.child(
            crate::kit::panel(ui, s().col().px(1.6).py(1.2))
                .child(head)
                .child(list),
        );
    }
    col
}

#[cfg(test)]
mod tests {
    use super::super::model::mock_party;
    use super::*;

    #[test]
    fn the_party_head_reads_like_the_web() {
        let d = mock_party();
        let p = d.st.active_party().unwrap();
        assert_eq!(party_kicker(p, d.st.address.as_deref()), "mira's party");
        assert_eq!(party_count(p), "2 in the party, 1 invited");
    }
}
