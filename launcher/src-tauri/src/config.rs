//! Launcher settings in ~/.config/rare/launcher.json. Secrets never go here: the wallet
//! vault lives in the UI's encrypted store.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// Interface language code (see src/lib/locales.ts); "en" until the first-run step or Settings changes it.
    #[serde(default = "default_language")]
    pub language: String,
    /// Extra folders scanned for games (one game per subfolder).
    pub game_folders: Vec<String>,
    /// Optional SteamGridDB API key for better cover art.
    pub steamgriddb_key: Option<String>,
    pub perf_profile: Option<crate::system::Profile>,
    /// The power limit picked on the slider (profile Custom).
    pub perf_tdp: Option<f64>,
    /// CPU boost set by hand; cleared when a profile is picked.
    pub perf_cpu_boost: Option<bool>,
    /// Performance overlay level with no game running (a game keeps its own, see power.rs).
    pub perf_hud: Option<crate::perfhud::HudLevel>,
    /// Start Steam quietly at login so Steam games start fast.
    pub steam_in_background: Option<bool>,
    /// When Steam starts quietly so Steam games launch fast: "focus" (default; when a Steam game
    /// is selected), "boot" (shortly after the launcher is idle) or "demand" (only on Play).
    pub steam_start: Option<String>,
    pub onboarded: bool,
    /// Browser games the user installed (from the Sui Games catalog or by URL).
    pub web_games: Vec<WebGame>,
    /// Pre-installed web games the user removed.
    pub removed_builtins: Vec<String>,
    /// UI sounds; None means on.
    pub ui_sounds: Option<bool>,
    /// Controller rumble on navigation; None means on.
    pub rumble: Option<bool>,
    /// The first-run step to resume at after a restart; None once finished or never started.
    pub onboarding_step: Option<String>,
    /// Share button clips (the last 30 seconds kept while a game runs); None means on.
    pub share_clips: Option<bool>,
    /// Switch the pad to a Steam Deck while Steam runs a game so Steam gets motion; None means
    /// off until the Deck pad path (gyro.rs, deckpad.rs) is proven on the device.
    pub gyro_in_steam: Option<bool>,
    /// Gyro master switch (off: the pad never becomes a Steam Deck for motion); None means on.
    pub gyro_enabled: Option<bool>,
    /// Gyro sensitivity in percent, 25 to 200; None means 100.
    pub gyro_sensitivity: Option<u32>,
    /// Rumble strength in percent, 0 to 100; None means 100.
    pub rumble_strength: Option<u32>,
    /// Stick deadzones in percent, 0 to 40; None means 10 (left) and 10 (right).
    pub deadzone_left: Option<u32>,
    pub deadzone_right: Option<u32>,
    /// Invert the vertical axis of a stick.
    pub invert_left_y: Option<bool>,
    pub invert_right_y: Option<bool>,
    /// What LC and RC do on a press: "capture", "keyboard", "perf" or "none"; None means the
    /// model's default (LC capture, RC keyboard). Read by the input driver (docs/CONTROLS.md).
    pub lc_action: Option<String>,
    pub rc_action: Option<String>,
    /// Power settings per game (run id), saved with "Apply to this game" in quick settings.
    pub game_power: BTreeMap<String, crate::power::GamePower>,
    /// Minutes idle in the launcher before sleeping on battery; None means 15, 0 means never.
    pub sleep_idle_battery: Option<u32>,
    /// The same while charging; None means never.
    pub sleep_idle_ac: Option<u32>,
    /// UI size on top of the automatic one: "large" or "larger"; None means automatic.
    pub ui_scale: Option<String>,
    /// Developer setting: "mainnet" (default) or "testnet". See network.rs.
    pub network: Option<String>,
    /// Testnet Sui RPC (gRPC-web) override; the public testnet fullnode when unset.
    pub testnet_rpc: Option<String>,
    /// Testnet rare.market API base override; rare.market itself (with ?network=testnet) when unset.
    pub testnet_api: Option<String>,
    /// Testnet gas sponsor base override; the same as the API base when unset.
    pub testnet_sponsor: Option<String>,
}

fn default_language() -> String {
    "en".into()
}

impl Config {
    pub fn steam_mode(&self) -> &str {
        match self.steam_start.as_deref() {
            Some(m @ ("focus" | "boot" | "demand")) => m,
            _ if self.steam_in_background == Some(true) => "boot",
            _ => "focus",
        }
    }
}

pub use rare_core::WebGame;

fn path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("rare/launcher.json")
}

pub fn load() -> Config {
    let mut c: Config = fs::read_to_string(path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::from_str("{}").unwrap_or_default());
    if c.language.is_empty() {
        c.language = "en".into();
    }
    c
}

pub fn save(c: &Config) -> Result<(), String> {
    let p = path();
    if let Some(d) = p.parent() {
        fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(c).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    fs::rename(tmp, p).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_english() {
        let c: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(c.language, "en");
    }

    #[test]
    fn old_config_without_new_fields_loads() {
        let c: Config = serde_json::from_str(r#"{"gameFolders":["/a"],"onboarded":true,"uiSounds":false}"#).unwrap();
        assert_eq!(c.language, "en");
        assert_eq!(c.game_folders, vec!["/a"]);
        assert_eq!(c.ui_sounds, Some(false));
        assert!(c.onboarded);
    }

    #[test]
    fn language_round_trips() {
        let mut c: Config = serde_json::from_str("{}").unwrap();
        c.language = "fr".into();
        let back: Config = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back.language, "fr");
    }
}
