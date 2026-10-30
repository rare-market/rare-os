//! Before Steam's own UI takes the screen (components/SteamHandoff.tsx): how to come back, for as
//! long as the native side waits before handing over.

use crate::kit::art::ArtFit;
use crate::kit::{self, s, tok, Ty, Ui, T};
use rare_render::scene::Shadow;
use rare_render::view::{El, Motion};

pub fn handoff(ui: &mut Ui) -> El {
    let logo = kit::art(ui, s().size(3.5), &["apps/steam.png".to_string()], 3.5, 3.5, ArtFit::Cover(0.5), None).radius(ui.r(3.5 * 0.22));
    let card = ui
        .bx(s().col().items_center().gap(0.9).w(30.0).px(2.0).py(1.8))
        .bg(tok::L2)
        .radius(ui.r(0.5))
        .border(1.0, tok::BORDER_LIGHT)
        .shadow(&Shadow::xl(0.4))
        .enter(Motion::POP)
        .child(logo)
        .child(ui.text("Opening Steam", T::new(Ty::Row).w(600)))
        .child(ui.text_s(s().wp(1.0), "Press the Rare button to come back to Rare. Steam stays open in Running until you close it.", T::new(Ty::Body).c(tok::MUTED).wrap().center()));
    ui.bx(s().fill().row().items_center().justify_center()).bg(tok::BLACK.alpha(0.55)).child(card)
}
