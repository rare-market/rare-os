//! Rare Store: thin Tauri wrappers over the `rare-store` crate (installs, licence gate), plus the
//! library mapping. The logic and its tests live in `launcher/crates/rare-store`.

use crate::model::{Art, Game, LaunchSpec, Source};
use std::path::Path;
use tauri::AppHandle;

pub use rare_store::*;

fn game_from(i: &Installed) -> Game {
    let r = &i.record;
    let run = run_dir_of(i);
    let program = Path::new(&r.exec).components().fold(run.clone(), |p, c| p.join(c));
    let one = |v: &Option<String>| v.iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>();
    Game {
        id: i.id.clone(),
        source: Source::Rare,
        title: r.name.clone(),
        installed: true,
        install_dir: Some(i.dir.clone()),
        size_bytes: Some(r.size),
        last_played: None,
        playtime_minutes: None,
        art: Art {
            cover: one(&r.images.cover),
            hero: one(&r.images.banner),
            logo: one(&r.images.logo),
            header: one(&r.images.banner),
            icon: vec![],
        },
        launch: LaunchSpec::Exec {
            program: program.to_string_lossy().into_owned(),
            args: r.args.clone(),
            cwd: Some(run.to_string_lossy().into_owned()),
            windows: r.runtime == unpack::Runtime::Proton,
        },
        origin: "Rare Store".into(),
        foreign: false,
    }
}

/// Installed Rare Store games, for the library.
pub fn library_games() -> Vec<Game> {
    rare_store::installed().iter().map(game_from).collect()
}

#[tauri::command]
pub fn store_platforms() -> Platforms {
    platforms(crate::library::steam::which("umu-run"))
}

#[tauri::command]
pub async fn store_challenge(address: String, slug: String, platform: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || challenge(&address, &slug, &platform)).await.map_err(|e| e.to_string())?
}

/// Downloads, verifies and installs a build. Progress arrives as `store-progress` events.
#[tauri::command]
pub async fn store_install(app: AppHandle, req: InstallRequest) -> Result<Installed, String> {
    tauri::async_runtime::spawn_blocking(move || {
        install(&crate::rare_ctx::ctx(&app), req).map_err(String::from)
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r)
}

#[tauri::command]
pub fn store_installed() -> Vec<Installed> {
    installed()
}

#[tauri::command]
pub fn store_mark_checked(slug: String, ok: bool) -> Result<(), String> {
    mark_checked(&slug, ok)
}

#[tauri::command]
pub fn store_uninstall(slug: String) -> Result<(), String> {
    uninstall(&slug)
}

#[tauri::command]
pub fn store_update_check(slug: String, chain_version: Option<u64>) -> Result<UpdateDecision, String> {
    update_check(&slug, chain_version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_an_install_to_a_library_game() {
        let record: InstallRecord = serde_json::from_value(serde_json::json!({
            "slug": "g", "name": "G", "version": 1, "edition": 0, "platform": "linux-x86_64", "sha256": "ab", "size": 5,
            "runtime": "native", "exec": "bin/game", "args": ["-x"], "installedMs": 1, "checkedMs": 1, "versionDir": "1",
            "images": { "cover": "https://x/c.png" }
        }))
        .unwrap();
        let i = Installed { record, id: "rare:g".into(), dir: "/nonexistent/g".into() };
        let g = game_from(&i);
        assert_eq!(g.source, Source::Rare);
        assert_eq!(g.art.cover, vec!["https://x/c.png"]);
        assert!(matches!(g.launch, LaunchSpec::Exec { windows: false, .. }));
    }
}
