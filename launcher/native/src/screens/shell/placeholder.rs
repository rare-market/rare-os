//! The empty page under tabs and overlays whose screens the app mounts itself (Market, Friends,
//! Settings, Share).

use crate::kit::{s, Ui};
use crate::page::{hash, Acts, Page};
use crate::state::State;
use rare_render::view::El;

pub struct Placeholder {
    pub title: &'static str,
    /// Full-screen (an overlay) rather than a page under the top bar.
    pub overlay: bool,
}

impl Page for Placeholder {
    fn stamp(&self, _st: &State) -> u64 {
        hash(self.title)
    }

    fn view(&mut self, ui: &mut Ui, _st: &State, _acts: &mut Acts) -> El {
        // The tab's or overlay's screen is scene-built and mounted by the app (app.rs legacy_page,
        // share_glue): this is only the empty page under it.
        ui.bx(if self.overlay { s().fill() } else { s().full() })
    }
}
