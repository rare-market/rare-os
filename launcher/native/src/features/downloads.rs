//! Steam installs and the Downloads view: Steam's download queue read live (rare-launch steamdl),
//! progress on library tiles and the game page, the Install button's drive sheet, the Rare menu's
//! Downloads row while anything downloads, and the "finished downloading" notice.
//!
//! The queue is read on a worker every few seconds (more often while something downloads) into
//! `Shared::snap`; screens read it from `State::feat.downloads` and rebuild on `ver`. Rows and bars
//! keep their size whatever the numbers say, so values change in place.

use crate::act::Act;
use crate::data::model::Game;
use crate::kit::format::fmt_size;
use crate::kit::{self, s, tok, Btn as B, Ty, Ui, T};
use crate::page::{hash, Acts, Btn, Feedback, Fx, Page};
use crate::screens::game::parts::{self, Variant};
use crate::state::{OptionItem, OptionsSpec, State, Tone};
pub use rare_launch::steamdl::{Dl, Folder, Snapshot, Started};
use rare_render::focus::hints::{hint, Hint, Key};
use rare_render::focus::{Focusable, Group};
use rare_render::view::{key, El, Motion};
use std::sync::Mutex;

/// The overlay kind of the Downloads view.
pub const KIND: &str = "downloads";

/// Poll intervals: while something downloads, while nothing does, under a game.
const POLL_BUSY: f64 = 2.0;
const POLL_IDLE: f64 = 8.0;
const POLL_GAME: f64 = 15.0;

/// What a Downloads control asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ctl {
    /// The row's sheet (A on a row).
    Menu,
    Pause,
    Resume,
    Top,
    Cancel,
    /// A waiting update: into the queue now.
    UpdateNow,
    /// The install's licence agreements, accepted.
    AcceptEula,
    /// The install waiting on them, cancelled.
    CancelInstall,
}

/// What `State::feat.downloads` keeps.
#[derive(Default)]
pub struct Shared {
    pub snap: Snapshot,
    /// Moves when `snap` or `asked` changed.
    pub ver: u64,
    next: f64,
    busy: bool,
    /// Installs asked for that Steam hasn't listed yet (game id, since).
    pub asked: Vec<(String, f64)>,
    /// Finished-download notices for the app to post.
    pub notices: Vec<rare_notify::LocalNotice>,
    watch: rare_notify::downloads::DownloadWatch,
    ids: Vec<u32>,
}

/// The Rare menu's Downloads line ("2 downloading, 45%"), None while nothing downloads. The menu is
/// built on the scene outside the State, so it reads this.
static MENU_LINE: Mutex<Option<String>> = Mutex::new(None);

pub fn menu_line() -> Option<String> {
    MENU_LINE.lock().unwrap().clone().or_else(super::stores::menu_line)
}

fn appid_of(id: &str) -> Option<u32> {
    id.strip_prefix("steam:")?.parse().ok()
}

/// The library's name for an app, else the manifest's, else Steam's number.
pub fn title_of(st: &State, d: &Dl) -> String {
    st.game(&format!("steam:{}", d.appid)).map(|g| g.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| if d.title.is_empty() { format!("Steam app {}", d.appid) } else { d.title.clone() })
}

/// The queue entry for a library game.
pub fn of<'a>(st: &'a State, id: &str) -> Option<&'a Dl> {
    let a = appid_of(id)?;
    st.feat.downloads.snap.items.iter().find(|d| d.appid == a)
}

/// A waiting (unscheduled) update.
pub fn waiting_of(st: &State, appid: u32) -> Option<&Dl> {
    st.feat.downloads.snap.waiting.iter().find(|d| d.appid == appid)
}

/// An install asked for and not yet in Steam's queue.
pub fn asked(st: &State, id: &str) -> bool {
    st.feat.downloads.asked.iter().any(|(a, _)| a == id)
}

pub fn fraction(d: &Dl) -> f32 {
    if d.total == 0 {
        0.0
    } else {
        (d.done as f64 / d.total as f64).clamp(0.0, 1.0) as f32
    }
}

/// "12.4 MB/s".
fn fmt_rate(b: u64) -> String {
    let b = b as f64;
    if b >= 1e9 {
        format!("{:.1} GB/s", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB/s", b / 1e6)
    } else {
        format!("{} KB/s", ((b / 1e3).round() as u64).max(1))
    }
}

/// "38 min left", "2 h 5 min left", "Less than a minute left".
fn fmt_eta(secs: u64) -> String {
    match secs {
        0 => String::new(),
        s if s < 60 => "Less than a minute left".into(),
        s if s < 3600 => format!("{} min left", (s + 59) / 60),
        s => {
            let m = (s % 3600 + 59) / 60;
            if m == 0 || m == 60 {
                format!("{} h left", (s + 3599) / 3600)
            } else {
                format!("{} h {m} min left", s / 3600)
            }
        }
    }
}

fn gb(b: u64) -> String {
    let z = fmt_size(Some(b as f64));
    if z.is_empty() {
        "0 MB".into()
    } else {
        z
    }
}

/// The status word: Downloading, Paused, Queued, Updating.
pub fn status(d: &Dl) -> &'static str {
    match (d.paused, d.active, d.update) {
        (true, _, _) => "Paused",
        (false, true, true) => "Updating",
        (false, true, false) => "Downloading",
        (false, false, _) => "Queued",
    }
}

/// "4.2 GB of 44.4 GB · 12.3 MB/s · 38 min left" (only what is known).
pub fn detail(d: &Dl) -> String {
    let mut parts = vec![if d.total > 0 { format!("{} of {}", gb(d.done), gb(d.total)) } else { String::new() }];
    if d.active && !d.paused {
        if d.rate > 0 {
            parts.push(fmt_rate(d.rate));
        }
        parts.push(fmt_eta(d.eta));
    }
    parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" \u{b7} ")
}

/// A thin progress bar `w` rem wide (None: full width). Same height whatever the value.
pub fn bar(ui: &Ui, w: Option<f32>, frac: f32, dim: bool) -> El {
    let st = match w {
        Some(w) => s().w(w),
        None => s().wp(1.0),
    };
    let fill = ui.bx(s().wp(frac.clamp(0.0, 1.0)).h_px(4.0)).bg(if dim { tok::fg(0.45) } else { tok::BRAND }).radius(ui.r(0.15));
    ui.bx(st.h_px(4.0)).bg(tok::fg(0.15)).radius(ui.r(0.15)).clip().child(fill)
}

/// A library tile's progress, laid over the bottom of its art (no layout change).
pub fn tile_overlay(ui: &Ui, st: &State, id: &str) -> Option<El> {
    let d = of(st, id);
    if d.is_none() && !asked(st, id) {
        return None;
    }
    let (frac, dim) = d.map(|d| (fraction(d), d.paused || !d.active)).unwrap_or((0.0, true));
    Some(ui.bx(s().abs(None, Some(0.5), Some(0.5), Some(0.5))).child(bar(ui, None, frac, dim)))
}

/// The game page's download block: status, bar, numbers. None when the game isn't in the queue.
pub fn game_panel(ui: &mut Ui, st: &State, g: &Game) -> Option<El> {
    let d = of(st, &g.id);
    if d.is_none() && !asked(st, &g.id) {
        return None;
    }
    let (word, line, frac, dim) = match d {
        Some(d) => (status(d).to_string(), detail(d), fraction(d), d.paused || !d.active),
        None => ("Starting the install".to_string(), "Steam is getting it ready".to_string(), 0.0, true),
    };
    let pct = d.filter(|d| d.total > 0).map(|d| format!("{}%", (fraction(d) * 100.0).floor() as u32)).unwrap_or_default();
    let head = ui.bx(s().row().items_center().gap(0.6).mb(0.45)).child(ui.text(word, T::new(Ty::Small).w(600))).child(ui.text_s(s().ml_auto(), pct, T::new(Ty::Small).w(600).tab()));
    Some(ui.bx(s().col().w(26.0)).child(head).child(bar(ui, None, frac, dim)).child(ui.text_s(s().mt(0.35), if line.is_empty() { " ".to_string() } else { line }, T::new(Ty::Small).c(tok::MUTED).tab().trunc())).key(key("dl-panel")))
}

/// The game page's Update now while Steam holds an update for it back.
pub fn update_now_button(ui: &mut Ui, st: &State, acts: &mut Acts, g: &Game) -> Option<El> {
    let appid = appid_of(&g.id)?;
    waiting_of(st, appid)?;
    Some(kit::button(ui, B::new("Update now", acts.game(Act::Download(appid, Ctl::UpdateNow), &g.id)).lg().ghost().game()))
}

/// The game page's main button while the game is in the queue (Pause or Resume), else None.
pub fn game_button(ui: &mut Ui, st: &State, acts: &mut Acts, g: &Game) -> Option<El> {
    let appid = appid_of(&g.id)?;
    if let Some(d) = of(st, &g.id) {
        let (label, ctl, icon) = if d.paused || !d.active { ("Resume", Ctl::Resume, "play") } else { ("Pause", Ctl::Pause, "pause") };
        return Some(kit::button(ui, B::new(label, acts.game(Act::Download(appid, ctl), &g.id)).primary().lg().def(true).hint(label).icon(kit::BtnIcon::Lucide(icon, 1.1, true)).min_w(10.0).game()));
    }
    if asked(st, &g.id) {
        return Some(kit::button(ui, B::new("Starting", acts.game(Act::None, &g.id)).primary().lg().def(true).min_w(10.0).game()));
    }
    if !g.installed && g.source == "steam" {
        let label = "Install";
        return Some(kit::button(ui, B::new(label, acts.game(Act::Install(g.id.clone()), &g.id)).primary().lg().def(true).hint(label).icon(kit::BtnIcon::Lucide("download", 1.1, false)).min_w(10.0).game()));
    }
    None
}

// ---- polling ---------------------------------------------------------------------------------

/// Every update: reads Steam's queue when due.
pub fn poll(st: &mut State, fx: &mut Fx, now: f64) {
    let sh = &mut st.feat.downloads;
    if sh.busy || now < sh.next {
        return;
    }
    sh.busy = true;
    fx.spawn(|b| {
        let snap = b.steam_downloads();
        Box::new(move |st: &mut State, now| apply(st, snap, now))
    });
}

fn apply(st: &mut State, snap: Option<Snapshot>, now: f64) {
    let gaming = !st.running.is_empty();
    // Titles and the finished notices.
    let lib_titles: Vec<(u32, String)> = snap.as_ref().map(|s| s.items.iter().map(|d| (d.appid, title_of(st, d))).collect()).unwrap_or_default();
    let finished_titles: Vec<String> = snap
        .as_ref()
        .map(|s| s.finished.iter().map(|a| st.game(&format!("steam:{a}")).map(|g| g.title.clone()).unwrap_or_else(|| format!("Steam app {a}"))).collect())
        .unwrap_or_default();
    let sh = &mut st.feat.downloads;
    sh.busy = false;
    let Some(mut snap) = snap else {
        sh.next = now + POLL_IDLE;
        return;
    };
    for d in snap.items.iter_mut() {
        if let Some((_, t)) = lib_titles.iter().find(|(a, _)| *a == d.appid) {
            d.title = t.clone();
        }
    }
    let as_notify: Vec<rare_notify::downloads::Download> =
        snap.items.iter().map(|d| rare_notify::downloads::Download { id: format!("steam:{}", d.appid), title: d.title.clone(), done: d.done as f64, total: d.total as f64, paused: d.paused }).collect();
    let mut notices = sh.watch.poll(Some(as_notify));
    for t in finished_titles {
        let title = format!("{t} finished downloading");
        if !notices.iter().any(|n| n.title == title) {
            notices.push(rare_notify::LocalNotice::new(rare_notify::types::NoticeKind::DownloadDone, title, rare_notify::types::NoticeAction::Downloads));
        }
    }
    let finished = !notices.is_empty();
    sh.notices.extend(notices);
    // Asked installs leave "Starting" once Steam lists them (or after two minutes).
    let listed: Vec<String> = snap.items.iter().map(|d| format!("steam:{}", d.appid)).collect();
    sh.asked.retain(|(id, t)| !listed.contains(id) && now - t < 120.0);
    let ids: Vec<u32> = snap.items.iter().map(|d| d.appid).collect();
    let set_changed = ids.iter().any(|a| !sh.ids.contains(a)) || sh.ids.iter().any(|a| !ids.contains(a));
    sh.ids = ids;
    let busy = !snap.items.is_empty() || !sh.asked.is_empty();
    sh.next = now + if gaming { POLL_GAME } else if busy { POLL_BUSY } else { POLL_IDLE };
    if snap != sh.snap {
        sh.snap = snap;
        sh.ver += 1;
    }
    let line = menu_summary(&sh.snap);
    *MENU_LINE.lock().unwrap() = line;
    // A new install's manifest appeared or one finished: the library moves it.
    if set_changed || finished {
        st.rescan_at = Some(st.rescan_at.unwrap_or(now + 1.0).min(now + 1.0));
    }
}

/// "Hades II, 45%" / "3 downloading" / "2 paused".
fn menu_summary(s: &Snapshot) -> Option<String> {
    let items = &s.items;
    if items.is_empty() {
        return None;
    }
    let running: Vec<&Dl> = items.iter().filter(|d| d.active && !d.paused).collect();
    Some(match (items.len(), running.first()) {
        (1, Some(d)) => format!("{}, {}%", d.title, (fraction(d) * 100.0).floor() as u32),
        (n, Some(d)) => format!("{}, {}%. {} more in the queue", d.title, (fraction(d) * 100.0).floor() as u32, n - 1),
        (n, None) => format!("{n} paused"),
    })
}

/// Forces the next read now (after an action).
fn soon(st: &mut State, now: f64) {
    st.feat.downloads.next = now + 0.6;
}

// ---- actions ---------------------------------------------------------------------------------

/// The shared acts: Install (the drive sheet), InstallTo, and the Downloads controls.
pub fn run(st: &mut State, fx: &mut Fx, a: Act, now: f64) {
    match a {
        Act::Install(id) => {
            let Some(g) = st.game(&id).cloned() else { return };
            fx.fb(Feedback::Open);
            fx.spawn(move |b| {
                let folders = b.steam_folders();
                Box::new(move |st: &mut State, now| drive_sheet(st, &g, folders, now))
            });
        }
        Act::InstallTo(id, path) => {
            let Some(appid) = appid_of(&id) else { return };
            let title = st.game(&id).map(|g| g.title.clone()).unwrap_or_default();
            st.feat.downloads.asked.retain(|(a, _)| *a != id);
            st.feat.downloads.asked.push((id.clone(), now));
            st.feat.downloads.ver += 1;
            fx.fb(Feedback::Launch);
            st.toast(format!("Starting the install of {title}"), Tone::Info, now);
            fx.spawn(move |b| {
                let r = b.steam_install(appid, &path);
                Box::new(move |st: &mut State, now| {
                    match r {
                        Ok(Started::Queued) => {}
                        Ok(Started::SteamShown) => st.toast("Finish the install in Steam. The Rare button brings you back.", Tone::Info, now),
                        Ok(Started::NeedsEula(eulas)) => eula_sheet(st, appid, &id, &eulas),
                        Err(e) => {
                            log::warn!("install {id}: {e}");
                            st.feat.downloads.asked.retain(|(a, _)| *a != id);
                            st.feat.downloads.ver += 1;
                            st.toast(if e.contains("busy") { "Close the game first, then install.".to_string() } else { e }, Tone::Error, now);
                        }
                    }
                    soon(st, now);
                })
            });
        }
        Act::Download(appid, Ctl::Menu) => row_sheet(st, appid),
        Act::Download(appid, ctl) => {
            fx.fb(Feedback::Select);
            // Values change at once; Steam's next read confirms.
            if let Some(d) = st.feat.downloads.snap.items.iter_mut().find(|d| d.appid == appid) {
                match ctl {
                    Ctl::Pause => d.paused = true,
                    Ctl::Resume => d.paused = false,
                    _ => {}
                }
                st.feat.downloads.ver += 1;
            }
            let c = match ctl {
                Ctl::Pause => rare_launch::steamdl::Control::Pause,
                Ctl::Resume => rare_launch::steamdl::Control::Resume,
                Ctl::Top => rare_launch::steamdl::Control::Top,
                Ctl::UpdateNow => rare_launch::steamdl::Control::UpdateNow,
                Ctl::AcceptEula => rare_launch::steamdl::Control::AcceptEula,
                Ctl::CancelInstall => rare_launch::steamdl::Control::CancelInstall,
                _ => rare_launch::steamdl::Control::Cancel,
            };
            fx.spawn(move |b| {
                let r = b.steam_control(appid, c);
                Box::new(move |st: &mut State, now| {
                    match r {
                        Ok(true) => {
                            if ctl == Ctl::Cancel {
                                st.toast("Removed from the queue", Tone::Info, now);
                            }
                            if ctl == Ctl::UpdateNow {
                                st.toast("Updating now", Tone::Info, now);
                            }
                            if ctl == Ctl::CancelInstall {
                                st.feat.downloads.asked.retain(|(a, _)| *a != format!("steam:{appid}"));
                                st.feat.downloads.ver += 1;
                            }
                        }
                        Ok(false) => st.toast("Steam's Downloads has the screen. The Rare button brings you back.", Tone::Info, now),
                        Err(e) if ctl == Ctl::AcceptEula => {
                            log::warn!("download {appid}: {e}");
                            st.feat.downloads.asked.retain(|(a, _)| *a != format!("steam:{appid}"));
                            st.feat.downloads.ver += 1;
                            st.toast(e, Tone::Error, now);
                        }
                        Err(e) => {
                            log::warn!("download {appid}: {e}");
                            st.toast(if e.contains("busy") { "Close the game first, then try again.".to_string() } else { "Steam didn't take that. Try again.".to_string() }, Tone::Error, now);
                        }
                    }
                    soon(st, now);
                })
            });
        }
        _ => {}
    }
}

/// The game's licence agreements before Steam installs it: accept in Rare, read one, or cancel.
fn eula_sheet(st: &mut State, appid: u32, id: &str, eulas: &[rare_library::appinfo::Eula]) {
    let title = st.game(id).map(|g| g.title.clone()).unwrap_or_else(|| "This game".into());
    let names: Vec<String> = eulas.iter().map(|e| e.name.clone()).filter(|n| !n.is_empty()).fold(Vec::new(), |mut v, n| {
        if !v.contains(&n) {
            v.push(n);
        }
        v
    });
    let mut items = vec![OptionItem { label: "Accept and install".into(), on: None, act: Act::Download(appid, Ctl::AcceptEula) }];
    for e in eulas.iter().filter(|e| !e.url.is_empty()).take(2) {
        items.push(OptionItem { label: format!("Read {}", if e.name.is_empty() { "the agreement" } else { e.name.as_str() }), on: None, act: Act::OpenWeb(e.url.replace("http://", "https://")) });
    }
    items.push(OptionItem { label: "Cancel the install".into(), on: None, act: Act::Download(appid, Ctl::CancelInstall) });
    let sub = if names.is_empty() { format!("{title} asks you to accept its licence agreement.") } else { format!("{title} asks you to accept: {}.", names.join(", ")) };
    st.options = Some(OptionsSpec { title: "Licence agreement".into(), sub: Some(sub), items });
}

/// "Install <game>": one row per drive with its free space.
fn drive_sheet(st: &mut State, g: &Game, folders: Vec<Folder>, now: f64) {
    if folders.is_empty() {
        st.toast("Steam has no library folder yet. Open Steam once, then try again.", Tone::Error, now);
        return;
    }
    let need = g.size_bytes.unwrap_or(0.0);
    let items = folders
        .iter()
        .map(|f| {
            let free = f.free.map(|b| format!("{} free", fmt_size(Some(b as f64)))).unwrap_or_default();
            let short = f.free.is_some_and(|b| need > 0.0 && (b as f64) < need);
            let label = [f.name.clone(), free, if short { "Not enough space".into() } else { String::new() }].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" \u{b7} ");
            let act = if short { Act::Toast(format!("{} needs {}. Free up space or pick another drive.", g.title, fmt_size(Some(need)))) } else { Act::InstallTo(g.id.clone(), f.path.clone()) };
            OptionItem { label, on: None, act }
        })
        .collect();
    let sub = if need > 0.0 { format!("Install size {}. Pick a drive.", fmt_size(Some(need))) } else { "Pick a drive.".to_string() };
    st.options = Some(OptionsSpec { title: format!("Install {}", g.title), sub: Some(sub), items });
}

/// A queue row's sheet: Pause or Resume, Move to the top, Cancel, the game's page.
fn row_sheet(st: &mut State, appid: u32) {
    let id = format!("steam:{appid}");
    if let Some(d) = waiting_of(st, appid).cloned() {
        let mut items = vec![OptionItem { label: "Update now".into(), on: None, act: Act::Download(appid, Ctl::UpdateNow) }];
        if st.game(&id).is_some() {
            items.push(OptionItem { label: "Game page".into(), on: None, act: Act::OpenGame(id) });
        }
        st.options = Some(OptionsSpec { title: title_of(st, &d), sub: Some("Waiting. Steam updates it later, or now if you ask.".into()), items });
        return;
    }
    let Some(d) = st.feat.downloads.snap.items.iter().find(|d| d.appid == appid).cloned() else { return };
    let mut items = vec![if d.paused || !d.active {
        OptionItem { label: "Resume".into(), on: None, act: Act::Download(appid, Ctl::Resume) }
    } else {
        OptionItem { label: "Pause".into(), on: None, act: Act::Download(appid, Ctl::Pause) }
    }];
    if !(d.active && !d.paused) {
        items.push(OptionItem { label: "Download this first".into(), on: None, act: Act::Download(appid, Ctl::Top) });
    }
    items.push(OptionItem { label: if d.update { "Remove the update from the queue".into() } else { "Cancel the install".into() }, on: None, act: Act::Download(appid, Ctl::Cancel) });
    if st.game(&id).is_some() {
        items.push(OptionItem { label: "Game page".into(), on: None, act: Act::OpenGame(id) });
    }
    st.options = Some(OptionsSpec { title: title_of(st, &d), sub: Some(status(&d).to_string()), items });
}

// ---- the Downloads view ----------------------------------------------------------------------

#[derive(Default)]
pub struct DownloadsPage;

fn row(ui: &mut Ui, st: &State, acts: &mut Acts, d: &Dl, def: bool, waiting: bool) -> El {
    let id = format!("steam:{}", d.appid);
    let g = st.game(&id);
    let title = title_of(st, d);
    let sources: Vec<String> = g.map(|g| if g.art.header.is_empty() { g.art.cover.clone() } else { g.art.header.clone() }).unwrap_or_else(|| vec![format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{}/header.jpg", d.appid)]);
    let art = ui.bx(s().wh(8.6, 4.0)).bg(tok::L3).radius(ui.r(0.4)).clip().child(parts::art(ui, &sources, &title, Variant::Header, 8.6, 4.0, 0.5));
    let pct = if d.total > 0 { format!("{}%", (fraction(d) * 100.0).floor() as u32) } else { String::new() };
    let mut word = status(d).to_string();
    if !d.active && !d.paused && d.queue == 1 {
        word = "Next".into();
    }
    let pct = if waiting { String::new() } else { pct };
    if waiting {
        word = "Waiting".into();
    }
    let head = ui
        .bx(s().row().items_center().gap(0.75))
        .child(ui.text_s(s().grow().min0(), title.clone(), T::new(Ty::Body).w(600).trunc()))
        .child(ui.text(word, T::new(Ty::Small).c(tok::MUTED)))
        .child(ui.text_s(s().w(3.2), pct, T::new(Ty::Small).w(600).tab().right()));
    let line = if waiting { if d.total > 0 { format!("Update, {}", gb(d.total)) } else { "Update".into() } } else { detail(d) };
    let col = ui
        .bx(s().col().grow().min0().gap(0.45))
        .child(head)
        .child(bar(ui, None, fraction(d), d.paused || !d.active))
        .child(ui.text(if line.is_empty() { " ".to_string() } else { line }, T::new(Ty::Small).c(tok::MUTED).tab().trunc()));
    let el = ui.bx(s().row().items_center().gap(1.1).h(5.5).wp(1.0).px(0.85)).radius(ui.r(0.5)).child(art).child(col);
    kit::nav(kit::row_look(el), acts.game(Act::Download(d.appid, Ctl::Menu), &id), Some("Options"), None, def).key(key(&id))
}

fn asked_row(ui: &mut Ui, st: &State, acts: &mut Acts, id: &str, def: bool) -> El {
    let g = st.game(id);
    let title = g.map(|g| g.title.clone()).unwrap_or_default();
    let sources: Vec<String> = g.map(|g| if g.art.header.is_empty() { g.art.cover.clone() } else { g.art.header.clone() }).unwrap_or_default();
    let art = ui.bx(s().wh(8.6, 4.0)).bg(tok::L3).radius(ui.r(0.4)).clip().child(parts::art(ui, &sources, &title, Variant::Header, 8.6, 4.0, 0.5));
    let head = ui.bx(s().row().items_center().gap(0.75)).child(ui.text_s(s().grow().min0(), title, T::new(Ty::Body).w(600).trunc())).child(ui.text("Starting", T::new(Ty::Small).c(tok::MUTED))).child(ui.bx(s().w(3.2)));
    let col = ui.bx(s().col().grow().min0().gap(0.45)).child(head).child(bar(ui, None, 0.0, true)).child(ui.text("Steam is getting it ready", T::new(Ty::Small).c(tok::MUTED).trunc()));
    let el = ui.bx(s().row().items_center().gap(1.1).h(5.5).wp(1.0).px(0.85)).radius(ui.r(0.5)).child(art).child(col);
    kit::nav(kit::row_look(el), acts.game(Act::OpenGame(id.to_string()), id), Some("Game page"), None, def).key(key(id))
}

impl Page for DownloadsPage {
    fn stamp(&self, st: &State) -> u64 {
        hash((st.feat.downloads.ver, st.ver.library, super::stores::ver()))
    }

    fn view(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El {
        let sh = &st.feat.downloads;
        let items = &sh.snap.items;
        // Epic and GOG installs, updates and repairs (features/stores.rs) under Steam's.
        let store_jobs = rare_stores::jobs::list();
        let rate: u64 = items.iter().filter(|d| d.active && !d.paused).map(|d| d.rate).sum();
        let note = if items.is_empty() && sh.asked.is_empty() {
            String::new()
        } else if rate > 0 {
            fmt_rate(rate)
        } else if items.iter().all(|d| d.paused) && !items.is_empty() {
            "Paused".into()
        } else {
            String::new()
        };
        let head = ui
            .bx(s().row().items_center().gap(0.75).h(2.5).mb(1.25))
            .child(kit::icon(ui, "download", 1.5, tok::ICON2))
            .child(ui.text("Downloads", T::new(Ty::Title).w(700)))
            .child(ui.text_s(s().ml_auto(), note, T::new(Ty::Small).c(tok::MUTED).tab()));
        let mut page = ui.bx(s().col().fill().px(2.5).py(2.0)).bg(tok::BG).enter(Motion::FADE).scrolls_y(key("downloads")).child(head);
        let waiting = &sh.snap.waiting;
        if items.is_empty() && sh.asked.is_empty() && store_jobs.is_empty() && waiting.is_empty() {
            let lib = kit::button(ui, B::new("Not installed games", acts.tag(Act::Local(1))).primary().def(true));
            page = page.child(kit::empty(ui, s(), "Nothing is downloading", Some("Games you install and updates Steam fetches show here, with their progress."), vec![lib]));
            return page;
        }
        let mut rows: Vec<El> = Vec::new();
        for (i, d) in items.iter().enumerate() {
            rows.push(row(ui, st, acts, d, i == 0, false));
        }
        for (i, (id, _)) in sh.asked.iter().enumerate() {
            if !items.iter().any(|d| format!("steam:{}", d.appid) == *id) {
                rows.push(asked_row(ui, st, acts, id, items.is_empty() && i == 0));
            }
        }
        for (i, j) in store_jobs.iter().enumerate() {
            rows.push(super::stores::download_row(ui, st, acts, j, rows.is_empty() && i == 0));
        }
        if !rows.is_empty() {
            page = page.child(ui.bx(s().col().gap(0.5)).group(Group::new("downloads", [0.0; 4]).remember()).kids(rows));
        }
        // Updates Steam has not scheduled: Waiting, A for Update now.
        if !waiting.is_empty() {
            let first = items.is_empty() && sh.asked.is_empty() && store_jobs.is_empty();
            let wrows: Vec<El> = waiting.iter().enumerate().map(|(i, d)| row(ui, st, acts, d, first && i == 0, true)).collect();
            page = page
                .child(ui.text_s(s().mt(1.5).mb(0.6).ml(0.85), "Waiting", T::new(Ty::Small).w(600).c(tok::MUTED)))
                .child(ui.bx(s().col().gap(0.5)).group(Group::new("downloads-waiting", [0.0; 4]).remember()).kids(wrows));
        }
        if !sh.snap.from_steam {
            page = page.child(ui.text_s(s().mt(1.25), "Speed and time left come from this device's network while Steam starts up.", T::new(Ty::Small).c(tok::MUTED).wrap()));
        }
        page
    }

    fn act(&mut self, st: &mut State, _fx: &mut Fx, code: u32) {
        if code == 1 {
            crate::screens::library::show_not_installed();
            st.close();
            st.set_tab(crate::state::Tab::Library, false);
        }
    }

    fn pad(&mut self, st: &mut State, fx: &mut Fx, b: Btn, cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        // X pauses or resumes the focused row.
        if b != Btn::X {
            return false;
        }
        // An Epic or GOG row (features/stores.rs).
        if let Some((_, Some(Act::Legacy(a)))) = cur {
            if let Some(id) = a.strip_prefix("stores:menu:") {
                let running = rare_stores::jobs::get(id).is_some_and(|j| matches!(j.phase, rare_stores::jobs::Phase::Running | rare_stores::jobs::Phase::Queued));
                let a = format!("stores:{}:{id}", if running { "pause" } else { "resume" });
                super::stores::act(st, &a, rare_render::clock::secs());
                return true;
            }
        }
        let Some((_, Some(Act::Download(appid, _)))) = cur else { return false };
        let appid = *appid;
        if waiting_of(st, appid).is_some() {
            run(st, fx, Act::Download(appid, Ctl::UpdateNow), rare_render::clock::secs());
            return true;
        }
        let Some(d) = st.feat.downloads.snap.items.iter().find(|d| d.appid == appid) else { return false };
        let ctl = if d.paused || !d.active { Ctl::Resume } else { Ctl::Pause };
        run(st, fx, Act::Download(appid, ctl), rare_render::clock::secs());
        true
    }

    fn hints(&self, st: &State) -> Vec<Hint> {
        if (st.feat.downloads.snap.items.is_empty() && st.feat.downloads.snap.waiting.is_empty() && rare_stores::jobs::list().is_empty()) || st.options.is_some() {
            return vec![];
        }
        vec![hint(Key::X, "Pause, resume or update now")]
    }
}

// ---- fixtures (RR_DOWNLOADS=1 with the mock backend) -------------------------------------------

pub mod mock {
    use super::*;
    use std::time::Instant;

    pub fn on() -> bool {
        std::env::var("RR_DOWNLOADS").is_ok()
    }

    struct M {
        t0: Instant,
        items: Vec<Dl>,
    }

    static M_: Mutex<Option<M>> = Mutex::new(None);

    fn with<R>(f: impl FnOnce(&mut M) -> R) -> R {
        let mut g = M_.lock().unwrap();
        let m = g.get_or_insert_with(|| M {
            t0: Instant::now(),
            items: vec![
                Dl { appid: 1086940, title: "Baldur's Gate 3".into(), done: 31_400_000_000, total: 122_000_000_000, paused: false, active: true, queue: 0, update: true, rate: 0, eta: 0 },
                Dl { appid: 1245620, title: "ELDEN RING".into(), done: 0, total: 1_900_000_000, paused: false, active: false, queue: 1, update: true, rate: 0, eta: 0 },
                Dl { appid: 1145360, title: "Hades".into(), done: 6_100_000_000, total: 15_000_000_000, paused: true, active: false, queue: -1, update: false, rate: 0, eta: 0 },
            ],
        });
        f(m)
    }

    pub fn snapshot() -> Snapshot {
        with(|m| {
            let secs = m.t0.elapsed().as_secs_f64();
            let mut items = m.items.clone();
            for d in items.iter_mut().filter(|d| d.active && !d.paused) {
                d.rate = 24_600_000;
                d.done = (d.done + (secs * d.rate as f64) as u64).min(d.total);
                d.eta = (d.total - d.done) / d.rate;
            }
            let mut waiting = vec![Dl { appid: 1794680, title: "Vampire Survivors".into(), done: 0, total: 312_000_000, paused: false, active: false, queue: -1, update: true, rate: 0, eta: 0 }];
            waiting.retain(|w| !items.iter().any(|d| d.appid == w.appid));
            Snapshot { items, from_steam: true, all_paused: false, finished: vec![], waiting }
        })
    }

    pub fn folders() -> Vec<Folder> {
        vec![
            Folder { index: 0, path: "/home/rare/.local/share/Steam".into(), name: "This device".into(), free: Some(181_200_000_000), total: Some(1_000_000_000_000), sd: false },
            Folder { index: 1, path: "/run/media/rare/SD/SteamLibrary".into(), name: "SD card".into(), free: Some(402_700_000_000), total: Some(512_000_000_000), sd: true },
        ]
    }

    pub fn install(appid: u32) -> Started {
        with(|m| {
            for d in m.items.iter_mut() {
                d.queue = if d.queue >= 0 { d.queue + 1 } else { -1 };
            }
            m.items.push(Dl { appid, title: String::new(), done: 0, total: 44_356_783_694, paused: false, active: false, queue: 1, update: false, rate: 0, eta: 0 });
        });
        Started::Queued
    }

    pub fn control(appid: u32, c: rare_launch::steamdl::Control) {
        use rare_launch::steamdl::Control;
        if c == Control::UpdateNow {
            with(|m| {
                if !m.items.iter().any(|d| d.appid == appid) {
                    m.items.push(Dl { appid, title: "Vampire Survivors".into(), done: 0, total: 312_000_000, paused: false, active: false, queue: -1, update: true, rate: 0, eta: 0 });
                }
            });
        }
        with(|m| match c {
            Control::Pause => {
                if let Some(d) = m.items.iter_mut().find(|d| d.appid == appid) {
                    d.paused = true;
                    d.active = false;
                    d.queue = -1;
                }
            }
            Control::Resume | Control::Top | Control::UpdateNow => {
                for d in m.items.iter_mut() {
                    if d.appid == appid {
                        d.paused = false;
                        d.active = true;
                        d.queue = 0;
                    } else {
                        if d.active {
                            d.active = false;
                        }
                        if d.queue >= 0 {
                            d.queue += 1;
                        }
                    }
                }
            }
            Control::Cancel => m.items.retain(|d| d.appid != appid),
            Control::AcceptEula | Control::CancelInstall => {}
        })
    }

    /// Two owned games that are not installed, for the Not installed filter.
    pub fn owned(now: i64) -> Vec<Game> {
        let _ = now;
        [(1374490u64, "RuneScape: Dragonwilds", 44.4), (2399830, "ARK: Survival Ascended", 230.0)]
            .iter()
            .map(|(appid, title, gb)| {
                let cdn = format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}");
                Game {
                    id: format!("steam:{appid}"),
                    source: "steam".into(),
                    title: (*title).into(),
                    installed: false,
                    install_dir: None,
                    size_bytes: Some(gb * 1e9),
                    last_played: None,
                    playtime_minutes: None,
                    art: crate::data::model::Art { cover: vec![format!("{cdn}/library_600x900.jpg")], hero: vec![format!("{cdn}/library_hero.jpg")], logo: vec![format!("{cdn}/logo.png")], header: vec![format!("{cdn}/header.jpg")], icon: vec![] },
                    launch: None,
                    origin: "Steam".into(),
                    foreign: false,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dl(done: u64, total: u64, paused: bool, active: bool) -> Dl {
        Dl { appid: 7, title: "Seven".into(), done, total, paused, active, queue: if active { 0 } else { 2 }, update: false, rate: 12_300_000, eta: 2280 }
    }

    #[test]
    fn lines_read_plainly() {
        assert_eq!(detail(&dl(4_200_000_000, 44_400_000_000, false, true)), "4.2 GB of 44.4 GB \u{b7} 12.3 MB/s \u{b7} 38 min left");
        assert_eq!(detail(&dl(4_200_000_000, 44_400_000_000, true, false)), "4.2 GB of 44.4 GB");
        assert_eq!(status(&dl(1, 2, false, false)), "Queued");
        assert_eq!(fmt_eta(7500), "2 h 5 min left");
        assert_eq!(fmt_eta(30), "Less than a minute left");
        assert!(!detail(&dl(1, 2, false, true)).contains('\u{2014}'));
    }

    #[test]
    fn a_finished_download_posts_one_notice_and_rescans() {
        let mut st = State::default();
        let d = Dl { done: 990, total: 1000, active: true, ..dl(0, 0, false, true) };
        apply(&mut st, Some(Snapshot { items: vec![d], ..Default::default() }), 1.0);
        assert!(st.feat.downloads.notices.is_empty());
        assert_eq!(menu_line().as_deref(), Some("Seven, 99%"));
        apply(&mut st, Some(Snapshot::default()), 3.0);
        assert_eq!(st.feat.downloads.notices.len(), 1);
        assert_eq!(st.feat.downloads.notices[0].title, "Seven finished downloading");
        assert!(st.rescan_at.is_some());
        assert_eq!(menu_line(), None);
    }
}
