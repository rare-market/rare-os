//! The performance overlay: Off, FPS, Basic (FPS, frame time, battery
//! watts) and Full (adds CPU, GPU, temperatures, RAM, VRAM and fan), drawn by mangoapp in
//! gamescope's external-overlay slot. The level is part of a game's power settings (power.rs)
//! so it is remembered per game and comes back when the game does.
//!
//! gamescope's own mangoapp first: gamescope-session starts one for the session with
//! `MANGOHUD_CONFIGFILE` (a file in /tmp that Steam's performance overlay levels write too), and
//! mangoapp reloads that file when it changes. Rare writes the level's preset there (`no_display`
//! for Off), in place, since mangoapp watches the file itself and a rename would end the watch.
//! Without that file or that mangoapp (another session, a desktop), Rare starts its own:
//! mangoapp reads `MANGOHUD_CONFIG` when it starts, so a level change restarts it.

use serde::{Deserialize, Serialize};
use std::process::{Child, Command};
use std::sync::Mutex;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HudLevel {
    #[default]
    Off,
    Fps,
    Basic,
    Full,
}

/// The `MANGOHUD_CONFIG` preset for a level; None for Off.
pub fn preset(level: HudLevel) -> Option<&'static str> {
    Some(match level {
        HudLevel::Off => return None,
        HudLevel::Fps => "fps_only,position=top-left,font_size=26,background_alpha=0.45,round_corners=8",
        HudLevel::Basic => "fps,frametime,battery,battery_watt,position=top-left,font_size=24,background_alpha=0.45,round_corners=8,hud_compact",
        HudLevel::Full => {
            "fps,frametime,cpu_stats,cpu_temp,cpu_power,gpu_stats,gpu_temp,gpu_power,ram,vram,fan,battery,battery_watt,position=top-left,font_size=22,background_alpha=0.5,round_corners=8,hud_compact"
        }
    })
}

struct State {
    /// The level last applied (Off at start: nothing of ours is running).
    level: HudLevel,
    child: Option<Child>,
}

fn state() -> std::sync::MutexGuard<'static, State> {
    static S: std::sync::OnceLock<Mutex<State>> = std::sync::OnceLock::new();
    S.get_or_init(|| Mutex::new(State { level: HudLevel::Off, child: None })).lock().unwrap_or_else(|e| e.into_inner())
}

/// The X display of gamescope's own server, where mangoapp belongs (Rare itself may live on
/// the games' server).
fn gamescope_display() -> String {
    std::env::var("RARE_GAMESCOPE_DISPLAY").unwrap_or_else(|_| ":0".into())
}

fn stop(s: &mut State) {
    if let Some(mut c) = s.child.take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

/// The session mangoapp's config file, when the session has one and its mangoapp runs.
fn session_file() -> Option<std::path::PathBuf> {
    let f = std::path::PathBuf::from(std::env::var_os("MANGOHUD_CONFIGFILE")?);
    if !f.is_absolute() || !f.is_file() {
        return None;
    }
    let running = Command::new("pgrep").args(["-x", "mangoapp"]).output().is_ok_and(|o| o.status.success());
    running.then_some(f)
}

/// The text of the session's config file for a level: one option per line.
pub fn file_text(level: HudLevel) -> String {
    match preset(level) {
        Some(p) => p.split(',').collect::<Vec<_>>().join("\n") + "\n",
        None => "no_display\n".into(),
    }
}

/// Writes the level into the session's file when it says something else. True when it did.
pub fn write_session_file(path: &std::path::Path, level: HudLevel) -> std::io::Result<bool> {
    use std::io::Write;
    let text = file_text(level);
    if std::fs::read_to_string(path).ok().as_deref() == Some(text.as_str()) {
        return Ok(false);
    }
    let mut f = std::fs::OpenOptions::new().write(true).truncate(true).open(path)?;
    f.write_all(text.as_bytes())?;
    Ok(true)
}

/// Puts the overlay at `level`. Errors only reach the log.
pub fn apply(level: HudLevel) {
    let mut s = state();
    if let Some(f) = session_file() {
        // Steam may have written its own level since: the file is checked every time.
        stop(&mut s);
        s.level = level;
        match write_session_file(&f, level) {
            Ok(true) => log::info!("perfhud: {level:?} through gamescope's mangoapp ({})", f.display()),
            Ok(false) => {}
            Err(e) => log::warn!("perfhud: {}: {e}", f.display()),
        }
        return;
    }
    if s.level == level {
        return;
    }
    stop(&mut s);
    s.level = level;
    let Some(cfg) = preset(level) else { return };
    // A mangoapp the session started on its own would hold the slot at its own level.
    let _ = Command::new("pkill").args(["-x", "mangoapp"]).status();
    match crate::host::env(Command::new("mangoapp").env("MANGOHUD_CONFIG", cfg).env("DISPLAY", gamescope_display())).spawn() {
        Ok(c) => s.child = Some(c),
        Err(e) => log::warn!("perfhud: mangoapp: {e}"),
    }
}

/// The next level when stepping through them (Off, FPS, Basic, Full, Off).
pub fn next(level: HudLevel) -> HudLevel {
    match level {
        HudLevel::Off => HudLevel::Fps,
        HudLevel::Fps => HudLevel::Basic,
        HudLevel::Basic => HudLevel::Full,
        HudLevel::Full => HudLevel::Off,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_grow_and_off_has_no_preset() {
        assert_eq!(preset(HudLevel::Off), None);
        let fps = preset(HudLevel::Fps).unwrap();
        let basic = preset(HudLevel::Basic).unwrap();
        let full = preset(HudLevel::Full).unwrap();
        assert!(fps.contains("fps_only") && !fps.contains("frametime"));
        assert!(basic.contains("frametime") && basic.contains("battery_watt") && !basic.contains("gpu_stats"));
        for k in ["cpu_stats", "gpu_stats", "gpu_temp", "ram", "vram", "fan"] {
            assert!(full.contains(k), "{k}");
        }
    }

    #[test]
    fn session_file_gets_one_option_a_line() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("mangohud.x");
        std::fs::write(&f, "no_display\n").unwrap();
        assert!(!write_session_file(&f, HudLevel::Off).unwrap());
        assert!(write_session_file(&f, HudLevel::Fps).unwrap());
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.starts_with("fps_only\nposition=top-left\n") && text.ends_with("round_corners=8\n"), "{text}");
        assert!(write_session_file(&f, HudLevel::Off).unwrap());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "no_display\n");
    }

    #[test]
    fn stepping_cycles() {
        let mut l = HudLevel::Off;
        let mut seen = vec![];
        for _ in 0..4 {
            l = next(l);
            seen.push(l);
        }
        assert_eq!(seen, [HudLevel::Fps, HudLevel::Basic, HudLevel::Full, HudLevel::Off]);
    }

    #[test]
    fn stored_lowercase_and_missing_is_off() {
        assert_eq!(serde_json::to_string(&HudLevel::Basic).unwrap(), "\"basic\"");
        assert_eq!(HudLevel::default(), HudLevel::Off);
    }
}
