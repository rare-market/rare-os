//! The unified library: every store and folder merged into one list.

pub mod appinfo;
pub mod apps;
pub mod art;
pub mod browser;
pub mod custom;
pub mod dupes;
pub mod cache;
pub mod esde;
#[cfg(test)]
mod fixture_tests;
pub mod heroic;
pub mod local;
pub mod model;
pub mod movegame;
pub mod profiles;
pub mod sdcard;
pub mod shortcuts;
pub mod state;
pub mod steam;
pub mod umu;
pub mod storage;
pub mod steam_cloud;
pub mod uninstall;
pub mod steam_dlc;
pub mod vdf;
pub mod vdfedit;
pub mod watch;

use crate::model::{Library, Source, SourceStatus};
use std::time::{SystemTime, UNIX_EPOCH};

pub mod roots {
    use std::fs;
    use std::path::PathBuf;

    fn subdirs(p: &str) -> Vec<PathBuf> {
        fs::read_dir(p)
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
            .unwrap_or_default()
    }

    /// Home directories of other users and of an earlier OS install on the same disk
    /// (bootc and ostree systems keep homes in /var/home, which survives an OS switch).
    pub fn other_homes() -> Vec<PathBuf> {
        let me = dirs::home_dir().and_then(|h| fs::canonicalize(h).ok());
        let mut out = Vec::new();
        for base in ["/home", "/var/home"] {
            for d in subdirs(base) {
                let c = fs::canonicalize(&d).unwrap_or(d);
                if Some(&c) != me.as_ref() && !out.contains(&c) && fs::read_dir(&c).is_ok() {
                    out.push(c);
                }
            }
        }
        out
    }

    /// Mounted drives: SD cards, USB drives, extra partitions.
    pub fn mount_points() -> Vec<PathBuf> {
        let mut out = Vec::new();
        for base in ["/run/media", "/media"] {
            for d in subdirs(base) {
                // /run/media/<user>/<label> and plain /media/<label>
                let inner = subdirs(&d.to_string_lossy());
                if inner.is_empty() {
                    out.push(d);
                } else {
                    out.extend(inner);
                }
            }
        }
        for base in ["/mnt", "/var/mnt"] {
            out.extend(subdirs(base));
        }
        out
    }
}

/// Web games that ship with Rare OS (profiles marked `builtin`): ACT, the reference game for
/// the web-game runtime. Its art ships in the UI's public/games/<slug>/.
pub fn builtin_web_games() -> Vec<rare_core::WebGame> {
    profiles::builtins()
        .into_iter()
        .map(|(id, title, url)| rare_core::WebGame {
            cover: Some(format!("games/{id}/cover.jpg")),
            hero: Some(format!("games/{id}/hero.jpg")),
            id,
            title,
            url,
        })
        .collect()
}

fn web_games(user: &[rare_core::WebGame], removed: &[String]) -> Vec<crate::model::Game> {
    let mut list: Vec<rare_core::WebGame> = builtin_web_games().into_iter().filter(|b| !removed.contains(&b.id) && !user.iter().any(|u| u.id == b.id)).collect();
    list.extend(user.iter().cloned());
    let builtin_ids: Vec<String> = builtin_web_games().into_iter().map(|b| b.id).collect();
    list.iter()
        .map(|w| crate::model::Game {
            id: format!("web:{}", w.id),
            source: Source::Web,
            title: w.title.clone(),
            installed: true,
            install_dir: None,
            size_bytes: None,
            last_played: None,
            playtime_minutes: None,
            art: crate::model::Art {
                cover: w.cover.iter().cloned().collect(),
                hero: w.hero.iter().cloned().collect(),
                logo: if builtin_ids.contains(&w.id) { vec![format!("games/{}/logo.svg", w.id)] } else { vec![] },
                ..Default::default()
            },
            launch: crate::model::LaunchSpec::Web { url: w.url.clone() },
            origin: "Web game".into(),
            foreign: false,
        })
        .collect()
}

/// `rare` is the Rare Store's installed games (the store crate owns reading them).
pub fn scan(extra_folders: &[String], web_user: &[rare_core::WebGame], removed: &[String], rare: Vec<crate::model::Game>) -> Library {
    let web = web_games(web_user, removed);
    let steam = steam::scan();
    let epic = heroic::scan_epic();
    let gog = heroic::scan_gog();
    let itch = local::scan_itch();
    let mut known_dirs: Vec<String> = Vec::new();
    for g in steam.games.iter().chain(epic.games.iter()).chain(gog.games.iter()).chain(itch.iter()) {
        if let Some(d) = &g.install_dir {
            known_dirs.push(d.clone());
        }
    }
    let mut local = local::scan_local(extra_folders, &known_dirs);
    // Shortcuts to emulators and apps join the emulators; shortcuts to games are games.
    let (app_shortcuts, game_shortcuts): (Vec<_>, Vec<_>) = shortcuts::scan(&steam.roots).into_iter().partition(|g| g.source == Source::Apps);
    let steam_titles: std::collections::HashSet<String> = steam.games.iter().map(|g| shortcuts::norm(&g.title)).collect();
    // A shortcut to a game Steam already lists would show it twice.
    local.extend(game_shortcuts.into_iter().filter(|g| !steam_titles.contains(&shortcuts::norm(&g.title))));
    let apps = apps::merge(apps::scan(), app_shortcuts);
    let emulation = esde::scan();
    let emulation_app = emulation.as_ref().and_then(|_| apps.iter().find(|a| shortcuts::norm(&a.title) == "esde").map(|a| a.id.clone()));

    let sources = vec![
        SourceStatus {
            source: Source::Steam,
            available: steam.installed,
            count: steam.games.len(),
            note: if steam.installed { "Steam".into() } else { "Steam is not set up yet".into() },
        },
        SourceStatus { source: Source::Epic, available: epic.available, count: epic.games.len(), note: epic.note },
        SourceStatus { source: Source::Gog, available: gog.available, count: gog.games.len(), note: gog.note },
        SourceStatus { source: Source::Itch, available: true, count: itch.len(), note: "itch.io downloads".into() },
        SourceStatus { source: Source::Local, available: true, count: local.len(), note: "Games folders".into() },
        SourceStatus { source: Source::Web, available: true, count: web.len(), note: "Browser games".into() },
        SourceStatus { source: Source::Rare, available: true, count: rare.len(), note: "Rare Store".into() },
        SourceStatus { source: Source::Apps, available: true, count: apps.len(), note: "Emulators".into() },
    ];

    // Owned Steam games that are not on this device (Library > Not installed, search, Install).
    let on_disk: std::collections::HashSet<u32> = steam.games.iter().filter_map(|g| g.id.strip_prefix("steam:").and_then(|a| a.parse().ok())).collect();
    let owned = steam::owned(&steam.roots, &on_disk);
    let mut games = steam.games;
    games.extend(epic.games);
    games.extend(gog.games);
    games.extend(itch);
    games.extend(local);
    games.extend(web);
    games.extend(rare);
    let by_recent = |a: &crate::model::Game, b: &crate::model::Game| {
        b.last_played.unwrap_or(0).cmp(&a.last_played.unwrap_or(0)).then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    };
    games.sort_by(by_recent);
    // Emulators and apps come after the games.
    let mut apps = apps;
    // ES-DE's last played comes from its gamelists.
    if let (Some(id), Some(e)) = (&emulation_app, &emulation) {
        if let Some(a) = apps.iter_mut().find(|a| &a.id == id) {
            a.last_played = a.last_played.max(e.recent.first().and_then(|r| r.last_played));
        }
    }
    apps.sort_by(by_recent);
    games.extend(apps);
    // Never the same entry twice.
    let mut ids = std::collections::HashSet::new();
    games.retain(|g| ids.insert(g.id.clone()));

    Library {
        games,
        sources,
        foreign_steam_libraries: steam.foreign_libraries,
        owned,
        emulation,
        emulation_app,
        keys: Default::default(),
        scanned_at: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
    }
}
