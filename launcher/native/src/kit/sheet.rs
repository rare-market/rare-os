//! The launcher's standard sheet (trade/parts.tsx `Sheet`): a centred card on the dimmed page,
//! title and line on top over a hairline, the body, an optional footer of buttons. It sits in the
//! sheet layer above the page (the host mounts it there) and leaves the hint bar visible (`bottom-[3rem]`), traps focus in its
//! scope and fades in rising 0.4rem (`.fade-in`). Options, View and a failed launch use it.

use super::tok;
use super::ui::{s, Ty, Ui, T};
use rare_render::scene::Shadow;
use rare_render::view::{key, El, Motion};

/// `title`, `sub`, the body's children, and the footer's (none: no footer). `scope` is the focus
/// scope the sheet traps (higher than whatever is under it). A body taller than the window scrolls
/// (`overflow-y-auto`) and follows focus.
pub fn sheet(ui: &mut Ui, title: &str, sub: Option<&str>, body: Vec<El>, footer: Vec<El>, wide: bool, scope: u32) -> El {
    let mut head = ui.bx(s().col().px(2.0).pb(1.0).pt(1.5)).child(ui.text(title, T::new(Ty::Row).w(600)));
    if let Some(sub) = sub {
        head = head.child(ui.text(sub, T::new(Ty::Small).c(tok::MUTED)));
    }
    head = head.child(ui.bx(s().abs(None, Some(0.0), Some(0.0), Some(0.0)).h_px(1.0)).bg(tok::HAIR));
    let mut card = ui
        .bx(s().col().wp(1.0).max_w(if wide { 60.0 } else { 44.0 }).max_hp(1.0))
        .bg(tok::BG)
        .radius(ui.r(1.25))
        .border(1.0, tok::BORDER_LIGHT)
        .shadow(&Shadow::xl2(0.4))
        .clip()
        .enter(Motion::FADE_IN)
        .child(head)
        .child(ui.bx(s().col().px(2.0).py(1.25).shrink().min_h0()).scrolls_y(key(&("sheet-body", title, scope))).kids(body));
    if !footer.is_empty() {
        card = card.child(ui.bx(s().row().gap(0.5).px(2.0).py(1.0)).child(ui.bx(s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h_px(1.0)).bg(tok::HAIR)).kids(footer));
    }
    ui.bx(s().abs(Some(0.0), Some(0.0), Some(3.0), Some(0.0)).row().items_center().justify_center().p(1.5))
        .scope(scope)
        .child(ui.bx(s().fill()).bg(tok::BG.alpha(0.8)))
        .child(card)
}
