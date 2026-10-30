//! The rows every settings section is built from (screens/settings/rows.tsx): SettingRow, SwitchRow,
//! ChoiceRow, SliderRow. Each takes the Nav so focusable rows register themselves. `first` drops the
//! hairline above the first row of a panel (`first:border-t-0`).

use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::theme::tok;
use rare_render::{Id, Node};

pub struct SettingRow {
    pub row: Id,
    pub title: Id,
    pub body: Option<Id>,
    /// The right-hand slot (a control, a value, buttons).
    pub right: Id,
}

fn shell(cx: &mut Cx, parent: Id, title: &str, body: Option<&str>, first: bool, focusable: bool) -> SettingRow {
    let r = cx.r();
    let row = cx.scene.add(parent, s().row().items_center().gap(1.25 * r).min_h(4.25 * r).wp(1.0).px(1.4 * r).py(0.85 * r).done(), Node::default());
    if !first {
        edge(cx, row, 0, tok::HAIR);
    }
    let col = cx.scene.add(row, s().col().grow().done(), Node::default());
    let t = cx.scene.add_text(cx.t, col, s().done(), title, ts(r, ty::BODY.0, ty::BODY.1, 600), tok::FG);
    let b = body.map(|x| cx.scene.add_text(cx.t, col, s().done(), x, ts(r, ty::SMALL.0, ty::SMALL.1, 400).wrap(), tok::MUTED));
    let right = cx.scene.add(row, s().row().items_center().gap(0.5 * r).no_shrink_().done(), Node::default());
    let _ = focusable;
    SettingRow { row, title: t, body: b, right }
}

/// One settings row: title, a muted line, a control on the right (not focusable itself).
pub fn setting_row(cx: &mut Cx, parent: Id, title: &str, body: Option<&str>, first: bool) -> SettingRow {
    shell(cx, parent, title, body, first, false)
}

pub struct SwitchRow {
    pub row: SettingRow,
    pub track: Id,
    pub knob: Id,
    pub on: bool,
}

/// A row that is itself the switch: A flips it.
pub fn switch_row(cx: &mut Cx, nav: &mut Nav, parent: Id, sid: &str, title: &str, body: &str, on: bool, first: bool, group: u32) -> SwitchRow {
    let row = shell(cx, parent, title, Some(body), first, true);
    let (track, knob) = toggle(cx, row.right, on);
    nav.add(Focusable { id: row.row, kind: Kind::Row, group, hint: if on { "Turn off".into() } else { "Turn on".into() }, tag: 0, sid: sid.into() });
    SwitchRow { row, track, knob, on }
}

pub fn set_switch(cx: &mut Cx, nav: &mut Nav, sw: &mut SwitchRow, on: bool, now: f64) {
    sw.on = on;
    set_toggle(cx, sw.track, sw.knob, on, now);
    if let Some(f) = nav.items.iter_mut().find(|f| f.id == sw.row.row) {
        f.hint = if on { "Turn off".into() } else { "Turn on".into() };
    }
}

pub struct ChoiceRow {
    pub row: SettingRow,
    pub chips: Vec<Id>,
    pub labels: Vec<Id>,
    pub value: Option<usize>,
}

/// A row with a few choices side by side; the picked one is filled. One focus stop per choice.
#[allow(clippy::too_many_arguments)]
pub fn choice_row(cx: &mut Cx, nav: &mut Nav, parent: Id, sid: &str, title: &str, body: Option<&str>, choices: &[&str], value: Option<usize>, first: bool, group: u32) -> ChoiceRow {
    let r = cx.r();
    let row = shell(cx, parent, title, body, first, false);
    let mut chips = Vec::new();
    let mut labels = Vec::new();
    for (i, l) in choices.iter().enumerate() {
        let on = value == Some(i);
        let chip = cx.scene.add(row.right, s().row().items_center().justify_center().h(2.25 * r).min_w(3.3 * r).px(0.8 * r).done(), Node::bg(if on { tok::FG } else { tok::L2 }).radius(0.25 * r));
        let tid = cx.scene.add_text(cx.t, chip, s().done(), l, ts(r, ty::SMALL.0, ty::SMALL.1, 500).tabular(), if on { tok::BG } else { tok::FG });
        // A choice button is a .btn: its focus look is the white fill.
        nav.add(Focusable { id: chip, kind: Kind::Btn { base: if on { tok::FG } else { tok::L2 }, fg: if on { tok::BG } else { tok::FG }, text: tid }, group, hint: "Choose".into(), tag: i as u32, sid: sid.into() });
        chips.push(chip);
        labels.push(tid);
    }
    ChoiceRow { row, chips, labels, value }
}

/// Picks a choice: the filled chip moves. (Focus colours are restored from the Nav's base colours.)
pub fn set_choice(cx: &mut Cx, nav: &mut Nav, ch: &mut ChoiceRow, value: usize, now: f64) {
    ch.value = Some(value);
    for (i, chip) in ch.chips.iter().enumerate() {
        let on = i == value;
        let (base, fg) = if on { (tok::FG, tok::BG) } else { (tok::L2, tok::FG) };
        if let Some(f) = nav.items.iter_mut().find(|f| f.id == *chip) {
            if let Kind::Btn { base: b, fg: g, .. } = &mut f.kind {
                *b = base;
                *g = fg;
            }
        }
        if nav.cur != Some(*chip) {
            cx.scene.n(*chip).bg.set(base, now, 0.11);
            cx.scene.n(ch.labels[i]).text.as_mut().unwrap().color.set(fg, now, 0.11);
        }
    }
}

pub struct SliderRow {
    pub row: SettingRow,
    pub fill: Id,
    pub knob: Id,
    pub value_text: Id,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub value: f32,
    pub suffix: String,
}

const TRACK_W: f32 = 14.0;

/// A slider row. Focus it and press left or right (a nudge).
#[allow(clippy::too_many_arguments)]
pub fn slider_row(cx: &mut Cx, nav: &mut Nav, parent: Id, sid: &str, title: &str, body: Option<&str>, value: f32, min: f32, max: f32, step: f32, suffix: &str, first: bool, group: u32) -> SliderRow {
    let r = cx.r();
    let row = shell(cx, parent, title, body, first, true);
    let track = cx.scene.add(row.right, s().wh(TRACK_W * r, 0.35 * r).done(), Node::bg(tok::L4).radius(0.175 * r));
    let pct = ((value - min) / (max - min).max(1.0)).clamp(0.0, 1.0);
    let fill = cx.scene.add(track, s().abs(Some(0.0), None, Some(0.0), Some(0.0)).w(pct * TRACK_W * r).done(), Node::bg(tok::FG).radius(0.175 * r));
    let knob = cx.scene.add(track, s().abs(Some(-0.35 * r), None, None, Some(pct * TRACK_W * r - 0.525 * r)).wh(1.05 * r, 1.05 * r).done(), Node::bg(tok::FG).radius(0.525 * r));
    let vt = cx.scene.add(row.right, s().w(3.5 * r).row().justify_end().done(), Node::default());
    let value_text = cx.scene.add_text(cx.t, vt, s().done(), &format!("{}{}", value.round() as i64, suffix), ts(r, ty::BODY.0, ty::BODY.1, 500).tabular(), tok::FG);
    nav.add(Focusable { id: row.row, kind: Kind::Row, group, hint: String::new(), tag: 0, sid: sid.into() });
    note_track(cx.scene, row.row, track, min, max, step, value);
    SliderRow { row, fill, knob, value_text, min, max, step, value, suffix: suffix.into() }
}

thread_local! {
    /// Every slider row drawn (row node: its track, min, max, step, value), for touch.
    static TRACKS: std::cell::RefCell<Vec<(Id, Id, f32, f32, f32, f32)>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn note_track(scene: &rare_render::Scene, row: Id, track: Id, min: f32, max: f32, step: f32, value: f32) {
    TRACKS.with(|t| {
        let mut t = t.borrow_mut();
        t.retain(|e| e.0 != row && scene.alive(e.0));
        t.push((row, track, min, max, step, value));
    });
}

/// Touch: how many steps (signed) the slider row `row` moves for a finger at window `x` on its
/// track. None when `row` is not a slider.
pub fn steps_to(scene: &rare_render::Scene, row: Id, x: f32, now: f64) -> Option<i32> {
    let (_, track, min, max, step, value) = TRACKS.with(|t| t.borrow().iter().find(|e| e.0 == row).copied())?;
    let r = scene.window_rect(track, now);
    let frac = ((x - r[0]) / r[2].max(1.0)).clamp(0.0, 1.0);
    let want = min + frac * (max - min);
    let want = (min + ((want - min) / step.max(1e-6)).round() * step).clamp(min, max);
    Some(((want - value) / step.max(1e-6)).round() as i32)
}

/// Applies a nudge (-1 or +1): the new value, or None at an end.
pub fn nudge(cx: &mut Cx, sl: &mut SliderRow, dir: i32) -> Option<f32> {
    let v = (sl.value + dir as f32 * sl.step).clamp(sl.min, sl.max);
    if (v - sl.value).abs() < 1e-6 {
        return None;
    }
    set_slider(cx, sl, v);
    Some(v)
}

pub fn set_slider(cx: &mut Cx, sl: &mut SliderRow, v: f32) {
    let r = cx.r();
    sl.value = v;
    TRACKS.with(|t| {
        if let Some(e) = t.borrow_mut().iter_mut().find(|e| e.0 == sl.row.row) {
            e.5 = v;
        }
    });
    let pct = ((v - sl.min) / (sl.max - sl.min).max(1.0)).clamp(0.0, 1.0);
    let mut st = cx.scene.style(sl.fill);
    st.size.width = rare_render::taffy::prelude::length(pct * TRACK_W * r);
    cx.scene.set_style(sl.fill, st);
    let mut st = cx.scene.style(sl.knob);
    st.inset.left = rare_render::taffy::prelude::length(pct * TRACK_W * r - 0.525 * r);
    cx.scene.set_style(sl.knob, st);
    let txt = format!("{}{}", v.round() as i64, sl.suffix);
    cx.scene.set_text(cx.t, sl.value_text, &txt);
}

/// The panel every section sits on: rounded-panel bg-l1 (a card never holds a card).
pub fn panel(cx: &mut Cx, parent: Id) -> Id {
    let r = cx.r();
    cx.scene.add(parent, s().col().wp(1.0).done(), Node::bg(tok::L1).radius(1.0 * r).clip())
}

/// A value a row now holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Val {
    Bool(bool),
    Num(f32),
    /// The index of the picked choice.
    Idx(usize),
}

/// The interactive rows of one section, so a section's press and nudge are one lookup by id.
#[derive(Default)]
pub struct RowSet {
    pub sw: Vec<(String, SwitchRow)>,
    pub sl: Vec<(String, SliderRow)>,
    pub ch: Vec<(String, ChoiceRow)>,
}

impl RowSet {
    #[allow(clippy::too_many_arguments)]
    pub fn switch(&mut self, cx: &mut Cx, nav: &mut Nav, parent: Id, sid: &str, title: &str, body: &str, on: bool, first: bool) {
        let r = switch_row(cx, nav, parent, sid, title, body, on, first, 10);
        self.sw.push((sid.into(), r));
    }

    #[allow(clippy::too_many_arguments)]
    pub fn slider(&mut self, cx: &mut Cx, nav: &mut Nav, parent: Id, sid: &str, title: &str, body: Option<&str>, value: f32, min: f32, max: f32, step: f32, suffix: &str, first: bool) {
        let r = slider_row(cx, nav, parent, sid, title, body, value, min, max, step, suffix, first, 10);
        self.sl.push((sid.into(), r));
    }

    #[allow(clippy::too_many_arguments)]
    pub fn choice(&mut self, cx: &mut Cx, nav: &mut Nav, parent: Id, sid: &str, title: &str, body: Option<&str>, choices: &[&str], value: Option<usize>, first: bool) {
        let r = choice_row(cx, nav, parent, sid, title, body, choices, value, first, 10);
        self.ch.push((sid.into(), r));
    }

    /// A on the focused row: a switch flips, a choice is picked. Returns the row's id and new value.
    pub fn press(&mut self, cx: &mut Cx, nav: &mut Nav, now: f64) -> Option<(String, Val)> {
        let f = nav.current()?.clone();
        if let Some(i) = self.sw.iter().position(|(sid, _)| *sid == f.sid) {
            let on = !self.sw[i].1.on;
            set_switch(cx, nav, &mut self.sw[i].1, on, now);
            return Some((f.sid, Val::Bool(on)));
        }
        if let Some(i) = self.ch.iter().position(|(sid, _)| *sid == f.sid) {
            let pick = f.tag as usize;
            if self.ch[i].1.value == Some(pick) {
                return None;
            }
            set_choice(cx, nav, &mut self.ch[i].1, pick, now);
            return Some((f.sid, Val::Idx(pick)));
        }
        None
    }

    /// Left or right on a focused slider.
    pub fn nudge(&mut self, cx: &mut Cx, nav: &mut Nav, dir: i32) -> Option<(String, Val)> {
        let f = nav.current()?.clone();
        let i = self.sl.iter().position(|(sid, _)| *sid == f.sid)?;
        let v = nudge(cx, &mut self.sl[i].1, dir)?;
        Some((f.sid, Val::Num(v)))
    }

    pub fn is_slider(&self, sid: &str) -> bool {
        self.sl.iter().any(|(s, _)| s == sid)
    }
}
