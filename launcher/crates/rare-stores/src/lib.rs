//! Epic Games and GOG in Rare without Heroic or Desktop mode: sign-in, the owned games, installs,
//! updates, repairs and cloud saves, through legendary (Epic) and gogdl (Heroic's GOG backend).
//! Both are GPL Python programs Rare ships next to the launcher (`~/rare-native/bin`) and in the
//! image (`/usr/libexec/rare/stores`). They only ever run as separate programs; no code is linked.
//! Games run through umu-run (Proton).
//!
//! Files: legendary keeps its own (`~/.config/legendary`). GOG's sign-in and installs live where
//! Heroic keeps them (`~/.config/heroic/gog_store/{auth,installed}.json`), so rare-library's scan,
//! the uninstall plan and a Heroic installed later all read the same lists. The owned lists Rare
//! fetches go in `~/.config/rare/stores/{epic,gog}-owned.json`; rare-library's scan adds those games
//! as not installed.

pub mod adopt;
pub mod drives;
pub mod epic;
pub mod gog;
pub mod jobs;
pub mod progress;
pub mod redist;
pub mod saves;
pub mod tools;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Store {
    Epic,
    Gog,
}

impl Store {
    /// "epic:<app>" or "gog:<id>", as in the library.
    pub fn of(id: &str) -> Option<(Store, &str)> {
        if let Some(a) = id.strip_prefix("epic:") {
            Some((Store::Epic, a))
        } else {
            id.strip_prefix("gog:").map(|a| (Store::Gog, a))
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Store::Epic => "Epic Games",
            Store::Gog => "GOG",
        }
    }
    /// "Epic" or "GOG", for "your Epic games".
    pub fn short(self) -> &'static str {
        match self {
            Store::Epic => "Epic",
            Store::Gog => "GOG",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Store::Epic => "epic",
            Store::Gog => "gog",
        }
    }
    /// The folder under a drive's Games folder its games go in.
    pub fn folder(self) -> &'static str {
        match self {
            Store::Epic => "Epic",
            Store::Gog => "GOG",
        }
    }
    pub fn signed_in(self) -> bool {
        match self {
            Store::Epic => epic::account().is_some(),
            Store::Gog => gog::account().is_some(),
        }
    }
}

/// The sign-in code in an address the login window is about to open, when it is the last step.
pub fn login_code(store: Store, url: &str) -> Option<String> {
    match store {
        Store::Gog => gog::code_in(url),
        Store::Epic => {
            let q = url.strip_prefix(epic::DONE)?.trim_start_matches('?');
            q.split('&').find_map(|kv| kv.strip_prefix("code=")).map(|c| c.to_string())
        }
    }
}

/// `~/.config/rare/stores`: the owned lists, the GOG user name, the cloud save marks.
pub fn rare_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config")).join("rare/stores")
}

/// Where drive_c is for a game Rare runs through umu-run: its prefix (rare-library umu: Rare's
/// own, or Heroic's for a game that came from Heroic), or that prefix's `pfx`.
pub fn prefix(id: &str) -> PathBuf {
    rare_library::umu::wine_root(&rare_library::umu::prefix(id))
}

/// One owned game in an owned list (the same keys as Heroic's library cache, so rare-library reads
/// both the same way).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Owned {
    pub app_name: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub art_square: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub art_cover: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub art_logo: String,
    /// "windows" or "linux": the build Rare installs.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub platform: String,
    /// Has cloud saves (Epic: CloudSaveFolder; GOG: known after install).
    #[serde(default)]
    pub cloud: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct OwnedFile {
    games: Vec<Owned>,
}

pub fn owned_path(store: Store) -> PathBuf {
    rare_dir().join(format!("{}-owned.json", store.key()))
}

pub fn load_owned(store: Store) -> Vec<Owned> {
    std::fs::read(owned_path(store)).ok().and_then(|b| serde_json::from_slice::<OwnedFile>(&b).ok()).map(|f| f.games).unwrap_or_default()
}

pub(crate) fn save_owned(store: Store, games: Vec<Owned>) {
    write_json(&owned_path(store), &OwnedFile { games });
}

pub(crate) fn write_json<T: Serialize>(path: &std::path::Path, v: &T) {
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_vec_pretty(v).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

pub(crate) fn read_json(path: &std::path::Path) -> Option<serde_json::Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// The signed-in accounts (display names), read from disk.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Accounts {
    pub epic: Option<String>,
    pub gog: Option<String>,
}

pub fn accounts() -> Accounts {
    Accounts { epic: epic::account(), gog: gog::account() }
}

// ---------------------------------------------------------------- change notices for the UI

static VER: AtomicU64 = AtomicU64::new(0);
type Waker = Box<dyn Fn() + Send + Sync>;
static WAKER: OnceLock<Waker> = OnceLock::new();

/// Called after every change (jobs, accounts, owned lists), from any thread.
pub fn set_waker(w: Waker) {
    let _ = WAKER.set(w);
}

/// Moves on every change; screens put it in their stamp.
pub fn version() -> u64 {
    VER.load(Ordering::Relaxed)
}

static ACCOUNTS: AtomicU64 = AtomicU64::new(0);

/// Moves when someone signs in or out (Settings > Accounts redraws on it).
pub fn accounts_version() -> u64 {
    ACCOUNTS.load(Ordering::Relaxed)
}

pub(crate) fn accounts_changed() {
    ACCOUNTS.fetch_add(1, Ordering::Relaxed);
    changed();
}

pub(crate) fn changed() {
    VER.fetch_add(1, Ordering::Relaxed);
    if let Some(w) = WAKER.get() {
        w();
    }
}

/// "12.3 GB" for a size in bytes (decimal, like the rest of Rare).
pub fn fmt_bytes(b: u64) -> String {
    let b = b as f64;
    if b >= 1e12 {
        format!("{:.1} TB", b / 1e12)
    } else if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else {
        format!("{:.0} KB", (b / 1e3).max(1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_sizes() {
        assert_eq!(Store::of("epic:Fortnite"), Some((Store::Epic, "Fortnite")));
        assert_eq!(Store::of("gog:1207658924"), Some((Store::Gog, "1207658924")));
        assert_eq!(Store::of("steam:1"), None);
        assert_eq!(fmt_bytes(12_340_000_000), "12.3 GB");
        assert_eq!(fmt_bytes(512_000_000), "512 MB");
        assert_eq!(login_code(Store::Epic, "https://rare.invalid/store-login?store=epic&code=abc"), Some("abc".into()));
        assert_eq!(login_code(Store::Epic, "https://rare.invalid/store-login?store=epic&code="), Some("".into()));
        assert_eq!(login_code(Store::Epic, "https://www.epicgames.com/id/login"), None);
    }
}
