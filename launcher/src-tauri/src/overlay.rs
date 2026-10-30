//! Rare over a game: thin Tauri layer over the `rare-overlay` crate. The crate holds the focus
//! keeper, the gamescope atoms, the Switcher and guest mode; this file gives it the main window,
//! the runner and the modules that have not moved into crates yet (`TauriOverlay`), and keeps the
//! old `crate::overlay::...` paths and `#[tauri::command]`s working.

use crate::gamepad::Pad;
use rare_core::Pad as CorePad;
use rare_overlay::OverlayHost;
use std::sync::Arc;
#[cfg(target_os = "linux")]
use std::time::Duration;
use tauri::{AppHandle, Manager, WebviewWindow};

pub use rare_overlay::{appid, trigger, Front, Route, Session};
pub use rare_overlay::is_open;

pub(crate) fn to_core(p: Pad) -> CorePad {
    match p {
        Pad::Up => CorePad::Up,
        Pad::Down => CorePad::Down,
        Pad::Left => CorePad::Left,
        Pad::Right => CorePad::Right,
        Pad::A => CorePad::A,
        Pad::B => CorePad::B,
        Pad::X => CorePad::X,
        Pad::Y => CorePad::Y,
        Pad::Lb => CorePad::Lb,
        Pad::Rb => CorePad::Rb,
        Pad::Lt => CorePad::Lt,
        Pad::Rt => CorePad::Rt,
        Pad::Start => CorePad::Start,
        Pad::Select => CorePad::Select,
        Pad::Guide => CorePad::Guide,
        Pad::GuideHold => CorePad::GuideHold,
        Pad::ExitHold => CorePad::ExitHold,
        Pad::Qam => CorePad::Qam,
    }
}

fn from_core(p: CorePad) -> Pad {
    match p {
        CorePad::Up => Pad::Up,
        CorePad::Down => Pad::Down,
        CorePad::Left => Pad::Left,
        CorePad::Right => Pad::Right,
        CorePad::A => Pad::A,
        CorePad::B => Pad::B,
        CorePad::X => Pad::X,
        CorePad::Y => Pad::Y,
        CorePad::Lb => Pad::Lb,
        CorePad::Rb => Pad::Rb,
        CorePad::Lt => Pad::Lt,
        CorePad::Rt => Pad::Rt,
        CorePad::Start => Pad::Start,
        CorePad::Select => Pad::Select,
        CorePad::Guide => Pad::Guide,
        CorePad::GuideHold => Pad::GuideHold,
        CorePad::ExitHold => Pad::ExitHold,
        CorePad::Qam => Pad::Qam,
    }
}

/// The front end's side of the overlay: the main window, the runner, what is in front.
struct TauriOverlay(AppHandle);

impl TauriOverlay {
    fn main(&self) -> Option<WebviewWindow> {
        self.0.get_webview_window("main")
    }
    fn runner(&self) -> tauri::State<'_, crate::AppState> {
        self.0.state::<crate::AppState>()
    }
}

/// The X11 window id of a Tauri window (GTK on X11). None on Wayland or other platforms.
#[cfg(target_os = "linux")]
fn xid(win: &WebviewWindow) -> Option<u64> {
    use gtk::prelude::*;
    let (tx, rx) = std::sync::mpsc::channel();
    let w = win.clone();
    win.run_on_main_thread(move || {
        let id = w
            .gtk_window()
            .ok()
            .and_then(|g| g.window())
            .and_then(|gw| gw.downcast::<gdkx11::X11Window>().ok())
            .map(|x| x.xid() as u64);
        let _ = tx.send(id);
    })
    .ok()?;
    rx.recv_timeout(Duration::from_millis(800)).ok().flatten()
}

#[cfg(not(target_os = "linux"))]
fn xid(_win: &WebviewWindow) -> Option<u64> {
    None
}

impl OverlayHost for TauriOverlay {
    fn log(&self, msg: &str) {
        crate::gamepad::log(msg);
    }

    fn main_exists(&self) -> bool {
        self.main().is_some()
    }
    fn main_xid(&self) -> Option<u64> {
        self.main().and_then(|w| xid(&w))
    }
    fn window_xid(&self, label: &str) -> Option<u64> {
        self.0.get_webview_window(label).and_then(|w| xid(&w))
    }
    fn main_show(&self) {
        if let Some(w) = self.main() {
            let _ = w.unminimize();
            let _ = w.show();
        }
    }
    fn main_focus(&self) {
        if let Some(w) = self.main() {
            let _ = w.set_focus();
        }
    }
    fn main_focused(&self) -> bool {
        self.main().and_then(|w| w.is_focused().ok()).unwrap_or(true)
    }
    fn main_always_on_top(&self, on: bool) {
        if let Some(w) = self.main() {
            let _ = w.set_always_on_top(on);
        }
    }
    fn main_minimize(&self) {
        if let Some(w) = self.main() {
            let _ = w.minimize();
        }
    }
    fn visible_other_windows(&self) -> Vec<String> {
        self.0.webview_windows().into_iter().filter(|(l, w)| l != "main" && w.is_visible().unwrap_or(false)).map(|(l, _)| l).collect()
    }
    fn hide_window(&self, label: &str) -> bool {
        self.0.get_webview_window(label).is_some_and(|w| w.hide().is_ok())
    }
    fn show_window(&self, label: &str) {
        if let Some(w) = self.0.get_webview_window(label) {
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
    fn hide_web_window(&self) {
        if let Some(w) = self.0.get_webview_window(crate::webgame::LABEL) {
            let _ = w.hide();
        }
    }
    fn wayland_client(&self) -> bool {
        crate::render::wayland_client()
    }

    fn running(&self) -> Vec<String> {
        self.runner().runner.running()
    }
    fn any_running(&self) -> bool {
        self.runner().runner.is_any_running()
    }
    fn child_pids(&self) -> Vec<(String, u32)> {
        self.runner().runner.child_pids()
    }
    fn with_descendants(&self, pids: &[u32]) -> Vec<u32> {
        #[cfg(target_os = "linux")]
        return crate::launch::with_descendants(pids);
        #[cfg(not(target_os = "linux"))]
        pids.to_vec()
    }
    fn focusable_apps_seen(&self, apps: &[u64]) {
        let runner = &self.runner().runner;
        // A game that ended hands the screen back without waiting for a button press.
        crate::apps::front::sync(&runner.running());
        runner.mark_shown(&crate::launch::shown_runs(&runner.app_ids(), apps));
    }

    fn pointer_active(&self) -> bool {
        crate::apps::front::pointer_active()
    }
    fn launcher_front(&self) -> bool {
        crate::apps::front::launcher_front()
    }
    fn steam_front(&self) -> bool {
        crate::apps::front::steam_front()
    }
    fn front_run(&self) -> Option<String> {
        crate::apps::front::front_run()
    }
    fn to_launcher(&self) {
        crate::apps::front::to_launcher();
    }
    fn to_front(&self, run: &str) {
        crate::apps::front::to_front(run);
    }

    fn web_visible(&self) -> bool {
        crate::webgame::visible()
    }
    fn web_current(&self) -> Option<String> {
        rare_webgame::state().current.lock().unwrap().clone().map(|(id, _)| id)
    }
    fn web_pad(&self, button: CorePad, down: bool) {
        crate::webgame::pad(&self.0, from_core(button), down);
    }
    fn web_suspend(&self) {
        crate::webgame::suspend();
    }
    fn web_resume(&self) {
        crate::webgame::resume(&self.0);
    }

    fn input_surface(&self, up: bool) {
        crate::inputplumber::surface(up);
    }
    fn pointer_prepare(&self) {
        crate::apps::pointer::prepare();
    }
    fn pointer_combo(&self, keys: &[u16]) {
        crate::apps::pointer::combo(keys);
    }

    fn thaw(&self, run: &str) {
        crate::freeze::thaw(run);
    }
    fn is_frozen(&self, run: &str) -> bool {
        crate::freeze::is_frozen(run)
    }
    fn max_frozen(&self) -> usize {
        crate::freeze::MAX_FROZEN
    }
    fn freeze_for_switch(&self, run: &str) -> Option<serde_json::Value> {
        let target = self.runner().runner.freeze_target(run);
        let out = crate::freeze::freeze(run, target);
        crate::gamepad::log(&format!("[switcher] leaving {run}: {out:?}"));
        match &out {
            crate::freeze::Outcome::Full { .. } => serde_json::to_value(&out).ok(),
            _ => None,
        }
    }
    fn freeze_sync(&self, running: &[String]) -> Option<String> {
        crate::freeze::sync(running)
    }
    fn freeze_drop_pending(&self) {
        crate::freeze::drop_pending();
    }

    fn steam_url(&self, url: &str) -> Result<(), String> {
        crate::launch::steam_url(url)
    }
}

/// Hands the crate this app's windows and runner. Idempotent; every wrapper that has an
/// `AppHandle` calls it first, so the crate works whichever entry point runs first.
pub fn ensure(app: &AppHandle) {
    if !rare_overlay::installed() {
        rare_overlay::install(crate::rare_ctx::ctx(app), Arc::new(TauriOverlay(app.clone())));
    }
}

/// gamescope's atoms and the base-layer keeper, with the two entry points that take a window.
pub mod gamescope {
    pub use rare_overlay::gamescope::*;
    use tauri::{Manager, WebviewWindow};

    /// Tags a Rare window so gamescope can focus it (Rare OS game mode only).
    pub fn claim(win: &WebviewWindow) {
        super::ensure(win.app_handle());
        rare_overlay::gamescope::claim(win.label());
    }

    /// Rare OS game mode, at start: tags the main window and keeps Rare's id in the base-layer list.
    pub fn session_start(main: &WebviewWindow) {
        super::ensure(main.app_handle());
        rare_overlay::gamescope::session_start();
    }
}

pub mod guest {
    /// For the UI's hints (lib/guest.ts).
    #[tauri::command]
    pub fn guest_mode() -> bool {
        rare_overlay::guest::guest()
    }
}

pub mod steampages {
    #[tauri::command]
    pub fn rare_menu_steam_page(app: tauri::AppHandle, page: String) -> Result<(), String> {
        super::ensure(&app);
        rare_overlay::steampages::open_page(&page)
    }
}

pub mod switcher {
    pub use rare_overlay::switcher::*;
    use tauri::AppHandle;

    pub fn start(app: AppHandle) {
        super::ensure(&app);
        rare_overlay::switcher::start();
    }

    #[tauri::command]
    pub fn switcher_state(app: AppHandle) -> SwitcherState {
        super::ensure(&app);
        rare_overlay::switcher::switcher_state()
    }

    #[tauri::command]
    pub fn switcher_switch(app: AppHandle, to: Option<String>) -> Result<(), String> {
        super::ensure(&app);
        rare_overlay::switcher::switcher_switch(to)
    }

    /// "Keep it running": the game that didn't fit stays running.
    #[tauri::command]
    pub fn freeze_keep_pending(app: AppHandle) {
        super::ensure(&app);
        rare_overlay::switcher::freeze_keep_pending();
    }
}

pub fn init() {
    rare_overlay::init();
}

/// Sees every press and release first.
pub fn route(app: &AppHandle, button: Pad, down: bool) -> Route {
    ensure(app);
    rare_overlay::route(to_core(button), down)
}

/// Called on every input pass: opens the menu when the guest-mode View hold completes.
pub fn tick(app: &AppHandle) {
    ensure(app);
    rare_overlay::tick();
}

pub fn open(app: &AppHandle, game_id: Option<String>) -> Result<Session, String> {
    ensure(app);
    rare_overlay::open(game_id)
}

pub fn close(app: &AppHandle) -> Result<(), String> {
    ensure(app);
    rare_overlay::close()
}

pub fn quick_chord(app: &AppHandle) {
    ensure(app);
    rare_overlay::quick_chord();
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn power_menu(app: &AppHandle) {
    ensure(app);
    rare_overlay::power_menu();
}

pub fn exit_chord(app: &AppHandle) {
    ensure(app);
    rare_overlay::exit_chord();
}

pub fn bring_main_front(app: &AppHandle) -> Option<Front> {
    ensure(app);
    rare_overlay::bring_main_front()
}

pub fn release_front(front: Front) {
    rare_overlay::release_front(front);
}

#[tauri::command]
pub fn steam_menu(app: AppHandle) -> Result<(), String> {
    ensure(&app);
    rare_overlay::steam_menu()
}

#[tauri::command]
pub fn overlay_open(app: AppHandle, game_id: Option<String>) -> Result<Session, String> {
    open(&app, game_id)
}

#[tauri::command]
pub fn overlay_close(app: AppHandle) -> Result<(), String> {
    close(&app)
}

#[tauri::command]
pub fn overlay_state() -> Option<Session> {
    rare_overlay::overlay_state()
}
