//! lib/types.ts Config, as the settings screens read it (the saved launcher config). Defaults are
//! what an unset field means in the web launcher.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    pub language: Option<String>,
    pub game_folders: Vec<String>,
    pub steamgriddb_key: Option<String>,
    pub perf_profile: Option<String>,
    pub perf_tdp: Option<f32>,
    pub perf_cpu_boost: Option<bool>,
    pub steam_in_background: Option<bool>,
    pub steam_start: Option<String>,
    pub ui_sounds: Option<bool>,
    pub rumble: Option<bool>,
    pub gyro_in_steam: Option<bool>,
    pub gyro_enabled: Option<bool>,
    pub gyro_sensitivity: Option<f32>,
    pub rumble_strength: Option<f32>,
    pub deadzone_left: Option<f32>,
    pub deadzone_right: Option<f32>,
    pub invert_left_y: Option<bool>,
    pub invert_right_y: Option<bool>,
    pub lc_action: Option<String>,
    pub rc_action: Option<String>,
    /// LC and RC while a game has the screen (none until picked).
    pub lc_game_action: Option<String>,
    pub rc_game_action: Option<String>,
    pub sleep_idle_battery: Option<f32>,
    pub sleep_idle_ac: Option<f32>,
    pub ui_scale: Option<String>,
    pub network: Option<String>,
    pub testnet_rpc: Option<String>,
    pub testnet_api: Option<String>,
    pub testnet_sponsor: Option<String>,
    /// "Allow network apps": the Local API also answers on the local network (off unless true).
    pub allow_network_apps: Option<bool>,
    /// Instant replay (off unless true), its length in seconds and the space clips may take.
    pub share_clips: Option<bool>,
    pub replay_seconds: Option<f32>,
    pub clips_cap_gb: Option<f32>,
    /// "Show what you play on Discord" (on unless false).
    pub discord_presence: Option<bool>,
    /// Streaming this device with Sunshine (off unless true).
    pub stream_host: Option<bool>,
    /// Ask Rare (features/agent): the provider id, model, address and switches. The key is not
    /// here: it is sealed in Rare Wallet's vault.
    pub agent_provider: Option<String>,
    pub agent_model: Option<String>,
    pub agent_url: Option<String>,
    pub agent_voice: Option<bool>,
    pub agent_hold: Option<bool>,
    pub agent_shots: Option<bool>,
}

/// The config of a fresh device, the one the web launcher's mock shows.
pub fn fixture() -> Config {
    let mut c = Config { game_folders: vec!["/home/rare/Games".into()], ..Default::default() };
    if std::env::var("RR_ASK_MOCK").is_ok() {
        // Ask Rare set up (shots).
        c.agent_provider = Some("claude".into());
        c.agent_voice = Some(true);
        c.agent_hold = Some(true);
    }
    c
}

// ---------------------------------------------------------------------------------- the file

use serde_json::Value;
use std::path::Path;

fn str_of(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(String::from)
}
fn num_of(v: &Value, k: &str) -> Option<f32> {
    v.get(k).and_then(|x| x.as_f64()).map(|x| x as f32)
}
fn bool_of(v: &Value, k: &str) -> Option<bool> {
    v.get(k).and_then(|x| x.as_bool())
}

impl Config {
    /// The fields Settings edits, from the launcher's JSON (src-tauri config.rs, camelCase).
    pub fn from_json(v: &Value) -> Config {
        Config {
            language: str_of(v, "language"),
            game_folders: v.get("gameFolders").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
            steamgriddb_key: str_of(v, "steamgriddbKey"),
            perf_profile: str_of(v, "perfProfile"),
            perf_tdp: num_of(v, "perfTdp"),
            perf_cpu_boost: bool_of(v, "perfCpuBoost"),
            steam_in_background: bool_of(v, "steamInBackground"),
            steam_start: str_of(v, "steamStart"),
            ui_sounds: bool_of(v, "uiSounds"),
            rumble: bool_of(v, "rumble"),
            gyro_in_steam: bool_of(v, "gyroInSteam"),
            gyro_enabled: bool_of(v, "gyroEnabled"),
            gyro_sensitivity: num_of(v, "gyroSensitivity"),
            rumble_strength: num_of(v, "rumbleStrength"),
            deadzone_left: num_of(v, "deadzoneLeft"),
            deadzone_right: num_of(v, "deadzoneRight"),
            invert_left_y: bool_of(v, "invertLeftY"),
            invert_right_y: bool_of(v, "invertRightY"),
            lc_action: str_of(v, "lcAction"),
            rc_action: str_of(v, "rcAction"),
            lc_game_action: str_of(v, "lcGameAction"),
            rc_game_action: str_of(v, "rcGameAction"),
            sleep_idle_battery: num_of(v, "sleepIdleBattery"),
            sleep_idle_ac: num_of(v, "sleepIdleAc"),
            ui_scale: str_of(v, "uiScale"),
            network: str_of(v, "network"),
            testnet_rpc: str_of(v, "testnetRpc"),
            testnet_api: str_of(v, "testnetApi"),
            testnet_sponsor: str_of(v, "testnetSponsor"),
            allow_network_apps: bool_of(v, "allowNetworkApps"),
            share_clips: bool_of(v, "shareClips"),
            replay_seconds: num_of(v, "replaySeconds"),
            clips_cap_gb: num_of(v, "clipsCapGb"),
            discord_presence: bool_of(v, "discordPresence"),
            stream_host: bool_of(v, "streamHost"),
            agent_provider: str_of(v, "agentProvider"),
            agent_model: str_of(v, "agentModel"),
            agent_url: str_of(v, "agentUrl"),
            agent_voice: bool_of(v, "agentVoice"),
            agent_hold: bool_of(v, "agentHold"),
            agent_shots: bool_of(v, "agentShots"),
        }
    }

    /// (key, value) for every field; Null for an unset one. Whole numbers stay integers because the
    /// launcher reads most of these as u32.
    fn pairs(&self) -> Vec<(&'static str, Value)> {
        let s = |o: &Option<String>| o.clone().map(Value::from).unwrap_or(Value::Null);
        let b = |o: &Option<bool>| o.map(Value::from).unwrap_or(Value::Null);
        let n = |o: &Option<f32>| match o {
            Some(x) if x.fract() == 0.0 && *x >= 0.0 => Value::from(*x as u64),
            Some(x) => Value::from(*x as f64),
            None => Value::Null,
        };
        vec![
            ("language", s(&self.language)),
            ("gameFolders", Value::from(self.game_folders.clone())),
            ("steamgriddbKey", s(&self.steamgriddb_key)),
            ("perfProfile", s(&self.perf_profile)),
            ("perfTdp", n(&self.perf_tdp)),
            ("perfCpuBoost", b(&self.perf_cpu_boost)),
            ("steamInBackground", b(&self.steam_in_background)),
            ("steamStart", s(&self.steam_start)),
            ("uiSounds", b(&self.ui_sounds)),
            ("rumble", b(&self.rumble)),
            ("gyroInSteam", b(&self.gyro_in_steam)),
            ("gyroEnabled", b(&self.gyro_enabled)),
            ("gyroSensitivity", n(&self.gyro_sensitivity)),
            ("rumbleStrength", n(&self.rumble_strength)),
            ("deadzoneLeft", n(&self.deadzone_left)),
            ("deadzoneRight", n(&self.deadzone_right)),
            ("invertLeftY", b(&self.invert_left_y)),
            ("invertRightY", b(&self.invert_right_y)),
            ("lcAction", s(&self.lc_action)),
            ("rcAction", s(&self.rc_action)),
            ("lcGameAction", s(&self.lc_game_action)),
            ("rcGameAction", s(&self.rc_game_action)),
            ("sleepIdleBattery", n(&self.sleep_idle_battery)),
            ("sleepIdleAc", n(&self.sleep_idle_ac)),
            ("uiScale", s(&self.ui_scale)),
            ("network", s(&self.network)),
            ("testnetRpc", s(&self.testnet_rpc)),
            ("testnetApi", s(&self.testnet_api)),
            ("testnetSponsor", s(&self.testnet_sponsor)),
            ("allowNetworkApps", b(&self.allow_network_apps)),
            ("shareClips", b(&self.share_clips)),
            ("replaySeconds", n(&self.replay_seconds)),
            ("clipsCapGb", n(&self.clips_cap_gb)),
            ("discordPresence", b(&self.discord_presence)),
            ("streamHost", b(&self.stream_host)),
            ("agentProvider", s(&self.agent_provider)),
            ("agentModel", s(&self.agent_model)),
            ("agentUrl", s(&self.agent_url)),
            ("agentVoice", b(&self.agent_voice)),
            ("agentHold", b(&self.agent_hold)),
            ("agentShots", b(&self.agent_shots)),
        ]
    }
}

/// Reads the launcher config (~/.config/rare/launcher.json); defaults when it is missing.
pub fn load() -> Config {
    load_from(&rare_core::paths::config_file())
}

pub fn load_from(path: &Path) -> Config {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).map(|v| Config::from_json(&v)).unwrap_or_default()
}

/// Writes only what changed from `old` to `new` into the file, so fields Settings does not edit
/// (power, web games, onboarding...) and anything another process wrote meanwhile are kept.
pub fn save_diff(old: &Config, new: &Config) -> Result<(), String> {
    save_diff_to(&rare_core::paths::config_file(), old, new)
}

pub fn save_diff_to(path: &Path, old: &Config, new: &Config) -> Result<(), String> {
    let mut root: Value = std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).filter(|v: &Value| v.is_object()).unwrap_or_else(|| serde_json::json!({}));
    let obj = root.as_object_mut().ok_or("bad config")?;
    let mut changed = false;
    for ((k, a), (_, b)) in old.pairs().into_iter().zip(new.pairs()) {
        if a != b {
            changed = true;
            if b.is_null() {
                obj.remove(k);
            } else {
                obj.insert(k.into(), b);
            }
        }
    }
    if !changed {
        return Ok(());
    }
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&root).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_only_what_changed_and_keeps_the_rest() {
        let dir = std::env::temp_dir().join(format!("rare-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("launcher.json");
        std::fs::write(&p, r#"{"onboarded":true,"perfProfile":"quiet","uiSounds":true,"webGames":[{"id":"x"}]}"#).unwrap();
        let old = load_from(&p);
        assert_eq!(old.perf_profile.as_deref(), Some("quiet"));
        // Another writer changes the profile after we loaded.
        std::fs::write(&p, r#"{"onboarded":true,"perfProfile":"balanced","uiSounds":true,"webGames":[{"id":"x"}]}"#).unwrap();
        let mut new = old.clone();
        new.ui_sounds = Some(false);
        new.sleep_idle_battery = Some(30.0);
        new.ui_scale = Some("large".into());
        save_diff_to(&p, &old, &new).unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(v["perfProfile"], "balanced");
        assert_eq!(v["uiSounds"], false);
        assert_eq!(v["sleepIdleBattery"], 30);
        assert_eq!(v["uiScale"], "large");
        assert_eq!(v["onboarded"], true);
        assert!(v["webGames"].is_array());
        // Clearing a field removes the key.
        let mut cleared = new.clone();
        cleared.ui_scale = None;
        save_diff_to(&p, &new, &cleared).unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert!(v.get("uiScale").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
