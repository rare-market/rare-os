mod apps;
mod brightness;
mod devmode;
mod display;
mod config;
mod ctx;
mod deckpad;
mod emu;
mod freeze;
mod gamepad;
mod gyro;
mod hud;
mod inputplumber;
mod instant;
mod journey;
mod launch;
mod library;
mod localapi;
mod model;
mod network;
mod onboard;
mod osupdate;
mod perfhud;
mod overlay;
mod powerbtn;
mod radios;
mod power;
mod render;
mod share;
mod sleepwatch;
mod rare_ctx;
mod store;
mod steamcfg;
mod stream;
mod sui;
mod sysbuttons;
mod syskeys;
mod sysglue;
mod system;
mod trophies;
mod upscale;
mod wallet;
mod webgame;

use model::{Game, Library};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_deep_link::DeepLinkExt;

struct AppState {
    library: Mutex<Option<Library>>,
    runner: launch::Runner,
    config: Mutex<config::Config>,
}

fn find_game(state: &State<AppState>, id: &str) -> Result<Game, String> {
    let lib = state.library.lock().unwrap();
    lib.as_ref().and_then(|l| l.games.iter().find(|g| g.id == id).cloned()).ok_or_else(|| "Game not found, rescan the library".into())
}

#[tauri::command]
async fn library_scan(state: State<'_, AppState>) -> Result<Library, String> {
    let (folders, web, removed) = {
        let c = state.config.lock().unwrap();
        (c.game_folders.clone(), c.web_games.clone(), c.removed_builtins.clone())
    };
    let t0 = std::time::Instant::now();
    let lib = tauri::async_runtime::spawn_blocking(move || library::scan(&folders, &web, &removed)).await.map_err(|e| e.to_string())?;
    eprintln!("library: {} in {} ms (done at {} ms since start)", scan_summary(&lib), t0.elapsed().as_millis(), since_start());
    let cached = lib.clone();
    std::thread::spawn(move || library::cache::save(&cached));
    *state.library.lock().unwrap() = Some(lib.clone());
    Ok(lib)
}

/// The last scan, from disk, so the library shows at once while a fresh scan runs.
#[tauri::command]
fn library_cached(state: State<AppState>) -> Option<Library> {
    state.library.lock().unwrap().clone()
}

/// "4 steam, 3 apps, 1 web": counts per source, in the order of `lib.sources`.
fn scan_summary(lib: &Library) -> String {
    let parts: Vec<String> = lib
        .sources
        .iter()
        .filter(|s| s.count > 0)
        .map(|s| format!("{} {}", s.count, serde_json::to_value(s.source).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()))
        .collect();
    if parts.is_empty() { "nothing found".into() } else { parts.join(", ") }
}

/// A short controller rumble: kind is "move", "select" or "back".
#[tauri::command]
fn pad_rumble(state: State<AppState>, kind: String) -> Result<(), String> {
    if state.config.lock().unwrap().rumble == Some(false) {
        return Ok(());
    }
    let strength = state.config.lock().unwrap().rumble_strength.unwrap_or(100).min(100) as f32 / 100.0;
    gamepad::rumble(&kind, strength)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DebugEnv {
    nav: Option<String>,
    /// RARE_DEBUG_INPUT=1: log every input source and arbitration decision.
    input: bool,
}

/// Debug switches also work for a launcher started by Steam (where env can't be set): a file
/// $XDG_RUNTIME_DIR/rare-debug-nav holds a nav script and is removed once read, and while
/// one was found, UI log lines also go to $XDG_RUNTIME_DIR/rare-launcher-debug.log.
fn runtime_file(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|d| std::path::PathBuf::from(d).join(name))
}

static DEBUG_LOG_FILE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The WebKit renderer (render.rs), so the page decodes art the way that renderer needs.
#[tauri::command]
fn render_mode() -> &'static str {
    render::mode_name(&|k: &str| std::env::var(k).ok())
}

#[tauri::command]
fn debug_env() -> DebugEnv {
    let mut nav = std::env::var("RARE_DEBUG_NAV").ok();
    if nav.is_none() {
        if let Some(f) = runtime_file("rare-debug-nav") {
            if let Ok(t) = std::fs::read_to_string(&f) {
                let _ = std::fs::remove_file(&f);
                nav = Some(t.trim().to_string()).filter(|t| !t.is_empty());
                DEBUG_LOG_FILE.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }
    let input = crate::gamepad::debug_input() || runtime_file("rare-debug-input").is_some_and(|f| f.exists());
    if input {
        DEBUG_LOG_FILE.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    DebugEnv { nav, input }
}

/// Lets the UI write to the launcher's log (the journal on Rare OS).
#[tauri::command]
fn debug_log(msg: String) {
    let msg: String = msg.chars().take(2000).collect();
    eprintln!("[ui] {msg}");
    if DEBUG_LOG_FILE.load(std::sync::atomic::Ordering::Relaxed) {
        if let Some(f) = runtime_file("rare-launcher-debug.log") {
            use std::io::Write;
            if let Ok(mut h) = std::fs::OpenOptions::new().create(true).append(true).open(f) {
                let _ = writeln!(h, "[ui] {msg}");
            }
        }
    }
}

#[tauri::command]
fn game_launch(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    let g = if let Some(slug) = id.strip_prefix("rare:") {
        // Offline grace for games with a launch check; the UI checks online first.
        store::launch_gate(slug)?;
        // Just installed: not in the cached library until the next scan.
        find_game(&state, &id).or_else(|e| store::library_games().into_iter().find(|g| g.id == id).ok_or(e))?
    } else {
        find_game(&state, &id)?
    };
    let ctx = rare_ctx::ctx(&app);
    let launched = rare_launch::api::launch_game(&ctx, &state.runner, &g, |g| {
        // Emulators with no config yet get the tuned first-run settings (never overwrites).
        let program = if let model::LaunchSpec::Exec { program, .. } = &g.launch { Some(program.as_str()) } else { None };
        emu::prepare_launch(program, &g.title);
        // Its power profile, until the user saves one for this game (never overwrites a saved one).
        let mut c = state.config.lock().unwrap();
        if emu::seed_power(&mut c, &g.id, program, &g.title) {
            let _ = config::save(&c);
        }
    })?;
    // CPU boost while it loads (Balanced and Performance keep the profile's setting after).
    let mut load_boost: Option<Box<dyn FnOnce() + Send>> = None;
    if launched.boost {
        let runner = state.runner.clone();
        let (id, target) = (g.id.clone(), launched.boost_target);
        load_boost = Some(Box::new(move || system::load_boost(id, target, move |id| runner.running().iter().any(|r| r == id))));
    }
    // Its power settings (saved for it, or Balanced) go on now, and the load boost window opens
    // after them (it follows the game's profile); the runner restores the launcher's when it ends.
    power::started(&app, &g.id, move || {
        if let Some(f) = load_boost {
            f();
        }
    });
    apps::front::to_front(&g.id);
    Ok(())
}

/// Opens a browser game without installing it (from the Sui Games catalog).
#[tauri::command]
fn webgame_open(app: AppHandle, state: State<AppState>, id: String, title: String, url: String) -> Result<Option<String>, String> {
    rare_launch::api::webgame_open(&rare_ctx::ctx(&app), &state.runner, &id, &title, &url).map_err(String::from)
}

/// Opens a web game in a Chromium browser on request ("Open in Chrome").
#[tauri::command]
fn webgame_open_browser(state: State<AppState>, id: String, url: String) -> Result<String, String> {
    rare_launch::api::webgame_open_browser(&state.runner, &id, &url).map_err(String::from)
}

/// The Chromium browser web games fall back to, if one is installed.
#[tauri::command]
fn browser_info() -> Option<library::browser::Browser> {
    library::browser::find()
}

/// Ends a running game. A stream also quits the game on the PC (in the background).
#[tauri::command]
fn game_quit(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    if let Some((uuid, _)) = stream::store::parse_run_id(&id) {
        std::thread::spawn(move || {
            if let Err(e) = stream::quit(&app, &uuid) {
                eprintln!("[stream] quit: {e}");
            }
        });
        return Ok(());
    }
    state.runner.quit(&rare_ctx::ctx(&app), &id).map_err(String::from)
}

/// Puts a running game back on screen: the web game window, or the launcher steps aside so
/// the game under it shows (desktop sessions; in Game Mode gamescope shows the game).
#[tauri::command]
fn game_resume(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    rare_launch::api::game_resume(&rare_ctx::ctx(&app), &state.runner, &id).map_err(String::from)
}

/// The UI tells the native side which Rare Wallet account web games may see.
#[tauri::command]
fn wallet_set_session(session: webgame::WalletSession) {
    webgame::set_session(session);
}

/// Apps on this device: how each opens, and what to install first.
#[tauri::command]
async fn apps_status() -> Result<Vec<apps::catalog::Status>, String> {
    tauri::async_runtime::spawn_blocking(apps::statuses).await.map_err(|e| e.to_string())
}

/// Opens an app (its default way, or `way`); returns what it opened in.
#[tauri::command]
fn app_open(app: AppHandle, state: State<AppState>, id: String, way: Option<usize>) -> Result<String, String> {
    apps::open(&rare_ctx::ctx(&app), &state.runner, &id, way).map_err(String::from)
}

#[tauri::command]
fn app_resume(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    apps::resume(&rare_ctx::ctx(&app), &state.runner, &id).map_err(String::from)
}

/// Installs the Flatpak an app needs from Flathub, for this user (no sudo). Progress comes as
/// `app-install` events.
#[tauri::command]
fn app_install(app: AppHandle, id: String) -> Result<(), String> {
    let e = apps::env();
    let entry = apps::catalog::get(&id).ok_or("Unknown app")?;
    let offer = apps::catalog::status(entry, &e).offer.ok_or("Nothing to install")?;
    apps::install::start(rare_ctx::ctx(&app), id, offer.flatpak, e.user_flathub).map_err(String::from)
}

#[tauri::command]
fn running_games(state: State<AppState>) -> Vec<String> {
    state.runner.running()
}

/// Starts Steam quietly ahead of a Play press (the UI calls it when a Steam game is selected and
/// the setting is "focus"; "demand" refuses).
#[tauri::command]
fn steam_prewarm(state: State<AppState>) -> Result<(), String> {
    let mode = state.config.lock().unwrap().steam_mode().to_string();
    rare_launch::api::steam_prewarm(&mode).map_err(String::from)
}

/// Proton tools, the global default and one game's override (Steam's own CompatToolMapping).
#[tauri::command]
fn proton_status(appid: Option<u64>) -> Result<steamcfg::ProtonStatus, String> {
    rare_launch::api::proton_status(appid).map_err(String::from)
}

/// Installs GE-Proton ("ge") or Proton-CachyOS ("cachyos") from the official GitHub release, after
/// its SHA-512 matches the release's checksum file. Progress arrives as `proton-install` events.
#[tauri::command]
async fn proton_install(app: AppHandle, kind: String) -> Result<String, String> {
    let ctx = rare_ctx::ctx(&app);
    tauri::async_runtime::spawn_blocking(move || rare_launch::api::proton_install(&ctx, &kind).map_err(String::from)).await.map_err(|e| e.to_string())?
}

/// Sets (or clears, with null) one Steam game's Proton. Steam has to be closed to edit its files, so
/// this closes it quietly (never while a game runs) and starts it again afterwards.
#[tauri::command]
async fn proton_set_game(state: State<'_, AppState>, appid: u64, tool: Option<String>) -> Result<(), String> {
    let restart = state.config.lock().unwrap().steam_mode() != "demand";
    let runner = state.runner.clone();
    tauri::async_runtime::spawn_blocking(move || rare_launch::api::proton_set_game(&runner, appid, tool.as_deref(), restart).map_err(String::from))
        .await
        .map_err(|e| e.to_string())?
}

/// Removes an installed community Proton (the user pressed Remove).
#[tauri::command]
fn proton_remove(name: String) -> Result<(), String> {
    rare_launch::api::proton_remove(&name).map_err(String::from)
}

/// "steam", "game", "playing" or "none" for a launched game; see Runner::phase.
#[tauri::command]
fn launch_phase(state: State<AppState>, id: String) -> &'static str {
    state.runner.phase(&id)
}

/// Steam's own UI, as the app "app:steam" (apps::steam): Home comes back to Rare, and Steam stays
/// in the Running list until it quits.
#[tauri::command]
fn steam_big_picture(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    rare_launch::api::steam_big_picture(&rare_ctx::ctx(&app), &state.runner).map_err(String::from)
}

/// Steam's store page for an app, in Steam (the same "app:steam").
#[tauri::command]
fn steam_store(app: AppHandle, state: State<AppState>, appid: u32) -> Result<(), String> {
    rare_launch::api::steam_store(&rare_ctx::ctx(&app), &state.runner, appid).map_err(String::from)
}

#[tauri::command]
async fn wifi_scan() -> Result<Vec<onboard::WifiNet>, String> {
    tauri::async_runtime::spawn_blocking(onboard::wifi_scan).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn wifi_connect(ssid: String, password: Option<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || onboard::wifi_connect(&ssid, password.as_deref())).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn wifi_forget(ssid: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || onboard::wifi_forget(&ssid)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn radios_get() -> radios::Radios {
    tauri::async_runtime::spawn_blocking(radios::state).await.unwrap_or_default()
}

#[tauri::command]
async fn radios_set(kind: String, on: bool) -> Result<radios::Radios, String> {
    tauri::async_runtime::spawn_blocking(move || radios::set(&kind, on)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn net_status() -> onboard::NetStatus {
    tauri::async_runtime::spawn_blocking(onboard::net_status).await.unwrap()
}

#[tauri::command]
async fn steam_account() -> Option<String> {
    tauri::async_runtime::spawn_blocking(onboard::steam_account).await.unwrap_or(None)
}

#[tauri::command]
fn steam_add_library(path: String) -> Result<(), String> {
    library::steam::add_library(&path)
}

#[tauri::command]
async fn device_info() -> system::DeviceInfo {
    tauri::async_runtime::spawn_blocking(system::device_info).await.unwrap()
}

#[tauri::command]
async fn perf_get(state: State<'_, AppState>) -> Result<system::Perf, String> {
    // While a game runs: that game's settings.
    let w = power::wanted_now(&state.config.lock().unwrap());
    tauri::async_runtime::spawn_blocking(move || system::perf_get(&w)).await.map_err(|e| e.to_string())
}

/// A profile (its TDP, CPU boost and power preferences), or the slider's watts with "custom".
/// A call that only carries `fps` for the current profile leaves the power settings alone.
/// While a game runs this changes that game's settings (power.rs), never the launcher's own.
#[tauri::command]
async fn perf_set(state: State<'_, AppState>, profile: system::Profile, tdp: Option<f64>, fps: Option<u32>) -> Result<(), String> {
    let power = {
        let mut c = state.config.lock().unwrap();
        let w = rare_system::power::perf_set_config(&mut *c, profile, tdp, fps);
        config::save(&c)?;
        w
    };
    tauri::async_runtime::spawn_blocking(move || rare_system::power::perf_set_apply(tdp, power, fps)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn perf_boost(state: State<'_, AppState>, on: bool) -> Result<(), String> {
    {
        let mut c = state.config.lock().unwrap();
        rare_system::power::perf_boost_config(&mut *c, on);
        config::save(&c)?;
    }
    tauri::async_runtime::spawn_blocking(move || system::set_cpu_boost(on)).await.map_err(|e| e.to_string())?
}

/// The game whose power settings quick settings change, and whether they are saved for it.
#[tauri::command]
fn game_power_get(state: State<AppState>) -> Option<power::GameView> {
    power::view(&state.config.lock().unwrap())
}

/// "Apply to this game" in quick settings: save the current settings for the running game, or
/// forget them (Balanced next time).
#[tauri::command]
fn game_power_save(state: State<AppState>, save: bool) -> Result<(), String> {
    let mut c = state.config.lock().unwrap();
    if !power::save_for_game(&mut c, save) {
        return Err("No game is running".into());
    }
    config::save(&c)
}

/// The performance overlay level now: the running game's, or the launcher's own.
#[tauri::command]
fn hud_get(state: State<AppState>) -> perfhud::HudLevel {
    power::wanted_hud(&state.config.lock().unwrap())
}

/// Sets the performance overlay level. In a game it is that game's (remembered); with none it is the launcher's.
#[tauri::command]
async fn hud_set(state: State<'_, AppState>, level: perfhud::HudLevel) -> Result<(), String> {
    {
        let mut c = state.config.lock().unwrap();
        rare_system::power::hud_set_config(&mut *c, level);
        config::save(&c)?;
    }
    tauri::async_runtime::spawn_blocking(move || perfhud::apply(level)).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn perf_fan(mode: system::FanMode) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || system::set_fan(mode)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn battery_care_get() -> Option<system::BatteryCare> {
    tauri::async_runtime::spawn_blocking(system::battery_care).await.ok().flatten()
}

#[tauri::command]
async fn battery_care_set(on: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || system::set_battery_care(on)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn levels_get() -> system::Levels {
    system::levels()
}

#[tauri::command]
fn levels_set(brightness: Option<u8>, volume: Option<u8>) -> Result<(), String> {
    if let Some(b) = brightness {
        system::set_brightness(b)?;
    }
    if let Some(v) = volume {
        system::set_volume(v)?;
    }
    Ok(())
}

#[tauri::command]
async fn steam_downloads() -> Vec<library::steam::Download> {
    tauri::async_runtime::spawn_blocking(library::steam::downloads).await.unwrap_or_default()
}

#[tauri::command]
async fn os_update_status() -> osupdate::OsUpdate {
    tauri::async_runtime::spawn_blocking(osupdate::status).await.unwrap_or_default()
}

#[tauri::command]
async fn os_update_check() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(osupdate::check_now).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn os_restore_available() -> bool {
    tauri::async_runtime::spawn_blocking(osupdate::restore_available).await.unwrap_or(false)
}

#[tauri::command]
async fn os_restore(reboot: bool) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || osupdate::restore(reboot)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn os_boot_menu_next() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(osupdate::boot_menu_next).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn power(action: String) -> Result<(), String> {
    system::power(&action)
}

#[tauri::command]
fn config_get(state: State<AppState>) -> config::Config {
    static FIRST: std::sync::Once = std::sync::Once::new();
    FIRST.call_once(|| eprintln!("[startup] page is running: first config request at {} ms", since_start()));
    state.config.lock().unwrap().clone()
}

#[tauri::command]
fn config_set(state: State<AppState>, mut value: config::Config) -> Result<(), String> {
    // Per-game power is kept by its own commands; a settings write from the UI never drops it.
    value.game_power = state.config.lock().unwrap().game_power.clone();
    config::save(&value)?;
    *state.config.lock().unwrap() = value;
    Ok(())
}

/// Cover art from SteamGridDB, when the user has added an API key.
#[tauri::command]
async fn art_lookup(state: State<'_, AppState>, title: String) -> Result<Vec<String>, String> {
    let Some(key) = state.config.lock().unwrap().steamgriddb_key.clone().filter(|k| !k.is_empty()) else {
        return Ok(vec![]);
    };
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<String>, String> {
        let auth = format!("Bearer {key}");
        let term: String = title.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
        let search: serde_json::Value = ureq::get(&format!("https://www.steamgriddb.com/api/v2/search/autocomplete/{}", term.trim().replace(' ', "%20")))
            .set("Authorization", &auth)
            .call()
            .map_err(|e| e.to_string())?
            .into_json()
            .map_err(|e| e.to_string())?;
        let Some(id) = search.pointer("/data/0/id").and_then(|v| v.as_u64()) else { return Ok(vec![]) };
        let grids: serde_json::Value = ureq::get(&format!("https://www.steamgriddb.com/api/v2/grids/game/{id}?dimensions=600x900"))
            .set("Authorization", &auth)
            .call()
            .map_err(|e| e.to_string())?
            .into_json()
            .map_err(|e| e.to_string())?;
        Ok(grids["data"].as_array().map(|a| a.iter().filter_map(|g| g["url"].as_str().map(String::from)).take(3).collect()).unwrap_or_default())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// GET from the rare.market API (it sends no CORS headers, so the webview can't fetch it directly),
/// through the data cache (instant.rs): display data answers from memory or disk at once and is
/// revalidated behind; data a transaction is built from is always fetched.
#[tauri::command]
async fn rare_get(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<Box<serde_json::value::RawValue>, String> {
    if !instant::market_path_ok(&path) {
        return Err("path not allowed".into());
    }
    let key = instant::market_key(&state.config.lock().unwrap(), &path);
    let store = app.state::<instant::Instant>().state.clone();
    tauri::async_runtime::spawn_blocking(move || instant::read(&store, &key)).await.map_err(|e| e.to_string())?
}

/// POST (JSON) to the rare.market API: only the gas sponsor routes. Answers `{ status, body }` so
/// the API's error codes (sponsor unavailable, refused, expired) reach the UI.
#[tauri::command]
async fn rare_post(state: State<'_, AppState>, path: String, body: serde_json::Value) -> Result<serde_json::Value, String> {
    if !(path == "/v1/sponsor" || path == "/v1/sponsor/execute") {
        return Err("path not allowed".into());
    }
    let url = network::sponsor_url(&state.config.lock().unwrap(), &path);
    tauri::async_runtime::spawn_blocking(move || {
        let res = ureq::post(&url)
            .set("User-Agent", "RareLauncher/0.1")
            .timeout(std::time::Duration::from_secs(30))
            .send_json(body);
        let resp = match res {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => return Err(e.to_string()),
        };
        let status = resp.status();
        let body: serde_json::Value = if status == 204 { serde_json::Value::Null } else { resp.into_json().unwrap_or(serde_json::Value::Null) };
        Ok(serde_json::json!({ "status": status, "body": body }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The Trade tab's market data and swap API (token lists, charts, quotes, unsigned transaction
/// builds). Most of these paths send no CORS headers, so the Rust side calls them. Only these
/// paths on this one host; nothing here signs or submits anything.
const TRADE_HOST: &str = "https://rare.market/v1/trade";
const TRADE_PATHS: [&str; 10] = [
    "/ext/",
    "/search?",
    "/perps?l=1",
    "/token/",
    "/tokens?",
    "/auth/",
    "/swap/",
    // Top traders and trader pages (the trader ledger): boards, profile, stats, P/L, trades, links.
    "/leaderboard?",
    "/leaderboard/extras?",
    "/users/",
];
/// Read-only prefixes: only GET goes through (no follows, no profile edits from the launcher).
const TRADE_GET_ONLY: [&str; 3] = ["/leaderboard?", "/leaderboard/extras?", "/users/"];

/// GET or POST (JSON) to the Trade API. Answers `{ status, body }` so API errors reach the UI.
#[tauri::command]
async fn trade_api(method: String, path: String, body: Option<serde_json::Value>, auth: Option<String>) -> Result<serde_json::Value, String> {
    if !TRADE_PATHS.iter().any(|p| path.starts_with(p)) || path.contains("..") || path.contains('#') {
        return Err("path not allowed".into());
    }
    if method != "GET" && method != "POST" {
        return Err("method not allowed".into());
    }
    if method != "GET" && TRADE_GET_ONLY.iter().any(|p| path.starts_with(p)) {
        return Err("method not allowed".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let url = format!("{TRADE_HOST}{path}");
        let mut req = if method == "POST" { ureq::post(&url) } else { ureq::get(&url) };
        req = req.set("User-Agent", "RareLauncher/0.1").timeout(std::time::Duration::from_secs(25));
        if let Some(a) = auth.as_deref() {
            req = req.set("Authorization", &format!("Bearer {a}"));
        }
        let res = match (method.as_str(), body) {
            ("POST", Some(b)) => req.send_json(b),
            ("POST", None) => req.send_json(serde_json::json!({})),
            _ => req.call(),
        };
        let resp = match res {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(e) => return Err(e.to_string()),
        };
        let status = resp.status();
        let body: serde_json::Value = if status == 204 { serde_json::Value::Null } else { resp.into_json().unwrap_or(serde_json::Value::Null) };
        Ok(serde_json::json!({ "status": status, "body": body }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Opens a rare.market page (Marketplace, Games, Launchpad, Bets) in a fullscreen in-app
/// browser window driven by the controller.
#[tauri::command]
fn open_web(app: AppHandle, url: String) -> Result<(), String> {
    let allowed = ["https://rare.market", "https://www.steamgriddb.com", "https://accounts.google.com/", "https://appleid.apple.com/"]
        .iter()
        .any(|p| url.starts_with(p));
    if !allowed {
        return Err("Only Rare pages open in the launcher".into());
    }
    if let Some(w) = app.get_webview_window("web") {
        let _ = w.close();
    }
    let parsed = url.parse().map_err(|e: url::ParseError| e.to_string())?;
    let mut b = WebviewWindowBuilder::new(&app, "web", WebviewUrl::External(parsed))
        .title("Rare Marketplace")
        .initialization_script(WEB_NAV_JS);
    if let Some(main) = app.get_webview_window("main") {
        if let (Ok(size), Ok(scale)) = (main.inner_size(), main.scale_factor()) {
            b = b.inner_size(size.width as f64 / scale, size.height as f64 / scale);
        }
        if main.is_fullscreen().unwrap_or(false) {
            b = b.fullscreen(true);
        }
    }
    if render::wayland_client() {
        // No window manager draws a frame under gamescope; on Wayland GTK would draw its own.
        b = b.decorations(false);
    }
    let w = b.build().map_err(|e| e.to_string())?;
    overlay::gamescope::claim(&w);
    Ok(())
}

/// Controller handling inside the in-app browser: focus moves through links and buttons,
/// A clicks, B closes, the sticks scroll.
const WEB_NAV_JS: &str = r#"
window.__rarePad = function (b) {
  const els = () => Array.from(document.querySelectorAll('a[href],button,[role=button],input,[tabindex]:not([tabindex="-1"])'))
    .filter(e => { const r = e.getBoundingClientRect(); return r.width > 4 && r.height > 4 && r.bottom > 0 && r.top < innerHeight; });
  const cur = document.activeElement;
  if (b === 'down' || b === 'up') {
    const list = els(); const i = list.indexOf(cur);
    if (i < 0 || (b === 'down' && i === list.length - 1) || (b === 'up' && i === 0)) { scrollBy({ top: b === 'down' ? innerHeight * 0.6 : -innerHeight * 0.6, behavior: 'smooth' }); return; }
    const n = list[i + (b === 'down' ? 1 : -1)]; n.focus(); n.scrollIntoView({ block: 'center', behavior: 'smooth' }); return;
  }
  if (b === 'right' || b === 'left') { const list = els(); const i = list.indexOf(cur); const n = list[Math.max(0, Math.min(list.length - 1, i + (b === 'right' ? 1 : -1)))]; n && n.focus(); return; }
  if (b === 'a' && cur && cur !== document.body) { cur.click(); return; }
  if (b === 'lb') { history.back(); return; }
  if (b === 'rb') { history.forward(); return; }
};
const s = document.createElement('style');
s.textContent = ':focus{outline:3px solid #5C76FF !important;outline-offset:3px !important;border-radius:8px}';
document.addEventListener('DOMContentLoaded', () => document.head.appendChild(s));
"#;

/// Debug entry point: runs the library scan with the saved config and prints it.
/// Picks how WebKitGTK draws under gamescope (render.rs). Call before GTK or any thread starts.
pub fn render_prepare() {
    render::prepare();
}

pub fn print_scan() {
    let c = config::load();
    let t0 = std::time::Instant::now();
    let lib = library::scan(&c.game_folders, &c.web_games, &c.removed_builtins);
    eprintln!("library: {} in {} ms", scan_summary(&lib), t0.elapsed().as_millis());
    println!("{}", serde_json::to_string_pretty(&lib).unwrap_or_default());
}

/// Panics also go to $XDG_RUNTIME_DIR/rare-launcher-panic.log and ~/.cache/rare/panic.log: under
/// Steam a shortcut's stderr is lost, and a panic on the main thread (inside a GTK or WebKit
/// callback) aborts the launcher with nothing else to go on.
fn log_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let line = format!("{ms} [panic] thread '{thread}' (launcher {}): {info}", env!("CARGO_PKG_VERSION"));
        let files = [runtime_file("rare-launcher-panic.log"), dirs::cache_dir().map(|d| d.join("rare/panic.log"))];
        for f in files.into_iter().flatten() {
            use std::io::Write;
            if let Some(dir) = f.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(mut h) = std::fs::OpenOptions::new().create(true).append(true).open(f) {
                let _ = writeln!(h, "{line}");
            }
        }
        default(info);
    }));
}

/// When the process started, for the `[startup]` timings in the log.
static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

fn since_start() -> u128 {
    STARTED.get().map(|t| t.elapsed().as_millis()).unwrap_or(0)
}

pub fn run() {
    sysglue::install_host();
    STARTED.get_or_init(std::time::Instant::now);
    log_panics();
    let t = std::time::Instant::now();
    let cfg = config::load();
    let cfg_ms = t.elapsed().as_millis();
    let t = std::time::Instant::now();
    let cached = library::cache::load();
    eprintln!("[startup] config {cfg_ms} ms, library cache {} ms ({} games)", t.elapsed().as_millis(), cached.as_ref().map(|l| l.games.len()).unwrap_or(0));
    tauri::Builder::default()
        // One launcher at a time: starting it again (Steam's "Rare" shortcut, the desktop entry)
        // brings the running one to the front instead of stacking a second copy that also reads
        // the controller.
        // In Rare OS game mode a plain focus does not change what gamescope shows: the launcher
        // takes the screen the way Home does (its id first in the base layer), so the leftover
        // Bazzite "Rare" shortcut in Steam's library works as a way back from Steam.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // `rare-launcher rare://...` (xdg-open from a game, a QR, a notice): the running
            // launcher opens the link instead.
            let args: Vec<String> = args.into_iter().skip(1).collect();
            match devmode::parse_args(args.clone()) {
                Ok(Some(g)) => return devmode::launch(app, g),
                Ok(None) => {}
                Err(e) => eprintln!("[dev] {e}"),
            }
            let links = localapi::links_in(args);
            if !links.is_empty() {
                for l in links {
                    localapi::open_link(app, &l);
                }
                return;
            }
            eprintln!("[launcher] second start: showing the running launcher");
            apps::front::to_launcher();
            // The page goes Home, as for a Home press.
            let _ = tauri::Emitter::emit_to(app, "main", "pad", gamepad::PadEvent { button: gamepad::Pad::Guide, repeat: false });
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        // rare:// links: registration (macOS bundle, Linux desktop entry) and delivery on macOS.
        // On Linux a link arrives as an argument, at the first start or through single-instance.
        .plugin(tauri_plugin_deep_link::init())
        // Rare notices (a sale, an offer, a price alert) as desktop notifications while the
        // launcher is in the background.
        .plugin(tauri_plugin_notification::init())
        .manage(AppState { library: Mutex::new(cached), runner: launch::Runner::default(), config: Mutex::new(cfg) })
        // Data cache and image pipeline (launcher/crates), open before the page asks for anything.
        .manage(instant::open())
        .register_asynchronous_uri_scheme_protocol("rimg", instant::protocol)
        .setup(|app| {
            let t_setup = std::time::Instant::now();
            wallet::manage(app);
            let handle = app.handle().clone();
            instant::start(&handle);
            // The Local API games call (loopback HTTP; web games use rare:platform), before any
            // game can be launched so every launch gets its token.
            localapi::start(handle.clone());
            #[cfg(target_os = "linux")]
            if std::env::var_os("APPIMAGE").is_some() {
                // The AppImage has no installed desktop entry; the plugin writes one with the scheme.
                if let Err(e) = app.deep_link().register_all() {
                    eprintln!("[deeplink] register: {e}");
                }
            }
            let hl = handle.clone();
            app.deep_link().on_open_url(move |e| {
                for u in e.urls() {
                    localapi::open_link(&hl, u.as_str());
                }
            });
            for l in localapi::links_in(std::env::args().skip(1)) {
                localapi::open_link(&handle, &l);
            }
            match devmode::parse_args(std::env::args().skip(1)) {
                Ok(Some(g)) => devmode::launch(&handle, g),
                Ok(None) => {}
                Err(e) => eprintln!("[dev] {e}"),
            }
            let state = app.state::<AppState>();
            ctx::install_global(&handle);
            state.runner.watch(rare_ctx::ctx(&handle));
            // Rare OS game mode: gamescope --steam shows nothing until the main window carries an
            // app id, so this is not on the deferred path below (its own threads; no xprop here).
            if let Some(w) = app.get_webview_window("main") {
                overlay::gamescope::session_start(&w);
            }
            // "boot" setting: Steam starts quietly once the launcher has been up a while.
            let hb = handle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(25));
                if hb.state::<AppState>().config.lock().unwrap().steam_mode() == "boot" {
                    let _ = launch::steam_prewarm();
                }
            });
            // Background services start a moment after the window is up, so the first library scan
            // and the first frame have the disk and the cores to themselves. Nothing here is needed
            // before then: they poll, probe tools or apply settings. (The share probe alone took
            // 1.4 s on the device.)
            let h = handle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(2500));
                let t = std::time::Instant::now();
                // Share button: keeps the clip recorder running while a game runs.
                share::start(h.clone());
                { let hh = h.clone(); library::watch::start(rare_ctx::ctx(&h), move || hh.state::<AppState>().config.lock().unwrap().game_folders.clone()); }
                trophies::watch(h.clone());
                // A base layer a previous gamescope run still held (runs xprop, so not in setup).
                overlay::init();
                // Quick resume: games a crashed run left frozen thaw; ended games leave the Switcher.
                freeze::thaw_stale();
                overlay::switcher::start(h.clone());
                brightness::apply_saved();
                // Power button with no Steam running (the base ignores it in logind and leaves it to Steam).
                powerbtn::start(h.clone());
                // Volume keys (Rare + volume: brightness) over any app, with the level on the HUD.
                // LC and RC arrive through inputplumber.rs and go to sysbuttons.rs.
                syskeys::start();
                // Steam gets the gyro: the pad target switches to a Steam Deck while Steam has a game.
                gyro::start(h.clone());
                // The Rare, Quick, LC and RC buttons (InputPlumber D-Bus actions) and intercept.
                inputplumber::start(h.clone());
                // Tells the page when the device woke up, so it can refresh quietly.
                sleepwatch::start(h.clone());
                // Power profile (Balanced unless picked otherwise): applied now and again after a
                // resume or a charger change. While a game runs, that game's settings (power.rs).
                let hp = h.clone();
                system::keep(move || power::wanted_now(&hp.state::<AppState>().config.lock().unwrap()));
                // Wi-Fi power save off while a game or stream has the screen.
                let hw = h.clone();
                system::wifi::start(move || (apps::front::front(), hw.state::<AppState>().runner.running()));
                eprintln!("[startup] background services started at {} ms ({} ms)", since_start(), t.elapsed().as_millis());
            });
            gamepad::start(handle.clone(), |app, button, down| {
                // The Quick button, from any app (a web game or the in-app browser too): Quick
                // Settings over it (overlay::quick_chord decides between the launcher's own sheet
                // and the overlay, and closes it when pressed again). Off this thread: opening the
                // overlay captures a frame first.
                if button == gamepad::Pad::Qam {
                    if down {
                        let app = app.clone();
                        std::thread::spawn(move || overlay::quick_chord(&app));
                    }
                    return false;
                }
                // The overlay sees everything first: the Rare button (the menu and the Switcher over
                // a game), guest mode's hold-View trigger, and every press while it is up.
                match overlay::route(app, button, down) {
                    overlay::Route::Ui => return true,
                    overlay::Route::Drop => return false,
                    overlay::Route::Pass => {}
                }
                // An app in front that doesn't read the controller gets it as a mouse and keyboard.
                let running = app.state::<AppState>().runner.running();
                if apps::route_pad(&running, button, down) {
                    return false;
                }
                // Guide hands the screen (and the pad) back to the launcher. Holding it also hides a
                // web window, and the launcher offers to close what is running.
                if down && matches!(button, gamepad::Pad::Guide | gamepad::Pad::GuideHold) {
                    apps::front::to_launcher();
                }
                if button == gamepad::Pad::GuideHold {
                    webgame::pad(app, gamepad::Pad::Guide, true);
                }
                // A running web game gets presses and releases, as keys.
                if webgame::pad(app, button, down) {
                    return false;
                }
                if !down {
                    return false;
                }
                // The in-app browser gets the controller while it is open.
                if let Some(web) = app.get_webview_window("web") {
                    if button == gamepad::Pad::B {
                        let _ = web.close();
                    } else {
                        let _ = web.eval(&format!("window.__rarePad && window.__rarePad('{}')", button.name()));
                    }
                    return false;
                }
                let Some(main) = app.get_webview_window("main") else { return false };
                let focused = main.is_focused().unwrap_or(true);
                // Guide always brings the launcher back to the front.
                let guide = matches!(button, gamepad::Pad::Guide | gamepad::Pad::GuideHold);
                if guide && !focused {
                    let _ = main.unminimize();
                    let _ = main.show();
                    let _ = main.set_focus();
                }
                // While a game or app has the screen only Guide reaches us (the Quick button is
                // handled above). Once Guide brought the launcher back, it has the pad even with
                // things still running.
                let gaming = app.state::<AppState>().runner.is_any_running() && !apps::front::launcher_front();
                // B cancels a start that shows no window yet: the launcher is what is on screen.
                let cancel = button == gamepad::Pad::B && apps::front::front_run().is_some_and(|r| app.state::<AppState>().runner.starting(&r));
                (focused && !gaming) || guide || cancel
            });
            // Fullscreen in the gamescope session, or when asked to.
            if std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_some() || std::env::var("RARE_FULLSCREEN").as_deref() == Ok("1") {
                if let Some(w) = app.get_webview_window("main") {
                    match render::wayland_client().then(|| render::fit_output(&w)).flatten() {
                        // gamescope never sizes an xdg window (render.rs): take the output's size.
                        Some((width, height)) => eprintln!("[startup] Wayland client of gamescope, window {width}x{height}"),
                        None => {
                            let _ = w.set_fullscreen(true);
                            let _ = w.set_decorations(false);
                        }
                    }
                }
            }
            eprintln!("[startup] setup {} ms, at {} ms since start", t_setup.elapsed().as_millis(), since_start());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            sleepwatch::sleep_state,
            sui::saves::saves_detect,
            sui::saves::saves_pack,
            sui::saves::saves_restore,
            trophies::trophies_steam,
            trophies::trophies_steam_app,
            trophies::trophy_attest,
            emu::emu_list,
            emu::emu_check,
            emu::emu_update,
            emu::emu_apply_defaults,
            emu::actions::emu_actions,
            emu::actions::emu_do,
            library_scan,
            library_cached,
            game_launch,
            game_quit,
            game_resume,
            webgame_open_browser,
            browser_info,
            running_games,
            apps_status,
            app_open,
            app_resume,
            app_install,
            steam_big_picture,
            overlay::steam_menu,
            steam_prewarm,
            proton_status,
            proton_install,
            proton_set_game,
            proton_remove,
            launch_phase,
            steam_store,
            steam_add_library,
            wifi_scan,
            wifi_connect,
            wifi_forget,
            net_status,
            radios_get,
            radios_set,
            steam_account,
            device_info,
            perf_get,
            perf_set,
            perf_boost,
            perf_fan,
            battery_care_get,
            battery_care_set,
            display::display_state,
            brightness::brightness_info,
            brightness::brightness_dim_idle,
            brightness::brightness_idle,
            upscale::upscale_get,
            upscale::upscale_set,
            game_power_get,
            game_power_save,
            hud_get,
            hud_set,
            levels_get,
            levels_set,
            power,
            steam_downloads,
            os_update_status,
            os_update_check,
            os_restore_available,
            os_restore,
            os_boot_menu_next,
            config_get,
            gyro::input_status,
            inputplumber::input_controls,
            config_set,
            art_lookup,
            rare_get,
            instant::state_get,
            instant::state_boot,
            instant::state_put,
            instant::img_preload,
            instant::instant_stats,
            devmode::dev_game_info,
            rare_post,
            trade_api,
            open_web,
            webgame_open,
            wallet_set_session,
            pad_rumble,
            debug_env,
            render_mode,
            debug_log,
            journey::journey_probe,
            journey::journey_log,
            journey::journey_cmd,
            webgame::wallet_reply,
            webgame::wallet_focus,
            overlay::overlay_open,
            overlay::overlay_close,
            overlay::overlay_state,
            hud::hud_toast,
            overlay::switcher::switcher_state,
            overlay::guest::guest_mode,
            overlay::steampages::rare_menu_steam_page,
            overlay::switcher::switcher_switch,
            overlay::switcher::freeze_keep_pending,
            freeze::freeze_policy,
            freeze::freeze_policy_set,
            freeze::freeze_note,
            webgame::wallet_set_approved,
            localapi::localapi_ready,
            localapi::localapi_reply,
            localapi::localapi_notice,
            localapi::localapi_info,
            wallet::vault::vault_status,
            wallet::vault::vault_create,
            wallet::vault::vault_unlock,
            wallet::vault::vault_read,
            wallet::vault::vault_save,
            wallet::vault::vault_lock,
            wallet::vault::vault_verify,
            wallet::vault::vault_change_secret,
            wallet::vault::vault_reset,
            wallet::accounts::wallet_accounts_read,
            wallet::accounts::wallet_accounts_write,
            wallet::accounts::wallet_accounts_clear,
            wallet::ledger::ledger_list,
            wallet::ledger::ledger_open,
            wallet::ledger::ledger_exchange,
            wallet::ledger::ledger_close,
            wallet::bluetooth::bluetooth_state,
            wallet::bluetooth::bluetooth_power_on,
            wallet::keystone::keystone_list,
            wallet::keystone::keystone_send,
            wallet::passkey::passkey_register,
            wallet::passkey::passkey_assert,
            wallet::passkey::passkey_pin,
            wallet::passkey::passkey_prf,
            wallet::passkey::passkey_cancel,
            share::share_status,
            share::share_set_clips,
            share::share_screenshot,
            share::share_clip,
            share::share_list,
            share::share_read,
            share::share_delete,
            stream::stream_status,
            stream::stream_refresh,
            stream::stream_discover,
            stream::stream_add,
            stream::stream_pair_start,
            stream::stream_pair_poll,
            stream::stream_pair_cancel,
            stream::stream_apps,
            stream::stream_launch,
            stream::stream_resume,
            stream::stream_quit,
            stream::stream_wake,
            stream::stream_settings_set,
            stream::stream_install,
            store::store_platforms,
            store::store_challenge,
            store::store_install,
            store::store_installed,
            store::store_mark_checked,
            store::store_uninstall,
            store::store_update_check
        ])
        .build(tauri::generate_context!())
        .expect("error while running Rare Launcher")
        .run(|_app, event| {
            // The clip recorder runs in its own process group; stop it with the launcher.
            if let tauri::RunEvent::Exit = event {
                // Nothing stays frozen without a launcher to thaw it.
                freeze::thaw_all();
                share::shutdown();
                stream::pair::cancel();
            }
        });
}
