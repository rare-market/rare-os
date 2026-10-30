//! Per-game web and native data from `src/games/profiles.json` (shared with the UI): where a
//! web game plays, how the controller maps onto it, and which web games ship pre-installed.

use serde_json::Value;
use std::sync::OnceLock;

const RAW: &str = include_str!("../../../src/games/profiles.json");

fn all() -> &'static serde_json::Map<String, Value> {
    static P: OnceLock<serde_json::Map<String, Value>> = OnceLock::new();
    P.get_or_init(|| {
        serde_json::from_str::<Value>(RAW)
            .ok()
            .and_then(|v| v.get("games").and_then(Value::as_object).cloned())
            .unwrap_or_default()
    })
}

fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url).ok()?.host_str().map(|h| h.to_ascii_lowercase())
}

/// The profile for a web game: by its id first ("act", "web:act"), then by the URL's host.
pub fn for_game(id: &str, url: &str) -> Option<(String, Value)> {
    let bare = id.strip_prefix("web:").unwrap_or(id);
    if let Some(p) = all().get(bare) {
        return Some((bare.to_string(), p.clone()));
    }
    let host = host_of(url)?;
    let bare_host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    all()
        .iter()
        .find(|(_, p)| {
            p.get("hosts").and_then(Value::as_array).is_some_and(|hs| {
                hs.iter().filter_map(Value::as_str).any(|h| {
                    let h = h.to_ascii_lowercase();
                    h == host || h.strip_prefix("www.").unwrap_or(&h) == bare_host
                })
            })
        })
        .map(|(k, v)| (k.clone(), v.clone()))
}

/// True when the profile says the game needs a Chromium browser instead of WebKitGTK.
pub fn wants_chrome(profile: &Value) -> bool {
    profile.get("browser").and_then(Value::as_str) == Some("chrome")
}

/// Web games that ship with Rare OS, in profile order (ACT first).
pub fn builtins() -> Vec<(String, String, String)> {
    all()
        .iter()
        .filter(|(_, p)| p.get("builtin").and_then(Value::as_bool) == Some(true))
        .filter_map(|(slug, p)| {
            let url = p.get("playUrl")?.as_str()?.to_string();
            let title = p.get("title").and_then(Value::as_str).map(String::from).unwrap_or_else(|| slug.to_uppercase());
            Some((slug.clone(), title, url))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_profiles() {
        let (slug, p) = for_game("web:act", "https://play.actuniverse.com/").unwrap();
        assert_eq!(slug, "act");
        assert_eq!(p["input"], "gamepad");
        // A game added by URL still gets its profile through the host.
        let (slug, p) = for_game("web:my-copy", "https://www.pawtato.gg/?x=1").unwrap();
        assert_eq!(slug, "pawtato-land");
        assert_eq!(p["input"], "focus");
        assert!(for_game("web:other", "https://example.com/").is_none());
        let b: Vec<String> = builtins().into_iter().map(|b| b.0).collect();
        assert_eq!(b, vec!["act"]);
    }
}
