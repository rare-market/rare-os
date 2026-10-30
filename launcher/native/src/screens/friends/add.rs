//! Add a friend (friends/ui/AddPane.tsx): one box for a username, a SuiNS name or an address (the
//! on-screen keyboard shows who it is as you type). Under "Other ways to add": a friend's code, or
//! this player's QR code with its 6-letter code, valid 15 minutes.

use super::bits::{lucide, qr};
use super::model::FriendsData;
use super::screen::{T_ADD_FIELD, T_ENTER_CODE, T_MORE, T_NEW_CODE, T_PICK_USERNAME};
use crate::kit::{button, Btn};
use crate::screens::market::host::*;
use rare_render::view::El;

/// The pane's own state: "Other ways to add" open.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AddState {
    pub more: bool,
}

/// "K7Q 4MX"
pub fn spaced_code(c: &str) -> String {
    let mid = c.char_indices().nth(3).map(|(i, _)| i).unwrap_or(c.len());
    format!("{} {}", &c[..mid], &c[mid..])
}

/// Minutes a code still works (Math.round, never below 0).
pub fn minutes_left(expires_at: i64, now: i64) -> i64 {
    (((expires_at - now) as f64 / 60_000.0) + 0.5)
        .floor()
        .max(0.0) as i64
}

pub fn add_pane(ui: &mut Ui, d: &FriendsData, st: &AddState) -> El {
    // ---- add by username
    let field = ui
        .bx(s().row().items_center().h(3.25).wp(1.0).gap(0.8).px(1.0))
        .bg(tok::BG)
        .radius(ui.r(0.5))
        .border(1.0, tok::BORDER_LIGHT)
        .child(lucide(ui, "search", 1.1, tok::MUTED.alpha(0.8)))
        .child(ui.text(
            "Username, name.sui or 0x...",
            T::new(Ty::Row).c(tok::MUTED.alpha(0.8)).trunc(),
        ));
    let field = fill_nav(field, T_ADD_FIELD, Some("Type"), None, false);
    let mut by_name = crate::kit::panel(ui, s().col().gap(0.9).px(1.4).py(1.2))
        .child(
            ui.bx(s().col())
                .child(ui.text("Add by username", T::new(Ty::Body).w(600)))
                .child(ui.text("Type a username. A SuiNS name like name.sui or a 0x address works too. They get a request to accept.", T::new(Ty::Small).c(tok::MUTED).wrap())),
        )
        .child(field);
    if d.my_username.is_none() {
        let pick = button(ui, Btn::new("Pick a username", T_PICK_USERNAME).sm());
        by_name = by_name.child(
            ui.bx(s().row().items_center().gap(1.0))
                .child(ui.text_s(
                    s().grow(),
                    "Friends find you by your username. You have not picked one yet.",
                    T::new(Ty::Small).c(tok::MUTED).wrap(),
                ))
                .child(pick),
        );
    }

    // ---- the disclosure
    let more = ui
        .bx(s()
            .row()
            .items_center()
            .min_h(3.25)
            .wp(1.0)
            .gap(0.75)
            .px(1.4))
        .radius(ui.r(0.5))
        .child(ui.text(
            "Other ways to add",
            T::new(Ty::Body).w(600).c(tok::fg(0.85)),
        ))
        .child(lucide(
            ui,
            if st.more {
                "chevron-up"
            } else {
                "chevron-down"
            },
            1.1,
            tok::fg(0.85),
        ));
    let more = fill_nav(
        more,
        T_MORE,
        Some(if st.more { "Hide" } else { "Show" }),
        None,
        false,
    );

    let mut col = ui.bx(s().col().gap(1.25)).child(by_name).child(more);
    if st.more {
        let enter = button(ui, Btn::new("Enter a code", T_ENTER_CODE));
        let code_row = ui
            .bx(s()
                .row()
                .items_center()
                .min_h(4.25)
                .gap(1.25)
                .px(1.4)
                .py(0.85))
            .child(
                ui.bx(s().col().grow())
                    .child(ui.text("Friend code", T::new(Ty::Body).w(600)))
                    .child(ui.text(
                        "The code under your friend's QR code. You become friends right away.",
                        T::new(Ty::Small).c(tok::MUTED).wrap(),
                    )),
            )
            .child(enter);
        col = col.child(crate::kit::panel(ui, s().col()).child(code_row));

        let live = d.code.as_ref().filter(|c| c.expires_at > d.now_ms);
        let pic = match live {
            Some(c) => qr(ui, s(), &c.url, 12.5),
            None => ui.bx(s().size(12.5)).bg(tok::L3).radius(ui.r(0.375)),
        };
        let left = live
            .map(|c| minutes_left(c.expires_at, d.now_ms))
            .unwrap_or(0);
        let works = if live.is_some() {
            format!(
                " Works for {left} more {}.",
                if left == 1 { "minute" } else { "minutes" }
            )
        } else {
            String::new()
        };
        let big = ui.text_s(
            s().mt(0.9).h(2.25),
            live.map(|c| spaced_code(&c.code)).unwrap_or_default(),
            T::new(Ty::Px(2.25, 2.25)).w(600).track(0.12).tab(),
        );
        let fresh = button(ui, Btn::new("New code", T_NEW_CODE));
        let words = ui
            .bx(s().col().grow())
            .child(ui.text("Your QR code", T::new(Ty::Label).c(tok::MUTED)))
            .child(ui.text_s(s().mt(0.2), d.my_name.clone(), T::new(Ty::Row).w(600).trunc()))
            .child(big)
            .child(ui.text_s(s().mt(0.9).max_w(30.0), format!("A friend scans it with their phone, or types the code in Rare Launcher.{works}"), T::new(Ty::Small).c(tok::MUTED).wrap()))
            .child(ui.bx(s().row().mt(1.0).gap(0.75)).child(fresh));
        col = col.child(
            crate::kit::panel(ui, s().row().items_center().gap(2.0).px(1.6).py(1.5))
                .child(pic)
                .child(words),
        );
    }
    col
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_reads_in_two_halves_with_minutes_left() {
        assert_eq!(spaced_code("K7Q4MX"), "K7Q 4MX");
        assert_eq!(minutes_left(15 * 60_000, 0), 15);
        assert_eq!(minutes_left(90_000, 0), 2);
        assert_eq!(minutes_left(0, 60_000), 0);
    }
}
