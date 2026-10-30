//! The Sound section (SystemSections.tsx SoundSection): volume, the output, the microphone and UI
//! sounds.

use super::{config::Config, rows::*, SectionOut, SectionView};
use crate::screens::overlay::common::*;
use rare_render::Id;
use rare_system::audio::{Audio, Device, Kind};

pub struct SoundSection {
    rows: RowSet,
    /// The output ids, in chip order.
    outputs: Vec<u32>,
}

/// A fixture run's sound: the speaker and a mic.
fn fixture_audio() -> Audio {
    Audio {
        outputs: vec![Device { id: 1, name: "speaker".into(), label: "Speaker".into(), kind: Kind::Speaker, default: true }],
        inputs: vec![Device { id: 2, name: "mic".into(), label: "Built-in mic".into(), kind: Kind::Speaker, default: true }],
        mic_muted: Some(false),
        mic_level: Some(70),
        louder: Some(true),
    }
}

impl SoundSection {
    pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, cfg: &Config, now: f64) -> SoundSection {
        let (volume, audio) = match super::live::get() {
            Some(s) => (s.volume, s.audio),
            None => (Some(60.0), Some(fixture_audio())),
        };
        SoundSection::build_with(cx, nav, parent, cfg, volume, audio, now)
    }

    /// `volume`: None when no sound output was found. `audio`: None until the first reading.
    pub fn build_with(cx: &mut Cx, nav: &mut Nav, parent: Id, cfg: &Config, volume: Option<f32>, audio: Option<Audio>, _now: f64) -> SoundSection {
        let p = panel(cx, parent);
        let mut rows = RowSet::default();
        match volume {
            Some(v) => rows.slider(cx, nav, p, "volume", "Volume", Some("Left and right to change."), v, 0.0, 100.0, 5.0, "%", true),
            None => {
                setting_row(cx, p, "Volume", Some("No sound output found."), true);
            }
        }
        let mut first = false;
        let mut outputs = vec![];
        if let Some(a) = &audio {
            if !a.outputs.is_empty() {
                let mut seen: Vec<String> = vec![];
                let labels: Vec<String> = a
                    .outputs
                    .iter()
                    .enumerate()
                    .map(|(i, d)| {
                        let mut l = d.short();
                        if seen.contains(&l) {
                            l = format!("{l} {}", i + 1);
                        }
                        seen.push(l.clone());
                        l
                    })
                    .collect();
                let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
                let cur = a.outputs.iter().position(|d| d.default);
                let body = match cur.map(|i| &a.outputs[i]) {
                    Some(d) => format!("Playing on {}.", d.label),
                    None => "Pick where sound plays.".to_string(),
                };
                if a.outputs.len() == 1 && cur == Some(0) {
                    // One output and it is in use: nothing to pick, so no button to press.
                    setting_row(cx, p, "Output", Some(&body), first);
                } else {
                    rows.choice(cx, nav, p, "output", "Output", Some(&body), &refs, cur, first);
                    outputs = a.outputs.iter().map(|d| d.id).collect();
                }
                first = false;
            }
            // The SuiPlay0X1's speaker chain: more makeup gain, the limiter keeps peaks clean.
            if let Some(on) = a.louder {
                rows.switch(cx, nav, p, "louder", "Louder speakers", "More volume from the built-in speakers. A limiter keeps loud parts clean.", on, first);
                first = false;
            }
            match a.mic_muted {
                Some(muted) => {
                    let body = a.default_input().map(|d| d.label.clone()).unwrap_or_else(|| "Microphone".into());
                    rows.switch(cx, nav, p, "mic", "Microphone", &format!("{body}. Off mutes it for every app."), !muted, first);
                    if let Some(l) = a.mic_level {
                        rows.slider(cx, nav, p, "mic-level", "Mic level", Some("How loud you sound to others."), l as f32, 0.0, 100.0, 5.0, "%", false);
                    }
                }
                None => {
                    setting_row(cx, p, "Microphone", Some("No microphone found."), first);
                }
            }
            first = false;
        }
        rows.switch(cx, nav, p, "sounds", "Sounds", "Soft clicks when you move, select and go back.", cfg.ui_sounds.unwrap_or(true), first);
        SoundSection { rows, outputs }
    }
}

impl SectionView for SoundSection {
    fn press(&mut self, cx: &mut Cx, nav: &mut Nav, cfg: &mut Config, now: f64) -> SectionOut {
        match self.rows.press(cx, nav, now) {
            Some((sid, Val::Bool(v))) if sid == "sounds" => {
                cfg.ui_sounds = Some(v);
                SectionOut::Changed
            }
            // The switch is "on" when the mic is not muted.
            Some((sid, Val::Bool(v))) if sid == "mic" => SectionOut::Act(format!("sound:mic-mute:{}", !v)),
            Some((sid, Val::Bool(v))) if sid == "louder" => SectionOut::Act(format!("sound:louder:{v}")),
            Some((sid, Val::Idx(i))) if sid == "output" => match self.outputs.get(i) {
                Some(id) => SectionOut::Act(format!("sound:output:{id}")),
                None => SectionOut::None,
            },
            _ => SectionOut::None,
        }
    }

    fn nudge(&mut self, cx: &mut Cx, nav: &mut Nav, _cfg: &mut Config, dir: i32, _now: f64) -> SectionOut {
        match self.rows.nudge(cx, nav, dir) {
            Some((sid, Val::Num(v))) if sid == "volume" => SectionOut::Act(format!("sound:volume:{v}")),
            Some((sid, Val::Num(v))) if sid == "mic-level" => SectionOut::Act(format!("sound:mic-level:{v}")),
            Some(_) => SectionOut::None,
            None if nav.current().is_some_and(|f| self.rows.is_slider(&f.sid)) => SectionOut::None,
            None => SectionOut::Pass,
        }
    }
}
