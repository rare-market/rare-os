//! The library in the shared data cache (rare-state), so the native UI reads it from memory like
//! any other keyed JSON and the web UI's boot snapshot can carry it.

use crate::model::Library;
use rare_state::Store;

/// The key the library is stored under.
pub const KEY: &str = "library";

/// Publishes a scan to the store (memory, then disk).
pub fn publish(store: &Store, lib: &Library) -> Result<(), String> {
    store.put(KEY, serde_json::to_string(lib).map_err(|e| e.to_string())?)
}

/// The last published library, if the store has one.
pub fn peek(store: &Store) -> Option<Library> {
    serde_json::from_str(&store.peek(KEY)?.body).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_then_peek() {
        let store = Store::open(rare_state::Config::default(), rare_state::Policy::new(rare_state::Freshness::Swr { ttl: std::time::Duration::from_secs(60) }), |_| None).unwrap();
        let lib = Library { games: vec![], sources: vec![], foreign_steam_libraries: vec!["/mnt/x".into()], owned: vec![], scanned_at: 7, emulation: None, emulation_app: None, keys: Default::default() };
        publish(&store, &lib).unwrap();
        let back = peek(&store).unwrap();
        assert_eq!(back.scanned_at, 7);
        assert_eq!(back.foreign_steam_libraries, vec!["/mnt/x"]);
    }
}
