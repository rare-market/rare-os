//! Journey probe for tools/journeys (the automated experience runner).
//!
//! Off unless the file $XDG_RUNTIME_DIR/rare-journey exists, which only the runner creates during a
//! test window. While it is on, the UI reports what is on screen (tab, overlay, focused element,
//! pad presses it handled, paints) through `journey_log`, and each line goes to
//! $XDG_RUNTIME_DIR/rare-journey.log stamped with CLOCK_MONOTONIC nanoseconds, the clock the runner
//! uses for injected input and captured frames. `journey_cmd` hands the UI one setup command from
//! $XDG_RUNTIME_DIR/rare-journey.cmd ("tab:library", "open:game:steam:1145350", "open:app:youtube",
//! "close") so a journey can reach a page without timing that part. Nothing is read or written
//! while the flag file is absent.

use std::io::Write;
use std::path::PathBuf;

fn runtime(name: &str) -> Option<PathBuf> {
    rare_core::paths::runtime_file(name)
}

fn enabled() -> bool {
    runtime("rare-journey").is_some_and(|f| f.exists())
}

#[cfg(unix)]
pub fn mono_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime only writes the timespec we pass.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

#[cfg(not(unix))]
pub fn mono_ns() -> u64 {
    0
}

/// One log line: "<monotonic ns> <text>", newlines in the text flattened.
pub fn line(ns: u64, text: &str) -> String {
    let text: String = text.chars().take(400).map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
    format!("{ns} {text}\n")
}

/// Whether the probe is on (the UI checks this on input, never on a timer while off).
pub fn journey_probe() -> bool {
    enabled()
}

pub fn journey_log(lines: Vec<String>) {
    if !enabled() {
        return;
    }
    let Some(f) = runtime("rare-journey.log") else { return };
    let ns = mono_ns();
    let mut out = String::new();
    for l in lines.iter().take(50) {
        out.push_str(&line(ns, l));
    }
    if let Ok(mut h) = std::fs::OpenOptions::new().create(true).append(true).open(f) {
        let _ = h.write_all(out.as_bytes());
    }
}

/// The next setup command, removed once read.
pub fn journey_cmd() -> Option<String> {
    if !enabled() {
        return None;
    }
    let f = runtime("rare-journey.cmd")?;
    let t = std::fs::read_to_string(&f).ok()?;
    let _ = std::fs::remove_file(&f);
    Some(t.trim().to_string()).filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_is_one_line_with_the_stamp() {
        assert_eq!(line(42, "screen tab=home\nx"), "42 screen tab=home x\n");
        assert_eq!(line(1, &"a".repeat(1000)).len(), 2 + 400 + 1);
    }

    #[cfg(unix)]
    #[test]
    fn mono_moves_forward() {
        let a = mono_ns();
        let b = mono_ns();
        assert!(a > 0 && b >= a);
    }
}
