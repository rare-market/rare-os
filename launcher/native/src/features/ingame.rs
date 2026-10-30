//! Rare over a running game, on the device (docs/OVERLAY.md): the native launcher is rare-overlay's
//! front end, as the Tauri shell is (src-tauri/src/overlay.rs). The crate keeps the focus keeper,
//! the gamescope base-layer control and its keeper, the frame capture, the Switcher's frames and
//! quick resume; this file gives it the window, the runner and what is in front, and sends the
//! pad's system buttons through it. The app shows what its `overlay` events say (app/ingame_glue.rs).
//!
//! Only the device build installs it (not fixtures, not a bench build next to the live launcher,
//! which sets RR_NO_IP): it takes gamescope's base layer.

use rare_core::Pad;
use rare_overlay::OverlayHost;
use rare_render::winit::window::Window;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

static WINDOW: OnceLock<Arc<Window>> = OnceLock::new();
static ON: AtomicBool = AtomicBool::new(false);
/// rare-overlay says a Rare surface is over a game (InputPlumber intercepts the whole pad).
static SURFACE: AtomicBool = AtomicBool::new(false);

/// The overlay is the native launcher's (the device, not a bench build).
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// A Rare surface is over a game (rare-overlay's `input_surface`).
pub fn surface() -> bool {
    SURFACE.load(Ordering::Relaxed)
}

fn runner() -> &'static rare_launch::launch::Runner {
    crate::data::backend::runner()
}

/// stderr (native.log), and the input log while $XDG_RUNTIME_DIR/rare-debug-input exists.
fn log(msg: &str) {
    rare_input::log(msg);
}

/// The X11 id of our window (we are an Xwayland window under gamescope).
fn xid() -> Option<u64> {
    use rare_render::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let w = WINDOW.get()?;
    match w.window_handle().ok()?.as_raw() {
        RawWindowHandle::Xlib(h) => Some(h.window as u64),
        RawWindowHandle::Xcb(h) => Some(h.window.get() as u64),
        _ => None,
    }
}

fn wayland() -> bool {
    use rare_render::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    WINDOW.get().and_then(|w| w.window_handle().ok().map(|h| matches!(h.as_raw(), RawWindowHandle::Wayland(_)))).unwrap_or(false)
}

struct NativeOverlay;

impl OverlayHost for NativeOverlay {
    fn log(&self, msg: &str) {
        log(msg);
    }
    fn main_exists(&self) -> bool {
        WINDOW.get().is_some()
    }
    fn main_xid(&self) -> Option<u64> {
        xid()
    }
    fn window_xid(&self, label: &str) -> Option<u64> {
        if label == "main" {
            xid()
        } else {
            None
        }
    }
    fn main_show(&self) {
        if let Some(w) = WINDOW.get() {
            w.set_minimized(false);
            w.set_visible(true);
        }
    }
    fn main_focus(&self) {
        if let Some(w) = WINDOW.get() {
            w.focus_window();
        }
    }
    fn main_focused(&self) -> bool {
        WINDOW.get().map(|w| w.has_focus()).unwrap_or(true)
    }
    fn main_always_on_top(&self, on: bool) {
        use rare_render::winit::window::WindowLevel;
        if let Some(w) = WINDOW.get() {
            w.set_window_level(if on { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal });
        }
    }
    fn main_minimize(&self) {
        if let Some(w) = WINDOW.get() {
            w.set_minimized(true);
        }
    }
    fn wayland_client(&self) -> bool {
        wayland()
    }

    fn running(&self) -> Vec<String> {
        runner().running()
    }
    fn any_running(&self) -> bool {
        runner().is_any_running()
    }
    fn child_pids(&self) -> Vec<(String, u32)> {
        runner().child_pids()
    }
    fn with_descendants(&self, pids: &[u32]) -> Vec<u32> {
        #[cfg(target_os = "linux")]
        return rare_launch::launch::with_descendants(pids);
        #[cfg(not(target_os = "linux"))]
        pids.to_vec()
    }
    fn focusable_apps_seen(&self, apps: &[u64]) {
        let r = runner();
        // A game that ended hands the screen back without waiting for a button press.
        rare_launch::apps::front::sync(&r.running());
        r.mark_shown(&rare_launch::launch::shown_runs(&r.app_ids(), apps));
    }

    fn pointer_active(&self) -> bool {
        rare_launch::apps::front::pointer_active()
    }
    fn launcher_front(&self) -> bool {
        // A Steam game still starting shows Rare, not Steam: the Rare button acts as on the launcher.
        rare_launch::apps::front::launcher_front() || rare_launch::apps::front::steam_starting()
    }
    fn steam_front(&self) -> bool {
        rare_launch::apps::front::steam_front()
    }
    fn front_run(&self) -> Option<String> {
        rare_launch::apps::front::front_run()
    }
    fn to_launcher(&self) {
        rare_launch::apps::front::to_launcher();
    }
    fn to_front(&self, run: &str) {
        rare_launch::apps::front::to_front(run);
    }

    // Web games and webview apps: rare-webgame's own window (features/webgame.rs). Its still is
    // captured like a native game's; it stays mapped under the launcher while Rare is up, hidden
    // when Rare goes Home over it.
    fn web_visible(&self) -> bool {
        rare_webgame::visible()
    }
    fn web_current(&self) -> Option<String> {
        rare_webgame::current_id()
    }
    fn web_suspend(&self) {
        crate::features::webgame::suspend();
    }
    fn web_resume(&self) {
        rare_webgame::resume();
    }
    fn hide_web_window(&self) {
        rare_webgame::hide();
    }
    fn web_pad(&self, button: Pad, down: bool) {
        rare_webgame::pad(button, down);
    }

    fn input_surface(&self, up: bool) {
        SURFACE.store(up, Ordering::Relaxed);
        // The intercept mode follows now (as the Tauri shell does), not at the next 5 s reconcile:
        // until then the game kept the pad under the menu, and after Resume it got none.
        rare_input::ip::surface(crate::app::surface_up());
        rare_input::reset::request(if up { "Rare surface up" } else { "Rare surface closed" });
    }
    fn pointer_prepare(&self) {
        rare_launch::apps::pointer::prepare();
    }
    fn pointer_combo(&self, keys: &[u16]) {
        rare_launch::apps::pointer::combo(keys);
    }

    fn thaw(&self, run: &str) {
        rare_system::freeze::thaw(run);
    }
    fn is_frozen(&self, run: &str) -> bool {
        rare_system::freeze::is_frozen(run)
    }
    fn max_frozen(&self) -> usize {
        rare_system::freeze::MAX_FROZEN
    }
    fn freeze_for_switch(&self, run: &str) -> Option<serde_json::Value> {
        let out = rare_system::freeze::freeze(run, runner().freeze_target(run));
        log(&format!("[switcher] leaving {run}: {out:?}"));
        match &out {
            rare_system::freeze::Outcome::Full { .. } => serde_json::to_value(&out).ok(),
            _ => None,
        }
    }
    fn freeze_sync(&self, running: &[String]) -> Option<String> {
        rare_system::freeze::sync(running)
    }
    fn freeze_drop_pending(&self) {
        rare_system::freeze::drop_pending();
    }

    fn steam_url(&self, url: &str) -> Result<(), String> {
        rare_launch::launch::steam_url(url)
    }
}

/// Installs the native launcher as rare-overlay's front end, once the window exists: tags the
/// window for gamescope and keeps Rare's place in the base-layer list (Rare OS game mode), lets go
/// of a base layer a crashed run held, thaws what it left frozen, and starts the Switcher's
/// bookkeeping.
pub fn install(window: Arc<Window>) {
    if WINDOW.set(window).is_err() {
        return;
    }
    rare_overlay::install(crate::features::bus::ctx(), Arc::new(NativeOverlay));
    ON.store(true, Ordering::Relaxed);
    rare_overlay::gamescope::session_start();
    std::thread::spawn(|| {
        // xprop and the freezer, off the first frame's path.
        std::thread::sleep(std::time::Duration::from_millis(1500));
        rare_overlay::init();
        rare_system::freeze::thaw_stale();
        rare_overlay::switcher::start();
        rare_system::brightness::apply_saved();
        // Night light and the TV's picture, kept in place.
        rare_system::screen::start();
        // Quick settings' sound rows are in place on the first open after boot.
        crate::screens::quick::audio::remember(rare_system::audio::read());
        start_system_keys();
        // Settings > Controls > Button layout: the saved layout back into InputPlumber.
        rare_input::remap::start();
    });
}

/// The volume keys and the power button, whatever has the screen (src-tauri lib.rs: syskeys and
/// powerbtn). Volume steps the default sink with Rare's bar on the HUD; Rare + volume steps the
/// screen brightness; a power press sleeps and a 1 s hold opens the power menu over anything
/// (docs/CONTROLS.md). While a Rare surface intercepts the pad the volume keys come as InputPlumber
/// actions instead and rare-render's input thread hands them to `rare_input::volume::key`.
fn start_system_keys() {
    if rare_overlay::gamescope::session_client() {
        rare_input::volume::start();
    } else {
        log("[syskeys] not Rare's own gamescope session: volume keys left to Steam or the desktop");
    }
    rare_input::power::start(rare_overlay::power_menu);
}

/// At exit: nothing stays frozen.
pub fn shutdown() {
    if on() {
        rare_system::freeze::thaw_all();
    }
}

/// What the pad thread does with a press before the app sees it (src-tauri lib.rs gamepad::start):
/// true when the app should get it. Quick goes to rare-overlay (Quick Settings over the game in
/// front, or the launcher's own sheet), the Rare button to the focus keeper, and while a surface is
/// up every press is the UI's.
pub fn route_press(pad: Pad) -> bool {
    if !on() {
        return true;
    }
    if pad == Pad::Qam {
        std::thread::spawn(rare_overlay::quick_chord);
        return false;
    }
    // A press rare-overlay passes goes to the launcher unless gamescope shows a game or app: then
    // it is the game's. (It reached the hidden launcher too: its Home focus moved with the game's
    // d-pad and A started whatever tile it sat on, Yu-Gi-Oh! over Battle for Bikini Bottom.) This
    // asks gamescope what is on screen, not what the launcher thinks is in front: that state can
    // still name an app Rare was raised over (the Rare button's "Launcher" step).
    match rare_overlay::route(pad, true) {
        rare_overlay::Route::Drop => false,
        rare_overlay::Route::Pass => matches!(pad, Pad::Guide | Pad::GuideHold) || !rare_overlay::gamescope::other_app_shown(),
        _ => true,
    }
}


/// A release, for the guest-mode View hold.
pub fn route_release(pad: Pad) {
    if on() {
        let _ = rare_overlay::route(pad, false);
    }
}

/// View + Menu held: the Switcher from anywhere (over a game through the overlay). False when the
/// app should open it itself (no overlay host).
pub fn exit_chord() -> bool {
    if !on() {
        return false;
    }
    std::thread::spawn(rare_overlay::exit_chord);
    true
}
