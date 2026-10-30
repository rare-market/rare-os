//! The power menu (overlay/PowerMenu.tsx): Sleep, Restart, Turn off over the frozen frame; B backs out.

use super::common::*;
use super::fixtures::Session;
use super::lay::s;
use rare_render::input::Pad;
use rare_render::theme::tok;
use rare_render::scene::Shadow;
use rare_render::Node;

pub struct PowerMenu {
    nav: Nav,
}

impl PowerMenu {
    pub fn build(cx: &mut Cx, session: &Session, now: f64) -> PowerMenu {
        let r = cx.r();
        let root = cx.scene.root;
        let page = cx.scene.add(root, s().fill().items_center().justify_center().done(), Node::default());
        if session.frame.is_none() && session.game_id.is_none() {
            // Over the launcher: the page stays, dimmed (the Rare menu's look), not a black screen.
            cx.scene.add(page, s().fill().done(), Node::bg(rare_render::Color::BLACK.alpha(0.55)));
        } else {
            frozen(cx, page, session.frame.clone());
        }
        let card = cx.scene.add(
            page,
            s().col().w(22.0 * r).gap(0.5 * r).p(1.0 * r).done(),
            Node::bg(tok::L1).radius(0.5 * r).border(1.0, tok::HAIR).shadow(&[Shadow::rem(0.0, 2.0, 4.0, 0.45, r)]),
        );
        fade_in(cx, card, now);
        let head = cx.scene.add(card, s().pad(0.4 * r, 0.0, 0.4 * r, 0.3 * r).done(), Node::default());
        text(cx, head, ty::ROW, 600, tok::FG, "Power");
        let mut nav = Nav::default();
        // Over the launcher the sheet also leaves for Desktop mode (last, as in Settings > System).
        let labels: &[&str] = if session.game_id.is_some() { &["Sleep", "Restart", "Turn off"] } else { &["Sleep", "Restart", "Turn off", "Desktop mode"] };
        for (i, label) in labels.iter().enumerate() {
            let b = button(cx, card, label, Variant::Secondary, Sz::Md, true).tag(i as u32);
            nav.add(b);
        }
        let foot = cx.scene.add(card, s().row().justify_end().gap(1.1 * r).pad(0.4 * r, 0.3 * r, 0.4 * r, 0.0).done(), Node::default());
        hint(cx, foot, G::A, "Select");
        hint(cx, foot, G::B, "Back");
        let first = nav.items[0].id;
        nav.focus(cx.scene, first, now, false);
        PowerMenu { nav }
    }
}

impl Screen for PowerMenu {
    fn focus_id(&self) -> Option<rare_render::Id> {
        self.nav.cur
    }

    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.nav.focus_at(cx.scene, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        match pad {
            Pad::B => Out::Close,
            Pad::A => match self.nav.current().map(|f| f.tag) {
                Some(0) => Out::Act("suspend".into()),
                Some(1) => Out::Act("reboot".into()),
                Some(2) => Out::Act("poweroff".into()),
                Some(3) => Out::Act("desktop".into()),
                _ => Out::None,
            },
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                if self.nav.step(cx.scene, pad, now) {
                    Out::Moved
                } else {
                    Out::None
                }
            }
            _ => Out::None,
        }
    }
}
