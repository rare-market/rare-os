//! The Rare menu's Game tab (overlay/GameTab.tsx): the game card, Resume, your items, Screenshot and
//! Save clip with their chords, Performance, the Steam row, Steam pages, emulator rows, Game
//! details, and Quit game with its armed second press.

use super::model::{game_kind, items_line, playing_for, GameKind};
use super::{EmuAction, MenuData};
use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::theme::tok;
use rare_render::{Fit, Id, ImgReq, Node};

/// Steam's own UI as an app (apps/mod.rs STEAM_RUN).
pub const STEAM_UI: &str = "app:steam";

pub const STEAM_PAGES: [(&str, &str); 4] = [("library", "Library"), ("store", "Store"), ("downloads", "Downloads"), ("settings", "Steam settings")];

// Focus groups (data-nav-group).
const G_RESUME: u32 = 1;
const G_GAME: u32 = 2;
const G_QUIT: u32 = 3;

// Row tags, mapped to actions by `act`.
pub const T_RESUME: u32 = 1;
pub const T_ITEMS: u32 = 2;
pub const T_SCREENSHOT: u32 = 3;
pub const T_CLIP: u32 = 4;
pub const T_PERF: u32 = 5;
pub const T_STEAM: u32 = 6;
pub const T_DETAILS: u32 = 7;
pub const T_QUIT: u32 = 8;
pub const T_DISCORD_MUTE: u32 = 9;
pub const T_DISCORD_DEAFEN: u32 = 10;
pub const T_OPEN_STEAM: u32 = 11;
pub const T_STEAM_PAGE: u32 = 100;
pub const T_EMU: u32 = 200;

/// The nodes the screen edits after the build (the quit row's armed state).
pub struct GameRefs {
    pub quit_title: Option<Id>,
    pub quit_sub: Option<Id>,
    pub quit_what: String,
    pub quit_label: String,
}

fn profile_name(p: &str) -> &'static str {
    match p {
        "quiet" => "Quiet",
        "balanced" => "Balanced",
        "performance" => "Performance",
        "custom" => "Custom",
        _ => "Balanced",
    }
}

/// The Performance row's sub line.
pub fn perf_sub(d: &MenuData) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(p) = &d.perf {
        let tdp = p.tdp.filter(|t| *t > 0.0).map(|t| format!(" · {} W", t.round() as i64)).unwrap_or_default();
        parts.push(format!("{}{}", profile_name(&p.profile), tdp));
    }
    if d.power_saved {
        parts.push(format!("Used for {}", d.game.title));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

pub fn quit_texts(rid: &str, title: &str, kind: Option<GameKind>) -> (String, String) {
    let label = if rid == STEAM_UI {
        "Close Steam"
    } else if kind == Some(GameKind::App) {
        "Close app"
    } else {
        "Quit game"
    };
    let what = if rid == STEAM_UI {
        "Steam closes, with any game it runs, and Rare has the screen.".to_string()
    } else if kind == Some(GameKind::Stream) {
        format!("Ends {title} on your PC and goes back to Rare.")
    } else {
        format!("Ends {title} and goes back to Rare.")
    };
    (label.to_string(), what)
}

/// "Rare+Y" chord, right of Screenshot and Save clip.
fn chord(cx: &mut Cx, parent: Id, b: G) {
    let r = cx.r();
    let c = cx.scene.add(parent, s().row().items_center().gap(0.25 * r).done(), Node::default());
    glyph(cx, c, G::Rare, true);
    text(cx, c, ty::LABEL, 400, tok::MUTED, "+");
    glyph(cx, c, b, true);
}

/// One menu line: a Row with title (+ sub) and an optional chord. Registers it on the nav.
fn line(cx: &mut Cx, nav: &mut Nav, parent: Id, title: &str, sub: Option<&str>, tag: u32, group: u32, hint: &str, chord_of: Option<G>) -> RowParts {
    let mut p = row(cx, parent, title, sub);
    match chord_of {
        // X: a press inside the menu, not a chord with the Rare button.
        Some(G::X) => {
            glyph(cx, p.right, G::X, true);
        }
        Some(b) => chord(cx, p.right, b),
        None => {}
    }
    p.f = p.f.clone().tag(tag).group(group).hint(hint);
    nav.add(p.f.clone());
    p
}

pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, d: &MenuData, now: f64) -> GameRefs {
    let r = cx.r();
    let rid = d.rid.clone().unwrap_or_default();
    let kind = game_kind(d.rid.as_deref());
    let col = cx.scene.add(parent, s().col().gap(0.6 * r).px(0.1 * r).done(), Node::default());

    // The game card.
    let card = cx.scene.add(col, s().row().items_center().gap(0.9 * r).pad(0.65 * r, 0.3 * r, 0.65 * r, 0.0).done(), Node::default());
    if let Some(src) = d.game.banner.clone() {
        let (w, h) = (9.0 * r, 4.25 * r);
        let a = cx.scene.add(card, s().wh(w, h).done(), Node::bg(tok::L3).radius(0.5 * r).clip());
        let k = cx.img.request(ImgReq { src, w: w as u32, h: h as u32, fit: Fit::Cover { fy: 0.5 } });
        cx.scene.add(a, s().fill().done(), Node::default().image(k));
    } else if let Some(src) = d.game.logo.clone() {
        let w = 3.5 * r;
        let k = cx.img.request(ImgReq { src, w: w as u32, h: w as u32, fit: Fit::Contain });
        cx.scene.add(card, s().wh(w, w).done(), Node::default().radius(0.22 * w).image(k).contain([0.5, 0.5]));
    }
    let names = cx.scene.add(card, s().col().shrink().done(), Node::default());
    text_tr(cx, names, ty::ROW, 600, tok::FG, &d.game.title);
    if let Some(p) = playing_for(d.started_ms, d.now_ms) {
        text_tr(cx, names, ty::SMALL, 400, tok::MUTED, &p);
    }

    // Resume, primary and focused first.
    let rw = cx.scene.add(col, s().col().px(0.65 * r).done(), Node::default());
    let b = button(cx, rw, "Resume", Variant::Primary, Sz::Md, true).tag(T_RESUME).group(G_RESUME).hint("Resume");
    let first = nav.add(b);
    // Music or video playing anywhere: its controls right under Resume.
    super::media::build(cx, nav, col);

    // Menu rows.
    let grp = cx.scene.add(col, s().col().done(), Node::default());
    if let Some(items) = d.items.as_ref().filter(|i| !i.is_empty()) {
        line(cx, nav, grp, "Your items", Some(&items_line(items)), T_ITEMS, G_GAME, "Open", None);
    }
    let shot = if d.taking == Some("screenshot") { "Saving..." } else { "Screenshot" };
    line(cx, nav, grp, shot, None, T_SCREENSHOT, G_GAME, "Take a screenshot", Some(G::Y));
    let clip = if d.taking == Some("clip") { "Saving...".to_string() } else { d.replay.map(crate::features::capture::save_label).unwrap_or_else(|| "Save clip".into()) };
    let clip_sub = match d.replay {
        Some(_) => None,
        None => Some("Instant replay is off. Turn it on in Quick settings."),
    };
    line(cx, nav, grp, &clip, clip_sub, T_CLIP, G_GAME, "Save the clip", Some(G::Select));
    let ps = perf_sub(d);
    line(cx, nav, grp, "Performance", ps.as_deref(), T_PERF, G_GAME, "Change", None);
    // Discord's voice, while it is open: mute and deafen without leaving the game.
    if let Some((muted, deafened)) = d.discord {
        let rows = [
            (if muted { "Unmute Discord" } else { "Mute Discord" }, if muted { "Your microphone is muted" } else { "Your microphone is on" }, T_DISCORD_MUTE, if muted { "Unmute" } else { "Mute" }, "discord-mute"),
            (if deafened { "Undeafen Discord" } else { "Deafen Discord" }, if deafened { "You hear nothing from Discord" } else { "You hear Discord" }, T_DISCORD_DEAFEN, if deafened { "Undeafen" } else { "Deafen" }, "discord-deafen"),
        ];
        for (title, sub, tag, hint, sid) in rows {
            let mut p = row(cx, grp, title, Some(sub));
            p.f = p.f.clone().tag(tag).group(G_GAME).hint(hint).sid(sid);
            nav.add(p.f.clone());
        }
    }
    if kind == Some(GameKind::Steam) || rid.starts_with("shortcut:") {
        line(cx, nav, grp, "Steam", Some("Steam's overlay: friends, guides, controller layout"), T_STEAM, G_GAME, "Open Steam's overlay", None);
    }
    // Steam's own UI, from any game (X does the same).
    if rid != STEAM_UI {
        line(cx, nav, grp, "Open Steam", None, T_OPEN_STEAM, G_GAME, "Open Steam", Some(G::X));
    }
    if rid == STEAM_UI {
        for (i, (_, title)) in STEAM_PAGES.iter().enumerate() {
            line(cx, nav, grp, title, None, T_STEAM_PAGE + i as u32, G_GAME, "Open in Steam", None);
        }
    }
    // State slots (RetroArch ROMs) sit under Save state as pictures: A loads one.
    let first_slot = d.emu.iter().position(|a| a.id.starts_with("load:"));
    for (i, a) in d.emu.iter().enumerate() {
        if a.id.starts_with("load:") {
            if Some(i) == first_slot {
                slots(cx, nav, grp, d, i);
            }
            continue;
        }
        if a.ready {
            let sub = if a.id.starts_with("save:") && first_slot.is_none() { Some("No saved states yet") } else { None };
            line(cx, nav, grp, &a.label, sub, T_EMU + i as u32, G_GAME, &a.label, None);
        } else {
            // Not set up in the emulator yet: why, as a line focus skips (A would do nothing).
            row(cx, grp, &a.label, a.note.as_deref());
        }
    }
    if d.in_library || d.is_app {
        line(cx, nav, grp, if d.is_app { "App details" } else { "Game details" }, None, T_DETAILS, G_GAME, "Open", None);
    }

    // Quit game, which asks once more.
    let (label, what) = quit_texts(&rid, &d.game.title, kind);
    let mut refs = GameRefs { quit_title: None, quit_sub: None, quit_what: what.clone(), quit_label: label.clone() };
    if d.running {
        let qg = cx.scene.add(col, s().col().mt(0.2 * r).pad(0.0, 0.35 * r + 1.0, 0.0, 0.0).done(), Node::default());
        edge(cx, qg, 0, tok::HAIR);
        let p = line(cx, nav, qg, &label, Some(&what), T_QUIT, G_QUIT, &label, None);
        refs.quit_title = Some(p.title);
        refs.quit_sub = p.sub;
    }
    nav.focus(cx.scene, first, now, false);
    refs
}

impl EmuAction {
    pub fn act_name(&self) -> String {
        format!("emu:{}", self.id)
    }
}

/// Focus group of the state slot cards (left and right move between them).
const G_SLOTS: u32 = 4;

/// The saved state slots as picture cards in one row, each with its slot and when it was saved.
fn slots(cx: &mut Cx, nav: &mut Nav, parent: Id, d: &MenuData, from: usize) {
    let r = cx.r();
    let strip = cx.scene.add(parent, s().row().gap(0.6 * r).pad(0.35 * r, 0.75 * r, 0.5 * r, 0.75 * r).done(), Node::default());
    for (i, a) in d.emu.iter().enumerate().skip(from).filter(|(_, a)| a.id.starts_with("load:")).take(4) {
        let (w, h) = (6.6 * r, 3.7 * r);
        let card = cx.scene.add(strip, s().col().w(w).gap(0.3 * r).done(), Node::default());
        let pic = cx.scene.add(card, s().wh(w, h).items_center().justify_center().done(), Node::bg(tok::L3).radius(0.4 * r).clip());
        match a.thumb.clone() {
            Some(src) => {
                let k = cx.img.request(ImgReq { src, w: w as u32, h: h as u32, fit: Fit::Cover { fy: 0.5 } });
                cx.scene.add(pic, s().fill().done(), Node::default().image(k));
            }
            None => {
                text(cx, pic, ty::LABEL, 500, tok::MUTED, "No picture");
            }
        }
        let names = cx.scene.add(card, s().row().items_center().gap(0.4 * r).done(), Node::default());
        text(cx, names, ty::LABEL, 600, tok::FG, &a.label);
        if let Some(sub) = &a.sub {
            text_tr(cx, names, ty::LABEL, 400, tok::MUTED, sub);
        }
        nav.add(Focusable { id: card, kind: Kind::Tile { scale: 1.05 }, group: G_SLOTS, hint: "Load".into(), tag: T_EMU + i as u32, sid: String::new() });
    }
}
