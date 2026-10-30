//! Settings > Controls > In games (`Overlay::Other(KIND)`): the one button map every game and
//! emulator follows on the SuiPlay0X1 (`rare_emu::controls::STANDARD`), then each console's
//! buttons (`rare_emu::controls::CONSOLES`). docs/EMULATION.md has the same tables.

use crate::act::Act;
use crate::kit::{self, s, tok, Ty, Ui, T};
use crate::page::{hash, Acts, Page};
use crate::state::State;
use rare_emu::controls::{CONSOLES, STANDARD};
use rare_render::focus::Group;
use rare_render::view::{key, El, Motion};

pub const KIND: &str = "game-buttons";

#[derive(Default)]
pub struct GameButtonsPage;

/// One "button: what it does" line.
fn pair(ui: &mut Ui, button: &str, what: &str) -> El {
    ui.bx(s().row().items_start().gap(1.0).wp(1.0).min0())
        .child(ui.text_s(s().w(11.0), button.to_string(), T::new(Ty::Small).w(600)))
        .child(ui.text_s(s().grow().min0(), what.to_string(), T::new(Ty::Small).c(tok::fg(0.8)).wrap()))
}

fn card(ui: &mut Ui, acts: &mut Acts, title: &str, rows: &[(&str, &str)], first: bool, k: &str) -> El {
    let mut col = ui.bx(s().col().gap(0.45).wp(1.0).px(1.25).py(1.0)).child(ui.text_s(s().mb(0.25), title.to_string(), T::new(Ty::Body).w(600).trunc()));
    for (b, w) in rows {
        col = col.child(pair(ui, b, w));
    }
    kit::nav(kit::row_look(col.bg(tok::L1).radius(ui.r(0.6))), acts.tag(Act::None), None, None, first).key(key(&("game-buttons", k)))
}

impl Page for GameButtonsPage {
    fn stamp(&self, _st: &State) -> u64 {
        hash(KIND)
    }

    fn view(&mut self, ui: &mut Ui, _st: &State, acts: &mut Acts) -> El {
        let head = ui
            .bx(s().row().items_center().gap(0.75).h(2.5).mb(0.5))
            .child(kit::icon(ui, "gamepad-2", 1.5, tok::ICON2))
            .child(ui.text("Buttons in games", T::new(Ty::Title).w(700)));
        let intro = ui.text_s(
            s().mb(1.25).max_w(62.0),
            "The same in every game and emulator. Rare sets each emulator up this way and keeps your own changes.",
            T::new(Ty::Small).c(tok::MUTED).wrap(),
        );
        let mut cards = vec![card(ui, acts, "Every game", STANDARD, true, "standard")];
        for (system, rows) in CONSOLES {
            cards.push(card(ui, acts, system, rows, false, system));
        }
        ui.bx(s().col().fill().px(2.5).py(2.0))
            .bg(tok::BG)
            .enter(Motion::FADE)
            .scrolls_y(key(KIND))
            .child(head)
            .child(intro)
            .child(ui.bx(s().col().gap(0.6).max_w(70.0)).group(Group::new("game-buttons", [0.0; 4]).remember()).kids(cards))
    }
}
