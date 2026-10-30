//! The Switcher's side in the launcher: the still frame of each running game (taken by gamescope
//! when you switch away, which always goes through the Rare menu or the Switcher over the game),
//! the most-recently-used order, and the switch itself with quick resume (freeze.rs).
//!
//! Frames live in `~/.cache/rare/switcher/`, one file per game, replaced on every switch away and
//! removed when the game ends. A new file name each time, so the webview never shows a cached
//! older frame.

use super::focus::{leave, Effect, Mru};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use crate::host::host;

struct State {
    mru: Mru,
    frames: HashMap<String, PathBuf>,
}

static STATE: std::sync::OnceLock<Mutex<State>> = std::sync::OnceLock::new();

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.get_or_init(|| Mutex::new(State { mru: Mru::new(), frames: HashMap::new() })).lock().unwrap_or_else(|e| e.into_inner())
}

fn dir() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("rare/switcher"))
}

/// A file name for a run id: letters and digits, the rest as '-'.
pub fn slug(run: &str) -> String {
    run.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect()
}

/// `run` has the screen: it thaws (before anything shows it) and moves to the front of the order.
/// Called from apps::front::to_front, so every way back to a game goes through it.
pub fn fronted(run: &str) {
    host().thaw(run);
    state().mru.touch(run);
}

/// Keeps `frame` (a gamescope capture just taken for the overlay) as `run`'s still frame.
pub fn keep_frame(run: &str, frame: &Path) {
    let Some(d) = dir() else { return };
    let _ = std::fs::create_dir_all(&d);
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let dest = d.join(format!("{}-{ms}.png", slug(run)));
    if std::fs::copy(frame, &dest).is_err() {
        return;
    }
    if let Some(old) = state().frames.insert(run.to_string(), dest) {
        let _ = std::fs::remove_file(old);
    }
}

fn forget(run: &str) {
    if let Some(f) = state().frames.remove(run) {
        let _ = std::fs::remove_file(f);
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub rid: String,
    /// The still frame taken when you left it (a local PNG), when there is one.
    pub frame: Option<String>,
    /// Frozen: the card says "Paused".
    pub paused: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SwitcherState {
    /// Running games, most recently used first.
    pub cards: Vec<Card>,
    /// What had the screen under the Switcher (None: the launcher).
    pub from: Option<String>,
    pub max_paused: usize,
}

pub fn switcher_state() -> SwitcherState {
    let running = host().running();
    let s = state();
    let cards = s
        .mru
        .order(&running)
        .into_iter()
        .map(|rid| Card { frame: s.frames.get(&rid).map(|p| p.to_string_lossy().into_owned()), paused: host().is_frozen(&rid), rid })
        .collect();
    drop(s);
    let from = super::under().or_else(|| host().front_run());
    SwitcherState { cards, from, max_paused: host().max_frozen() }
}

/// Carries out `leave`: the game you go to thaws, the one you leave freezes when its decision
/// says so. A full set (two paused already) sends `freeze-full` so the UI asks which to close.
pub fn apply(from: Option<&str>, to: Option<&str>) {
    for e in leave(from, to) {
        match e {
            Effect::Thaw(r) => host().thaw(&r),
            Effect::Freeze(r) => freeze_now(&r),
        }
    }
}

fn freeze_now(run: &str) {
    if let Some(full) = host().freeze_for_switch(run) {
        crate::host::emit("freeze-full", full);
    }
}

/// A card was picked in the Switcher (`to`, None for Home). The surface over the game closes,
/// what was under it is left; the UI then resumes `to` its usual way (which calls
/// apps::front::to_front, so it thaws before it shows).
pub fn switcher_switch(to: Option<String>) -> Result<(), String> {
    let from = super::under().or_else(|| host().front_run());
    super::close_for(to.is_none())?;
    // The thaw now, so it is done before the UI's resume call puts the game in front.
    if let Some(t) = to.as_deref() {
        host().thaw(t);
    }
    std::thread::spawn(move || apply(from.as_deref(), to.as_deref()));
    Ok(())
}

/// "Keep it running": the game that didn't fit stays running.
pub fn freeze_keep_pending() {
    host().freeze_drop_pending();
}

/// Ended games leave the order, lose their frame and their place in the frozen set; a game waiting
/// for room is frozen once the player closed a paused one.
pub fn start() {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(1500));
        let running = host().running();
        let ended: Vec<String> = {
            let mut s = state();
            s.mru.retain(&running);
            s.frames.keys().filter(|r| !running.contains(r)).cloned().collect()
        };
        for r in ended {
            forget(&r);
        }
        if let Some(p) = host().freeze_sync(&running) {
            // Only while it is still away from the screen.
            if host().front_run().as_deref() != Some(p.as_str()) && super::under().as_deref() != Some(p.as_str()) {
                freeze_now(&p);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_names() {
        assert_eq!(slug("steam:620"), "steam-620");
        assert_eq!(slug("stream:AB-12:3"), "stream-ab-12-3");
        assert_eq!(slug("web:act"), "web-act");
    }
}
