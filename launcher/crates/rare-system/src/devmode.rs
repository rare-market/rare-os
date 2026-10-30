//! Developer mode: `rare-launcher --dev-game <url>` opens a local web game in the web game
//! runtime, with Rare Wallet and `rare:platform` (the Local API) injected like any installed game,
//! plus a banner so nobody mistakes it for a shipped title. The Local API scopes every call to the
//! game's catalog collections; a dev game isn't in the catalog, so the collections come from
//! `--dev-collection <nft type>` (repeatable) and the slug from `--dev-slug` (see docs/WEBGAMES.md).
//!
//!   rare-launcher --dev-game http://localhost:5173 --dev-slug my-game \
//!     --dev-collection 0xabc::items::Sword
//!
//! A second `rare-launcher --dev-game ...` while one is running reloads the game in the running one.

use serde::Serialize;
use std::sync::Mutex;

const BANNER_JS: &str = include_str!("webgame-dev.js");

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevGame {
    pub url: String,
    pub slug: String,
    pub id: String,
    pub title: String,
    pub collections: Vec<String>,
}

static DEV: Mutex<Option<DevGame>> = Mutex::new(None);

fn slugify(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let t = t.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if t.is_empty() { "dev-game".into() } else { t }
}

/// The dev game asked for on the command line, if any. `Err` is a sentence for stderr.
pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Option<DevGame>, String> {
    let mut it = args.into_iter();
    let (mut url, mut slug, mut cols) = (None, None, Vec::new());
    while let Some(a) = it.next() {
        let (key, inline) = match a.split_once('=') {
            Some((k, v)) if k.starts_with("--dev-") => (k.to_string(), Some(v.to_string())),
            _ => (a.clone(), None),
        };
        if !matches!(key.as_str(), "--dev-game" | "--dev-slug" | "--dev-collection") {
            continue;
        }
        let Some(v) = inline.or_else(|| it.next()) else { return Err(format!("{key} needs a value")) };
        match key.as_str() {
            "--dev-game" => url = Some(v),
            "--dev-slug" => slug = Some(slugify(&v)),
            _ => cols.push(v),
        }
    }
    let Some(url) = url else { return Ok(None) };
    let parsed: url::Url = url.parse().map_err(|_| format!("--dev-game: \"{url}\" is not a URL (a local path isn't served; run your dev server and pass its URL)"))?;
    if !crate::host::get().web_url_allowed(&parsed) {
        return Err("--dev-game: use https, or http on localhost".into());
    }
    let slug = slug.unwrap_or_else(|| slugify(&format!("dev-{}", parsed.host_str().unwrap_or("game")).replace("dev-localhost", "dev-game")));
    Ok(Some(DevGame { id: format!("web:{slug}"), title: format!("{slug} (dev)"), url: parsed.to_string(), slug, collections: cols }))
}

/// The running dev game, for the UI (Local API scoping, Settings > Developer).
pub fn dev_game_info() -> Option<DevGame> {
    DEV.lock().unwrap().clone()
}

/// Records the dev game for `dev_game_info` and `script_for`; the front end then opens it.
pub fn set_active(g: DevGame) {
    *DEV.lock().unwrap() = Some(g);
}

/// Init script for the dev game's window: the banner. Empty for every other game.
pub fn script_for(game_id: &str, network: &str) -> String {
    match DEV.lock().unwrap().as_ref() {
        Some(g) if g.id == game_id => format!("window.__rareDev = {};\n{BANNER_JS}", serde_json::json!({ "slug": g.slug, "network": network, "collections": g.collections })),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &[&str]) -> Result<Option<DevGame>, String> {
        parse_args(a.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_flag_no_dev_game() {
        assert_eq!(p(&["rare-launcher"]), Ok(None));
        assert_eq!(p(&["rare-launcher", "rare://item/0x1"]), Ok(None));
    }

    #[test]
    fn url_slug_and_collections() {
        let g = p(&["x", "--dev-game", "http://localhost:5173", "--dev-slug", "My Game!", "--dev-collection", "0xa::m::A", "--dev-collection=0xb::m::B"]).unwrap().unwrap();
        assert_eq!(g.slug, "my-game");
        assert_eq!(g.id, "web:my-game");
        assert_eq!(g.url, "http://localhost:5173/");
        assert_eq!(g.collections, vec!["0xa::m::A", "0xb::m::B"]);
    }

    #[test]
    fn equals_form_and_default_slug() {
        let g = p(&["x", "--dev-game=https://play.example.com/g"]).unwrap().unwrap();
        assert_eq!(g.slug, "dev-play-example-com");
        let g = p(&["x", "--dev-game=http://127.0.0.1:8080"]).unwrap().unwrap();
        assert_eq!(g.slug, "dev-127-0-0-1");
    }

    #[test]
    fn refuses_what_a_web_game_may_not_be() {
        assert!(p(&["x", "--dev-game", "http://example.com"]).is_err());
        assert!(p(&["x", "--dev-game", "/home/me/game"]).is_err());
        assert!(p(&["x", "--dev-game"]).is_err());
    }
}
