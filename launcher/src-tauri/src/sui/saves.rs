//! Cloud saves, Tauri side: the commands the web UI calls. Detection, packing and restore live in
//! `rare_sui::saves::local` (shared with the native UI).
//!
//! `saves_restore` takes the raw archive as the IPC body. The save set `id` and `rel` travel in the
//! headers `x-rare-id` and `x-rare-rel`, each encoded as base64url (no padding) of the UTF-8 string.

pub use rare_sui::saves::local::SaveSet;
use rare_sui::saves::local::{detect_in, pack_dir, restore_into};
use std::path::{Path, PathBuf};

fn home() -> Result<PathBuf, String> {
    rare_sui::saves::local::home()
}

#[tauri::command]
pub async fn saves_detect() -> Result<Vec<SaveSet>, String> {
    let home = home()?;
    tauri::async_runtime::spawn_blocking(move || detect_in(&home)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn saves_pack(id: String) -> Result<tauri::ipc::Response, String> {
    let home = home()?;
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        let set = detect_in(&home).into_iter().find(|s| s.id == id).ok_or("save set not found")?;
        pack_dir(Path::new(&set.path))
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(tauri::ipc::Response::new(bytes))
}

fn header_str(req: &tauri::ipc::Request<'_>, name: &str) -> Result<String, String> {
    let v = req.headers().get(name).ok_or_else(|| format!("missing header {name}"))?;
    let raw = v.to_str().map_err(|_| format!("bad header {name}"))?;
    rare_sui::saves::local::from_b64url(raw).ok_or_else(|| format!("bad header {name}"))
}

#[tauri::command]
pub async fn saves_restore(request: tauri::ipc::Request<'_>) -> Result<String, String> {
    let id = header_str(&request, "x-rare-id")?;
    let rel = header_str(&request, "x-rare-rel")?;
    let data: Vec<u8> = match request.body() {
        tauri::ipc::InvokeBody::Raw(b) => b.clone(),
        _ => return Err("expected a raw body".into()),
    };
    let home = home()?;
    tauri::async_runtime::spawn_blocking(move || restore_into(&home, &id, &rel, &data)).await.map_err(|e| e.to_string())?
}
