//! The last library scan, kept on disk so the launcher shows the library the moment it opens
//! and rescans in the background. ~/.cache/rare/library.json.

use crate::model::Library;
use std::fs;
use std::path::PathBuf;

/// Bumped when the shape of `Library` changes, so an old cache is ignored.
const VERSION: u32 = 2;

fn path() -> Option<PathBuf> {
    Some(rare_core::paths::cache_dir().join("library.json"))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Stored {
    version: u32,
    library: Library,
}

pub fn load() -> Option<Library> {
    let s: Stored = serde_json::from_slice(&fs::read(path()?).ok()?).ok()?;
    (s.version == VERSION).then_some(s.library)
}

pub fn save(lib: &Library) {
    let Some(p) = path() else { return };
    if let Some(d) = p.parent() {
        let _ = fs::create_dir_all(d);
    }
    let tmp = p.with_extension("json.tmp");
    if let Ok(bytes) = serde_json::to_vec(&Stored { version: VERSION, library: lib.clone() }) {
        if fs::write(&tmp, bytes).is_ok() {
            let _ = fs::rename(tmp, p);
        }
    }
}
