//! share/entry.tsx: ways into Share from elsewhere. `share_entry` is the quick-settings style
//! button the in-game overlay drops in (`ShareEntry`); the acts are what its owner reports to the
//! app (`openShare`, `takeScreenshot`, `saveClip`).

use crate::kit::{self, icon, s, tok, Ty, Ui, T};
use rare_render::view::El;

/// Opens the Share screen (`openShare`).
pub const OPEN: &str = "share:open-screen";
/// Takes a screenshot of what's under the launcher's own panels: they close first, then 350 ms.
pub const SCREENSHOT_CLOSING: &str = "share:screenshot:close-first";
/// Saves the last 30 seconds.
pub const SAVE_CLIP: &str = "share:clip";
/// How long the panels get to close before a screenshot (entry.tsx takeScreenshot).
pub const CLOSE_FIRST_MS: u64 = 350;

/// `ShareEntry`: `flex h-[2.9rem] items-center gap-[0.6rem] rounded-card bg-l2 px-[0.9rem]
/// text-small font-medium`, the images icon, "Share", and "N new" at the end when there are new
/// captures. A `[data-nav]` row: focus fills it with layer 04.
pub fn share_entry(ui: &mut Ui, fresh: usize, tag: u32) -> El {
    let mut el = ui
        .bx(s().row().h(2.9).items_center().gap(0.6).px(0.9))
        .bg(tok::L2)
        .radius(ui.r(0.5))
        .child(icon(ui, "images", 1.15, tok::FG))
        .child(ui.text("Share", T::new(Ty::Small).w(500)));
    if fresh > 0 {
        el = el.child(ui.text_s(s().ml_auto(), format!("{fresh} new"), T::new(Ty::Meta).c(tok::MUTED)));
    }
    kit::nav(kit::row_look(el), tag, Some("Open Share"), None, false)
}

/// `?share=grid|detail|clip|uploading|done` (shareDevState): the fixture that shows that state.
pub fn dev_state(mode: &str) -> &'static str {
    match mode {
        "detail" => "share-detail",
        "clip" => "share-clip",
        "uploading" => "share-uploading",
        "done" => "share-done",
        _ => "share-grid",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rare_render::ImageLoader;
    use std::sync::Arc;

    #[test]
    fn the_entry_says_how_many_are_new() {
        let mut img = ImageLoader::new(1, Arc::new(|| {}), None);
        let mut arts = kit::Arts::default();
        let mut ui = Ui::new(24.0, 1920.0, 1200.0, &mut img, &mut arts, 0.0);
        let with = share_entry(&mut ui, 3, 7);
        let without = share_entry(&mut ui, 0, 7);
        assert_eq!(with.kids.len(), 3);
        assert_eq!(without.kids.len(), 2);
        assert_eq!(dev_state("uploading"), "share-uploading");
        assert_eq!(dev_state(""), "share-grid");
    }
}
