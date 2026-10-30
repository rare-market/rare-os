//! The SuiPlay's two small top buttons, LC and RC. Thin wrapper: when a press
//! or a hold fires and what it means live in the rare-input crate (launcher/crates/rare-input/src/side.rs);
//! this module runs the action with the launcher's state.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

pub use rare_input::side::{action, hold_action, Action, Fire, HoldAction, Key};
use std::sync::Arc;
use tauri::Manager;

/// Steps the performance overlay to its next level, for the game in front (or the launcher's
/// own), and saves it: the same as picking it in quick settings.
fn perf_next(app: &tauri::AppHandle) {
    use crate::perfhud::HudLevel;
    let st = app.state::<crate::AppState>();
    let level = {
        let mut c = st.config.lock().unwrap();
        let next = crate::perfhud::next(crate::power::wanted_hud(&c));
        if crate::power::change(&mut c, |g| g.with_hud(next)).is_none() {
            c.perf_hud = Some(next);
        }
        if let Err(e) = crate::config::save(&c) {
            crate::gamepad::log(&format!("[sysbuttons] saving the overlay level: {e}"));
        }
        next
    };
    std::thread::spawn(move || crate::perfhud::apply(level));
    let name = match level {
        HudLevel::Off => "Off",
        HudLevel::Fps => "FPS",
        HudLevel::Basic => "Basic",
        HudLevel::Full => "Full",
    };
    crate::hud::toast(&format!("Performance overlay: {name}"), None);
}

/// A press (`down`) or release of LC or RC.
pub fn press(app: &tauri::AppHandle, k: Key, down: bool) {
    let app = app.clone();
    rare_input::side::press(
        k,
        down,
        Arc::new(move |k, f| match f {
            Fire::Tap => tap(&app, k),
            Fire::Hold => hold(&app, k),
        }),
    );
}

/// The Settings > Controls choice for a button's press (config `lc_action` / `rc_action`).
fn configured(app: &tauri::AppHandle, k: Key) -> Option<String> {
    let st = app.state::<crate::AppState>();
    let c = st.config.lock().unwrap();
    match k {
        Key::Lc => c.lc_action.clone(),
        Key::Rc => c.rc_action.clone(),
    }
}

fn tap(app: &tauri::AppHandle, k: Key) {
    let a = action(k, configured(app, k).as_deref());
    crate::gamepad::log(&format!("[sysbuttons] {k:?} press: {a:?}"));
    match a {
        Action::Capture => crate::share::chord(app, crate::share::Chord::Screenshot),
        Action::Keyboard => keyboard(app),
        Action::Perf => perf_next(app),
        Action::Nothing => {}
    }
}

fn hold(app: &tauri::AppHandle, k: Key) {
    let Some(h) = hold_action(k, configured(app, k).as_deref()) else { return };
    crate::gamepad::log(&format!("[sysbuttons] {k:?} held"));
    match h {
        HoldAction::Clip => crate::share::chord(app, crate::share::Chord::Clip),
        HoldAction::Perf => perf_next(app),
    }
}

/// The on-screen keyboard for whatever has focus. Rare's own keyboard types only into the
/// launcher's fields (Osk.tsx); for a game or app, Steam's keyboard types into the focused window
/// while Steam runs.
fn keyboard(app: &tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    if crate::apps::front::launcher_front() && !crate::webgame::visible() {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.emit("osk-toggle", ());
        }
        return;
    }
    if crate::launch::steam_running() {
        let _ = crate::launch::steam_url("steam://open/keyboard");
    } else {
        crate::hud::toast("Keyboard", Some("Open Steam once to type in games and apps"));
    }
}
