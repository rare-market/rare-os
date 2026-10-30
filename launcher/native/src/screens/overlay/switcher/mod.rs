//! The Switcher and "Close a paused game?" (overlay/Switcher.tsx). Hold the Rare button for a
//! full-screen row of cards: Home first, then everything running, most recently used first, each
//! the still frame gamescope took when you left it; then up to four recent games, smaller.
//! A switches, X closes (X again confirms), B goes back.

use crate::screens::overlay::common::*;
use crate::screens::overlay::fixtures::{self, Session};
use crate::screens::overlay::lay::s;
use rare_render::input::Pad;
use rare_render::scene::{GradDir, Gradient};
use rare_render::theme::tok;
use rare_render::{Fit, Id, ImgReq, Node, Src};

pub mod model;
mod paused;

pub use model::*;
pub use paused::PausedFull;

pub const ARM_SECS: f64 = 4.0;

struct Card {
    /// None is Home.
    rid: Option<String>,
    id: Id,
    status: Option<Id>,
    base_status: String,
}

pub struct Switcher {
    nav: Nav,
    cards: Vec<Card>,
    armed: Option<(String, f64)>,
    from: Option<String>,
    recent: Vec<RecentGame>,
}

fn art(cx: &mut Cx, parent: Id, src: Src, w: f32, h: f32, radius: f32) -> Id {
    let k = cx.img.request(ImgReq { src, w: w.round() as u32, h: h.round() as u32, fit: Fit::Cover { fy: 0.5 } });
    cx.scene.add(parent, s().fill().done(), Node::default().image(k).radius(radius))
}

impl Switcher {
    /// `from` is the run id of the game the menu was opened over (None over the launcher).
    pub fn build(cx: &mut Cx, session: &Session, st: &SwitcherState, running: Vec<RunItem>, library: &[RecentGame], now: f64) -> Switcher {
        let r = cx.r();
        let (sw, sh) = (cx.scene.w, cx.scene.h);
        let root = cx.scene.root;
        let page = cx.scene.add(root, s().fill().col().done(), Node::default());
        // The dimmed frame.
        let back = cx.scene.add(page, s().fill().done(), Node::bg(tok::BG).clip());
        if let Some(src) = session.frame.clone() {
            let k = cx.img.request(ImgReq { src, w: sw as u32, h: sh as u32, fit: Fit::Cover { fy: 0.5 } });
            let img = cx.scene.add(back, s().fill().done(), Node::default().image(k));
            fade_in(cx, img, now);
        }
        cx.scene.add(back, s().fill().done(), Node::bg(rare_render::Color::BLACK.alpha(0.8)));
        let _ = (GradDir::ToBottom, Gradient { dir: GradDir::ToBottom, stops: [tok::BG; 3], mid: 0.5 });

        let items = in_order(running, &st.order());
        let recent = recent_not_running(library, &items.iter().map(|i| i.rid.clone()).collect::<Vec<_>>(), 4);

        let mid = cx.scene.add(page, s().col().grow().justify_center().gap(2.25 * r).px(2.5 * r).done(), Node::default());
        fade_in(cx, mid, now);
        let mut nav = Nav::default();
        let mut cards: Vec<Card> = Vec::new();

        let row_ = cx.scene.add(mid, s().row().items_start().gap(1.25 * r).py(0.75 * r).done(), Node::default());
        // Home.
        {
            let c = card_shell(cx, row_, "Home", "", &mut nav, 0, Some(("Home", None)), true);
            let face = cx.scene.add(c.1, s().fill().items_center().justify_center().done(), Node::bg(tok::L2));
            cx.scene.add_text(cx.t, face, s().done(), "Home", ts(r, 1.375, 1.75, 600), tok::FG.alpha(0.9));
            cards.push(Card { rid: None, id: c.0, status: c.2, base_status: String::new() });
        }
        for (i, it) in items.iter().enumerate() {
            let c = st.card(&it.rid);
            let status = if c.map(|c| c.paused).unwrap_or(false) {
                "Paused"
            } else if it.app {
                "Open"
            } else {
                "Running"
            };
            let (id, face_parent, status_id) = card_shell(cx, row_, &it.name, status, &mut nav, (i + 1) as u32, Some(("Switch", Some("Close"))), false);
            card_art(cx, face_parent, it, c.and_then(|c| c.frame.clone()), 22.0 * r, 13.75 * r);
            cards.push(Card { rid: Some(it.rid.clone()), id, status: status_id, base_status: status.into() });
        }

        if !recent.is_empty() {
            let sec = cx.scene.add(mid, s().col().gap(0.6 * r).done(), Node::default());
            text(cx, sec, ty::LABEL, 500, tok::MUTED, "Recent");
            let rr = cx.scene.add(sec, s().row().gap(1.0 * r).done(), Node::default());
            for (k, g) in recent.iter().enumerate() {
                let tile = cx.scene.add(rr, s().wh(7.5 * r, 11.25 * r).done(), Node::bg(tok::L3).radius(0.5 * r).clip());
                if let Some(src) = g.cover.clone() {
                    art(cx, tile, src, 7.5 * r, 11.25 * r, 0.5 * r);
                }
                nav.add(Focusable { id: tile, kind: Kind::Tile { scale: 1.06 }, group: 2, hint: "Play".into(), tag: 1000 + k as u32, sid: g.id.clone() });
            }
        }

        // Footer.
        let foot = cx.scene.add(page, s().row().items_center().gap(1.25 * r).h(3.0 * r).px(2.5 * r).done(), Node::bg(tok::BG.alpha(0.6)));
        edge(cx, foot, 0, tok::HAIR);
        for (g2, label) in [(G::Y, "Screenshot"), (G::Select, "Save clip")] {
            let c = cx.scene.add(foot, s().row().items_center().gap(0.35 * r).done(), Node::default());
            glyph(cx, c, G::Rare, true);
            text(cx, c, ty::SMALL, 400, tok::FG.alpha(0.75), "+");
            glyph(cx, c, g2, true);
            text(cx, c, ty::SMALL, 400, tok::FG.alpha(0.75), label);
        }
        let right = cx.scene.add(foot, s().row().items_center().gap(1.25 * r).ml_auto().done(), Node::default());
        hint(cx, right, G::A, "Switch");
        hint(cx, right, G::X, "Close");
        hint(cx, right, G::B, "Back");

        // Focus lands on the game you were in (one A is "back"), else on Home.
        let start = session.game_id.as_ref().and_then(|g| cards.iter().find(|c| c.rid.as_deref() == Some(g)).map(|c| c.id)).unwrap_or(cards[0].id);
        nav.focus(cx.scene, start, now, false);
        Switcher { nav, cards, armed: None, from: session.game_id.clone(), recent }
    }

    fn cur_rid(&self) -> Option<Option<String>> {
        let id = self.nav.cur?;
        self.cards.iter().find(|c| c.id == id).map(|c| c.rid.clone())
    }

    fn disarm(&mut self, cx: &mut Cx) {
        if let Some((rid, _)) = self.armed.take() {
            if let Some(c) = self.cards.iter().find(|c| c.rid.as_deref() == Some(rid.as_str())) {
                if let Some(st) = c.status {
                    cx.scene.set_text(cx.t, st, &c.base_status);
                }
            }
        }
    }
}

/// A card: the 16:10 button (tile, scale 1.03) and the title and status under it.
fn card_shell(cx: &mut Cx, parent: Id, title: &str, status: &str, nav: &mut Nav, tag: u32, hints: Option<(&str, Option<&str>)>, home: bool) -> (Id, Id, Option<Id>) {
    let r = cx.r();
    let wrap = cx.scene.add(parent, s().col().w(22.0 * r).done(), Node::default());
    let btn = cx.scene.add(wrap, s().wh(22.0 * r, 13.75 * r).done(), Node::bg(tok::L3).radius(0.5 * r).clip());
    let hint_a = hints.map(|h| h.0).unwrap_or("");
    nav.add(Focusable { id: btn, kind: Kind::Tile { scale: 1.03 }, group: 1, hint: hint_a.into(), tag, sid: if home { "home".into() } else { String::new() } });
    let label = cx.scene.add(wrap, s().col().mt(0.7 * r).px(0.1 * r).done(), Node::default());
    cx.scene.add_text(cx.t, label, s().done(), title, ts(r, ty::BODY.0, ty::BODY.1, 500).truncate(), tok::FG);
    let st = if status.is_empty() && !home { None } else { Some(cx.scene.add_text(cx.t, label, s().done(), if status.is_empty() { " " } else { status }, ts(r, ty::SMALL.0, ty::SMALL.1, 400).truncate(), tok::MUTED)) };
    (btn, btn, st)
}

/// The still frame from when you left it; else the game's art, or the app's icon.
fn card_art(cx: &mut Cx, face: Id, it: &RunItem, frame: Option<Src>, w: f32, h: f32) {
    let r = cx.r();
    if let Some(f) = frame {
        art(cx, face, f, w, h, 0.5 * r);
    } else if it.app {
        let c = cx.scene.add(face, s().fill().items_center().justify_center().done(), Node::bg(tok::L2));
        if let Some(src) = it.icon.clone() {
            let k = cx.img.request(ImgReq { src, w: (4.5 * r) as u32, h: (4.5 * r) as u32, fit: Fit::Contain });
            cx.scene.add(c, s().wh(4.5 * r, 4.5 * r).done(), Node::default().image(k).contain([0.5, 0.5]));
        }
    } else if let Some(src) = it.art.clone() {
        art(cx, face, src, w, h, 0.5 * r);
    }
}

impl Screen for Switcher {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.nav.focus_at(cx.scene, x, y, now)
    }
    fn focus_id(&self) -> Option<Id> {
        self.nav.current().map(|f| f.id)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        match pad {
            Pad::B => Out::Close,
            Pad::A => {
                let Some(f) = self.nav.current().cloned() else { return Out::None };
                if f.tag >= 1000 {
                    let g = &self.recent[(f.tag - 1000) as usize];
                    return Out::Act(format!("launch:{}", g.id));
                }
                match self.cur_rid() {
                    Some(None) => Out::Act("home".into()),
                    Some(Some(rid)) => {
                        // The game you were in: one press of A is "back".
                        if self.from.as_deref() == Some(rid.as_str()) {
                            Out::Close
                        } else {
                            Out::Act(format!("switch:{rid}"))
                        }
                    }
                    None => Out::None,
                }
            }
            Pad::X => {
                let Some(Some(rid)) = self.cur_rid() else { return Out::None };
                match &self.armed {
                    Some((a, _)) if *a == rid => {
                        self.armed = None;
                        Out::Act(format!("quit:{rid}"))
                    }
                    _ => {
                        self.disarm(cx);
                        if let Some(c) = self.cards.iter().find(|c| c.rid.as_deref() == Some(rid.as_str())) {
                            if let Some(st) = c.status {
                                cx.scene.set_text(cx.t, st, "Press X again to close");
                            }
                        }
                        self.armed = Some((rid, now + ARM_SECS));
                        Out::None
                    }
                }
            }
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

    fn tick(&mut self, cx: &mut Cx, now: f64) {
        if self.armed.as_ref().is_some_and(|(_, at)| now >= *at) {
            self.disarm(cx);
        }
    }

    fn next_deadline(&self) -> Option<f64> {
        self.armed.as_ref().map(|(_, at)| *at)
    }
}

pub fn fixture(name: &str, cx: &mut Cx, now: f64) -> Option<Box<dyn Screen>> {
    let mut sess = fixtures::session();
    let st = model::fixture_state();
    Some(match name {
        "switcher" => Box::new(Switcher::build(cx, &sess, &st, model::fixture_running(), &[], now)),
        "switcher-home" => {
            sess.game_id = None;
            sess.frame = None;
            Box::new(Switcher::build(cx, &sess, &SwitcherState::default(), vec![], &[], now))
        }
        "switcher-armed" => {
            let mut sw = Switcher::build(cx, &sess, &st, model::fixture_running(), &[], now);
            sw.nav.step(cx.scene, Pad::Right, now);
            sw.pad(cx, Pad::X, now);
            Box::new(sw)
        }
        "switcher-recent" => Box::new(Switcher::build(cx, &sess, &st, model::fixture_running(), &model::fixture_library(), now)),
        "paused-full" => Box::new(PausedFull::build(cx, &model::fixture_freeze_full(), &model::fixture_running(), now)),
        _ => return None,
    })
}
