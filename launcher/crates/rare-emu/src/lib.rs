//! Emulators as first-class apps: update checks and one-press updates for the AppImages in
//! ~/Applications, first-run settings tuned for the 780M handheld, and a default power profile each.

pub mod actions;
pub mod art;
pub mod bios;
pub mod catalog;
pub mod controls;
pub mod dat;
pub mod defaults;
pub mod ident;
pub mod install;
pub mod md5;
pub mod release;
pub mod retro;
pub mod retroarch;
pub mod roms;
pub mod serial;
pub mod setup;
pub mod systems;
pub mod unzip;

pub use rare_core::events::EmuProgress;

use rare_core::{Ctx, Error, Event, Profile};
use rare_system::power::GamePower;
use release::Forge;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct Emu {
    pub id: &'static str,
    pub name: &'static str,
    /// Normalised names (lowercase letters and digits) that identify it in a file name or title.
    keys: &'static [&'static str],
    forge: Forge,
    /// 0 rejects a release asset name; the highest score wins.
    score: fn(&str) -> u32,
    /// Power profile it starts with: heavy ones get more watts, light ones fewer.
    pub power: Profile,
    /// True when first-run settings are written for it.
    pub tuned: bool,
}

fn low(n: &str) -> String {
    n.to_ascii_lowercase()
}
fn image(n: &str) -> bool {
    let l = low(n);
    l.ends_with(".appimage") && !l.contains("aarch64") && !l.contains("arm64") && !l.contains("zsync")
}
fn s_duckstation(n: &str) -> u32 {
    // The SSE2 build is the legacy one: on the 780M it opens with a warning to switch.
    let l = low(n);
    if image(n) && l.contains("x64") {
        1 + !l.contains("sse2") as u32
    } else {
        0
    }
}
fn s_pcsx2(n: &str) -> u32 {
    let l = low(n);
    if image(n) && l.contains("qt") && l.contains("x64") {
        1 + !l.contains("avx") as u32
    } else {
        0
    }
}
fn s_rpcs3(n: &str) -> u32 {
    (image(n) && low(n).contains("linux64")) as u32
}
fn s_cemu(n: &str) -> u32 {
    (image(n) && low(n).contains("x86_64")) as u32
}
fn s_azahar(n: &str) -> u32 {
    let l = low(n);
    if image(n) {
        3
    } else if l.ends_with(".tar.gz") && l.contains("appimage") && !l.contains("aarch64") && !l.contains("arm") {
        1
    } else {
        0
    }
}
fn s_citron(n: &str) -> u32 {
    let l = low(n);
    if image(n) && l.contains("x86_64") {
        1 + l.contains("v3") as u32
    } else {
        0
    }
}
fn s_esde(n: &str) -> u32 {
    let l = low(n);
    (image(n) && l.contains("x64") && !l.contains("steamdeck")) as u32
}
fn s_shadps4(n: &str) -> u32 {
    (image(n) && low(n).contains("qt")) as u32
}

pub const EMULATORS: &[Emu] = &[
    Emu { id: "duckstation", name: "DuckStation", keys: &["duckstation"], forge: Forge::GitHub("stenzek/duckstation"), score: s_duckstation, power: Profile::Quiet, tuned: true },
    Emu { id: "pcsx2", name: "PCSX2", keys: &["pcsx2"], forge: Forge::GitHub("PCSX2/pcsx2"), score: s_pcsx2, power: Profile::Balanced, tuned: true },
    Emu { id: "rpcs3", name: "RPCS3", keys: &["rpcs3"], forge: Forge::GitHub("RPCS3/rpcs3-binaries-linux"), score: s_rpcs3, power: Profile::Performance, tuned: true },
    Emu { id: "cemu", name: "Cemu", keys: &["cemu"], forge: Forge::GitHub("cemu-project/Cemu"), score: s_cemu, power: Profile::Balanced, tuned: true },
    Emu { id: "azahar", name: "Azahar", keys: &["azahar"], forge: Forge::GitHub("azahar-emu/azahar"), score: s_azahar, power: Profile::Quiet, tuned: true },
    Emu { id: "citron", name: "Citron", keys: &["citron"], forge: Forge::Gitea("https://git.citron-emu.org", "Citron/Emulator"), score: s_citron, power: Profile::Performance, tuned: true },
    Emu { id: "shadps4", name: "shadPS4", keys: &["shadps4"], forge: Forge::GitHub("shadps4-emu/shadPS4"), score: s_shadps4, power: Profile::Performance, tuned: false },
    Emu { id: "esde", name: "ES-DE", keys: &["esde", "emulationstationde"], forge: Forge::GitLab("es-de%2Femulationstation-de"), score: s_esde, power: Profile::Quiet, tuned: false },
];

pub fn by_id(id: &str) -> Option<&'static Emu> {
    EMULATORS.iter().find(|e| e.id == id)
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

/// The emulator a file name or title belongs to ("Citron-0.12.25-anylinux-x86_64_v3.AppImage", "ES-DE").
pub fn identify(name: &str) -> Option<&'static Emu> {
    let n = norm(name);
    EMULATORS.iter().find(|e| e.keys.iter().any(|k| n.contains(k)))
}

/// The power profile a game's emulator wants, for the launcher's power code to use as its default
/// until a per-game profile is stored. Takes the program path or the title.
pub fn power_default(program_or_title: &str) -> Option<Profile> {
    identify(program_or_title).map(|e| e.power)
}

fn applications_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join("Applications")
}

/// The installed AppImage for an emulator in `dir`: `.AppImage` files only (so `.bak` and `_old` copies
/// never count), newest first.
pub fn find_installed(e: &Emu, dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|f| {
            let n = f.file_name().to_string_lossy().into_owned();
            !n.starts_with('.') && n.to_ascii_lowercase().ends_with(".appimage") && e.keys.iter().any(|k| norm(&n).contains(k))
        })
        .filter_map(|f| Some((f.metadata().ok()?.modified().ok()?, f.path())))
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().next().map(|f| f.1)
}

// ---- What the launcher remembers about updates it made --------------------------------------

#[derive(Serialize, Deserialize, Default, Clone)]
struct Record {
    tag: String,
    asset: String,
    updated: i64,
}

fn state_path() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("rare/emulators.json")
}
fn load_state() -> HashMap<String, Record> {
    std::fs::read_to_string(state_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}
fn save_state(m: &HashMap<String, Record>) {
    let p = state_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(t) = serde_json::to_string_pretty(m) {
        let _ = std::fs::write(p, t);
    }
}

// ---- Commands ------------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmuInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub path: Option<String>,
    /// File date of the installed AppImage, unix seconds.
    pub modified: Option<i64>,
    /// The release tag of the last update made here, if any.
    pub tag: Option<String>,
    pub power: Profile,
    pub tuned: bool,
    /// Where the official releases are listed.
    pub releases_url: String,
}

fn mtime(p: &Path) -> Option<i64> {
    p.metadata().ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

/// Every known emulator and what is installed. No network.
pub fn emu_list() -> Vec<EmuInfo> {
    let dir = applications_dir();
    let st = load_state();
    EMULATORS
        .iter()
        .map(|e| {
            let p = find_installed(e, &dir);
            EmuInfo {
                id: e.id,
                name: e.name,
                modified: p.as_deref().and_then(mtime),
                path: p.map(|p| p.to_string_lossy().into_owned()),
                tag: st.get(e.id).map(|r| r.tag.clone()),
                power: e.power,
                tuned: e.tuned,
                releases_url: e.forge.page(),
            }
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub id: String,
    pub tag: String,
    pub published: String,
    pub page: String,
    pub asset: String,
    pub size: u64,
    /// "checksum" when the release publishes a SHA-256 for the file, else "none".
    pub verify: &'static str,
    /// True when the release is newer than what is installed.
    pub available: bool,
}

struct Found {
    release: release::Release,
    asset: release::Asset,
}

fn fetch(e: &Emu) -> Result<Found, String> {
    let agent = ureq::AgentBuilder::new().timeout_connect(std::time::Duration::from_secs(10)).timeout_read(std::time::Duration::from_secs(20)).build();
    let body = agent
        .get(&e.forge.api_url())
        .set("User-Agent", "rare-launcher")
        .set("Accept", "application/json")
        .call()
        .map_err(|x| format!("Could not reach the {} releases: {x}", e.name))?
        .into_string()
        .map_err(|x| x.to_string())?;
    let rs = e.forge.parse(&body);
    let (r, a) = release::pick(&rs, e.score).ok_or_else(|| format!("{} has no Linux AppImage in its releases", e.name))?;
    Ok(Found { release: r.clone(), asset: a.clone() })
}

/// Newer than the installed copy? By tag when this launcher updated it last, else by file date.
fn is_newer(rel: &release::Release, installed: Option<&Path>, rec: Option<&Record>) -> bool {
    if let Some(r) = rec {
        return r.tag != rel.tag;
    }
    match (installed.and_then(mtime), release::iso_to_unix(&rel.published)) {
        (Some(have), Some(pub_at)) => pub_at > have + 3600,
        (None, _) => true,
        _ => false,
    }
}

fn info(e: &Emu, f: &Found) -> UpdateInfo {
    let installed = find_installed(e, &applications_dir());
    let st = load_state();
    UpdateInfo {
        id: e.id.into(),
        tag: f.release.tag.clone(),
        published: f.release.published.clone(),
        page: f.release.page.clone(),
        asset: f.asset.name.clone(),
        size: f.asset.size,
        verify: if f.asset.sha256.is_some() || release::sidecar(&f.release, &f.asset).is_some() { "checksum" } else { "none" },
        available: is_newer(&f.release, installed.as_deref(), st.get(e.id)),
    }
}

/// Asks the official releases for the newest version. Changes nothing.
pub fn emu_check(id: &str) -> Result<UpdateInfo, Error> {
    let e = by_id(id).ok_or("Unknown emulator")?;
    Ok(info(e, &fetch(e)?))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDone {
    pub id: String,
    pub tag: String,
    pub path: String,
    /// The previous version, kept next to it.
    pub backup: Option<String>,
    pub verified: install::Verified,
}

/// Downloads, verifies and swaps in the newest AppImage. Only ever runs from a button press.
pub fn emu_update(ctx: &Ctx, id: String) -> Result<UpdateDone, Error> {
    (|| -> Result<UpdateDone, String> {
        let e = by_id(&id).ok_or("Unknown emulator")?;
        let f = fetch(e)?;
        let dir = applications_dir();
        std::fs::create_dir_all(&dir).map_err(|x| x.to_string())?;
        let target = find_installed(e, &dir).unwrap_or_else(|| dir.join(format!("{}.AppImage", e.name.replace(' ', ""))));
        let tmp = dir.join(format!(".{}.rare-new", target.file_name().unwrap_or_default().to_string_lossy()));
        let result = (|| {
            let progress_id = id.clone();
            let handle = ctx.clone();
            let is_tgz = f.asset.name.to_ascii_lowercase().ends_with(".tar.gz");
            let dl = if is_tgz { dir.join(format!(".{}.rare-dl", e.id)) } else { tmp.clone() };
            install::download(&f.asset.url, &dl, |done, total| {
                handle.send(Event::EmuUpdateProgress(EmuProgress { id: progress_id.clone(), done, total }));
            })?;
            // Checksum: GitHub's digest, else a published sidecar file; archives are checked as downloaded.
            let mut sha = f.asset.sha256.clone();
            if sha.is_none() {
                if let Some(sc) = release::sidecar(&f.release, &f.asset) {
                    sha = ureq::get(&sc.url).set("User-Agent", "rare-launcher").call().ok().and_then(|r| r.into_string().ok()).and_then(|t| release::parse_checksum(&t));
                }
            }
            if is_tgz {
                if let (Some(want), true) = (&sha, true) {
                    let got = sha256_file(&dl)?;
                    if !got.eq_ignore_ascii_case(want) {
                        let _ = std::fs::remove_file(&dl);
                        return Err("The download does not match the checksum the release publishes".to_string());
                    }
                }
                let r = install::extract_appimage_tgz(&dl, &tmp);
                let _ = std::fs::remove_file(&dl);
                r?;
                let v = install::verify_file(&tmp, None, 0)?;
                Ok(if sha.is_some() { install::Verified::Checksum } else { v })
            } else {
                install::verify_file(&tmp, sha.as_deref(), f.asset.size)
            }
        })();
        let verified = match result {
            Ok(v) => v,
            Err(m) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(m);
            }
        };
        let backup = install::swap_in(&tmp, &target).map_err(|x| format!("Could not replace the old version: {x}"))?;
        let mut st = load_state();
        st.insert(e.id.into(), Record { tag: f.release.tag.clone(), asset: f.asset.name.clone(), updated: now() });
        save_state(&st);
        Ok(UpdateDone {
            id,
            tag: f.release.tag,
            path: target.to_string_lossy().into_owned(),
            backup: backup.map(|b| b.to_string_lossy().into_owned()),
            verified,
        })
    })()
    .map_err(Error::from)
}

fn sha256_file(p: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(p).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h).map_err(|e| e.to_string())?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Writes the tuned first-run settings for one emulator. Existing config files are never touched.
pub fn apply_defaults(e: &Emu, env: &defaults::Env, app_dir: Option<&Path>) -> Result<defaults::Report, String> {
    let r = match e.id {
        "duckstation" => defaults::duckstation(env, app_dir),
        "pcsx2" => defaults::pcsx2(env),
        "rpcs3" => defaults::rpcs3(env),
        "cemu" => defaults::cemu(env),
        "azahar" => defaults::azahar(env),
        "citron" => defaults::citron(env),
        _ => return Ok(defaults::Report { notes: vec![format!("{} has no tuned defaults yet.", e.name)], ..Default::default() }),
    };
    r.map_err(|x| format!("{}: {x}", e.name))
}

/// Applies the tuned defaults for one emulator on request (Settings, Emulators).
pub fn emu_apply_defaults(id: &str) -> Result<defaults::Report, Error> {
    let e = by_id(id).ok_or("Unknown emulator")?;
    Ok(apply_defaults(e, &defaults::Env::real(), Some(&applications_dir()))?)
}

/// Gives an emulator game its emulator's power profile when nothing is saved for the game yet
/// (`game_power` is the config's per-game map). True when the map changed.
pub fn seed_power(game_power: &mut std::collections::BTreeMap<String, GamePower>, game_id: &str, program: Option<&str>, title: &str) -> bool {
    if game_power.contains_key(game_id) {
        return false;
    }
    let e = program.and_then(|p| Path::new(p).file_name().and_then(|n| n.to_str()).and_then(identify)).or_else(|| identify(title));
    let Some(e) = e else { return false };
    game_power.insert(game_id.to_string(), GamePower { profile: e.power, ..GamePower::DEFAULT });
    true
}

/// Called before a game starts: if it is an emulator with no config yet, give it the tuned defaults.
/// Never fails the launch.
pub fn prepare_launch(program: Option<&str>, title: &str) {
    let e = program.and_then(|p| Path::new(p).file_name().and_then(|n| n.to_str()).and_then(identify)).or_else(|| identify(title));
    if let Some(e) = e.filter(|e| e.tuned) {
        let dir = program.and_then(|p| Path::new(p).parent().map(Path::to_path_buf));
        match apply_defaults(e, &defaults::Env::real(), dir.as_deref()) {
            Ok(r) if !r.written.is_empty() => eprintln!("[emu] {}: first-run settings written: {}", e.name, r.written.join(", ")),
            Ok(_) => {}
            Err(m) => eprintln!("[emu] {m}"),
        }
    }
    // The controller profile (one pad, every button), then the Rare menu's hotkey keys.
    if let Some(e) = e {
        let home = dirs::home_dir().unwrap_or_default();
        let w = controls::ensure(e.id, &home);
        if !w.is_empty() {
            eprintln!("[emu] {}: controller profile written: {}", e.name, w.join(", "));
        }
    }
    actions::prepare(program, title);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_by_name_and_title() {
        assert_eq!(identify("Citron-0.12.25-anylinux-x86_64_v3.AppImage").unwrap().id, "citron");
        assert_eq!(identify("pcsx2-Qt.AppImage").unwrap().id, "pcsx2");
        assert_eq!(identify("ES-DE").unwrap().id, "esde");
        assert_eq!(identify("EmulationStationDE").unwrap().id, "esde");
        assert_eq!(identify("Shadps4-qt.AppImage").unwrap().id, "shadps4");
        assert!(identify("Hades").is_none());
    }

    #[test]
    fn power_mapping() {
        assert_eq!(power_default("DuckStation.AppImage"), Some(Profile::Quiet));
        assert_eq!(power_default("Cemu.AppImage"), Some(Profile::Balanced));
        assert_eq!(power_default("rpcs3.AppImage"), Some(Profile::Performance));
        assert_eq!(power_default("citron-nightly.AppImage"), Some(Profile::Performance));
        assert_eq!(power_default("Hades"), None);
    }

    #[test]
    fn seeds_power_once_and_never_over_a_saved_one() {
        let mut c = std::collections::BTreeMap::<String, GamePower>::new();
        assert!(seed_power(&mut c, "shortcut:1", None, "RPCS3"));
        assert_eq!(c["shortcut:1"].profile, Profile::Performance);
        // The user changes it: a second launch keeps their choice.
        c.get_mut("shortcut:1").unwrap().profile = Profile::Quiet;
        assert!(!seed_power(&mut c, "shortcut:1", None, "RPCS3"));
        assert_eq!(c["shortcut:1"].profile, Profile::Quiet);
        // A game that is not an emulator gets nothing.
        assert!(!seed_power(&mut c, "steam:5", None, "Hades"));
        assert!(seed_power(&mut c, "x", Some("/home/b/Applications/DuckStation.AppImage"), "Whatever"));
        assert_eq!(c["x"].profile, Profile::Quiet);
    }

    #[test]
    fn finds_only_live_appimages() {
        let d = tempfile::tempdir().unwrap();
        for n in ["rpcs3.AppImage", "rpcs3.AppImage_old", "rpcs3.AppImage.bak", ".rpcs3.AppImage.rare-new", "Cemu.AppImage", "notes.txt"] {
            std::fs::write(d.path().join(n), b"x").unwrap();
        }
        let p = find_installed(by_id("rpcs3").unwrap(), d.path()).unwrap();
        assert_eq!(p.file_name().unwrap(), "rpcs3.AppImage");
        assert!(find_installed(by_id("duckstation").unwrap(), d.path()).is_none());
    }

    #[test]
    fn asset_scores_pick_the_x86_64_appimage() {
        assert!(s_duckstation("DuckStation-x64.AppImage") > s_duckstation("DuckStation-x64-SSE2.AppImage"));
        assert_eq!(s_duckstation("DuckStation-x64-SSE2.AppImage"), 1);
        assert_eq!(s_duckstation("DuckStation-arm64.AppImage"), 0);
        assert!(s_pcsx2("pcsx2-v2.4.0-linux-appimage-x64-Qt.AppImage") > s_pcsx2("pcsx2-v2.4.0-linux-appimage-x64-Qt-avx2.AppImage"));
        assert_eq!(s_pcsx2("pcsx2-v2.4.0-windows-x64-Qt.7z"), 0);
        assert_eq!(s_rpcs3("rpcs3-v0.0.39-1_abc_linux_aarch64.AppImage"), 0);
        assert_eq!(s_cemu("Cemu-2.6-x86_64.AppImage"), 1);
        assert!(s_azahar("azahar.AppImage") > s_azahar("azahar-2123-linux-appimage.tar.gz"));
        assert!(s_citron("Citron-0.12.25-anylinux-x86_64_v3.AppImage") > s_citron("Citron-0.12.25-anylinux-x86_64.AppImage"));
        assert_eq!(s_esde("ES-DE_x64.AppImage"), 1);
    }

    #[test]
    fn newer_by_tag_or_file_date() {
        let rel = release::Release { tag: "v2".into(), published: "2026-10-01T00:00:00Z".into(), page: String::new(), prerelease: false, assets: vec![] };
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("a.AppImage");
        std::fs::write(&f, b"x").unwrap(); // dated now: after the release
        assert!(!is_newer(&rel, Some(&f), None));
        assert!(is_newer(&rel, None, None));
        assert!(!is_newer(&rel, Some(&f), Some(&Record { tag: "v2".into(), ..Default::default() })));
        assert!(is_newer(&rel, Some(&f), Some(&Record { tag: "v1".into(), ..Default::default() })));
        let old = release::Release { published: "2099-01-01T00:00:00Z".into(), ..rel };
        assert!(is_newer(&old, Some(&f), None));
    }

    #[test]
    fn apply_defaults_covers_every_tuned_emulator_once() {
        let t = tempfile::tempdir().unwrap();
        let env = defaults::Env { home: t.path().to_path_buf(), pad_guid: Some("0".repeat(32)), pad_name: None, xdg: None };
        for e in EMULATORS.iter().filter(|e| e.tuned) {
            let first = apply_defaults(e, &env, None).unwrap();
            assert!(!first.written.is_empty(), "{}", e.id);
            let again = apply_defaults(e, &env, None).unwrap();
            assert!(again.written.is_empty(), "{} must not rewrite", e.id);
        }
        assert!(apply_defaults(by_id("esde").unwrap(), &env, None).unwrap().written.is_empty());
    }
}
