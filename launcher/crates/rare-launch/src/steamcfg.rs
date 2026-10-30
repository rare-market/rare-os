//! Steam client defaults and Proton tool choice for this device.
//!
//! Everything here edits Steam's own files, so it only runs while Steam is closed, backs the file
//! up once (`config.vdf.bak-rare`), edits through `library::vdfedit` (byte-preserving, verified)
//! and swaps the file in atomically. The Proton choice lives in Steam's own
//! `CompatToolMapping` (app "0" is the global default, an app id is a per-game override), so Steam
//! and the launcher always agree.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use rare_library::vdfedit as vdf;
use serde::Serialize;
use sha2::{Digest, Sha256, Sha512};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use rare_core::events::ProtonProgress;
use rare_core::{Ctx, Event};

const UA: &str = "RareLauncher/0.1";
const STEAM: [&str; 4] = ["InstallConfigStore", "Software", "Valve", "Steam"];
/// Settings > Downloads > "Allow background processing of Vulkan shaders". The key is the registry
/// path steamclient.so carries (`Software\Valve\Steam\ShaderCacheManager\EnableShaderBackgroundProcessing`);
/// Steam keeps its config.vdf `Software/Valve/Steam` section in the same place and already writes
/// `ShaderCacheManager` there.
const SHADER_BG: [&str; 6] = ["InstallConfigStore", "Software", "Valve", "Steam", "ShaderCacheManager", "EnableShaderBackgroundProcessing"];
const PRIORITY_GLOBAL: &str = "75";
const PRIORITY_GAME: &str = "250";

fn steam_path<'a>(extra: &[&'a str]) -> Vec<&'a str> {
    let mut v = STEAM.to_vec();
    v.extend_from_slice(extra);
    v
}

pub fn primary_root() -> Option<PathBuf> {
    let roots = rare_library::steam::find_roots();
    roots.iter().find(|r| r.primary).or(roots.first()).map(|r| r.path.clone())
}

fn config_path(root: &Path) -> PathBuf {
    root.join("config/config.vdf")
}

fn tools_dir(root: &Path) -> PathBuf {
    root.join("compatibilitytools.d")
}

/// Backs the file up once, edits, verifies the bytes on disk and swaps them in.
fn edit_config(path: &Path, still_closed: &dyn Fn() -> bool, f: &dyn Fn(&str) -> Result<String, String>) -> Result<bool, String> {
    let old = fs::read_to_string(path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    let new = f(&old)?;
    if new == old {
        return Ok(false);
    }
    let bak = path.with_extension("vdf.bak-rare");
    if !bak.exists() {
        fs::copy(path, &bak).map_err(|e| format!("Couldn't back up {}: {e}", path.display()))?;
    }
    let tmp = path.with_extension("vdf.rare-tmp");
    let write = || -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(new.as_bytes())?;
        f.sync_all()?;
        if let Ok(m) = fs::metadata(path) {
            let _ = fs::set_permissions(&tmp, m.permissions());
        }
        Ok(())
    };
    if let Err(e) = write() {
        let _ = fs::remove_file(&tmp);
        return Err(format!("Couldn't write {}: {e}", tmp.display()));
    }
    // What is on disk is what was checked, and Steam is still closed.
    if fs::read_to_string(&tmp).ok().as_deref() != Some(new.as_str()) || !still_closed() {
        let _ = fs::remove_file(&tmp);
        return Err("Steam started while its settings were being edited; nothing was changed.".into());
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("Couldn't replace {}: {e}", path.display())
    })?;
    Ok(true)
}

// ---- tools -------------------------------------------------------------------------------------

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    /// Steam's internal name, the value stored in CompatToolMapping.
    pub name: String,
    pub label: String,
    /// "valve", "experimental", "ge", "cachyos" or "other".
    pub kind: String,
    /// An old build nothing needs (the user can remove it).
    pub stale: bool,
}

/// Highest "Proton N.M" in any Steam library, as Valve's internal name `proton_N`.
fn valve_stable(libs: &[PathBuf]) -> Option<Tool> {
    let mut best: Option<(u32, u32, String)> = None;
    for lib in libs {
        let Ok(rd) = fs::read_dir(lib.join("steamapps/common")) else { continue };
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            let Some(v) = n.strip_prefix("Proton ") else { continue };
            let Some((a, b)) = v.split_once('.') else { continue };
            let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) else { continue };
            if e.path().join("proton").is_file() && best.as_ref().is_none_or(|(x, y, _)| (a, b) > (*x, *y)) {
                best = Some((a, b, n));
            }
        }
    }
    best.map(|(a, _, n)| Tool { name: format!("proton_{a}"), label: n, kind: "valve".into(), stale: false })
}

fn valve_experimental(libs: &[PathBuf]) -> Option<Tool> {
    libs.iter().any(|l| l.join("steamapps/common/Proton - Experimental/proton").is_file()).then(|| Tool {
        name: "proton_experimental".into(),
        label: "Proton Experimental".into(),
        kind: "experimental".into(),
        stale: false,
    })
}

/// Tools in compatibilitytools.d, named by the first key of their compatibilitytool.vdf.
fn community_tools(root: &Path) -> Vec<Tool> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(tools_dir(root)) else { return out };
    for e in rd.flatten() {
        let dir = e.file_name().to_string_lossy().into_owned();
        if dir.starts_with('.') {
            continue;
        }
        let Ok(text) = fs::read_to_string(e.path().join("compatibilitytool.vdf")) else { continue };
        let Ok(names) = vdf::keys(&text, &["compatibilitytools", "compat_tools"]) else { continue };
        let Some(name) = names.into_iter().next() else { continue };
        let label = vdf::get(&text, &["compatibilitytools", "compat_tools", &name, "display_name"]).ok().flatten().unwrap_or_else(|| name.clone());
        let lower = name.to_ascii_lowercase();
        let kind = if lower.starts_with("ge-proton") {
            "ge"
        } else if lower.contains("cachyos") {
            "cachyos"
        } else {
            "other"
        };
        out.push(Tool { stale: lower.starts_with("ulwgl-"), name, label, kind: kind.into() });
    }
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out
}

pub fn tools(root: &Path) -> Vec<Tool> {
    let libs = rare_library::steam::registered_libraries(root);
    let mut v = Vec::new();
    v.extend(valve_stable(&libs));
    v.extend(valve_experimental(&libs));
    v.extend(community_tools(root));
    v
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProtonStatus {
    pub tools: Vec<Tool>,
    /// Steam's global default tool, if one is set.
    pub global: Option<String>,
    /// The per-game override for the asked app, if one is set.
    pub game: Option<String>,
    /// Community kinds ("ge", "cachyos") with nothing installed yet.
    pub installable: Vec<String>,
}

pub fn status(root: &Path, appid: Option<u64>) -> ProtonStatus {
    let text = fs::read_to_string(config_path(root)).unwrap_or_default();
    let tool_of = |id: &str| vdf::get(&text, &steam_path(&["CompatToolMapping", id, "name"])).ok().flatten().filter(|s| !s.is_empty());
    let tools = tools(root);
    let installable = ["ge", "cachyos"].iter().filter(|k| !tools.iter().any(|t| t.kind == **k)).map(|k| k.to_string()).collect();
    ProtonStatus { global: tool_of("0"), game: appid.and_then(|a| tool_of(&a.to_string())), tools, installable }
}

fn set_mapping(text: &str, id: &str, name: &str, priority: &str) -> Result<String, String> {
    let t = vdf::set(text, &steam_path(&["CompatToolMapping", id, "name"]), name)?;
    let t = vdf::set(&t, &steam_path(&["CompatToolMapping", id, "config"]), &vdf::get(&t, &steam_path(&["CompatToolMapping", id, "config"]))?.unwrap_or_default())?;
    vdf::set(&t, &steam_path(&["CompatToolMapping", id, "priority"]), priority)
}

/// A per-game choice (`None` goes back to the default). Steam must be closed.
pub fn set_game_tool(root: &Path, appid: u64, tool: Option<&str>, still_closed: &dyn Fn() -> bool) -> Result<(), String> {
    if !still_closed() {
        return Err("Steam is running.".into());
    }
    let id = appid.to_string();
    if let Some(t) = tool {
        if !tools(root).iter().any(|x| x.name == t) {
            return Err(format!("{t} is not installed."));
        }
    }
    edit_config(&config_path(root), still_closed, &|text| match tool {
        Some(t) => set_mapping(text, &id, t, PRIORITY_GAME),
        None => {
            let t = vdf::remove(text, &steam_path(&["CompatToolMapping", &id]))?;
            // Don't leave an empty table behind when it was ours alone.
            let map = steam_path(&["CompatToolMapping"]);
            if vdf::has(&t, &map)? && vdf::keys(&t, &map)?.is_empty() {
                vdf::remove(&t, &map)
            } else {
                Ok(t)
            }
        }
    })
    .map(|_| ())
}

/// Device defaults, once: Valve's stable Proton as the global tool when the user has not chosen one,
/// and background Vulkan shader processing off (shader pre-caching stays on). Nothing is touched
/// while Steam runs, and nothing the user already set is overridden. Returns what changed.
pub fn apply_defaults(root: &Path, marker: &Path, still_closed: &dyn Fn() -> bool) -> Result<Vec<&'static str>, String> {
    if marker.exists() || !still_closed() || !config_path(root).is_file() {
        return Ok(Vec::new());
    }
    let libs = rare_library::steam::registered_libraries(root);
    let stable = valve_stable(&libs);
    let changed = std::cell::RefCell::new(Vec::new());
    edit_config(&config_path(root), still_closed, &|text| {
        let mut t = text.to_string();
        changed.borrow_mut().clear();
        if let Some(s) = &stable {
            if !vdf::has(&t, &steam_path(&["CompatToolMapping", "0"]))? {
                t = set_mapping(&t, "0", &s.name, PRIORITY_GLOBAL)?;
                changed.borrow_mut().push("proton");
            }
        }
        if !vdf::has(&t, &SHADER_BG)? {
            t = vdf::set(&t, &SHADER_BG, "0")?;
            changed.borrow_mut().push("shader-background");
        }
        Ok(t)
    })?;
    if let Some(dir) = marker.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(marker, "1\n");
    Ok(changed.into_inner())
}

fn defaults_marker() -> PathBuf {
    dirs::data_dir().unwrap_or_default().join("rare/steam-defaults-v1")
}

/// Called right before the launcher starts Steam. Never fails the start.
pub fn prepare_start() {
    if crate::launch::steam_running() {
        return;
    }
    let Some(root) = primary_root() else { return };
    match apply_defaults(&root, &defaults_marker(), &|| !crate::launch::steam_running()) {
        Ok(c) if !c.is_empty() => eprintln!("[steamcfg] applied defaults: {}", c.join(", ")),
        Ok(_) => {}
        Err(e) => eprintln!("[steamcfg] defaults skipped: {e}"),
    }
}

/// Removes a community tool the user picked (not Valve's, not anything outside compatibilitytools.d).
pub fn remove_tool(root: &Path, name: &str) -> Result<(), String> {
    for e in fs::read_dir(tools_dir(root)).map_err(|e| e.to_string())?.flatten() {
        let dir = e.file_name().to_string_lossy().into_owned();
        if dir.starts_with('.') || !e.path().is_dir() || e.path().is_symlink() {
            continue;
        }
        let one = community_tools_in(&e.path(), &dir);
        if one.as_deref() == Some(name) {
            return fs::remove_dir_all(e.path()).map_err(|e| format!("Couldn't remove {name}: {e}"));
        }
    }
    Err(format!("{name} is not installed in compatibilitytools.d."))
}

fn community_tools_in(path: &Path, _dir: &str) -> Option<String> {
    let text = fs::read_to_string(path.join("compatibilitytool.vdf")).ok()?;
    vdf::keys(&text, &["compatibilitytools", "compat_tools"]).ok()?.into_iter().next()
}

// ---- install from GitHub releases --------------------------------------------------------------

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

struct Source {
    repo: &'static str,
}

fn source(kind: &str) -> Result<Source, String> {
    match kind {
        "ge" => Ok(Source { repo: "GloriousEggroll/proton-ge-custom" }),
        "cachyos" => Ok(Source { repo: "CachyOS/proton-cachyos" }),
        _ => Err(format!("Unknown Proton build {kind:?}.")),
    }
}

/// x86-64-v3 (what Zen 4 and every AVX2 CPU since Haswell supports), from /proc/cpuinfo flags.
fn cpu_is_v3(flags: &str) -> bool {
    let has = |f: &str| flags.split_whitespace().any(|x| x == f);
    ["avx", "avx2", "bmi1", "bmi2", "f16c", "fma", "abm", "movbe", "xsave"].iter().all(|f| has(f))
}

/// The archive to download out of a release's asset names.
pub fn pick_asset<'a>(kind: &str, names: &'a [String], v3: bool) -> Option<&'a String> {
    let ok = |n: &&String| !n.contains("aarch64") && !n.contains("arm64");
    match kind {
        "ge" => names.iter().filter(ok).find(|n| n.ends_with(".tar.gz")),
        _ => {
            let want = if v3 { "-x86_64_v3.tar.xz" } else { "-x86_64.tar.xz" };
            names.iter().filter(ok).find(|n| n.ends_with(want)).or_else(|| names.iter().filter(ok).find(|n| n.ends_with("-x86_64.tar.xz")))
        }
    }
}

/// The hash in a `.sha512sum` file ("<hex>  <name>") for `name`.
pub fn parse_sha512sum(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let (h, f) = l.split_once(char::is_whitespace)?;
        let f = f.trim().trim_start_matches('*');
        (f == name && h.len() == 128 && h.bytes().all(|b| b.is_ascii_hexdigit())).then(|| h.to_ascii_lowercase())
    })
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(30)).timeout_read(Duration::from_secs(60)).user_agent(UA).build()
}

fn get_text(url: &str) -> Result<String, String> {
    agent().get(url).call().map_err(|e| format!("Request to {url} failed: {e}"))?.into_string().map_err(|e| e.to_string())
}

/// Installs the latest GE-Proton or Proton-CachyOS into compatibilitytools.d. The archive's
/// SHA-512 must match the release's own `.sha512sum` file (and SHA-256 the GitHub asset digest when
/// the API gives one) before anything is unpacked. Returns the tool's internal name.
pub fn install(ctx: &Ctx, root: &Path, kind: &str) -> Result<String, String> {
    let src = source(kind)?;
    let emit = |phase: &str, done: u64, total: u64| {
        ctx.send(Event::ProtonInstall(ProtonProgress { kind: kind.to_string(), phase: phase.to_string(), done, total }));
    };
    emit("lookup", 0, 0);
    let rel: serde_json::Value = serde_json::from_str(&get_text(&format!("https://api.github.com/repos/{}/releases/latest", src.repo))?).map_err(|e| e.to_string())?;
    let assets = rel["assets"].as_array().ok_or("The release has no files.")?;
    let names: Vec<String> = assets.iter().filter_map(|a| a["name"].as_str().map(String::from)).collect();
    let v3 = fs::read_to_string("/proc/cpuinfo").map(|t| t.lines().find(|l| l.starts_with("flags")).is_some_and(|l| cpu_is_v3(l) || cpu_is_v3(&l.replace("lzcnt", "abm")))).unwrap_or(false);
    let name = pick_asset(kind, &names, v3).ok_or("No x86-64 build in the latest release.")?.clone();
    let asset = assets.iter().find(|a| a["name"].as_str() == Some(&name)).ok_or("Asset vanished.")?;
    let size = asset["size"].as_u64().ok_or("The release gives no size.")?;
    let url = asset["browser_download_url"].as_str().ok_or("The release gives no link.")?.to_string();
    let sum_asset = assets.iter().find(|a| a["name"].as_str() == Some(&format!("{name}.sha512sum"))).ok_or("This release publishes no checksum, so it is not installed.")?;
    let sum_url = sum_asset["browser_download_url"].as_str().ok_or("The release gives no checksum link.")?;
    let want512 = parse_sha512sum(&get_text(sum_url)?, &name).ok_or("The checksum file does not list the archive.")?;
    let want256 = asset["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).map(str::to_string);

    let dir = tools_dir(root);
    fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    let work = dir.join(format!(".rare-dl-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let r = (|| -> Result<String, String> {
        let archive = work.join(&name);
        let resp = agent().get(&url).call().map_err(|e| format!("The download failed: {e}"))?;
        let mut body = resp.into_reader().take(size + 1);
        let mut f = fs::File::create(&archive).map_err(|e| e.to_string())?;
        let (mut h512, mut h256) = (Sha512::new(), Sha256::new());
        let mut buf = vec![0u8; 256 * 1024];
        let (mut done, mut last) = (0u64, std::time::Instant::now());
        emit("download", 0, size);
        loop {
            let n = match body.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(format!("The download stopped: {e}")),
            };
            done += n as u64;
            if done > size {
                return Err("The download is larger than the release says.".into());
            }
            h512.update(&buf[..n]);
            h256.update(&buf[..n]);
            f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            if last.elapsed() > Duration::from_millis(400) {
                emit("download", done, size);
                last = std::time::Instant::now();
            }
        }
        f.sync_all().map_err(|e| e.to_string())?;
        emit("verify", done, size);
        if done != size || hex(&h512.finalize()) != want512 || want256.as_ref().is_some_and(|w| !w.eq_ignore_ascii_case(&hex(&h256.finalize()))) {
            return Err("The download did not match its checksum, so it was thrown away.".into());
        }
        emit("unpack", done, size);
        let out = work.join("out");
        fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        let st = std::process::Command::new("tar").arg("-xf").arg(&archive).arg("-C").arg(&out).status().map_err(|e| format!("tar failed to start: {e}"))?;
        if !st.success() {
            return Err("The archive could not be unpacked.".into());
        }
        let tops: Vec<_> = fs::read_dir(&out).map_err(|e| e.to_string())?.flatten().collect();
        let [top] = tops.as_slice() else { return Err("The archive has an unexpected layout.".into()) };
        let dname = top.file_name().to_string_lossy().into_owned();
        if !top.path().is_dir() || dname.is_empty() || !dname.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) || dname.starts_with('.') {
            return Err("The archive has an unexpected layout.".into());
        }
        let tool = community_tools_in(&top.path(), &dname).ok_or("The archive is not a Proton build (no compatibilitytool.vdf).")?;
        let dest = dir.join(&dname);
        if dest.exists() {
            return Ok(tool);
        }
        fs::rename(top.path(), &dest).map_err(|e| format!("Couldn't install into {}: {e}", dest.display()))?;
        Ok(tool)
    })();
    let _ = fs::remove_dir_all(&work);
    emit(if r.is_ok() { "done" } else { "failed" }, 0, 0);
    r
}


// ---- launch options ----------------------------------------------------------------------------

const LOCAL_APPS: [&str; 5] = ["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"];

/// Every Steam user's localconfig.vdf, the most recently changed first.
fn local_configs(root: &Path) -> Vec<PathBuf> {
    let mut v: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(root.join("userdata"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|u| u.file_name().to_str().is_some_and(|n| n != "0" && n.chars().all(|c| c.is_ascii_digit())))
        .map(|u| u.path().join("config/localconfig.vdf"))
        .filter_map(|p| Some((fs::metadata(&p).ok()?.modified().ok()?, p)))
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().map(|(_, p)| p).collect()
}

/// A game's launch options as Steam has them (the newest user's), empty when none.
pub fn launch_options(root: &Path, appid: u64) -> Option<String> {
    let id = appid.to_string();
    let mut path = LOCAL_APPS.to_vec();
    path.extend([id.as_str(), "LaunchOptions"]);
    let p = local_configs(root).into_iter().next()?;
    let text = fs::read_to_string(p).ok()?;
    Some(vdf::get(&text, &path).ok().flatten().unwrap_or_default())
}

/// Sets (empty: clears) a game's launch options for every Steam user. Steam must be closed.
pub fn set_launch_options(root: &Path, appid: u64, opts: &str, still_closed: &dyn Fn() -> bool) -> Result<(), String> {
    if !still_closed() {
        return Err("Steam is running.".into());
    }
    let files = local_configs(root);
    if files.is_empty() {
        return Err("Sign in to Steam first.".into());
    }
    let id = appid.to_string();
    let mut path = LOCAL_APPS.to_vec();
    path.extend([id.as_str(), "LaunchOptions"]);
    let opts = opts.trim();
    for f in files {
        edit_config(&f, still_closed, &|text| if opts.is_empty() { vdf::remove(text, &path) } else { vdf::set(text, &path, opts) })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);

    /// A fake Steam root with Proton 10.0 and 11.0, a stale community tool and a config.vdf.
    fn fixture(config: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("rare-steamcfg-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&root);
        for p in ["Proton 10.0", "Proton 11.0", "Proton - Experimental"] {
            fs::create_dir_all(root.join("steamapps/common").join(p)).unwrap();
            fs::write(root.join("steamapps/common").join(p).join("proton"), "#!/bin/sh\n").unwrap();
        }
        fs::create_dir_all(root.join("compatibilitytools.d/ULWGL-Proton-8.0-5-3")).unwrap();
        fs::write(
            root.join("compatibilitytools.d/ULWGL-Proton-8.0-5-3/compatibilitytool.vdf"),
            "\"compatibilitytools\"\n{\n  \"compat_tools\"\n  {\n    \"ULWGL-Proton-8.0-5-3\" // Internal name\n    {\n      \"install_path\" \".\"\n      \"display_name\" \"ULWGL-Proton-8.0-5-3\"\n    }\n  }\n}\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(config_path(&root), config).unwrap();
        root
    }

    const CFG: &str = "\"InstallConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"AutoUpdateWindowEnabled\"\t\t\"0\"\n\t\t\t\t\"ShaderCacheManager\"\n\t\t\t\t{\n\t\t\t\t\t\"HasCurrentBucket\"\t\t\"1\"\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n";
    const OPEN: &dyn Fn() -> bool = &|| true;

    #[test]
    fn lists_tools_and_flags_the_stale_one() {
        let root = fixture(CFG);
        let t = tools(&root);
        assert_eq!(t[0].name, "proton_11");
        assert_eq!(t[0].label, "Proton 11.0");
        assert_eq!(t[1].name, "proton_experimental");
        let old = t.iter().find(|x| x.stale).unwrap();
        assert_eq!((old.name.as_str(), old.kind.as_str()), ("ULWGL-Proton-8.0-5-3", "other"));
        assert_eq!(status(&root, None).installable, ["ge", "cachyos"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn defaults_apply_once_back_up_and_roundtrip() {
        let root = fixture(CFG);
        let marker = root.join("marker");
        let c = apply_defaults(&root, &marker, OPEN).unwrap();
        assert_eq!(c, ["proton", "shader-background"]);
        let after = fs::read_to_string(config_path(&root)).unwrap();
        assert_eq!(fs::read_to_string(root.join("config/config.vdf.bak-rare")).unwrap(), CFG);
        assert_eq!(status(&root, None).global.as_deref(), Some("proton_11"));
        assert_eq!(vdf::get(&after, &SHADER_BG).unwrap().as_deref(), Some("0"));
        assert_eq!(vdf::get(&after, &steam_path(&["CompatToolMapping", "0", "priority"])).unwrap().as_deref(), Some("75"));
        assert!(after.contains("\"AutoUpdateWindowEnabled\"\t\t\"0\""));
        // Second run: marker present, file untouched.
        assert!(apply_defaults(&root, &marker, OPEN).unwrap().is_empty());
        assert_eq!(fs::read_to_string(config_path(&root)).unwrap(), after);
        assert!(!root.join("config/config.vdf.rare-tmp").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn defaults_never_override_a_user_choice() {
        let cfg = CFG.replace("\t\t\t\t\"ShaderCacheManager\"", "\t\t\t\t\"CompatToolMapping\"\n\t\t\t\t{\n\t\t\t\t\t\"0\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"name\"\t\t\"proton_experimental\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t\t\"ShaderCacheManager\"").replace("\"HasCurrentBucket\"\t\t\"1\"", "\"HasCurrentBucket\"\t\t\"1\"\n\t\t\t\t\t\"EnableShaderBackgroundProcessing\"\t\t\"1\"");
        let root = fixture(&cfg);
        let c = apply_defaults(&root, &root.join("marker"), OPEN).unwrap();
        assert!(c.is_empty());
        assert_eq!(fs::read_to_string(config_path(&root)).unwrap(), cfg);
        assert_eq!(status(&root, None).global.as_deref(), Some("proton_experimental"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn nothing_changes_while_steam_runs_or_on_a_broken_file() {
        let root = fixture(CFG);
        assert!(apply_defaults(&root, &root.join("marker"), &|| false).unwrap().is_empty());
        assert_eq!(fs::read_to_string(config_path(&root)).unwrap(), CFG);
        assert!(set_game_tool(&root, 1145360, Some("proton_11"), &|| false).is_err());
        let broken = "\"InstallConfigStore\"\n{\n\t\"Software\"\n";
        fs::write(config_path(&root), broken).unwrap();
        assert!(apply_defaults(&root, &root.join("marker"), OPEN).is_err());
        assert_eq!(fs::read_to_string(config_path(&root)).unwrap(), broken);
        assert!(!root.join("marker").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn per_game_choice_set_and_clear() {
        let root = fixture(CFG);
        assert!(set_game_tool(&root, 1145360, Some("GE-Proton99"), OPEN).is_err(), "unknown tool refused");
        set_game_tool(&root, 1145360, Some("proton_experimental"), OPEN).unwrap();
        assert_eq!(status(&root, Some(1145360)).game.as_deref(), Some("proton_experimental"));
        assert_eq!(status(&root, Some(1)).game, None);
        set_game_tool(&root, 1145360, None, OPEN).unwrap();
        assert_eq!(fs::read_to_string(config_path(&root)).unwrap(), CFG);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removes_only_the_named_community_tool() {
        let root = fixture(CFG);
        assert!(remove_tool(&root, "proton_11").is_err());
        remove_tool(&root, "ULWGL-Proton-8.0-5-3").unwrap();
        assert!(!root.join("compatibilitytools.d/ULWGL-Proton-8.0-5-3").exists());
        assert!(root.join("steamapps/common/Proton 11.0/proton").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn picks_assets_and_parses_checksums() {
        let ge: Vec<String> = ["GE-Proton11-11-aarch64.sha512sum", "GE-Proton11-11-aarch64.tar.gz", "GE-Proton11-11-x86_64.sha512sum", "GE-Proton11-11-x86_64.tar.gz"].map(String::from).to_vec();
        assert_eq!(pick_asset("ge", &ge, true).unwrap(), "GE-Proton11-11-x86_64.tar.gz");
        let cy: Vec<String> = ["proton-cachyos-11.0-20261015-slr-arm64.tar.xz", "proton-cachyos-11.0-20261015-slr-x86_64.tar.xz", "proton-cachyos-11.0-20261015-slr-x86_64_v3.tar.xz"].map(String::from).to_vec();
        assert_eq!(pick_asset("cachyos", &cy, true).unwrap(), "proton-cachyos-11.0-20261015-slr-x86_64_v3.tar.xz");
        assert_eq!(pick_asset("cachyos", &cy, false).unwrap(), "proton-cachyos-11.0-20261015-slr-x86_64.tar.xz");
        let h = "a".repeat(128);
        assert_eq!(parse_sha512sum(&format!("{h}  GE-Proton11-11-x86_64.tar.gz\n"), "GE-Proton11-11-x86_64.tar.gz").as_deref(), Some(h.as_str()));
        assert!(parse_sha512sum(&format!("{h}  other.tar.gz\n"), "GE-Proton11-11-x86_64.tar.gz").is_none());
        assert!(parse_sha512sum("short  x\n", "x").is_none());
        assert!(cpu_is_v3("flags : fpu avx avx2 bmi1 bmi2 f16c fma abm movbe xsave"));
        assert!(!cpu_is_v3("flags : fpu avx"));
    }
}
