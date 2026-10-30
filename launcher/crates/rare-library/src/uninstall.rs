//! Uninstalling: what removing a game deletes, the space that frees, and the removal itself.
//!
//! Steam removes its own games (`How::Steam`: the caller sends steam://uninstall/<appid>); the
//! Rare Store and web games are removed by their owners (`How::Rare`, `How::Web`); Flatpak apps by
//! `flatpak uninstall`. Everything else is removed here: the game's folder, its entry in the
//! store's own list (legendary's and Heroic's installed.json, a Rare manifest), and its Wine prefix
//! unless the player keeps the saves in it.
//!
//! Nothing outside the library roots is ever deleted: every folder and file goes through `guard`
//! (absolute, no `..`, not a link, strictly inside a root), the way rare-store's uninstall_in
//! refuses a slug that is not one folder of its own root.

use crate::model::{Game, LaunchSpec, Source};
use serde_json::Value;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Who removes the game.
#[derive(Debug, Clone, PartialEq)]
pub enum How {
    /// Steam: steam://uninstall/<appid>. Steam deletes the files and may ask to confirm.
    Steam { appid: u32, manifest: PathBuf },
    /// Rare deletes the plan's folders and files and edits its lists.
    Files,
    /// `flatpak uninstall` from the user or the system installation.
    Flatpak { app: String, user: bool },
    /// The Rare Store (rare-store `uninstall`).
    Rare { slug: String },
    /// A web game: dropped from launcher.json by the caller.
    Web { id: String },
}

/// An entry to drop from a store's own list, so the store stops listing the game.
#[derive(Debug, Clone, PartialEq)]
pub enum ListEdit {
    /// A key of a JSON object (legendary's installed.json).
    Key { file: PathBuf, key: String },
    /// The items of the array `array` whose `field` is `value` (Heroic's GOG installed.json,
    /// its sideload library.json).
    Item { file: PathBuf, array: String, field: String, value: String },
}

/// What the saves are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavesKind {
    /// A Wine prefix: Windows games keep saves in its user folders.
    Prefix,
    /// A web game's storage.
    Web,
    /// A Flatpak app's data folder (~/.var/app/<id>).
    AppData,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Saves {
    pub dir: PathBuf,
    pub size: u64,
    /// True when it holds files the player made (a save, a setting), not only what the runtime
    /// wrote. The sheet asks whether to keep them only then.
    pub found: bool,
    pub kind: SavesKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub id: String,
    pub title: String,
    pub how: How,
    /// Folders Rare deletes (already checked by `guard`).
    pub dirs: Vec<PathBuf>,
    /// Files Rare deletes (already checked by `guard`).
    pub files: Vec<PathBuf>,
    pub lists: Vec<ListEdit>,
    /// Bytes the game itself frees (the saves come on top when they go too).
    pub size: u64,
    pub saves: Option<Saves>,
    /// Where it is installed, for the sheet.
    pub place: Option<String>,
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

fn data() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| home().join(".local/share"))
}

fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(p).ok()?).ok()
}

/// Rare's own Wine prefix for a game it runs through umu-run (rare-launch launch.rs).
pub fn rare_prefix(id: &str) -> PathBuf {
    data().join("rare/prefixes").join(id.replace(':', "-"))
}

/// A web game's storage (rare-webgame's data_dir).
pub fn web_data(id: &str) -> PathBuf {
    let safe: String = id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    data().join("rare/webgames").join(safe)
}

/// Heroic's settings: the default install folder and prefix folder, per config dir.
fn heroic_defaults() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for c in crate::heroic::config_dirs() {
        let Some(v) = read_json(&c.join("heroic/config.json")) else { continue };
        for k in ["defaultInstallPath", "defaultWinePrefix"] {
            if let Some(p) = v.pointer(&format!("/defaultSettings/{k}")).and_then(Value::as_str).filter(|p| !p.is_empty()) {
                out.push(PathBuf::from(p.replace('~', &home().to_string_lossy())));
            }
        }
    }
    out
}

/// The folders games live in. A folder is only ever deleted when it sits strictly inside one of
/// these. A root that is `/`, a home folder or one of their parents is dropped, so a Games folder
/// set to the home folder can never make the home folder's other contents deletable.
pub fn roots(extra: &[String]) -> Vec<PathBuf> {
    let h = home();
    let mut v: Vec<PathBuf> = vec![h.join("Games"), h.join("legendary"), h.join("GOG Games")];
    for o in crate::roots::other_homes() {
        v.push(o.join("Games"));
    }
    for m in crate::roots::mount_points() {
        v.push(m.join("Games"));
        v.push(m.join("games"));
    }
    v.extend(extra.iter().map(PathBuf::from));
    if let Some(c) = dirs::config_dir() {
        v.push(c.join("itch/apps"));
    }
    v.push(data().join("rare/prefixes"));
    v.push(data().join("rare/webgames"));
    v.extend(heroic_defaults());
    if let Some(e) = crate::esde::scan().filter(|e| !e.rom_dir.is_empty()) {
        v.push(PathBuf::from(e.rom_dir));
    }
    clean_roots(v, &h)
}

fn clean_roots(v: Vec<PathBuf>, home: &Path) -> Vec<PathBuf> {
    let home = fs::canonicalize(home).unwrap_or(home.to_path_buf());
    let mut out: Vec<PathBuf> = Vec::new();
    for r in v {
        let Ok(c) = fs::canonicalize(&r) else { continue };
        let top = c.parent().is_none() || home.starts_with(&c) || c.components().count() < 3;
        if !top && !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// `p` when it is safe to delete: absolute, no `.` or `..`, not a link, and strictly inside one
/// of `roots` (canonical paths). Returns the canonical path.
pub fn guard(p: &Path, roots: &[PathBuf]) -> Result<PathBuf, String> {
    let shown = p.display();
    if !p.is_absolute() || p.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        return Err(format!("{shown} isn't a game folder."));
    }
    let m = fs::symlink_metadata(p).map_err(|_| format!("{shown} isn't there anymore."))?;
    if m.file_type().is_symlink() {
        return Err(format!("{shown} is a link, not a folder. Remove it in Desktop mode."));
    }
    let c = fs::canonicalize(p).map_err(|e| format!("Couldn't read {shown}: {e}"))?;
    if roots.iter().any(|r| c != *r && c.starts_with(r)) {
        Ok(c)
    } else {
        Err(format!("Rare only deletes games in your game folders, and {shown} isn't in one."))
    }
}

/// Bytes on disk under `p` (links not followed).
pub fn size_of(p: &Path) -> u64 {
    let Ok(m) = fs::symlink_metadata(p) else { return 0 };
    if m.is_dir() {
        fs::read_dir(p).map(|rd| rd.flatten().map(|e| size_of(&e.path())).sum()).unwrap_or(0)
    } else {
        disk_bytes(&m)
    }
}

#[cfg(unix)]
fn disk_bytes(m: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    m.blocks() * 512
}

#[cfg(not(unix))]
fn disk_bytes(m: &fs::Metadata) -> u64 {
    m.len()
}

/// Folders inside a Windows user folder that the runtime fills by itself.
const NOT_SAVES: &[&str] = &["microsoft", "temp", "openxr", "vkd3d_cache", "dxvk_cache", "nvidia corporation", "desktop", "start menu", "templates", "favorites"];

fn any_file(dir: &Path, depth: usize) -> bool {
    let Ok(rd) = fs::read_dir(dir) else { return false };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_lowercase();
        let Ok(m) = fs::symlink_metadata(e.path()) else { continue };
        if m.is_dir() {
            if depth > 0 && !NOT_SAVES.contains(&name.as_str()) && any_file(&e.path(), depth - 1) {
                return true;
            }
        } else if m.is_file() && m.len() > 0 && !name.ends_with(".lnk") && name != "desktop.ini" {
            return true;
        }
    }
    false
}

/// True when a Wine prefix (or a Proton compatdata folder, which keeps it in `pfx`) has files in
/// its users' AppData, Documents or Saved Games.
pub fn prefix_has_saves(prefix: &Path) -> bool {
    let root = if prefix.join("pfx").is_dir() { prefix.join("pfx") } else { prefix.to_path_buf() };
    let Ok(rd) = fs::read_dir(root.join("drive_c/users")) else { return false };
    for u in rd.flatten() {
        let u = u.path();
        for sub in ["AppData/Roaming", "AppData/Local", "AppData/LocalLow", "Documents", "My Documents", "Saved Games", "Application Data"] {
            if any_file(&u.join(sub), 6) {
                return true;
            }
        }
    }
    false
}

fn saves_at(dir: PathBuf, kind: SavesKind, roots: &[PathBuf]) -> Option<Saves> {
    let dir = guard(&dir, roots).ok()?;
    let size = size_of(&dir);
    let found = match kind {
        SavesKind::Prefix => prefix_has_saves(&dir),
        SavesKind::Web | SavesKind::AppData => size > 0 && any_file(&dir, 8),
    };
    Some(Saves { dir, size, found, kind })
}

/// The prefix Heroic keeps for a game (GamesConfig/<app>.json winePrefix).
fn heroic_prefix(app: &str) -> Option<PathBuf> {
    for c in crate::heroic::config_dirs() {
        let Some(v) = read_json(&c.join(format!("heroic/GamesConfig/{app}.json"))) else { continue };
        if let Some(p) = v.get(app).and_then(|g| g.get("winePrefix")).and_then(Value::as_str).filter(|p| !p.is_empty()) {
            return Some(PathBuf::from(p.replace('~', &home().to_string_lossy())));
        }
    }
    None
}

/// The installed.json (legendary or Heroic's copy of it) that lists `app`.
fn legendary_list(app: &str) -> Option<PathBuf> {
    for c in crate::heroic::config_dirs() {
        for f in ["legendary/installed.json", "heroic/legendaryConfig/legendary/installed.json"] {
            let p = c.join(f);
            if read_json(&p).is_some_and(|v| v.get(app).is_some()) {
                return Some(p);
            }
        }
    }
    None
}

fn heroic_list(rel: &str, array: &str, field: &str, app: &str) -> Option<ListEdit> {
    for c in crate::heroic::config_dirs() {
        let p = c.join(rel);
        let listed = read_json(&p).and_then(|v| v.get(array).and_then(Value::as_array).map(|a| a.iter().any(|g| g.get(field).and_then(Value::as_str) == Some(app))));
        if listed == Some(true) {
            return Some(ListEdit::Item { file: p, array: array.into(), field: field.into(), value: app.into() });
        }
    }
    None
}

/// The Flatpak an app entry runs (`flatpak run [--opts] <app id>`).
pub fn flatpak_of(spec: &LaunchSpec) -> Option<String> {
    let LaunchSpec::Exec { program, args, .. } = spec else { return None };
    if Path::new(program).file_name().and_then(|n| n.to_str()) != Some("flatpak") {
        return None;
    }
    let run = args.iter().position(|a| a == "run")?;
    args[run + 1..].iter().find(|a| !a.starts_with('-')).cloned()
}

fn flatpak_dir(app: &str) -> Option<(PathBuf, bool)> {
    let user = data().join("flatpak/app").join(app);
    if user.is_dir() {
        return Some((user, true));
    }
    let system = Path::new("/var/lib/flatpak/app").join(app);
    system.is_dir().then_some((system, false))
}

fn pretty(p: &Path) -> String {
    let s = p.to_string_lossy().into_owned();
    let h = home().to_string_lossy().into_owned();
    match s.strip_prefix(&h) {
        Some(rest) if !h.is_empty() => format!("~{rest}"),
        _ => s,
    }
}

/// What uninstalling `g` does. `extra` is launcher.json's gameFolders. Errors are the sentence
/// the sheet shows when Rare can't remove it.
pub fn plan(g: &Game, extra: &[String]) -> Result<Plan, String> {
    plan_in(g, &roots(extra))
}

pub fn plan_in(g: &Game, roots: &[PathBuf]) -> Result<Plan, String> {
    let mut p = Plan { id: g.id.clone(), title: g.title.clone(), how: How::Files, dirs: vec![], files: vec![], lists: vec![], size: 0, saves: None, place: None };
    let dir = g.install_dir.as_ref().map(PathBuf::from);
    let native = g.id.split_once(':').map(|x| x.1).unwrap_or(&g.id).to_string();
    // The game's own folder, checked, with its size.
    let own_dir = |p: &mut Plan, roots: &[PathBuf]| -> Result<(), String> {
        let d = dir.clone().ok_or("Rare doesn't know where this game is installed.")?;
        let c = guard(&d, roots)?;
        p.size += size_of(&c);
        p.place = Some(pretty(&c));
        p.dirs.push(c);
        Ok(())
    };
    match (&g.source, &g.launch) {
        (_, LaunchSpec::SteamShortcut { .. }) => return Err("This is a shortcut in Steam. Remove it in Steam.".into()),
        (Source::Steam, LaunchSpec::Steam { appid }) => {
            let d = dir.clone().ok_or("Steam doesn't say where this game is installed.")?;
            let steamapps = d.parent().and_then(Path::parent).ok_or("Steam doesn't say where this game is installed.")?.to_path_buf();
            let manifest = steamapps.join(format!("appmanifest_{appid}.acf"));
            p.place = Some(pretty(&d));
            if g.foreign {
                // A library this Steam doesn't know: Steam can't remove it, so Rare does.
                let lib_roots = [fs::canonicalize(&steamapps).map_err(|e| format!("Couldn't read {}: {e}", steamapps.display()))?];
                own_dir(&mut p, &lib_roots)?;
                p.files.push(guard(&manifest, &lib_roots)?);
                p.saves = saves_at(steamapps.join("compatdata").join(appid.to_string()), SavesKind::Prefix, &lib_roots);
            } else {
                p.size = g.size_bytes.unwrap_or(0);
                p.how = How::Steam { appid: *appid, manifest };
            }
        }
        (Source::Rare, _) => {
            p.how = How::Rare { slug: native.clone() };
            if let Some(d) = &dir {
                p.size = size_of(d);
                p.place = Some(pretty(d));
            }
            if matches!(g.launch, LaunchSpec::Exec { windows: true, .. }) {
                p.saves = saves_at(rare_prefix(&g.id), SavesKind::Prefix, roots);
            }
        }
        (Source::Web, _) => {
            p.how = How::Web { id: native.clone() };
            p.saves = saves_at(web_data(&g.id), SavesKind::Web, roots);
        }
        (Source::Apps, spec) => {
            let app = flatpak_of(spec).ok_or("Rare can only uninstall Flatpak apps. Remove this one in Desktop mode.")?;
            let (d, user) = flatpak_dir(&app).ok_or("This Flatpak isn't installed anymore.")?;
            p.size = size_of(&d);
            p.place = Some(if user { "Installed for you".into() } else { "Installed for everyone".into() });
            // ~/.var/app/<id>: the app's settings and saves; flatpak deletes it with --delete-data.
            let data_dir = home().join(".var/app").join(&app);
            if data_dir.is_dir() {
                let size = size_of(&data_dir);
                p.saves = Some(Saves { found: size > 0, dir: data_dir, size, kind: SavesKind::AppData });
            }
            p.how = How::Flatpak { app, user };
        }
        (Source::Epic, spec) => {
            own_dir(&mut p, roots)?;
            if let Some(f) = legendary_list(&native) {
                p.lists.push(ListEdit::Key { file: f, key: native.clone() });
            }
            p.saves = match spec {
                LaunchSpec::Heroic { .. } => heroic_prefix(&native).and_then(|d| saves_at(d, SavesKind::Prefix, roots)),
                // legendary through umu-run: the game's prefix (Heroic's when it came from Heroic).
                _ => saves_at(crate::umu::prefix(&g.id), SavesKind::Prefix, roots),
            };
        }
        (Source::Gog, spec) => {
            own_dir(&mut p, roots)?;
            p.lists.extend(heroic_list("heroic/gog_store/installed.json", "installed", "appName", &native));
            p.saves = match spec {
                LaunchSpec::Heroic { .. } => heroic_prefix(&native).and_then(|d| saves_at(d, SavesKind::Prefix, roots)),
                LaunchSpec::Exec { windows: true, .. } => saves_at(crate::umu::prefix(&g.id), SavesKind::Prefix, roots),
                _ => None,
            };
        }
        (Source::Local, LaunchSpec::Heroic { .. }) => {
            // Added to Heroic by hand.
            own_dir(&mut p, roots)?;
            p.lists.extend(heroic_list("heroic/sideload_apps/library.json", "games", "app_name", &native));
            p.saves = heroic_prefix(&native).and_then(|d| saves_at(d, SavesKind::Prefix, roots));
        }
        (Source::Local | Source::Itch, spec) => {
            if g.origin == "Added by you" {
                // A manifest: it goes, and the folder only when it is in a game folder.
                let LaunchSpec::Exec { program, .. } = spec else { return Err("Rare can't remove this one.".into()) };
                let m = manifest_for(program).ok_or("Rare can't find the file that adds this game.")?;
                if !m.starts_with(data()) {
                    return Err("This game comes with Rare OS.".into());
                }
                p.files.push(m);
                if let Some(d) = dir.clone().and_then(|d| guard(&d, roots).ok()) {
                    p.size += size_of(&d);
                    p.place = Some(pretty(&d));
                    p.dirs.push(d);
                }
            } else {
                own_dir(&mut p, roots)?;
            }
            if matches!(spec, LaunchSpec::Exec { windows: true, .. }) {
                p.saves = saves_at(rare_prefix(&g.id), SavesKind::Prefix, roots);
            }
        }
        _ => return Err("Rare can't remove this one.".into()),
    }
    Ok(p)
}

/// The hand-added manifest whose `exec` is `program`.
fn manifest_for(program: &str) -> Option<PathBuf> {
    for dir in crate::local::manifest_dirs() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            if e.path().extension().and_then(|x| x.to_str()) == Some("json") && read_json(&e.path()).is_some_and(|v| v.get("exec").and_then(Value::as_str) == Some(program)) {
                return Some(e.path());
            }
        }
    }
    None
}

/// A ROM in ES-DE's folder: `rel` is the gamelist path inside the system's folder.
pub fn plan_rom(rom_dir: &Path, system: &str, rel: &str, name: &str) -> Result<Plan, String> {
    let sys = rom_dir.join(system);
    let root = [fs::canonicalize(&sys).map_err(|_| "This system's ROM folder isn't there anymore.".to_string())?];
    let rel = rel.trim_start_matches("./");
    let target = guard(&sys.join(rel), &root)?;
    let size = size_of(&target);
    let mut p = Plan { id: format!("rom:{system}/{rel}"), title: name.to_string(), how: How::Files, dirs: vec![], files: vec![], lists: vec![], size, saves: None, place: Some(pretty(&target)) };
    if target.is_dir() {
        p.dirs.push(target);
    } else {
        p.files.push(target);
    }
    Ok(p)
}

/// Deletes `p` (folder or file, links removed as links), adding each file's bytes to `done` and
/// calling `tick` as it goes.
pub fn remove_tree(p: &Path, done: &mut u64, tick: &mut dyn FnMut(u64)) -> std::io::Result<()> {
    let m = fs::symlink_metadata(p)?;
    if m.is_dir() {
        for e in fs::read_dir(p)? {
            remove_tree(&e?.path(), done, tick)?;
        }
        fs::remove_dir(p)
    } else {
        fs::remove_file(p)?;
        *done += disk_bytes(&m);
        tick(*done);
        Ok(())
    }
}

fn write_json(file: &Path, v: &Value) -> Result<(), String> {
    let tmp = file.with_extension("json.rare-tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(v).unwrap_or_default()).and_then(|_| fs::rename(&tmp, file)).map_err(|e| format!("Couldn't update {}: {e}", file.display()))
}

fn apply_list(edit: &ListEdit) -> Result<(), String> {
    match edit {
        ListEdit::Key { file, key } => {
            let Some(mut v) = read_json(file) else { return Ok(()) };
            if v.as_object_mut().and_then(|o| o.remove(key)).is_some() {
                write_json(file, &v)?;
            }
        }
        ListEdit::Item { file, array, field, value } => {
            let Some(mut v) = read_json(file) else { return Ok(()) };
            if let Some(a) = v.get_mut(array).and_then(Value::as_array_mut) {
                let n = a.len();
                a.retain(|g| g.get(field).and_then(Value::as_str) != Some(value.as_str()));
                if a.len() != n {
                    write_json(file, &v)?;
                }
            }
        }
    }
    Ok(())
}

/// Bytes `remove` deletes with this choice.
pub fn total(p: &Plan, keep_saves: bool) -> u64 {
    p.size + p.saves.as_ref().filter(|_| !keep_saves).map(|s| s.size).unwrap_or(0)
}

/// Removes what the plan says Rare removes (`How::Files`, and the saves folder for every kind
/// unless kept). `tick(done, total)` follows the bytes. Steam, Flatpak, the Rare Store and web
/// games are the caller's (it runs `flatpak_uninstall` or the owner's call first, then this for
/// the saves).
pub fn remove(p: &Plan, keep_saves: bool, tick: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
    let all = total(p, keep_saves).max(1);
    let mut done = 0u64;
    let mut t = |d: u64| tick(d.min(all), all);
    if p.how == How::Files {
        for d in &p.dirs {
            remove_tree(d, &mut done, &mut t).map_err(|e| format!("Couldn't delete {}: {e}", pretty(d)))?;
        }
        for f in &p.files {
            if fs::symlink_metadata(f).is_ok() {
                remove_tree(f, &mut done, &mut t).map_err(|e| format!("Couldn't delete {}: {e}", pretty(f)))?;
            }
        }
        for l in &p.lists {
            apply_list(l)?;
        }
    } else {
        done = p.size;
        t(done);
    }
    if let Some(s) = p.saves.as_ref().filter(|_| !keep_saves) {
        if s.kind != SavesKind::AppData && fs::symlink_metadata(&s.dir).is_ok() {
            remove_tree(&s.dir, &mut done, &mut t).map_err(|e| format!("Couldn't delete {}: {e}", pretty(&s.dir)))?;
        }
    }
    tick(all, all);
    Ok(())
}

/// `flatpak uninstall` for `How::Flatpak`; `delete_data` also removes ~/.var/app/<id>.
pub fn flatpak_uninstall(app: &str, user: bool, delete_data: bool) -> Result<(), String> {
    let mut args = vec!["uninstall", if user { "--user" } else { "--system" }, "-y", "--noninteractive"];
    if delete_data {
        args.push("--delete-data");
    }
    args.push(app);
    let out = std::process::Command::new("flatpak").args(&args).output().map_err(|e| format!("Couldn't run flatpak: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !user && (err.contains("uthoriz") || err.contains("ermission") || err.contains("polkit")) {
        return Err("Flatpak needs a password to remove system apps. Remove it in Desktop mode.".into());
    }
    Err(if err.is_empty() { "Flatpak couldn't remove it.".into() } else { format!("Flatpak couldn't remove it: {}", err.lines().last().unwrap_or_default()) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Art;

    fn game(id: &str, source: Source, dir: Option<&Path>, launch: LaunchSpec) -> Game {
        Game {
            id: id.into(),
            source,
            title: "Test".into(),
            installed: true,
            install_dir: dir.map(|d| d.to_string_lossy().into_owned()),
            size_bytes: Some(5),
            last_played: None,
            playtime_minutes: None,
            art: Art::default(),
            launch,
            origin: "Games folder".into(),
            foreign: false,
        }
    }

    fn exec(windows: bool) -> LaunchSpec {
        LaunchSpec::Exec { program: "/x/run".into(), args: vec![], cwd: None, windows }
    }

    #[test]
    fn guard_keeps_to_the_roots() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("Games");
        let game = root.join("Celeste");
        fs::create_dir_all(game.join("data")).unwrap();
        fs::create_dir_all(t.path().join("Other")).unwrap();
        let roots = clean_roots(vec![root.clone()], Path::new("/nonexistent-home"));
        assert!(guard(&game, &roots).is_ok());
        assert!(guard(&root, &roots).is_err(), "never the root itself");
        assert!(guard(&t.path().join("Other"), &roots).is_err());
        assert!(guard(&root.join("Celeste/../../Other"), &roots).is_err(), "no ..");
        assert!(guard(Path::new("Games/Celeste"), &roots).is_err(), "absolute only");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(t.path().join("Other"), root.join("Link")).unwrap();
            assert!(guard(&root.join("Link"), &roots).is_err(), "no links");
        }
    }

    #[test]
    fn roots_never_include_home_or_its_parents() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home/me");
        fs::create_dir_all(home.join("Games")).unwrap();
        let roots = clean_roots(vec![home.clone(), home.parent().unwrap().to_path_buf(), PathBuf::from("/"), home.join("Games")], &home);
        assert_eq!(roots, vec![fs::canonicalize(home.join("Games")).unwrap()]);
    }

    #[test]
    fn removes_a_local_game_and_its_prefix_unless_kept() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("Games");
        let dir = root.join("Hades");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Hades.exe"), vec![1u8; 9000]).unwrap();
        let roots = clean_roots(vec![root.clone()], Path::new("/nonexistent-home"));
        let g = game("local:1", Source::Local, Some(&dir), exec(false));
        let p = plan_in(&g, &roots).unwrap();
        assert_eq!(p.how, How::Files);
        assert_eq!(p.dirs, vec![fs::canonicalize(&dir).unwrap()]);
        assert!(p.size > 0);
        let mut last = (0, 0);
        remove(&p, false, &mut |d, a| last = (d, a)).unwrap();
        assert!(!dir.exists() && root.exists());
        assert_eq!(last.0, last.1);
        // A folder outside the roots is refused.
        let out = t.path().join("Elsewhere");
        fs::create_dir_all(&out).unwrap();
        assert!(plan_in(&game("local:2", Source::Local, Some(&out), exec(false)), &roots).is_err());
    }

    #[test]
    fn finds_saves_in_a_prefix() {
        let t = tempfile::tempdir().unwrap();
        let pfx = t.path().join("pfx-game");
        let user = pfx.join("drive_c/users/steamuser");
        fs::create_dir_all(user.join("AppData/Roaming/Microsoft/Windows")).unwrap();
        fs::write(user.join("AppData/Roaming/Microsoft/Windows/x.ini"), "a").unwrap();
        assert!(!prefix_has_saves(&pfx), "the runtime's own files are not saves");
        fs::create_dir_all(user.join("AppData/LocalLow/Team Cherry/Hollow Knight")).unwrap();
        fs::write(user.join("AppData/LocalLow/Team Cherry/Hollow Knight/user1.dat"), "save").unwrap();
        assert!(prefix_has_saves(&pfx));
    }

    #[test]
    fn edits_store_lists() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("installed.json");
        fs::write(&f, r#"{"Fortnite":{"title":"F"},"Celeste":{"title":"C"}}"#).unwrap();
        apply_list(&ListEdit::Key { file: f.clone(), key: "Celeste".into() }).unwrap();
        let v = read_json(&f).unwrap();
        assert!(v.get("Celeste").is_none() && v.get("Fortnite").is_some());
        let g = t.path().join("gog.json");
        fs::write(&g, r#"{"installed":[{"appName":"1"},{"appName":"2"}]}"#).unwrap();
        apply_list(&ListEdit::Item { file: g.clone(), array: "installed".into(), field: "appName".into(), value: "1".into() }).unwrap();
        assert_eq!(read_json(&g).unwrap()["installed"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn steam_and_shortcuts() {
        let g = game("steam:620", Source::Steam, Some(Path::new("/lib/steamapps/common/Portal 2")), LaunchSpec::Steam { appid: 620 });
        let p = plan_in(&g, &[]).unwrap();
        assert_eq!(p.how, How::Steam { appid: 620, manifest: PathBuf::from("/lib/steamapps/appmanifest_620.acf") });
        assert_eq!(p.size, 5);
        assert!(p.dirs.is_empty() && p.files.is_empty());
        let s = game("shortcut:1", Source::Local, None, LaunchSpec::SteamShortcut { gameid: 1 });
        assert!(plan_in(&s, &[]).is_err());
    }

    #[test]
    fn reads_flatpak_ids() {
        let spec = LaunchSpec::Exec { program: "/usr/bin/flatpak".into(), args: vec!["run".into(), "--branch=stable".into(), "--command=xemu".into(), "app.xemu.xemu".into()], cwd: None, windows: false };
        assert_eq!(flatpak_of(&spec).as_deref(), Some("app.xemu.xemu"));
        assert_eq!(flatpak_of(&exec(false)), None);
    }

    #[test]
    fn roms_stay_in_their_system() {
        let t = tempfile::tempdir().unwrap();
        let sys = t.path().join("roms/gc");
        fs::create_dir_all(&sys).unwrap();
        fs::write(sys.join("Game.iso"), vec![0u8; 5000]).unwrap();
        let p = plan_rom(&t.path().join("roms"), "gc", "./Game.iso", "Game").unwrap();
        assert_eq!(p.files.len(), 1);
        assert!(plan_rom(&t.path().join("roms"), "gc", "../gc", "x").is_err());
        remove(&p, true, &mut |_, _| {}).unwrap();
        assert!(!sys.join("Game.iso").exists() && sys.exists());
    }
}
