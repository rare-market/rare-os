//! Frame generation (lsfg-vk, Lossless Scaling's frame generation as a Vulkan layer) and picture
//! filters (vkBasalt: CAS sharpening, a colour LUT, the player's own ReShade shaders), per game.
//!
//! Both are Vulkan layers, so they work in any Vulkan or Proton (DXVK, VKD3D) game. Neither layer
//! is loaded system-wide: lsfg-vk's own manifest would load it into every Vulkan process
//! (gamescope included) and vkBasalt's needs `ENABLE_VKBASALT=1` in the game's environment, which
//! Rare can't give a game Steam starts. Instead Rare writes one implicit-layer manifest per game
//! that has one of them on, in `~/.local/share/vulkan/implicit_layer.d/rare-fx-*.json`, enabled
//! only by that game's own environment:
//!   - a Steam game by `SteamAppId=<appid>` (Steam sets it for every game it starts, and Proton and
//!     the Steam Linux Runtime pass it on);
//!   - a game Rare starts itself (umu, legendary, a native program) by `RARE_FX=<slug>`, which
//!     rare-launch adds to its environment (`launch_env`).
//! The manifest is a copy of the layer's own with a unique name (the Vulkan loader keeps only the
//! first layer of a name), an absolute library path and that `enable_environment`.
//!
//! lsfg-vk 2 reads `~/.config/lsfg-vk/conf.toml` (version 2): one `[[profile]]` per game, found by
//! Steam App ID (`active_in = ["<appid>"]`) or by `LSFGVK_PROFILE` for games Rare starts. It
//! reloads the multiplier and flow scale while the game runs, so 2x and 3x change at once; a
//! multiplier of 1 passes frames through (Off while the game runs). Turning it on needs the next
//! start of the game, since the layer is loaded when the game starts. The frame generation itself
//! is Lossless Scaling's: lsfg-vk loads `lsfg-vk.dll` (the Steam app's "lsfg-vk" beta branch) or
//! `Lossless.dll` from the player's own Lossless Scaling install. `status` finds it in the Steam
//! libraries; the UI says what is missing.
//!
//! vkBasalt reads its config when the game starts and never again: `~/.config/vkBasalt/vkBasalt.conf`
//! for a Steam game (rewritten for the game as it starts, `starting`), a per-game file through
//! `VKBASALT_CONFIG_FILE` for a game Rare starts. Colour is a 3D LUT Rare writes (vibrance: the
//! less saturated a colour, the more it gains), so no third-party shader is needed for it.
//! ReShade `.fx` files the player puts in `~/.local/share/rare/reshade/Shaders` (textures in
//! `Textures`) run after the built-in effects when ReShade is on for the game.
//!
//! Libraries: lsfg-vk from the image (`/usr/lib/rare/lsfg-vk`) or the launcher's own folder
//! (`<launcher>/lsfg-vk`, until the image has it), both laid out as lsfg-vk's release archive
//! (`lib/`, `share/vulkan/implicit_layer.d/`). vkBasalt is the distribution's package (its manifest
//! in `/usr/share/vulkan/implicit_layer.d`).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Lossless Scaling's Steam app id.
pub const LOSSLESS_APPID: u32 = 993090;

/// Frame generation for one game.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", default)]
pub struct FrameGen {
    /// 1 is off (frames pass through), 2 or 3 generated frames per real one minus one.
    pub mult: u8,
    /// Flow scale in percent, 25 to 100: lower is faster and softer.
    pub flow: u8,
    /// lsfg-vk's lighter model; much faster on a handheld GPU for a small loss in quality.
    pub perf: bool,
}

impl Default for FrameGen {
    fn default() -> Self {
        FrameGen { mult: 1, flow: 75, perf: true }
    }
}

impl FrameGen {
    pub fn on(&self) -> bool {
        self.mult >= 2
    }
}

/// Picture filters for one game.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase", default)]
pub struct Filters {
    /// CAS sharpening: 0 off, 1 low, 2 medium, 3 high.
    pub sharpen: u8,
    /// Colour: 0 natural, 1 vivid, 2 bold.
    pub color: u8,
    /// The player's ReShade shaders.
    pub reshade: bool,
}

impl Filters {
    pub fn on(&self) -> bool {
        self.sharpen > 0 || self.color > 0 || self.reshade
    }
    pub fn cas(&self) -> Option<f32> {
        match self.sharpen {
            0 => None,
            1 => Some(0.3),
            2 => Some(0.6),
            _ => Some(0.9),
        }
    }
    /// Vibrance strength of the colour LUT.
    pub fn vibrance(&self) -> Option<f32> {
        match self.color {
            0 => None,
            1 => Some(0.35),
            _ => Some(0.7),
        }
    }
}

/// What frame generation can do on this device now.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FgStatus {
    Ready,
    /// The lsfg-vk layer is not on this device (an image without it and no launcher copy).
    NoLayer,
    /// Lossless Scaling is not installed in any Steam library.
    NoDll,
    /// Only Lossless.dll: the app's default branch, which lsfg-vk may not match.
    OldBranch,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub framegen: FgStatus,
    /// vkBasalt is installed.
    pub filters: bool,
    /// ReShade .fx files in the player's folder.
    pub reshade: usize,
}

/// Where everything lives; `Paths::system()` on the device, a temp dir in tests.
#[derive(Clone, Debug)]
pub struct Paths {
    /// $XDG_DATA_HOME (~/.local/share).
    pub data: PathBuf,
    /// $XDG_CONFIG_HOME (~/.config).
    pub config: PathBuf,
    /// lsfg-vk release trees, first found wins.
    pub lsfg: Vec<PathBuf>,
    /// System implicit-layer folders (vkBasalt's manifest).
    pub layers: Vec<PathBuf>,
    /// Steam roots (each has steamapps/libraryfolders.vdf).
    pub steam: Vec<PathBuf>,
}

impl Paths {
    pub fn system() -> Paths {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        let env_dir = |k: &str, d: &str| std::env::var_os(k).map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home.join(d));
        let mut lsfg = vec![PathBuf::from("/usr/lib/rare/lsfg-vk")];
        if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
            lsfg.push(dir.join("lsfg-vk"));
        }
        let data = env_dir("XDG_DATA_HOME", ".local/share");
        lsfg.push(data.join("rare/lsfg-vk"));
        Paths {
            config: env_dir("XDG_CONFIG_HOME", ".config"),
            lsfg,
            layers: vec!["/usr/share/vulkan/implicit_layer.d".into(), "/etc/vulkan/implicit_layer.d".into(), "/usr/local/share/vulkan/implicit_layer.d".into()],
            steam: vec![data.join("Steam"), home.join(".steam/steam"), home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam")],
            data,
        }
    }

    fn layer_dir(&self) -> PathBuf {
        self.data.join("vulkan/implicit_layer.d")
    }
    fn own(&self) -> PathBuf {
        self.data.join("rare/gamefx")
    }
    pub fn reshade(&self) -> PathBuf {
        self.data.join("rare/reshade")
    }
    fn lsfg_conf(&self) -> PathBuf {
        self.config.join("lsfg-vk/conf.toml")
    }
    fn basalt_conf(&self) -> PathBuf {
        self.config.join("vkBasalt/vkBasalt.conf")
    }
}

/// How a game's environment names it to its manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Steam(u32),
    Rare(String),
}

impl Key {
    pub fn of(id: &str) -> Key {
        if let Some(n) = id.strip_prefix("steam:").and_then(|s| s.parse().ok()) {
            return Key::Steam(n);
        }
        Key::Rare(slug(id))
    }
    /// The file and layer name part.
    fn tag(&self) -> String {
        match self {
            Key::Steam(n) => format!("steam_{n}"),
            Key::Rare(s) => s.clone(),
        }
    }
    fn env(&self) -> (&'static str, String) {
        match self {
            Key::Steam(n) => ("SteamAppId", n.to_string()),
            Key::Rare(s) => ("RARE_FX", s.clone()),
        }
    }
    fn profile(&self) -> String {
        format!("rare {}", self.tag())
    }
}

/// A game id as a file and layer name part: letters and digits, the rest `_`, at most 48 bytes
/// with a hash of the whole id when it was cut or changed.
pub fn slug(id: &str) -> String {
    let clean: String = id.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    if clean.len() <= 40 && clean.chars().zip(id.chars()).all(|(a, b)| a == b.to_ascii_lowercase() || (a == '_' && b == ':')) {
        return clean;
    }
    let mut h: u64 = 0xcbf29ce484222325;
    for b in id.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{}_{:08x}", &clean[..clean.len().min(39)], h as u32)
}

/// One game's settings as `sync` sees them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Game {
    pub framegen: Option<FrameGen>,
    pub filters: Option<Filters>,
}

/// The lsfg-vk manifests found (64-bit first), as (manifest path, parsed json).
fn lsfg_templates(p: &Paths) -> Vec<(PathBuf, Value)> {
    for root in &p.lsfg {
        let dir = root.join("share/vulkan/implicit_layer.d");
        let mut found: Vec<(PathBuf, Value)> = read_manifests(&dir).into_iter().filter(|(_, v)| layer_name(v).starts_with("VK_LAYER_LSFGVK_frame_generation")).collect();
        found.retain(|(m, v)| lib_path(m, v).is_some_and(|l| l.is_file()));
        if !found.is_empty() {
            found.sort_by_key(|(_, v)| layer_name(v).ends_with("_x86"));
            return found;
        }
    }
    Vec::new()
}

fn basalt_template(p: &Paths) -> Option<(PathBuf, Value)> {
    p.layers.iter().flat_map(|d| read_manifests(d)).find(|(_, v)| layer_name(v) == "VK_LAYER_VKBASALT_post_processing")
}

fn read_manifests(dir: &Path) -> Vec<(PathBuf, Value)> {
    let mut out: Vec<(PathBuf, Value)> = fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| Some((p.clone(), serde_json::from_slice(&fs::read(&p).ok()?).ok()?)))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn layer_name(v: &Value) -> &str {
    v.pointer("/layer/name").and_then(Value::as_str).unwrap_or_default()
}

/// The manifest's library as an absolute path; None for a bare name or `$LIB` (kept as it is).
fn lib_path(manifest: &Path, v: &Value) -> Option<PathBuf> {
    let l = v.pointer("/layer/library_path").and_then(Value::as_str)?;
    if l.contains('$') || !l.contains('/') {
        return None;
    }
    let p = Path::new(l);
    Some(if p.is_absolute() { p.to_path_buf() } else { manifest.parent()?.join(p) })
}

/// A copy of a layer manifest for one game: its own name, an absolute library and the game's
/// environment as the only way to turn it on.
fn game_manifest(template: &(PathBuf, Value), key: &Key) -> Value {
    let (path, v) = template;
    let mut m = v.clone();
    let name = format!("{}_rare_{}", layer_name(v), key.tag());
    if let Some(l) = lib_path(path, v) {
        let abs = fs::canonicalize(&l).unwrap_or(l);
        m["layer"]["library_path"] = Value::String(abs.to_string_lossy().into_owned());
    }
    m["layer"]["name"] = Value::String(name);
    let (k, val) = key.env();
    m["layer"]["enable_environment"] = serde_json::json!({ k: val });
    m
}

/// Lossless Scaling's frame generation DLL in a Steam library: (path, is it lsfg-vk.dll).
pub fn find_dll(p: &Paths) -> Option<(PathBuf, bool)> {
    let mut libs: Vec<PathBuf> = Vec::new();
    for root in &p.steam {
        libs.push(root.clone());
        if let Ok(t) = fs::read_to_string(root.join("steamapps/libraryfolders.vdf")) {
            libs.extend(vdf_paths(&t));
        }
    }
    let mut old = None;
    for lib in libs {
        let dir = lib.join("steamapps/common/Lossless Scaling");
        if dir.join("lsfg-vk.dll").is_file() {
            return Some((dir.join("lsfg-vk.dll"), true));
        }
        if old.is_none() && dir.join("Lossless.dll").is_file() {
            old = Some((dir.join("Lossless.dll"), false));
        }
    }
    old
}

/// The `"path"` values of libraryfolders.vdf.
fn vdf_paths(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("\"path\"")?.trim();
            let v = rest.strip_prefix('"')?.strip_suffix('"')?;
            Some(PathBuf::from(v.replace("\\\\", "\\")))
        })
        .collect()
}

/// ReShade effects in the player's folder: (name, file), sorted by name.
pub fn reshade_effects(p: &Paths) -> Vec<(String, PathBuf)> {
    let dir = p.reshade().join("Shaders");
    let mut out: Vec<(String, PathBuf)> = fs::read_dir(&dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("fx"))).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|f| {
            let stem = f.file_stem()?.to_str()?;
            // vkBasalt reads `name = path` lines: the name must be one plain word.
            let name: String = stem.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            (!name.is_empty()).then_some((format!("rs_{name}"), f))
        })
        .collect();
    out.sort();
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

pub fn status_at(p: &Paths) -> Status {
    let framegen = if lsfg_templates(p).is_empty() {
        FgStatus::NoLayer
    } else {
        match find_dll(p) {
            Some((_, true)) => FgStatus::Ready,
            Some((_, false)) => FgStatus::OldBranch,
            None => FgStatus::NoDll,
        }
    };
    Status { framegen, filters: basalt_template(p).is_some(), reshade: reshade_effects(p).len() }
}

pub fn status() -> Status {
    status_at(&Paths::system())
}

/// conf.toml for lsfg-vk 2: a profile for every game with frame generation saved (off ones at
/// multiplier 1, so a game running with the layer passes frames through).
pub fn lsfg_conf(games: &BTreeMap<String, Game>, dll: Option<&Path>) -> String {
    let mut s = String::from("# Written by Rare from each game's Performance settings. Changes here are replaced.\nversion = 2\n\n[global]\nallow_fp16 = true\n");
    if let Some(d) = dll {
        s += &format!("dll = {}\n", toml_str(&d.to_string_lossy()));
    }
    for (id, g) in games {
        let Some(f) = g.framegen else { continue };
        let key = Key::of(id);
        s += &format!("\n[[profile]]\nname = {}\n", toml_str(&key.profile()));
        match &key {
            Key::Steam(n) => s += &format!("active_in = [{}]\n", toml_str(&n.to_string())),
            Key::Rare(_) => s += "active_in = []\n",
        }
        s += &format!("multiplier = {}\nflow_scale = {:.2}\nperformance_mode = {}\n", if f.on() { f.mult.min(4) } else { 1 }, f.flow.clamp(25, 100) as f32 / 100.0, f.perf);
    }
    s
}

fn toml_str(v: &str) -> String {
    let mut o = String::from("\"");
    for c in v.chars() {
        match c {
            '"' => o += "\\\"",
            '\\' => o += "\\\\",
            c if (c as u32) < 0x20 => o += &format!("\\u{:04x}", c as u32),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// vkBasalt's config for one game's filters.
pub fn basalt_conf(p: &Paths, title: &str, f: &Filters) -> String {
    let mut effects: Vec<String> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    if let Some(c) = f.cas() {
        effects.push("cas".into());
        lines.push(format!("casSharpness = {c:.2}"));
    }
    if let Some(v) = f.vibrance() {
        effects.push("lut".into());
        lines.push(format!("lutFile = {}", lut_path(p, v).display()));
    }
    if f.reshade {
        let fx = reshade_effects(p);
        if !fx.is_empty() {
            lines.push(format!("reshadeIncludePath = {}", p.reshade().join("Shaders").display()));
            lines.push(format!("reshadeTexturePath = {}", p.reshade().join("Textures").display()));
        }
        for (name, file) in fx {
            effects.push(name.clone());
            lines.push(format!("{name} = {}", file.display()));
        }
    }
    let title: String = title.chars().filter(|c| !c.is_control()).collect();
    let mut s = format!("# Written by Rare for {title}. Changes here are replaced when a game starts.\n");
    // An empty list would make vkBasalt fall back to its default (CAS); the layer is not loaded
    // for a game with nothing on, so this only matters for a hand-started one.
    s += &format!("effects = {}\n", if effects.is_empty() { "cas".to_string() } else { effects.join(":") });
    if effects.is_empty() {
        s += "casSharpness = -1.0\nenableOnLaunch = False\n";
    } else {
        s += "enableOnLaunch = True\n";
    }
    s += "toggleKey = Home\ndepthCapture = off\n";
    for l in lines {
        s += &l;
        s.push('\n');
    }
    s
}

fn lut_path(p: &Paths, v: f32) -> PathBuf {
    p.own().join(format!("vibrance-{}.cube", (v * 100.0).round() as u32))
}

/// A 3D LUT (.cube, 17 points per side, red fastest) that adds vibrance: each colour moves away
/// from its luma by `strength * (1 - saturation)`, so dull colours gain most and skin tones and
/// already bright colours little.
pub fn vibrance_cube(strength: f32) -> String {
    const N: usize = 17;
    let mut s = format!("# Rare vibrance {strength:.2}\nLUT_3D_SIZE {N}\nDOMAIN_MIN 0.0 0.0 0.0\nDOMAIN_MAX 1.0 1.0 1.0\n");
    for b in 0..N {
        for g in 0..N {
            for r in 0..N {
                let c = [r as f32, g as f32, b as f32].map(|x| x / (N - 1) as f32);
                let o = vibrance(c, strength);
                s += &format!("{:.6} {:.6} {:.6}\n", o[0], o[1], o[2]);
            }
        }
    }
    s
}

pub fn vibrance(c: [f32; 3], strength: f32) -> [f32; 3] {
    let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    let k = 1.0 + strength * (1.0 - (max - min));
    c.map(|x| (luma + (x - luma) * k).clamp(0.0, 1.0))
}

fn write_if_changed(path: &Path, text: &str) -> std::io::Result<bool> {
    if fs::read_to_string(path).ok().as_deref() == Some(text) {
        return Ok(false);
    }
    if let Some(d) = path.parent() {
        fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("rare-tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(true)
}

/// Writes every game's manifests, lsfg-vk's config, the LUTs and the per-game vkBasalt configs,
/// and removes manifests of games that no longer have anything on. Returns the files written.
pub fn sync_at(p: &Paths, games: &BTreeMap<String, Game>) -> Result<Vec<PathBuf>, String> {
    let mut wrote = Vec::new();
    let mut keep: Vec<PathBuf> = Vec::new();
    let dll = find_dll(p).map(|d| d.0);
    // lsfg-vk ends the game when it can't load Lossless Scaling's DLL: no DLL, no manifest.
    let lsfg = if dll.is_some() { lsfg_templates(p) } else { Vec::new() };
    let basalt = basalt_template(p);
    let layer_dir = p.layer_dir();
    let mut err = Vec::new();
    let mut put = |path: PathBuf, text: String, wrote: &mut Vec<PathBuf>| match write_if_changed(&path, &text) {
        Ok(true) => wrote.push(path),
        Ok(false) => {}
        Err(e) => err.push(format!("{}: {e}", path.display())),
    };
    for (id, g) in games {
        let key = Key::of(id);
        if g.framegen.is_some_and(|f| f.on()) {
            for (i, t) in lsfg.iter().enumerate() {
                let path = layer_dir.join(format!("rare-fx-{}-lsfg{}.json", key.tag(), if i == 0 { String::new() } else { i.to_string() }));
                put(path.clone(), serde_json::to_string_pretty(&game_manifest(t, &key)).unwrap_or_default(), &mut wrote);
                keep.push(path);
            }
        }
        if let (Some(f), Some(t)) = (g.filters.filter(Filters::on), basalt.as_ref()) {
            let path = layer_dir.join(format!("rare-fx-{}-basalt.json", key.tag()));
            put(path.clone(), serde_json::to_string_pretty(&game_manifest(t, &key)).unwrap_or_default(), &mut wrote);
            keep.push(path);
            if let Some(v) = f.vibrance() {
                let lut = lut_path(p, v);
                if !lut.is_file() {
                    put(lut, vibrance_cube(v), &mut wrote);
                }
            }
            put(p.own().join(format!("{}.conf", key.tag())), basalt_conf(p, id, &f), &mut wrote);
        }
    }
    if games.values().any(|g| g.framegen.is_some()) {
        put(p.lsfg_conf(), lsfg_conf(games, dll.as_deref()), &mut wrote);
    }
    // Manifests of games that have nothing on any more.
    if let Ok(rd) = fs::read_dir(&layer_dir) {
        for e in rd.flatten() {
            let path = e.path();
            let ours = path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("rare-fx-") && n.ends_with(".json"));
            if ours && !keep.contains(&path) {
                if let Err(e) = fs::remove_file(&path) {
                    err.push(format!("{}: {e}", path.display()));
                }
            }
        }
    }
    if err.is_empty() {
        Ok(wrote)
    } else {
        Err(err.join("; "))
    }
}

pub fn sync(games: &BTreeMap<String, Game>) {
    match sync_at(&Paths::system(), games) {
        Ok(w) if !w.is_empty() => log::info!("gamefx: wrote {}", w.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")),
        Ok(_) => {}
        Err(e) => log::warn!("gamefx: {e}"),
    }
}

/// A Steam game is starting: vkBasalt's user config becomes this game's (vkBasalt reads it once,
/// as the game starts). Nothing changes for a game without filters.
pub fn starting_at(p: &Paths, id: &str, filters: Option<Filters>) -> Result<bool, String> {
    let Some(f) = filters.filter(Filters::on) else { return Ok(false) };
    if !matches!(Key::of(id), Key::Steam(_)) {
        return Ok(false);
    }
    write_if_changed(&p.basalt_conf(), &basalt_conf(p, id, &f)).map_err(|e| e.to_string())
}

pub fn starting(id: &str, filters: Option<Filters>) {
    if let Err(e) = starting_at(&Paths::system(), id, filters) {
        log::warn!("gamefx: vkBasalt config: {e}");
    }
}

/// The environment for a game Rare starts itself: the key its manifests wait for, its lsfg-vk
/// profile and its own vkBasalt config. Empty for Steam games (Steam starts those).
pub fn launch_env_at(p: &Paths, id: &str, g: &Game) -> Vec<(String, String)> {
    let key = Key::of(id);
    if matches!(key, Key::Steam(_)) {
        return Vec::new();
    }
    let mut env = vec![("RARE_FX".to_string(), key.tag())];
    if g.framegen.is_some() {
        env.push(("LSFGVK_PROFILE".into(), key.profile()));
    }
    if g.filters.is_some_and(|f| f.on()) {
        env.push(("VKBASALT_CONFIG_FILE".into(), p.own().join(format!("{}.conf", key.tag())).to_string_lossy().into_owned()));
    }
    env
}

pub fn launch_env(id: &str, g: &Game) -> Vec<(String, String)> {
    launch_env_at(&Paths::system(), id, g)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(root: &Path) -> Paths {
        Paths {
            data: root.join("data"),
            config: root.join("config"),
            lsfg: vec![root.join("missing"), root.join("lsfg")],
            layers: vec![root.join("sys-layers")],
            steam: vec![root.join("steam")],
        }
    }

    /// lsfg-vk's release tree and vkBasalt's package manifest, as on the device.
    fn install(root: &Path) {
        let l = root.join("lsfg/share/vulkan/implicit_layer.d");
        fs::create_dir_all(&l).unwrap();
        fs::create_dir_all(root.join("lsfg/lib")).unwrap();
        fs::write(root.join("lsfg/lib/liblsfg-vk-layer.so"), b"x").unwrap();
        fs::write(root.join("lsfg/lib/liblsfg-vk-layer.x86.so"), b"x").unwrap();
        for (f, n, lib) in [("VkLayer_LSFGVK_frame_generation.json", "VK_LAYER_LSFGVK_frame_generation", "liblsfg-vk-layer.so"), ("VkLayer_LSFGVK_frame_generation.x86.json", "VK_LAYER_LSFGVK_frame_generation_x86", "liblsfg-vk-layer.x86.so")] {
            let j = serde_json::json!({"file_format_version": "1.1.0", "layer": {"name": n, "library_path": format!("../../../lib/{lib}"), "type": "GLOBAL", "api_version": "1.4.350", "implementation_version": "2", "description": "x", "disable_environment": {"DISABLE_LSFGVK": "1"}}});
            fs::write(l.join(f), j.to_string()).unwrap();
        }
        fs::create_dir_all(root.join("sys-layers")).unwrap();
        let b = serde_json::json!({"file_format_version": "1.0.0", "layer": {"name": "VK_LAYER_VKBASALT_post_processing", "type": "GLOBAL", "library_path": "/usr/$LIB/vkbasalt/libvkbasalt.so", "api_version": "1.3.223", "implementation_version": "1", "description": "a post processing layer", "functions": {"vkGetInstanceProcAddr": "vkBasalt_GetInstanceProcAddr", "vkGetDeviceProcAddr": "vkBasalt_GetDeviceProcAddr"}, "enable_environment": {"ENABLE_VKBASALT": "1"}, "disable_environment": {"DISABLE_VKBASALT": "1"}}});
        fs::write(root.join("sys-layers/vkBasalt.json"), b.to_string()).unwrap();
    }

    fn fg(mult: u8) -> Option<FrameGen> {
        Some(FrameGen { mult, ..Default::default() })
    }

    #[test]
    fn slugs_are_plain_and_unique() {
        assert_eq!(slug("local:abc"), "local_abc");
        assert_eq!(Key::of("steam:1449850"), Key::Steam(1449850));
        assert_eq!(Key::of("gog:12"), Key::Rare("gog_12".into()));
        let a = slug("local:/home/x/Games/Some Game (2024)/game.exe");
        let b = slug("local:/home/x/Games/Some Game (2025)/game.exe");
        assert_ne!(a, b);
        assert!(a.len() <= 48 && a.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'), "{a}");
    }

    #[test]
    fn status_says_what_is_missing() {
        let t = tempfile::tempdir().unwrap();
        let p = paths(t.path());
        assert_eq!(status_at(&p), Status { framegen: FgStatus::NoLayer, filters: false, reshade: 0 });
        install(t.path());
        assert_eq!(status_at(&p).framegen, FgStatus::NoDll);
        assert!(status_at(&p).filters);
        // Lossless Scaling in a second library (an SD card), default branch first.
        let sd = t.path().join("sd");
        fs::create_dir_all(t.path().join("steam/steamapps")).unwrap();
        fs::write(t.path().join("steam/steamapps/libraryfolders.vdf"), format!("\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n", sd.display())).unwrap();
        let ls = sd.join("steamapps/common/Lossless Scaling");
        fs::create_dir_all(&ls).unwrap();
        fs::write(ls.join("Lossless.dll"), b"x").unwrap();
        assert_eq!(status_at(&p).framegen, FgStatus::OldBranch);
        fs::write(ls.join("lsfg-vk.dll"), b"x").unwrap();
        assert_eq!(status_at(&p).framegen, FgStatus::Ready);
        assert_eq!(find_dll(&p).unwrap().0, ls.join("lsfg-vk.dll"));
        fs::create_dir_all(p.reshade().join("Shaders")).unwrap();
        fs::write(p.reshade().join("Shaders/Vibrance.fx"), b"x").unwrap();
        fs::write(p.reshade().join("Shaders/ReShade.fxh"), b"x").unwrap();
        assert_eq!(status_at(&p).reshade, 1);
    }

    #[test]
    fn manifests_follow_the_settings() {
        let t = tempfile::tempdir().unwrap();
        let p = paths(t.path());
        install(t.path());
        let mut games = BTreeMap::new();
        games.insert("steam:1449850".to_string(), Game { framegen: fg(3), filters: Some(Filters { sharpen: 2, color: 1, reshade: false }) });
        // Without Lossless Scaling there is no frame generation manifest (lsfg-vk would end the game).
        sync_at(&p, &games).unwrap();
        assert!(!p.layer_dir().join("rare-fx-steam_1449850-lsfg.json").exists());
        assert!(p.layer_dir().join("rare-fx-steam_1449850-basalt.json").exists());
        let ls = t.path().join("steam/steamapps/common/Lossless Scaling");
        fs::create_dir_all(&ls).unwrap();
        fs::write(ls.join("lsfg-vk.dll"), b"x").unwrap();
        games.insert("gog:7".to_string(), Game { framegen: fg(1), filters: None });
        sync_at(&p, &games).unwrap();
        let dir = p.layer_dir();
        let lsfg: Value = serde_json::from_slice(&fs::read(dir.join("rare-fx-steam_1449850-lsfg.json")).unwrap()).unwrap();
        assert_eq!(lsfg["layer"]["name"], "VK_LAYER_LSFGVK_frame_generation_rare_steam_1449850");
        assert_eq!(lsfg["layer"]["enable_environment"], serde_json::json!({"SteamAppId": "1449850"}));
        assert_eq!(lsfg["layer"]["disable_environment"], serde_json::json!({"DISABLE_LSFGVK": "1"}));
        let lib = lsfg["layer"]["library_path"].as_str().unwrap();
        assert!(Path::new(lib).is_absolute() && lib.ends_with("lib/liblsfg-vk-layer.so"), "{lib}");
        let x86: Value = serde_json::from_slice(&fs::read(dir.join("rare-fx-steam_1449850-lsfg1.json")).unwrap()).unwrap();
        assert_eq!(x86["layer"]["name"], "VK_LAYER_LSFGVK_frame_generation_x86_rare_steam_1449850");
        let b: Value = serde_json::from_slice(&fs::read(dir.join("rare-fx-steam_1449850-basalt.json")).unwrap()).unwrap();
        assert_eq!(b["layer"]["library_path"], "/usr/$LIB/vkbasalt/libvkbasalt.so");
        assert_eq!(b["layer"]["enable_environment"], serde_json::json!({"SteamAppId": "1449850"}));
        assert_eq!(b["layer"]["functions"]["vkGetInstanceProcAddr"], "vkBasalt_GetInstanceProcAddr");
        // The game with frame generation off has no manifest, but keeps its profile at 1x.
        assert!(!dir.join("rare-fx-gog_7-lsfg.json").exists());
        let conf = fs::read_to_string(p.lsfg_conf()).unwrap();
        assert!(conf.contains("version = 2\n"), "{conf}");
        assert!(conf.contains(&format!("dll = \"{}\"\n", ls.join("lsfg-vk.dll").display())), "{conf}");
        assert!(conf.contains("name = \"rare steam_1449850\"\nactive_in = [\"1449850\"]\nmultiplier = 3\nflow_scale = 0.75\nperformance_mode = true\n"), "{conf}");
        assert!(conf.contains("name = \"rare gog_7\"\nactive_in = []\nmultiplier = 1\n"), "{conf}");
        let lut = p.own().join("vibrance-35.cube");
        assert!(fs::read_to_string(&lut).unwrap().contains("LUT_3D_SIZE 17"));
        let bc = fs::read_to_string(p.own().join("steam_1449850.conf")).unwrap();
        assert!(bc.contains("effects = cas:lut\n") && bc.contains("casSharpness = 0.60\n") && bc.contains(&format!("lutFile = {}\n", lut.display())), "{bc}");
        // Starting the Steam game makes its config vkBasalt's user config.
        assert!(starting_at(&p, "steam:1449850", games["steam:1449850"].filters).unwrap());
        assert_eq!(fs::read_to_string(p.basalt_conf()).unwrap(), bc);
        // Everything off: the manifests go, the profile stays at 1x.
        games.insert("steam:1449850".to_string(), Game { framegen: fg(1), filters: Some(Filters::default()) });
        sync_at(&p, &games).unwrap();
        assert!(fs::read_dir(&dir).unwrap().next().is_none());
        assert!(fs::read_to_string(p.lsfg_conf()).unwrap().contains("active_in = [\"1449850\"]\nmultiplier = 1\n"));
        // A game with filters off leaves vkBasalt's config alone.
        assert!(!starting_at(&p, "steam:1449850", games["steam:1449850"].filters).unwrap());
    }

    #[test]
    fn rare_started_games_get_their_env() {
        let t = tempfile::tempdir().unwrap();
        let p = paths(t.path());
        let g = Game { framegen: fg(2), filters: Some(Filters { sharpen: 1, ..Default::default() }) };
        let env = launch_env_at(&p, "gog:7", &g);
        assert_eq!(env[0], ("RARE_FX".to_string(), "gog_7".to_string()));
        assert_eq!(env[1], ("LSFGVK_PROFILE".to_string(), "rare gog_7".to_string()));
        assert_eq!(env[2].0, "VKBASALT_CONFIG_FILE");
        assert!(launch_env_at(&p, "steam:10", &g).is_empty());
        assert_eq!(launch_env_at(&p, "gog:7", &Game::default()).len(), 1);
    }

    #[test]
    fn reshade_effects_follow_the_builtins() {
        let t = tempfile::tempdir().unwrap();
        let p = paths(t.path());
        fs::create_dir_all(p.reshade().join("Shaders")).unwrap();
        fs::write(p.reshade().join("Shaders/Film Grain.fx"), b"x").unwrap();
        let c = basalt_conf(&p, "steam:1", &Filters { sharpen: 3, color: 0, reshade: true });
        assert!(c.contains("effects = cas:rs_FilmGrain\n"), "{c}");
        assert!(c.contains("rs_FilmGrain = ") && c.contains("reshadeIncludePath = "), "{c}");
    }

    #[test]
    fn vibrance_spares_grey_and_saturated() {
        assert_eq!(vibrance([0.5, 0.5, 0.5], 0.7), [0.5, 0.5, 0.5]);
        let red = vibrance([1.0, 0.0, 0.0], 0.7);
        assert_eq!(red, [1.0, 0.0, 0.0]);
        let dull = vibrance([0.6, 0.5, 0.45], 0.7);
        assert!(dull[0] > 0.6 && dull[2] < 0.45, "{dull:?}");
        let cube = vibrance_cube(0.35);
        assert_eq!(cube.lines().filter(|l| l.starts_with(|c: char| c.is_ascii_digit())).count(), 17 * 17 * 17);
        // Red fastest: the second entry is (1/16, 0, 0) unchanged-ish, never a green one.
        let second = cube.lines().filter(|l| l.starts_with(|c: char| c.is_ascii_digit())).nth(1).unwrap();
        assert!(second.ends_with(" 0.000000 0.000000"), "{second}");
    }

    /// For a loader check on a Linux box: RARE_FX_HOME=<dir> RARE_FX_LSFG=<lsfg-vk release tree>
    /// cargo test -p rare-system gamefx::tests::real_home -- --ignored; then run a Vulkan program
    /// with HOME, XDG_DATA_HOME and XDG_CONFIG_HOME in that dir and SteamAppId=123.
    #[test]
    #[ignore]
    fn real_home() {
        let home = PathBuf::from(std::env::var("RARE_FX_HOME").unwrap());
        let mut p = Paths::system();
        p.data = home.join(".local/share");
        p.config = home.join(".config");
        p.lsfg = vec![PathBuf::from(std::env::var("RARE_FX_LSFG").unwrap())];
        p.steam = vec![home.join(".local/share/Steam")];
        if let Ok(l) = std::env::var("RARE_FX_LAYERS") {
            p.layers = vec![PathBuf::from(l)];
        }
        let mut games = BTreeMap::new();
        games.insert("steam:123".to_string(), Game { framegen: fg(3), filters: Some(Filters { sharpen: 3, color: 2, reshade: false }) });
        let w = sync_at(&p, &games).unwrap();
        assert!(starting_at(&p, "steam:123", games["steam:123"].filters).unwrap());
        eprintln!("{w:#?}\n{:?}", status_at(&p));
    }

    #[test]
    fn settings_store_compact() {
        let g = FrameGen { mult: 2, flow: 50, perf: true };
        assert_eq!(serde_json::to_value(g).unwrap(), serde_json::json!({"mult": 2, "flow": 50, "perf": true}));
        let f: Filters = serde_json::from_str(r#"{"sharpen":1}"#).unwrap();
        assert_eq!(f, Filters { sharpen: 1, color: 0, reshade: false });
        assert!(!FrameGen::default().on());
    }
}
