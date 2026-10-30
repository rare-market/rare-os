//! The Display section (SystemSections.tsx DisplaySection, DisplayRows.tsx): what the screen is
//! (built in, or docked to a TV), the UI size, brightness, dim after idle. lib/display.ts logic.

use super::{config::Config, rows::*, SectionOut, SectionView};
use crate::screens::overlay::common::*;
use rare_render::theme::tok;
use rare_render::{Id, Node};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiScale {
    Auto,
    Large,
    Larger,
}

pub const UI_SCALES: [UiScale; 3] = [UiScale::Auto, UiScale::Large, UiScale::Larger];

/// Multiplier on top of the automatic size, written to --ui-scale.
pub fn scale_factor(s: UiScale) -> f32 {
    match s {
        UiScale::Large => 1.15,
        UiScale::Larger => 1.3,
        UiScale::Auto => 1.0,
    }
}

pub fn scale_of(c: &Config) -> UiScale {
    match c.ui_scale.as_deref() {
        Some("large") => UiScale::Large,
        Some("larger") => UiScale::Larger,
        _ => UiScale::Auto,
    }
}

pub fn next_scale(s: UiScale) -> UiScale {
    let i = UI_SCALES.iter().position(|x| *x == s).unwrap_or(0);
    UI_SCALES[(i + 1) % UI_SCALES.len()]
}

pub fn scale_label(s: UiScale) -> &'static str {
    match s {
        UiScale::Auto => "Automatic",
        UiScale::Large => "Large",
        UiScale::Larger => "Larger",
    }
}

/// The root font size in px for a window: 16 at 1280x800, 24 at 1920x1200, 21.6 at 1920x1080.
pub fn root_px(width: f32, height: f32, s: UiScale) -> f32 {
    (width / 80.0).min(height / 50.0) * scale_factor(s)
}

/// "3840x2160" for a window, rounded to what the output really is.
pub fn size_label(w: f32, h: f32) -> String {
    format!("{}x{}", w.round() as i64, h.round() as i64)
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub struct DisplayOutput {
    pub name: String,
    pub internal: bool,
    pub connected: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub struct DisplayState {
    pub outputs: Vec<DisplayOutput>,
    pub active: Option<String>,
    pub docked: bool,
    pub dock_name: Option<String>,
    pub dock_mode: Option<String>,
}

/// The Docked line for Settings > Display.
pub fn docked_line(d: Option<&DisplayState>) -> String {
    let Some(d) = d else { return String::new() };
    if !d.docked {
        return "Built-in screen".into();
    }
    let name = d.dock_name.clone().or_else(|| d.active.clone()).unwrap_or_else(|| "external display".into());
    match &d.dock_mode {
        Some(m) if !m.is_empty() => format!("Docked to {name} {m}"),
        _ => format!("Docked to {name}"),
    }
}

/// The toast for a change between two readings, or None when nothing worth saying changed.
pub fn dock_change(prev: Option<&DisplayState>, next: &DisplayState) -> Option<String> {
    let prev = prev?; // the first reading: starting up already docked is not news
    if !prev.docked && next.docked {
        return Some(docked_line(Some(next)));
    }
    if prev.docked && !next.docked {
        return Some("Back on the built-in screen".into());
    }
    if prev.docked && next.docked && (prev.dock_name != next.dock_name || prev.dock_mode != next.dock_mode) {
        return Some(docked_line(Some(next)));
    }
    None
}

/// What the Display section reads from the system.
#[derive(Clone, Debug)]
pub struct DisplayInfo {
    pub state: Option<DisplayState>,
    /// None: no brightness on this device.
    pub brightness: Option<f32>,
    pub dim_idle: Option<bool>,
    pub window: (f32, f32),
    /// Night light and the TV's picture (rare_system::screen).
    pub screen: rare_system::screen::ScreenSettings,
    /// The TV's modes when docked.
    pub tv_modes: Option<Vec<(u32, u32, u32)>>,
}

pub fn fixture_info(window: (f32, f32)) -> DisplayInfo {
    DisplayInfo {
        state: Some(DisplayState { outputs: vec![DisplayOutput { name: "DSI-1".into(), internal: true, connected: true }], active: Some("DSI-1".into()), ..Default::default() }),
        brightness: Some(70.0),
        dim_idle: Some(false),
        window,
        screen: Default::default(),
        tv_modes: None,
    }
}

pub struct DisplaySection {
    rows: RowSet,
    /// The TV rows' choices: sizes (0 x 0 the TV's own) and refresh rates (0 the TV's own).
    tv: (Vec<(u32, u32)>, Vec<u32>),
}

const WARMTH: (f32, f32, f32) = (10.0, 100.0, 10.0);

/// Night light: on, how warm, only at night. Its own panel under the screen rows.
fn build_night(cx: &mut Cx, nav: &mut Nav, parent: Id, rows: &mut RowSet, n: &rare_system::screen::NightLight) {
    let p = panel(cx, parent);
    rows.switch(cx, nav, p, "night-light", "Night light", "Warmer colours that are easier on the eyes at night. Games too.", n.on, true);
    rows.slider(cx, nav, p, "night-warmth", "Warmth", Some("Left and right to change."), n.warmth.clamp(10, 100) as f32, WARMTH.0, WARMTH.1, WARMTH.2, "%", false);
    rows.switch(cx, nav, p, "night-schedule", "Only at night", "On from 9 PM to 7 AM. Off: on all the time while Night light is on.", n.scheduled, false);
}

/// "4K", "1440p", "1080p", "720p" for a size.
pub fn tv_size_label(w: u32, h: u32) -> String {
    match (w, h) {
        (0, _) | (_, 0) => "TV's own".into(),
        (3840, 2160) => "4K".into(),
        (_, h) => format!("{h}p"),
    }
}

/// On a TV: the size games and Rare draw at, and the refresh rate. Nothing on the built-in screen.
fn build_tv(cx: &mut Cx, nav: &mut Nav, parent: Id, rows: &mut RowSet, docked: bool, cur: rare_system::screen::TvMode, modes: Option<&[(u32, u32, u32)]>) -> (Vec<(u32, u32)>, Vec<u32>) {
    let Some(modes) = modes.filter(|m| docked && !m.is_empty()) else { return (vec![], vec![]) };
    let p = panel(cx, parent);
    let mut sizes = vec![(0, 0)];
    sizes.extend(rare_system::screen::tv_sizes(modes).into_iter().filter(|s| Some(*s) != modes.first().map(|m| (m.0, m.1))));
    let labels: Vec<String> = sizes.iter().map(|(w, h)| tv_size_label(*w, *h)).collect();
    let lr: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
    let at = sizes.iter().position(|s| *s == (cur.width, cur.height)).unwrap_or(0);
    rows.choice(cx, nav, p, "tv-size", "Resolution", Some("What games and Rare draw at. The TV shows it full screen. Lower is smoother in heavy games."), &lr, Some(at), true);
    let mut rates = vec![0];
    rates.extend(rare_system::screen::tv_refreshes(modes));
    if rates.len() > 2 {
        let labels: Vec<String> = rates.iter().map(|r| if *r == 0 { "TV's own".to_string() } else { format!("{r} Hz") }).collect();
        let lr: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
        let at = rates.iter().position(|r| *r == cur.refresh).unwrap_or(0);
        rows.choice(cx, nav, p, "tv-refresh", "Refresh rate", Some("How many times a second the TV draws a new picture."), &lr, Some(at), false);
    }
    (sizes, rates)
}

impl DisplaySection {
    pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, cfg: &Config, now: f64) -> DisplaySection {
        let w = (cx.scene.w, cx.scene.h);
        let info = match super::live::get() {
            Some(s) => DisplayInfo { state: s.display.0, brightness: s.display.1, dim_idle: s.display.2, window: w, screen: rare_system::screen::saved(), tv_modes: rare_system::screen::cached_tv_modes() },
            None => fixture_info(w),
        };
        DisplaySection::build_with(cx, nav, parent, cfg, &info, now)
    }

    pub fn build_with(cx: &mut Cx, nav: &mut Nav, parent: Id, cfg: &Config, info: &DisplayInfo, _now: f64) -> DisplaySection {
        let r = cx.r();
        let p = panel(cx, parent);
        let mut rows = RowSet::default();
        let docked = info.state.as_ref().map(|s| s.docked).unwrap_or(false);
        let external = info.state.as_ref().map(|s| s.outputs.iter().filter(|o| !o.internal && o.connected).count()).unwrap_or(0);
        // DisplayRows: the Docked line and UI size.
        let row_ = setting_row(cx, p, "", None, true);
        cx.scene.set_text(cx.t, row_.title, "");
        // The title is a dot and the line, side by side.
        let line = docked_line(info.state.as_ref());
        let line = if line.is_empty() { "Built-in screen".to_string() } else { line };
        let tp = cx.scene.parent(row_.title).unwrap();
        cx.scene.remove(row_.title);
        let inline = cx.scene.insert(tp, 0, super::super::overlay::lay::s().row().items_center().gap(0.6 * r).done(), Node::default());
        dot(cx, inline, if docked { tok::FG } else { tok::MUTED });
        cx.scene.add_text(cx.t, inline, super::super::overlay::lay::s().done(), &line, ts(r, ty::BODY.0, ty::BODY.1, 600), tok::FG);
        let (w, h) = info.window;
        let body = if docked {
            format!("The picture and the sound are on the display. Showing at {}. Unplug the cable to come back to the built-in screen.", size_label(w, h))
        } else if external > 0 {
            "A display is plugged in and will take over in a moment.".to_string()
        } else {
            "Plug a TV or monitor into the USB-C port and the picture and the sound move to it.".to_string()
        };
        if let Some(b) = row_.body {
            cx.scene.set_text(cx.t, b, &body);
        } else {
            let col = tp;
            cx.scene.add_text(cx.t, col, super::super::overlay::lay::s().done(), &body, ts(r, ty::SMALL.0, ty::SMALL.1, 400).wrap(), tok::MUTED);
        }
        let labels: Vec<&str> = UI_SCALES.iter().map(|s| scale_label(*s)).collect();
        let cur = UI_SCALES.iter().position(|s| *s == scale_of(cfg));
        rows.choice(cx, nav, p, "ui-size", "UI size", Some("Makes text and buttons bigger on top of the automatic size. Kept after a restart."), &labels, cur, false);
        if let Some(level) = info.brightness {
            let body = if level < 10.0 { "Night dim. Darker to look at, but it does not save battery." } else { "Left and right to change." };
            rows.slider(cx, nav, p, "brightness", "Brightness", Some(body), level, 0.0, 100.0, 5.0, "%", false);
            if let Some(d) = info.dim_idle {
                rows.choice(cx, nav, p, "dim-idle", "Dim after idle", Some("Dims the screen to 30% after 60 seconds with no input in the launcher. Never in a game."), &["Off", "60 s"], Some(d as usize), false);
            }
        } else if info.brightness.is_none() {
            setting_row(cx, p, "Screen", Some("This device has no screen brightness to change."), false);
        }
        build_night(cx, nav, parent, &mut rows, &info.screen.night_light);
        let tv = build_tv(cx, nav, parent, &mut rows, docked, info.screen.tv, info.tv_modes.as_deref());
        DisplaySection { rows, tv }
    }
}

impl SectionView for DisplaySection {
    fn press(&mut self, cx: &mut Cx, nav: &mut Nav, cfg: &mut Config, now: f64) -> SectionOut {
        match self.rows.press(cx, nav, now) {
            Some((sid, Val::Idx(i))) if sid == "ui-size" => {
                cfg.ui_scale = match UI_SCALES[i] {
                    UiScale::Auto => None,
                    UiScale::Large => Some("large".into()),
                    UiScale::Larger => Some("larger".into()),
                };
                SectionOut::Changed
            }
            Some((sid, Val::Idx(i))) if sid == "dim-idle" => SectionOut::Act(format!("display:dim-idle:{}", i == 1)),
            Some((sid, Val::Bool(on))) if sid == "night-light" => SectionOut::Act(format!("display:night-on:{on}")),
            Some((sid, Val::Bool(on))) if sid == "night-schedule" => SectionOut::Act(format!("display:night-schedule:{on}")),
            Some((sid, Val::Idx(i))) if sid == "tv-size" => self.tv.0.get(i).map(|(w, h)| SectionOut::Act(format!("display:tv-size:{w}x{h}"))).unwrap_or(SectionOut::None),
            Some((sid, Val::Idx(i))) if sid == "tv-refresh" => self.tv.1.get(i).map(|r| SectionOut::Act(format!("display:tv-refresh:{r}"))).unwrap_or(SectionOut::None),
            _ => SectionOut::None,
        }
    }

    fn nudge(&mut self, cx: &mut Cx, nav: &mut Nav, _cfg: &mut Config, dir: i32, _now: f64) -> SectionOut {
        match self.rows.nudge(cx, nav, dir) {
            Some((sid, Val::Num(v))) if sid == "brightness" => SectionOut::Act(format!("display:brightness:{v}")),
            Some((sid, Val::Num(v))) if sid == "night-warmth" => SectionOut::Act(format!("display:night-warmth:{v}")),
            Some(_) => SectionOut::None,
            None => {
                if nav.current().is_some_and(|f| self.rows.is_slider(&f.sid)) {
                    SectionOut::None
                } else {
                    SectionOut::Pass
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docked(name: &str, mode: &str) -> DisplayState {
        DisplayState { docked: true, dock_name: Some(name.into()), dock_mode: Some(mode.into()), ..Default::default() }
    }

    #[test]
    fn scale_factor_and_labels() {
        assert_eq!(scale_factor(UiScale::Auto), 1.0);
        assert_eq!(scale_factor(UiScale::Large), 1.15);
        assert_eq!(scale_factor(UiScale::Larger), 1.3);
        assert_eq!(scale_label(UiScale::Auto), "Automatic");
        assert_eq!(next_scale(UiScale::Larger), UiScale::Auto);
        assert_eq!(scale_of(&Config { ui_scale: Some("large".into()), ..Default::default() }), UiScale::Large);
        assert_eq!(scale_of(&Config { ui_scale: Some("huge".into()), ..Default::default() }), UiScale::Auto);
    }

    #[test]
    fn root_size_follows_the_window() {
        assert_eq!(root_px(1280.0, 800.0, UiScale::Auto), 16.0);
        assert_eq!(root_px(1920.0, 1200.0, UiScale::Auto), 24.0);
        assert!((root_px(1920.0, 1080.0, UiScale::Auto) - 21.6).abs() < 1e-4);
        assert!((root_px(3840.0, 2160.0, UiScale::Auto) - 43.2).abs() < 1e-4);
        assert!((root_px(1920.0, 1200.0, UiScale::Large) - 27.6).abs() < 1e-4);
        assert_eq!(size_label(3840.2, 2160.0), "3840x2160");
    }

    #[test]
    fn the_docked_line() {
        assert_eq!(docked_line(None), "");
        assert_eq!(docked_line(Some(&DisplayState::default())), "Built-in screen");
        assert_eq!(docked_line(Some(&docked("LG TV", "3840x2160"))), "Docked to LG TV 3840x2160");
        let d = DisplayState { docked: true, active: Some("HDMI-A-1".into()), ..Default::default() };
        assert_eq!(docked_line(Some(&d)), "Docked to HDMI-A-1");
    }

    #[test]
    fn dock_changes_say_something_only_when_it_matters() {
        let off = DisplayState::default();
        assert_eq!(dock_change(None, &docked("TV", "1080p")), None);
        assert_eq!(dock_change(Some(&off), &docked("TV", "1080p")).as_deref(), Some("Docked to TV 1080p"));
        assert_eq!(dock_change(Some(&docked("TV", "1080p")), &off).as_deref(), Some("Back on the built-in screen"));
        assert_eq!(dock_change(Some(&docked("TV", "1080p")), &docked("TV", "1080p")), None);
        assert_eq!(dock_change(Some(&docked("TV", "1080p")), &docked("TV", "4k")).as_deref(), Some("Docked to TV 4k"));
        assert_eq!(dock_change(Some(&off), &off), None);
    }
}
