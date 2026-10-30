//! The launch flows both front ends share, as plain functions over a `Ctx` and the `Runner`
//! (the Tauri commands in src-tauri/src/lib.rs are one-line calls into these).

use crate::apps::front;
use crate::launch::{self, Runner};
use crate::steamcfg;
use rare_core::{Ctx, Error, WEB_WINDOW};
use rare_library::model::{Game, LaunchSpec};
use std::time::Duration;

/// What a started game needs next from the caller: power settings go on now, and the load boost
/// window opens after them. `boost_target` is the app id gamescope shows the game under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launched {
    /// A web game or browser: nothing to boost.
    pub boost: bool,
    pub boost_target: Option<u64>,
}

/// Opens a web game: in Rare's own sandboxed window, or in a Chromium browser when its profile
/// says it needs one (the browser's name comes back then).
pub fn open_web_game(ctx: &Ctx, runner: &Runner, id: &str, title: &str, url: &str) -> Result<Option<String>, Error> {
    let wants_chrome = rare_library::profiles::for_game(id, url).is_some_and(|(_, p)| rare_library::profiles::wants_chrome(&p));
    if wants_chrome {
        return Ok(runner.launch_browser(id, url).map(Some)?);
    }
    ctx.host.open_web_window(id, title, url, None)?;
    runner.track_window(id, WEB_WINDOW);
    Ok(None)
}

/// Opens a browser game without installing it (from the Sui Games catalog).
pub fn webgame_open(ctx: &Ctx, runner: &Runner, id: &str, title: &str, url: &str) -> Result<Option<String>, Error> {
    let id = web_id(id);
    let r = open_web_game(ctx, runner, &id, title, url)?;
    front::to_front(&id);
    Ok(r)
}

/// Opens a web game in a Chromium browser on request ("Open in Chrome").
pub fn webgame_open_browser(runner: &Runner, id: &str, url: &str) -> Result<String, Error> {
    let id = web_id(id);
    let name = runner.launch_browser(&id, url)?;
    // Chrome has the screen (Rare OS game mode puts its app id first).
    front::to_front(&id);
    Ok(name)
}

fn web_id(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("web:{id}")
    }
}

/// Starts a game. `prepare` runs first for anything that is not a Steam game or a web game (the
/// emulator first-run settings and power seeding live in rare-emu, which depends on this crate).
/// The caller then puts the game's power settings on, opens the load boost window, and calls
/// `front::to_front(&game.id)`.
pub fn launch_game(ctx: &Ctx, runner: &Runner, g: &Game, prepare: impl FnOnce(&Game)) -> Result<Launched, Error> {
    let mut out = Launched { boost: false, boost_target: None };
    if let LaunchSpec::Web { url } = &g.launch {
        open_web_game(ctx, runner, &g.id, &g.title, url)?;
    } else if let LaunchSpec::Cloud { app, url } = &g.launch {
        crate::apps::open_cloud(ctx, runner, app, &g.id, &g.title, url)?;
    } else {
        if !matches!(g.launch, LaunchSpec::Steam { .. }) {
            prepare(g);
        }
        runner.launch(g)?;
        out.boost = true;
        out.boost_target = match &g.launch {
            LaunchSpec::Steam { appid } => Some(*appid as u64),
            // A non-Steam shortcut shows in gamescope under its 32-bit app id.
            LaunchSpec::SteamShortcut { gameid } => Some(*gameid >> 32),
            _ => None,
        };
    }
    if matches!(g.launch, LaunchSpec::Steam { .. } | LaunchSpec::SteamShortcut { .. }) {
        front::mark_steam(&g.id);
        // Rare keeps the screen until the game has a window or Steam asks the player something.
        front::steam_pending(&g.id);
    }
    Ok(out)
}

/// Puts a running game back on screen: the web game window, or the launcher steps aside so the
/// game under it shows (desktop sessions; in Game Mode gamescope shows the game).
pub fn game_resume(ctx: &Ctx, runner: &Runner, id: &str) -> Result<(), Error> {
    if !runner.running().iter().any(|r| r == id) {
        return Err(Error::new("That game is not running"));
    }
    front::to_front(id);
    if (id.starts_with("web:") || id.starts_with("cloud:")) && ctx.host.resume_web() {
        return Ok(());
    }
    ctx.host.step_aside();
    Ok(())
}

/// Starts Steam quietly ahead of a Play press; "demand" refuses.
pub fn steam_prewarm(steam_mode: &str) -> Result<(), Error> {
    if steam_mode == "demand" {
        return Ok(());
    }
    Ok(launch::steam_prewarm()?)
}

/// Sets (or clears) one Steam game's Proton. Steam has to be closed to edit its files, so this
/// closes it quietly (never while a game runs) and starts it again afterwards when `restart`.
/// Blocks; run it off the UI thread.
pub fn proton_set_game(runner: &Runner, appid: u64, tool: Option<&str>, restart: bool) -> Result<(), Error> {
    if !runner.running().is_empty() {
        return Err(Error::new("Close the running game first."));
    }
    let root = steamcfg::primary_root().ok_or("Steam is not installed.")?;
    if launch::steam_running() {
        let _ = launch::host_command("steam", &["-shutdown".to_string()], "steam").status();
        for _ in 0..60 {
            if !launch::steam_running() {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        if launch::steam_running() {
            return Err(Error::new("Steam did not close. Close it and try again."));
        }
    }
    let r = steamcfg::set_game_tool(&root, appid, tool, &|| !launch::steam_running());
    if restart {
        let _ = launch::steam_prewarm();
    }
    Ok(r?)
}

/// Closes the Steam client (for edits of its files); Err when it won't go.
fn close_steam() -> Result<(), Error> {
    if launch::steam_running() {
        let _ = launch::host_command("steam", &["-shutdown".to_string()], "steam").status();
        for _ in 0..60 {
            if !launch::steam_running() {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        if launch::steam_running() {
            return Err(Error::new("Steam did not close. Close it and try again."));
        }
    }
    Ok(())
}

/// A Steam game's launch options (empty when none; Err when Steam is not installed).
pub fn steam_launch_options(appid: u64) -> Result<String, Error> {
    let root = steamcfg::primary_root().ok_or("Steam is not installed.")?;
    Ok(steamcfg::launch_options(&root, appid).unwrap_or_default())
}

/// Sets a Steam game's launch options (Steam keeps them in localconfig.vdf, which it rewrites on
/// exit, so Steam is closed around the edit and started again when `restart`).
pub fn steam_set_launch_options(runner: &Runner, appid: u64, opts: &str, restart: bool) -> Result<(), Error> {
    if !runner.running().is_empty() {
        return Err(Error::new("Close the running game first."));
    }
    let root = steamcfg::primary_root().ok_or("Steam is not installed.")?;
    close_steam()?;
    let r = steamcfg::set_launch_options(&root, appid, opts, &|| !launch::steam_running());
    if restart {
        let _ = launch::steam_prewarm();
    }
    Ok(r?)
}

/// Adds a games folder to Steam's libraryfolders.vdf. Steam rewrites that file while it runs, so a
/// running Steam is closed around the edit and started again when `restart` (or it was running).
pub fn steam_add_library(runner: &Runner, path: &str, restart: bool) -> Result<(), Error> {
    if runner.running().iter().any(|r| r.starts_with("steam:")) {
        return Err(Error::new("Close the running Steam game first."));
    }
    let was = launch::steam_running();
    close_steam()?;
    let r = rare_library::steam::add_library(path);
    if restart || was {
        let _ = launch::steam_prewarm();
    }
    r.map_err(Error::new)
}

/// Proton tools, the global default and one game's override.
pub fn proton_status(appid: Option<u64>) -> Result<steamcfg::ProtonStatus, Error> {
    let root = steamcfg::primary_root().ok_or("Steam is not installed.")?;
    Ok(steamcfg::status(&root, appid))
}

/// Installs GE-Proton ("ge") or Proton-CachyOS ("cachyos"). Blocks; progress arrives as events.
pub fn proton_install(ctx: &Ctx, kind: &str) -> Result<String, Error> {
    let root = steamcfg::primary_root().ok_or("Steam is not installed.")?;
    Ok(steamcfg::install(ctx, &root, kind)?)
}

/// Removes an installed community Proton.
pub fn proton_remove(name: &str) -> Result<(), Error> {
    let root = steamcfg::primary_root().ok_or("Steam is not installed.")?;
    Ok(steamcfg::remove_tool(&root, name)?)
}

/// Turns one DLC of a Steam game on or off (Steam's Properties > DLC). Steam rewrites its
/// manifests while it runs, so this closes it quietly first (never while a game runs) and starts
/// it again afterwards when `restart`. Blocks; run it off the UI thread.
pub fn dlc_set(runner: &Runner, appid: u32, dlc: u32, on: bool, restart: bool) -> Result<(), Error> {
    if !runner.running().is_empty() {
        return Err(Error::new("Close the running game first."));
    }
    let was = launch::steam_running();
    if was {
        let _ = launch::host_command("steam", &["-shutdown".to_string()], "steam").status();
        for _ in 0..60 {
            if !launch::steam_running() {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        if launch::steam_running() {
            return Err(Error::new("Steam did not close. Close it and try again."));
        }
    }
    let r = rare_library::steam_dlc::set_enabled(appid, dlc, on);
    if restart && was {
        let _ = launch::steam_prewarm();
    }
    r.map_err(Error::new)
}

/// Steam's own UI as the app "app:steam" (Big Picture).
pub fn steam_big_picture(ctx: &Ctx, runner: &Runner) -> Result<(), Error> {
    crate::apps::steam(ctx, runner, "steam://open/bigpicture")
}

/// A game's own page in Steam (Properties, Verify files, Launch options, Cloud one press away), in
/// Steam's UI as "app:steam": the Rare button and Steam's Switch to Desktop lead back.
pub fn steam_game_page(ctx: &Ctx, runner: &Runner, appid: u32) -> Result<(), Error> {
    crate::apps::steam(ctx, runner, &format!("steam://nav/games/details/{appid}"))
}

/// Steam's store page for an app, in Steam (the same "app:steam").
pub fn steam_store(ctx: &Ctx, runner: &Runner, appid: u32) -> Result<(), Error> {
    crate::apps::steam(ctx, runner, &format!("steam://store/{appid}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_ids_get_their_prefix_once() {
        assert_eq!(web_id("act"), "web:act");
        assert_eq!(web_id("web:act"), "web:act");
    }

    #[test]
    fn resume_refuses_a_game_that_is_not_running() {
        let r = Runner::default();
        assert_eq!(game_resume(&Ctx::headless(), &r, "steam:1").unwrap_err().to_string(), "That game is not running");
    }

    #[test]
    fn demand_mode_never_prewarms() {
        assert!(steam_prewarm("demand").is_ok());
    }
}
