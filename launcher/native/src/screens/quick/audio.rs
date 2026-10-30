//! Quick settings' sound rows under Volume: the output picker (only with two or more outputs) and
//! the mic (mute switch, level). The last reading is kept here, so the rows are in place the
//! moment the sheet opens and a fresh reading only changes values.

use super::{chip, chips, slider, switch_row};
use crate::act::Act;
use crate::kit::{s, Ui};
use crate::page::{Acts, Feedback, Fx};
use crate::state::{State, Tone};
use rare_render::view::El;
use rare_system::audio::Audio;
use std::sync::Mutex;

/// Act codes 500..599: 500 + i picks output i, then the mic switch and slider.
const OUT: u32 = 500;
const MUTE: u32 = 590;
pub const LEVEL: u32 = 591;

static LAST: Mutex<Option<Audio>> = Mutex::new(None);

pub fn owns(code: u32) -> bool {
    (500..600).contains(&code)
}

pub fn last() -> Option<Audio> {
    LAST.lock().ok()?.clone()
}

/// Keeps a reading (Settings' readings land here too). True when it changed.
pub fn remember(a: Audio) -> bool {
    match LAST.lock() {
        Ok(mut l) if l.as_ref() != Some(&a) => {
            *l = Some(a);
            true
        }
        _ => false,
    }
}

fn edit(f: impl FnOnce(&mut Audio)) {
    if let Ok(mut l) = LAST.lock() {
        if let Some(a) = l.as_mut() {
            f(a);
        }
    }
}

/// Reads outputs and the mic with the rest of Quick settings.
pub fn load(fx: &mut Fx) {
    fx.spawn(|b| {
        let a = b.audio();
        Box::new(move |st: &mut State, _| {
            if a.is_some_and(remember) {
                st.ver.qs += 1;
            }
        })
    });
}

/// Re-reads after a change; a failure is a toast.
fn after(fx: &mut Fx, what: &'static str, f: impl FnOnce() -> Result<(), String> + Send + 'static) {
    fx.spawn(move |b| {
        let r = f();
        let a = b.audio();
        Box::new(move |st: &mut State, now| {
            if let Err(e) = r {
                st.toast(crate::screens::settings::live::failed(what, &e, None), Tone::Error, now);
            }
            if let Some(a) = a {
                remember(a);
            }
            st.ver.qs += 1;
        })
    });
}

/// The rows, or None when there is nothing to show (no reading, one output and no mic).
pub fn view(ui: &mut Ui, acts: &mut Acts) -> Option<El> {
    let a = last()?;
    let mut col = ui.bx(s().col().gap(0.6));
    let mut any = false;
    if a.outputs.len() >= 2 {
        let mut seen: Vec<String> = Vec::new();
        let items: Vec<El> = a
            .outputs
            .iter()
            .take(50)
            .enumerate()
            .map(|(i, d)| {
                let mut l = d.short();
                if seen.contains(&l) {
                    l = format!("{l} {}", i + 1);
                }
                seen.push(l.clone());
                chip(ui, &l, d.default, acts.tag(Act::Local(OUT + i as u32)))
            })
            .collect();
        col = col.child(chips(ui, "Output", "qam-output", items));
        any = true;
    }
    if let Some(muted) = a.mic_muted {
        let sub = if muted { "Muted".to_string() } else { a.default_input().map(|d| d.label.clone()).unwrap_or_else(|| "On".into()) };
        col = col.child(switch_row(ui, "Microphone", &sub, !muted, acts.tag(Act::Local(MUTE))));
        any = true;
    }
    if let Some(l) = a.mic_level {
        col = col.child(slider(ui, "Mic level", "mic", l as f32, 0.0, 100.0, "%", acts.tag(Act::Local(LEVEL)), false));
        any = true;
    }
    any.then_some(col)
}

pub fn act(st: &mut State, fx: &mut Fx, code: u32) {
    let Some(a) = last() else { return };
    match code {
        MUTE => {
            let Some(muted) = a.mic_muted else { return };
            fx.fb(Feedback::Toggle);
            edit(|a| a.mic_muted = Some(!muted));
            st.ver.qs += 1;
            after(fx, if muted { "Couldn't turn the mic on." } else { "Couldn't mute the mic." }, move || rare_system::audio::set_mic_muted(!muted));
        }
        c if (OUT..OUT + 50).contains(&c) => {
            let Some(d) = a.outputs.get((c - OUT) as usize).cloned() else { return };
            if d.default {
                return;
            }
            edit(|a| a.outputs.iter_mut().for_each(|o| o.default = o.id == d.id));
            st.ver.qs += 1;
            after(fx, "Couldn't switch the output.", move || rare_system::audio::set_default_output(&d.id.to_string()));
        }
        _ => {}
    }
}

/// The mic level now (0 to 100), when known.
pub fn mic_level() -> Option<f32> {
    last().and_then(|a| a.mic_level).map(|v| v as f32)
}

/// Left or right on the mic slider. False when the focus is not ours.
pub fn nudge(st: &mut State, fx: &mut Fx, code: u32, step: f32) -> bool {
    if code != LEVEL {
        return false;
    }
    let Some(v) = last().and_then(|a| a.mic_level) else { return true };
    let n = (v as f32 + step).clamp(0.0, 100.0) as u8;
    edit(|a| a.mic_level = Some(n));
    st.ver.qs += 1;
    fx.spawn(move |_| {
        let _ = rare_system::audio::set_mic_level(n);
        Box::new(|_: &mut State, _| {})
    });
    true
}
