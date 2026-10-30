//! The SuiPlay's LC and RC buttons and the capture chords, in the native launcher (the Tauri
//! shell's sysbuttons.rs and share.rs): what a press or a hold means comes from rare-input's
//! `side` module and Settings > Controls (`lcAction`, `rcAction` in launcher.json, read at the
//! moment of the press so a change applies at once); this module runs the action.
//!
//! Captures run on their own threads. Anything that needs the app (a toast, the keyboard) is
//! queued here and taken by `App::update` through [`take`].

use rare_input::side::{Action, Fire, HoldAction, Key};
use rare_share::ShareHost;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// What a button asks of the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Req {
    /// The on-screen keyboard (RC's default press).
    Keyboard,
    /// A message for the toasts: (text, is an error).
    Toast(String, bool),
}

static QUEUE: Mutex<Vec<Req>> = Mutex::new(Vec::new());
static WAKER: OnceLock<Arc<dyn Fn() + Send + Sync>> = OnceLock::new();
static RUNNING: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
static FRONT: AtomicBool = AtomicBool::new(true);

pub fn set_waker(w: Arc<dyn Fn() + Send + Sync>) {
    let _ = WAKER.set(w);
}

fn push(r: Req) {
    if let Ok(mut q) = QUEUE.lock() {
        if q.len() < 32 {
            q.push(r);
        }
    }
    if let Some(w) = WAKER.get() {
        w();
    }
}

/// A message for the toasts from a worker thread.
pub fn toast(text: &str, err: bool) {
    push(Req::Toast(text.to_string(), err));
}

/// The games running now (id, title), as the capture tags and Discord read them.
pub fn running_titles() -> Vec<(String, String)> {
    RUNNING.lock().map(|r| r.clone()).unwrap_or_default()
}

/// What the app queued up for itself since the last call.
pub fn take() -> Vec<Req> {
    QUEUE.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
}

/// The games running now (id, title) and whether the launcher is the window in front, for the
/// capture's game tag and where its message shows.
pub fn set_running(running: Vec<(String, String)>) {
    FRONT.store(running.is_empty(), Ordering::Relaxed);
    if let Ok(mut r) = RUNNING.lock() {
        *r = running;
    }
}

fn config() -> crate::screens::settings::config::Config {
    crate::screens::settings::config::load()
}

/// Rumble strength, 0 to 1, from Settings > Controls (off: 0).
pub fn rumble_strength() -> f32 {
    let c = config();
    if c.rumble == Some(false) {
        0.0
    } else {
        (c.rumble_strength.unwrap_or(100.0) / 100.0).clamp(0.0, 1.0)
    }
}

struct NativeShare;

impl ShareHost for NativeShare {
    fn clips_enabled(&self) -> bool {
        crate::features::capture::replay_on()
    }
    fn replay_seconds(&self) -> u32 {
        crate::features::capture::replay_seconds()
    }
    fn clips_cap_bytes(&self) -> Option<u64> {
        Some((crate::features::capture::cap_gb() as u64) << 30)
    }
    fn running(&self) -> Vec<String> {
        RUNNING.lock().map(|r| r.iter().map(|(id, _)| id.clone()).collect()).unwrap_or_default()
    }
    fn title_of(&self, id: &str) -> Option<String> {
        RUNNING.lock().ok().and_then(|r| r.iter().find(|(i, _)| i == id).map(|(_, t)| t.clone()))
    }
    fn rumble_enabled(&self) -> bool {
        config().rumble != Some(false)
    }
    fn rumble(&self) {
        let _ = rare_input::rumble::play("capture", rumble_strength());
    }
    fn launcher_front(&self) -> bool {
        FRONT.load(Ordering::Relaxed)
    }
    fn toast(&self, text: &str) {
        push(Req::Toast(text.to_string(), false));
    }
}

/// The host the Share screen and the buttons give rare-share.
pub fn share_host() -> Arc<dyn ShareHost> {
    Arc::new(NativeShare)
}

/// Starts the clip recorder supervisor (it records only while a game runs and instant replay is
/// on), Discord, the Sunshine host and the network announcement.
pub fn start_share() {
    rare_share::set_host_env(|cmd| {
        rare_system::host::env(cmd);
    });
    rare_share::start(share_host());
    if let Some(w) = WAKER.get() {
        crate::features::capture::start(w.clone());
    }
}

/// Takes a screenshot or saves a clip on its own thread; the result is a toast.
pub fn capture(clip: bool) {
    std::thread::spawn(move || {
        let host = NativeShare;
        let r = if clip { rare_share::save_clip(&host) } else { rare_share::take_screenshot(&host) };
        match r {
            Ok(cap) => {
                if host.rumble_enabled() {
                    host.rumble();
                }
                let what = if clip { "Clip saved" } else { "Screenshot saved" };
                push(Req::Toast(match cap.game.as_ref().map(|g| g.name.trim()).filter(|n| !n.is_empty()) {
                    Some(n) => format!("{what}: {n}"),
                    None => what.to_string(),
                }, false));
            }
            Err(e) => push(Req::Toast(e, true)),
        }
    });
}

/// Steps the performance overlay to its next level and saves it, as quick settings does.
pub fn perf_next() {
    use rare_system::perfhud::{self, HudLevel};
    use rare_system::power::PowerSettings;
    std::thread::spawn(|| {
        let path = rare_core::paths::config_file();
        let mut v: serde_json::Value = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| serde_json::json!({}));
        let cur: PowerSettings = serde_json::from_value(v.clone()).unwrap_or_default();
        let next = perfhud::next(cur.perf_hud.unwrap_or(HudLevel::Off));
        if let (Some(obj), Ok(level)) = (v.as_object_mut(), serde_json::to_value(next)) {
            obj.insert("perfHud".into(), level);
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, serde_json::to_vec_pretty(&v).unwrap_or_default()).is_ok() {
                let _ = std::fs::rename(tmp, &path);
            }
        }
        perfhud::apply(next);
        let name = match next {
            HudLevel::Off => "Off",
            HudLevel::Fps => "FPS",
            HudLevel::Basic => "Basic",
            HudLevel::Full => "Full",
        };
        push(Req::Toast(format!("Performance overlay: {name}"), false));
    });
}

/// LC's or RC's action: the running game's own (its page's Controls), else Settings > Controls.
/// While a game has the screen it is Settings > Controls > LC / RC in games, which is nothing until
/// picked (the standard in-game map, rare_emu::controls::STANDARD).
fn side_setting(k: Key) -> Option<String> {
    let running: Vec<String> = RUNNING.lock().map(|r| r.iter().map(|(id, _)| id.clone()).collect()).unwrap_or_default();
    if let Some(a) = crate::data::gameset::side_action(&running, k == Key::Lc) {
        return Some(a);
    }
    let c = config();
    let in_game = !running.is_empty() && !rare_launch::apps::front::launcher_front();
    match (k, in_game) {
        (Key::Lc, true) => Some(c.lc_game_action.unwrap_or_else(|| "none".into())),
        (Key::Rc, true) => Some(c.rc_game_action.unwrap_or_else(|| "none".into())),
        (Key::Lc, false) => c.lc_action,
        (Key::Rc, false) => c.rc_action,
    }
}

/// What a press of LC or RC does with the config as it is now.
pub fn tap_action(k: Key) -> Action {
    rare_input::side::action(k, side_setting(k).as_deref())
}

/// What holding LC or RC does, if anything.
pub fn hold_action(k: Key) -> Option<HoldAction> {
    rare_input::side::hold_action(k, side_setting(k).as_deref())
}

fn tap(k: Key) {
    match tap_action(k) {
        Action::Capture => capture(false),
        Action::Keyboard => push(Req::Keyboard),
        Action::Perf => perf_next(),
        Action::Nothing => {}
    }
}

fn hold(k: Key) {
    // RC held with Ask Rare's "Hold RC to talk" on: Rare listens until RC goes up.
    if k == Key::Rc && crate::features::agent::rc_hold() {
        return;
    }
    match hold_action(k) {
        Some(HoldAction::Clip) => capture(true),
        Some(HoldAction::Perf) => perf_next(),
        None => {}
    }
}

/// LC or RC went down or up (InputPlumber's `ui_screenshot` / `ui_osk` actions).
pub fn press(k: Key, down: bool) {
    if k == Key::Rc && !down {
        crate::features::agent::rc_released();
    }
    rare_input::side::press(
        k,
        down,
        Arc::new(|k, f| match f {
            Fire::Tap => tap(k),
            Fire::Hold => hold(k),
        }),
    );
}

/// A capture chord from the pad (Rare or Guide + Y or View).
pub fn chord(s: rare_input::chords::Share) {
    capture(matches!(s, rare_input::chords::Share::Clip));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_queue_hands_over_once() {
        push(Req::Keyboard);
        let a = take();
        assert!(a.contains(&Req::Keyboard));
        assert!(take().is_empty());
    }
}
