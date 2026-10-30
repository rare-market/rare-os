//! Fixtures for the native launcher, ported from the web launcher's TS mocks by
//! `launcher/native/tests/fixtures/export.mjs` (see its header). A fixture is what the backend
//! commands and rare.market return, in their JSON shape, grouped by mock module:
//!
//! ```text
//! { "lib":   { "library": Library, "device": DeviceInfo, "netStatus": .., "config": .., "running": [..], .. },
//!   "stream": { "status", "hosts", "discover", "apps" },   "share": { "status", "list" },
//!   "notify": { "recent", "alerts" },   "friends": { "overview", "code" },
//!   "store": { "config", "games", "licences", "price" },   "trophies": { "steam", "config" },
//!   "inventory": { "inventory" },   "move": { "scan" } }
//! ```
//!
//! - In a golden run `RR_FIXTURE` names the state's fixture (image URLs already replaced by local
//!   placeholder files): [`Fixture::from_env`]. A screen that finds one shows it instead of asking
//!   the backend, and takes device facts (battery, Wi-Fi, clock) from it too.
//! - In tests, [`Fixture::set`] loads a set by name ("default", or a web shot's name through
//!   [`Fixture::for_shot`]) from the repo.
//!
//! Every fixture is taken at 2026-10-27 13:41:00 UTC ([`FIXED_UNIX`]); golden runs pin the wall
//! clock there, so "2 h ago" reads the same as on the web.

use serde::de::DeserializeOwned;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The instant every fixture was taken at (index.json `fixedMs` / 1000).
pub const FIXED_UNIX: i64 = 1_793_108_460;

#[derive(Clone, Debug, Default)]
pub struct Fixture {
    pub data: Value,
}

impl Fixture {
    /// The fixture of a golden run (`RR_FIXTURE=<json>`), if any.
    pub fn from_env() -> Option<Fixture> {
        let p = std::env::var_os("RR_FIXTURE")?;
        Fixture::from_file(Path::new(&p)).ok()
    }

    pub fn from_file(p: &Path) -> Result<Fixture, String> {
        let b = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
        let data = serde_json::from_slice(&b).map_err(|e| format!("{}: {e}", p.display()))?;
        Ok(Fixture { data })
    }

    /// The committed fixtures (launcher/native/tests/fixtures).
    pub fn dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../native/tests/fixtures")
    }

    fn index() -> Result<Value, String> {
        let p = Fixture::dir().join("index.json");
        serde_json::from_slice(&std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?).map_err(|e| e.to_string())
    }

    /// A set by name, merged from its modules' variants (art URLs left as the mocks have them).
    pub fn set(name: &str) -> Result<Fixture, String> {
        let idx = Fixture::index()?;
        let mods = idx["sets"][name]["modules"].as_object().ok_or_else(|| format!("no fixture set {name}"))?;
        let mut data = serde_json::Map::new();
        for (m, v) in mods {
            let v = v.as_str().ok_or("bad index")?;
            let p = Fixture::dir().join(m).join(format!("{v}.json"));
            let b = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            data.insert(m.clone(), serde_json::from_slice(&b).map_err(|e| format!("{}: {e}", p.display()))?);
        }
        Ok(Fixture { data: Value::Object(data) })
    }

    /// The set a web screenshot (scripts/shots.mjs name) was taken with.
    pub fn for_shot(shot: &str) -> Result<Fixture, String> {
        let idx = Fixture::index()?;
        let set = idx["shots"][shot].as_str().ok_or_else(|| format!("no web shot {shot}"))?.to_string();
        Fixture::set(&set)
    }

    /// One answer, raw: `raw("lib", "device")`.
    pub fn raw(&self, module: &str, key: &str) -> Option<&Value> {
        self.data.get(module)?.get(key).filter(|v| !v.is_null())
    }

    /// One answer as the backend type: `get::<Library>("lib", "library")`.
    pub fn get<T: DeserializeOwned>(&self, module: &str, key: &str) -> Option<T> {
        serde_json::from_value(self.raw(module, key)?.clone()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Battery {
        percent: u8,
    }
    #[derive(Deserialize)]
    struct Device {
        battery: Battery,
        #[serde(rename = "isSuiplay")]
        is_suiplay: bool,
    }

    #[test]
    fn default_set_has_the_mock_device_and_library() {
        let f = Fixture::set("default").unwrap();
        let d: Device = f.get("lib", "device").unwrap();
        assert_eq!(d.battery.percent, 82);
        assert!(d.is_suiplay);
        let games = f.raw("lib", "library").unwrap()["games"].as_array().unwrap();
        assert!(games.iter().any(|g| g["title"] == "Hades II"));
    }

    #[test]
    fn web_shots_map_to_sets() {
        let none = Fixture::for_shot("home-nothing-installed").unwrap();
        let games = none.raw("lib", "library").unwrap()["games"].as_array().unwrap();
        assert!(games.iter().all(|g| g["source"] == "web"), "?nogames leaves only the built-in web games");
        assert!(Fixture::for_shot("no-such-shot").is_err());
    }

    #[test]
    fn fixed_instant_matches_the_index() {
        let idx = Fixture::index().unwrap();
        assert_eq!(idx["fixedMs"].as_i64().unwrap() / 1000, FIXED_UNIX);
    }

    #[test]
    fn rr_fixture_file() {
        let p = std::env::temp_dir().join(format!("rr-fixture-{}.json", std::process::id()));
        std::fs::write(&p, r#"{"lib":{"running":["steam:1"],"steamAccount":null}}"#).unwrap();
        let f = Fixture::from_file(&p).unwrap();
        assert_eq!(f.get::<Vec<String>>("lib", "running").unwrap(), vec!["steam:1"]);
        assert!(f.raw("lib", "steamAccount").is_none());
        let _ = std::fs::remove_file(p);
    }
}
