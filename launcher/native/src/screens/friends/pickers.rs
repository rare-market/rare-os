//! The pickers that take the right side over until B (FriendsScreen.tsx): invite friends, pick the
//! party's game, pick an item to send.

use super::bits::*;
use super::model::{FriendsData, LibGame};
use super::screen::{hair_top, T_GAME, T_INVITE_CANCEL, T_INVITE_GO, T_ITEM, T_PICK_FRIEND};
use crate::kit::art::{art, ArtFit};
use crate::kit::{button, empty, group, nav, tile, Btn};
use crate::screens::market::host::*;
use rare_friends::types::{Friend, GameRef, Party};
use rare_render::view::{key, El};

/// `PickerHead`: a title and a muted note on one baseline.
fn picker_head(ui: &mut Ui, title: &str, note: &str) -> El {
    baseline_row(
        ui,
        s().mb(0.9).gap(0.75),
        vec![
            (T::new(Ty::Row).w(600), title.into()),
            (T::new(Ty::Small).c(tok::MUTED), note.into()),
        ],
    )
}

/// Friends not in the party yet.
pub fn invite_list<'a>(d: &'a FriendsData, party: Option<&Party>) -> Vec<&'a Friend> {
    d.st.friends
        .iter()
        .filter(|f| !party.is_some_and(|p| p.members.iter().any(|m| m.address == f.address)))
        .collect()
}

/// How many more fit (a party holds 8).
pub fn invite_room(party: Option<&Party>) -> usize {
    7 - party
        .map(|p| p.members.len().saturating_sub(1))
        .unwrap_or(0)
}

pub fn invite_picker(
    ui: &mut Ui,
    d: &FriendsData,
    party_id: Option<&str>,
    picked: &[String],
) -> El {
    let party = party_id.and_then(|id| d.st.parties.iter().find(|p| p.id == id));
    let list = invite_list(d, party);
    let room = invite_room(party);
    let head = picker_head(ui, "Invite friends", &format!("Up to {room} more"));
    let mut panel = crate::kit::panel(ui, s().col());
    for (i, f) in list.iter().enumerate() {
        let on = picked.contains(&f.address);
        let av = friend_avatar(ui, s(), f, 2.25);
        let pres = presence_line(ui, s(), &f.presence, false, d.now_ms, Ty::Meta);
        let words = ui
            .bx(s().col().grow())
            .child(ui.text(label(Some(f)), T::new(Ty::Body).w(500).trunc()))
            .child(pres);
        let mut check = ui
            .bx(s().size(1.5).row().items_center().justify_center())
            .radius(ui.r(0.25));
        check = if on {
            check
                .bg(tok::FG)
                .child(crate::kit::icon(ui, "check", 1.0, tok::BG))
        } else {
            check.border(1.0, tok::BORDER_LIGHT)
        };
        let mut row = ui.bx(s()
            .row()
            .items_center()
            .min_h(4.0)
            .wp(1.0)
            .gap(1.0)
            .px(1.4)
            .py(0.5));
        if i > 0 {
            row = row.child(hair_top(ui));
        }
        let row = row.child(av).child(words).child(check);
        panel = panel.child(
            fill_nav(
                row,
                T_PICK_FRIEND + i as u32,
                Some(if on { "Remove" } else { "Add" }),
                None,
                false,
            )
            .key(key(&f.address)),
        );
    }
    if list.is_empty() {
        panel = panel.child(ui.text_s(
            s().px(1.4).py(1.2),
            "Every friend is already in this party.",
            T::new(Ty::Small).c(tok::MUTED),
        ));
    }
    let n = picked.len();
    let go = button(
        ui,
        Btn::new(
            if n > 1 {
                format!("Invite {n} friends")
            } else {
                "Invite".into()
            },
            T_INVITE_GO,
        )
        .primary()
        .disabled(n == 0),
    );
    let cancel = button(ui, Btn::new("Cancel", T_INVITE_CANCEL).ghost());
    ui.bx(s().col())
        .child(head)
        .child(panel)
        .child(ui.bx(s().row().mt(1.0).gap(0.75)).child(go).child(cancel))
}

/// Installed games, last played first, at most 18 (GamePicker).
pub fn picker_games(d: &FriendsData) -> Vec<&LibGame> {
    let mut v: Vec<&LibGame> = d
        .library
        .iter()
        .filter(|g| g.source != "apps" && g.installed)
        .collect();
    v.sort_by(|a, b| {
        b.last_played
            .unwrap_or(0.0)
            .partial_cmp(&a.last_played.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    v.truncate(18);
    v
}

/// The width of one of six grid columns on the right side, in rem.
fn grid_col(ui: &Ui) -> f32 {
    // page px 2.5 x 2, the list (24rem less its -1rem margin), the 2rem gap; six columns, 1rem gaps.
    let right = ui.w / ui.rem - 5.0 - 23.0 - 2.0;
    (right - 5.0) / 6.0
}

fn as_ref(g: &LibGame) -> GameRef {
    match g.id.strip_prefix("steam:").and_then(|a| a.parse().ok()) {
        Some(appid) => GameRef::steam(&g.id, &g.title, appid),
        None => GameRef::other(&g.id, &g.title),
    }
}

pub fn game_picker(ui: &mut Ui, d: &FriendsData) -> El {
    let games = picker_games(d);
    let head = picker_head(ui, "Pick the party's game", "Join opens it for everyone");
    let w = grid_col(ui);
    let mut grid = ui
        .bx(s().grid(&[Track::Fr(1.0); 6]).gap(1.0))
        .group(group("party-games", false));
    for (i, g) in games.iter().enumerate() {
        let r = as_ref(g);
        let pic = game_art(ui, s().fill(), d, &r, false, w, w * 1.5);
        let t = nav(
            tile(
                ui.bx(s().wp(1.0).aspect(2.0 / 3.0))
                    .bg(tok::L3)
                    .radius(ui.r(0.5))
                    .clip()
                    .child(pic),
                1.06,
            ),
            T_GAME + i as u32,
            Some("Pick"),
            None,
            false,
        );
        grid = grid.child(
            ui.bx(s().col().min0())
                .child(t)
                .child(ui.text_s(
                    s().mt(0.6),
                    g.title.clone(),
                    T::new(Ty::Meta).w(500).c(tok::fg(0.85)).trunc(),
                ))
                .key(key(&g.id)),
        );
    }
    let mut col = ui.bx(s().col()).child(head).child(grid);
    if games.is_empty() {
        col = col.child(empty(ui, s(), "No games installed yet", None, Vec::new()));
    }
    col
}

pub fn item_picker(ui: &mut Ui, d: &FriendsData, address: &str) -> El {
    let who = d.st.person_of(address).cloned();
    let head = picker_head(
        ui,
        &format!("Send an item to {}", label(who.as_ref())),
        "Rare Wallet shows the review before anything is sent",
    );
    let mut col = ui.bx(s().col()).child(head);
    let items = d.items.as_deref().unwrap_or_default();
    if d.items.as_ref().is_some_and(|v| v.is_empty()) {
        col = col.child(empty(
            ui,
            s(),
            "No collectibles in this wallet",
            None,
            Vec::new(),
        ));
    }
    let w = grid_col(ui);
    let mut grid = ui
        .bx(s().grid(&[Track::Fr(1.0); 6]).gap(1.0))
        .group(group("send-items", false));
    for (i, it) in items.iter().enumerate() {
        // Resolved to rare.market's chain at the drawn size by kit::art.
        let chain: Vec<String> = it.image_url.iter().cloned().collect();
        let fb = ui
            .bx(s().fill().row().items_center().justify_center())
            .bg(tok::L3)
            .child(ui.text(
                crate::kit::market::initial(&it.name),
                T::new(Ty::px(1.6)).w(600).c(tok::MUTED.alpha(0.6)),
            ));
        let pic = art(ui, s().fill(), &chain, w, w, ArtFit::Cover(0.5), Some(fb));
        let t = nav(
            tile(
                ui.bx(s().wp(1.0).aspect(1.0))
                    .bg(tok::L3)
                    .radius(ui.r(0.5))
                    .border(1.0, tok::HAIR)
                    .clip()
                    .child(pic),
                1.06,
            ),
            T_ITEM + i as u32,
            Some("Send"),
            None,
            false,
        );
        let mut cell = ui.bx(s().col().min0()).child(t).child(ui.text_s(
            s().mt(0.6),
            it.name.clone(),
            T::new(Ty::Meta).w(500).c(tok::fg(0.85)).trunc(),
        ));
        if let Some(c) = &it.collection {
            cell = cell.child(ui.text(c.clone(), T::new(Ty::Label).c(tok::MUTED).trunc()));
        }
        grid = grid.child(cell.key(key(&it.id)));
    }
    col.child(grid)
}

#[cfg(test)]
mod tests {
    use super::super::model::{mock, mock_party};
    use super::*;

    #[test]
    fn invite_list_leaves_out_members_and_counts_the_room() {
        let d = mock_party();
        let p = d.st.active_party().unwrap();
        assert_eq!(invite_list(&d, Some(p)).len(), 5);
        assert_eq!(invite_room(Some(p)), 5);
        let d = mock();
        assert_eq!(invite_list(&d, None).len(), 7);
        assert_eq!(invite_room(None), 7);
    }
}
