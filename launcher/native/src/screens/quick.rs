//! Quick Settings (components/QuickSettings.tsx): the device and nothing else. A right sheet, 27rem,
//! sliding in over a dimmed screen: battery and clock, the four radio tiles, brightness and
//! volume, performance (profile, overlay, More), capture, power, All settings. Left/right move a
//! focused slider; B, Quick or Menu close it.

use crate::act::Act;
use crate::kit::{self, s, tok, GSize, Pb, Track, Ty, Ui, T};
use crate::page::{hash, Acts, Btn, Feedback, Fx, Page};
use rare_i18n::{tr, trf};
use crate::state::{Qs, State, Tab};
use rare_render::focus::hints::Hint;
use rare_render::focus::{Focusable, Group};
use rare_render::scene::Shadow;
use rare_render::view::{key, El, Look, Motion};

fn profiles() -> [(&'static str, &'static str); 3] {
    [("quiet", tr().quick.profile_quiet), ("balanced", tr().quick.profile_balanced), ("performance", tr().quick.profile_performance)]
}
fn huds() -> [(&'static str, &'static str); 4] {
    [("off", tr().common.off), ("fps", "FPS"), ("basic", tr().quick.hud_compact), ("full", tr().quick.hud_full)]
}
const FPS: [u32; 5] = [0, 15, 20, 30, 60];
/// rare-fand's presets as Quick settings names them; Custom opens the curve editor.
fn fans() -> [(&'static str, &'static str); 4] {
    [("quiet", tr().quick.profile_quiet), ("default", tr().quick.profile_balanced), ("cool", tr().quick.fan_max), ("custom", tr().quick.fan_custom)]
}
/// Frame generation multipliers and flow scales (percent).
fn fg_mults() -> [(u8, &'static str); 3] {
    [(1, tr().common.off), (2, "2x"), (3, "3x")]
}
const FG_FLOWS: [(u8, &str); 3] = [(50, "50%"), (75, "75%"), (100, "100%")];

// Local act codes.
const WIFI: u32 = 1;
const BT: u32 = 2;
const AIR: u32 = 3;
const DND: u32 = 4;
const BRIGHT: u32 = 5;
const VOL: u32 = 6;
const MORE: u32 = 7;
const SAVE: u32 = 8;
const BOOST: u32 = 9;
const SLEEP: u32 = 10;
const RESTART: u32 = 11;
const OFF: u32 = 12;
const SETTINGS: u32 = 13;
const SHOT: u32 = 14;
const CLIP: u32 = 15;
const SHARE: u32 = 16;
const TDP: u32 = 17;
const REPLAY: u32 = 18;
const STEAM: u32 = 19;
const KEYBOARD: u32 = 20;
const MOUSE: u32 = 21;
const PROFILE: u32 = 100;
const HUD: u32 = 200;
const FPSC: u32 = 300;
const FAN: u32 = 400;
const FG: u32 = 800;
const FLOW: u32 = 810;
// Upscaling (the gamescope filter, its sharpness, the render size): 600 + row * 10 + chip.
const UP: u32 = 600;
fn up_modes() -> [(rare_system::upscale::Mode, &'static str); 3] {
    [(rare_system::upscale::Mode::Off, tr().common.off), (rare_system::upscale::Mode::Fsr, "FSR"), (rare_system::upscale::Mode::Nis, "NIS")]
}
fn up_sharp() -> [(u32, &'static str); 3] {
    [(4, tr().quick.sharp_soft), (10, tr().quick.sharp_medium), (16, tr().quick.sharp_sharp)]
}
fn up_sizes() -> [(u32, u32, &'static str); 3] {
    [(0, 0, tr().quick.size_native), (1280, 800, "800p"), (960, 600, "600p")]
}
// 500..599: the sound rows (quick/audio.rs). 700..709: Now playing (quick/media.rs).

pub mod audio;
pub mod media;

#[derive(Default)]
pub struct QuickSettings {
    more: bool,
    loaded: bool,
    /// Upscaling as saved (~/.config/rare/upscale.json); applied now and when a game starts.
    up: rare_system::upscale::Upscale,
}

/// A chip that can be greyed out (Sharpness while upscaling is off): it keeps its place.
fn chip_off(ui: &mut Ui, label: &str, on: bool, tag: u32, disabled: bool) -> El {
    let c = chip(ui, label, on && !disabled, tag);
    if disabled {
        c.opacity(0.4).nav(|f| f.disabled(true))
    } else {
        c
    }
}

/// Upscaling rows (UpscaleRows.tsx): filter, sharpness, render size. Same height whatever is picked.
fn upscale_rows(ui: &mut Ui, acts: &mut Acts, u: &rare_system::upscale::Upscale) -> Vec<El> {
    let off = u.mode == rare_system::upscale::Mode::Off;
    let modes: Vec<El> = up_modes().iter().enumerate().map(|(i, (m, l))| chip(ui, l, u.mode == *m, acts.tag(Act::Local(UP + i as u32)))).collect();
    let sharp: Vec<El> = up_sharp().iter().enumerate().map(|(i, (v, l))| chip_off(ui, l, u.sharpness == *v, acts.tag(Act::Local(UP + 10 + i as u32)), off)).collect();
    let sizes: Vec<El> = up_sizes().iter().enumerate().map(|(i, (w, h, l))| chip(ui, l, (u.width, u.height) == (*w, *h) || (*w == 0 && (u.width == 0 || u.height == 0)), acts.tag(Act::Local(UP + 20 + i as u32)))).collect();
    vec![chips(ui, tr().quick.upscaling, "qam-up", modes), chips(ui, tr().quick.sharpness, "qam-up-sharp", sharp), chips(ui, tr().quick.render_size, "qam-up-size", sizes)]
}

/// What frame generation needs before it can run, in a line; None when it can.
pub fn framegen_note(status: &rare_system::gamefx::Status, fg: &rare_system::gamefx::FrameGen) -> Option<&'static str> {
    use rare_system::gamefx::FgStatus;
    match status.framegen {
        FgStatus::NoLayer => Some(tr().quick.fg_no_layer),
        FgStatus::NoDll => Some(tr().quick.fg_no_dll),
        FgStatus::OldBranch => Some(tr().quick.fg_old_branch),
        FgStatus::Ready if fg.on() => Some(tr().quick.fg_ready),
        FgStatus::Ready => None,
    }
}

/// Frame generation for the game in front: Off, 2x, 3x, and the flow scale while it is on.
fn framegen_rows(ui: &mut Ui, acts: &mut Acts, fg: &rare_system::gamefx::FrameGen, status: &rare_system::gamefx::Status) -> Vec<El> {
    use rare_system::gamefx::FgStatus;
    let usable = matches!(status.framegen, FgStatus::Ready | FgStatus::OldBranch);
    let cur = if fg.on() { fg.mult.min(3) } else { 1 };
    let mults: Vec<El> = fg_mults().iter().enumerate().map(|(i, (m, l))| chip_off(ui, l, cur == *m, acts.tag(Act::Local(FG + i as u32)), !usable)).collect();
    let mut out = vec![chips(ui, tr().quick.frame_generation, "qam-fg", mults)];
    if fg.on() && usable {
        let flows: Vec<El> = FG_FLOWS.iter().enumerate().map(|(i, (f, l))| chip(ui, l, fg.flow == *f, acts.tag(Act::Local(FLOW + i as u32)))).collect();
        out.push(chips(ui, tr().quick.flow_scale, "qam-flow", flows));
    }
    if let Some(n) = framegen_note(status, fg) {
        out.push(ui.text(n, T::new(Ty::Meta).c(tok::MUTED).wrap()));
    }
    out
}

/// "3 h 10 min left", "Full in 40 min" (QuickSettings.tsx batteryTimeLine).
pub fn battery_line(percent: u8, charging: bool, minutes: Option<u32>) -> Option<String> {
    if charging && percent >= 100 {
        return Some(tr().quick.battery_full.into());
    }
    let m = minutes?;
    let (h, mm) = (m / 60, m % 60);
    let t = if h > 0 {
        if mm > 0 {
            trf!(tr().quick.hours_minutes, h = h, m = mm)
        } else {
            trf!(tr().quick.hours, h = h)
        }
    } else {
        trf!(tr().quick.minutes, m = mm.max(1))
    };
    Some(if charging { trf!(tr().quick.full_in, time = t) } else { trf!(tr().quick.time_left, time = t) })
}

fn load(fx: &mut Fx) {
    fx.spawn(|b| {
        let qs = Qs { perf: b.perf(), levels: b.levels(), radios: b.radios(), hud: b.hud(), game: b.game_power(), dnd: crate::social::dnd(), fx: b.game_fx() };
        Box::new(move |st: &mut State, _| {
            st.game_perf.perf = qs.perf.clone();
            st.game_perf.power = qs.game.clone();
            st.qs = qs;
            st.ver.qs += 1;
        })
    });
    audio::load(fx);
}

/// A chip in a row of choices (h-[2.25rem] min-w-[3.3rem] rounded-chip, white when chosen).
fn chip(ui: &mut Ui, label: &str, on: bool, tag: u32) -> El {
    let b = ui.bx(s().row().items_center().justify_center().h(2.25).min_w(3.3).px(0.8)).bg(if on { tok::FG } else { tok::L2 }).radius(ui.r(0.25)).child(ui.text_s(s().shrink().min0(), label, T::new(Ty::Small).w(500).tab().c(if on { tok::BG } else { tok::FG }).fit()).focus_color(tok::BG));
    let fl = b.look.clone().bg(tok::FG).scale(1.04);
    kit::nav(b.focus_look(fl), tag, None, None, false).key(key(label))
}

/// A row of chips with its label on the left.
fn chips(ui: &mut Ui, label: &str, group: &str, items: Vec<El>) -> El {
    ui.bx(s().row().items_center().gap(0.5)).group(Group::new(group, [0.0; 4])).child(ui.text_s(s().mr_auto().shrink(), label, T::new(Ty::Small).c(tok::fg(0.85)).wrap())).kids(items)
}

/// Quick settings' slider: label, value, a track with a white fill and knob.
fn slider(ui: &mut Ui, label: &str, icon: &str, value: f32, min: f32, max: f32, suffix: &str, tag: u32, def: bool) -> El {
    let pct = ((value - min) / (max - min)).clamp(0.0, 1.0);
    let head = ui
        .bx(s().row().items_center().gap(0.55).mb(0.6))
        .child(kit::icon(ui, icon, 1.0, tok::ICON2))
        .child(ui.text(label, T::new(Ty::Small).c(tok::fg(0.85))))
        .child(ui.text_s(s().ml_auto(), format!("{}{suffix}", value.round()), T::new(Ty::Small).w(500).tab()));
    let track = ui
        .bx(s().h(0.35))
        .bg(tok::L4)
        .radius(ui.r(0.175))
        .child(ui.bx(s().abs(Some(0.0), None, Some(0.0), Some(0.0)).wp(pct)).bg(tok::FG).radius(ui.r(0.175)))
        .child(ui.bx(s().abs_pct(Some(0.5), None, None, Some(pct)).size(1.05)).look(Look::default().bg(tok::FG).radius(ui.r(0.525)).translate(-ui.r(0.525), -ui.r(0.525))));
    let b = ui.bx(s().col().px(0.95).py(0.8)).bg(tok::L2).radius(ui.r(0.5)).child(head).child(track);
    kit::row_look(b)
        .nav(move |f| {
            let f = f.tag(tag).hints(Some(""), None, None);
            if def {
                f.default()
            } else {
                f
            }
        })
        .key(key(label))
}

/// A row with a switch (Use for this game, CPU boost).
fn switch_row(ui: &mut Ui, title: &str, sub: &str, on: bool, tag: u32) -> El {
    let b = ui
        .bx(s().row().items_center().gap(0.75).px(0.95).py(0.8))
        .bg(tok::L2)
        .radius(ui.r(0.5))
        .child(ui.bx(s().col().grow()).child(ui.text(title, T::new(Ty::Small).c(tok::fg(0.85)).trunc())).child(ui.text(sub, T::new(Ty::Label).c(tok::MUTED).trunc())))
        .child(kit::toggle(ui, on));
    kit::nav(kit::row_look(b), tag, Some(if on { tr().common.turn_off } else { tr().common.turn_on }), None, false).key(key(title))
}

/// A plain button tile (QBtn): icon and label on layer 02.
fn qbtn(ui: &mut Ui, icon: &str, label: &str, tag: u32, hint: Option<&str>, span2: bool) -> El {
    let mut b = ui.bx(s().row().items_center().gap(0.6).h(2.9).px(0.9).min0()).bg(tok::L2).radius(ui.r(0.5)).child(kit::icon(ui, icon, 1.15, tok::FG)).child(ui.text_s(s().shrink().min0(), label, T::new(Ty::Small).w(500).fit()));
    if span2 {
        b = b.style(|st| st.grid_column = rare_render::taffy::prelude::span(2));
    }
    kit::nav(kit::row_look(b), tag, hint, None, false).key(key(label))
}

/// A capture tile with its state on the right ("Instant replay  On").
fn qstate(ui: &mut Ui, icon: &str, label: &str, state: &str, tag: u32, hint: Option<&str>) -> El {
    let b = ui
        .bx(s().row().items_center().gap(0.6).h(2.9).px(0.9))
        .bg(tok::L2)
        .radius(ui.r(0.5))
        .child(kit::icon(ui, icon, 1.15, tok::FG))
        .child(ui.text_s(s().shrink().min0(), label, T::new(Ty::Small).w(500).trunc()))
        .child(ui.text_s(s().ml_auto(), state, T::new(Ty::Small).c(tok::MUTED)));
    kit::nav(kit::row_look(b), tag, hint, None, false).key(key(label))
}

/// The game the panel is for, when it is one the controller can drive as a mouse: the one
/// running now (as for "Use for this game" above; the screen is Rare's while the panel is up),
/// not an app, a web game or a stream (those have their own input).
fn mouse_game() -> Option<String> {
    let run = rare_system::power::current_game()?;
    let own = ["app:", "web:", "stream:", "cloud:"].iter().any(|p| run.starts_with(p));
    (!own).then_some(run)
}

fn section_label(ui: &mut Ui, label: &str) -> El {
    ui.text(label, T::new(Ty::Label).w(500).c(tok::MUTED))
}

impl Page for QuickSettings {
    fn stamp(&self, st: &State) -> u64 {
        hash((st.ver.qs, st.ver.device, self.more, self.loaded, (self.up.mode as u8, self.up.sharpness, self.up.width, self.up.height), rare_system::media::version(), crate::features::capture::version(), crate::data::gameset::version()))
    }

    fn view(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El {
        let q = &st.qs;
        let fg80 = tok::fg(0.8);
        // Header: battery and time left, the clock.
        let mut right = ui.bx(s().row().items_center().gap(0.9).ml_auto().shrink().min0());
        if let Some(b) = st.device.as_ref().and_then(|d| d.battery.as_ref()) {
            let mut bat = ui.bx(s().row().items_center().gap(0.35).shrink().min0()).child(kit::icon(ui, kit::icon::battery_name(b.percent, b.charging), 1.3, fg80)).child(ui.text(format!("{}%", b.percent), T::new(Ty::Small).tab().c(fg80)));
            if let Some(l) = battery_line(b.percent, b.charging, b.minutes) {
                bat = bat.child(ui.text_s(s().shrink().min0(), l, T::new(Ty::Small).c(tok::MUTED).fit()));
            }
            right = right.child(bat);
        }
        right = right.child(ui.text(crate::screens::shell::clock_text(), T::new(Ty::Body).w(500).tab()));
        let header = ui.bx(s().row().items_center().gap(0.75)).child(ui.text(tr().common.quick_settings, T::new(Ty::Row).w(600))).child(right);

        // The four tiles.
        let r = q.radios;
        let tile = |ui: &mut Ui, acts: &mut Acts, icon: &str, label: &str, line: String, on: bool, disabled: bool, code: u32, details: bool| -> El {
            let tag = acts.tag(Act::Local(code));
            let mut b = ui
                .bx(s().col().items_center().justify_center().gap(0.25).h(5.9).px(0.3).min0())
                .bg(if on { tok::FG } else { tok::L2 })
                .radius(ui.r(0.5))
                .child(kit::focus_icon(ui, icon, 1.4, false, if on { tok::BG } else { tok::FG }, tok::BG))
                .child(ui.text_s(s().wp(1.0).min0(), label, T::new(Ty::Small).w(600).c(if on { tok::BG } else { tok::FG }).center().fit()).focus_color(tok::BG))
                .child(ui.text(line, T::new(Ty::Label).c(if on { tok::BG.alpha(0.7) } else { tok::MUTED }).trunc()).focus_color(tok::BG.alpha(0.7)));
            if disabled {
                b = b.opacity(0.4);
            }
            let fl = b.look.clone().bg(tok::FG).scale(1.04);
            let x = details.then_some(tr().common.details);
            // One focusable: a second `.nav` would replace this one's tag (every tile acting as the first).
            let a = if on { tr().common.turn_off } else { tr().common.turn_on };
            b.focus_look(fl).nav(move |f| f.tag(tag).hints(Some(a), x, None).disabled(disabled)).key(key(label))
        };
        let wifi_line = match r.and_then(|r| r.wifi) {
            Some(false) => tr().common.off.to_string(),
            _ if crate::features::portal::needs_sign_in() => tr().quick.sign_in_needed.to_string(),
            _ => st.device.as_ref().and_then(|d| d.wifi.clone()).filter(|w| !w.is_empty()).unwrap_or_else(|| tr().common.on.into()),
        };
        // Airplane, then Wi-Fi straight above where the panel opens (Brightness: Up lands on it),
        // Bluetooth after it.
        let tiles = vec![
            tile(ui, acts, "plane", tr().quick.airplane, if r.is_some_and(|r| r.airplane) { tr().common.on.into() } else { tr().common.off.into() }, r.is_some_and(|r| r.airplane), r.is_none(), AIR, false),
            tile(ui, acts, "wifi", "Wi-Fi", wifi_line, r.and_then(|r| r.wifi) == Some(true), r.is_none_or(|r| r.wifi.is_none()), WIFI, true),
            tile(ui, acts, "bluetooth", "Bluetooth", if r.and_then(|r| r.bluetooth) == Some(false) { tr().common.off.into() } else { tr().common.on.into() }, r.and_then(|r| r.bluetooth) == Some(true), r.is_none_or(|r| r.bluetooth.is_none()), BT, true),
            tile(ui, acts, "bell-off", tr().quick.do_not_disturb, if q.dnd { tr().common.on.into() } else { tr().common.off.into() }, q.dnd, false, DND, false),
        ];
        let tiles = ui.bx(s().grid(&[Track::Fr(1.0), Track::Fr(1.0), Track::Fr(1.0), Track::Fr(1.0)]).gap(0.5)).group(Group::new("qam-tiles", [0.0; 4])).kids(tiles);

        let mut levels = ui.bx(s().col().gap(0.6));
        if let Some(b) = q.levels.and_then(|l| l.brightness) {
            let tag = acts.tag(Act::Local(BRIGHT));
            levels = levels.child(slider(ui, tr().quick.brightness, "sun", b, 0.0, 100.0, "%", tag, true));
            if b < 10.0 {
                levels = levels.child(ui.text(tr().quick.night_dim, T::new(Ty::Meta).c(tok::MUTED).wrap()));
            }
        }
        if let Some(v) = q.levels.and_then(|l| l.volume) {
            let tag = acts.tag(Act::Local(VOL));
            levels = levels.child(slider(ui, tr().quick.volume, "volume-2", v, 0.0, 100.0, "%", tag, false));
        }
        if let Some(a) = audio::view(ui, acts) {
            levels = levels.child(a);
        }
        if let Some(m) = media::view(ui, acts) {
            levels = levels.child(m);
        }

        // Performance.
        let mut perf = ui.bx(s().col().gap(0.6)).child(section_label(ui, tr().quick.performance));
        let profile = q.perf.as_ref().map(|p| p.profile.clone()).unwrap_or_default();
        let mut profs = ui.bx(s().grid(&[Track::Fr(1.0), Track::Fr(1.0), Track::Fr(1.0)]).gap(0.5)).group(Group::new("qam-profile", [0.0; 4]));
        for (i, (id, label)) in profiles().iter().enumerate() {
            let on = profile == *id;
            let watts = q.perf.as_ref().map(|p| p.watts[i]).unwrap_or([10.0, 15.0, 25.0][i]);
            let tag = acts.tag(Act::Local(PROFILE + i as u32));
            // The chosen profile carries a check, so it still reads as chosen under the focus ring.
            let mut name = ui.bx(s().row().items_center().gap(0.3)).child(ui.text(*label, T::new(Ty::Small).w(600).c(if on { tok::BG } else { tok::FG })).focus_color(tok::BG));
            if on {
                name = name.child(ui.bx(s().wh(0.9, 0.9)).child(kit::focus_icon(ui, "check", 0.9, false, tok::BG, tok::BG)));
            }
            let b = ui
                .bx(s().col().items_start().px(0.85).py(0.7))
                .bg(if on { tok::FG } else { tok::L2 })
                .radius(ui.r(0.5))
                .child(name)
                .child(ui.text(crate::kit::format::watts(watts), T::new(Ty::Label).tab().c(if on { tok::BG.alpha(0.7) } else { tok::MUTED })).focus_color(tok::BG.alpha(0.7)));
            let fl = b.look.clone().bg(tok::FG).scale(1.04);
            profs = profs.child(kit::nav(b.focus_look(fl), tag, None, None, false).key(key(id)));
        }
        perf = perf.child(profs);
        let huds: Vec<El> = huds().iter().enumerate().map(|(i, (id, l))| chip(ui, l, q.hud == *id, acts.tag(Act::Local(HUD + i as u32)))).collect();
        perf = perf.child(chips(ui, tr().quick.performance_overlay, "qam-hud", huds));
        if let Some((fg, status)) = &q.fx {
            perf = perf.kids(framegen_rows(ui, acts, fg, status));
        }
        if let Some((gid, saved)) = &q.game {
            let name = st.game(gid).map(|g| g.title.clone()).unwrap_or_else(|| tr().quick.this_game.into());
            let tag = acts.tag(Act::Local(SAVE));
            perf = perf.child(switch_row(ui, &trf!(tr().quick.use_for_game, name = name), if *saved { tr().quick.use_for_saved } else { tr().quick.use_for_default }, *saved, tag));
        }
        let more_tag = acts.tag(Act::Local(MORE));
        let chev = kit::icon(ui, "chevron-down", 1.0, tok::FG);
        let chev = if self.more { chev.look(Look::default().scale(-1.0)) } else { chev };
        let more = ui.bx(s().row().items_center().justify_between().h(2.4).px(0.95)).bg(tok::L2).radius(ui.r(0.5)).child(ui.text(tr().common.more, T::new(Ty::Small).w(500)).focus_color(tok::BG)).child(chev);
        let fl = more.look.clone().bg(tok::FG).scale(1.04);
        perf = perf.child(kit::nav(more.focus_look(fl), more_tag, Some(if self.more { tr().quick.show_less } else { tr().common.more }), None, false));
        if self.more {
            let mut m = ui.bx(s().col().gap(0.6));
            if let Some(p) = &q.perf {
                let tag = acts.tag(Act::Local(TDP));
                m = m.child(slider(ui, tr().quick.power_limit, "zap", p.tdp.unwrap_or(15.0), p.tdp_min, p.tdp_max, tr().common.watts.trim_start_matches("{n}"), tag, false));
                if let Some(b) = p.cpu_boost {
                    let tag = acts.tag(Act::Local(BOOST));
                    m = m.child(switch_row(ui, tr().quick.cpu_boost, tr().quick.cpu_boost_sub, b, tag));
                }
                if let Some(f) = &p.fan {
                    let fans: Vec<El> = fans().iter().enumerate().map(|(i, (id, l))| chip(ui, l, f == id, acts.tag(Act::Local(FAN + i as u32)))).collect();
                    m = m.child(chips(ui, tr().quick.fan, "qam-fan", fans));
                }
                let fps: Vec<El> = FPS.iter().enumerate().map(|(i, f)| chip(ui, &if *f == 0 { tr().common.off.to_string() } else { f.to_string() }, p.fps_limit.unwrap_or(0) == *f, acts.tag(Act::Local(FPSC + i as u32)))).collect();
                m = m.child(chips(ui, tr().quick.frame_limit, "qam-fps", fps));
            }
            m = m.kids(upscale_rows(ui, acts, &self.up));
            perf = perf.child(m);
        }
        if let Some(n) = q.perf.as_ref().map(|p| p.note.clone()).filter(|n| !n.is_empty()) {
            perf = perf.child(ui.text(n, T::new(Ty::Meta).c(tok::MUTED).wrap()));
        }

        let (replay_on, replay_secs) = if std::env::var_os("RR_FIXTURE").is_some() { (true, 30) } else { (crate::features::capture::replay_on(), crate::features::capture::replay_seconds()) };
        let capture = ui
            .bx(s().col().gap(0.6))
            .child(section_label(ui, tr().quick.capture))
            .child(
                ui.bx(s().grid(&[Track::Fr(1.0), Track::Fr(1.0)]).gap(0.5))
                    .group(Group::new("qam-share", [0.0; 4]))
                    .child(qbtn(ui, "camera", tr().quick.screenshot, acts.tag(Act::Local(SHOT)), Some(tr().quick.take_screenshot), false))
                    .child(qbtn(ui, "video", &crate::features::capture::save_label(replay_secs), acts.tag(Act::Local(CLIP)), Some(tr().quick.save_clip), false))
                    .child(qstate(ui, "history", tr().quick.instant_replay, if replay_on { tr().common.on } else { tr().common.off }, acts.tag(Act::Local(REPLAY)), Some(if replay_on { tr().common.turn_off } else { tr().common.turn_on })))
                    .child(qbtn(ui, "images", tr().quick.open_share, acts.tag(Act::Local(SHARE)), Some(tr().quick.open_share), false)),
            );
        let power = ui
            .bx(s().col().gap(0.6))
            .child(section_label(ui, tr().quick.power))
            .child(
                ui.bx(s().grid(&[Track::Fr(1.0), Track::Fr(1.0), Track::Fr(1.0)]).gap(0.5))
                    .group(Group::new("qam-power", [0.0; 4]))
                    .child(qbtn(ui, "moon", tr().quick.sleep, acts.tag(Act::Local(SLEEP)), None, false))
                    .child(qbtn(ui, "rotate-ccw", tr().common.restart, acts.tag(Act::Local(RESTART)), None, false))
                    .child(qbtn(ui, "power", tr().common.turn_off, acts.tag(Act::Local(OFF)), None, false)),
            );
        let mut all = ui
            .bx(s().grid(&[Track::Fr(1.0), Track::Fr(1.0)]).gap(0.5).mt_auto())
            .group(Group::new("qam-end", [0.0; 4]))
            .child(qbtn(ui, "keyboard", tr().quick.keyboard, acts.tag(Act::Local(KEYBOARD)), Some(tr().quick.open_keyboard), true));
        // Over a game: the controller as its mouse or not, without closing it (the game page
        // hides its settings while it runs).
        if let Some(game) = mouse_game() {
            let on = crate::data::gameset::get(&game).mouse;
            let state = if on { tr().game.pad_as_mouse } else { tr().game.pad_as_gamepad };
            let row = qstate(ui, "mouse", tr().quick.pad_in_game, state, acts.tag(Act::Local(MOUSE)), Some(if on { tr().common.turn_off } else { tr().common.turn_on }));
            all = all.child(row.style(|st| st.grid_column = rare_render::taffy::prelude::span(2)));
        }
        let all = all
            .child(qbtn(ui, "settings", tr().quick.all_settings, acts.tag(Act::Local(SETTINGS)), None, false))
            .child(qbtn(ui, "gamepad-2", tr().quick.open_steam, acts.tag(Act::Local(STEAM)), Some(tr().quick.open_steam), false));
        let small = T::new(Ty::Small).c(tok::fg(0.7));
        // What the buttons do here, always: Y opens Steam from anywhere in the panel.
        let foot = ui
            .bx(s().row().items_center().justify_end().gap(1.1))
            .child(kit::hint(ui, Pb::A, tr().common.select, small))
            .child(kit::hint(ui, Pb::Y, tr().quick.open_steam, small))
            .child(kit::hint(ui, Pb::B, tr().common.close, small));
        let _ = GSize::Sm;

        // The panel scrolls (overflow-y-auto, following focus); the button hints stay put under it.
        let body = ui
            .bx(s().col().gap(1.1).grow().min0().p(1.4).pb(0.6))
            .scrolls_y(rare_render::view::key("qam-aside"))
            .child(header)
            .child(tiles)
            .child(levels)
            .child(perf)
            .child(capture)
            .child(power)
            .child(all);
        let aside = ui
            .bx(s().col().w(27.0).hp(1.0))
            .bg(tok::L1)
            .border(1.0, tok::BORDER_LIGHT)
            .shadow(&[Shadow::rem(-2.0, 0.0, 4.0, 0.45, ui.rem)])
            .enter(Motion::SLIDE_IN_RIGHT)
            .child(body)
            .child(ui.bx(s().col().px(1.4).py(0.9)).child(ui.bx(s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h_px(1.0)).bg(tok::HAIR)).child(foot));
        ui.bx(s().fill().row().justify_end()).bg(tok::BLACK.alpha(0.4)).child(aside)
    }

    fn pad(&mut self, st: &mut State, fx: &mut Fx, b: Btn, cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        if !self.loaded {
            self.loaded = true;
            load(fx);
        }
        match b {
            Btn::B | Btn::Qam | Btn::Start => {
                st.close();
                true
            }
            Btn::X => {
                // X on a radio tile: its page in Settings (the networks, the Bluetooth devices).
                let section = match cur.and_then(|c| c.1) {
                    Some(Act::Local(WIFI)) => Some(crate::screens::settings::sections::SectionId::Network),
                    Some(Act::Local(BT)) => Some(crate::screens::settings::sections::SectionId::Devices),
                    _ => None,
                };
                if let Some(sec) = section {
                    st.close();
                    crate::screens::settings::open_section_next(sec);
                    st.set_tab(Tab::Settings, false);
                    fx.fb(Feedback::Open);
                }
                true
            }
            Btn::Y => {
                // Y: Steam, from anywhere in the panel (over a game too).
                st.close();
                fx.run(Act::Legacy("rare:steam".into()));
                true
            }
            Btn::Left | Btn::Right => {
                let step = if b == Btn::Left { -5.0 } else { 5.0 };
                match cur.and_then(|c| c.1) {
                    Some(Act::Local(BRIGHT)) => {
                        let v = st.qs.levels.and_then(|l| l.brightness).unwrap_or(0.0);
                        let n = (v + step).clamp(0.0, 100.0);
                        if let Some(l) = st.qs.levels.as_mut() {
                            l.brightness = Some(n);
                        }
                        st.ver.qs += 1;
                        fx.spawn(move |be| {
                            let _ = be.set_levels(Some(n), None);
                            Box::new(|_: &mut State, _| {})
                        });
                        true
                    }
                    Some(Act::Local(VOL)) => {
                        let v = st.qs.levels.and_then(|l| l.volume).unwrap_or(0.0);
                        let n = (v + step).clamp(0.0, 100.0);
                        if let Some(l) = st.qs.levels.as_mut() {
                            l.volume = Some(n);
                        }
                        st.ver.qs += 1;
                        fx.spawn(move |be| {
                            let _ = be.set_levels(None, Some(n));
                            Box::new(|_: &mut State, _| {})
                        });
                        true
                    }
                    Some(Act::Local(TDP)) => {
                        let Some(p) = st.qs.perf.as_mut() else { return true };
                        let n = (p.tdp.unwrap_or(15.0) + if b == Btn::Left { -1.0 } else { 1.0 }).clamp(p.tdp_min, p.tdp_max);
                        p.tdp = Some(n);
                        p.profile = "custom".into();
                        st.ver.qs += 1;
                        fx.spawn(move |be| {
                            let r = be.set_perf("custom", Some(n), None);
                            Box::new(move |st: &mut State, now| {
                                if let Err(e) = r {
                                    st.toast(e, crate::state::Tone::Error, now);
                                }
                            })
                        });
                        true
                    }
                    Some(Act::Local(c)) if audio::owns(*c) => audio::nudge(st, fx, *c, step as f32),
                    // The performance modes work like a switch: moving along them picks the mode
                    // focus lands on (focus moves too).
                    Some(Act::Local(c)) if (PROFILE..PROFILE + 3).contains(c) => {
                        let i = (c - PROFILE) as i32 + if b == Btn::Left { -1 } else { 1 };
                        if (0..3).contains(&i) {
                            fx.run(Act::Local(PROFILE + i as u32));
                        }
                        false
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    fn is_slider(&self, cur: (&Focusable, Option<&Act>)) -> bool {
        matches!(cur.1, Some(Act::Local(BRIGHT | VOL | TDP | audio::LEVEL)))
    }

    fn slide(&mut self, st: &mut State, fx: &mut Fx, cur: (&Focusable, Option<&Act>), frac: f32) -> bool {
        let Some(Act::Local(code)) = cur.1 else { return false };
        let pct = (frac.clamp(0.0, 1.0) * 100.0).round();
        match *code {
            BRIGHT | VOL => {
                let bright = *code == BRIGHT;
                let Some(l) = st.qs.levels.as_mut() else { return true };
                let slot = if bright { &mut l.brightness } else { &mut l.volume };
                if *slot == Some(pct) {
                    return true;
                }
                *slot = Some(pct);
                st.ver.qs += 1;
                fx.spawn(move |be| {
                    let _ = if bright { be.set_levels(Some(pct), None) } else { be.set_levels(None, Some(pct)) };
                    Box::new(|_: &mut State, _| {})
                });
                true
            }
            TDP => {
                let Some(p) = st.qs.perf.as_mut() else { return true };
                let n = (p.tdp_min + frac.clamp(0.0, 1.0) * (p.tdp_max - p.tdp_min)).round().clamp(p.tdp_min, p.tdp_max);
                if p.tdp == Some(n) {
                    return true;
                }
                p.tdp = Some(n);
                p.profile = "custom".into();
                st.ver.qs += 1;
                fx.spawn(move |be| {
                    let r = be.set_perf("custom", Some(n), None);
                    Box::new(move |st: &mut State, now| {
                        if let Err(e) = r {
                            st.toast(e, crate::state::Tone::Error, now);
                        }
                    })
                });
                true
            }
            audio::LEVEL => match audio::mic_level() {
                Some(v) if (pct - v).abs() >= 1.0 => audio::nudge(st, fx, audio::LEVEL, pct - v),
                _ => true,
            },
            _ => false,
        }
    }

    fn tick(&mut self, st: &mut State, fx: &mut Fx, _now: f64) -> Option<f64> {
        if !self.loaded {
            self.loaded = true;
            self.up = rare_system::upscale::saved();
            let dnd = crate::social::dnd();
            if st.qs.dnd != dnd {
                st.qs.dnd = dnd;
                st.ver.qs += 1;
            }
            load(fx);
        }
        // Now playing reads every second while the sheet is up.
        Some(_now + 1.0)
    }

    fn act(&mut self, st: &mut State, fx: &mut Fx, code: u32) {
        let radio = |fx: &mut Fx, kind: &'static str, on: bool| {
            fx.fb(Feedback::Toggle);
            fx.spawn(move |b| {
                let r = b.set_radio(kind, on);
                Box::new(move |st: &mut State, now| match r {
                    Ok(r) => {
                        st.qs.radios = Some(r);
                        st.ver.qs += 1;
                    }
                    Err(e) => st.toast(e, crate::state::Tone::Error, now),
                })
            });
        };
        let r = st.qs.radios;
        match code {
            WIFI => radio(fx, "wifi", !r.and_then(|r| r.wifi).unwrap_or(false)),
            BT => radio(fx, "bluetooth", !r.and_then(|r| r.bluetooth).unwrap_or(false)),
            AIR => radio(fx, "airplane", !r.is_some_and(|r| r.airplane)),
            DND => {
                fx.fb(Feedback::Toggle);
                st.qs.dnd = !st.qs.dnd;
                st.ver.qs += 1;
                // The same switch as Settings > Notifications, saved with the notice prefs.
                crate::social::set_dnd(st.qs.dnd);
            }
            // Capture: the app closes Quick settings first so the shot is of the game or the screen
            // under it, not of this panel.
            SHOT => fx.run(Act::Legacy("qs:screenshot".into())),
            // The keyboard over the game or app under the panel (or the launcher's own); the app
            // closes the panel first so the app has the screen and focus.
            KEYBOARD => fx.run(Act::Legacy("qs:keyboard".into())),
            MOUSE => {
                let Some(game) = mouse_game() else { return };
                let mut on = false;
                crate::data::gameset::update(&game, |s| {
                    s.mouse = !s.mouse;
                    on = s.mouse;
                });
                rare_launch::apps::front::refresh_pointer();
                fx.fb(Feedback::Toggle);
                st.toast(if on { tr().game.pad_mouse_on } else { tr().game.pad_mouse_off }, crate::state::Tone::Info, rare_render::clock::secs());
            }
            CLIP => fx.run(Act::Legacy("qs:clip".into())),
            REPLAY => {
                fx.fb(Feedback::Toggle);
                let msg = crate::features::capture::set_replay(!crate::features::capture::replay_on());
                st.toast(msg, crate::state::Tone::Info, rare_render::clock::secs());
            }
            MORE => self.more = !self.more,
            c if (UP..UP + 30).contains(&c) => {
                let (row, i) = ((c - UP) / 10, ((c - UP) % 10) as usize);
                let mut u = self.up;
                match row {
                    0 => u.mode = up_modes()[i.min(2)].0,
                    1 => u.sharpness = up_sharp()[i.min(2)].0,
                    _ => (u.width, u.height) = { let z = up_sizes()[i.min(2)]; (z.0, z.1) },
                }
                if u == self.up {
                    return;
                }
                self.up = u;
                fx.fb(Feedback::Toggle);
                fx.spawn(move |_| {
                    let r = rare_system::upscale::upscale_set(&u);
                    Box::new(move |st: &mut State, now| {
                        if let Err(e) = r {
                            log::info!("upscale: {e}");
                            st.toast(tr().quick.upscale_saved, crate::state::Tone::Info, now);
                        }
                    })
                });
            }
            SETTINGS => {
                st.close();
                st.set_tab(Tab::Settings, false);
            }
            // Steam's big-screen UI (over a game too); the Rare button comes back.
            STEAM => {
                st.close();
                fx.run(Act::Legacy("rare:steam".into()));
            }
            SLEEP | RESTART | OFF => {
                let a = match code {
                    SLEEP => "suspend",
                    RESTART => "reboot",
                    _ => "poweroff",
                };
                fx.spawn(move |b| {
                    let r = b.power(a);
                    Box::new(move |st: &mut State, now| {
                        if let Err(e) = r {
                            st.toast(e, crate::state::Tone::Error, now);
                        }
                    })
                });
            }
            SHARE => st.open(Some(crate::state::Overlay::Other("share"))),
            c if (PROFILE..PROFILE + 3).contains(&c) => {
                let id = profiles()[(c - PROFILE) as usize].0;
                if st.qs.perf.as_ref().is_some_and(|p| p.profile == id) {
                    st.toast(trf!(tr().quick.profile_is_on, name = profiles()[(c - PROFILE) as usize].1), crate::state::Tone::Info, rare_render::clock::secs());
                    return;
                }
                // The chosen tile moves now; the reading after the change confirms it.
                fx.fb(Feedback::Toggle);
                if let Some(p) = st.qs.perf.as_mut() {
                    p.profile = id.into();
                    st.ver.qs += 1;
                }
                fx.spawn(move |b| {
                    let r = b.set_perf(id, None, None);
                    let p = b.perf();
                    Box::new(move |st: &mut State, now| {
                        if let Err(e) = r {
                            st.toast(e, crate::state::Tone::Error, now);
                        }
                        st.qs.perf = p;
                        st.game_perf.perf = st.qs.perf.clone();
                        st.ver.qs += 1;
                    })
                });
            }
            c if (HUD..HUD + 4).contains(&c) => {
                let id = huds()[(c - HUD) as usize].0;
                st.qs.hud = id.into();
                st.ver.qs += 1;
                fx.spawn(move |b| {
                    let r = b.set_hud(id);
                    Box::new(move |st: &mut State, now| {
                        if let Err(e) = r {
                            st.toast(crate::screens::settings::live::failed(tr().quick.err_overlay, &e, None), crate::state::Tone::Error, now);
                        }
                    })
                });
            }
            c if (FPSC..FPSC + 5).contains(&c) => {
                let f = FPS[(c - FPSC) as usize];
                let profile = st.qs.perf.as_ref().map(|p| p.profile.clone()).unwrap_or_else(|| "balanced".into());
                fx.spawn(move |b| {
                    let r = b.set_perf(&profile, None, Some(f));
                    let p = b.perf();
                    Box::new(move |st: &mut State, now| {
                        if let Err(e) = r {
                            st.toast(crate::screens::settings::live::failed(tr().quick.err_frame_limit, &e, None), crate::state::Tone::Error, now);
                        }
                        st.qs.perf = p;
                        st.game_perf.perf = st.qs.perf.clone();
                        st.ver.qs += 1;
                    })
                });
            }
            c if c == FAN + 3 => st.open(Some(crate::state::Overlay::Other(crate::features::fancurve::KIND))),
            c if (FG..FG + 3).contains(&c) || (FLOW..FLOW + 3).contains(&c) => {
                let Some((fg, status)) = st.qs.fx.clone() else { return };
                if !matches!(status.framegen, rare_system::gamefx::FgStatus::Ready | rare_system::gamefx::FgStatus::OldBranch) {
                    return;
                }
                let mut next = fg;
                if c < FLOW {
                    next.mult = fg_mults()[(c - FG) as usize].0;
                } else {
                    next.flow = FG_FLOWS[(c - FLOW) as usize].0;
                }
                if next == fg {
                    return;
                }
                fx.fb(Feedback::Toggle);
                st.qs.fx = Some((next, status));
                st.ver.qs += 1;
                fx.spawn(move |b| {
                    let r = b.set_framegen(next);
                    let g = b.game_power();
                    Box::new(move |st: &mut State, now| {
                        if let Err(e) = r {
                            log::warn!("framegen: {e}");
                            st.toast(tr().quick.err_framegen, crate::state::Tone::Error, now);
                        }
                        st.qs.game = g;
                        st.ver.qs += 1;
                    })
                });
            }
            c if (FAN..FAN + 3).contains(&c) => {
                let m = fans()[(c - FAN) as usize].0;
                fx.spawn(move |b| {
                    let r = b.set_fan(m);
                    let p = b.perf();
                    Box::new(move |st: &mut State, now| {
                        if let Err(e) = r {
                            st.toast(crate::screens::settings::live::failed(tr().quick.err_fan, &e, None), crate::state::Tone::Error, now);
                        }
                        st.qs.perf = p;
                        st.game_perf.perf = st.qs.perf.clone();
                        st.ver.qs += 1;
                    })
                });
            }
            BOOST => {
                let on = !st.qs.perf.as_ref().and_then(|p| p.cpu_boost).unwrap_or(false);
                fx.spawn(move |b| {
                    let r = b.set_cpu_boost(on);
                    let p = b.perf();
                    Box::new(move |st: &mut State, now| {
                        if let Err(e) = r {
                            st.toast(crate::screens::settings::live::failed(tr().quick.err_cpu_boost, &e, None), crate::state::Tone::Error, now);
                        }
                        st.qs.perf = p;
                        st.game_perf.perf = st.qs.perf.clone();
                        st.ver.qs += 1;
                    })
                });
            }
            SAVE => {
                let save = !st.qs.game.as_ref().is_some_and(|g| g.1);
                fx.spawn(move |b| {
                    let r = b.save_game_power(save);
                    let g = b.game_power();
                    Box::new(move |st: &mut State, now| {
                        if let Err(e) = r {
                            st.toast(crate::screens::settings::live::failed(tr().quick.err_game_settings, &e, None), crate::state::Tone::Error, now);
                        }
                        st.qs.game = g;
                        st.game_perf.power = st.qs.game.clone();
                        st.ver.qs += 1;
                    })
                });
            }
            c if audio::owns(c) => audio::act(st, fx, c),
            c if media::owns(c) => media::act(st, fx, c),
            _ => {}
        }
    }

    fn hints(&self, _st: &State) -> Vec<Hint> {
        vec![]
    }
}
