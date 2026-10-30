//! Now playing: whatever plays music or video on the device (Spotify, a Chrome tab, Twitch, a
//! Flatpak player) through MPRIS on the session bus, so the Rare menu and Quick settings show the
//! title and art and can play, pause and skip while a game has the screen.
//!
//! A watcher thread reads the players (`ListNames`, then each one's `Player` properties) every
//! second while a surface asked for them in the last [`HOT_FOR`] and every [`COLD_EVERY`]
//! otherwise, so the first look already has an answer. [`now`] returns the last reading and
//! [`version`] bumps when it changes. Which player shows: one that is playing (the one that
//! started last, when several are), else the one that played last, else any with a title.

use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// A surface looked within this long: read every second.
pub const HOT_FOR: Duration = Duration::from_secs(20);
/// Otherwise read this often.
pub const COLD_EVERY: Duration = Duration::from_secs(5);

const PREFIX: &str = "org.mpris.MediaPlayer2.";

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    /// The player's bus name (org.mpris.MediaPlayer2.spotify).
    pub player: String,
    /// What the player calls itself ("Spotify", "Google Chrome").
    pub app: String,
    pub title: String,
    /// Artists joined with ", ".
    pub artist: String,
    pub album: String,
    /// mpris:artUrl (https or file://).
    pub art: Option<String>,
    pub playing: bool,
    pub can_play_pause: bool,
    pub can_next: bool,
    pub can_previous: bool,
}

impl NowPlaying {
    /// The line under the title: the artist, else the app.
    pub fn sub(&self) -> String {
        if !self.artist.is_empty() {
            self.artist.clone()
        } else {
            self.app.clone()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    PlayPause,
    Next,
    Previous,
}

impl Cmd {
    fn method(self) -> &'static str {
        match self {
            Cmd::PlayPause => "PlayPause",
            Cmd::Next => "Next",
            Cmd::Previous => "Previous",
        }
    }
}

/// One player as read from the bus.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Player {
    pub name: String,
    pub identity: String,
    /// "Playing", "Paused", "Stopped".
    pub status: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub art: Option<String>,
    pub url: Option<String>,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_next: bool,
    pub can_previous: bool,
}

/// A readable name from a bus name when the player has no Identity ("spotify" to "Spotify",
/// "chromium.instance123" to "Chromium").
pub fn name_of(bus: &str) -> String {
    let short = bus.strip_prefix(PREFIX).unwrap_or(bus).split('.').next().unwrap_or("");
    let mut c = short.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// A title for a player that sends none (a web page playing without metadata): the page's host.
fn title_of(p: &Player) -> String {
    if !p.title.trim().is_empty() {
        return p.title.trim().to_string();
    }
    p.url.as_deref().and_then(|u| u.split_once("://")).map(|(_, r)| r.split(['/', '?', '#']).next().unwrap_or("").trim_start_matches("www.").to_string()).unwrap_or_default()
}

/// Which player to show, given the players, the one shown last and when each started playing.
pub fn pick(players: &[Player], last: Option<&str>, started: &[(String, u64)]) -> Option<usize> {
    let since = |n: &str| started.iter().find(|(s, _)| s == n).map(|(_, t)| *t).unwrap_or(0);
    let shown = |p: &Player| !title_of(p).is_empty() && p.status != "Stopped";
    let playing = players.iter().enumerate().filter(|(_, p)| p.status == "Playing" && shown(p)).max_by_key(|(_, p)| since(&p.name)).map(|(i, _)| i);
    playing
        .or_else(|| last.and_then(|l| players.iter().position(|p| p.name == l && shown(p))))
        .or_else(|| players.iter().position(shown))
}

pub fn now_playing(p: &Player) -> NowPlaying {
    NowPlaying {
        player: p.name.clone(),
        app: if p.identity.is_empty() { name_of(&p.name) } else { p.identity.clone() },
        title: title_of(p),
        artist: p.artists.iter().map(|a| a.trim()).filter(|a| !a.is_empty()).collect::<Vec<_>>().join(", "),
        album: p.album.clone(),
        // A Flatpak browser's file:// art sits in its own /tmp, out of reach: no art then.
        art: p.art.clone().filter(|a| a.starts_with("https://") || a.starts_with("http://") || a.strip_prefix("file://").is_some_and(|f| std::path::Path::new(f).is_file())),
        playing: p.status == "Playing",
        can_play_pause: p.can_play || p.can_pause,
        can_next: p.can_next,
        can_previous: p.can_previous,
    }
}

struct Shared {
    now: Option<NowPlaying>,
    last: Option<String>,
    /// Bus name and the reading count when it started playing.
    started: Vec<(String, u64)>,
    asked: Option<Instant>,
}

static VERSION: AtomicU64 = AtomicU64::new(0);

fn shared() -> &'static Mutex<Shared> {
    static S: OnceLock<Mutex<Shared>> = OnceLock::new();
    S.get_or_init(|| {
        start();
        Mutex::new(Shared { now: None, last: None, started: Vec::new(), asked: None })
    })
}

/// The player to show, from the last reading. Asking keeps the readings coming every second.
pub fn now() -> Option<NowPlaying> {
    let mut s = shared().lock().unwrap_or_else(|e| e.into_inner());
    s.asked = Some(Instant::now());
    s.now.clone()
}

/// Bumps when [`now`] would answer something else.
pub fn version() -> u64 {
    let _ = shared();
    VERSION.load(Ordering::Relaxed)
}

/// Starts the watcher (also started by the first [`now`] or [`version`]).
pub fn warm() {
    let _ = shared();
}

/// Plays or pauses, skips or goes back on the player shown, then reads again at once.
pub fn command(c: Cmd) -> Result<(), String> {
    let Some(p) = now().map(|n| n.player) else { return Err("Nothing is playing".into()) };
    call(&p, c)?;
    // Mark the guess at once so the button changes before the next reading.
    if c == Cmd::PlayPause {
        let mut s = shared().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(n) = s.now.as_mut() {
            n.playing = !n.playing;
            VERSION.fetch_add(1, Ordering::Relaxed);
        }
    }
    poke();
    Ok(())
}

fn poke() {
    if let Some(t) = THREAD.get() {
        t.unpark();
    }
}

static THREAD: OnceLock<std::thread::Thread> = OnceLock::new();

fn start() {
    let h = std::thread::Builder::new().name("rare-media".into()).spawn(|| {
        let mut n: u64 = 0;
        loop {
            n += 1;
            let players = read();
            {
                let mut s = shared().lock().unwrap_or_else(|e| e.into_inner());
                // Note when each started playing, so the newest player wins.
                s.started.retain(|(name, _)| players.iter().any(|p| &p.name == name && p.status == "Playing"));
                for p in players.iter().filter(|p| p.status == "Playing") {
                    if !s.started.iter().any(|(name, _)| name == &p.name) {
                        s.started.push((p.name.clone(), n));
                    }
                }
                let i = pick(&players, s.last.as_deref(), &s.started);
                let next = i.map(|i| now_playing(&players[i]));
                if let Some(np) = &next {
                    s.last = Some(np.player.clone());
                }
                if next != s.now {
                    s.now = next;
                    VERSION.fetch_add(1, Ordering::Relaxed);
                }
            }
            let hot = shared().lock().ok().and_then(|s| s.asked).is_some_and(|a| a.elapsed() < HOT_FOR);
            std::thread::park_timeout(if hot { Duration::from_secs(1) } else { COLD_EVERY });
        }
    });
    if let Ok(h) = h {
        let _ = THREAD.set(h.thread().clone());
    }
}

#[cfg(target_os = "linux")]
mod bus {
    use super::{Cmd, Player, PREFIX};
    use dbus::arg::{prop_cast, PropMap, RefArg};
    use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
    use dbus::blocking::Connection;
    use std::cell::RefCell;
    use std::time::Duration;

    const PATH: &str = "/org/mpris/MediaPlayer2";
    const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
    const TIMEOUT: Duration = Duration::from_millis(400);

    thread_local! {
        static CONN: RefCell<Option<Connection>> = const { RefCell::new(None) };
    }

    fn with<R>(f: impl FnOnce(&Connection) -> Result<R, dbus::Error>) -> Result<R, String> {
        CONN.with(|c| {
            let mut c = c.borrow_mut();
            if c.is_none() {
                *c = Some(Connection::new_session().map_err(|e| e.to_string())?);
            }
            let r = f(c.as_ref().unwrap());
            if r.is_err() {
                // A dropped bus connection comes back on the next call.
                *c = None;
            }
            r.map_err(|e| e.to_string())
        })
    }

    /// Strings in a value: a string, or every string inside an array or variant.
    fn strings(v: &dyn RefArg, out: &mut Vec<String>) {
        if let Some(s) = v.as_str() {
            out.push(s.to_string());
        } else if let Some(it) = v.as_iter() {
            for x in it {
                strings(x, out);
            }
        }
    }

    fn metadata(p: &mut Player, v: &dyn RefArg) {
        let Some(mut it) = v.as_iter() else { return };
        while let (Some(k), Some(val)) = (it.next(), it.next()) {
            let mut s = Vec::new();
            strings(val, &mut s);
            match k.as_str().unwrap_or("") {
                "xesam:title" => p.title = s.join(" "),
                "xesam:artist" => p.artists = s,
                "xesam:album" => p.album = s.join(" "),
                "mpris:artUrl" => p.art = s.into_iter().next().filter(|a| !a.is_empty()),
                "xesam:url" => p.url = s.into_iter().next(),
                _ => {}
            }
        }
    }

    pub fn read() -> Vec<Player> {
        with(|c| {
            let proxy = c.with_proxy("org.freedesktop.DBus", "/org/freedesktop/DBus", TIMEOUT);
            let (names,): (Vec<String>,) = proxy.method_call("org.freedesktop.DBus", "ListNames", ())?;
            let mut out = Vec::new();
            for name in names.into_iter().filter(|n| n.starts_with(PREFIX)) {
                let pp = c.with_proxy(name.as_str(), PATH, TIMEOUT);
                let Ok(props): Result<PropMap, _> = pp.get_all(PLAYER) else { continue };
                let identity: String = pp.get("org.mpris.MediaPlayer2", "Identity").unwrap_or_default();
                let flag = |k: &str| prop_cast::<bool>(&props, k).copied().unwrap_or(false);
                let mut p = Player {
                    status: prop_cast::<String>(&props, "PlaybackStatus").cloned().unwrap_or_default(),
                    identity,
                    can_play: flag("CanPlay"),
                    can_pause: flag("CanPause"),
                    can_next: flag("CanGoNext"),
                    can_previous: flag("CanGoPrevious"),
                    ..Default::default()
                };
                if let Some(m) = props.get("Metadata") {
                    metadata(&mut p, &*m.0);
                }
                p.name = name;
                out.push(p);
            }
            Ok(out)
        })
        .unwrap_or_default()
    }

    pub fn call(name: &str, c: Cmd) -> Result<(), String> {
        with(|conn| conn.with_proxy(name, PATH, TIMEOUT).method_call::<(), _, _, _>(PLAYER, c.method(), ()))
    }
}

#[cfg(target_os = "linux")]
use bus::{call, read};

#[cfg(not(target_os = "linux"))]
fn read() -> Vec<Player> {
    Vec::new()
}

#[cfg(not(target_os = "linux"))]
fn call(_name: &str, _c: Cmd) -> Result<(), String> {
    Err("Media controls need Linux".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, status: &str, title: &str) -> Player {
        Player { name: format!("{PREFIX}{name}"), status: status.into(), title: title.into(), ..Default::default() }
    }

    #[test]
    fn picks_the_newest_playing_then_the_last_shown() {
        let ps = vec![p("spotify", "Paused", "Song"), p("chromium.instance7", "Playing", "Stream"), p("vlc", "Playing", "Film")];
        let started = vec![(ps[1].name.clone(), 3), (ps[2].name.clone(), 9)];
        assert_eq!(pick(&ps, None, &started), Some(2));
        // Nothing playing: the one shown last stays, else the first with a title.
        let ps = vec![p("vlc", "Paused", "Film"), p("spotify", "Paused", "Song")];
        assert_eq!(pick(&ps, Some(&ps[1].name), &[]), Some(1));
        assert_eq!(pick(&ps, None, &[]), Some(0));
        // A stopped player or one with nothing to show is left out.
        let ps = vec![p("vlc", "Stopped", "Film"), p("x", "Playing", "")];
        assert_eq!(pick(&ps, None, &[]), None);
    }

    #[test]
    fn makes_a_plain_now_playing() {
        let mut s = p("spotify", "Playing", "Song");
        s.artists = vec!["A".into(), " B ".into()];
        s.art = Some("https://i.scdn.co/image/ab".into());
        s.can_pause = true;
        let n = now_playing(&s);
        assert_eq!((n.app.as_str(), n.artist.as_str(), n.playing, n.can_play_pause), ("Spotify", "A, B", true, true));
        assert_eq!(n.sub(), "A, B");
        // A page with no title shows its host; no artist shows the app.
        let mut c = p("chromium.instance12", "Playing", "");
        c.url = Some("https://www.twitch.tv/somebody".into());
        c.art = Some("data:image/png;base64,xx".into());
        let n = now_playing(&c);
        assert_eq!((n.title.as_str(), n.app.as_str(), n.sub(), n.art), ("twitch.tv", "Chromium", "Chromium".to_string(), None));
        assert_eq!(pick(&[c], None, &[]), Some(0));
    }
}
