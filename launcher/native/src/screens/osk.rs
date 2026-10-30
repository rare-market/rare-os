//! The on-screen keyboard (components/Osk.tsx), driven by the controller: the d-pad moves (rows wrap
//! sideways; the bottom row maps across), A types, X deletes, Y toggles shift, Menu finishes, B
//! cancels. A bottom sheet sliding up over a dimmed screen; the focused key white.

use crate::act::Act;
use crate::kit::{self, s, tok, GSize, Pb, Ty, Ui, T};
use crate::page::{hash, Acts, Btn, Feedback, Fx, Page};
use crate::state::{OskFor, OskReq, State};
use rare_render::focus::Focusable;
use rare_render::scene::Shadow;
use rare_render::view::{key, El, Motion};

const LOWER: [[&str; 10]; 4] = [
    ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"],
    ["q", "w", "e", "r", "t", "y", "u", "i", "o", "p"],
    ["a", "s", "d", "f", "g", "h", "j", "k", "l", "@"],
    ["z", "x", "c", "v", "b", "n", "m", ".", "-", "_"],
];
const UPPER: [[&str; 10]; 4] = [
    ["!", "?", "#", "$", "%", "&", "*", "(", ")", "/"],
    ["Q", "W", "E", "R", "T", "Y", "U", "I", "O", "P"],
    ["A", "S", "D", "F", "G", "H", "J", "K", "L", ":"],
    ["Z", "X", "C", "V", "B", "N", "M", ",", "'", "+"],
];
const SPECIAL: [&str; 5] = ["shift", "space", "paste", "del", "done"];

pub struct Osk {
    shift: bool,
    /// [row, col]; row 4 is the special row.
    pos: (usize, usize),
    /// The key to focus after the next build (a tag).
    want: Option<u32>,
}

impl Default for Osk {
    fn default() -> Self {
        Osk { shift: false, pos: (1, 0), want: Some(10) }
    }
}

fn tag_of(pos: (usize, usize)) -> u32 {
    (pos.0 * 10 + pos.1) as u32
}

/// What a finished keyboard hands back (the web's askText promise).
pub fn finish(st: &mut State, req: OskReq, value: Option<String>) {
    match (req.purpose, value) {
        (OskFor::Settings(k), v) => {
            st.settings_input = Some((k, v));
        }
        (OskFor::Search, Some(v)) => {
            st.search_before = None;
            if st.search != v {
                st.search = v;
                st.ver.search += 1;
            }
        }
        (OskFor::Search, None) => {
            if let Some(b) = st.search_before.take() {
                if st.search != b {
                    st.search = b;
                    st.ver.search += 1;
                }
            }
            // Cancelled with no search yet: the Search page goes too.
            if st.search.is_empty() {
                st.close();
            }
        }
        (OskFor::WebGameUrl, Some(v)) if !v.trim().is_empty() => match crate::features::webgame::normalise_url(&v) {
            Ok(url) => {
                let host = url.split("://").nth(1).and_then(|r| r.split(['/', '?', '#']).next()).unwrap_or("");
                let host = host.trim_start_matches("www.").to_string();
                st.osk = Some(OskReq::new("Name for this game", host, None, OskFor::WebGameTitle { url }));
            }
            Err(e) => st.toast(crate::screens::settings::live::failed("That isn't a web address. Check it and try again.", &e, None), crate::state::Tone::Error, rare_render::clock::secs()),
        },
        (OskFor::WebGameTitle { url }, Some(t)) if !t.trim().is_empty() => {
            st.acts.push(crate::act::Act::AddWebGame { id: None, title: t.trim().to_string(), url, cover: None, hero: None });
        }
        (OskFor::Screen(k), v) => st.osk_answer = Some((k, v)),
        (OskFor::SameGame(id), Some(q)) if !q.trim().is_empty() => st.acts.push(crate::act::Act::SameGameFind(id, q.trim().to_string())),
        (OskFor::Collection { rename, game }, Some(name)) => {
            let now = rare_render::clock::secs();
            let r = match &rename {
                Some(old) => st.user.rename(old, &name).map(|n| format!("Renamed to {n}")),
                None => st.user.create(&name, game.as_deref()).map(|n| if game.is_some() { format!("Added to {n}") } else { format!("{n} created") }),
            };
            match r {
                Ok(text) => {
                    st.user_changed();
                    st.toast(text, crate::state::Tone::Info, now);
                }
                Err(e) => st.toast(e, crate::state::Tone::Error, now),
            }
        }
        _ => {}
    }
}

impl Osk {
    fn press(&mut self, st: &mut State, k: &str) {
        let Some(req) = st.osk.as_mut() else { return };
        match k {
            "shift" => self.shift = !self.shift,
            "space" => req.value.push(' '),
            "del" => {
                req.value.pop();
            }
            "done" => {
                let req = st.osk.take().unwrap();
                let v = req.value.clone();
                finish(st, req, Some(v));
            }
            "paste" => match rare_system::ssh::clipboard_text().filter(|c| !c.trim().is_empty()) {
                Some(c) => req.value.push_str(c.trim()),
                // Say so, instead of a key that seems to do nothing.
                None => st.toast("Nothing to paste. Copy some text first.", crate::state::Tone::Info, rare_render::clock::secs()),
            },
            c => {
                req.value.push_str(c);
                if self.shift && c.chars().all(|c| c.is_ascii_uppercase()) {
                    self.shift = false;
                }
            }
        }
    }

    fn rows(&self) -> &'static [[&'static str; 10]; 4] {
        if self.shift {
            &UPPER
        } else {
            &LOWER
        }
    }

    fn key_at(&self, pos: (usize, usize)) -> &'static str {
        if pos.0 == 4 {
            SPECIAL[pos.1]
        } else {
            self.rows()[pos.0][pos.1]
        }
    }
}

impl Page for Osk {
    fn stamp(&self, st: &State) -> u64 {
        hash((st.osk.as_ref().map(|o| (&o.title, &o.value, &o.status)), self.shift))
    }

    fn view(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El {
        let Some(req) = &st.osk else { return ui.bx(s()) };
        let shown = if req.secret { "\u{2022}".repeat(req.value.chars().count()) } else { req.value.clone() };
        let mut field = ui.bx(s().row().items_center().h(3.25).px(1.0).mb(1.0)).bg(tok::BG).radius(ui.r(0.5)).border(1.0, tok::BORDER_LIGHT);
        field = if shown.is_empty() {
            field.child(ui.text(req.placeholder.clone().unwrap_or_default(), T::new(Ty::Row).c(tok::MUTED.alpha(0.7))))
        } else {
            field.child(ui.text(shown, T::new(Ty::Row).mono().trunc()))
        };
        let caret_name = ui.loop_name(&CARET, 2.0);
        field = field.child(ui.bx(s().w_px(2.0).h(1.5).ml(1.0 / ui.rem)).bg(tok::FG).name(caret_name));

        let mut keys = ui.bx(s().col().gap(0.45));
        for (ri, row) in self.rows().iter().enumerate() {
            let mut r = ui.bx(s().row().gap(0.45));
            for (ci, k) in row.iter().enumerate() {
                let tag = acts.tag(Act::Local((ri * 10 + ci) as u32));
                let _ = tag;
                let b = ui.bx(s().row().items_center().justify_center().h(2.9).grow()).bg(tok::L3).radius(ui.r(0.5)).child(ui.text(*k, T::new(Ty::Row).w(500)).focus_color(tok::BG));
                let fl = b.look.clone().bg(tok::FG).scale(1.06);
                r = r.child(kit::nav(b.focus_look(fl).transition(rare_render::view::Transition::pro(0.1)), (ri * 10 + ci) as u32, Some("Type"), None, false).key(key(&(ri, ci))));
            }
            keys = keys.child(r);
        }
        let mut sp = ui.bx(s().row().gap(0.45));
        for (ci, k) in SPECIAL.iter().enumerate() {
            let tag = acts.tag(Act::Local((40 + ci) as u32));
            let _ = tag;
            let glyph = match *k {
                "shift" => Some(Pb::Y),
                "del" => Some(Pb::X),
                "done" => Some(Pb::Start),
                _ => None,
            };
            let label = match *k {
                "shift" => if self.shift { "abc" } else { "Shift" },
                "del" => "Delete",
                "done" => "Done",
                "paste" => "Paste",
                _ => "Space",
            };
            let fill = if *k == "done" { tok::BRAND } else { tok::L4 };
            let mut b = ui.bx(s().row().items_center().justify_center().gap(0.5).h(2.9).flex(if *k == "space" { 4.0 } else { 1.4 })).bg(fill).radius(ui.r(0.5));
            if let Some(g) = glyph {
                b = b.child(kit::glyph(ui, g, GSize::Sm));
            }
            b = b.child(ui.text(label, T::new(Ty::Body).w(500).c(if *k == "done" { tok::WHITE } else { tok::FG })).focus_color(tok::BG));
            let fl = b.look.clone().bg(tok::FG).scale(1.04);
            sp = sp.child(kit::nav(b.focus_look(fl).transition(rare_render::view::Transition::pro(0.1)), (40 + ci) as u32, Some("Type"), None, false).key(key(k)));
        }
        keys = keys.child(sp);
        let small = T::new(Ty::Small).c(tok::fg(0.75));
        let hints = ui.bx(s().row().justify_end().gap(1.3).mt(0.9)).child(kit::hint(ui, Pb::A, "Type", small)).child(kit::hint(ui, Pb::B, "Cancel", small));
        let mut inner = ui.bx(s().col().wp(1.0).max_w(64.0).mx_auto()).child(ui.text_s(s().mb(0.5), req.title.clone(), T::new(Ty::Small).c(tok::MUTED))).child(field);
        if let Some(line) = &req.status {
            // The asking screen's live line (askText's status), between the field and the keys.
            inner = inner.child(ui.text_s(s().mt(-0.5).mb(0.75), line.clone(), T::new(Ty::Small).c(tok::MUTED).trunc()));
        }
        let inner = inner.child(keys).child(hints);
        // rounded-t-sheet: the bottom corners sit below the screen edge.
        let sheet = ui
            .bx(s().col().px(2.5).pb(1.1 + 1.25).pt(1.2).mb(-1.25))
            .bg(tok::L1)
            .radius(ui.r(1.25))
            .border(1.0, tok::BORDER_LIGHT)
            .shadow(&[Shadow::new(0.0, -ui.r(1.0), ui.r(3.0), 0.0, tok::BLACK.alpha(0.5))])
            .enter(Motion::SLIDE_UP)
            .child(inner);
        ui.bx(s().fill().col().justify_end()).bg(tok::BLACK.alpha(0.55)).enter(Motion::FADE).child(sheet)
    }

    fn pad(&mut self, st: &mut State, fx: &mut Fx, b: Btn, cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        // The focused key is where the keyboard is (a touch puts focus on a key without moving here).
        if let Some((f, _)) = cur.filter(|(f, _)| f.scope == crate::app::SCOPE_OSK) {
            let p = if f.tag >= 40 { (4, (f.tag - 40) as usize) } else { ((f.tag / 10) as usize, (f.tag % 10) as usize) };
            if p.0 <= 4 && p.1 < if p.0 == 4 { SPECIAL.len() } else { 10 } {
                self.pos = p;
            }
        }
        let (r, c) = self.pos;
        let last = 4;
        let cols = |row: usize| if row == last { SPECIAL.len() } else { 10 };
        let next = match b {
            Btn::Up => Some((r.saturating_sub(1), if r == last { ((c as f32 / (SPECIAL.len() - 1) as f32) * 9.0).round() as usize } else { c })),
            Btn::Down => Some(if r + 1 == last { (last, ((c as f32 / 9.0) * (SPECIAL.len() - 1) as f32).round() as usize) } else { ((r + 1).min(last), c) }),
            Btn::Left => Some((r, (c + cols(r) - 1) % cols(r))),
            Btn::Right => Some((r, (c + 1) % cols(r))),
            _ => None,
        };
        if let Some(p) = next {
            if p != self.pos {
                fx.fb(Feedback::Move);
            }
            self.pos = p;
            self.want = Some(tag_of(p));
            return true;
        }
        match b {
            Btn::A => {
                let k = self.key_at(self.pos);
                self.press(st, k);
            }
            Btn::X => self.press(st, "del"),
            Btn::Y => self.press(st, "shift"),
            Btn::Start => self.press(st, "done"),
            Btn::B => {
                if let Some(req) = st.osk.take() {
                    finish(st, req, None);
                }
            }
            _ => {}
        }
        true
    }

    fn take_focus(&mut self, _cur: Option<&Focusable>) -> Option<u32> {
        self.want.take()
    }
}

/// animate-pulse: opacity 1 → 0.5 → 1 over 2 s.
pub static CARET: rare_render::anim::Keys = rare_render::anim::Keys { frames: &[(0.0, 1.0), (0.5, 0.5), (1.0, 1.0)], ease: rare_render::Ease(0.4, 0.0, 0.6, 1.0) };
