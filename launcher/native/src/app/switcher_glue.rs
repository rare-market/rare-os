//! The Switcher over the launcher (App.tsx guidehold / exithold, overlay/Overlay.tsx
//! LauncherSwitcher): Home, then what runs now, then recent games; A switches, X twice closes.
//! Also whether a Rare surface is up over a game, which InputPlumber's intercept mode follows.

use super::*;
use crate::screens::overlay::fixtures::Session;
use crate::screens::overlay::switcher::{RecentGame, RunItem, Switcher, SwitcherState};
use std::sync::atomic::{AtomicBool, Ordering};

static SURFACE: AtomicBool = AtomicBool::new(false);

/// A Rare surface (the Rare menu, the Switcher) is over a running game: InputPlumber sends the
/// whole pad to Rare as `ui_*` actions while it is (rare_input::ip).
pub fn surface_up() -> bool {
    SURFACE.load(Ordering::Relaxed) || crate::features::ingame::surface()
}

fn first_src(v: &[String]) -> Option<rare_render::Src> {
    v.iter().find_map(|s| crate::data::art::src_of(s))
}

impl App {
    pub(super) fn set_surface(&self) {
        SURFACE.store(self.legacy.is_some() && !self.st.running.is_empty(), Ordering::Relaxed);
    }

    /// The Rare button held, or View + Menu: the Switcher (nothing during setup).
    pub(super) fn open_switcher(&mut self) {
        if matches!(self.st.overlay, Some(Overlay::Other("firstrun" | "signin"))) {
            return;
        }
        let now = clock::secs();
        self.close_legacy();
        self.st.close();
        self.st.options = None;
        let running: Vec<RunItem> = self
            .st
            .running
            .iter()
            .map(|rid| {
                let app = crate::screens::apps::catalog::app_by_run_id(rid);
                let game = self.st.game(rid);
                RunItem {
                    rid: rid.clone(),
                    name: self.st.run_name(rid),
                    app: app.is_some(),
                    art: game.and_then(|g| first_src(&g.art.hero).or_else(|| first_src(&g.art.header)).or_else(|| first_src(&g.art.cover))),
                    icon: app.and_then(|a| crate::data::art::src_of(&a.icon)),
                }
            })
            .collect();
        let recent: Vec<RecentGame> = self
            .st
            .games()
            .iter()
            .filter(|g| g.source != "apps" && g.last_played.is_some())
            .map(|g| RecentGame { id: g.id.clone(), title: g.title.clone(), installed: g.installed, last_played: g.last_played.unwrap_or(0), cover: first_src(&g.art.cover).or_else(|| first_src(&g.art.header)) })
            .collect();
        let state = if crate::features::ingame::on() {
            // rare-overlay's order (most recently used), the still each game left, Paused.
            let s = rare_overlay::switcher::switcher_state();
            SwitcherState {
                cards: s.cards.into_iter().map(|c| crate::screens::overlay::switcher::model::CardState { rid: c.rid, paused: c.paused, frame: c.frame.map(|f| rare_render::Src::File(f.into())) }).collect(),
                from: s.from,
                max_paused: s.max_paused as u32,
            }
        } else {
            // Most recently used first: the newest start leads (the runner lists in start order).
            let order: Vec<String> = self.st.running.iter().rev().cloned().collect();
            SwitcherState { cards: order.iter().map(|rid| crate::screens::overlay::switcher::model::CardState { rid: rid.clone(), paused: false, frame: None }).collect(), from: None, max_paused: 0 }
        };
        // Over a running game: its still behind the cards.
        let session = match self.ingame_session() {
            Some((game_id, frame)) => Session { game_id, frame, guest: rare_overlay::guest() },
            None => Session { game_id: None, frame: None, guest: false },
        };
        let root = self.scene.root;
        let before = self.scene.children(root);
        let screen: Box<dyn crate::screens::overlay::Screen> = {
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            Box::new(Switcher::build(&mut cx, &session, &state, running, &recent, now))
        };
        let nodes: Vec<Id> = self.scene.children(root).into_iter().filter(|n| !before.contains(n)).collect();
        for n in &nodes {
            self.scene.n(*n).z = layer::SHEET + 5;
        }
        self.fx.fb(Feedback::Open);
        self.legacy = Some((screen, nodes));
        self.switcher_up = true;
    }

    /// What a Switcher card does. True when `a` was one of its acts.
    pub(super) fn switcher_act(&mut self, a: &str) -> bool {
        if a == "home" {
            self.st.close();
            self.st.set_tab(Tab::Home, false);
            self.fx.fb(Feedback::Back);
            return true;
        }
        if let Some(id) = a.strip_prefix("launch:") {
            self.launch(id);
            return true;
        }
        if let Some(rid) = a.strip_prefix("switch:") {
            // Back to a running game, app or stream: its window comes to the front.
            let rid = rid.to_string();
            self.fx.fb(Feedback::Launch);
            self.fx.spawn(move |b| {
                let r = b.launch(&rid);
                Box::new(move |st: &mut State, now| {
                    if let Err(e) = r {
                        log::warn!("switch: {e}");
                        st.toast("Couldn't switch to it. Try again.", Tone::Error, now);
                    }
                })
            });
            return true;
        }
        if let Some(rid) = a.strip_prefix("quit:") {
            // Close it, then show the Switcher again without it.
            let rid = rid.to_string();
            let name = self.st.run_name(&rid);
            self.fx.spawn(move |b| {
                let r = b.quit(&rid);
                let running = b.running();
                Box::new(move |st: &mut State, now| {
                    st.set_running(running);
                    match r {
                        Ok(()) => st.reopen_switcher = true,
                        Err(e) => {
                            log::warn!("quit {rid}: {e}");
                            st.toast(format!("Couldn't close {name}."), Tone::Error, now);
                        }
                    }
                })
            });
            self.open_switcher();
            return true;
        }
        false
    }
}
