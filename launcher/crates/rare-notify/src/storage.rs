//! Where notify keeps its small per-device state (the notice list, read marks, settings, the
//! stream's resume id, the device id). The web launcher used local storage; here the host hands a
//! [`Storage`], usually a [`FileStorage`] under its config directory. Keys and JSON values are the
//! same as the web launcher's, so state can be carried over as-is.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use serde_json::Value;

pub trait Storage: Send + Sync {
    /// The stored JSON text for a key.
    fn get(&self, key: &str) -> Option<String>;
    /// Stores JSON text, or removes the key with `None`. Failures are swallowed: state then lasts
    /// this session only, as with a full or private local storage.
    fn set(&self, key: &str, value: Option<&str>);
}

/// A key's value as JSON (`None` when missing or not JSON).
pub fn read_value(s: &dyn Storage, key: &str) -> Option<Value> {
    serde_json::from_str(&s.get(key)?).ok()
}

/// Stores a value as JSON; `None` (or JSON null) removes the key.
pub fn write_value<T: Serialize + ?Sized>(s: &dyn Storage, key: &str, value: Option<&T>) {
    match value.and_then(|v| serde_json::to_string(v).ok()).filter(|t| t != "null") {
        Some(t) => s.set(key, Some(&t)),
        None => s.set(key, None),
    }
}

/// In memory, for tests and for a launcher without a writable config directory.
#[derive(Default)]
pub struct MemStorage {
    map: Mutex<HashMap<String, String>>,
}

impl MemStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Storage for MemStorage {
    fn get(&self, key: &str) -> Option<String> {
        self.map.lock().unwrap().get(key).cloned()
    }
    fn set(&self, key: &str, value: Option<&str>) {
        let mut m = self.map.lock().unwrap();
        match value {
            Some(v) => m.insert(key.to_string(), v.to_string()),
            None => m.remove(key),
        };
    }
}

/// One `<key>.json` file per key under a directory the host gives (created on first write).
/// Writes go through a temporary file and a rename, so a crash never leaves half a file.
pub struct FileStorage {
    dir: PathBuf,
}

impl FileStorage {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        FileStorage { dir: dir.into() }
    }

    fn path(&self, key: &str) -> PathBuf {
        // Keys are fixed names like "rare.notify.list"; anything else is made file-safe.
        let safe: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' { c } else { '_' }).collect();
        self.dir.join(format!("{safe}.json"))
    }
}

impl Storage for FileStorage {
    fn get(&self, key: &str) -> Option<String> {
        std::fs::read_to_string(self.path(key)).ok()
    }
    fn set(&self, key: &str, value: Option<&str>) {
        let p = self.path(key);
        match value {
            None => {
                let _ = std::fs::remove_file(&p);
            }
            Some(v) => {
                let _ = std::fs::create_dir_all(&self.dir);
                let tmp = p.with_extension("json.tmp");
                if std::fs::write(&tmp, v).is_ok() {
                    let _ = std::fs::rename(&tmp, &p);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_storage_round_trips_and_removes() {
        let dir = std::env::temp_dir().join(format!("rare-notify-storage-{}", std::process::id()));
        let s = FileStorage::new(&dir);
        assert_eq!(s.get("rare.notify.last"), None);
        write_value(&s, "rare.notify.last", Some("42"));
        assert_eq!(read_value(&s, "rare.notify.last"), Some(Value::String("42".into())));
        write_value::<str>(&s, "rare.notify.last", None);
        assert_eq!(s.get("rare.notify.last"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn junk_reads_as_missing() {
        let s = MemStorage::new();
        s.set("k", Some("{not json"));
        assert_eq!(read_value(&s, "k"), None);
    }
}
