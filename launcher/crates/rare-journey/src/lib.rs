//! Journey probe for `tools/journeys` in the native launcher (Rare Render).
//!
//! The same contract as the web launcher (`launcher/src/nav/journey.ts` plus
//! `src-tauri/src/journey.rs`), so the device runner drives either launcher unchanged:
//!
//! - Off unless `$XDG_RUNTIME_DIR/rare-journey` exists. While off the probe touches no file except
//!   that flag, and only on a pad press, at most every 2 s ([`Probe::pad`]). Never on a timer.
//! - While on, it appends to `$XDG_RUNTIME_DIR/rare-journey.log`, one line per event, stamped
//!   with CLOCK_MONOTONIC nanoseconds (the clock of the runner's injected input and capture):
//!   ```text
//!   <ns> probe on
//!   <ns> screen tab=<tab> overlay=<kind[:id]|-> running=<n> launching=<id|->   on every change
//!   <ns> painted                                    first present after a screen or ingame change
//!   <ns> focus <tag>[@<group>] "<text>" at <x>,<y> <w>x<h>
//!   <ns> pad <button>[ repeat]                      every press the UI handled
//!   <ns> toast <tone> <text>
//!   <ns> ingame mode=<panel|running|-> game=<id|->
//!   <ns> quit <id> / quit <id> failed <error>
//!   <ns> echo <text> / unknown command <c>
//!   ```
//! - Setup commands arrive in `$XDG_RUNTIME_DIR/rare-journey.cmd`, one per line, read and deleted
//!   every 250 ms by [`Probe::poll`]: `tab:<tab>`, `open:game:<id>`, `open:app:<id>`,
//!   `open:<qam|running|search|profile>`, `close`, `quit:<id|all>`, `state`, `echo:<text>`.
//!   After the UI applies them it calls [`Probe::after_commands`], which logs the screen and focus
//!   lines whatever changed (the runner's confirmed-Home check waits for exactly that).
//!   Every 2 s `poll` also checks the flag and turns the probe off when the runner removed it.
//!
//! Use from the UI loop:
//! ```no_run
//! # use rare_journey::*;
//! # use std::time::Instant;
//! let mut probe = Probe::from_env();
//! // on every pad press the UI handled:
//! if probe.pad("rb", false, Instant::now()) {
//!     // just turned on: report where we are
//!     probe.state(&Screen { tab: "home", overlay: None, running: 0, launching: None }, &Focus::nothing());
//! }
//! // every frame (cheap when off):
//! for cmd in probe.poll(Instant::now()) { /* apply cmd to the app state */ let _ = cmd; }
//! ```

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const CHECK_EVERY: Duration = Duration::from_secs(2);
const POLL_EVERY: Duration = Duration::from_millis(250);

/// CLOCK_MONOTONIC in nanoseconds.
pub fn mono_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime only writes the timespec we pass.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

/// One log line: "<ns> <text>\n", newlines flattened, text cut at 400 characters (as journey.rs).
pub fn line(ns: u64, text: &str) -> String {
    let text: String = text.chars().take(400).map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
    format!("{ns} {text}\n")
}

/// What `screen` lines report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Screen<'a> {
    pub tab: &'a str,
    /// (kind, id): `("game", Some("steam:1145350"))`, `("raremenu", None)`.
    pub overlay: Option<(&'a str, Option<&'a str>)>,
    pub running: usize,
    pub launching: Option<&'a str>,
}

impl Screen<'_> {
    pub fn line(&self) -> String {
        let overlay = match self.overlay {
            None => "-".to_string(),
            Some((k, Some(id))) if !id.is_empty() => format!("{k}:{id}"),
            Some((k, _)) => k.to_string(),
        };
        format!("screen tab={} overlay={} running={} launching={}", self.tab, overlay, self.running, self.launching.unwrap_or("-"))
    }
}

/// The focused element, described like the web's `describe()` (nav/debug.ts).
#[derive(Clone, Debug, PartialEq)]
pub struct Focus<'a> {
    /// The web tag the element stands for: "button", "input", "a", "div".
    pub tag: &'a str,
    /// Its nav group (`data-nav-group` on the web), if any.
    pub group: Option<&'a str>,
    /// Visible text (or aria label): whitespace folded, cut at 40 characters.
    pub text: &'a str,
    /// Layout rect in logical pixels.
    pub rect: (f32, f32, f32, f32),
    pub nothing: bool,
}

impl<'a> Focus<'a> {
    pub fn nothing() -> Focus<'static> {
        Focus { tag: "", group: None, text: "", rect: (0.0, 0.0, 0.0, 0.0), nothing: true }
    }

    pub fn describe(&self) -> String {
        if self.nothing {
            return "(nothing)".into();
        }
        let text: String = self.text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(40).collect();
        let (x, y, w, h) = self.rect;
        let g = self.group.map(|g| format!("@{g}")).unwrap_or_default();
        format!("{}{} \"{}\" at {},{} {}x{}", self.tag, g, text, x.round() as i64, y.round() as i64, w.round() as i64, h.round() as i64)
    }
}

/// A setup command from the runner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cmd {
    Tab(String),
    OpenGame(String),
    OpenApp(String),
    /// qam, running, search or profile.
    Open(String),
    Close,
    /// An id, or "all" for everything running (cleanup).
    Quit(String),
    State,
    Echo(String),
    Unknown(String),
}

impl Cmd {
    pub fn parse(c: &str) -> Cmd {
        let c = c.trim();
        if let Some(t) = c.strip_prefix("tab:") {
            Cmd::Tab(t.into())
        } else if let Some(id) = c.strip_prefix("open:game:") {
            Cmd::OpenGame(id.into())
        } else if let Some(id) = c.strip_prefix("open:app:") {
            Cmd::OpenApp(id.into())
        } else if let Some(k @ ("qam" | "running" | "search" | "profile" | "downloads" | "fan-curve")) = c.strip_prefix("open:") {
            Cmd::Open(k.into())
        } else if c == "close" {
            Cmd::Close
        } else if let Some(id) = c.strip_prefix("quit:") {
            Cmd::Quit(id.into())
        } else if c == "state" {
            Cmd::State
        } else if let Some(t) = c.strip_prefix("echo:") {
            Cmd::Echo(t.into())
        } else {
            Cmd::Unknown(c.into())
        }
    }
}

pub struct Probe {
    dir: Option<PathBuf>,
    on: bool,
    last_check: Option<Instant>,
    last_poll: Option<Instant>,
    last_flag: Option<Instant>,
    screen: String,
    focus: String,
    ingame: String,
    paint_pending: bool,
}

impl Probe {
    /// The probe for `$XDG_RUNTIME_DIR` (none without it: never on).
    pub fn from_env() -> Probe {
        Probe::with_dir(std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from))
    }

    pub fn with_dir(dir: Option<PathBuf>) -> Probe {
        Probe { dir, on: false, last_check: None, last_poll: None, last_flag: None, screen: String::new(), focus: String::new(), ingame: String::new(), paint_pending: false }
    }

    fn path(&self, name: &str) -> Option<PathBuf> {
        self.dir.as_deref().map(|d| d.join(name))
    }

    fn flag(&self) -> bool {
        self.path("rare-journey").is_some_and(|p| p.exists())
    }

    pub fn on(&self) -> bool {
        self.on
    }

    /// Writes one line (only while on).
    pub fn log(&mut self, text: &str) {
        if !self.on {
            return;
        }
        let Some(p) = self.path("rare-journey.log") else { return };
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
            let _ = f.write_all(line(mono_ns(), text).as_bytes());
        }
    }

    /// Every pad press the UI handled. While on: logs it. While off: looks for the flag, at most
    /// every 2 s; returns true when that turned the probe on (then report the screen and focus
    /// with [`Probe::state`]).
    pub fn pad(&mut self, button: &str, repeat: bool, now: Instant) -> bool {
        if self.on {
            self.log(&format!("pad {button}{}", if repeat { " repeat" } else { "" }));
            return false;
        }
        if self.last_check.is_some_and(|t| now.duration_since(t) < CHECK_EVERY) {
            return false;
        }
        self.last_check = Some(now);
        if !self.flag() {
            return false;
        }
        self.on = true;
        self.last_flag = Some(now);
        self.screen.clear();
        self.focus.clear();
        self.ingame.clear();
        self.log("probe on");
        true
    }

    /// The screen, focus and in-game lines as they are now, whether or not they changed.
    pub fn state(&mut self, s: &Screen, f: &Focus) {
        if !self.on {
            return;
        }
        let (sl, fl) = (s.line(), format!("focus {}", f.describe()));
        self.log(&sl);
        self.log(&fl);
        self.screen = sl;
        self.focus = fl;
    }

    /// After the UI applied what [`Probe::poll`] returned: screen and focus, always.
    pub fn after_commands(&mut self, s: &Screen, f: &Focus) {
        self.state(s, f)
    }

    /// Call whenever the app state may have changed: logs the screen line when it differs and
    /// arms `painted` for the next present.
    pub fn screen(&mut self, s: &Screen) {
        if !self.on {
            return;
        }
        let l = s.line();
        if l != self.screen {
            self.log(&l);
            self.screen = l;
            self.paint_pending = true;
        }
    }

    pub fn focus(&mut self, f: &Focus) {
        if !self.on {
            return;
        }
        let l = format!("focus {}", f.describe());
        if l != self.focus {
            self.log(&l);
            self.focus = l;
        }
    }

    pub fn ingame(&mut self, mode: Option<&str>, game: Option<&str>) {
        if !self.on {
            return;
        }
        let l = format!("ingame mode={} game={}", mode.unwrap_or("-"), game.unwrap_or("-"));
        if l != self.ingame {
            self.log(&l);
            self.ingame = l;
            self.paint_pending = true;
        }
    }

    /// A toast appeared (tone: info, error, ok...).
    pub fn toast(&mut self, tone: &str, text: &str) {
        self.log(&format!("toast {tone} {text}"));
    }

    /// The result of a `quit:` command.
    pub fn quit_result(&mut self, id: &str, err: Option<&str>) {
        match err {
            None => self.log(&format!("quit {id}")),
            Some(e) => self.log(&format!("quit {id} failed {e}")),
        }
    }

    /// After each present: the first one after a screen or in-game change logs `painted`
    /// (the web logs it two animation frames after the change, i.e. once it is on screen).
    pub fn presented(&mut self) {
        if self.on && self.paint_pending {
            self.paint_pending = false;
            self.log("painted");
        }
    }

    /// While on, every 250 ms: the runner's commands (file read, then deleted), `echo` and
    /// unknown commands already answered. Every 2 s the flag is checked; gone means off.
    pub fn poll(&mut self, now: Instant) -> Vec<Cmd> {
        if !self.on || self.last_poll.is_some_and(|t| now.duration_since(t) < POLL_EVERY) {
            return Vec::new();
        }
        self.last_poll = Some(now);
        if self.last_flag.map_or(true, |t| now.duration_since(t) >= CHECK_EVERY) {
            self.last_flag = Some(now);
            if !self.flag() {
                self.on = false;
                self.paint_pending = false;
                return Vec::new();
            }
        }
        let Some(p) = self.path("rare-journey.cmd") else { return Vec::new() };
        let cmds = read_cmds(&p);
        let mut out = Vec::new();
        for c in cmds {
            match &c {
                Cmd::Echo(t) => self.log(&format!("echo {t}")),
                Cmd::Unknown(u) => self.log(&format!("unknown command {u}")),
                _ => {}
            }
            out.push(c);
        }
        out
    }
}

fn read_cmds(p: &Path) -> Vec<Cmd> {
    let Ok(t) = std::fs::read_to_string(p) else { return Vec::new() };
    let _ = std::fs::remove_file(p);
    t.lines().map(str::trim).filter(|l| !l.is_empty()).map(Cmd::parse).collect()
}

#[cfg(test)]
mod tests;
