//! Wi-Fi sign-in (captive portals on hotel and airport networks). A watcher thread reads
//! NetworkManager's connectivity after Wi-Fi connects or the network changes (the app nudges it),
//! and every 30 s while the network wants a sign-in or has no internet. It never runs on the UI
//! thread and rests while a game has the screen, unless the sign-in window is open.
//!
//! When the network turns to a portal the app offers "Sign in to <network>" once; Settings > Network
//! and Quick settings show it until it is done. The sign-in page opens in the web window
//! (`rare_webgame::open_portal`); when the internet works the window closes by itself.

use rare_system::portal::Connectivity;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// How often to check while the network wants a sign-in or has no internet.
const CLOSE_SECS: u64 = 30;
/// How often to read NetworkManager's own last state otherwise (no new check, cheap).
const CALM_SECS: u64 = 120;

struct St {
    started: bool,
    conn: Connectivity,
    ssid: Option<String>,
    /// The app should offer a sign-in for this network.
    prompt: Option<String>,
    /// Already offered for this portal (not again until the network changes or works).
    offered: bool,
    /// The sign-in worked: the app toasts "Connected to <network>".
    connected: Option<String>,
    nudged: bool,
    front: bool,
    wifi_seen: Option<Option<String>>,
}

static ST: Mutex<St> = Mutex::new(St { started: false, conn: Connectivity::Unknown, ssid: None, prompt: None, offered: false, connected: None, nudged: false, front: true, wifi_seen: None });
static CV: Condvar = Condvar::new();

fn with<R>(f: impl FnOnce(&mut St) -> R) -> R {
    let mut g = ST.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut g)
}

/// Starts the watcher once (the device, not a fixture run).
pub fn start(waker: Arc<dyn Fn() + Send + Sync>) {
    if with(|s| std::mem::replace(&mut s.started, true)) {
        return;
    }
    nudge();
    std::thread::Builder::new().name("portal".into()).spawn(move || run(waker)).ok();
}

/// Check now (Wi-Fi joined, the network changed, the launcher came to the front).
pub fn nudge() {
    with(|s| s.nudged = true);
    CV.notify_all();
}

/// The launcher has the screen. Coming to the front checks at once.
pub fn set_front(on: bool) {
    let was = with(|s| std::mem::replace(&mut s.front, on));
    if on && !was {
        nudge();
    }
}

/// The app's Wi-Fi name from its device reading: a change checks at once.
pub fn note_wifi(wifi: Option<String>) {
    let moved = with(|s| {
        let moved = s.wifi_seen.as_ref() != Some(&wifi);
        s.wifi_seen = Some(wifi);
        moved
    });
    if moved {
        nudge();
    }
}

/// The network wants a sign-in.
pub fn needs_sign_in() -> bool {
    with(|s| s.conn == Connectivity::Portal)
}

/// The network that wants it, if known.
pub fn ssid() -> Option<String> {
    with(|s| s.ssid.clone())
}

/// A sign-in to offer (once per portal).
pub fn take_prompt() -> Option<String> {
    with(|s| if s.conn == Connectivity::Portal { s.prompt.take() } else { None })
}

/// The sign-in worked and its window closed: the network's name for the toast.
pub fn take_connected() -> Option<String> {
    with(|s| s.connected.take())
}

/// Opens the network's sign-in page in the web window.
pub fn open() -> Result<(), String> {
    with(|s| s.prompt = None);
    let url = rare_system::portal::portal_url();
    crate::features::webgame::open_portal(&url)
}

fn name(ssid: &Option<String>) -> String {
    ssid.clone().unwrap_or_else(|| "this network".into())
}

fn run(waker: Arc<dyn Fn() + Send + Sync>) {
    loop {
        let fresh = {
            let mut g = ST.lock().unwrap_or_else(|e| e.into_inner());
            let wait = Duration::from_secs(if g.conn.watch_closely() { CLOSE_SECS } else { CALM_SECS });
            g = CV.wait_timeout_while(g, wait, |s| !s.nudged).map(|r| r.0).unwrap_or_else(|e| e.into_inner().0);
            let nudged = std::mem::take(&mut g.nudged);
            if !(g.front || rare_webgame::portal_open()) {
                continue;
            }
            nudged || g.conn.watch_closely()
        };
        // A new check fetches NetworkManager's check page; otherwise its last state is enough
        // unless that says something is wrong.
        let mut c = if fresh { rare_system::portal::connectivity() } else { rare_system::portal::cached() };
        if !fresh && c != Connectivity::Full && c != Connectivity::Unknown {
            c = rare_system::portal::connectivity();
        }
        let ssid = if matches!(c, Connectivity::None | Connectivity::Unknown) { None } else { rare_system::netctl::active_wifi() };
        let window = rare_webgame::portal_open();
        let (changed, close) = with(|s| apply(s, c, ssid, window));
        if close {
            rare_webgame::close_external();
        }
        if changed {
            crate::screens::settings::live::touch();
            waker();
        }
    }
}

/// One reading into the state. Returns (something to show changed, close the sign-in window).
fn apply(s: &mut St, c: Connectivity, ssid: Option<String>, window: bool) -> (bool, bool) {
    let before = (s.conn, s.ssid.clone());
    if ssid != s.ssid || matches!(c, Connectivity::Full | Connectivity::None) {
        s.offered = false;
    }
    if c == Connectivity::Portal && !s.offered {
        s.offered = true;
        s.prompt = Some(name(&ssid));
    }
    if c != Connectivity::Portal {
        s.prompt = None;
    }
    let close = c == Connectivity::Full && window;
    if close {
        s.connected = Some(ssid.clone().unwrap_or_else(|| "the internet".into()));
    }
    s.conn = c;
    s.ssid = ssid;
    ((s.conn, s.ssid.clone()) != before || close || s.prompt.is_some(), close)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st() -> St {
        St { started: true, conn: Connectivity::Unknown, ssid: None, prompt: None, offered: false, connected: None, nudged: false, front: true, wifi_seen: None }
    }

    #[test]
    fn offers_once_and_closes_when_online() {
        let mut s = st();
        let hotel = Some("Hotel".to_string());
        assert!(apply(&mut s, Connectivity::Portal, hotel.clone(), false).0);
        assert_eq!(s.prompt.take().as_deref(), Some("Hotel"));
        apply(&mut s, Connectivity::Portal, hotel.clone(), false);
        assert!(s.prompt.is_none(), "offered once per portal");
        assert_eq!(apply(&mut s, Connectivity::Full, hotel.clone(), true), (true, true));
        assert_eq!(s.connected.as_deref(), Some("Hotel"));
        apply(&mut s, Connectivity::Portal, hotel, false);
        assert!(s.prompt.is_some(), "a new portal after working is offered again");
        assert_eq!(apply(&mut s, Connectivity::Full, None, false).1, false, "no window, nothing to close");
    }
}
