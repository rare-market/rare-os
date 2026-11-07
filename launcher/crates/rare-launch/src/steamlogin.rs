//! Whether Steam is waiting for the player to sign in, read from its sign-in log
//! (`<Steam>/logs/steamui_login.txt`).
//!
//! Steam logs each step of its sign-in as a `SetLoginState` line (seen on the SuiPlay, 2026-11-06,
//! the same in every one of 108 sign-ins there):
//!
//! ```text
//! [2026-11-06 19:08:24] [ None ] Starting login
//! [2026-11-06 19:08:24] [ None ] SetLoginState: WaitingForNetwork - OK
//! [2026-11-06 19:08:24] [ WaitingForNetwork ] SetLoginState: WaitingForServerResponse - OK
//! [2026-11-06 19:08:25] [ WaitingForServerResponse ] SetLoginState: WaitingForLibraryReady - OK
//! [2026-11-06 19:08:26] [ WaitingForLibraryReady ] SetLoginState: Success - OK
//! ```
//!
//! When the saved sign-in no longer works (it expired, the password changed, a new device) or there
//! is none, Steam's state is `WaitingForCredentials` (steamui.so's own state names): its sign-in
//! window is up, and Steam Guard's code or phone approval is part of that window. Steam stays there
//! until the player signs in, so no Steam game can start meanwhile.
//!
//! Rare starts Steam quietly (`-silent`) and keeps the screen while it does (overlay/gamescope.rs),
//! so that window would sit behind Rare. `needed` lets the launcher say so and hand Steam the screen
//! (launch.rs `steam_questions` for Play, the launcher's notice for everything else).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// What the launcher says while Steam waits for a sign-in.
pub const SIGN_IN: &str = "Steam needs you to sign in";

/// Steam's state while its sign-in window waits for the player.
const WAITING: &str = "WaitingForCredentials";

/// The sign-in state a log line sets: `Some("None")` for a new sign-in (Steam started),
/// `Some(state)` for a `SetLoginState` line, None for any other line.
pub fn state_of(line: &str) -> Option<&str> {
    if line.contains("] Starting login") {
        return Some("None");
    }
    let rest = &line[line.find("SetLoginState: ")? + "SetLoginState: ".len()..];
    let state = rest.split([' ', '\t', '\r']).next()?.trim();
    (!state.is_empty()).then_some(state)
}

/// The state the last state line of `text` set.
pub fn last_state(text: &str) -> Option<&str> {
    text.lines().rev().find_map(state_of)
}

/// Whether `state` means Steam waits for the player to sign in.
pub fn waiting(state: Option<&str>) -> bool {
    state == Some(WAITING)
}

/// The Steam client's pid and when it started (the earliest `steam` process). Linux only.
#[cfg(target_os = "linux")]
fn steam_client() -> Option<(u32, SystemTime)> {
    let btime: u64 = std::fs::read_to_string("/proc/stat").ok()?.lines().find_map(|l| l.strip_prefix("btime ")?.trim().parse().ok())?;
    // Only each process's stat is read (comm and start time in one file), not its command line.
    std::fs::read_dir("/proc")
        .ok()?
        .flatten()
        .filter_map(|e| {
            let pid: u32 = e.file_name().to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(e.path().join("stat")).ok()?;
            let (head, rest) = stat.rsplit_once(')')?;
            if head.split_once('(')?.1 != "steam" {
                return None;
            }
            // Field 22 (starttime, in USER_HZ = 100 ticks since boot), counted after the comm's ')'.
            let ticks: u64 = rest.split_whitespace().nth(19)?.parse().ok()?;
            Some((pid, SystemTime::UNIX_EPOCH + Duration::from_secs(btime) + Duration::from_millis(ticks * 10)))
        })
        .min_by_key(|(_, t)| *t)
}

#[cfg(not(target_os = "linux"))]
fn steam_client() -> Option<(u32, SystemTime)> {
    None
}

/// Reads the sign-in log and remembers the latest state, for the Steam client that wrote it.
#[derive(Debug)]
pub struct Watch {
    path: PathBuf,
    tail: Option<crate::steamlog::Tail>,
    state: Option<String>,
    /// The Steam client the state belongs to (a new client starts a new sign-in).
    client: Option<u32>,
}

/// How far back the first look reads (a sign-in is a dozen short lines; the file only grows).
const BACK: u64 = 64 * 1024;

impl Watch {
    pub fn new(path: PathBuf) -> Watch {
        Watch { path, tail: None, state: None, client: None }
    }

    /// Takes in what Steam logged since the last look. `client` is the running Steam client (pid,
    /// start time), None when Steam is not running.
    pub fn poll(&mut self, client: Option<(u32, SystemTime)>) {
        let Some((pid, started)) = client else {
            // No Steam: nothing waits. The next client's lines are read from where the log is now.
            self.state = None;
            self.client = None;
            self.tail = None;
            return;
        };
        if self.client != Some(pid) {
            self.client = Some(pid);
            self.state = None;
            self.tail = None;
        }
        if self.tail.is_none() {
            // First look at this client: the end of the log, if this client wrote it (a log older
            // than the client is the last client's).
            let fresh = std::fs::metadata(&self.path).and_then(|m| m.modified()).is_ok_and(|m| m + Duration::from_secs(1) >= started);
            if fresh {
                self.state = read_back(&self.path, BACK).as_deref().and_then(last_state).map(str::to_string);
            }
            self.tail = Some(crate::steamlog::Tail::new(self.path.clone()));
            return;
        }
        if let Some(t) = self.tail.as_mut() {
            for l in t.read_new() {
                if let Some(s) = state_of(&l) {
                    if s == WAITING && self.state.as_deref() != Some(WAITING) {
                        eprintln!("[steam] {}", l.trim());
                    }
                    self.state = Some(s.to_string());
                }
            }
        }
    }

    pub fn needed(&self) -> bool {
        waiting(self.state.as_deref())
    }

    /// Whether the running client logged a sign-in state yet.
    pub fn known(&self) -> bool {
        self.client.is_some() && self.state.is_some()
    }
}

/// The last `n` bytes of a file, as text.
fn read_back(path: &Path, n: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(n))).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

struct Cached {
    watch: Option<Watch>,
    at: Option<Instant>,
    needed: bool,
    /// Rare closed Steam to show its sign-in in Big Picture (launch::steam_sign_in_big_picture):
    /// the sign-in still counts as needed until the new Steam says where it is.
    restart: Option<Instant>,
}

static CACHE: Mutex<Cached> = Mutex::new(Cached { watch: None, at: None, needed: false, restart: None });

/// A restart for the sign-in counts this long at most (a cold start takes about 6 s, an update more).
const RESTART_MAX: Duration = Duration::from_secs(120);

/// Steam is closed and started again for its sign-in: `needed` stays true meanwhile, so a game
/// waiting for the sign-in keeps waiting (and no "signed in" is read from Steam being gone).
pub fn restarting() {
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    c.restart = Some(Instant::now());
    c.needed = true;
}

/// Looked at most this often (the runner and the launcher both ask).
const EVERY: Duration = Duration::from_millis(900);

/// True while the running Steam waits for the player to sign in (its sign-in window is up).
/// `RARE_STEAM_SIGN_IN=1` says yes (screenshots, device checks of the notice and the hand-over).
pub fn needed() -> bool {
    if let Ok(v) = std::env::var("RARE_STEAM_SIGN_IN") {
        return v == "1";
    }
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if c.at.is_some_and(|t| t.elapsed() < EVERY) {
        return c.needed;
    }
    c.at = Some(Instant::now());
    if c.watch.is_none() {
        let Some(root) = crate::steamcfg::primary_root() else { return false };
        c.watch = Some(Watch::new(root.join("logs/steamui_login.txt")));
    }
    let client = steam_client();
    let w = c.watch.as_mut().unwrap();
    w.poll(client);
    let known = w.known();
    let needed = w.needed();
    c.needed = match c.restart {
        Some(t) if t.elapsed() < RESTART_MAX && !(known && client.is_some_and(|(_, started)| started.elapsed().unwrap_or_default() < t.elapsed())) => true,
        _ => {
            c.restart = None;
            needed
        }
    };
    c.needed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const SIGNED_IN: &str = "[2026-11-06 19:08:24] Client version: 1792618285
[2026-11-06 19:08:24] [ None ] Starting login
[2026-11-06 19:08:24] [ None ] SetLoginState: WaitingForNetwork - OK
[2026-11-06 19:08:24] [ WaitingForNetwork ] Initiating LogOn via state machine transition
[2026-11-06 19:08:24] [ WaitingForNetwork ] SetLoginState: WaitingForServerResponse - OK
[2026-11-06 19:08:25] [ WaitingForServerResponse ] Received logon success response
[2026-11-06 19:08:25] [ WaitingForServerResponse ] SetLoginState: WaitingForLibraryReady - OK
[2026-11-06 19:08:25] [ WaitingForLibraryReady ] Received post-logon callback: L=1,P=1,A=1,R=0,C=1,I=0,CM=0
[2026-11-06 19:08:26] [ WaitingForLibraryReady ] SetLoginState: Success - OK
[2026-10-30 10:26:20] [ Success ] Received logon failure response
";

    #[test]
    fn reads_steams_sign_in_states() {
        assert_eq!(state_of("[x] [ None ] SetLoginState: WaitingForNetwork - OK"), Some("WaitingForNetwork"));
        assert_eq!(state_of("[x] [ WaitingForServerResponse ] SetLoginState: WaitingForCredentials - OK"), Some("WaitingForCredentials"));
        assert_eq!(state_of("[x] [ None ] Starting login"), Some("None"));
        assert_eq!(state_of("[x] [ Success ] Received logon failure response"), None);
        assert_eq!(state_of("[x] Client version: 1792618285"), None);
        // A signed-in Steam whose session refresh failed once is still signed in.
        assert_eq!(last_state(SIGNED_IN), Some("Success"));
        assert!(!waiting(last_state(SIGNED_IN)));
        let expired = format!("{SIGNED_IN}[x] [ None ] Starting login\n[x] [ None ] SetLoginState: WaitingForNetwork - OK\n[x] [ WaitingForServerResponse ] SetLoginState: WaitingForCredentials - OK\n");
        assert!(waiting(last_state(&expired)));
        // A Steam that starts again begins a new sign-in.
        assert!(!waiting(last_state(&format!("{expired}[x] [ None ] Starting login\n"))));
        assert!(!waiting(None));
    }

    #[test]
    fn follows_one_steam_client() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("steamui_login.txt");
        std::fs::write(&p, SIGNED_IN).unwrap();
        let before = SystemTime::now() - Duration::from_secs(60);
        let mut w = Watch::new(p.clone());
        // Steam running since before the log changed: its state is read from the end.
        w.poll(Some((100, before)));
        assert!(!w.needed());
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        writeln!(f, "[x] [ Success ] SetLoginState: WaitingForCredentials - OK").unwrap();
        w.poll(Some((100, before)));
        assert!(w.needed());
        // Signed in again.
        writeln!(f, "[x] [ WaitingForCredentials ] SetLoginState: WaitingForServerResponse - OK").unwrap();
        w.poll(Some((100, before)));
        assert!(!w.needed());
        writeln!(f, "[x] [ Success ] SetLoginState: WaitingForCredentials - OK").unwrap();
        w.poll(Some((100, before)));
        assert!(w.needed());
        // Steam quit: nothing waits.
        w.poll(None);
        assert!(!w.needed());
        // A new client that started after the log was last written: the old state is not its own.
        w.poll(Some((200, SystemTime::now() + Duration::from_secs(60))));
        assert!(!w.needed());
        // Its own lines count.
        writeln!(f, "[x] [ None ] SetLoginState: WaitingForCredentials - OK").unwrap();
        w.poll(Some((200, SystemTime::now() + Duration::from_secs(60))));
        assert!(w.needed());
        // A launcher that starts while Steam already waits sees it at once.
        let mut w2 = Watch::new(p.clone());
        w2.poll(Some((200, before)));
        assert!(w2.needed());
    }

    #[test]
    fn no_log_no_sign_in() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = Watch::new(dir.path().join("missing.txt"));
        w.poll(Some((1, SystemTime::now())));
        w.poll(Some((1, SystemTime::now())));
        assert!(!w.needed());
    }
}
