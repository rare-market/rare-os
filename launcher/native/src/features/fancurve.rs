//! The fan curve editor (`Overlay::Other(KIND)`), opened from Quick settings' Fan > Custom and
//! Settings > Power > Fan. It draws rare-fand's curve as bars at hhd's nine points (40 to 90 C),
//! with the temperature and fan speed now. LB and RB pick a preset (Quiet, Balanced, Max, which
//! apply at once); left and right pick a point, up and down change it by 5% (the points after it
//! rise with it, the ones before fall with it, so the fan never slows as it gets hotter); X saves
//! the curve as Custom. Reads the fan every 2 seconds while open.

use crate::act::Act;
use crate::kit::{self, s, tok, Ty, Ui, T};
use crate::page::{hash, Acts, Btn, Feedback, Fx, Page};
use crate::state::{State, Tone};
use rare_render::focus::hints::{hint, Hint, Key};
use rare_render::focus::{Focusable, Group};
use rare_render::view::{key, El, Motion};
use rare_system::system::FanStatus;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub const KIND: &str = "fan-curve";

/// hhd's points, the ones rare-fand's presets use.
pub const TEMPS: [u32; 9] = [40, 45, 50, 55, 60, 65, 70, 80, 90];
/// The presets LB and RB step through: (rare-fand name, label, what it does).
const PRESETS: [(&str, &str, &str); 4] = [
    ("quiet", "Quiet", "Stays low to about 65 C."),
    ("default", "Balanced", "The device's own curve."),
    ("cool", "Max", "Spins up early and runs flat out from 80 C."),
    ("custom", "Custom", "Your curve. Up and down change a point, X saves it."),
];
const STEP: u32 = 5;
/// Bar height in rem at 100%.
const BAR_H: f32 = 15.0;
const ACT_POINT: u32 = 100;
const ACT_SAVE: u32 = 200;

#[derive(Default)]
pub struct FanCurvePage {
    /// The fan as last read (a worker fills it; `seen` moves when it does).
    fan: Arc<Mutex<Option<FanStatus>>>,
    seen: Arc<AtomicU64>,
    /// The curve shown: the preset's, or the custom one being edited.
    points: Vec<u32>,
    /// The preset shown (index into PRESETS).
    preset: usize,
    /// Changed since the last save.
    dirty: bool,
    loaded: bool,
    next_read: f64,
    ver: u64,
}

/// Duty at each of TEMPS for a curve given as (temp, duty) points: linear between points, flat
/// outside them (rare-fand's interpolation).
pub fn at_temps(curve: &[(u32, u32)]) -> Vec<u32> {
    TEMPS
        .iter()
        .map(|&t| {
            let Some(first) = curve.first() else { return 0 };
            if t <= first.0 {
                return first.1;
            }
            for w in curve.windows(2) {
                let ((t0, d0), (t1, d1)) = (w[0], w[1]);
                if t <= t1 {
                    let f = (t - t0) as f32 / (t1 - t0).max(1) as f32;
                    return (d0 as f32 + (d1 as f32 - d0 as f32) * f).round() as u32;
                }
            }
            curve.last().map_or(0, |l| l.1)
        })
        .collect()
}

/// Moves point `i` by `delta` within 0..=100, keeping the curve from falling as it gets hotter.
pub fn nudge(points: &mut [u32], i: usize, delta: i32) {
    let Some(v) = points.get(i).copied() else { return };
    let n = (v as i32 + delta).clamp(0, 100) as u32;
    points[i] = n;
    for p in points[i + 1..].iter_mut() {
        *p = (*p).max(n);
    }
    for p in points[..i].iter_mut() {
        *p = (*p).min(n);
    }
}

fn preset_index(name: &str) -> usize {
    PRESETS.iter().position(|p| p.0 == name).unwrap_or(1)
}

/// "54 C, 2,898 rpm, fan at 41%".
pub fn now_line(f: &FanStatus) -> String {
    let mut parts = Vec::new();
    if let Some(t) = f.temp {
        parts.push(format!("{} C", t.round() as i64));
    }
    if let Some(r) = f.rpm {
        parts.push(format!("{} rpm", crate::kit::format::grouped(r as f64, 0, 0)));
    }
    if let Some(d) = f.duty {
        parts.push(format!("fan at {d}%"));
    }
    if parts.is_empty() {
        "Reading the fan".into()
    } else {
        parts.join(", ")
    }
}

impl FanCurvePage {
    fn read(&mut self, fx: &mut Fx) {
        let (fan, seen) = (self.fan.clone(), self.seen.clone());
        fx.spawn(move |b| {
            let f = b.fan_status();
            Box::new(move |_: &mut State, _| {
                *fan.lock().unwrap() = f;
                seen.fetch_add(1, Ordering::Relaxed);
            })
        });
    }

    fn fan_now(&self) -> Option<FanStatus> {
        self.fan.lock().unwrap().clone()
    }

    /// The fan's state arrived: the first time, it sets what the editor shows.
    fn take(&mut self, f: &FanStatus) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        self.preset = preset_index(&f.preset);
        self.points = if f.curve.is_empty() { at_temps(&rare_system::system::fan_preset_curve(&f.preset)) } else { at_temps(&f.curve) };
        self.ver += 1;
    }

    fn show_preset(&mut self, i: usize) {
        self.preset = i;
        let name = PRESETS[i].0;
        if name != "custom" {
            self.points = at_temps(&rare_system::system::fan_preset_curve(name));
            self.dirty = false;
        }
        self.ver += 1;
    }
}

impl Page for FanCurvePage {
    fn stamp(&self, _st: &State) -> u64 {
        let f = self.fan_now();
        hash((self.ver, &self.points, self.preset, self.dirty, f.map(|f| (f.temp.map(|t| t.round() as i64), f.rpm, f.duty, f.preset, f.mode))))
    }

    fn view(&mut self, ui: &mut Ui, _st: &State, acts: &mut Acts) -> El {
        let fan = self.fan_now();
        if let Some(f) = &fan {
            self.take(f);
        }
        let f = fan.as_ref();
        let now = f.map(now_line).unwrap_or_else(|| "Reading the fan".into());
        let head = ui
            .bx(s().row().items_center().gap(0.75).h(2.5).mb(0.5))
            .child(kit::icon(ui, "activity", 1.5, tok::ICON2))
            .child(ui.text("Fan curve", T::new(Ty::Title).w(700)))
            .child(ui.text_s(s().ml_auto(), now, T::new(Ty::Small).c(tok::MUTED).tab()));
        let mut page = ui.bx(s().col().fill().px(2.5).py(2.0)).bg(tok::BG).enter(Motion::FADE).scrolls_y(key("fan-curve")).child(head);
        if f.is_none() && self.points.is_empty() {
            return page.child(kit::empty(ui, s(), "No fan control here", Some("The fan follows the device's own curve. Rare OS controls the fan on the SuiPlay0X1."), vec![]));
        }
        // Presets: LB and RB.
        let mut chips = ui.bx(s().row().items_center().gap(0.5).mb(0.4));
        for (i, (_, label, _)) in PRESETS.iter().enumerate() {
            let on = i == self.preset;
            chips = chips.child(
                ui.bx(s().row().items_center().justify_center().h(2.25).min_w(5.5).px(1.0))
                    .bg(if on { tok::FG } else { tok::L2 })
                    .radius(ui.r(0.25))
                    .child(ui.text(*label, T::new(Ty::Small).w(500).c(if on { tok::BG } else { tok::FG })))
                    .key(key(&("fan-preset", i))),
            );
        }
        chips = chips.child(ui.text_s(s().ml(0.75).min0().shrink(), PRESETS[self.preset].2, T::new(Ty::Small).c(tok::MUTED).trunc()));
        page = page.child(chips);
        // The chart: one column per point; the one nearest the temperature now is marked.
        let near = f.and_then(|f| f.temp).map(|t| TEMPS.iter().enumerate().min_by_key(|(_, p)| ((**p as f64 - t).abs() * 10.0) as i64).map(|(i, _)| i).unwrap_or(0));
        let editing = PRESETS[self.preset].0 == "custom";
        let mut cols = ui.bx(s().row().items_end().gap(0.75).mt(1.25).wp(1.0).max_w(78.0)).group(Group::new("fan-points", [0.0; 4]).remember());
        for (i, (&t, &d)) in TEMPS.iter().zip(self.points.iter()).enumerate() {
            let bar = ui
                .bx(s().col().justify_end().h(BAR_H).wp(1.0))
                .bg(tok::L2)
                .radius(ui.r(0.4))
                .clip()
                .child(ui.bx(s().wp(1.0).h(BAR_H * d as f32 / 100.0)).bg(if editing { tok::fg(0.85) } else { tok::fg(0.45) }));
            let label = ui.text(format!("{t} C"), T::new(Ty::Small).tab().c(if near == Some(i) { tok::FG } else { tok::MUTED }));
            let marker = ui.text(if near == Some(i) { "Now" } else { " " }, T::new(Ty::Label).w(600).c(tok::FG));
            let col = ui
                .bx(s().col().items_center().gap(0.4).grow().min_w(4.0).py(0.5).px(0.4))
                .radius(ui.r(0.5))
                .child(ui.text(format!("{d}%"), T::new(Ty::Small).w(600).tab()))
                .child(bar)
                .child(label)
                .child(marker);
            let fl = col.look.clone().bg(tok::L3);
            let col = col.focus_look(fl);
            let tag = acts.tag(Act::Local(ACT_POINT + i as u32));
            // A on a point: edit the curve from here (a preset becomes Custom), save a changed one.
            let a = if !editing { "Edit" } else if self.dirty { "Save curve" } else { "Change" };
            cols = cols.child(kit::nav(col, tag, Some(a), None, i == 0).key(key(&("fan-col", i))));
        }
        page = page.child(cols);
        let note = if editing {
            if self.dirty {
                "Not saved yet. X saves this curve; B leaves it as it was."
            } else {
                "Up and down change the point you are on. The fan never slows down as it gets hotter."
            }
        } else {
            "Up or down on a point starts a custom curve from this one."
        };
        page = page.child(ui.text_s(s().mt(1.25), note, T::new(Ty::Small).c(tok::MUTED).wrap()));
        if f.is_some_and(|f| f.mode == "emergency" || f.mode == "failsafe") {
            page = page.child(ui.text_s(s().mt(0.5), "The fan is running flat out to cool the device down.", T::new(Ty::Small).c(tok::ATTN).wrap()));
        }
        let save = kit::button(ui, kit::Btn::new("Save curve", acts.tag(Act::Local(ACT_SAVE))).primary().disabled(!(editing && self.dirty)));
        page.child(ui.bx(s().row().mt(1.25)).child(save))
    }

    fn pad(&mut self, st: &mut State, fx: &mut Fx, b: Btn, cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        match b {
            Btn::Lb | Btn::Rb => {
                let n = PRESETS.len();
                let i = if b == Btn::Rb { (self.preset + 1) % n } else { (self.preset + n - 1) % n };
                self.show_preset(i);
                fx.fb(Feedback::Toggle);
                let name = PRESETS[i].0;
                if name == "custom" {
                    // Custom: the saved custom curve if there is one, else keep what is shown.
                    if let Some(c) = self.fan_now().filter(|f| f.preset == "custom" && !f.curve.is_empty()) {
                        self.points = at_temps(&c.curve);
                    }
                    return true;
                }
                let (fan, seen) = (self.fan.clone(), self.seen.clone());
                fx.spawn(move |be| {
                    let r = be.set_fan(name);
                    let f = be.fan_status();
                    Box::new(move |st: &mut State, now| {
                        if let Err(e) = r {
                            log::warn!("fan: {e}");
                            st.toast("Couldn't change the fan.", Tone::Error, now);
                        }
                        *fan.lock().unwrap() = f;
                        seen.fetch_add(1, Ordering::Relaxed);
                    })
                });
                true
            }
            Btn::Up | Btn::Down => {
                let Some(Act::Local(c)) = cur.and_then(|c| c.1) else { return false };
                let Some(i) = c.checked_sub(ACT_POINT).map(|i| i as usize).filter(|i| *i < TEMPS.len()) else { return false };
                if PRESETS[self.preset].0 != "custom" {
                    self.preset = preset_index("custom");
                }
                nudge(&mut self.points, i, if b == Btn::Up { STEP as i32 } else { -(STEP as i32) });
                self.dirty = true;
                self.ver += 1;
                fx.fb(Feedback::Move);
                true
            }
            Btn::X => {
                self.act(st, fx, ACT_SAVE);
                true
            }
            _ => false,
        }
    }

    fn act(&mut self, st: &mut State, fx: &mut Fx, code: u32) {
        if (ACT_POINT..ACT_POINT + TEMPS.len() as u32).contains(&code) {
            if PRESETS[self.preset].0 != "custom" {
                // Custom, starting from the curve shown: the bars light up for editing.
                self.preset = preset_index("custom");
                self.ver += 1;
                fx.fb(Feedback::Toggle);
            } else if self.dirty {
                return self.act(st, fx, ACT_SAVE);
            } else {
                let t = TEMPS[(code - ACT_POINT) as usize];
                st.toast(format!("Up and down change the fan at {t} C."), Tone::Info, rare_render::clock::secs());
            }
            return;
        }
        if code != ACT_SAVE || !self.dirty || PRESETS[self.preset].0 != "custom" {
            return;
        }
        let points: Vec<(u32, u32)> = TEMPS.iter().copied().zip(self.points.iter().copied()).collect();
        self.dirty = false;
        self.ver += 1;
        fx.fb(Feedback::Select);
        let (fan, seen) = (self.fan.clone(), self.seen.clone());
        fx.spawn(move |b| {
            let r = b.set_fan_curve(&points);
            let f = b.fan_status();
            Box::new(move |st: &mut State, now| {
                match r {
                    Ok(()) => st.toast("Fan curve saved", Tone::Info, now),
                    Err(e) => {
                        log::warn!("fan curve: {e}");
                        st.toast("Couldn't save the fan curve.", Tone::Error, now);
                    }
                }
                *fan.lock().unwrap() = f;
                seen.fetch_add(1, Ordering::Relaxed);
            })
        });
    }

    fn tick(&mut self, _st: &mut State, fx: &mut Fx, now: f64) -> Option<f64> {
        if now >= self.next_read {
            self.next_read = now + 2.0;
            self.read(fx);
        }
        Some(self.next_read)
    }

    fn hints(&self, _st: &State) -> Vec<Hint> {
        let mut h = vec![hint(Key::LbRb, "Preset"), hint(Key::B, "Back")];
        if self.dirty {
            h.insert(0, hint(Key::X, "Save curve"));
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_read_at_the_nine_points() {
        assert_eq!(at_temps(&rare_system::system::fan_preset_curve("quiet")), vec![20, 20, 20, 22, 26, 30, 50, 80, 100]);
        // Two points: flat outside, linear between.
        assert_eq!(at_temps(&[(50, 20), (90, 100)]), vec![20, 20, 20, 30, 40, 50, 60, 80, 100]);
        assert_eq!(at_temps(&[]), vec![0; 9]);
    }

    #[test]
    fn a_point_moves_its_neighbours_so_the_curve_never_falls() {
        let mut p = vec![20, 20, 20, 22, 26, 30, 50, 80, 100];
        nudge(&mut p, 2, 15);
        assert_eq!(p, vec![20, 20, 35, 35, 35, 35, 50, 80, 100]);
        nudge(&mut p, 6, -30);
        assert_eq!(p, vec![20, 20, 20, 20, 20, 20, 20, 80, 100]);
        nudge(&mut p, 8, 10);
        assert_eq!(p[8], 100);
        nudge(&mut p, 0, -40);
        assert_eq!(p[0], 0);
        assert!(p.windows(2).all(|w| w[1] >= w[0]));
    }

    #[test]
    fn the_now_line_reads_plainly() {
        let f = FanStatus { preset: "default".into(), curve: vec![], mode: "ec".into(), temp: Some(54.4), sensor: "edge".into(), duty: Some(41), rpm: Some(2898) };
        assert_eq!(now_line(&f), "54 C, 2,898 rpm, fan at 41%");
        assert_eq!(now_line(&FanStatus { temp: None, duty: None, rpm: None, ..f }), "Reading the fan");
    }
}
