//! The price alert sheet on a live coin page: opened from Trade's "Price alert" button over the
//! launcher, saved on rare.market for this device (`screens::notify::live`).

use super::*;
use crate::screens::notify::alert::AlertSheet;
use crate::screens::notify::live::{self, LiveAlerts};

impl App {
    pub(super) fn open_price_alert(&mut self, coin_type: &str, symbol: &str, price: f64, now: f64) {
        if !(price > 0.0) {
            self.st.toast("Price alerts need a price for this coin.", Tone::Info, now);
            return;
        }
        let t = clock::secs();
        let root = self.scene.root;
        let before = self.scene.children(root);
        let addr = if self.fixture { None } else { crate::data::account::current().map(|c| c.0) };
        let screen: Box<dyn crate::screens::overlay::Screen> = {
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            Box::new(AlertSheet::build(&mut cx, symbol, coin_type, price, addr, Box::new(LiveAlerts), t))
        };
        let nodes: Vec<Id> = self.scene.children(root).into_iter().filter(|n| !before.contains(n)).collect();
        for n in &nodes {
            self.scene.n(*n).z = layer::SHEET + 5;
        }
        self.fx.fb(Feedback::Open);
        self.close_legacy();
        self.legacy = Some((screen, nodes));
    }

    /// Keeps the stream up for this device's alerts and reads the list once.
    pub(super) fn alerts_update(&mut self) {
        if !live::live() {
            return;
        }
        if !self.alerts_seen.0 {
            self.alerts_seen.0 = true;
            live::refresh_async();
        }
        let on = live::has_active();
        if on != self.alerts_seen.1 {
            self.alerts_seen.1 = on;
            if let Some(s) = &self.social {
                s.set_has_alerts(on);
            }
        }
    }
}
