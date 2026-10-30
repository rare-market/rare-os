use serde::{Deserialize, Serialize};

/// Where a game comes from. The UI groups and filters by this.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Steam,
    Epic,
    Gog,
    Itch,
    Local,
    /// Browser games installed as sandboxed web apps.
    Web,
    /// Emulators, game frontends and non-Steam shortcuts.
    Apps,
    /// Games bought and installed through the Rare Store (~/Games/Rare).
    Rare,
    /// Cloud games: Xbox Cloud Gaming's catalogue and GeForce NOW games from the player's stores.
    Cloud,
    /// Android apps and games through Waydroid (the rare-android crate lists them).
    Android,
    /// ROMs in the Emulation folders, started in their system's emulator (rare-emu).
    Emu,
}

/// Artwork for a game. Local file paths are turned into asset URLs by the UI;
/// remote URLs are used as they are. Every field is optional and the UI falls
/// back in order (local, CDN, generated tile).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Art {
    /// Portrait cover, 600x900 (2:3).
    pub cover: Vec<String>,
    /// Wide hero banner, 3840x1240 or similar.
    pub hero: Vec<String>,
    /// Transparent logo.
    pub logo: Vec<String>,
    /// Landscape capsule, 460x215 or 920x430.
    pub header: Vec<String>,
    /// Square app icon (desktop entries, shortcut icons).
    #[serde(default)]
    pub icon: Vec<String>,
}

/// How to start a game. Opaque to the UI: it sends back the game id.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LaunchSpec {
    /// steam://rungameid/<appid>, handled by the Steam client.
    Steam { appid: u32 },
    /// A non-Steam shortcut, started by Steam with steam://rungameid/<gameid>.
    SteamShortcut { gameid: u64 },
    /// `legendary launch <app_name>` (GPL tool, called as an external program).
    Legendary { app_name: String },
    /// Through Heroic Games Launcher (`heroic://launch`), which keeps the user's Wine and
    /// Proton settings. `runner` is legendary, gog or sideload. `dir` tracks the running game.
    Heroic { runner: String, app_name: String, dir: Option<String> },
    /// A browser game, opened in its own fullscreen webview with isolated storage.
    Web { url: String },
    /// A cloud game: the Apps catalog's `app` (xcloud, geforcenow) opened at the game's deep link,
    /// with that app's sign-in and controller.
    Cloud { app: String, url: String },
    /// A program on disk. `windows` = run through umu-run (Proton) instead of directly.
    Exec {
        program: String,
        args: Vec<String>,
        cwd: Option<String>,
        windows: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    /// Stable id: "<source>:<native id>".
    pub id: String,
    pub source: Source,
    pub title: String,
    pub installed: bool,
    pub install_dir: Option<String>,
    pub size_bytes: Option<u64>,
    /// Unix seconds, 0 or None when never played.
    pub last_played: Option<u64>,
    /// Total playtime in minutes, when the store records it (Steam does).
    pub playtime_minutes: Option<u64>,
    pub art: Art,
    pub launch: LaunchSpec,
    /// Human description of where this was found, e.g. "Steam library on /run/media/sd".
    pub origin: String,
    /// True when the game sits in a library the current Steam does not know about
    /// (an older OS on the same disk, an SD card from another device). The UI offers "Add to Steam".
    pub foreign: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    pub source: Source,
    /// Tool or client found (Steam installed, legendary on PATH, ...).
    pub available: bool,
    pub count: usize,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub games: Vec<Game>,
    pub sources: Vec<SourceStatus>,
    /// Steam library folders found on disk that the current Steam does not list.
    pub foreign_steam_libraries: Vec<String>,
    /// Steam games the account owns that are not installed on this device (not in `games`).
    #[serde(default)]
    pub owned: Vec<Game>,
    pub scanned_at: u64,
    /// ROM systems and recently played ROMs behind the ES-DE entry, when ES-DE is set up.
    #[serde(default)]
    pub emulation: Option<crate::esde::Emulation>,
    /// Id of the ES-DE entry in `games`, so the UI can show the systems on its page.
    #[serde(default)]
    pub emulation_app: Option<String>,
    /// What a game is, when a store or the disc says so (game id to "steam:<appid>" or
    /// "<system>:<serial>"): copies with the same key are one game (dupes.rs).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub keys: std::collections::BTreeMap<String, String>,
}
