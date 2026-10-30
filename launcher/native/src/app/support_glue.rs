//! Settings > System > Support over the page: the time zone picker, the factory reset sheet and
//! the logs' link (screens/settings/supportsheets.rs), and what those sheets and the Support
//! workers hand back (screens/settings/support.rs).

use super::*;
use crate::screens::overlay::Screen;
use crate::screens::settings::support::{self, SheetReq};
use crate::screens::settings::supportsheets::{LinkSheet, ResetSheet, TzSheet};

impl App {
    /// `support:tz-open`, `support:reset-open`.
    pub(super) fn support_open(&mut self, a: &str) {
        let now = clock::secs();
        match a {
            "support:tz-open" => {
                let st = support::get();
                let zones = if st.zones.is_empty() { rare_system::support::zones() } else { st.zones };
                let current = st.time.and_then(|t| t.zone).or_else(|| rare_system::support::time_info().zone);
                self.open_support_sheet(move |cx| Box::new(TzSheet::new(cx, zones, current, now)));
            }
            "support:reset-open" => {
                if self.fixture || rare_system::support::reset_supported() {
                    self.open_support_sheet(move |cx| Box::new(ResetSheet::new(cx, now)));
                } else {
                    self.st.toast("Needs the next system update.", Tone::Info, now);
                }
            }
            _ => {}
        }
    }

    /// Acts the sheets finished with, and the logs' link once it is uploaded.
    pub(super) fn support_tick(&mut self) {
        for a in support::take_requests() {
            if a == "support:clock" {
                self.minute();
            } else {
                self.settings_act(&a, None);
            }
        }
        for s in support::take_sheets() {
            match s {
                SheetReq::Link(link) => {
                    let now = clock::secs();
                    self.close_legacy();
                    self.open_support_sheet(move |cx| Box::new(LinkSheet::new(cx, &link, now)));
                }
            }
        }
    }

    fn open_support_sheet(&mut self, build: impl FnOnce(&mut crate::screens::overlay::Cx) -> Box<dyn Screen>) {
        let root = self.scene.root;
        let before = self.scene.children(root);
        let screen = {
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            build(&mut cx)
        };
        let nodes: Vec<Id> = self.scene.children(root).into_iter().filter(|n| !before.contains(n)).collect();
        for n in &nodes {
            self.scene.n(*n).z = layer::SHEET + 5;
        }
        self.fx.fb(Feedback::Open);
        self.legacy = Some((screen, nodes));
    }
}
