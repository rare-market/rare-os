//! ~/.config/rare/launcher.json, the file both launchers share: read a field, merge fields back
//! without touching the rest.

use serde_json::Value;

fn read() -> Value {
    std::fs::read(rare_core::paths::config_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| serde_json::json!({}))
}

pub fn get(key: &str) -> Option<Value> {
    read().get(key).cloned().filter(|v| !v.is_null())
}

pub fn get_bool(key: &str) -> bool {
    get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

pub fn get_str(key: &str) -> Option<String> {
    get(key).and_then(|v| v.as_str().map(String::from))
}

/// Sets fields (null removes one) and writes the file whole.
pub fn set(fields: &[(&str, Value)]) {
    let path = rare_core::paths::config_file();
    let mut v = read();
    if let Some(o) = v.as_object_mut() {
        for (k, val) in fields {
            if val.is_null() {
                o.remove(*k);
            } else {
                o.insert((*k).to_string(), val.clone());
            }
        }
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_vec_pretty(&v).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(tmp, path);
    }
}
