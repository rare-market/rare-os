//! The unified library lives in the rare-library crate (shared with the native launcher). This
//! module re-exports it, so `crate::library::steam::...` and friends keep working in the shell.

pub use rare_library::*;

/// The scan with the Rare Store's installed games joined in (the store module is still here).
pub fn scan(extra_folders: &[String], web_user: &[crate::config::WebGame], removed: &[String]) -> rare_library::model::Library {
    rare_library::scan(extra_folders, web_user, removed, crate::store::library_games())
}
