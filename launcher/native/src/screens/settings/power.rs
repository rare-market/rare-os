//! The Power section (screens/settings/PowerSection.tsx, lib/sleep.ts pure parts): profile, power
//! limit, CPU boost, fan, sleep when idle, battery care and health.

use super::{config::Config, rows::*, SectionOut, SectionView};
use crate::screens::overlay::common::*;
use rare_render::Id;

pub const DEFAULT_IDLE_BATTERY_MIN: f32 = 15.0;
/// Choices shown in Settings; 0 is never.
pub const IDLE_CHOICES: [f32; 6] = [0.0, 5.0, 10.0, 15.0, 30.0, 60.0];

/// Minutes of idle before sleeping for this power source; 0 means never.
pub fn idle_limit_min(cfg: &Config, on_battery: bool) -> u32 {
    let v = if on_battery { cfg.sleep_idle_battery.unwrap_or(DEFAULT_IDLE_BATTERY_MIN) } else { cfg.sleep_idle_ac.unwrap_or(0.0) };
    if v.is_finite() && v > 0.0 {
        v.floor() as u32
    } else {
        0
    }
}

pub struct SleepInputs {
    pub idle_ms: f64,
    pub limit_min: u32,
    /// Games, apps and streams that are running.
    pub running: u32,
    pub audio_playing: bool,
    /// First run or sign-in is on screen.
    pub setup: bool,
}

pub fn should_sleep(i: &SleepInputs) -> bool {
    if i.limit_min == 0 || i.running > 0 || i.audio_playing || i.setup {
        return false;
    }
    i.idle_ms >= i.limit_min as f64 * 60_000.0
}

/// The next label in Settings' cycle.
pub fn next_idle_choice(cur: f32) -> f32 {
    match IDLE_CHOICES.iter().position(|c| *c == cur) {
        Some(i) => IDLE_CHOICES[(i + 1) % IDLE_CHOICES.len()],
        // An unknown value: JS indexOf is -1, so the cycle lands on the first choice.
        None => IDLE_CHOICES[0],
    }
}

pub fn idle_label(min: f32) -> String {
    if min <= 0.0 {
        "Never".into()
    } else if min >= 60.0 {
        format!("{} h", min / 60.0)
    } else {
        format!("{} min", min)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum CareState {
    Off,
    Unavailable,
    Unplugged,
    Low,
    Hot,
    Unknown,
    Charging,
    Holding,
}

/// The line under Battery care for a limit (rare-fand charges again 5 points under it, and
/// always charges 10 points under it).
pub fn care_note(s: CareState, limit: u32) -> String {
    let (low, floor) = (limit.saturating_sub(5), limit.saturating_sub(10));
    match s {
        CareState::Off => format!("Charging stops at {limit}% while plugged in and starts again at {low}%, so the battery ages more slowly."),
        CareState::Unavailable => String::new(),
        CareState::Unplugged => format!("Charging stops at {limit}% the next time you plug in."),
        CareState::Low => format!("Charging normally until the battery is above {floor}%."),
        CareState::Hot => "Charging normally while the battery is warm.".into(),
        CareState::Unknown => "Charging normally while the battery level can't be read.".into(),
        CareState::Charging => format!("Plugged in and charging to {limit}%."),
        CareState::Holding => format!("Plugged in and holding at {limit}%."),
    }
}

/// Battery health's line: wear and charge cycles.
pub fn health_note(health: u32, b: &BatteryExtra) -> String {
    let worn = 100u32.saturating_sub(health);
    let wear = if worn == 0 { "As much charge as when it was new.".to_string() } else { format!("Holds {worn}% less charge than when it was new.") };
    let cycles = match b.cycles {
        Some(1) if b.counted => " 1 charge cycle since Rare OS was installed.".to_string(),
        Some(c) if b.counted => format!(" {c} charge cycles since Rare OS was installed."),
        Some(1) => " 1 charge cycle.".to_string(),
        Some(c) => format!(" {c} charge cycles."),
        None => String::new(),
    };
    wear + &cycles
}

/// Charge cycles and the charge limit (rare-fand), beside health.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct BatteryExtra {
    pub cycles: Option<u32>,
    /// The count is rare-fand's, since it was installed (the battery reports none).
    pub counted: bool,
    /// Battery care's stop point when rare-fand can change it.
    pub limit: Option<u32>,
}

/// Charge limits Settings offers.
pub const LIMITS: [u32; 3] = [70, 80, 90];

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct BatteryCare {
    pub available: bool,
    pub on: bool,
    pub state: CareState,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Perf {
    pub available: bool,
    pub tdp: Option<f32>,
    pub tdp_min: f32,
    pub tdp_max: f32,
    /// "quiet", "balanced", "performance" or "custom".
    pub profile: String,
    pub note: String,
    pub cpu_boost: Option<bool>,
    /// "default", "cool", "quiet" or "custom".
    pub fan: Option<String>,
    pub profile_watts: Option<(f32, f32, f32)>,
    /// "54 C, 2,898 rpm" (rare-fand).
    #[serde(default)]
    pub fan_now: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct PowerInfo {
    /// None: the power service did not answer.
    pub perf: Option<Perf>,
    pub care: Option<BatteryCare>,
    pub health: Option<u32>,
    #[serde(default)]
    pub battery: BatteryExtra,
}

pub fn fixture_info() -> PowerInfo {
    PowerInfo {
        perf: Some(Perf { available: true, tdp: Some(15.0), tdp_min: 4.0, tdp_max: 30.0, profile: "balanced".into(), note: String::new(), cpu_boost: Some(true), fan: Some("default".into()), profile_watts: Some((10.0, 15.0, 25.0)), fan_now: Some("54 C, 2,898 rpm".into()) }),
        care: Some(BatteryCare { available: true, on: std::env::var("RR_CARE").is_ok(), state: if std::env::var("RR_CARE").is_ok() { CareState::Holding } else { CareState::Off } }),
        health: Some(92),
        battery: BatteryExtra { cycles: Some(37), counted: true, limit: Some(80) },
    }
}

const PROFILES: [(&str, &str); 3] = [("quiet", "Quiet"), ("balanced", "Balanced"), ("performance", "Performance")];
/// rare-fand's presets as Settings names them; Custom opens the curve editor.
const FANS: [(&str, &str); 4] = [("quiet", "Quiet"), ("default", "Balanced"), ("cool", "Max"), ("custom", "Custom")];

pub struct PowerSection {
    rows: RowSet,
    perf: Option<Perf>,
    care: Option<BatteryCare>,
    battery_btn: Id,
    ac_btn: Id,
    battery_label: Id,
    ac_label: Id,
}

impl PowerSection {
    pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, cfg: &Config, now: f64) -> PowerSection {
        let info = super::live::get().map(|s| s.power).unwrap_or_else(fixture_info);
        PowerSection::build_with(cx, nav, parent, cfg, &info, now)
    }

    pub fn build_with(cx: &mut Cx, nav: &mut Nav, parent: Id, cfg: &Config, info: &PowerInfo, _now: f64) -> PowerSection {
        let mut rows = RowSet::default();
        let p1 = panel(cx, parent);
        if let Some(perf) = &info.perf {
            if perf.available {
                let watts = if perf.profile == "custom" { perf.tdp.unwrap_or(15.0) } else { perf.profile_watts.map(|w| match perf.profile.as_str() { "quiet" => w.0, "balanced" => w.1, _ => w.2 }).unwrap_or(15.0) };
                let body = format!("{} W. Kept after a restart. Each game starts on Balanced unless you save settings for it in Quick settings.", watts.round() as i64);
                let labels: Vec<&str> = PROFILES.iter().map(|(_, l)| *l).collect();
                let cur = PROFILES.iter().position(|(k, _)| *k == perf.profile);
                rows.choice(cx, nav, p1, "profile", "Performance profile", Some(&body), &labels, cur, true);
                rows.slider(cx, nav, p1, "tdp", "Power limit", Some("A limit of your own instead of a profile."), perf.tdp.unwrap_or(15.0), perf.tdp_min, perf.tdp_max, 1.0, " W", false);
                if let Some(b) = perf.cpu_boost {
                    rows.switch(cx, nav, p1, "cpu-boost", "CPU boost", "Faster CPU, more heat and battery.", b, false);
                }
                if let Some(f) = &perf.fan {
                    let labels: Vec<&str> = FANS.iter().map(|(_, l)| *l).collect();
                    let cur = FANS.iter().position(|(k, _)| k == f);
                    let what = "Quiet stays low to about 65 C. Max spins up early. Custom opens the curve editor.";
                    let body = match &perf.fan_now {
                        Some(n) => format!("{n} now. {what}"),
                        None => what.to_string(),
                    };
                    rows.choice(cx, nav, p1, "fan", "Fan", Some(&body), &labels, cur, false);
                }
            } else {
                let note = if perf.note.is_empty() { "This device does not let Rare OS change its power limits." } else { &perf.note };
                setting_row(cx, p1, "Performance", Some(note), true);
            }
        }
        let p2 = panel(cx, parent);
        let row_ = setting_row(cx, p2, "Sleep when idle", Some("Sleeps after this long with no input. Never while a game runs or sound plays. Press the power button to wake."), true);
        let bat = cfg.sleep_idle_battery.unwrap_or(DEFAULT_IDLE_BATTERY_MIN);
        let ac = cfg.sleep_idle_ac.unwrap_or(0.0);
        let b1 = button(cx, row_.right, &format!("On battery: {}", idle_label(bat)), Variant::Secondary, Sz::Md, false).sid("sleep-idle").tag(1);
        let battery_label = match b1.kind {
            Kind::Btn { text, .. } => text,
            _ => unreachable!(),
        };
        let battery_btn = nav.add(b1.group(10));
        let b2 = button(cx, row_.right, &format!("Charging: {}", idle_label(ac)), Variant::Secondary, Sz::Md, false).sid("sleep-idle").tag(2);
        let ac_label = match b2.kind {
            Kind::Btn { text, .. } => text,
            _ => unreachable!(),
        };
        let ac_btn = nav.add(b2.group(10));
        // BatteryRows
        if let Some(care) = &info.care {
            if care.available {
                let limit = info.battery.limit.unwrap_or(80);
                let note = care_note(if care.on { care.state } else { CareState::Off }, limit);
                let note = if note.is_empty() { care_note(CareState::Off, limit) } else { note };
                rows.switch(cx, nav, p2, "battery-care", &format!("Battery care (stop at {limit}%)"), &note, care.on, false);
                if care.on && info.battery.limit.is_some() {
                    let labels: Vec<String> = LIMITS.iter().map(|l| format!("{l}%")).collect();
                    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
                    let cur = LIMITS.iter().position(|l| *l == limit);
                    rows.choice(cx, nav, p2, "charge-limit", "Stop charging at", Some("Lower keeps the battery healthy for longer. Higher lasts longer away from the charger."), &labels, cur, false);
                }
            }
        }
        if let Some(h) = info.health {
            setting_row(cx, p2, &format!("Battery health {h}%"), Some(&health_note(h, &info.battery)), false);
        }
        PowerSection { rows, perf: info.perf.clone(), care: info.care.clone(), battery_btn, ac_btn, battery_label, ac_label }
    }
}

impl SectionView for PowerSection {
    fn press(&mut self, cx: &mut Cx, nav: &mut Nav, cfg: &mut Config, now: f64) -> SectionOut {
        if let Some(cur) = nav.cur {
            if cur == self.battery_btn {
                let next = next_idle_choice(cfg.sleep_idle_battery.unwrap_or(DEFAULT_IDLE_BATTERY_MIN));
                cfg.sleep_idle_battery = Some(next);
                cx.scene.set_text(cx.t, self.battery_label, &format!("On battery: {}", idle_label(next)));
                return SectionOut::Changed;
            }
            if cur == self.ac_btn {
                let next = next_idle_choice(cfg.sleep_idle_ac.unwrap_or(0.0));
                cfg.sleep_idle_ac = Some(next);
                cx.scene.set_text(cx.t, self.ac_label, &format!("Charging: {}", idle_label(next)));
                return SectionOut::Changed;
            }
        }
        match self.rows.press(cx, nav, now) {
            Some((sid, Val::Idx(i))) if sid == "profile" => SectionOut::Act(format!("power:profile:{}", PROFILES[i].0)),
            Some((sid, Val::Idx(3))) if sid == "fan" => SectionOut::Act("open:fan-curve".into()),
            Some((sid, Val::Idx(i))) if sid == "fan" => SectionOut::Act(format!("power:fan:{}", FANS[i].0)),
            Some((sid, Val::Idx(i))) if sid == "charge-limit" => SectionOut::Act(format!("power:charge-limit:{}", LIMITS[i])),
            Some((sid, Val::Bool(v))) if sid == "cpu-boost" => SectionOut::Act(format!("power:cpu-boost:{v}")),
            Some((sid, Val::Bool(v))) if sid == "battery-care" => SectionOut::Act(format!("power:battery-care:{v}")),
            _ => SectionOut::None,
        }
    }

    fn nudge(&mut self, cx: &mut Cx, nav: &mut Nav, _cfg: &mut Config, dir: i32, _now: f64) -> SectionOut {
        match self.rows.nudge(cx, nav, dir) {
            Some((sid, Val::Num(v))) if sid == "tdp" => SectionOut::Act(format!("power:tdp:{v}")),
            Some(_) => SectionOut::None,
            None if nav.current().is_some_and(|f| self.rows.is_slider(&f.sid)) => SectionOut::None,
            None => SectionOut::Pass,
        }
    }
}

impl PowerSection {
    pub fn perf(&self) -> Option<&Perf> {
        self.perf.as_ref()
    }
    pub fn care(&self) -> Option<&BatteryCare> {
        self.care.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> SleepInputs {
        SleepInputs { idle_ms: 16.0 * 60_000.0, limit_min: 15, running: 0, audio_playing: false, setup: false }
    }

    #[test]
    fn defaults_to_15_minutes_on_battery_and_never_while_charging() {
        let c = Config::default();
        assert_eq!(idle_limit_min(&c, true), 15);
        assert_eq!(idle_limit_min(&c, false), 0);
        assert_eq!(idle_limit_min(&Config { sleep_idle_battery: Some(0.0), ..Default::default() }, true), 0);
        assert_eq!(idle_limit_min(&Config { sleep_idle_ac: Some(30.0), ..Default::default() }, false), 30);
    }

    #[test]
    fn sleeps_once_idle_past_the_limit() {
        assert!(should_sleep(&base()));
        assert!(!should_sleep(&SleepInputs { idle_ms: 14.0 * 60_000.0, ..base() }));
    }

    #[test]
    fn never_sleeps_under_a_game_audio_setup_or_a_zero_limit() {
        assert!(!should_sleep(&SleepInputs { running: 1, ..base() }));
        assert!(!should_sleep(&SleepInputs { audio_playing: true, ..base() }));
        assert!(!should_sleep(&SleepInputs { setup: true, ..base() }));
        assert!(!should_sleep(&SleepInputs { limit_min: 0, ..base() }));
    }

    #[test]
    fn battery_lines_follow_the_limit_and_the_count() {
        assert_eq!(care_note(CareState::Holding, 90), "Plugged in and holding at 90%.");
        assert_eq!(care_note(CareState::Off, 80), "Charging stops at 80% while plugged in and starts again at 75%, so the battery ages more slowly.");
        assert_eq!(care_note(CareState::Low, 70), "Charging normally until the battery is above 60%.");
        let b = BatteryExtra { cycles: Some(37), counted: true, limit: Some(80) };
        assert_eq!(health_note(92, &b), "Holds 8% less charge than when it was new. 37 charge cycles since Rare OS was installed.");
        assert_eq!(health_note(100, &BatteryExtra { cycles: Some(1), ..Default::default() }), "As much charge as when it was new. 1 charge cycle.");
        assert_eq!(health_note(90, &BatteryExtra::default()), "Holds 10% less charge than when it was new.");
    }

    #[test]
    fn cycles_the_settings_choices() {
        assert_eq!(next_idle_choice(15.0), 30.0);
        assert_eq!(next_idle_choice(60.0), 0.0);
        assert_eq!(next_idle_choice(7.0), 0.0);
        assert_eq!(idle_label(0.0), "Never");
        assert_eq!(idle_label(60.0), "1 h");
        assert_eq!(idle_label(15.0), "15 min");
    }
}
