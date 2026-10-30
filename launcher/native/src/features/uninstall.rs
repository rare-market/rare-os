//! The uninstall sheet, for every kind of game and app in the library and for ROMs: "Uninstall
//! <game>?", the space it frees and the device's free space, whether to keep the saves when the
//! game's Wine prefix (or a web game's storage, or an app's data) holds some, then progress. When
//! it is done the game leaves the library in place (the grid keeps its scroll and focus moves to
//! the next tile) and a toast says what was freed.
//!
//! Steam games: Rare asks first, then Steam removes the game (steam://uninstall through
//! rare-launch's steam_dialog). Steam always shows its own confirm too; Rare takes the screen back
//! when Steam is done.

use crate::act::Act;
use crate::kit::format::fmt_size;
use crate::kit::{self, s, tok, Btn as B, BtnIcon, Ty, Ui, T};
use crate::page::{hash, Acts, Btn, Feedback, Fx, Page};
use crate::state::{Overlay, State, Tone};
use crate::data::model::UninstallInfo;
use rare_render::focus::hints::{hint, Hint, Key};
use rare_render::focus::{Focusable, Group};
use rare_render::view::El;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub const KIND: &str = "uninstall";

const GO: u32 = 1;
const CANCEL: u32 = 2;
const KEEP: u32 = 3;
const HIDE: u32 = 4;

/// What the sheet was opened for, set by `open` before the page exists.
#[derive(Clone)]
struct Ask {
    id: String,
    title: String,
    /// The game page it was opened from: Cancel goes back to it.
    back: Option<Overlay>,
}

static ASK: Mutex<Option<Ask>> = Mutex::new(None);

/// A ROM's name from the library's ES-DE data.
fn rom_name(st: &State, id: &str) -> Option<String> {
    let e = st.library.as_ref()?.emulation.as_ref()?;
    // The name as the Played last row shows it: "Pokemon Violet", not its file's tags.
    e.recent.iter().find(|r| r.uninstall_id().as_deref() == Some(id)).map(|r| {
        let clean = r.name.split(['[', '(']).next().unwrap_or(&r.name).trim_end();
        if clean.is_empty() { r.name.clone() } else { clean.to_string() }
    })
}

/// Opens the sheet for a library game or app, or a ROM ("rom:<system>/<path>").
pub fn open(st: &mut State, id: &str) {
    let title = st.game(id).map(|g| g.title.clone()).or_else(|| rom_name(st, id)).unwrap_or_else(|| "this game".into());
    let back = st.overlay.clone().filter(|o| matches!(o, Overlay::Game(_)));
    *ASK.lock().unwrap() = Some(Ask { id: id.to_string(), title, back });
    st.options = None;
    st.open(Some(Overlay::Other(KIND)));
}

/// True for what the sheet can be asked about: everything installed but Steam shortcuts (Steam
/// owns those) and apps that aren't Flatpaks (EmuDeck's launchers, desktop programs).
pub fn can_uninstall(g: &crate::data::model::Game) -> bool {
    // Cloud games stream from their service: nothing on this device to remove.
    g.installed && g.source != "cloud" && !g.id.starts_with("shortcut:") && g.launch.as_ref().map(|l| l.kind.as_str()) != Some("steamShortcut") && (g.source != "apps" || g.origin == "Flatpak")
}

/// The button's label on the game page: "Uninstall (12.3 GB)" when the size is known.
pub fn label(g: &crate::data::model::Game) -> String {
    match fmt_size(g.size_bytes) {
        z if z.is_empty() => "Uninstall".into(),
        z => format!("Uninstall ({z})"),
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Phase {
    Loading,
    Ready(UninstallInfo),
    Working { steam: bool },
    Failed(String),
}

#[derive(Default)]
struct Shared {
    phase: Mutex<Option<Phase>>,
    ver: AtomicU64,
    done: AtomicU64,
    total: AtomicU64,
}

impl Shared {
    fn set(&self, p: Phase) {
        *self.phase.lock().unwrap() = Some(p);
        self.ver.fetch_add(1, Ordering::Relaxed);
    }
    fn phase(&self) -> Phase {
        self.phase.lock().unwrap().clone().unwrap_or(Phase::Loading)
    }
}

pub struct UninstallPage {
    ask: Option<Ask>,
    sh: Arc<Shared>,
    asked: bool,
    keep: bool,
}

impl Default for UninstallPage {
    fn default() -> Self {
        UninstallPage { ask: ASK.lock().unwrap().clone(), sh: Default::default(), asked: false, keep: true }
    }
}

/// "This frees 12.3 GB." with the saves when they go too.
fn frees(info: &UninstallInfo, keep: bool) -> u64 {
    info.size + info.saves.filter(|_| !keep).map(|s| s.0).unwrap_or(0)
}

fn size(b: u64) -> String {
    let z = fmt_size(Some(b as f64));
    if z.is_empty() {
        "0 MB".into()
    } else {
        z
    }
}

/// The sheet's sentence about the saves.
fn saves_line(kind: &str, sz: u64) -> String {
    match kind {
        "web" => format!("Its saves on this device ({}). Keep them to pick up where you left off if you add it again.", size(sz)),
        "app" => format!("Its settings and data ({}). Keep them to have them back if you install it again.", size(sz)),
        _ => format!("Its saves and settings live in its Windows folder ({}). Keep them to pick up where you left off if you install it again.", size(sz)),
    }
}

impl UninstallPage {
    fn body(&self, ui: &mut Ui, acts: &mut Acts, ask: &Ask) -> (String, Option<String>, Vec<El>, Vec<El>) {
        let title = format!("Uninstall {}?", ask.title);
        let muted = |ui: &mut Ui, t: String| ui.text_s(s().wp(1.0), t, T::new(Ty::Body).c(tok::MUTED).wrap());
        match self.sh.phase() {
            Phase::Loading => {
                let cancel = kit::button(ui, B::new("Cancel", acts.tag(Act::Local(CANCEL))).def(true).hint("Cancel"));
                (title, None, vec![muted(ui, "Checking its size".into())], vec![cancel])
            }
            Phase::Failed(e) => {
                let close = kit::button(ui, B::new("Close", acts.tag(Act::Local(CANCEL))).def(true).hint("Close"));
                (format!("Couldn't uninstall {}", ask.title), None, vec![muted(ui, e)], vec![close])
            }
            Phase::Ready(info) => {
                let sub = info.free.map(|f| format!("{} free on this device", size(f)));
                let mut body = Vec::new();
                let n = frees(&info, self.keep);
                let line = if info.steam {
                    format!("This frees {}. Saves in Steam Cloud are kept. Steam asks you to confirm.", size(n))
                } else if n > 0 {
                    format!("This frees {}.", size(n))
                } else {
                    "It leaves your library.".into()
                };
                body.push(ui.text_s(s().wp(1.0), line, T::new(Ty::Body).wrap()));
                if let Some(p) = &info.place {
                    body.push(ui.text_s(s().wp(1.0).mt(0.35), format!("Installed in {}", crate::data::model::pretty_path(p)), T::new(Ty::Small).c(tok::MUTED).trunc()));
                }
                if let Some((sz, kind)) = info.saves {
                    body.push(ui.text_s(s().wp(1.0).mt(1.0), saves_line(kind, sz), T::new(Ty::Small).c(tok::MUTED).wrap()));
                    let keep = kit::button(ui, B::new("Keep saves", acts.tag(Act::Local(KEEP))).full_start().gap(0.75).icon(BtnIcon::Check(self.keep)).hint(if self.keep { "Delete them too" } else { "Keep them" }));
                    body.push(ui.bx(s().col().mt(0.6)).group(Group::new("uninstall-keep", [0.0; 4])).child(keep));
                }
                let go = kit::button(ui, B::new("Uninstall", acts.tag(Act::Local(GO))).primary().hint("Uninstall"));
                let cancel = kit::button(ui, B::new("Cancel", acts.tag(Act::Local(CANCEL))).def(true).hint("Cancel"));
                (title, sub, body, vec![go, cancel])
            }
            Phase::Working { steam } => {
                let (done, total) = (self.sh.done.load(Ordering::Relaxed), self.sh.total.load(Ordering::Relaxed));
                let track = if steam || total == 0 {
                    let name = ui.loop_name(&rare_render::anim::BREATHE, 1.3);
                    let seg = ui.bx(s().wp(0.35).h_px(3.0)).bg(tok::fg(0.8)).radius(ui.r(0.15)).name(name);
                    ui.bx(s().wp(1.0).h_px(3.0)).bg(tok::fg(0.15)).radius(ui.r(0.15)).clip().child(seg)
                } else {
                    let f = (done as f32 / total as f32).clamp(0.0, 1.0);
                    let seg = ui.bx(s().wp(f).h_px(3.0)).bg(tok::fg(0.8)).radius(ui.r(0.15));
                    ui.bx(s().wp(1.0).h_px(3.0)).bg(tok::fg(0.15)).radius(ui.r(0.15)).clip().child(seg)
                };
                let text = if steam {
                    "Confirm in Steam. Rare comes back when Steam is done.".to_string()
                } else if total > 0 {
                    format!("Removed {} of {}", size(done), size(total))
                } else {
                    "Removing".to_string()
                };
                let hide = kit::button(ui, B::new("Hide", acts.tag(Act::Local(HIDE))).def(true).hint("Hide"));
                (format!("Uninstalling {}", ask.title), None, vec![track, ui.text_s(s().mt(0.6), text, T::new(Ty::Small).c(tok::MUTED))], vec![hide])
            }
        }
    }

    fn cancel(&mut self, st: &mut State) {
        let back = self.ask.as_ref().and_then(|a| a.back.clone());
        *ASK.lock().unwrap() = None;
        st.open(back);
    }

    fn start(&mut self, fx: &mut Fx) {
        let Some(ask) = self.ask.clone() else { return };
        let Phase::Ready(info) = self.sh.phase() else { return };
        let keep = self.keep && info.saves.is_some();
        let freed = frees(&info, keep);
        self.sh.set(Phase::Working { steam: info.steam });
        let sh = self.sh.clone();
        fx.fb(Feedback::Select);
        fx.spawn(move |b| {
            let tick_sh = sh.clone();
            let mut tick = move |d: u64, t: u64| {
                tick_sh.total.store(t, Ordering::Relaxed);
                tick_sh.done.store(d, Ordering::Relaxed);
            };
            let r = b.uninstall(&ask.id, keep, &mut tick);
            Box::new(move |st: &mut State, now| match r {
                Ok(()) => {
                    remove_from_library(st, &ask.id);
                    // A ROM goes back to its emulator's page; a game's page has nothing left to show.
                    if st.overlay == Some(Overlay::Other(KIND)) {
                        st.open(ask.back.clone().filter(|_| ask.id.starts_with("rom:")));
                    } else if st.overlay == Some(Overlay::Game(ask.id.clone())) {
                        st.close();
                    }
                    *ASK.lock().unwrap() = None;
                    let msg = if freed > 0 { format!("{} uninstalled. Freed {}.", ask.title, size(freed)) } else { format!("{} uninstalled.", ask.title) };
                    st.toast(msg, Tone::Info, now);
                }
                Err(e) => {
                    log::warn!("uninstall {}: {e}", ask.id);
                    sh.set(Phase::Failed(e.clone()));
                    if st.overlay != Some(Overlay::Other(KIND)) {
                        st.toast(format!("Couldn't uninstall {}. {e}", ask.title), Tone::Error, now);
                    }
                }
            })
        });
    }
}

/// The game leaves the library where it is: the list keeps its order and scroll.
fn remove_from_library(st: &mut State, id: &str) {
    // Another copy of the game takes over its favourite and collections (they follow the group).
    if let Some(o) = st.copy_ids(id).into_iter().find(|c| c != id && st.game(c).is_some()) {
        if st.favs.iter().any(|f| f == id) && !st.favs.contains(&o) {
            st.favs.push(o.clone());
        }
        let mut moved = false;
        for c in st.user.collections.iter_mut() {
            if c.games.iter().any(|g| g == id) && !c.games.contains(&o) {
                c.games.push(o.clone());
                moved = true;
            }
        }
        if moved {
            st.user.save();
        }
    }
    let Some(l) = st.library.as_mut() else { return };
    l.games.retain(|g| g.id != id);
    if let Some(e) = l.emulation.as_mut() {
        e.recent.retain(|r| r.uninstall_id().as_deref() != Some(id));
    }
    st.favs.retain(|f| f != id);
    crate::app::save_favs(&st.favs);
    st.regroup();
    st.ver.library += 1;
}

impl Page for UninstallPage {
    fn stamp(&self, st: &State) -> u64 {
        let pct = match (self.sh.done.load(Ordering::Relaxed), self.sh.total.load(Ordering::Relaxed)) {
            (_, 0) => 0,
            (d, t) => d * 200 / t,
        };
        hash((self.sh.ver.load(Ordering::Relaxed), pct, self.keep, self.ask.as_ref().map(|a| a.id.clone()), st.ver.library))
    }

    fn tick(&mut self, st: &mut State, fx: &mut Fx, now: f64) -> Option<f64> {
        if !self.asked {
            self.asked = true;
            let Some(ask) = self.ask.clone() else {
                st.close();
                return None;
            };
            let sh = self.sh.clone();
            fx.spawn(move |b| {
                let r = b.uninstall_info(&ask.id);
                Box::new(move |_, _| {
                    sh.set(match r {
                        Ok(i) => Phase::Ready(i),
                        Err(e) if e == crate::data::backend::NOT_WIRED => Phase::Failed("Uninstalling isn't available on this device yet.".into()),
                        Err(e) => Phase::Failed(e),
                    })
                })
            });
        }
        matches!(self.sh.phase(), Phase::Working { .. }).then_some(now + 0.25)
    }

    fn view(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El {
        // The library may land after the sheet opened: the title follows it.
        let live = self.ask.as_ref().and_then(|a| st.game(&a.id).map(|g| g.title.clone()).or_else(|| rom_name(st, &a.id)));
        if let (Some(a), Some(t)) = (self.ask.as_mut(), live) {
            a.title = t;
        }
        let Some(ask) = self.ask.clone() else { return ui.bx(s()) };
        let (title, sub, body, footer) = self.body(ui, acts, &ask);
        kit::sheet(ui, &title, sub.as_deref(), body, footer, false, crate::app::SCOPE_OVERLAY)
    }

    fn act(&mut self, st: &mut State, fx: &mut Fx, code: u32) {
        match code {
            GO => self.start(fx),
            CANCEL => self.cancel(st),
            KEEP => {
                self.keep = !self.keep;
                fx.fb(Feedback::Toggle);
            }
            HIDE => st.close(),
            _ => {}
        }
    }

    fn pad(&mut self, st: &mut State, fx: &mut Fx, b: Btn, _cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        match b {
            Btn::B => {
                fx.fb(Feedback::Back);
                if matches!(self.sh.phase(), Phase::Working { .. }) {
                    st.close();
                } else {
                    self.cancel(st);
                }
                true
            }
            Btn::Lb | Btn::Rb | Btn::Lt | Btn::Rt | Btn::X | Btn::Y | Btn::Start | Btn::Select => true,
            _ => false,
        }
    }

    fn hints(&self, _st: &State) -> Vec<Hint> {
        vec![hint(Key::B, if matches!(self.sh.phase(), Phase::Working { .. }) { "Hide" } else { "Cancel" })]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frees_counts_the_saves_only_when_they_go() {
        let i = UninstallInfo { steam: false, size: 4_000_000_000, saves: Some((1_000_000_000, "prefix")), place: None, free: None };
        assert_eq!(frees(&i, true), 4_000_000_000);
        assert_eq!(frees(&i, false), 5_000_000_000);
        assert_eq!(size(frees(&i, false)), "5.0 GB");
    }

    #[test]
    fn steam_shortcuts_are_steams() {
        let mut g = crate::data::backend::mock_library(0, false).games.into_iter().find(|g| g.id.starts_with("steam:")).unwrap();
        assert!(can_uninstall(&g));
        assert!(label(&g).starts_with("Uninstall ("));
        g.id = "shortcut:1".into();
        assert!(!can_uninstall(&g));
    }

    #[test]
    fn sheet_opens_and_cancel_goes_back_to_the_game() {
        let mut st = State::default();
        st.library = Some(crate::data::backend::mock_library(0, false));
        let id = st.games()[0].id.clone();
        st.open(Some(Overlay::Game(id.clone())));
        open(&mut st, &id);
        assert_eq!(st.overlay, Some(Overlay::Other(KIND)));
        let mut p = UninstallPage::default();
        p.cancel(&mut st);
        assert_eq!(st.overlay, Some(Overlay::Game(id.clone())));
        remove_from_library(&mut st, &id);
        assert!(st.game(&id).is_none());
    }
}
