//! The app side of Wi-Fi sign-in (features/portal.rs): starts the watcher, tells it when the
//! launcher is in front and when Wi-Fi changed, offers "Sign in to <network>" over the launcher and
//! toasts when the sign-in worked.

use super::*;
use crate::features::portal;

impl App {
    pub(super) fn portal_update(&mut self, now: f64) {
        if self.fixture {
            return;
        }
        portal::start(self.waker.clone());
        let front = crate::data::keep::front() && !rare_webgame::visible();
        portal::set_front(front);
        portal::note_wifi(self.st.device.as_ref().and_then(|d| d.wifi.clone()));
        if let Some(name) = portal::take_connected() {
            self.st.toast(format!("Connected to {name}"), Tone::Info, now);
        }
        // Offered once, when nothing else is up over the launcher.
        if front && self.legacy.is_none() && self.st.osk.is_none() {
            if let Some(name) = portal::take_prompt() {
                self.open_portal_sheet(&name);
            }
        }
    }

    fn open_portal_sheet(&mut self, name: &str) {
        use crate::screens::settings::ossheets::OsSheet;
        let now = clock::secs();
        let root = self.scene.root;
        let before = self.scene.children(root);
        let screen: Box<dyn crate::screens::overlay::Screen> = {
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            Box::new(OsSheet::offer(&mut cx, &format!("Sign in to {name}"), "Wi-Fi", "This network needs a sign-in before you can go online.", "Sign in", "network:portal", now))
        };
        let nodes: Vec<Id> = self.scene.children(root).into_iter().filter(|n| !before.contains(n)).collect();
        for n in &nodes {
            self.scene.n(*n).z = layer::SHEET + 5;
        }
        self.fx.fb(Feedback::Open);
        self.legacy = Some((screen, nodes));
    }

    /// Settings' "Sign in" button and the offer's yes: the sign-in page in the web window.
    pub(super) fn portal_sign_in(&mut self, now: f64) {
        if let Err(e) = portal::open() {
            log::warn!("wifi sign-in: {e}");
            self.st.toast("Couldn't open the sign-in page. Try again.", Tone::Error, now);
        }
    }
}
