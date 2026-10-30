//! Tauri side of the instant layer: the commands and the `rimg` protocol. The caches, their
//! policy and the reads live in the rare-system crate (launcher/crates/rare-system/src/instant.rs).

use rare_images::Source;
use rare_system::instant as core;
use serde_json::value::RawValue;
use tauri::{AppHandle, Emitter, Manager};

pub use core::{market_path_ok, open, read, Instant, InstantStats};

/// The cache key for a rare.market read under the current network.
pub fn market_key(cfg: &crate::config::Config, path: &str) -> String {
    core::market_key(cfg, path)
}

/// Hooks the caches to the app: change events, the game gate, start-up warming.
pub fn start(app: &AppHandle) {
    let inst = app.state::<Instant>();
    let gaming = {
        let h = app.clone();
        move || h.state::<crate::AppState>().runner.is_any_running()
    };
    let h = app.clone();
    core::start(
        &inst,
        move |key: &str| {
            let _ = h.emit_to("main", "rare-state", key);
        },
        gaming,
        crate::apps::front::launcher_front,
        || crate::since_start().to_string(),
    );
}

// ---------- commands ----------

/// A cached read by key: `rare:<path>` or `trade:<read-only Trade path>` (fetched by policy) or
/// `ui:<name>` (what the page put).
#[tauri::command]
pub async fn state_get(app: AppHandle, key: String) -> Result<Box<RawValue>, String> {
    let inst = app.state::<Instant>().inner().clone();
    let cfg = app.state::<crate::AppState>().config.lock().unwrap().clone();
    // A memory hit answers on this thread; a miss waits for the network on a blocking one.
    if core::state_get_is_instant(&inst, &cfg, &key) {
        return core::state_get(&inst, &cfg, key);
    }
    tauri::async_runtime::spawn_blocking(move || core::state_get(&inst, &cfg, key)).await.map_err(|e| e.to_string())?
}

/// What the page used most recently, for it to render its first frame from.
#[tauri::command]
pub async fn state_boot(app: AppHandle) -> Result<Box<RawValue>, String> {
    core::state_boot(&app.state::<Instant>())
}

/// Keeps page state (`ui:` keys) for the next start.
#[tauri::command]
pub async fn state_put(app: AppHandle, key: String, value: Box<RawValue>) -> Result<(), String> {
    core::state_put(&app.state::<Instant>(), key, value)
}

/// Makes images ready in memory ahead of a screen or a focus move.
#[tauri::command]
pub async fn img_preload(app: AppHandle, urls: Vec<String>) {
    core::img_preload(&app.state::<Instant>(), urls)
}

#[tauri::command]
pub fn instant_stats(app: AppHandle) -> InstantStats {
    core::instant_stats(&app.state::<Instant>())
}

// ---------- rimg:// ----------

fn respond(img: Result<rare_images::Image, rare_images::Error>, file: bool) -> tauri::http::Response<Vec<u8>> {
    let b = tauri::http::Response::builder().header("Access-Control-Allow-Origin", "*");
    match img {
        Ok(img) => b
            .status(200)
            .header("Content-Type", img.mime)
            // A URL names one picture forever; a file can be replaced (Steam updates a cover).
            .header("Cache-Control", if file { "max-age=600" } else { "max-age=31536000, immutable" })
            .body(img.bytes.to_vec())
            .unwrap(),
        Err(e) => b.status(e.status()).header("Cache-Control", "no-store").body(Vec::new()).unwrap(),
    }
}

/// The `rimg` protocol: a memory hit answers at once, anything else on a thread.
pub fn protocol<R: tauri::Runtime>(ctx: tauri::UriSchemeContext<'_, R>, req: tauri::http::Request<Vec<u8>>, responder: tauri::UriSchemeResponder) {
    let images = ctx.app_handle().state::<Instant>().images.clone();
    let Some((src, size)) = rare_images::parse_uri_path(req.uri().path()) else {
        responder.respond(respond(Err(rare_images::Error::NotFound), false));
        return;
    };
    let file = matches!(src, Source::File(_));
    if !file && images.in_memory(&src, size) {
        responder.respond(respond(images.get(&src, size), false));
        return;
    }
    std::thread::spawn(move || responder.respond(respond(images.get(&src, size), file)));
}
