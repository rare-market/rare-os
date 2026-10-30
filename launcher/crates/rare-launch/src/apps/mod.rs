//! Apps: Telegram, X, Netflix, YouTube, Discord, Chrome, Xbox Cloud Gaming and GeForce NOW as
//! console-style media tiles (the cloud apps' games are Library tiles too, see `cloud`).
//! The catalog is shared with the UI (src/apps/catalog.json). Apps run under the launcher's
//! runner as "app:<id>", so Resume, Close and the running dot work as they do for games.
//!
//! - catalog: the catalog and how each app opens on this device (pure, tested)
//! - front:   what has the screen, so the pad goes to the right place
//! - pointer: the pad as a mouse and keyboard for apps that don't read a controller
//! - install: Flathub installs into the user installation, with progress

pub mod catalog;
pub mod cloud;
pub mod front;
pub mod install;
pub mod pointer;

use catalog::{Controller, Env, Plan, WebOpts};
use std::path::{Path, PathBuf};
use rare_core::{host, Ctx, Error, Event};

pub fn flatpak_installed(id: &str) -> bool {
    let user = dirs::data_dir().map(|d| d.join("flatpak/app").join(id).join("current"));
    let system = Path::new("/var/lib/flatpak/app").join(id).join("current");
    user.is_some_and(|u| u.exists()) || system.exists()
}

/// The user installation already knows Flathub (`flatpak --user remotes`, read from disk).
fn user_flathub() -> bool {
    dirs::data_dir()
        .and_then(|d| std::fs::read_to_string(d.join("flatpak/repo/config")).ok())
        .is_some_and(|c| c.contains("[remote \"flathub\"]"))
}

/// What is installed: a few file checks, no processes.
pub fn env() -> Env {
    let (flatpaks, programs) = catalog::wanted();
    let which = rare_library::steam::which;
    let product = std::fs::read_to_string("/sys/class/dmi/id/product_name").unwrap_or_default();
    Env {
        flatpak: which("flatpak"),
        flatpaks: flatpaks.into_iter().filter(|f| flatpak_installed(f)).collect(),
        programs: programs.into_iter().filter(|p| which(p)).collect(),
        home: dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")),
        // The SuiPlay0X1's 7" 1920 x 1200 screen: the same 1.5x Rare's own UI uses.
        scale: (product.trim() == "SuiPlay0X1").then_some(1.5),
        user_flathub: user_flathub(),
    }
}

pub fn statuses() -> Vec<catalog::Status> {
    let e = env();
    catalog::all().iter().map(|a| catalog::status(a, &e)).collect()
}

/// The webview profile for apps in Rare's window. Focus: the d-pad walks the page's own buttons,
/// A clicks, B is Escape, the right stick drives a pointer, View opens the keyboard. Gamepad (cloud
/// gaming): on the way's `padHosts` the page reads the real controller through the Gamepad API and
/// Rare sends it nothing; other hosts (sign-in pages) get focus mode. `wallet` false is app mode
/// (rare-webgame leaves Rare Wallet and the Local API out of the page). Storage is the app's
/// (`dataId`), so its cloud games share the app's sign-in.
fn webview_profile(app_id: &str, controller: Controller, wallet: bool, web: &WebOpts) -> serde_json::Value {
    let mut p = serde_json::json!({ "input": "focus", "pointer": true, "keys": { "b": "Escape", "start": "Enter" }, "wallet": wallet, "dataId": catalog::run_id(app_id) });
    if controller == Controller::Gamepad {
        p["input"] = "native".into();
        p["padHosts"] = serde_json::json!(web.pad_hosts);
    }
    if let Some(ua) = &web.user_agent {
        p["userAgent"] = ua.clone().into();
    }
    if web.stream {
        p["stream"] = true.into();
    }
    // Discord on the web: arRPC's bridge, so the page shows what Rare and games report.
    if app_id == "discord" {
        p["script"] = rare_discord::BRIDGE_JS.into();
    }
    p
}

/// Opens one of a cloud app's games straight into its stream (a Library tile: Xbox Cloud Gaming or
/// GeForce NOW at the game's deep link), in Rare's window with the app's sign-in. Runs as `run_id`.
pub fn open_cloud(ctx: &Ctx, runner: &crate::launch::Runner, app_id: &str, run_id: &str, title: &str, url: &str) -> Result<(), Error> {
    let entry = catalog::get(app_id).ok_or("Unknown app")?;
    let Plan::Webview { controller, wallet, web, .. } = catalog::plan(entry, Some(0), &env())? else {
        return Err(Error::new(format!("{} games open in Rare's window", entry.name)));
    };
    ctx.host.open_web_window(run_id, title, url, Some(webview_profile(app_id, controller, wallet, &web)))?;
    runner.track_window(run_id, rare_core::WEB_WINDOW);
    front::set_pointer(run_id, None);
    front::to_front(run_id);
    Ok(())
}

/// Opens an app (with way `way`, or its default). Returns what it opened in.
pub fn open(ctx: &Ctx, runner: &crate::launch::Runner, id: &str, way: Option<usize>) -> Result<String, Error> {
    let entry = catalog::get(id).ok_or("Unknown app")?;
    let e = env();
    let plan = catalog::plan(entry, way, &e)?;
    let run_id = catalog::run_id(id);
    let label = way.and_then(|i| entry.ways.get(i)).map(|w| w.label.clone()).or_else(|| catalog::status(entry, &e).opens_in).unwrap_or_default();
    // Opening again while it runs just brings it back.
    if runner.running().contains(&run_id) {
        resume(ctx, runner, id)?;
        return Ok(label);
    }
    match plan {
        Plan::Webview { url, controller, wallet, web } => {
            ctx.host.open_web_window(&run_id, &entry.name, &url, Some(webview_profile(id, controller, wallet, &web)))?;
            runner.track_window(&run_id, rare_core::WEB_WINDOW);
            front::set_pointer(&run_id, None);
        }
        Plan::Steam { url } => {
            steam(ctx, runner, &url)?;
            return Ok(label);
        }
        Plan::Spawn { program, args, controller, back } => {
            if controller == Controller::Pointer {
                pointer::prepare();
            }
            let (mut cmd, unit) = crate::launch::host_command_unit(&program, &args, &run_id);
            let child = cmd.spawn().map_err(|err| format!("Could not open {}: {err}", entry.name))?;
            runner.track_child(&run_id, child, unit);
            front::set_pointer(&run_id, (controller == Controller::Pointer).then_some(back));
            step_aside(ctx);
        }
    }
    front::to_front(&run_id);
    Ok(label)
}

/// The launcher steps aside so the app's own window shows (desktop sessions; in Game Mode
/// gamescope shows the newest window).
pub fn step_aside(ctx: &Ctx) {
    ctx.host.step_aside();
}

/// Steam's run id: its own UI, an app like the others (catalog "steam").
pub const STEAM_RUN: &str = "app:steam";

/// How long the launcher shows "Press the Rare button to come back to Rare" before Steam takes the screen.
pub const STEAM_HINT: std::time::Duration = std::time::Duration::from_millis(1600);

/// Opens Steam's own UI at `url` (Big Picture, a store page, a game's page) as the app
/// "app:steam": Steam gets the screen once the launcher has shown how to come back (the
/// `steam-handoff` event), unless the player went somewhere else in the meantime. Home brings
/// Rare back; Steam stays in the Running list with Resume and Close until it quits.
pub fn steam(ctx: &Ctx, runner: &crate::launch::Runner, url: &str) -> Result<(), Error> {
    crate::launch::steam_url(url)?;
    runner.track_steam_ui(STEAM_RUN);
    front::mark_steam(STEAM_RUN);
    let gen = front::generation();
    ctx.send(Event::SteamHandoff(STEAM_HINT.as_millis() as u64));
    std::thread::spawn(move || {
        std::thread::sleep(STEAM_HINT);
        if front::to_front_if(gen, STEAM_RUN) {
            host::global().log("[steam] Steam has the screen; Home brings Rare back");
        }
    });
    Ok(())
}

/// Shows only a dialog of Steam's (an uninstall or install confirmation, a game's page action) and
/// then Rare again: Steam's UI opens at `url` the way `steam` does (the hint first), and once
/// `done()` says the job is through, or after `max`, Rare takes the screen back and Steam's UI
/// leaves the Running list (Steam keeps running). If the player pressed the Rare button in the
/// meantime, Rare has the screen already; Steam's UI still leaves the list. For other areas
/// (uninstall, installs, storage): `done` is checked every 500 ms off the calling thread.
pub fn steam_dialog(ctx: &Ctx, runner: &crate::launch::Runner, url: &str, done: impl Fn() -> bool + Send + 'static, max: std::time::Duration) -> Result<(), Error> {
    steam(ctx, runner, url)?;
    let runner = runner.clone();
    std::thread::spawn(move || {
        let t0 = std::time::Instant::now();
        std::thread::sleep(STEAM_HINT);
        while !done() && t0.elapsed() < max {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        runner.untrack_steam_ui();
        if front::front_run().as_deref() == Some(STEAM_RUN) {
            front::to_launcher();
        }
        host::global().log("[steam] Steam's dialog is done; Rare has the screen");
    });
    Ok(())
}

/// Puts a running app back on screen.
pub fn resume(ctx: &Ctx, runner: &crate::launch::Runner, id: &str) -> Result<(), Error> {
    let run_id = catalog::run_id(id);
    if !runner.running().contains(&run_id) {
        return Err(Error::new("That app is not open"));
    }
    if run_id == STEAM_RUN {
        return steam(ctx, runner, "steam://open/bigpicture");
    }
    front::to_front(&run_id);
    if ctx.host.current_web_id().as_deref() == Some(run_id.as_str()) && ctx.host.resume_web() {
        return Ok(());
    }
    step_aside(ctx);
    Ok(())
}

/// The input thread's first stop for every press and release. Returns true when pointer mode
/// took it (an app in front that doesn't read the controller).
pub fn route_pad(running: &[String], button: rare_core::Pad, down: bool) -> bool {
    front::sync(running);
    let Some(back) = front::pointer_back() else { return false };
    pointer::button(button, down, back)
}
