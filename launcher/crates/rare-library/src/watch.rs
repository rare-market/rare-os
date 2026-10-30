//! Watches the places games come from and tells the UI (`library-changed`) when something
//! changed, so installs, uninstalls, new shortcuts and new emulators show up without a manual
//! rescan. Polls a cheap signature (mtime + entry count) every few seconds.

use super::{apps, shortcuts, steam};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};
use rare_core::{Ctx, Event};

const EVERY: Duration = Duration::from_secs(4);

fn stamp(p: &Path, h: &mut DefaultHasher) {
    p.hash(h);
    let Ok(m) = fs::metadata(p) else {
        0u8.hash(h);
        return;
    };
    m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos()).hash(h);
    if m.is_dir() {
        fs::read_dir(p).map(|rd| rd.count()).unwrap_or(0).hash(h);
    } else {
        m.len().hash(h);
    }
}

/// Every path whose change can change the library.
pub fn watched_paths(game_folders: &[String]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let roots = steam::find_roots();
    for r in &roots {
        out.push(r.path.join("steamapps/libraryfolders.vdf"));
        out.push(r.path.join("config/libraryfolders.vdf"));
        for lib in steam::registered_libraries(&r.path) {
            out.push(lib.join("steamapps"));
        }
    }
    for lib in steam::libraries_on_mounts() {
        out.push(lib.join("steamapps"));
    }
    for c in shortcuts::user_config_dirs(&roots) {
        out.push(c.join("shortcuts.vdf"));
    }
    out.extend(apps::entry_dirs());
    if let Some(h) = dirs::home_dir() {
        out.push(h.join("Games"));
    }
    out.extend(game_folders.iter().map(PathBuf::from));
    out.sort();
    out.dedup();
    out
}

pub fn signature(game_folders: &[String]) -> u64 {
    let mut h = DefaultHasher::new();
    for p in watched_paths(game_folders) {
        stamp(&p, &mut h);
    }
    h.finish()
}

/// Starts the watcher thread: sends `Event::LibraryChanged` when something changed. `folders`
/// returns the configured extra game folders.
pub fn start<F>(ctx: Ctx, folders: F)
where
    F: Fn() -> Vec<String> + Send + 'static,
{
    std::thread::spawn(move || {
        let mut last: Option<u64> = None;
        loop {
            let sig = signature(&folders());
            if last.is_some_and(|l| l != sig) {
                ctx.send(Event::LibraryChanged);
            }
            last = Some(sig);
            std::thread::sleep(EVERY);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_changes_with_folder_contents() {
        let d = std::env::temp_dir().join(format!("rare-watch-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let folders = vec![d.to_string_lossy().into_owned()];
        let a = signature(&folders);
        assert_eq!(a, signature(&folders));
        fs::create_dir(d.join("New Game")).unwrap();
        assert_ne!(a, signature(&folders));
        let _ = fs::remove_dir_all(&d);
    }
}
