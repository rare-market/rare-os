//! Android apps and games on Rare OS through Waydroid (Library > Android).
//!
//! - Setup (`jobs`): the container service's D-Bus `Init` with the vanilla LineageOS image (the
//!   image's /usr/share/waydroid-extra/channels.cfg makes that need no password), then F-Droid
//!   and Aurora Store from F-Droid's repo. No Google apps, no Google sign-in.
//! - Library: every app Android shows in its launcher (Waydroid writes a desktop entry per app),
//!   minus Android's own, as `android:<package>` games that start `rare-android run <package>`:
//!   cage on gamescope's X11, Waydroid's full UI with the app, until it closes or Rare quits it.
//! - Uninstall: Waydroid's `app remove` fails on its Android 13 images (its PendingIntent has no
//!   mutability flag), so the app's data is cleared over binder (`cmd package clear`, Android
//!   sees this user as its system user) and its APK folder removed while Android is stopped;
//!   Android forgets the package on its next start.
//! - Touch controls (`touch`): per game, pad buttons pressed as touches at saved spots, through a
//!   virtual touchscreen Android reads like the pads.
//!
//! The scripts the launcher runs (`rare-android.sh`, `binder-cmd.py`) are written next to the
//! state in ~/.local/share/rare/android on start, so they always match this build.

pub mod fdroid;
pub mod jobs;
pub mod touch;

use rare_library::model::{Art, Game, LaunchSpec, Source};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

const SCRIPT: &str = include_str!("rare-android.sh");
const BINDER: &str = include_str!("binder-cmd.py");

/// The app stores Rare installs, in the order they show.
pub const STORES: [(&str, &str); 2] = [("org.fdroid.fdroid", "F-Droid"), ("com.aurora.store", "Aurora Store")];

/// Waydroid's state folder (root's) and the per-user one (Android's /data).
const WORK: &str = "/var/lib/waydroid";

static VER: AtomicU64 = AtomicU64::new(1);
static WAKE: Mutex<Option<Box<dyn Fn() + Send>>> = Mutex::new(None);

/// Moves on every change Rare should redraw for (a job's progress, setup, an app in or out).
pub fn ver() -> u64 {
    VER.load(Ordering::Relaxed)
}

/// What runs on every change (the launcher's frame wake-up).
pub fn on_change(f: impl Fn() + Send + 'static) {
    *WAKE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(f));
}

pub(crate) fn bump() {
    VER.fetch_add(1, Ordering::Relaxed);
    if let Some(f) = WAKE.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        f();
    }
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

/// ~/.local/share/rare/android: the scripts, the setup mark, downloads, saved frames.
pub fn dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| home().join(".local/share")).join("rare/android")
}

/// Android's /data as Waydroid keeps it for this user.
fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| home().join(".local/share")).join("waydroid/data")
}

fn apps_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| home().join(".local/share")).join("applications")
}

/// The runner script's path.
pub fn runner() -> PathBuf {
    dir().join("rare-android")
}

/// Writes the scripts when they differ from this build's. Cheap; call on start and before a job.
pub fn ensure_files() -> std::io::Result<()> {
    let d = dir();
    fs::create_dir_all(&d)?;
    for (name, body, exec) in [("rare-android", SCRIPT, true), ("binder-cmd.py", BINDER, false)] {
        let p = d.join(name);
        if fs::read_to_string(&p).ok().as_deref() != Some(body) {
            let tmp = d.join(format!(".{name}.tmp"));
            fs::write(&tmp, body)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&tmp, fs::Permissions::from_mode(if exec { 0o755 } else { 0o644 }))?;
            }
            #[cfg(not(unix))]
            let _ = exec;
            fs::rename(&tmp, &p)?;
        }
    }
    Ok(())
}

/// The size Android's screen runs at ("1920x1200"): the launcher's window, so the app fills it.
pub fn set_screen(w: u32, h: u32) {
    if w < 320 || h < 240 {
        return;
    }
    let s = format!("{w}x{h}");
    let p = dir().join("size");
    if fs::read_to_string(&p).ok().as_deref() != Some(s.as_str()) {
        let _ = fs::create_dir_all(dir());
        let _ = fs::write(p, s);
    }
}

/// The screen size `set_screen` saved, else the SuiPlay's.
pub fn screen() -> (u32, u32) {
    fs::read_to_string(dir().join("size"))
        .ok()
        .and_then(|s| {
            let (w, h) = s.trim().split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((1920, 1200))
}

/// Where Android stands on this device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// No Waydroid or no binder in the kernel (not Rare OS).
    Unavailable,
    /// Waydroid is there but this Rare OS can't set it up without a password (no image channels
    /// yet): the next Rare OS update brings them.
    NeedsUpdate,
    /// Not set up yet (or the stores are not in yet).
    NotSetUp,
    Ready,
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file())).unwrap_or(false)
}

/// Android's images are on the device (Waydroid is initialised).
pub fn initialized() -> bool {
    let cfg = fs::read_to_string(Path::new(WORK).join("waydroid.cfg")).unwrap_or_default();
    let images = cfg.lines().find_map(|l| l.trim().strip_prefix("images_path").map(|v| v.trim_start_matches([' ', '=']).trim().to_string())).unwrap_or_else(|| format!("{WORK}/images"));
    Path::new(&images).join("system.img").exists() && Path::new(&images).join("vendor.img").exists()
}

/// The image channels Waydroid's password-free setup needs.
pub fn has_channels() -> bool {
    ["/usr/share/waydroid-extra/channels.cfg", "/etc/waydroid-extra/channels.cfg"].iter().any(|p| fs::read_to_string(p).map(|s| s.contains("system_channel") && s.contains("https://")).unwrap_or(false))
}

fn binder_ok() -> bool {
    fs::read_to_string("/proc/filesystems").map(|s| s.lines().any(|l| l.trim_end().ends_with("binder"))).unwrap_or(false)
}

/// Setup finished (images, then the stores).
fn marked() -> bool {
    dir().join("ready").exists()
}

pub(crate) fn mark_ready() {
    let _ = fs::create_dir_all(dir());
    let _ = fs::write(dir().join("ready"), "1\n");
}

/// RARE_ANDROID_FAKE for headless shots: ready, many, notsetup, needsupdate, setup (a setup job
/// half way), install (an install job).
pub fn fake() -> Option<String> {
    std::env::var("RARE_ANDROID_FAKE").ok().filter(|v| !v.is_empty())
}

pub fn status() -> Status {
    match fake().as_deref() {
        Some("notsetup" | "setup") => return Status::NotSetUp,
        Some("needsupdate") => return Status::NeedsUpdate,
        Some(_) => return Status::Ready,
        None => {}
    }
    if !cfg!(target_os = "linux") || !on_path("waydroid") || !binder_ok() {
        return Status::Unavailable;
    }
    if initialized() {
        return if marked() { Status::Ready } else { Status::NotSetUp };
    }
    if has_channels() {
        Status::NotSetUp
    } else {
        Status::NeedsUpdate
    }
}

/// A library id's Android package.
pub fn package_of(id: &str) -> Option<&str> {
    id.strip_prefix("android:")
}

/// An app store Rare put in (they show under Android only, not in All or on Home).
pub fn is_store(id: &str) -> bool {
    package_of(id).is_some_and(|p| STORES.iter().any(|(s, _)| *s == p))
}

/// Android's own apps (Settings, Files, the camera and the rest): never library entries.
fn system_package(p: &str) -> bool {
    p.starts_with("com.android.") || p.starts_with("org.lineageos.") || p.starts_with("android.") || p == "com.google.android.webview"
}

/// Waydroid's desktop entries: (package, name, icon). Waydroid writes one per launcher app while
/// Android runs and removes it when the app goes.
pub fn entries() -> Vec<(String, String, Option<String>)> {
    entries_in(&apps_dir())
}

fn entries_in(dir: &Path) -> Vec<(String, String, Option<String>)> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(pkg) = name.strip_prefix("waydroid.").and_then(|n| n.strip_suffix(".desktop")) else { continue };
        if system_package(pkg) || !pkg.contains('.') {
            continue;
        }
        let Ok(text) = fs::read_to_string(e.path()) else { continue };
        let kv = rare_library::apps::parse_entry(&text);
        let title = kv.get("Name").cloned().unwrap_or_else(|| pkg.to_string());
        let icon = kv.get("Icon").filter(|i| Path::new(i.as_str()).is_file()).cloned();
        out.push((pkg.to_string(), title, icon));
    }
    out.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    out
}

/// The installed apps as library games (`android:<package>`), stores first.
pub fn games() -> Vec<Game> {
    if status() != Status::Ready {
        return Vec::new();
    }
    let played = played();
    let mut v: Vec<Game> = entries()
        .into_iter()
        .map(|(pkg, title, icon)| {
            let store = STORES.iter().any(|(s, _)| *s == pkg);
            Game {
                id: format!("android:{pkg}"),
                source: Source::Android,
                title,
                installed: true,
                install_dir: None,
                size_bytes: app_size(&pkg),
                last_played: played.get(&pkg).copied(),
                playtime_minutes: None,
                art: Art { icon: icon.into_iter().collect(), ..Default::default() },
                launch: LaunchSpec::Exec { program: runner().to_string_lossy().into_owned(), args: vec!["run".into(), pkg.clone()], cwd: None, windows: false },
                origin: if store { "Android app store".into() } else { "Android".into() },
                foreign: false,
            }
        })
        .collect();
    v.sort_by_key(|g| !is_store(&g.id));
    v
}

/// When each app was last started from Rare (unix seconds).
fn played() -> BTreeMap<String, u64> {
    fs::read_to_string(dir().join("played.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Notes a start (the library's Recently played).
pub fn note_played(id: &str) {
    let Some(pkg) = package_of(id) else { return };
    let mut m = played();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    m.insert(pkg.to_string(), now);
    let _ = fs::create_dir_all(dir());
    let _ = fs::write(dir().join("played.json"), serde_json::to_string(&m).unwrap_or_default());
}

fn tree_size(p: &Path, depth: u32) -> u64 {
    let Ok(md) = fs::symlink_metadata(p) else { return 0 };
    if md.is_file() {
        return md.len();
    }
    if !md.is_dir() || depth > 24 {
        return 0;
    }
    fs::read_dir(p).map(|rd| rd.flatten().map(|e| tree_size(&e.path(), depth + 1)).sum()).unwrap_or(0)
}

/// The folders holding an app's APK (/data/app/~~<random>/<package>-<random>).
pub fn apk_dirs(pkg: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(data_dir().join("app")) else { return out };
    let prefix = format!("{pkg}-");
    for outer in rd.flatten() {
        let p = outer.path();
        if p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&prefix)) {
            out.push(p);
            continue;
        }
        if let Ok(inner) = fs::read_dir(&p) {
            out.extend(inner.flatten().map(|e| e.path()).filter(|q| q.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&prefix))));
        }
    }
    out
}

/// An app's size on disk (its APK and libraries; its data is Android's and not readable here).
pub fn app_size(pkg: &str) -> Option<u64> {
    let n: u64 = apk_dirs(pkg).iter().map(|d| tree_size(d, 0)).sum();
    (n > 0).then_some(n)
}

/// Android's total on this device: its system images plus the readable part of its data.
pub fn total_size() -> u64 {
    let images = tree_size(&Path::new(WORK).join("images"), 0);
    let data = dirs::data_dir().map(|d| tree_size(&d.join("waydroid"), 0)).unwrap_or(0);
    images + data
}

/// Removes what the host keeps for an app (its desktop entry and icon). Android's side is the job's.
pub(crate) fn forget_entry(pkg: &str) {
    let _ = fs::remove_file(apps_dir().join(format!("waydroid.{pkg}.desktop")));
    let _ = fs::remove_file(dirs::data_dir().unwrap_or_default().join(format!("waydroid/data/icons/{pkg}.png")));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_skip_android_own_apps() {
        let d = tempfile::tempdir().unwrap();
        let w = |name: &str, body: &str| fs::write(d.path().join(name), body).unwrap();
        w("waydroid.com.android.settings.desktop", "[Desktop Entry]\nName=Settings\nExec=waydroid app launch com.android.settings\n");
        w("waydroid.org.lineageos.jelly.desktop", "[Desktop Entry]\nName=Browser\n");
        w("waydroid.org.fdroid.fdroid.desktop", "[Desktop Entry]\nName=F-Droid\nIcon=/nope.png\n");
        w("waydroid.org.secuso.privacyfriendly2048.desktop", "[Desktop Entry]\nName=2048\n");
        w("Waydroid.desktop", "[Desktop Entry]\nName=Waydroid\n");
        w("org.other.desktop", "[Desktop Entry]\nName=Other\n");
        let e = entries_in(d.path());
        let names: Vec<&str> = e.iter().map(|x| x.1.as_str()).collect();
        assert_eq!(names, ["2048", "F-Droid"]);
        assert_eq!(e[1].0, "org.fdroid.fdroid");
        assert!(e[1].2.is_none());
    }

    #[test]
    fn stores_and_packages() {
        assert!(is_store("android:org.fdroid.fdroid"));
        assert!(!is_store("android:org.secuso.privacyfriendly2048"));
        assert_eq!(package_of("android:a.b"), Some("a.b"));
        assert_eq!(package_of("steam:1"), None);
    }
}
