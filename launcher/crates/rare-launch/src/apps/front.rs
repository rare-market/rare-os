//! What has the screen: the launcher, or something it started (a game or an app). The
//! controller follows it: while an app in pointer mode is in front, the pad is its mouse and
//! keyboard; Guide always hands the screen back to the launcher.
//!
//! Steam is told apart (`steam_front`): in Rare OS game mode the launcher only yields gamescope's
//! base layer to Steam while a Steam game or Steam's own UI was asked for (overlay/gamescope.rs).
//! Steam's own UI is an app like any other ("app:steam", apps::open): it has a run id, shows in
//! the Running list with Resume and Close, and Guide (the SuiPlay's Home button) takes the screen
//! back from it as from a game.
//!
//! A Steam game started from Rare does not hand Steam the screen at once (`steam_pending`): Steam's
//! only window while it starts (a cold start, a cloud sync, shader checks) is its own UI, so Rare
//! keeps the screen and says "Starting" until the game has a window. Steam gets the screen early
//! only when it asks the player something (`steam_ask`: a cloud save conflict, a license), after
//! Rare said so for `ASK_HINT`, and loses it again once that is answered.

use super::catalog::Back;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long Rare says "Steam needs an answer" before Steam's dialog gets the screen.
pub const ASK_HINT: Duration = Duration::from_millis(2000);

static LAUNCHER: AtomicBool = AtomicBool::new(true);
/// True while an app in pointer mode is in front (read by the input thread every pass).
static POINTER: AtomicBool = AtomicBool::new(false);
/// Counts every change of what has the screen, so a delayed hand-over (`to_front_after`) can tell
/// that the player went somewhere else in the meantime.
static GEN: AtomicU64 = AtomicU64::new(0);

struct State {
    /// Run id of what was put in front last.
    front: Option<String>,
    /// Pointer-mode apps that are open, with the key B sends.
    pointer_apps: HashMap<String, Back>,
    /// Run ids Steam starts (Steam games and shortcuts).
    steam: HashSet<String>,
    /// Steam games started that have no window yet: Rare keeps the screen for them.
    pending: HashSet<String>,
    /// The Steam game whose start waits for the player in Steam's UI, and since when.
    ask: Option<(String, Instant)>,
}

fn state() -> std::sync::MutexGuard<'static, State> {
    static S: std::sync::OnceLock<Mutex<State>> = std::sync::OnceLock::new();
    S.get_or_init(|| Mutex::new(State { front: None, pointer_apps: HashMap::new(), steam: HashSet::new(), pending: HashSet::new(), ask: None })).lock().unwrap_or_else(|e| e.into_inner())
}

/// The launcher has the screen (start, Guide, or what was in front ended).
pub fn to_launcher() {
    GEN.fetch_add(1, Ordering::Relaxed);
    LAUNCHER.store(true, Ordering::Relaxed);
    POINTER.store(false, Ordering::Relaxed);
    super::pointer::stop();
}

/// A game or app was started or resumed and now has the screen.
pub fn to_front(run_id: &str) {
    GEN.fetch_add(1, Ordering::Relaxed);
    // Quick resume: a frozen game thaws before anything shows it (overlay/switcher.rs).
    rare_overlay::switcher::fronted(run_id);
    let mut s = state();
    s.front = Some(run_id.to_string());
    POINTER.store(s.pointer_apps.contains_key(run_id), Ordering::Relaxed);
    LAUNCHER.store(false, Ordering::Relaxed);
}

/// The current hand-over count, for `to_front_if`.
pub fn generation() -> u64 {
    GEN.load(Ordering::Relaxed)
}

/// `to_front`, unless the screen changed hands since `gen` was read (the player pressed Home, or
/// opened something else, while the hand-over waited). Returns whether it handed over.
pub fn to_front_if(gen: u64, run_id: &str) -> bool {
    if generation() != gen {
        return false;
    }
    to_front(run_id);
    true
}

/// `run_id` is started by Steam (call before `to_front`).
pub fn mark_steam(run_id: &str) {
    state().steam.insert(run_id.to_string());
}

/// A Steam game was just started: Rare keeps the screen until it has a window (`steam_shown`) or
/// Steam asks the player something (`steam_ask`).
pub fn steam_pending(run_id: &str) {
    let mut s = state();
    s.pending.insert(run_id.to_string());
    if s.ask.as_ref().is_some_and(|(r, _)| r == run_id) {
        s.ask = None;
    }
}

/// The Steam game has a window (or Steam is past starting it): Steam's choices show it.
pub fn steam_shown(run_id: &str) {
    let mut s = state();
    if s.pending.remove(run_id) {
        GEN.fetch_add(1, Ordering::Relaxed);
    }
}

/// Steam waits for the player before it can start `run_id` (`on`), or no longer does.
pub fn steam_ask(run_id: &str, on: bool) {
    let mut s = state();
    let same = s.ask.as_ref().is_some_and(|(r, _)| r == run_id);
    if on && !same {
        s.ask = Some((run_id.to_string(), Instant::now()));
    } else if !on && same {
        s.ask = None;
    }
}

fn steam_shows(s: &State, f: &str) -> bool {
    if !s.steam.contains(f) {
        return false;
    }
    !s.pending.contains(f) || s.ask.as_ref().is_some_and(|(r, t)| r == f && t.elapsed() >= ASK_HINT)
}

/// Steam was asked for the screen: a Steam game in front that has a window or a question for the
/// player, or Steam's UI ("app:steam"). Quiet Steam (prewarm) never counts, nor a Steam game still
/// starting: the launcher keeps the screen then.
pub fn steam_front() -> bool {
    if launcher_front() {
        return false;
    }
    let s = state();
    s.front.as_ref().is_some_and(|f| steam_shows(&s, f))
}

/// A Steam game is in front but still starting with nothing to show: Rare has the screen, and the
/// Rare button acts as on the launcher.
pub fn steam_starting() -> bool {
    if launcher_front() {
        return false;
    }
    let s = state();
    s.front.as_ref().is_some_and(|f| s.steam.contains(f) && !steam_shows(&s, f))
}

/// An app opened in pointer mode (`Some`) or in a mode where Rare sends nothing (`None`).
pub fn set_pointer(run_id: &str, back: Option<Back>) {
    let mut s = state();
    match back {
        Some(b) => {
            s.pointer_apps.insert(run_id.to_string(), b);
        }
        None => {
            s.pointer_apps.remove(run_id);
        }
    }
}

/// The run id of what Rare started that has the screen; None while the launcher has it.
pub fn front_run() -> Option<String> {
    if launcher_front() {
        return None;
    }
    state().front.clone()
}

pub fn launcher_front() -> bool {
    LAUNCHER.load(Ordering::Relaxed)
}

/// The run id that has the screen; None while the launcher has it.
pub fn front() -> Option<String> {
    if launcher_front() {
        return None;
    }
    state().front.clone()
}

pub fn pointer_active() -> bool {
    POINTER.load(Ordering::Relaxed)
}

/// The key B sends for the app in front, while pointer mode is on.
pub fn pointer_back() -> Option<Back> {
    if !pointer_active() {
        return None;
    }
    let s = state();
    s.front.as_ref().and_then(|f| s.pointer_apps.get(f).copied())
}

/// Drops apps that ended; when the one in front ended, the launcher has the screen again.
pub fn sync(running: &[String]) {
    let mut s = state();
    s.pointer_apps.retain(|id, _| running.contains(id));
    s.pending.retain(|id| running.contains(id));
    if s.ask.as_ref().is_some_and(|(r, _)| !running.contains(r)) {
        s.ask = None;
    }
    let gone = s.front.as_ref().is_some_and(|f| !running.contains(f));
    if gone {
        s.front = None;
        drop(s);
        to_launcher();
    }
}

/// Tests that touch the process-wide front state hold this (stream's tests do too).
/// (Public so the shell's tests can take it too.)
pub static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    // The state is process-wide: hold TEST_LOCK.
    #[test]
    fn front_follows_launches() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        to_launcher();
        sync(&[]);
        assert!(launcher_front());
        set_pointer("app:netflix", Some(Back::AltLeft));
        to_front("app:netflix");
        assert!(!launcher_front());
        assert_eq!(front_run().as_deref(), Some("app:netflix"));
        assert!(pointer_active());
        assert_eq!(pointer_back(), Some(Back::AltLeft));
        // Guide: the launcher is in front, Netflix keeps running, the pad is the launcher's.
        to_launcher();
        assert_eq!(front_run(), None);
        assert!(!pointer_active());
        assert_eq!(pointer_back(), None);
        // A game in front: no pointer mode.
        to_front("steam:1");
        assert!(!pointer_active());
        assert!(!steam_front(), "not marked as Steam's");
        // Resume Netflix, then it closes: the launcher has the screen again.
        to_front("app:netflix");
        assert!(pointer_active());
        sync(&["steam:1".to_string()]);
        assert!(launcher_front());
        assert!(!pointer_active());
    }

    #[test]
    fn steam_has_the_screen_only_when_asked() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        to_launcher();
        sync(&[]);
        // Steam started quietly: nothing changes here, the launcher keeps the screen.
        assert!(!steam_front());
        // Play on a Steam game.
        mark_steam("steam:620");
        to_front("steam:620");
        assert!(steam_front());
        // The overlay over it, then back to the game.
        to_launcher();
        assert!(!steam_front());
        to_front("steam:620");
        assert!(steam_front());
        // A web game or an app in front is not Steam's.
        to_front("web:act");
        assert!(!steam_front());
        // The game ends: the launcher again.
        to_front("steam:620");
        sync(&[]);
        assert!(!steam_front());
        assert!(launcher_front());
        // Steam's UI is an app with a run id: it has the screen until Home, or until Steam quits.
        mark_steam("app:steam");
        to_front("app:steam");
        assert!(steam_front());
        assert_eq!(front_run().as_deref(), Some("app:steam"));
        to_launcher();
        assert!(!steam_front(), "Home takes the screen back from Steam's UI");
        to_front("app:steam");
        sync(&["app:steam".to_string()]);
        assert!(steam_front(), "still open");
        sync(&[]);
        assert!(!steam_front(), "Steam quit: the launcher again");
        assert!(launcher_front());
    }

    #[test]
    fn a_starting_steam_game_keeps_rare_in_front() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        to_launcher();
        sync(&[]);
        // Play on a Steam game: Steam's UI is its only window while it starts; Rare stays.
        mark_steam("steam:1145350");
        steam_pending("steam:1145350");
        to_front("steam:1145350");
        assert!(!steam_front());
        assert!(steam_starting());
        // Steam asks about the cloud save: Rare says so first, then Steam's dialog has the screen.
        steam_ask("steam:1145350", true);
        assert!(!steam_front(), "the banner shows first");
        state().ask.as_mut().unwrap().1 -= ASK_HINT;
        assert!(steam_front());
        assert!(!steam_starting());
        // Answered: Rare again until the game has a window, then the game.
        steam_ask("steam:1145350", false);
        assert!(!steam_front());
        steam_shown("steam:1145350");
        assert!(steam_front());
        // The game ends: Rare.
        sync(&[]);
        assert!(launcher_front());
        assert!(!steam_front() && !steam_starting());
    }

    #[test]
    fn a_delayed_hand_over_yields_to_home() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        to_launcher();
        // Steam was asked for; the launcher shows how to come back first.
        let g = generation();
        assert!(to_front_if(g, "app:steam"));
        assert_eq!(front().as_deref(), Some("app:steam"));
        // Asked again, but the player pressed Home while the hint showed: no hand-over.
        let g = generation();
        to_launcher();
        assert!(!to_front_if(g, "app:steam"));
        assert!(launcher_front());
    }
}
