//! RetroArch as Rare runs it: where its config, cores and states are, the settings Rare adds at
//! launch (`--appendconfig`, so the player's retroarch.cfg is never edited), and its network
//! command port, which the Rare menu uses for states, fast forward, rewind, discs and its menu.

use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const FLATPAK: &str = "org.libretro.RetroArch";
pub const CMD_PORT: u16 = 55355;
/// The state slots the Rare menu shows.
pub const SLOTS: u32 = 4;

/// RetroArch's config folder: the Flatpak's, else a native install's.
pub fn config_dir(home: &Path) -> PathBuf {
    let flat = home.join(".var/app").join(FLATPAK).join("config/retroarch");
    if flat.is_dir() || !home.join(".config/retroarch").is_dir() {
        flat
    } else {
        home.join(".config/retroarch")
    }
}

/// `key = "value"` from retroarch.cfg text.
pub fn cfg_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
    })
}

fn expand(v: &str, home: &Path, base: &Path) -> Option<PathBuf> {
    if v.is_empty() || v == "default" {
        return None;
    }
    if let Some(r) = v.strip_prefix("~/") {
        return Some(home.join(r));
    }
    if let Some(r) = v.strip_prefix(":/") {
        return Some(base.join(r));
    }
    Some(PathBuf::from(v))
}

/// What Rare reads from the player's retroarch.cfg.
#[derive(Debug, Clone)]
pub struct Paths {
    pub config: PathBuf,
    pub cores: PathBuf,
    pub states: PathBuf,
    pub saves: PathBuf,
    pub system: PathBuf,
}

pub fn paths(home: &Path) -> Paths {
    let dir = config_dir(home);
    let text = std::fs::read_to_string(dir.join("retroarch.cfg")).unwrap_or_default();
    // RetroArch uses its default folder when the configured one isn't there.
    let get = |k: &str, def: PathBuf| cfg_value(&text, k).and_then(|v| expand(&v, home, &dir)).filter(|p| p.is_dir()).unwrap_or(def);
    Paths {
        cores: get("libretro_directory", dir.join("cores")),
        states: get("savestate_directory", dir.join("states")),
        saves: get("savefile_directory", dir.join("saves")),
        system: get("system_directory", dir.join("system")),
        config: dir,
    }
}

pub fn core_path(p: &Paths, core: &str) -> PathBuf {
    p.cores.join(format!("{core}_libretro.so"))
}

/// Rare's launch-time settings (rare.cfg next to retroarch.cfg): the command port, state
/// thumbnails and no state slot auto-increment (the Rare menu picks slots), and retro trophies:
/// rcheevos pointed at rare.market (`cheevos_custom_host`, RetroArch 1.15 and later) with this
/// device's token, hardcore as set in Rare, RetroArch's own unlock and sign-in pop-ups off (Rare
/// shows unlocks over the game). Written on every launch, so a change in Rare's settings applies
/// at the next start.
pub fn rare_cfg(retro: Option<&crate::retro::Sign>) -> String {
    let mut s = String::from("# Written by Rare at each launch. Your own settings stay in retroarch.cfg.\n");
    for (k, v) in [
        ("network_cmd_enable", "true"),
        ("network_cmd_port", "55355"),
        ("savestate_thumbnail_enable", "true"),
        ("savestate_auto_index", "false"),
        ("video_shader_enable", "true"),
        ("pause_nonactive", "false"),
        ("ui_companion_start_on_boot", "false"),
    ] {
        s.push_str(&format!("{k} = \"{v}\"\n"));
    }
    let clean = |v: &str| v.replace(['"', '\n', '\r'], "");
    match retro.filter(|a| !a.token.is_empty()) {
        Some(a) => {
            for (k, v) in [
                ("cheevos_enable", "true".to_string()),
                ("cheevos_custom_host", clean(&crate::retro::host())),
                ("cheevos_username", clean(&a.user)),
                ("cheevos_token", clean(&a.token)),
                ("cheevos_password", String::new()),
                ("cheevos_hardcore_mode_enable", a.hardcore.to_string()),
                ("cheevos_visibility_unlock", "false".into()),
                ("cheevos_visibility_mastery", "false".into()),
                ("cheevos_visibility_account", "false".into()),
                ("cheevos_unlock_sound_enable", "false".into()),
                // Rare shows the unlocks: none of RetroArch's own pop-ups, indicators or badges.
                ("cheevos_visibility_lboard_start", "false".into()),
                ("cheevos_visibility_lboard_submit", "false".into()),
                ("cheevos_visibility_lboard_cancel", "false".into()),
                ("cheevos_visibility_lboard_trackers", "false".into()),
                ("cheevos_visibility_progress_tracker", "false".into()),
                ("cheevos_visibility_summary", "2".into()),
                ("cheevos_challenge_indicators", "false".into()),
                ("cheevos_badges_enable", "false".into()),
            ] {
                s.push_str(&format!("{k} = \"{v}\"\n"));
            }
        }
        None => s.push_str("cheevos_enable = \"false\"\n"),
    }
    s
}

/// On a device with InputPlumber's pad: SDL's joypad driver, so the grabbed source pad stays
/// hidden (controls::launch_env) and the SuiPlay's pad is port 1. The Flatpak's default, udev,
/// lists the silent source pad first and gives it port 1, so no button reached the game. The pad's
/// autoconfig profile (045e:0b00) comes with RetroArch; every RetroPad button, both sticks, analog
/// L2 and R2 and rumble were checked on the build machine (docs/EMULATION.md).
/// The standard in-game map (controls::STANDARD) also means no RetroArch hotkey on the pad: no
/// menu combo, no hotkey enable button (the Rare menu sends RetroArch's commands instead).
pub const PAD_CFG: &str = "input_joypad_driver = \"sdl2\"\ninput_autodetect_enable = \"true\"\ninput_menu_toggle_gamepad_combo = \"0\"\ninput_enable_hotkey_btn = \"nul\"\ninput_enable_hotkey_axis = \"nul\"\ninput_menu_toggle_btn = \"nul\"\ninput_exit_emulator_btn = \"nul\"\n";

/// Rewind for light systems, off for the rest (a second append file, so a light game's rewind
/// never stays on for a heavy one if RetroArch saves its config).
pub const REWIND_CFG: &str = "rewind_enable = \"true\"\nrewind_granularity = \"1\"\nrewind_buffer_size = \"20\"\n";
pub const NO_REWIND_CFG: &str = "rewind_enable = \"false\"\n";

/// The `--appendconfig` value for a game: rare.cfg, then the rewind file.
pub fn append_arg(home: &Path, rewind: bool) -> String {
    let dir = config_dir(home);
    format!("{}|{}", dir.join("rare.cfg").display(), dir.join(if rewind { "rare-rewind.cfg" } else { "rare-norewind.cfg" }).display())
}

/// Writes rare.cfg and both rewind files; returns the `--appendconfig` value.
pub fn write_append(home: &Path, retro: Option<&crate::retro::Sign>, rewind: bool) -> std::io::Result<String> {
    let dir = config_dir(home);
    std::fs::create_dir_all(&dir)?;
    let mut cfg = rare_cfg(retro);
    if !crate::controls::launch_env().is_empty() {
        cfg.push_str(PAD_CFG);
    }
    std::fs::write(dir.join("rare.cfg"), cfg)?;
    std::fs::write(dir.join("rare-rewind.cfg"), REWIND_CFG)?;
    std::fs::write(dir.join("rare-norewind.cfg"), NO_REWIND_CFG)?;
    Ok(append_arg(home, rewind))
}

/// A fresh retroarch.cfg for a device that has none: EmuDeck's folder layout (saves and states
/// under ~/Emulation/saves/retroarch, BIOS in ~/Emulation/bios), Vulkan, fullscreen at the
/// panel's size, the pad driver SDL2 and Rare's Rare-button menu instead of RetroArch's combo.
pub fn fresh_cfg(home: &Path) -> String {
    let e = home.join("Emulation");
    let p = |s: &str| e.join(s).to_string_lossy().into_owned();
    let lines = [
        ("savefile_directory", p("saves/retroarch/saves")),
        ("savestate_directory", p("saves/retroarch/states")),
        ("system_directory", p("bios")),
        ("screenshot_directory", p("storage/retroarch/screenshots")),
        ("video_driver", "vulkan".into()),
        ("video_fullscreen", "true".into()),
        ("video_windowed_fullscreen", "true".into()),
        ("video_vsync", "true".into()),
        ("video_smooth", "false".into()),
        ("video_scale_integer", "false".into()),
        ("aspect_ratio_index", "22".into()),
        ("input_joypad_driver", "sdl2".into()),
        ("input_autodetect_enable", "true".into()),
        ("input_menu_toggle_gamepad_combo", "0".into()),
        ("menu_driver", "ozone".into()),
        // Rare's launch settings never end up in this file.
        ("config_save_on_exit", "false".into()),
        ("sort_savestates_enable", "false".into()),
        ("sort_savestates_by_content_enable", "false".into()),
        ("sort_savefiles_enable", "false".into()),
        ("sort_savefiles_by_content_enable", "false".into()),
        ("savestate_thumbnail_enable", "true".into()),
        ("audio_driver", "pipewire".into()),
        ("notification_show_remap_load", "false".into()),
        ("notification_show_config_override_load", "false".into()),
        ("video_font_size", "28.000000".into()),
        ("menu_scale_factor", "1.200000".into()),
    ];
    let mut s = String::new();
    for (k, v) in lines {
        s.push_str(&format!("{k} = \"{v}\"\n"));
    }
    s
}

// ---------------------------------------------------------------- state slots

/// One state slot of a game: its file and thumbnail when saved.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub slot: u32,
    /// Unix seconds when saved; None when empty.
    pub saved: Option<i64>,
    pub thumb: Option<String>,
}

fn state_name(stem: &str, slot: u32) -> String {
    if slot == 0 {
        format!("{stem}.state")
    } else {
        format!("{stem}.state{slot}")
    }
}

/// The folders a game's states can be in: the states folder, and one level below it (RetroArch's
/// "sort by core" and "sort by content folder" options).
fn state_dirs(states: &Path) -> Vec<PathBuf> {
    let mut v = vec![states.to_path_buf()];
    if let Ok(rd) = std::fs::read_dir(states) {
        let mut subs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        subs.sort();
        v.extend(subs);
    }
    v
}

/// Slots 1..=SLOTS of a game (RetroArch names states after the content file's stem).
pub fn slots(states: &Path, rom: &Path) -> Vec<Slot> {
    let stem = rom.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let dirs = state_dirs(states);
    (1..=SLOTS)
        .map(|n| {
            let name = state_name(&stem, n);
            let found = dirs.iter().map(|d| d.join(&name)).find(|p| p.is_file());
            let saved = found.as_ref().and_then(|p| p.metadata().ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64);
            let thumb = found.as_ref().map(|p| p.with_file_name(format!("{name}.png"))).filter(|p| p.is_file()).map(|p| p.to_string_lossy().into_owned());
            Slot { slot: n, saved, thumb }
        })
        .collect()
}

/// The slot a new save goes to: the first empty one, else the oldest.
pub fn next_slot(s: &[Slot]) -> u32 {
    s.iter().find(|x| x.saved.is_none()).or_else(|| s.iter().min_by_key(|x| x.saved)).map(|x| x.slot).unwrap_or(1)
}

// ---------------------------------------------------------------- network commands

/// Sends one command to the running RetroArch.
pub fn send(cmd: &str) -> Result<(), String> {
    let s = UdpSocket::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    s.send_to(cmd.as_bytes(), ("127.0.0.1", CMD_PORT)).map_err(|e| format!("RetroArch didn't take the command: {e}"))?;
    Ok(())
}

/// Sends a command that answers, and waits for the reply. Never GET_STATUS: RetroArch 1.22 stops
/// answering any command after it.
pub fn ask(cmd: &str) -> Result<String, String> {
    let s = UdpSocket::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_millis(600))).ok();
    s.send_to(cmd.as_bytes(), ("127.0.0.1", CMD_PORT)).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 4096];
    let (n, _) = s.recv_from(&mut buf).map_err(|_| "RetroArch isn't answering. Is the game still running?".to_string())?;
    Ok(String::from_utf8_lossy(&buf[..n]).trim().to_string())
}

/// Saves into a slot. RetroArch 1.22 has no SAVE_STATE_SLOT, so the slot is set by steps: down to
/// its lowest (-1, "auto") then up to `slot`, and SAVE_STATE saves there (with its picture). The
/// steps are hotkeys to RetroArch: one in two frames back to back reads as a held key and counts
/// once, so they go 90 ms apart.
pub fn save_slot(slot: u32) -> Result<(), String> {
    let s = UdpSocket::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let go = |c: &str| -> Result<(), String> {
        s.send_to(c.as_bytes(), ("127.0.0.1", CMD_PORT)).map_err(|e| format!("RetroArch didn't take the command: {e}"))?;
        std::thread::sleep(Duration::from_millis(90));
        Ok(())
    };
    for _ in 0..(SLOTS + 3) {
        go("STATE_SLOT_MINUS")?;
    }
    for _ in 0..=slot {
        go("STATE_SLOT_PLUS")?;
    }
    std::thread::sleep(Duration::from_millis(100));
    go("SAVE_STATE")
}

pub fn load_slot(slot: u32) -> Result<(), String> {
    send(&format!("LOAD_STATE_SLOT {slot}"))
}

/// Rewinds about `secs` seconds: REWIND is a held key, so it is sent once a frame for that long.
pub fn rewind(secs: f32) -> Result<(), String> {
    let s = UdpSocket::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let frames = (secs * 60.0) as u32;
    for _ in 0..frames {
        s.send_to(b"REWIND", ("127.0.0.1", CMD_PORT)).map_err(|e| e.to_string())?;
        std::thread::sleep(Duration::from_millis(16));
    }
    Ok(())
}

/// Next disc: open the tray, change the disc, close it.
pub fn swap_disc() -> Result<(), String> {
    send("DISK_EJECT_TOGGLE")?;
    std::thread::sleep(Duration::from_millis(250));
    send("DISK_NEXT")?;
    std::thread::sleep(Duration::from_millis(250));
    send("DISK_EJECT_TOGGLE")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_paths_from_the_players_config() {
        let t = tempfile::tempdir().unwrap();
        let h = t.path();
        let dir = h.join(".var/app/org.libretro.RetroArch/config/retroarch");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("retroarch.cfg"), "savestate_directory = \"~/Emulation/saves/retroarch/states\"\nlibretro_directory = \":/cores\"\nsystem_directory = \"default\"\n").unwrap();
        // A folder that isn't there: RetroArch uses its own.
        assert_eq!(paths(h).states, dir.join("states"));
        std::fs::create_dir_all(h.join("Emulation/saves/retroarch/states")).unwrap();
        std::fs::create_dir_all(dir.join("cores")).unwrap();
        let p = paths(h);
        assert_eq!(p.states, h.join("Emulation/saves/retroarch/states"));
        assert_eq!(p.cores, dir.join("cores"));
        assert_eq!(p.system, dir.join("system"));
        assert_eq!(core_path(&p, "snes9x"), dir.join("cores/snes9x_libretro.so"));
    }

    #[test]
    fn slots_and_thumbnails() {
        let t = tempfile::tempdir().unwrap();
        let st = t.path().join("states");
        std::fs::create_dir_all(st.join("Gambatte")).unwrap();
        std::fs::write(st.join("Tetris (World).state2"), b"s").unwrap();
        std::fs::write(st.join("Tetris (World).state2.png"), b"p").unwrap();
        std::fs::write(st.join("Gambatte/Tetris (World).state1"), b"s").unwrap();
        let s = slots(&st, Path::new("/roms/gb/Tetris (World).gb"));
        assert_eq!(s.len(), 4);
        assert!(s[0].saved.is_some() && s[0].thumb.is_none());
        assert!(s[1].thumb.as_deref().unwrap().ends_with("Tetris (World).state2.png"));
        assert!(s[2].saved.is_none());
        assert_eq!(next_slot(&s), 3);
    }

    #[test]
    fn append_config() {
        let c = rare_cfg(None);
        assert!(c.contains("network_cmd_enable = \"true\"") && c.contains("cheevos_enable = \"false\""));
        let a = crate::retro::Sign { user: "player".into(), token: "tok".into(), address: "0x1".into(), hardcore: true };
        let c = rare_cfg(Some(&a));
        assert!(c.contains("cheevos_token = \"tok\"") && c.contains("cheevos_hardcore_mode_enable = \"true\""));
        assert!(c.contains("cheevos_custom_host = \"https://rare.market/v1/retro\"") && c.contains("cheevos_visibility_unlock = \"false\""));
        assert!(fresh_cfg(Path::new("/home/x")).contains("savestate_directory = \"/home/x/Emulation/saves/retroarch/states\""));
    }
}
