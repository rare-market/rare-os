//! Where Rare keeps things. One place, so both front ends agree.

use std::path::PathBuf;

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// ~/.config/rare
pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("rare")
}

/// ~/.config/rare/launcher.json
pub fn config_file() -> PathBuf {
    config_dir().join("launcher.json")
}

/// ~/.cache/rare
pub fn cache_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(|| PathBuf::from(".")).join("rare")
}

/// ~/.local/share/rare
pub fn data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("rare")
}

/// $XDG_RUNTIME_DIR/<name>, when the session has a runtime dir.
pub fn runtime_file(name: &str) -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|d| PathBuf::from(d).join(name))
}
