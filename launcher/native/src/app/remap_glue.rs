//! Settings > Controls > Button layout: the sheet over Settings (screens/settings/remap.rs), and
//! the Controls page drawn again when it closes so its summary line is current.

use super::*;

impl App {
    /// The acts of the layout sheet and its Controls row. False when it is not one of them.
    pub(super) fn remap_act(&mut self, a: &str) -> bool {
        match a {
            "controls:remap-open" => self.open_remap_sheet(),
            "controls:remap-done" => {
                self.fx.fb(Feedback::Back);
                let now = clock::secs();
                if let Some((_, screen, _)) = self.legacy_page.as_mut() {
                    let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
                    screen.command(&mut cx, "refresh", "", now);
                }
            }
            _ => return false,
        }
        true
    }

    fn open_remap_sheet(&mut self) {
        use crate::screens::settings::remap::RemapSheet;
        let now = clock::secs();
        let live = !self.fixture;
        let layout = if live { rare_input::remap::load() } else { rare_input::remap::Layout::default() };
        let root = self.scene.root;
        let before = self.scene.children(root);
        let screen: Box<dyn crate::screens::overlay::Screen> = {
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            Box::new(RemapSheet::open(&mut cx, layout, live, now))
        };
        let nodes: Vec<Id> = self.scene.children(root).into_iter().filter(|n| !before.contains(n)).collect();
        for n in &nodes {
            self.scene.n(*n).z = layer::SHEET + 5;
        }
        self.fx.fb(Feedback::Open);
        self.legacy = Some((screen, nodes));
    }
}
