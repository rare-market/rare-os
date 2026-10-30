//! Settings > Controls > Button layout: which pad button does what, for every game and the
//! launcher (rare_input::remap, an InputPlumber device profile). A sheet over Settings: the
//! buttons in two columns with what each does now; A on one shows the choices, A on a choice sets
//! it. Swap A and B, Swap X and Y and Reset to default sit in the footer. Rare and Quick are never
//! listed: they always stay Rare's.

use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_input::remap::{self, Button, Layout, Status, Target, BUTTONS};
use rare_render::input::Pad;
use rare_render::theme::tok;
use rare_render::{Id, Node};
use std::sync::{Arc, Mutex};

const SUB: &str = "Applies to all games and the launcher. Rare and Quick always stay as they are.";
const HINT_LIST: &str = "Pick a button to change what it does.";

const T_SWAP_AB: u32 = 100;
const T_SWAP_XY: u32 = 101;
const T_RESET: u32 = 102;
const T_DONE: u32 = 103;
const T_BACK: u32 = 104;
/// A choice in pick mode: 200 + the index in BUTTONS, NOTHING for Nothing.
const T_PICK: u32 = 200;
const T_NOTHING: u32 = 299;

pub struct RemapSheet {
    nav: Nav,
    body: Id,
    footer: Id,
    note: Id,
    layout: Layout,
    /// Choosing what this button does.
    picking: Option<Button>,
    /// Device run: save and load into InputPlumber. Fixtures only draw.
    live: bool,
    pending: Option<Arc<Mutex<Option<Status>>>>,
}

impl RemapSheet {
    pub fn open(cx: &mut Cx, layout: Layout, live: bool, now: f64) -> RemapSheet {
        let r = cx.r();
        let root = cx.scene.root;
        let sh = sheet(cx, root, "Button layout", Some(SUB), true, true, now);
        // The status line keeps two lines of room, so the list never moves when it changes.
        let note_box = cx.scene.add(sh.body, s().col().min_h(2.0 * ty::SMALL.1 * r).mb(0.5 * r).done(), Node::default());
        let first = if live { remap::status().line() } else { None };
        let note = cx.scene.add_text(cx.t, note_box, s().done(), first.as_deref().unwrap_or(HINT_LIST), ts(r, ty::SMALL.0, ty::SMALL.1, 400).wrap(), tok::MUTED);
        let body = cx.scene.add(sh.body, s().col().wp(1.0).done(), Node::default());
        let mut me = RemapSheet { nav: Nav::default(), body, footer: sh.footer.unwrap(), note, layout, picking: None, live, pending: None };
        me.draw(cx, now, Some(0));
        me
    }

    fn set_note(&mut self, cx: &mut Cx, t: &str) {
        cx.scene.set_text(cx.t, self.note, t);
    }

    /// Builds the body and footer for the current mode and focuses the item tagged `focus`.
    fn draw(&mut self, cx: &mut Cx, now: f64, focus: Option<u32>) {
        let r = cx.r();
        self.nav.clear();
        cx.scene.clear_children(self.body);
        cx.scene.clear_children(self.footer);
        match self.picking {
            None => {
                let cols = cx.scene.add(self.body, s().row().wp(1.0).gap(1.0 * r).done(), Node::default());
                for half in BUTTONS.chunks(8) {
                    let p = cx.scene.add(cols, s().col().grow().done(), Node::bg(tok::L1).radius(1.0 * r).clip());
                    for (j, b) in half.iter().enumerate() {
                        let i = BUTTONS.iter().position(|x| x == b).unwrap();
                        let t = self.layout.target(*b);
                        let row = cx.scene.add(p, s().row().items_center().gap(1.0 * r).min_h(3.25 * r).wp(1.0).px(1.4 * r).py(0.6 * r).done(), Node::default());
                        if j > 0 {
                            edge(cx, row, 0, tok::HAIR);
                        }
                        cx.scene.add_text(cx.t, row, s().grow().done(), b.label(), ts(r, ty::BODY.0, ty::BODY.1, 600), tok::FG);
                        let changed = t != Target::Button(*b);
                        cx.scene.add_text(cx.t, row, s().done(), t.label(), ts(r, ty::BODY.0, ty::BODY.1, if changed { 600 } else { 400 }), if changed { tok::FG } else { tok::MUTED });
                        self.nav.add(Focusable { id: row, kind: Kind::Row, group: 1, hint: "Change".into(), tag: i as u32, sid: format!("remap-{}", b.key()) });
                    }
                }
                let ab = if self.layout.swapped(Button::A, Button::B) { "Swap A and B back" } else { "Swap A and B" };
                let xy = if self.layout.swapped(Button::X, Button::Y) { "Swap X and Y back" } else { "Swap X and Y" };
                for (label, tag, v) in [(ab, T_SWAP_AB, Variant::Secondary), (xy, T_SWAP_XY, Variant::Secondary), ("Reset to default", T_RESET, if self.layout.is_default() { Variant::Dimmed } else { Variant::Secondary }), ("Done", T_DONE, Variant::Primary)] {
                    self.nav.add(button(cx, self.footer, label, v, Sz::Md, false).hint(label).tag(tag).group(2));
                }
            }
            Some(b) => {
                let head = cx.scene.add(self.body, s().mb(0.75 * r).done(), Node::default());
                text(cx, head, ty::BODY, 600, tok::FG, &format!("What {} does", b.label()));
                let grid = cx.scene.add(self.body, s().row().wrap().gap(0.5 * r).wp(1.0).done(), Node::default());
                let cur = self.layout.target(b);
                let mut choices: Vec<(Target, u32)> = BUTTONS.iter().enumerate().map(|(i, x)| (Target::Button(*x), T_PICK + i as u32)).collect();
                choices.push((Target::Nothing, T_NOTHING));
                for (t, tag) in choices {
                    let on = t == cur;
                    let label = if t == Target::Button(b) { format!("{} (its own)", t.label()) } else { t.label().to_string() };
                    let (base, fg) = if on { (tok::FG, tok::BG) } else { (tok::L2, tok::FG) };
                    let chip = cx.scene.add(grid, s().row().items_center().justify_center().h(2.6 * r).min_w(5.5 * r).px(1.0 * r).done(), Node::bg(base).radius(0.25 * r));
                    let tid = cx.scene.add_text(cx.t, chip, s().done(), &label, ts(r, ty::SMALL.0, ty::SMALL.1, 500), fg);
                    self.nav.add(Focusable { id: chip, kind: Kind::Btn { base, fg, text: tid }, group: 1, hint: "Pick".into(), tag, sid: String::new() });
                }
                self.nav.add(button(cx, self.footer, "Back", Variant::Secondary, Sz::Md, false).hint("Back").tag(T_BACK).group(2));
            }
        }
        let target = focus.and_then(|t| self.nav.items.iter().find(|f| f.tag == t)).or_else(|| self.nav.items.first()).map(|f| f.id);
        if let Some(id) = target {
            self.nav.focus(cx.scene, id, now, false);
            self.nav.reveal(cx.scene, id, now);
        }
    }

    /// A new layout: refused when it would lock the person out, else saved and loaded.
    fn change(&mut self, cx: &mut Cx, new: Layout) -> bool {
        if new == self.layout {
            return false;
        }
        if let Err(m) = new.check() {
            self.set_note(cx, m);
            return false;
        }
        self.layout = new;
        if self.live {
            let slot = Arc::new(Mutex::new(None));
            let s2 = slot.clone();
            let l = self.layout.clone();
            std::thread::spawn(move || {
                let st = remap::apply(&l);
                if let Ok(mut g) = s2.lock() {
                    *g = Some(st);
                }
            });
            self.pending = Some(slot);
        } else {
            self.set_note(cx, HINT_LIST);
        }
        true
    }

    fn back_to_list(&mut self, cx: &mut Cx, b: Button, now: f64) {
        self.picking = None;
        let i = BUTTONS.iter().position(|x| *x == b).unwrap_or(0) as u32;
        self.draw(cx, now, Some(i));
    }
}

impl Screen for RemapSheet {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.nav.focus_at(cx.scene, x, y, now)
    }
    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        match pad {
            Pad::B => {
                if let Some(b) = self.picking {
                    self.back_to_list(cx, b, now);
                    return Out::Moved;
                }
                Out::Act("controls:remap-done".into())
            }
            Pad::A => {
                let Some(tag) = self.nav.current().map(|f| f.tag) else { return Out::None };
                match (self.picking, tag) {
                    (None, t) if (t as usize) < BUTTONS.len() => {
                        let b = BUTTONS[t as usize];
                        self.picking = Some(b);
                        let focus = match self.layout.target(b) {
                            Target::Nothing => T_NOTHING,
                            Target::Button(x) => T_PICK + BUTTONS.iter().position(|y| *y == x).unwrap_or(0) as u32,
                        };
                        self.draw(cx, now, Some(focus));
                        Out::Moved
                    }
                    (None, T_SWAP_AB) | (None, T_SWAP_XY) => {
                        let (a, b) = if tag == T_SWAP_AB { (Button::A, Button::B) } else { (Button::X, Button::Y) };
                        let mut l = self.layout.clone();
                        l.toggle_swap(a, b);
                        if self.change(cx, l) {
                            self.draw(cx, now, Some(tag));
                        }
                        Out::Moved
                    }
                    (None, T_RESET) => {
                        if self.change(cx, Layout::default()) {
                            self.draw(cx, now, Some(T_DONE));
                        }
                        Out::Moved
                    }
                    (None, T_DONE) => Out::Act("controls:remap-done".into()),
                    (Some(b), T_BACK) => {
                        self.back_to_list(cx, b, now);
                        Out::Moved
                    }
                    (Some(b), t) if t == T_NOTHING || (T_PICK..T_PICK + BUTTONS.len() as u32).contains(&t) => {
                        let target = if t == T_NOTHING { Target::Nothing } else { Target::Button(BUTTONS[(t - T_PICK) as usize]) };
                        let mut l = self.layout.clone();
                        l.set(b, target);
                        if self.change(cx, l) || self.layout.target(b) == target {
                            self.back_to_list(cx, b, now);
                        }
                        Out::Moved
                    }
                    _ => Out::None,
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

    fn tick(&mut self, cx: &mut Cx, _now: f64) {
        let done = self.pending.as_ref().and_then(|p| p.lock().ok().and_then(|mut g| g.take()));
        if let Some(st) = done {
            self.pending = None;
            let line = st.line().unwrap_or_else(|| HINT_LIST.to_string());
            self.set_note(cx, &line);
        }
    }

    fn next_deadline(&self) -> Option<f64> {
        self.pending.as_ref().map(|_| rare_render::clock::secs() + 0.1)
    }

    fn focus_id(&self) -> Option<Id> {
        self.nav.cur
    }
}

/// Fixtures: `settings-remap` (the list, A and B swapped) and `settings-remap-pick` (choosing
/// what A does).
pub fn fixture(name: &str, cx: &mut Cx, now: f64) -> Option<Box<dyn Screen>> {
    let mut l = Layout::default();
    l.toggle_swap(Button::A, Button::B);
    l.set(Button::View, Target::Nothing);
    match name {
        "settings-remap" => Some(Box::new(RemapSheet::open(cx, l, false, now))),
        "settings-remap-pick" => {
            let mut sh = RemapSheet::open(cx, l, false, now);
            sh.pad(cx, Pad::A, now);
            Some(Box::new(sh))
        }
        _ => None,
    }
}
