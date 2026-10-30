//! Retro trophies: achievements for classic games with no RetroAchievements account. The
//! emulators' achievement code (rcheevos) talks to rare.market's RetroAchievements-compatible
//! endpoint (`https://rare.market/v1/retro`), which serves the achievement sets and records unlocks
//! as the Rare account's trophies. Rare gives each device a token for it (`POST /v1/retro/token`,
//! signed by the device's friends key; the launcher does that call) and keeps it in
//! ~/.config/rare/retro.json, readable by the player only. At each launch the token goes into the
//! emulator's settings: RetroArch's rare.cfg (`cheevos_custom_host`), DuckStation, PCSX2, PPSSPP
//! and Dolphin (each has a custom host setting for rcheevos).
//!
//! This module also reads the account's retro trophies back for the trophy case, the ROM page and
//! the unlock pop-ups (`/v1/retro/me*`), and hashes ROMs the way rcheevos does.

use crate::roms::Rom;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// rare.market's endpoint; RARE_RETRO_HOST points tests elsewhere.
pub fn host() -> String {
    std::env::var("RARE_RETRO_HOST").ok().filter(|h| !h.is_empty()).unwrap_or_else(|| "https://rare.market/v1/retro".into()).trim_end_matches('/').to_string()
}

/// This device's retro sign-in: the name emulators show, the token, the account it is for.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Sign {
    pub user: String,
    pub token: String,
    /// The Rare account (address) the token acts for.
    #[serde(default)]
    pub address: String,
    #[serde(default = "yes")]
    pub hardcore: bool,
}

fn yes() -> bool {
    true
}

fn path() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("rare/retro.json")
}

pub fn sign() -> Option<Sign> {
    std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).filter(|s: &Sign| !s.token.is_empty())
}

pub fn save(s: Option<&Sign>) -> Result<(), String> {
    let p = path();
    match s {
        None => {
            let _ = std::fs::remove_file(&p);
            Ok(())
        }
        Some(s) => {
            std::fs::create_dir_all(p.parent().unwrap_or(Path::new("/"))).map_err(|e| e.to_string())?;
            let tmp = p.with_extension("tmp");
            std::fs::write(&tmp, serde_json::to_vec_pretty(s).unwrap_or_default()).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
            }
            std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
        }
    }
}

/// Hardcore is on unless the player turned it off.
pub fn set_hardcore(on: bool) -> Result<(), String> {
    let mut s = sign().ok_or("Retro trophies aren't set up on this device yet.")?;
    s.hardcore = on;
    save(Some(&s))
}

/// The answer to `POST /v1/retro/token`, as a sign-in for `address`.
pub fn from_token_answer(v: &Value, address: &str, hardcore: bool) -> Option<Sign> {
    let token = v.get("token")?.as_str()?.to_string();
    let user = v.get("user").and_then(Value::as_str).unwrap_or("Rare player").to_string();
    Some(Sign { user, token, address: address.to_string(), hardcore })
}

// ---------------------------------------------------------------- the emulators' settings

/// Hardcore for the emulators: Rare's setting, on by default.
pub fn hardcore() -> bool {
    sign().map(|s| s.hardcore).unwrap_or(true)
}

/// rcheevos settings for DuckStation, PCSX2, PPSSPP and Dolphin, written before each launch.
/// DuckStation and PCSX2 only when their settings file exists (a file Rare made up would skip their
/// first-run setup); PPSSPP's and Dolphin's are made when missing (they have no first-run setup and
/// fill in what is not there).
pub fn write_standalone(home: &Path, emu: &str, s: &Sign) {
    let host = host();
    let hc = s.hardcore.to_string();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0).to_string();
    match emu {
        "duckstation" => {
            let token = duckstation_token(&s.token, &s.user, &machine_key());
            let keys = [("Enabled", "true".to_string()), ("Host", host), ("Username", s.user.clone()), ("Token", token), ("LoginTimestamp", now), ("ChallengeMode", hc)];
            for f in [home.join(".local/share/duckstation/settings.ini"), home.join(".var/app/org.duckstation.DuckStation/data/duckstation/settings.ini")] {
                if edit_ini(&f, "Cheevos", &keys, false) {
                    quiet_ini(home, &f, "Cheevos", DUCKSTATION_QUIET);
                }
            }
        }
        "pcsx2" => {
            // PCSX2 moves a Token found here into its secrets file on start.
            let keys = [("Enabled", "true".to_string()), ("Host", host), ("Username", s.user.clone()), ("Token", s.token.clone()), ("LoginTimestamp", now), ("ChallengeMode", hc)];
            for f in [home.join(".config/PCSX2/inis/PCSX2.ini"), home.join(".var/app/net.pcsx2.PCSX2/config/PCSX2/inis/PCSX2.ini")] {
                if edit_ini(&f, "Achievements", &keys, false) {
                    quiet_ini(home, &f, "Achievements", PCSX2_QUIET);
                }
            }
        }
        "ppsspp" => {
            // The token is a secret file next to ppsspp.ini (NativeSaveSecret: ppsspp_retroachievements.dat).
            let keys = [("AchievementsEnable", "True".to_string()), ("AchievementsHost", host), ("AchievementsUserName", s.user.clone()), ("AchievementsChallengeMode", if s.hardcore { "True" } else { "False" }.to_string())];
            for sys in ppsspp_dirs(home) {
                if edit_ini(&sys.join("ppsspp.ini"), "Achievements", &keys, true) {
                    let _ = write_private(&sys.join("ppsspp_retroachievements.dat"), s.token.as_bytes());
                    quiet_ini(home, &sys.join("ppsspp.ini"), "Achievements", PPSSPP_QUIET);
                }
            }
        }
        "dolphin" => {
            let keys = [("Enabled", "True".to_string()), ("HostUrl", host), ("Username", s.user.clone()), ("ApiToken", s.token.clone()), ("HardcoreEnabled", if s.hardcore { "True" } else { "False" }.to_string())];
            for f in dolphin_files(home) {
                if edit_ini(&f, "Achievements", &keys, true) {
                    quiet_ini(home, &f, "Achievements", DOLPHIN_QUIET);
                }
            }
        }
        _ => {}
    }
}

/// The emulators' own achievement pop-ups, indicators and sounds, off: Rare shows each unlock over
/// the game itself (HUD toast with the badge), and the emulators' pop-ups stayed on screen (PCSX2's
/// challenge and progress indicators). Achievements stay on, so unlocks still reach rare.market.
/// (key, Rare's value, the emulator's default)
pub const PCSX2_QUIET: &[(&str, &str, &str)] = &[
    ("Notifications", "false", "true"),
    ("LeaderboardNotifications", "false", "true"),
    ("SoundEffects", "false", "true"),
    ("InfoSound", "false", "true"),
    ("UnlockSound", "false", "true"),
    ("LBSubmitSound", "false", "true"),
    ("Overlays", "false", "true"),
    ("LBOverlays", "false", "true"),
];
pub const DUCKSTATION_QUIET: &[(&str, &str, &str)] = &[
    ("Notifications", "false", "true"),
    ("LeaderboardNotifications", "false", "true"),
    ("LeaderboardTrackers", "false", "true"),
    ("SoundEffects", "false", "true"),
    ("ChallengeIndicatorMode", "Disabled", "Notification"),
    ("ProgressIndicatorMode", "Disabled", "IconAndTitle"),
];
/// PPSSPP: -1 is "None" for a notification's position.
pub const PPSSPP_QUIET: &[(&str, &str, &str)] = &[
    ("AchievementsSoundEffects", "False", "True"),
    ("AchievementsUnlockedPos", "-1", "4"),
    ("AchievementsChallengePos", "-1", "3"),
    ("AchievementsProgressPos", "-1", "3"),
    ("AchievementsLeaderboardTrackerPos", "-1", "3"),
    ("AchievementsLeaderboardStartedOrFailedPos", "-1", "3"),
    ("AchievementsLeaderboardSubmittedPos", "-1", "3"),
];
pub const DOLPHIN_QUIET: &[(&str, &str, &str)] = &[
    ("LeaderboardTrackerEnabled", "False", "True"),
    ("ChallengeIndicatorsEnabled", "False", "True"),
    ("ProgressEnabled", "False", "False"),
];

fn quiet_manifest(home: &Path) -> PathBuf {
    home.join(".local/share/rare/emu-quiet.json")
}

/// Sets each key to Rare's value unless the player changed it: a key that is missing, at the
/// emulator's default, or still what Rare wrote last time is Rare's to set; any other value is the
/// player's and stays (what Rare wrote is kept in ~/.local/share/rare/emu-quiet.json).
pub fn quiet_ini(home: &Path, f: &Path, section: &str, keys: &[(&str, &str, &str)]) {
    let Ok(old) = std::fs::read_to_string(f) else { return };
    let mpath = quiet_manifest(home);
    let mut m: std::collections::BTreeMap<String, String> = std::fs::read_to_string(&mpath).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let body = crate::controls::ini_section(&old, section).unwrap_or_default();
    let current = |k: &str| body.lines().find_map(|l| l.split_once('=').filter(|(a, _)| a.trim() == k).map(|(_, v)| v.trim().to_string()));
    let mut t = old.clone();
    for (k, rare, default) in keys {
        let id = format!("{}#{section}#{k}", f.display());
        let ours = match current(k) {
            None => true,
            Some(v) => v.eq_ignore_ascii_case(default) || m.get(&id).is_some_and(|w| v.eq_ignore_ascii_case(w)),
        };
        if ours {
            t = crate::actions::upsert(&t, section, k, rare, false);
            m.insert(id, rare.to_string());
        }
    }
    if t != old {
        let tmp = f.with_extension("rare-tmp");
        if std::fs::write(&tmp, &t).is_ok() {
            let _ = std::fs::rename(&tmp, f);
        }
    }
    let _ = std::fs::create_dir_all(mpath.parent().unwrap_or(Path::new("/")));
    let _ = std::fs::write(&mpath, serde_json::to_string_pretty(&m).unwrap_or_default());
}

/// PPSSPP's PSP/SYSTEM folders: the Flatpak's when it is installed, else the plain one.
fn ppsspp_dirs(home: &Path) -> Vec<PathBuf> {
    let flat = home.join(".var/app/org.ppsspp.PPSSPP");
    if flat.is_dir() {
        vec![flat.join("config/ppsspp/PSP/SYSTEM")]
    } else if home.join(".config/ppsspp").is_dir() {
        vec![home.join(".config/ppsspp/PSP/SYSTEM")]
    } else {
        vec![]
    }
}

fn dolphin_files(home: &Path) -> Vec<PathBuf> {
    let flat = home.join(".var/app/org.DolphinEmu.dolphin-emu");
    let mut v = Vec::new();
    if flat.is_dir() {
        v.push(flat.join("config/dolphin-emu/RetroAchievements.ini"));
    }
    if home.join(".config/dolphin-emu").is_dir() {
        v.push(home.join(".config/dolphin-emu/RetroAchievements.ini"));
    }
    v
}

fn write_private(f: &Path, b: &[u8]) -> std::io::Result<()> {
    if let Some(d) = f.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = f.with_extension("rare-tmp");
    std::fs::write(&tmp, b)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, f)
}

/// Sets `keys` in `section` of an INI file. Returns whether the file has them now.
fn edit_ini(f: &Path, section: &str, keys: &[(&str, String)], create: bool) -> bool {
    let old = match std::fs::read_to_string(f) {
        Ok(t) => t,
        Err(_) if create && f.parent().is_some() => String::new(),
        Err(_) => return false,
    };
    let mut t = old.clone();
    for (k, v) in keys {
        t = crate::actions::upsert(&t, section, k, v, false);
    }
    if t == old && !old.is_empty() {
        return true;
    }
    write_private(f, t.as_bytes()).is_ok()
}

/// DuckStation keeps the token AES-128-CBC encrypted with a key from the machine id and the user
/// name (achievements.cpp, EncryptLoginToken): SHA-256 of both, 100 more rounds of SHA-256, the
/// first 16 bytes the key and the last 16 the IV, the token zero-padded, then base64.
pub fn duckstation_token(token: &str, user: &str, machine: &[u8]) -> String {
    use aes::cipher::{BlockEncrypt, KeyInit};
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(machine);
    h.update(user.as_bytes());
    let mut key: [u8; 32] = h.finalize().into();
    for _ in 0..100 {
        key = Sha256::digest(key).into();
    }
    let cipher = aes::Aes128::new_from_slice(&key[..16]).expect("16 byte key");
    let mut data = token.as_bytes().to_vec();
    data.resize(token.len().div_ceil(16) * 16, 0);
    let mut prev: [u8; 16] = key[16..32].try_into().unwrap();
    for chunk in data.chunks_mut(16) {
        for (b, p) in chunk.iter_mut().zip(prev.iter()) {
            *b ^= p;
        }
        let mut block = aes::Block::clone_from_slice(chunk);
        cipher.encrypt_block(&mut block);
        chunk.copy_from_slice(&block);
        prev.copy_from_slice(chunk);
    }
    base64(&data)
}

/// DuckStation's machine key on Linux: /etc/machine-id as it is on disk.
fn machine_key() -> Vec<u8> {
    std::fs::read("/etc/machine-id").unwrap_or_default()
}

fn base64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

// ---------------------------------------------------------------- which game a ROM is

/// How rcheevos hashes a cartridge system's ROM: bytes to skip at the start, or the file name.
fn hash_rule(sys: &str, head: &[u8], size: u64) -> Option<u64> {
    Some(match sys {
        "nes" if head.starts_with(b"NES\x1a") => 16,
        "fds" if head.starts_with(b"FDS\x1a") => 16,
        "atarilynx" if head.starts_with(b"LYNX") => 64,
        "snes" if size % 0x2000 == 512 => 512,
        "pcengine" if size % 0x2000 == 512 => 512,
        "nes" | "fds" | "atarilynx" | "snes" | "pcengine" | "gb" | "gbc" | "gba" | "genesis" | "mastersystem" | "gamegear" | "sega32x" | "virtualboy" | "atari2600" | "ngp" | "ngpc" | "wonderswan" | "wonderswancolor" | "n64" => 0,
        _ => return None,
    })
}

/// The RetroAchievements hash of a ROM, for the systems hashed from the file itself.
pub fn rom_hash(rom: &Rom) -> Option<String> {
    let sys = rom.system.id;
    if matches!(sys, "arcade" | "neogeo" | "mame") {
        // Arcade games are known by their set name.
        return Some(crate::md5::hex(rom.file.file_stem()?.to_string_lossy().as_bytes()));
    }
    let ext = rom.file.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let data: Vec<u8> = if ext == "zip" {
        let z = std::fs::read(&rom.file).ok()?;
        if z.len() > 64 << 20 {
            return None;
        }
        crate::unzip::entry(&z, "").ok()?
    } else {
        let len = rom.file.metadata().ok()?.len();
        if len > 64 << 20 || ext == "7z" {
            return None;
        }
        std::fs::read(&rom.file).ok()?
    };
    let skip = hash_rule(sys, &data[..data.len().min(16)], data.len() as u64)? as usize;
    let mut body = data.get(skip..)?.to_vec();
    if sys == "n64" && body.len() >= 4 {
        // rcheevos hashes the big-endian (.z64) byte order.
        match body[..4] {
            [0x37, 0x80, 0x40, 0x12] => body.chunks_exact_mut(2).for_each(|c| c.swap(0, 1)),
            [0x40, 0x12, 0x37, 0x80] => body.chunks_exact_mut(4).for_each(|c| c.reverse()),
            _ => {}
        }
    }
    Some(crate::md5::hex(&body))
}

/// ROMs Rare saw an emulator start as a game (disc games, whose hash only the emulator makes):
/// ROM id -> game id, in ~/.cache/rare/emu/retro-games.json.
fn known_path() -> PathBuf {
    crate::roms::home().join(".cache/rare/emu/retro-games.json")
}

pub fn known_game(rom_id: &str) -> Option<u32> {
    let t = std::fs::read(known_path()).ok()?;
    serde_json::from_slice::<std::collections::HashMap<String, u32>>(&t).ok()?.get(rom_id).copied()
}

pub fn remember_game(rom_id: &str, game: u32) {
    let p = known_path();
    let mut m: std::collections::HashMap<String, u32> = std::fs::read(&p).ok().and_then(|t| serde_json::from_slice(&t).ok()).unwrap_or_default();
    if m.get(rom_id) == Some(&game) {
        return;
    }
    m.insert(rom_id.to_string(), game);
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(&p, serde_json::to_vec(&m).unwrap_or_default());
}

/// The console's short name ("NES") for a RetroAchievements console id ("" when Rare has none).
pub fn console_short(id: u32) -> &'static str {
    crate::systems::SYSTEMS.iter().find(|s| s.ra == id && id != 0).map(|s| s.short).unwrap_or("")
}

/// The console's name for a RetroAchievements console id ("" when Rare has no system for it).
pub fn console_name(id: u32) -> &'static str {
    crate::systems::SYSTEMS.iter().find(|s| s.ra == id && id != 0).map(|s| s.name).unwrap_or("")
}

// ---------------------------------------------------------------- the account's retro trophies

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout_read(Duration::from_secs(25)).user_agent(concat!("RareLauncher/", env!("CARGO_PKG_VERSION"))).build()
}

fn get(s: &Sign, path: &str) -> Result<Value, String> {
    let r = agent().get(&format!("{}{path}", host())).set("Authorization", &format!("Bearer {}", s.token)).call().map_err(|e| match e {
        ureq::Error::Status(401, _) => "unauthorized".to_string(),
        ureq::Error::Status(c, r) => r.into_json::<Value>().ok().and_then(|j| j["error"]["message"].as_str().map(str::to_string)).unwrap_or_else(|| format!("rare.market {c}")),
        e => format!("Couldn't reach rare.market ({e})."),
    })?;
    r.into_json::<Value>().map_err(|e| e.to_string())
}

/// Whether an error from these reads means the token no longer works.
pub fn is_unauthorized(e: &str) -> bool {
    e == "unauthorized"
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Played {
    pub game_id: u32,
    pub title: String,
    pub console_id: u32,
    #[serde(default)]
    pub icon_url: String,
    pub total: u32,
    pub earned: u32,
    #[serde(default)]
    pub earned_hardcore: u32,
    /// Unix ms.
    pub last_played_at: i64,
    pub last_unlock_at: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Achievement {
    pub id: u64,
    pub title: String,
    pub description: String,
    pub points: u32,
    #[serde(default)]
    pub badge: String,
    #[serde(default)]
    pub badge_url: String,
    #[serde(default)]
    pub locked_url: String,
    /// Unix ms, when the player has it.
    pub earned_at: Option<i64>,
    #[serde(default)]
    pub hardcore: bool,
    /// RetroAchievements' share of the set's players with it, softcore and hardcore (percent).
    #[serde(default)]
    pub rarity: Option<f32>,
    #[serde(default)]
    pub rarity_hardcore: Option<f32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GameProgress {
    pub game_id: u32,
    pub title: String,
    pub console_id: u32,
    #[serde(default)]
    pub icon_url: String,
    pub achievements: Vec<Achievement>,
}

impl GameProgress {
    pub fn earned(&self) -> usize {
        self.achievements.iter().filter(|a| a.earned_at.is_some()).count()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Unlock {
    pub id: u64,
    pub game_id: u32,
    pub title: String,
    pub description: String,
    pub points: u32,
    #[serde(default)]
    pub badge_url: String,
    pub game: String,
    pub console_id: u32,
    /// Unix ms.
    pub at: i64,
    pub hardcore: bool,
}

/// Every retro game the account started, newest first.
pub fn played(s: &Sign) -> Result<Vec<Played>, String> {
    let v = get(s, "/me")?;
    serde_json::from_value(v.get("games").cloned().unwrap_or(Value::Array(vec![]))).map_err(|e| e.to_string())
}

fn game_answer(v: Value) -> Result<Option<GameProgress>, String> {
    match v.get("game") {
        Some(Value::Null) | None => Ok(None),
        Some(g) => serde_json::from_value(g.clone()).map(Some).map_err(|e| e.to_string()),
    }
}

/// A game's achievements with the account's unlocks.
pub fn progress(s: &Sign, game: u32) -> Result<Option<GameProgress>, String> {
    game_answer(get(s, &format!("/me/game/{game}"))?)
}

/// A ROM's achievements: by its hash, or the game an emulator started it as (None: no set for it,
/// or a disc game not played yet).
pub fn progress_for(s: &Sign, rom: &Rom) -> Result<Option<GameProgress>, String> {
    if rom.system.ra == 0 {
        return Ok(None);
    }
    if let Some(g) = known_game(&rom.id) {
        return progress(s, g);
    }
    match rom_hash(rom) {
        Some(h) => {
            let p = game_answer(get(s, &format!("/me/hash/{h}"))?)?;
            if let Some(p) = &p {
                remember_game(&rom.id, p.game_id);
            }
            Ok(p)
        }
        None => Ok(None),
    }
}

/// A badge picture's bytes (PNG), for the pop-up over the game.
pub fn fetch_picture(url: &str) -> Option<Vec<u8>> {
    if !url.starts_with("https://") {
        return None;
    }
    let r = agent().get(url).call().ok()?;
    let mut b = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(r.into_reader(), 1 << 20), &mut b).ok()?;
    Some(b)
}

/// A RetroAchievements title as Rare shows it: "~Homebrew~ 2048" reads "2048".
pub fn display_title(t: &str) -> String {
    let mut s = t.trim();
    while let Some(rest) = s.strip_prefix('~').and_then(|r| r.split_once('~')).map(|(_, r)| r.trim_start()) {
        s = rest;
    }
    if s.is_empty() { t.trim().to_string() } else { s.to_string() }
}

/// Unlocks after `after_ms`, newest first.
pub fn recent(s: &Sign, after_ms: i64) -> Result<Vec<Unlock>, String> {
    let v = get(s, &format!("/me/recent?after={}", after_ms.max(0)))?;
    serde_json::from_value(v.get("data").cloned().unwrap_or(Value::Array(vec![]))).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_follow_rcheevos() {
        let t = tempfile::tempdir().unwrap();
        let mk = |name: &str, sys: &str, data: &[u8]| {
            let p = t.path().join(name);
            std::fs::write(&p, data).unwrap();
            Rom { id: format!("emu:{sys}/{name}"), system: crate::systems::by_id(sys).unwrap(), path: p.clone(), file: p, title: "x".into(), tags: String::new(), discs: 1, bytes: data.len() as u64, sd: false }
        };
        // iNES header skipped.
        let mut nes = b"NES\x1a".to_vec();
        nes.extend([0u8; 12]);
        nes.extend(b"abc");
        assert_eq!(rom_hash(&mk("a.nes", "nes", &nes)).unwrap(), crate::md5::hex(b"abc"));
        assert_eq!(rom_hash(&mk("b.gb", "gb", b"abc")).unwrap(), "900150983cd24fb0d6963f7d28e17f72");
        // N64 .v64 is byte swapped back to .z64 order.
        let z64 = [0x80, 0x37, 0x12, 0x40, 1, 2, 3, 4];
        let v64 = [0x37, 0x80, 0x40, 0x12, 2, 1, 4, 3];
        assert_eq!(rom_hash(&mk("c.v64", "n64", &v64)), rom_hash(&mk("d.z64", "n64", &z64)));
        assert_eq!(rom_hash(&mk("sf2.zip", "arcade", b"zz")).unwrap(), crate::md5::hex(b"sf2"));
        assert!(rom_hash(&mk("e.iso", "ps2", b"x")).is_none());
    }

    #[test]
    fn duckstation_token_matches_its_cipher() {
        // AES-128-CBC of "abc" zero-padded, key and IV from the stretched SHA-256: the same bytes
        // `openssl enc -aes-128-cbc -nopad` gives for that key and IV.
        let t = duckstation_token("abc", "player", b"0123456789abcdef0123456789abcdef\n");
        assert_eq!(t, "T2qQtdPVC9LUgRMpk1Up9A==");
        assert_ne!(t, duckstation_token("abc", "other", b"0123456789abcdef0123456789abcdef\n"));
        assert_eq!(base64(b"hello"), "aGVsbG8=");
        assert_eq!(base64(b"hel"), "aGVs");
    }

    #[test]
    fn standalone_settings() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        let s = Sign { user: "Rare player".into(), token: "rr_tok".into(), address: "0x1".into(), hardcore: true };
        std::fs::create_dir_all(home.join(".config/PCSX2/inis")).unwrap();
        std::fs::write(home.join(".config/PCSX2/inis/PCSX2.ini"), "[UI]\nSettingsVersion = 1\n").unwrap();
        std::fs::create_dir_all(home.join(".var/app/org.DolphinEmu.dolphin-emu")).unwrap();
        std::fs::create_dir_all(home.join(".var/app/org.ppsspp.PPSSPP")).unwrap();
        for e in ["pcsx2", "dolphin", "ppsspp", "duckstation"] {
            write_standalone(home, e, &s);
        }
        let pcsx2 = std::fs::read_to_string(home.join(".config/PCSX2/inis/PCSX2.ini")).unwrap();
        assert!(pcsx2.contains("[Achievements]") && pcsx2.contains("Host = https://rare.market/v1/retro") && pcsx2.contains("Token = rr_tok") && pcsx2.contains("ChallengeMode = true"));
        assert!(pcsx2.contains("SettingsVersion = 1"));
        let dol = std::fs::read_to_string(home.join(".var/app/org.DolphinEmu.dolphin-emu/config/dolphin-emu/RetroAchievements.ini")).unwrap();
        assert!(dol.contains("HostUrl = https://rare.market/v1/retro") && dol.contains("ApiToken = rr_tok") && dol.contains("HardcoreEnabled = True"));
        let sys = home.join(".var/app/org.ppsspp.PPSSPP/config/ppsspp/PSP/SYSTEM");
        assert!(std::fs::read_to_string(sys.join("ppsspp.ini")).unwrap().contains("AchievementsHost = https://rare.market/v1/retro"));
        assert_eq!(std::fs::read_to_string(sys.join("ppsspp_retroachievements.dat")).unwrap(), "rr_tok");
        // DuckStation has no settings file yet: left for its first-run setup.
        assert!(!home.join(".local/share/duckstation/settings.ini").exists());
    }

    #[test]
    fn emulators_own_achievement_popups_go_quiet_but_not_over_the_players_choice() {
        let t = tempfile::tempdir().unwrap();
        let h = t.path();
        let f = h.join("PCSX2.ini");
        // PCSX2's own defaults, plus one the player turned off and one they set to something else.
        std::fs::write(&f, "[Achievements]\nEnabled = true\nNotifications = true\nOverlays = true\nSoundEffects = false\nLBOverlays = maybe\n").unwrap();
        quiet_ini(h, &f, "Achievements", PCSX2_QUIET);
        let s = std::fs::read_to_string(&f).unwrap();
        assert!(s.contains("Enabled = true") && s.contains("Notifications = false") && s.contains("Overlays = false") && s.contains("UnlockSound = false"), "{s}");
        assert!(s.contains("LBOverlays = maybe"), "{s}");
        // The player turns notifications back on: Rare leaves it.
        std::fs::write(&f, s.replace("Notifications = false", "Notifications = TRUE")).unwrap();
        quiet_ini(h, &f, "Achievements", PCSX2_QUIET);
        let s2 = std::fs::read_to_string(&f).unwrap();
        // "TRUE" is PCSX2's default value again, so it reads as the default: off again.
        assert!(s2.contains("Notifications = false"));
        std::fs::write(&f, s2.replace("Overlays = false\n", "Overlays = 1\n")).unwrap();
        quiet_ini(h, &f, "Achievements", PCSX2_QUIET);
        assert!(std::fs::read_to_string(&f).unwrap().contains("Overlays = 1"));
        // RetroArch: Rare's own launch file.
        let a = crate::retro::Sign { user: "u".into(), token: "t".into(), address: String::new(), hardcore: false };
        let c = crate::retroarch::rare_cfg(Some(&a));
        assert!(c.contains("cheevos_enable = \"true\"") && c.contains("cheevos_challenge_indicators = \"false\"") && c.contains("cheevos_visibility_progress_tracker = \"false\""));
    }

    #[test]
    fn reads_rare_answers() {
        let v: Value = serde_json::json!({"game": {"gameId": 6070, "title": "~Homebrew~ 2048", "consoleId": 7, "iconUrl": "https://rare.market/v1/retro/Images/066649.png", "icon": "x",
            "achievements": [{"id": 37118, "title": "Four!", "description": "Get a 4!", "points": 1, "badge": "40312", "badgeUrl": "u", "lockedUrl": "l", "earnedAt": 1700000000000i64, "hardcore": true}]}});
        let g = game_answer(v).unwrap().unwrap();
        assert_eq!((g.game_id, g.earned()), (6070, 1));
        assert!(game_answer(serde_json::json!({"game": null})).unwrap().is_none());
        assert_eq!(console_name(7), "Nintendo Entertainment System");
        assert_eq!(display_title("~Homebrew~ ~Demo~ Persona 4"), "Persona 4");
        assert_eq!(display_title("Sonic"), "Sonic");
        let s = from_token_answer(&serde_json::json!({"user": "Rare player", "token": "rr_x", "host": "h"}), "0xabc", true).unwrap();
        assert_eq!((s.user.as_str(), s.token.as_str(), s.hardcore), ("Rare player", "rr_x", true));
    }
}
