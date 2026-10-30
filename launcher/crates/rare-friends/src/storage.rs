//! Small key-value storage the host chooses: where the device keys live (localStorage in the web
//! launcher) and where the last friends overview is kept for the next start. A missing or broken
//! store only means friends asks to be turned on again, so every method is infallible.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub trait Storage: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    /// Writes a value; None removes it.
    fn set(&self, key: &str, value: Option<&str>);
}

/// In memory only (tests, and a session that must not leave a key behind).
#[derive(Default)]
pub struct MemoryStorage(Mutex<HashMap<String, String>>);

impl MemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Storage for MemoryStorage {
    fn get(&self, key: &str) -> Option<String> {
        self.0.lock().unwrap().get(key).cloned()
    }
    fn set(&self, key: &str, value: Option<&str>) {
        let mut g = self.0.lock().unwrap();
        match value {
            Some(v) => g.insert(key.to_string(), v.to_string()),
            None => g.remove(key),
        };
    }
}

/// One file per key under a directory, readable by this user only (the device key's secret is
/// in there). Writes go to a temporary file first so a crash never leaves half a key.
pub struct FileStorage {
    dir: PathBuf,
}

impl FileStorage {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        FileStorage { dir: dir.into() }
    }

    /// `~/.local/share/rare/friends`.
    pub fn default_dir() -> PathBuf {
        rare_core::paths::data_dir().join("friends")
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn file(&self, key: &str) -> PathBuf {
        let name: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' }).collect();
        self.dir.join(name)
    }
}

impl Storage for FileStorage {
    fn get(&self, key: &str) -> Option<String> {
        std::fs::read_to_string(self.file(key)).ok()
    }

    fn set(&self, key: &str, value: Option<&str>) {
        let path = self.file(key);
        let Some(v) = value else {
            let _ = std::fs::remove_file(path);
            return;
        };
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let tmp = path.with_extension("tmp");
        if write_private(&tmp, v.as_bytes()).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_round_trip_and_stay_private() {
        let dir = std::env::temp_dir().join(format!("rare-friends-storage-{}", std::process::id()));
        let s = FileStorage::new(&dir);
        assert_eq!(s.get("rare.friends.key.0xab"), None);
        s.set("rare.friends.key.0xab", Some("{\"secret\":\"x\"}"));
        s.set("ui:friends:0xab", Some("{}"));
        assert_eq!(s.get("rare.friends.key.0xab").as_deref(), Some("{\"secret\":\"x\"}"));
        assert_eq!(s.get("ui:friends:0xab").as_deref(), Some("{}"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("rare.friends.key.0xab")).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        s.set("rare.friends.key.0xab", None);
        assert_eq!(s.get("rare.friends.key.0xab"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
