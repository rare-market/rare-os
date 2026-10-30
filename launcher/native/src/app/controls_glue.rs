//! Settings > Controls and Sound, live in the app: UI sounds and rumble (rare-render's feedback
//! player, with the rumble strength), and LC and RC (`crate::side`). The values come from
//! launcher.json, read again whenever Settings saved.

use super::*;
use rare_render::feedback::{Feedback as Player, Kind};

fn kind_of(f: Feedback) -> Kind {
    match f {
        Feedback::Move => Kind::Move,
        Feedback::Select => Kind::Select,
        Feedback::Back => Kind::Back,
        Feedback::Tab => Kind::Tab,
        Feedback::Open => Kind::Open,
        Feedback::Close => Kind::Close,
        Feedback::Launch => Kind::Launch,
        Feedback::Toggle => Kind::Toggle,
        Feedback::Error => Kind::Error,
        Feedback::Boot => Kind::Boot,
    }
}

/// The player for this run: silent in fixtures, golden shots and on a development Mac.
pub(super) fn player(fixture: bool) -> Player {
    Player::new(rare_rendered_real() && !fixture)
}

fn rare_rendered_real() -> bool {
    rare_render::feedback::real_app()
}

impl App {
    /// Sounds on or off, rumble on or off and its strength, from the saved config.
    pub(super) fn apply_controls(&mut self) {
        let c = crate::screens::settings::config::load();
        self.player.set(Some(c.ui_sounds.unwrap_or(true)), Some(c.rumble.unwrap_or(true)));
        self.player.strength = (c.rumble_strength.unwrap_or(100.0) / 100.0).clamp(0.0, 1.0);
    }

    /// Plays what the handlers asked for since the last frame.
    pub(super) fn play_feedback(&mut self, now: f64) {
        for f in self.fx.feedback.drain(..) {
            log::debug!("feedback {f:?}");
            self.player.play(kind_of(f), now);
        }
    }

    /// LC and RC: what they asked of the app, and which games run for the capture's tag.
    pub(super) fn side_update(&mut self, now: f64) {
        let v = (self.st.ver.library, self.st.ver.running);
        if v != self.side_seen {
            self.side_seen = v;
            let running: Vec<(String, String)> = self.st.running.iter().map(|rid| (rid.clone(), self.st.run_name(rid))).collect();
            crate::side::set_running(running);
        }
        for r in crate::side::take() {
            match r {
                crate::side::Req::Toast(text, err) => self.st.toast(text, if err { Tone::Error } else { Tone::Info }, now),
                crate::side::Req::Keyboard => {
                    if self.st.osk.is_some() {
                        self.st.osk = None;
                    } else if self.st.overlay.is_none() {
                        self.ask_search();
                    }
                }
            }
        }
    }
}
