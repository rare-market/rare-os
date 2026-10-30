//! "Keep running in the background" on a game's page (overlay/BackgroundSwitch.tsx, background.ts
//! text): quick resume pauses a game you switch away from; this keeps one running instead. On by
//! default for online games and apps; not offered where Rare never pauses.

use super::common::*;
use super::lay::s;
use rare_render::theme::tok;
use rare_render::{Id, Node};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BackgroundWhy {
    Online,
    App,
    WebGame,
    None,
}

#[derive(Clone, Copy, Debug)]
pub struct Background {
    /// Pauses when you switch away.
    pub pause: bool,
    pub why: BackgroundWhy,
    pub can_change: bool,
}

/// The line under the switch: what happens, and why when it is the default.
pub fn background_line(b: &Background) -> &'static str {
    if b.pause {
        return "Pauses when you switch away, and picks up where you left it.";
    }
    match b.why {
        BackgroundWhy::Online => "On for online games, so you stay connected.",
        BackgroundWhy::App => "On for apps, so calls, messages and music carry on.",
        BackgroundWhy::WebGame => "On for web games, so you stay connected.",
        BackgroundWhy::None => "Keeps running when you switch away.",
    }
}

pub struct BackgroundSwitch {
    pub focusable: Option<Id>,
    track: Option<(Id, Id)>,
    pub on: bool,
}

impl BackgroundSwitch {
    /// Adds the switch to `parent` and registers it; nothing when the game can't change it.
    pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, b: Option<Background>) -> BackgroundSwitch {
        let Some(b) = b.filter(|b| b.can_change) else { return BackgroundSwitch { focusable: None, track: None, on: false } };
        let r = cx.r();
        let on = !b.pause;
        let btn = cx.scene.add(parent, s().row().items_center().gap(0.9 * r).px(0.75 * r).py(0.5 * r).done(), Node::default().radius(0.5 * r));
        let tr = toggle(cx, btn, on);
        let col = cx.scene.add(btn, s().col().done(), Node::default());
        text(cx, col, ty::SMALL, 500, tok::FG, "Keep running in the background");
        text(cx, col, ty::LABEL, 400, tok::MUTED, background_line(&b));
        nav.add(Focusable { id: btn, kind: Kind::Row, group: 30, hint: if on { "Turn off".into() } else { "Turn on".into() }, tag: 0, sid: "background".into() });
        BackgroundSwitch { focusable: Some(btn), track: Some(tr), on }
    }

    /// A pressed on it: flips and reports the new `on` (the host calls setBackground).
    pub fn flip(&mut self, cx: &mut Cx, nav: &mut Nav, now: f64) -> Option<bool> {
        let (t, k) = self.track?;
        self.on = !self.on;
        set_toggle(cx, t, k, self.on, now);
        if let Some(f) = nav.items.iter_mut().find(|f| Some(f.id) == self.focusable) {
            f.hint = if self.on { "Turn off".into() } else { "Turn on".into() };
        }
        Some(self.on)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_says_what_happens_and_why() {
        let b = |pause, why| Background { pause, why, can_change: true };
        assert_eq!(background_line(&b(true, BackgroundWhy::Online)), "Pauses when you switch away, and picks up where you left it.");
        assert_eq!(background_line(&b(false, BackgroundWhy::Online)), "On for online games, so you stay connected.");
        assert_eq!(background_line(&b(false, BackgroundWhy::App)), "On for apps, so calls, messages and music carry on.");
        assert_eq!(background_line(&b(false, BackgroundWhy::WebGame)), "On for web games, so you stay connected.");
        assert_eq!(background_line(&b(false, BackgroundWhy::None)), "Keeps running when you switch away.");
    }
}
