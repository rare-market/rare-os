//! Ask Rare's requests of the shell (features/agent): the Rare menu steps aside for a sheet the
//! agent opened, the game is left for a page or another game, RC held opens the menu on Ask.

use super::App;
use crate::features::agent::{self, UiReq};

impl App {
    /// Rare Wallet's review sheet draws in the sheet layer, under the Rare menu (and the Switcher):
    /// while a review is up, those step under the sheets so the review is on top and takes the
    /// presses (a purchase from the menu's Market tab, anything Ask Rare prepared).
    fn review_over_menu(&mut self) {
        use rare_render::layer;
        let want = if crate::screens::wallet::sign::host::active() { layer::SHEET - 1 } else { layer::SHEET + 5 };
        if let Some((_, nodes)) = self.legacy.as_ref() {
            let nodes = nodes.clone();
            for n in nodes {
                if self.scene.n(n).z != want {
                    self.scene.n(n).z = want;
                    self.scene.mark_dirty();
                }
            }
        }
    }

    pub(super) fn agent_update(&mut self, _now: f64) {
        self.review_over_menu();
        // Shots: questions asked at start, one after the other (RR_ASK="a|b" with RR_ASK_MOCK).
        static ASKED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        if let Ok(qs) = std::env::var("RR_ASK") {
            let n = ASKED.load(std::sync::atomic::Ordering::SeqCst);
            if let Some(q) = qs.split('|').nth(n) {
                if !agent::view().busy {
                    ASKED.store(n + 1, std::sync::atomic::Ordering::SeqCst);
                    agent::ask(q, std::env::var("RR_ASK_SHOT").is_ok());
                }
            }
        }
        for r in agent::take_ui() {
            match r {
                UiReq::CloseMenu => {
                    if self.legacy.is_some() && !self.switcher_up {
                        self.close_legacy();
                        // Over a game the menu comes back when the sheet closes: on Ask.
                        agent::open_on_ask(self.ingame.is_some());
                    }
                }
                UiReq::Leave => {
                    if self.ingame.is_some() {
                        self.ingame_act("home");
                    } else {
                        self.close_legacy();
                    }
                }
                UiReq::Launch(id) => {
                    if self.ingame.is_some() {
                        self.ingame_act(&format!("launch:{id}"));
                    } else {
                        self.close_legacy();
                        self.run(crate::act::Act::Launch(id));
                    }
                }
                UiReq::OpenAsk => {
                    if self.ingame.is_some() || self.st.running.is_empty() || !crate::features::ingame::on() {
                        // The launcher (or the session over the game) is in front: the menu, on Ask.
                        self.close_legacy();
                        agent::open_on_ask(true);
                        self.open_rare_menu();
                    } else {
                        // A game has the screen: the overlay brings the menu up over it, on Ask.
                        agent::open_on_ask(true);
                        std::thread::spawn(|| {
                            if let Err(e) = rare_overlay::open(None) {
                                log::warn!("ask over the game: {e}");
                            }
                        });
                    }
                }
            }
        }
    }
}
