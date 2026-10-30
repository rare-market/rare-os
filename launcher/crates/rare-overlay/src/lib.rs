//! Rare over a game: the Rare menu (press the Rare button) and the Switcher (hold it). The menu
//! has the game's tabs (Game, Items, Market, Trophies, Friends, Notifications) and opens on Game
//! with Resume focused, so Rare then A is always back to the game; pressing Rare again goes Home.
//! The Switcher is every running game as a still frame, with quick resume (freeze.rs). What the
//! button does where is the focus keeper's (focus.rs). Details: docs/OVERLAY.md.
//!
//! In Rare OS the Rare button arrives as Guide (gamepad.rs) and Steam never sees it. In guest mode
//! (Rare as a shortcut inside someone else's Steam Game Mode) Steam owns Guide, so the old
//! triggers stay there: hold View for the menu. Hold View + Menu (Select + Start) is the Switcher
//! everywhere, a silent rescue.
//!
//! How it gets on screen, in Game Mode (gamescope shows one focused app at a time):
//! - The overlay is the main Rare window. Before it comes up, gamescope saves a frame of the game
//!   (`GAMESCOPECTRL_REQUEST_SCREENSHOT`, base plane only), and the UI shows that frame dimmed
//!   behind the panel. Then `GAMESCOPECTRL_BASELAYER_WINDOW` puts the main window in front.
//!   Closing removes the control and gamescope goes back to Steam's choice: the game.
//! - Taking focus is the point: the game's window loses input focus, so SDL games (their default)
//!   and most others stop reading the controller while the panel is up. An external overlay
//!   window (`GAMESCOPE_EXTERNAL_OVERLAY`, how mangoapp draws) would show the live game but
//!   leave the game reading every press, and mangoapp already holds that one slot.
//! - Web games are Rare's own `webgame` window, so they work the same way, and the controller is
//!   Rare's to route: View presses reach the game as taps, a hold never does.
//! - Outside gamescope (desktop session, development) the main window goes always-on-top.

pub mod appid;
pub mod focus;
pub mod gamescope;
pub mod guest;
pub mod host;
pub mod journey;
pub mod steampages;
pub mod switcher;
pub mod trigger;

pub use host::{install, installed, OverlayHost};
use host::host;
use rare_core::Pad;
use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use trigger::{Hold, Release};

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mechanism {
    /// gamescope's base-layer window control.
    Gamescope,
    /// A normal window raised always-on-top (desktop sessions, development).
    Window,
}

/// What the overlay shows.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// The Rare menu: press the Rare button (hold View in guest mode).
    Menu,
    /// Every running game as a card: hold the Rare button, or View + Menu.
    Switcher,
    /// Quick Settings over the game: the Quick button.
    Quick,
    /// Sleep, Restart, Turn off: the power button held (powerbtn.rs).
    Power,
}

impl Mode {
    fn surface(self) -> focus::Surface {
        match self {
            Mode::Menu => focus::Surface::Menu,
            Mode::Switcher => focus::Surface::Switcher,
            Mode::Quick | Mode::Power => focus::Surface::Other,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub open: bool,
    /// Library id of the game under the overlay ("web:act", "steam:123"), when known.
    pub game_id: Option<String>,
    pub web: bool,
    /// A frame of the game to show dimmed behind the panel (a local PNG path).
    pub frame: Option<String>,
    pub mechanism: Mechanism,
    pub mode: Mode,
    /// Rare runs inside someone else's Steam Game Mode: Steam owns the Rare button (hints differ).
    pub guest: bool,
    /// Sent with `open: false` when the overlay closed to go Home (the Rare button pressed again).
    pub home: bool,
    #[serde(skip)]
    prev_control: Option<u64>,
}

struct State {
    hold: Hold,
    session: Option<Session>,
    /// An open is in progress (capture and raise take a moment); presses already go to the UI.
    opening: bool,
}

static STATE: Mutex<State> = Mutex::new(State { hold: Hold::new(), session: None, opening: false });

pub fn is_open() -> bool {
    let s = STATE.lock().unwrap();
    s.session.is_some() || s.opening
}

/// A game is on screen and the launcher isn't: a web game shown, or something Rare started is
/// in front (apps::front) or simply has focus. Apps driven as a mouse (pointer mode) keep View.
fn game_has_screen() -> bool {
    if host().web_visible() {
        return true;
    }
    if host().pointer_active() {
        return false;
    }
    host().any_running() && (!host().launcher_front() || !host().main_focused())
}

pub enum Route {
    /// Send the press to the main window's UI.
    Ui,
    /// Swallow it.
    Drop,
    /// Not the overlay's business: the normal routing goes on.
    Pass,
}

/// Rare runs as a shortcut inside someone else's Steam Game Mode (the launcher-only AppImage, or
/// Bazzite's Steam session): Steam owns Guide and quick access there, so the Rare button is not
/// Rare's and the View hold opens the menu instead (guest.rs).
pub fn guest() -> bool {
    guest::guest()
}

/// The surface open now, if any (an open in progress counts as the menu: presses go to the UI).
fn surface() -> Option<focus::Surface> {
    let s = STATE.lock().unwrap();
    s.session.as_ref().map(|x| x.mode.surface()).or(s.opening.then_some(focus::Surface::Menu))
}

/// The game under the open overlay (its run id), if any.
pub fn under() -> Option<String> {
    STATE.lock().unwrap().session.as_ref().and_then(|s| s.game_id.clone())
}

fn spawn_open(mode: Mode) {
    std::thread::spawn(move || {
        if let Err(e) = open_over_front(mode) {
            host::log(&format!("[overlay] {e}"));
        }
    });
}

/// Carries out the focus keeper's step for a Rare button press (or the rescue chord).
fn step(press: focus::Press) -> Route {
    let open = surface();
    let front = something_in_front();
    let st = focus::on_press(open, front, press);
    host::log(&format!("[overlay] Rare button {press:?}: {st:?}"));
    match st {
        focus::Step::OpenMenu => spawn_open(Mode::Menu),
        focus::Step::OpenSwitcher => spawn_open(Mode::Switcher),
        focus::Step::Show(s) => show(s),
        focus::Step::Home => {
            std::thread::spawn(go_home);
        }
        focus::Step::Launcher => return Route::Pass,
        focus::Step::Nothing => {}
    }
    Route::Drop
}

/// The open surface shows `s` instead (the Rare menu and the Switcher swap in place).
fn show(s: focus::Surface) {
    let mode = match s {
        focus::Surface::Switcher => Mode::Switcher,
        _ => Mode::Menu,
    };
    let sess = {
        let mut st = STATE.lock().unwrap();
        st.session.as_mut().map(|x| {
            x.mode = mode;
            x.clone()
        })
    };
    if let Some(sess) = sess {
        emit(&sess);
    }
}

/// Sees every press and release first.
pub fn route(button: Pad, down: bool) -> Route {
    // The Rare button: the focus keeper decides (Rare OS; Steam's in guest mode).
    if matches!(button, Pad::Guide | Pad::GuideHold) && !guest() {
        if !down {
            return Route::Drop;
        }
        if STATE.lock().unwrap().opening {
            return Route::Drop;
        }
        return step(if button == Pad::Guide { focus::Press::Rare } else { focus::Press::Hold });
    }
    if is_open() {
        let mut s = STATE.lock().unwrap();
        // The release of the hold that opened us is not a press for the panel.
        if button == Pad::Select && !down {
            s.hold.release(Instant::now());
        }
        return if down { Route::Ui } else { Route::Drop };
    }
    // Hold View for the menu: guest mode only. In Rare OS View is the game's, always (holding it is
    // a map or a scoreboard in many games).
    if !guest() {
        return Route::Pass;
    }
    let web = host().web_visible();
    if !game_has_screen() {
        STATE.lock().unwrap().hold.reset();
        return Route::Pass;
    }
    let now = Instant::now();
    if button == Pad::Select {
        let released = {
            let mut s = STATE.lock().unwrap();
            if down {
                s.hold.press(now);
                return Route::Drop;
            }
            s.hold.release(now)
        };
        match released {
            Release::Tap if web => {
                host().web_pad(Pad::Select, true);
                std::thread::sleep(Duration::from_millis(30));
                host().web_pad(Pad::Select, false);
                Route::Drop
            }
            Release::Chord | Release::Idle if web => Route::Pass,
            _ => Route::Drop,
        }
    } else {
        let chorded = down && STATE.lock().unwrap().hold.chord();
        if chorded && web {
            // The game gets View down now, before this button.
            host().web_pad(Pad::Select, true);
        }
        Route::Pass
    }
}

/// Called on every input pass: opens the menu when the guest-mode View hold completes.
pub fn tick() {
    let fire = STATE.lock().unwrap().hold.poll(Instant::now());
    if fire {
        spawn_open(Mode::Menu);
    }
}

/// The game under the overlay: the web game shown, else the running game gamescope has on
/// screen (Steam app id), else the one running game.
fn foreground_game(web: bool) -> Option<String> {
    if web {
        if let Some(id) = host().web_current() {
            return Some(id);
        }
    }
    let mut running: Vec<String> = host().running().into_iter().filter(|id| !id.starts_with("web:")).collect();
    running.sort();
    if gamescope::active() && running.len() > 1 {
        if let Some(appid) = gamescope::focused_app() {
            let want = format!("steam:{appid}");
            if running.contains(&want) {
                return Some(want);
            }
        }
    }
    running.into_iter().next()
}

fn frame_path() -> Option<std::path::PathBuf> {
    let dir = rare_core::paths::cache_dir().join("overlay");
    // One file per open so the webview never shows a cached older frame; old ones go.
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    Some(dir.join(format!("frame-{ms}.png")))
}

fn emit(s: &Session) {
    host::emit("overlay", serde_json::to_value(s).unwrap_or(serde_json::Value::Null));
}

/// Brings the Rare menu up over the game. `game_id` overrides which game it is for.
pub fn open(game_id: Option<String>) -> Result<Session, String> {
    open_as(game_id, Mode::Menu)
}

/// Something Rare started has the screen (a web game, or a game or app in front of the launcher).
/// Unlike `game_has_screen`, an app driven as a mouse counts too.
fn something_in_front() -> bool {
    if host().web_visible() {
        return true;
    }
    host().any_running() && (!host().launcher_front() || !host().main_focused())
}

/// The Quick button (gamepad.rs, inputplumber.rs, whatever has focus): Quick Settings over
/// whatever is in front (a game, an app, a web game, Steam's UI) through the overlay's base-layer
/// and frame path, so the app keeps running under it. Pressed again: back to it. Over the Rare
/// menu or the Switcher: Quick Settings there. With the launcher in front: the launcher's own sheet.
pub fn quick_chord() {
    sheet(Mode::Quick);
}

/// The power button held (powerbtn.rs): the power menu over whatever is in front.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn power_menu() {
    sheet(Mode::Power);
}

/// A system sheet (Quick Settings, the power menu) from a button.
fn sheet(mode: Mode) {
    let open_now = {
        let mut s = STATE.lock().unwrap();
        if s.opening {
            return;
        }
        match s.session.as_mut() {
            Some(sess) if sess.mode == mode => None,
            Some(sess) => {
                sess.mode = mode;
                Some(Some(sess.clone()))
            }
            None => Some(None),
        }
    };
    let Some(open_now) = open_now else {
        host::log(&format!("[overlay] {mode:?} again: back to the game"));
        if let Err(e) = close() {
            host::log(&format!("[overlay] {e}"));
        }
        return;
    };
    if let Some(sess) = open_now {
        host::log(&format!("[overlay] switching the open overlay to {mode:?}"));
        emit(&sess);
        return;
    }
    // Quick Settings over the launcher is the launcher's own sheet; the power menu always comes
    // up as the overlay.
    if mode == Mode::Power || !host().launcher_front() || something_in_front() {
        host::log(&format!("[overlay] {mode:?} over {}", host().front_run().as_deref().unwrap_or("what is in front")));
        if let Err(e) = open_over_front(mode) {
            host::log(&format!("[overlay] {e}"));
        }
        return;
    }
    emit_pad(Pad::Qam);
}

/// A button for the launcher's UI, as if the pad had pressed it.
fn emit_pad(button: Pad) {
    host::emit("pad", serde_json::json!({ "button": button, "repeat": false }));
}

/// Opens `mode` over whatever Rare started that has the screen. The run in front is read before
/// the launcher takes the pad (an app in pointer mode gets it back on Resume).
fn open_over_front(mode: Mode) -> Result<Session, String> {
    let game_id = host().front_run();
    open_as(game_id, mode)
}

/// View + Menu held (gamepad.rs, read from evdev whatever has focus): the Switcher, from anywhere,
/// in Rare OS and in guest mode alike. A silent rescue for when a system button ever breaks.
pub fn exit_chord() {
    if STATE.lock().unwrap().opening {
        return;
    }
    if let Route::Pass = step(focus::Press::Hold) {
        // The launcher has the screen already (or Steam's UI does, which it takes back): it shows
        // the Switcher itself.
        host::log("[overlay] the way out: the launcher has the screen");
        host().to_launcher();
        emit_pad(Pad::ExitHold);
    }
}

fn open_as(game_id: Option<String>, mode: Mode) -> Result<Session, String> {
    {
        let mut s = STATE.lock().unwrap();
        if let Some(cur) = s.session.clone() {
            return Ok(cur);
        }
        if s.opening {
            return Err("The overlay is already opening".into());
        }
        s.opening = true;
    }
    let result = open_inner(game_id, mode);
    {
        let mut s = STATE.lock().unwrap();
        s.opening = false;
        if let Ok(sess) = &result {
            s.session = Some(sess.clone());
        }
    }
    // A Rare surface is over the game: every press goes to Rare, none to the game or Steam.
    if result.is_ok() {
        host().input_surface(true);
    }
    result
}

fn open_inner(game_id: Option<String>, mode: Mode) -> Result<Session, String> {
    if !host().main_exists() {
        return Err("No launcher window".into());
    }
    let web = host().web_visible();
    let game_id = game_id.or_else(|| foreground_game(web));
    let t0 = Instant::now();
    // The frame first, while the game is still the one on screen. It is also the game's still
    // frame in the Switcher: every way away from a game goes through here.
    let frame = if gamescope::active() {
        frame_path().and_then(|p| gamescope::screenshot(&p, Duration::from_millis(900))).map(|p| p.to_string_lossy().into_owned())
    } else {
        None
    };
    if let (Some(id), Some(f)) = (game_id.as_deref(), frame.as_deref()) {
        switcher::keep_frame(id, std::path::Path::new(f));
    }
    // The pad is the launcher's while Rare is up (pointer mode stops; Resume gives it back).
    host().to_launcher();
    if web {
        // The web game stops getting the controller; its window stays mapped under us.
        host().web_suspend();
    }
    host().main_show();
    let mut mechanism = Mechanism::Window;
    let mut prev_control = None;
    if gamescope::active() {
        match host().main_xid() {
            Some(xid) => match gamescope::raise(xid) {
                Ok(prev) => {
                    mechanism = Mechanism::Gamescope;
                    prev_control = prev;
                    std::thread::sleep(Duration::from_millis(60));
                    let shown = gamescope::focused_window();
                    host::log(&format!("[overlay] raised 0x{xid:x} on {} (gamescope shows 0x{shown:x})", gamescope::base_display()));
                }
                Err(e) => host::log(&format!("[overlay] base layer control failed: {e}")),
            },
            None if gamescope::hold_front() => {
                mechanism = Mechanism::Gamescope;
                host::log("[overlay] holding the screen by app id (Wayland client)");
            }
            None => host::log("[overlay] no X11 id for the launcher window (Wayland?)"),
        }
    }
    if mechanism == Mechanism::Window {
        host().main_always_on_top(true);
    }
    host().main_focus();
    let s = Session { open: true, game_id, web, frame, mechanism, mode, guest: guest(), home: false, prev_control };
    host::log(&format!("[overlay] open {:?} for {:?} ({}, frame {}) in {} ms", mode, s.game_id, if web { "web" } else { "native" }, if s.frame.is_some() { "yes" } else { "no" }, t0.elapsed().as_millis()));
    emit(&s);
    Ok(s)
}

/// The launcher's main window brought in front of a web game for a sheet (wallet approval), outside the
/// overlay. In Game Mode it holds gamescope's base layer, so give it back with `release_front`.
pub struct Front {
    gamescope: bool,
    prev_control: Option<u64>,
}

/// Brings the main window to the front the way the overlay does: gamescope's base-layer control in
/// Game Mode (a plain `set_focus` does not change what gamescope shows), a normal focus elsewhere.
/// None when the overlay is open (the main window already has the screen and owns the control).
pub fn bring_main_front() -> Option<Front> {
    if is_open() {
        return None;
    }
    if !host().main_exists() {
        return None;
    }
    host().main_show();
    let mut front = Front { gamescope: false, prev_control: None };
    if gamescope::active() {
        match host().main_xid().map(gamescope::raise) {
            Some(Ok(prev)) => {
                front = Front { gamescope: true, prev_control: prev };
                host::log("[overlay] sheet in front (base layer)");
            }
            Some(Err(e)) => host::log(&format!("[overlay] base layer control failed: {e}")),
            None if gamescope::hold_front() => {
                front = Front { gamescope: true, prev_control: None };
                host::log("[overlay] sheet in front (app id, Wayland client)");
            }
            None => host::log("[overlay] no X11 id for the launcher window (Wayland?)"),
        }
    }
    host().main_focus();
    Some(front)
}

/// Lets go of what `bring_main_front` took (the game comes back on screen). Leaves the base layer
/// alone when the overlay has opened since: it owns the control now.
pub fn release_front(front: Front) {
    if front.gamescope && !is_open() {
        if let Err(e) = gamescope::release(front.prev_control) {
            host::log(&format!("[overlay] releasing the base layer failed: {e}"));
        }
    }
}

/// Lets go of the screen the overlay held (gamescope's base layer, or always-on-top).
fn release_screen(s: &Session) {
    match s.mechanism {
        Mechanism::Gamescope => {
            if let Err(e) = gamescope::release(s.prev_control) {
                host::log(&format!("[overlay] releasing the base layer failed: {e}"));
            }
        }
        Mechanism::Window => host().main_always_on_top(false),
    }
}

/// In Rare OS game mode the base-layer keeper (gamescope.rs) moves the game's place in the app
/// list after `to_front`, within its 100 ms look. Letting go of the window control before that
/// would show the launcher for a moment, so wait for the list (at most 300 ms).
fn wait_for_list(web: bool) {
    if web || !gamescope::session_client() {
        return;
    }
    let base = gamescope::base_display();
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(300) {
        if gamescope::get(&base, "GAMESCOPECTRL_BASELAYER_APPID").first() != Some(&(gamescope::RARE_APPID as u64)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Back to the game (Resume, B).
pub fn close() -> Result<(), String> {
    let Some(s) = STATE.lock().unwrap().session.take() else { return Ok(()) };
    // The game has the pad again.
    host().input_surface(false);
    let still_running = s.game_id.as_ref().is_some_and(|id| host().running().contains(id));
    match s.game_id.as_deref() {
        // The game has the screen again (and an app in pointer mode its mouse); it thaws first.
        Some(id) if still_running => {
            host().to_front(id);
            wait_for_list(s.web);
        }
        // It was quit from the overlay: the launcher has the screen.
        _ => host().to_launcher(),
    }
    release_screen(&s);
    if s.web && still_running {
        // Shows and focuses the game window again and hands it the controller.
        host().web_resume();
    } else if still_running && s.mechanism == Mechanism::Window {
        // Desktop: step aside so the game's window is in front again.
        host().main_minimize();
    }
    emit(&Session { open: false, ..s });
    Ok(())
}

/// The keys that open Steam's own menu over a Steam game: gamescope's GuideKeyboardHotkey
/// (Shift+Tab, registered when Steam starts in game mode: "binding: (GuideKeyboardHotkey) ->
/// Adding new trigger [Tab + Shift_L]"), which is also the Steam overlay's own in-game hotkey.
pub const STEAM_MENU_KEYS: [u16; 2] = [KEY_LEFTSHIFT, KEY_TAB];

/// Linux input event codes (apps/pointer.rs has the same).
const KEY_TAB: u16 = 15;
const KEY_LEFTSHIFT: u16 = 42;

/// "Steam menu" in the panel of a Steam game: Home is Rare's, so Steam's in-game menu is reached
/// from here. The panel closes (the game has the screen and focus again), then Shift+Tab.
pub fn steam_menu() -> Result<(), String> {
    close()?;
    // The virtual keyboard first, so gamescope has picked it up by the time the keys go.
    host().pointer_prepare();
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(350));
        host().pointer_combo(&STEAM_MENU_KEYS);
        host::log("[overlay] Steam menu: sent Shift+Tab to the game");
    });
    Ok(())
}

/// Closes the overlay with the launcher keeping the screen: going Home (`home`), or a Switcher card
/// whose resume the UI does next. A web game under it is hidden; it stays where it was.
pub fn close_for(home: bool) -> Result<(), String> {
    let Some(s) = STATE.lock().unwrap().session.take() else { return Ok(()) };
    host().input_surface(false);
    host().to_launcher();
    release_screen(&s);
    if s.web {
        host().hide_web_window();
    }
    host().main_focus();
    emit(&Session { open: false, home, ..s });
    Ok(())
}

/// The Rare button pressed in the Rare menu: Home. The game under it keeps running behind the
/// launcher, or is frozen when its decision says so (freeze.rs).
fn go_home() {
    let from = under();
    if let Err(e) = close_for(true) {
        host::log(&format!("[overlay] {e}"));
    }
    switcher::apply(from.as_deref(), None);
}

pub fn overlay_state() -> Option<Session> {
    STATE.lock().unwrap().session.clone()
}

/// At start: a previous run may have died holding the base layer.
pub fn init() {
    if gamescope::active() {
        gamescope::release_stale();
    }
}
