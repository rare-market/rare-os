//! Android in the native launcher (rare-android, Waydroid): Library > Android's setup, the F-Droid
//! search and installs, notices when a job ends, the Storage rows, and touch controls: the pad
//! diverted to taps while an Android game with touch spots is in front, and their editor.
//!
//! Actions are `Act::Legacy("android:...")`: `android:setup`, `android:find`,
//! `android:get:<package>|<name>`, `android:touch:<library id>`.

use crate::act::Act;
use crate::kit::{self, s, tok, Btn as B, Ty, Ui, T};
use crate::page::{hash, Acts, Btn, Feedback, Fx, Page};
use crate::state::{OptionItem, OptionsSpec, Overlay, State, Tone};
use rare_android::jobs::{self, Kind};
use rare_android::Status;
use rare_render::focus::hints::{hint, Hint, Key};
use rare_render::focus::{Focusable, Group};
use rare_render::view::{key, El};
use std::sync::Mutex;

/// The touch controls editor's overlay kind.
pub const TOUCH: &str = "android-touch";

/// Once at start: the scripts, change notices to the frame loop, library rescans after a job, and
/// the pad's touch sink.
pub fn boot() {
    if let Err(e) = rare_android::ensure_files() {
        log::warn!("android files: {e}");
    }
    rare_android::on_change(super::bus::wake);
    jobs::on_library(|| super::bus::ctx().send(rare_core::Event::LibraryChanged));
    rare_input::ip::set_divert_sink(|p, down| match pad_name(p) {
        Some(b) => rare_android::touch::press(b, down),
        None => false,
    });
}

fn pad_name(p: rare_input::Pad) -> Option<&'static str> {
    use rare_input::Pad;
    Some(match p {
        Pad::A => "a",
        Pad::B => "b",
        Pad::X => "x",
        Pad::Y => "y",
        Pad::Up => "up",
        Pad::Down => "down",
        Pad::Left => "left",
        Pad::Right => "right",
        Pad::Lb => "lb",
        Pad::Rb => "rb",
        Pad::Lt => "lt",
        Pad::Rt => "rt",
        Pad::Start => "start",
        Pad::Select => "select",
        _ => return None,
    })
}

/// Moves on every Android change (pages put it in their stamp).
pub fn ver() -> u64 {
    rare_android::ver()
}

/// Android's status, read at most every two seconds (it looks at a few files).
pub fn status() -> Status {
    static C: Mutex<Option<(f64, u64, Status)>> = Mutex::new(None);
    let now = rare_render::clock::secs();
    let v = ver();
    let mut g = C.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((t, cv, s)) = *g {
        if cv == v && now - t < 2.0 {
            return s;
        }
    }
    let s = rare_android::status();
    *g = Some((now, v, s));
    s
}

/// The search typed in Library > Android, and its answer when it lands.
static SEARCH: Mutex<Option<(String, Option<Result<Vec<rare_android::fdroid::Hit>, String>>)>> = Mutex::new(None);
const FIND: &str = "android-find";

/// Runs an `android:` action. True when it was one.
pub fn act(st: &mut State, a: &str, now: f64) -> bool {
    let Some(rest) = a.strip_prefix("android:") else { return false };
    let (verb, arg) = rest.split_once(':').unwrap_or((rest, ""));
    match verb {
        "setup" => {
            if let Err(e) = jobs::start(Kind::Setup) {
                st.toast(e, Tone::Info, now);
            }
        }
        "find" => st.ask_text(FIND, "Find Android games on F-Droid", "", Some("Game name")),
        "get" => {
            let (package, name) = arg.split_once('|').unwrap_or((arg, arg));
            match jobs::start(Kind::Install { package: package.into(), name: name.into() }) {
                Ok(()) => st.toast(format!("Getting {name}"), Tone::Info, now),
                Err(e) => st.toast(e, Tone::Info, now),
            }
        }
        "touch" => open_touch(st, arg),
        _ => {}
    }
    true
}

/// Every update: the keyboard's search answer, search results, job notices, touch controls.
pub fn tick_state(st: &mut State) {
    let now = rare_render::clock::secs();
    if let Some(Some(q)) = st.take_answer(FIND) {
        let q = q.trim().to_string();
        if !q.is_empty() {
            *SEARCH.lock().unwrap() = Some((q.clone(), None));
            st.toast(format!("Searching F-Droid for {q}"), Tone::Info, now);
            std::thread::spawn(move || {
                let r = rare_android::fdroid::search(&q);
                if let Some(s) = SEARCH.lock().unwrap().as_mut().filter(|s| s.0 == q) {
                    s.1 = Some(r);
                }
                super::bus::wake();
            });
        }
    }
    let landed = {
        let mut g = SEARCH.lock().unwrap();
        if g.as_ref().is_some_and(|s| s.1.is_some()) {
            g.take()
        } else {
            None
        }
    };
    if let Some((q, Some(r))) = landed {
        match r {
            Ok(hits) if hits.is_empty() => st.toast(format!("Nothing on F-Droid for {q}"), Tone::Info, now),
            Ok(hits) => st.options = Some(results_sheet(&q, &hits)),
            Err(e) => {
                log::warn!("f-droid search: {e}");
                st.toast("Couldn't reach F-Droid. Check the network and try again.", Tone::Error, now);
            }
        }
    }
    for e in jobs::take_ended() {
        match (&e.kind, e.result) {
            (_, Ok(msg)) => st.toast(msg, Tone::Info, now),
            (Kind::Setup, Err(m)) => st.toast(m, Tone::Error, now),
            (_, Err(m)) => st.toast(m, Tone::Error, now),
        }
    }
    touch_tick(st);
}

/// The F-Droid results as a sheet: up to 20, the name and what it is.
fn results_sheet(q: &str, hits: &[rare_android::fdroid::Hit]) -> OptionsSpec {
    let items = hits
        .iter()
        .take(20)
        .map(|h| {
            let label = if h.summary.is_empty() { h.name.clone() } else { format!("{} \u{b7} {}", h.name, h.summary) };
            let name = h.name.replace('|', " ");
            OptionItem { label, on: None, act: Act::Legacy(format!("android:get:{}|{}", h.package, name)) }
        })
        .collect();
    OptionsSpec { title: format!("F-Droid: {q}"), sub: Some("Free and open source. Pick one to install.".into()), items }
}

/// The Android job's line and bar for an app (an install or uninstall of it), or setup's.
pub fn job_for(id: Option<&str>) -> Option<jobs::Job> {
    jobs::current().filter(|j| j.kind.id().as_deref() == id)
}

/// Touch controls follow the game in front: on while an Android game with spots is in front
/// and no Rare surface is up.
fn touch_tick(_st: &State) {
    let front = rare_launch::apps::front::front();
    let pkg = front.as_deref().and_then(rare_android::package_of).map(String::from);
    let surface = super::ingame::surface();
    match pkg {
        Some(p) => {
            let on = rare_android::touch::begin(&p);
            if surface {
                rare_android::touch::lift_all();
            }
            rare_input::ip::divert(on && !surface);
        }
        None => {
            rare_android::touch::end();
            rare_input::ip::divert(false);
        }
    }
}

/// The Rare menu came up over an Android game: its frame is kept for the touch editor.
pub fn on_overlay(game: Option<&str>, frame: Option<&std::path::Path>) {
    if let (Some(pkg), Some(f)) = (game.and_then(rare_android::package_of), frame) {
        rare_android::touch::keep_frame(pkg, f);
    }
}

/// Settings > Storage: each Android app with its size, and Android itself.
pub fn storage_items() -> Vec<rare_library::storage::Item> {
    use rare_library::storage::{Item, ItemKind};
    if rare_android::status() != Status::Ready {
        return Vec::new();
    }
    let mut v: Vec<Item> = Vec::new();
    let mut apps = 0u64;
    for (pkg, name, _) in rare_android::entries() {
        let bytes = rare_android::app_size(&pkg).unwrap_or(0);
        apps += bytes;
        let id = format!("android:{pkg}");
        let kind = if rare_android::is_store(&id) { ItemKind::App } else { ItemKind::Game };
        v.push(Item { key: id.clone(), title: name, kind, source: "Android".into(), game_id: Some(id), path: rare_android::apk_dirs(&pkg).into_iter().next(), bytes, no_move: Some("Android apps stay inside Android on this device.".into()) });
    }
    let total = rare_android::total_size();
    v.push(Item {
        key: "android:system".into(),
        title: "Android".into(),
        kind: ItemKind::App,
        source: "Android".into(),
        game_id: None,
        path: None,
        bytes: total.saturating_sub(apps),
        no_move: Some("Android's system and data stay on this device.".into()),
    });
    v
}

/// Uninstall details for an Android app (the sheet's "This frees").
pub fn uninstall_info(id: &str) -> Result<crate::data::model::UninstallInfo, String> {
    let pkg = rare_android::package_of(id).ok_or("Not an Android app")?;
    Ok(crate::data::model::UninstallInfo { steam: false, size: rare_android::app_size(pkg).unwrap_or(0), saves: None, place: None, free: None })
}

/// Uninstalls an Android app (waits for the job; call off the UI thread).
pub fn uninstall(id: &str) -> Result<(), String> {
    let pkg = rare_android::package_of(id).ok_or("Not an Android app")?;
    let name = jobs::name_of(id);
    jobs::run_wait(Kind::Remove { package: pkg.to_string(), name }).map(|_| ())
}

// ---- the touch controls editor ----

static EDIT: Mutex<Option<(String, String)>> = Mutex::new(None);

fn open_touch(st: &mut State, id: &str) {
    let Some(pkg) = rare_android::package_of(id) else { return };
    let title = st.game(id).map(|g| g.title.clone()).unwrap_or_else(|| pkg.to_string());
    *EDIT.lock().unwrap() = Some((pkg.to_string(), title));
    st.options = None;
    st.open(Some(Overlay::Other(TOUCH)));
}

/// Touch controls: the buttons down the left with their spots, the game's last frame on the
/// right with the spots on it. A on a button places its spot (move with the d-pad, A to set,
/// B to cancel); X clears it.
pub struct TouchPage {
    pkg: String,
    title: String,
    map: rare_android::touch::Map,
    /// Placing a spot for button index i at (x, y).
    placing: Option<(usize, f32, f32)>,
    fine: bool,
    focus: usize,
    want: Option<u32>,
    tags: Vec<u32>,
}

impl Default for TouchPage {
    fn default() -> Self {
        let (pkg, title) = EDIT.lock().unwrap().clone().unwrap_or_default();
        let map = rare_android::touch::map(&pkg);
        TouchPage { pkg, title, map, placing: None, fine: false, focus: 0, want: None, tags: Vec::new() }
    }
}

const BUTTONS: [(&str, &str); 14] = rare_android::touch::BUTTONS;

fn pct(v: f32) -> String {
    format!("{}%", (v * 100.0).round() as i32)
}

impl TouchPage {
    fn save(&mut self, st: &mut State, now: f64) {
        if let Err(e) = rare_android::touch::set_map(&self.pkg, self.map.clone()) {
            st.toast(format!("Couldn't save the touch controls. {e}"), Tone::Error, now);
        }
    }

    /// The preview: the frame (or a plain canvas) with every spot, and the cursor while placing.
    fn canvas(&self, ui: &mut Ui, cw: f32, ch: f32) -> El {
        let frame = rare_android::touch::frame_path(&self.pkg);
        let mut c = ui.bx(s().wh(cw, ch)).bg(tok::L2).radius(ui.r(0.5)).clip();
        if frame.is_file() {
            c = c.child(kit::art(ui, s().fill(), &[frame.to_string_lossy().into_owned()], cw, ch, kit::ArtFit::Cover(0.5), None));
        } else {
            let note = ui.text_s(s().wp(0.8), "Open the game, press Rare, then come back here to see it behind the spots.", T::new(Ty::Small).c(tok::MUTED).center().wrap());
            c = c.child(ui.bx(s().fill().col().items_center().justify_center()).child(note));
        }
        let d = 2.6;
        for (i, (b, label)) in BUTTONS.iter().enumerate() {
            let pos = match self.placing {
                Some((pi, x, y)) if pi == i => Some((x, y)),
                _ => self.map.get(*b).map(|p| (p[0], p[1])),
            };
            let Some((x, y)) = pos else { continue };
            let live = self.placing.is_some_and(|p| p.0 == i);
            let left = (x * cw - d / 2.0).clamp(0.0, cw - d);
            let top = (y * ch - d / 2.0).clamp(0.0, ch - d);
            let fill = if live { tok::FG } else { tok::BG.alpha(0.78) };
            let ink = if live { tok::BG } else { tok::FG };
            let dot = ui
                .bx(s().abs(Some(top), None, None, Some(left)).size(d).row().items_center().justify_center())
                .bg(fill)
                .radius(ui.r(d / 2.0))
                .border(2.0, tok::FG.alpha(if live { 1.0 } else { 0.85 }))
                .child(ui.text(*label, T::new(Ty::Label).w(700).c(ink)));
            c = c.child(dot.key(key(b)));
        }
        c
    }
}

impl Page for TouchPage {
    fn stamp(&self, _st: &State) -> u64 {
        hash((&self.pkg, format!("{:?}{:?}", self.map, self.placing), self.fine))
    }

    fn view(&mut self, ui: &mut Ui, _st: &State, acts: &mut Acts) -> El {
        let w = ui.w / ui.rem;
        let h = ui.h / ui.rem;
        let list_w = 21.0;
        let (sw, sh) = rare_android::screen();
        let room_w = (w - list_w - 2.5 * 3.0).max(10.0);
        let room_h = (h - 3.0 - 2.5 - 6.0).max(6.0);
        let aspect = sw as f32 / sh.max(1) as f32;
        let (cw, ch) = if room_w / room_h > aspect { (room_h * aspect, room_h) } else { (room_w, room_w / aspect) };
        self.tags.clear();
        let mut rows: Vec<El> = Vec::new();
        for (i, (b, label)) in BUTTONS.iter().enumerate() {
            let tag = acts.tag(Act::Local(i as u32));
            self.tags.push(tag);
            let spot = match self.placing {
                Some((pi, x, y)) if pi == i => format!("{}, {}", pct(x), pct(y)),
                _ => self.map.get(*b).map(|p| format!("{}, {}", pct(p[0]), pct(p[1]))).unwrap_or_else(|| "Not set".into()),
            };
            let set = self.map.contains_key(*b);
            let row = ui
                .bx(s().row().items_center().gap(0.75).h(2.6).px(0.9).wp(1.0))
                .radius(ui.r(0.45))
                .child(ui.text_s(s().w(4.0), *label, T::new(Ty::Body).w(600)))
                .child(ui.text_s(s().grow().min0(), spot, T::new(Ty::Small).c(if set { tok::fg(0.85) } else { tok::MUTED }).trunc()));
            let row = kit::nav(kit::row_look(row), tag, Some(if self.placing.is_some() { "Set" } else { "Set spot" }), set.then_some("Clear"), i == 0);
            rows.push(row.key(key(b)));
        }
        let list = ui.bx(s().col().w(list_w).gap(0.15)).group(Group::new("android-touch", [0.0; 4]).remember()).kids(rows);
        let sub = match self.placing {
            Some((i, _, _)) => format!("Placing {}: move with the d-pad{}, A to set, B to cancel", BUTTONS[i].1, if self.fine { " (fine)" } else { "" }),
            None => "Each button presses its spot on the screen while you play. Buttons with no spot do nothing.".into(),
        };
        let head = ui
            .bx(s().col().gap(0.3).mb(1.25))
            .child(ui.text_s(s().wp(1.0).min0(), format!("Touch controls \u{b7} {}", self.title), T::new(Ty::Row).w(600).trunc()))
            .child(ui.text_s(s().wp(1.0), sub, T::new(Ty::Small).c(tok::MUTED).trunc()));
        let done = kit::button(ui, B::new("Done", acts.tag(Act::Local(100))).sm().hint("Done"));
        let body = ui.bx(s().row().gap(2.5).items_start()).child(list).child(self.canvas(ui, cw, ch));
        ui.bx(s().abs(Some(0.0), Some(0.0), Some(3.0), Some(0.0)).col().px(2.5).pt(2.0))
            .scope(crate::app::SCOPE_OVERLAY)
            .bg(tok::BG)
            .child(ui.bx(s().row().items_start()).child(ui.bx(s().grow().min0()).child(head)).child(done))
            .child(body)
    }

    fn act(&mut self, st: &mut State, fx: &mut Fx, code: u32) {
        let now = rare_render::clock::secs();
        if code == 100 {
            st.close();
            return;
        }
        let i = code as usize;
        if i >= BUTTONS.len() {
            return;
        }
        if let Some((pi, x, y)) = self.placing.take() {
            self.map.insert(BUTTONS[pi].0.to_string(), [x, y]);
            self.save(st, now);
            fx.fb(Feedback::Select);
            return;
        }
        let (x, y) = self.map.get(BUTTONS[i].0).map(|p| (p[0], p[1])).unwrap_or((0.5, 0.5));
        self.placing = Some((i, x, y));
        fx.fb(Feedback::Open);
    }

    fn pad(&mut self, st: &mut State, fx: &mut Fx, b: Btn, _cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        let now = rare_render::clock::secs();
        if let Some((i, x, y)) = self.placing {
            let step = if self.fine { 0.005 } else { 0.02 };
            let (nx, ny) = match b {
                Btn::Left => (x - step, y),
                Btn::Right => (x + step, y),
                Btn::Up => (x, y - step),
                Btn::Down => (x, y + step),
                Btn::Y => {
                    self.fine = !self.fine;
                    return true;
                }
                Btn::B => {
                    self.placing = None;
                    fx.fb(Feedback::Back);
                    return true;
                }
                Btn::A => {
                    self.map.insert(BUTTONS[i].0.to_string(), [x, y]);
                    self.placing = None;
                    self.save(st, now);
                    fx.fb(Feedback::Select);
                    return true;
                }
                _ => return true,
            };
            self.placing = Some((i, nx.clamp(0.0, 1.0), ny.clamp(0.0, 1.0)));
            fx.fb(Feedback::Move);
            return true;
        }
        match b {
            Btn::X => {
                if self.map.remove(BUTTONS[self.focus.min(BUTTONS.len() - 1)].0).is_some() {
                    self.save(st, now);
                    fx.fb(Feedback::Toggle);
                }
                true
            }
            Btn::Start => {
                st.close();
                true
            }
            Btn::Lb | Btn::Rb | Btn::Lt | Btn::Rt | Btn::Y | Btn::Select => true,
            Btn::B => {
                fx.fb(Feedback::Back);
                st.close();
                true
            }
            _ => false,
        }
    }

    fn hints(&self, _st: &State) -> Vec<Hint> {
        if self.placing.is_some() {
            vec![hint(Key::A, "Set"), hint(Key::Y, if self.fine { "Coarse" } else { "Fine" }), hint(Key::B, "Cancel")]
        } else {
            vec![hint(Key::A, "Set spot"), hint(Key::X, "Clear"), hint(Key::B, "Done")]
        }
    }

    fn focused(&mut self, cur: Option<&Focusable>, _acts: &Acts, _now: f64) -> bool {
        if let Some(i) = cur.and_then(|c| self.tags.iter().position(|t| *t == c.tag)) {
            if i != self.focus {
                self.focus = i;
            }
        }
        false
    }

    fn take_focus(&mut self, _cur: Option<&Focusable>) -> Option<u32> {
        self.want.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_install_by_package() {
        let hits = vec![rare_android::fdroid::Hit { package: "org.a.game".into(), name: "A | Game".into(), summary: "Fun".into(), icon: None }];
        let o = results_sheet("game", &hits);
        assert_eq!(o.items.len(), 1);
        assert_eq!(o.items[0].label, "A | Game \u{b7} Fun");
        assert!(matches!(&o.items[0].act, Act::Legacy(a) if a == "android:get:org.a.game|A   Game"));
    }
}
