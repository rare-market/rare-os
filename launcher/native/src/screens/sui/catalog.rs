//! The Sui Games catalog as the screens use it (launcher/src/games/catalog.ts, lib/rare.ts
//! `SuiGame`, `clean`, `withBuiltins`): rare.market's games with the launcher's own per-game
//! profiles (launcher/src/games/profiles.json, read from the same file) and the bundled seed
//! (launcher/src/lib/games.seed.json) for offline and the fixtures.

use crate::data::model::Game;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::sync::OnceLock;

static PROFILES_JSON: &str = include_str!("../../../../src/games/profiles.json");
static SEED_JSON: &str = include_str!("../../../../src/lib/games.seed.json");

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Collection {
    pub nft_type: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Token {
    pub symbol: String,
    pub coin_type: String,
}

/// lib/rare.ts `SuiGame` after `clean` (missing strings empty, lists empty, token null).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SuiGame {
    pub slug: String,
    pub name: String,
    pub studio: String,
    pub genre: String,
    /// "live" | "beta" | "coming" | "announced" | "paused" | "dead"
    pub status: Option<String>,
    /// "web" | "pc" | "mobile" | "suiplay"
    pub platforms: Vec<String>,
    pub website: String,
    pub play_url: Option<String>,
    pub x: String,
    pub description: String,
    pub logo_url: String,
    pub banner_url: String,
    pub collections: Vec<Collection>,
    pub token: Option<Token>,
    pub featured: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NativeInfo {
    pub store: String,
    pub store_url: String,
    pub steam_app_ids: Vec<u64>,
    pub titles: Vec<String>,
    pub how: String,
}

/// games/catalog.ts `Profile` (what the screens read).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    pub title: Option<String>,
    /// Some(None) when the profile says `playUrl: null`
    pub play_url: Option<Option<String>>,
    pub builtin: bool,
    pub unavailable: Option<String>,
    pub native: Option<NativeInfo>,
    pub genre: Option<String>,
    pub description: Option<String>,
}

fn profiles() -> &'static Map<String, Value> {
    static P: OnceLock<Map<String, Value>> = OnceLock::new();
    P.get_or_init(|| serde_json::from_str::<Value>(PROFILES_JSON).ok().and_then(|v| v.get("games").and_then(|g| g.as_object().cloned())).unwrap_or_default())
}

fn str_of(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}

/// `profile(slug)` (a `web:` prefix is dropped).
pub fn profile(slug: &str) -> Option<Profile> {
    let v = profiles().get(slug.strip_prefix("web:").unwrap_or(slug))?;
    Some(Profile {
        title: str_of(v, "title"),
        play_url: v.get("playUrl").map(|p| p.as_str().map(str::to_string)),
        builtin: v.get("builtin").and_then(Value::as_bool).unwrap_or(false),
        unavailable: str_of(v, "unavailable"),
        native: v.get("native").and_then(|n| serde_json::from_value(n.clone()).ok()),
        genre: str_of(v, "genre"),
        description: str_of(v, "description"),
    })
}

/// GamePanels.tsx `WebGameInfo`: the input label and the controls line for a library web game
/// (profile by id, then by the host of its URL).
pub fn web_controls(id: &str, url: Option<&str>) -> (&'static str, String) {
    let by_id = profiles().get(id.strip_prefix("web:").unwrap_or(id));
    let host = url.and_then(|u| u.split("://").nth(1)).and_then(|r| r.split('/').next()).map(|h| h.trim_start_matches("www.").to_string());
    let p = by_id.or_else(|| {
        let host = host.as_ref()?;
        profiles().values().find(|v| v.get("hosts").and_then(Value::as_array).is_some_and(|hs| hs.iter().any(|h| h.as_str().is_some_and(|h| h.trim_start_matches("www.") == host))))
    });
    let mode = p.and_then(|v| v.get("input")).and_then(Value::as_str).unwrap_or("keys");
    let label = match mode {
        "gamepad" => "Controller",
        "focus" => "Controller picks buttons",
        _ => "Controller as keyboard",
    };
    let text = p.and_then(|v| v.get("controls")).and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| {
        if mode == "keys" {
            "D-pad and left stick are the arrow keys, A Enter, B Escape, X Space. The right stick moves a pointer, RB clicks. View opens the keyboard.".into()
        } else {
            "Guide returns to Rare with the game still running.".into()
        }
    });
    (label, text)
}

/// GameDetail.tsx `antiCheat`: Linux anti-cheat status for a Steam game, as (bad, text).
pub fn anti_cheat(id: &str) -> Option<(bool, String)> {
    static AC: OnceLock<Map<String, Value>> = OnceLock::new();
    let ac = AC.get_or_init(|| serde_json::from_str::<Map<String, Value>>(include_str!("../../../../src/lib/anticheat.json")).unwrap_or_default());
    let e = ac.get(id.strip_prefix("steam:")?)?.as_array()?;
    let status = e.first()?.as_str()?;
    let names = e.get(1)?.as_array()?.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ");
    match status {
        "Denied" => Some((true, format!("Online play won't run here: the publisher blocks Linux in its anti-cheat ({names}). Offline modes may still work."))),
        "Broken" => Some((true, format!("Online play won't run here: its anti-cheat ({names}) doesn't support Linux yet. Offline modes may still work."))),
        "Supported" => Some((false, format!("{names} supports Linux and Proton."))),
        "Running" => Some((false, format!("Runs with {names} on Proton, unofficially."))),
        _ => None,
    }
}

/// `isActive`: live and beta are shown as games; everything else waits under Coming.
pub fn is_active(g: &SuiGame) -> bool {
    matches!(g.status.as_deref(), Some("live" | "beta"))
}

/// `STATUS_LABEL`.
pub fn status_label(s: &str) -> String {
    match s {
        "live" => "Live",
        "beta" => "Beta",
        "coming" => "Coming soon",
        "announced" => "Announced",
        "paused" => "Paused",
        "dead" => "Offline",
        other => other,
    }
    .to_string()
}

pub fn platform_label(p: &str) -> String {
    match p {
        "web" => "Web",
        "pc" => "PC",
        "mobile" => "Mobile",
        "suiplay" => "SuiPlay",
        other => other,
    }
    .to_string()
}

/// `playUrl`: where a catalog game plays in the browser; the profile wins over the catalog.
pub fn play_url(g: &SuiGame) -> Option<String> {
    if let Some(p) = profile(&g.slug) {
        if let Some(u) = p.play_url {
            return u;
        }
    }
    if let Some(u) = g.play_url.as_ref().filter(|u| u.starts_with("https://")) {
        return Some(u.clone());
    }
    if g.platforms.iter().any(|p| p == "web") && g.website.starts_with("https://") {
        return Some(g.website.clone());
    }
    None
}

/// `unavailable`: why a web game can't be played right now.
pub fn unavailable(slug: &str) -> Option<String> {
    profile(slug).and_then(|p| p.unavailable)
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).collect()
}

/// `installedNative`: the installed library game for a native Sui title, by Steam app id then title.
pub fn installed_native<'a>(slug: &str, name: &str, library: &'a [Game]) -> Option<&'a Game> {
    let n = profile(slug).and_then(|p| p.native);
    let games: Vec<&Game> = library.iter().filter(|g| g.installed && g.source != "web").collect();
    let ids: Vec<String> = n.as_ref().map(|n| n.steam_app_ids.iter().map(|i| format!("steam:{i}")).collect()).unwrap_or_default();
    if let Some(g) = games.iter().find(|g| ids.contains(&g.id)) {
        return Some(g);
    }
    let mut titles = vec![norm(name)];
    titles.extend(n.map(|n| n.titles.iter().map(|t| norm(t)).collect::<Vec<_>>()).unwrap_or_default());
    games.into_iter().find(|g| titles.contains(&norm(&g.title)))
}

/// `builtinCatalog`: games that ship with the launcher.
pub fn builtin_catalog() -> Vec<SuiGame> {
    let mut out = Vec::new();
    for (slug, v) in profiles() {
        if !v.get("builtin").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let Some(url) = str_of(v, "playUrl") else { continue };
        out.push(SuiGame {
            slug: slug.clone(),
            name: str_of(v, "title").unwrap_or_else(|| slug.to_uppercase()),
            genre: str_of(v, "genre").unwrap_or_default(),
            status: Some("live".into()),
            platforms: vec!["web".into()],
            website: url,
            description: str_of(v, "description").unwrap_or_default(),
            logo_url: format!("games/{slug}/icon.svg"),
            banner_url: format!("games/{slug}/hero.jpg"),
            featured: true,
            ..Default::default()
        });
    }
    out
}

/// `withBuiltins`: the catalog plus built-in games it doesn't list yet.
pub fn with_builtins(mut games: Vec<SuiGame>) -> Vec<SuiGame> {
    let extra: Vec<SuiGame> = builtin_catalog().into_iter().filter(|b| !games.iter().any(|g| g.slug == b.slug)).collect();
    games.extend(extra);
    games
}

/// `clean`: entries without a slug and name are dropped; JSON nulls become the TS defaults.
pub fn clean(raw: &Value) -> Vec<SuiGame> {
    let Some(list) = raw.as_array() else { return Vec::new() };
    list.iter()
        .filter(|g| g.get("slug").is_some_and(Value::is_string) && g.get("name").is_some_and(Value::is_string))
        .filter_map(|g| {
            let mut g = g.clone();
            if let Some(o) = g.as_object_mut() {
                for k in ["studio", "genre", "website", "x", "description", "logoUrl", "bannerUrl"] {
                    if o.get(k).is_some_and(Value::is_null) {
                        o.insert(k.into(), Value::String(String::new()));
                    }
                }
                for k in ["platforms", "collections"] {
                    if !o.get(k).is_some_and(Value::is_array) {
                        o.insert(k.into(), Value::Array(Vec::new()));
                    }
                }
                if o.get("featured").is_some_and(Value::is_null) {
                    o.insert("featured".into(), Value::Bool(false));
                }
            }
            serde_json::from_value(g).ok()
        })
        .collect()
}

/// The bundled catalog with the built-ins (offline, fixtures): `{ games, live: false }`.
pub fn seed() -> Vec<SuiGame> {
    with_builtins(serde_json::from_str::<Value>(SEED_JSON).map(|v| clean(&v)).unwrap_or_default())
}

/// Key art without baked-in text, by game (SuiGames.tsx `CLEAN_ART`, `CLEAN_BANNERS`).
pub fn clean_art(g: &SuiGame) -> Option<String> {
    match g.slug.as_str() {
        "act" => return Some("/games/act/hero.jpg".into()),
        _ => {}
    }
    const CLEAN_BANNERS: [&str; 8] = ["warped-universe", "eve-frontier", "pawtato-land", "cosmocadia", "brave-frontier-versus", "doubleup", "m10", "darktimes"];
    (CLEAN_BANNERS.contains(&g.slug.as_str()) && usable_banner(g)).then(|| g.banner_url.clone())
}

/// Narrower banners are promo thumbnails: never used as key art.
pub const MIN_BANNER_WIDTH: u32 = 1280;
/// A tiny logo is shown at its own size, never stretched.
pub const MIN_LOGO_WIDTH: u32 = 128;

/// `usableBanner`: there is one and it is not an .ico.
pub fn usable_banner(g: &SuiGame) -> bool {
    if g.banner_url.is_empty() {
        return false;
    }
    let lower = g.banner_url.to_lowercase();
    let path = lower.split(['?', '#']).next().unwrap_or("");
    !path.ends_with(".ico")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_and_profiles() {
        let games = seed();
        assert!(games.len() >= 22);
        let act = games.iter().find(|g| g.slug == "act").unwrap();
        assert_eq!(act.name, "ACT");
        assert!(is_active(act));
        assert_eq!(play_url(act).as_deref(), Some("https://play.actuniverse.com/"));
        assert_eq!(status_label("dead"), "Offline");
        let s8192 = games.iter().find(|g| g.slug == "sui-8192").unwrap();
        assert!(!is_active(s8192));
        assert!(unavailable("sui-8192").is_some());
        let eve = games.iter().find(|g| g.slug == "eve-frontier").unwrap();
        assert!(profile("eve-frontier").unwrap().native.is_some());
        assert_eq!(clean_art(act).as_deref(), Some("/games/act/hero.jpg"));
        assert_eq!(clean_art(eve), usable_banner(eve).then(|| eve.banner_url.clone()));
        let g = SuiGame { banner_url: "https://x/y.ICO?v=1".into(), ..Default::default() };
        assert!(!usable_banner(&g));
    }

    #[test]
    fn installed_by_steam_id_then_title() {
        let p = profile("eve-frontier").unwrap().native.unwrap();
        let mk = |id: &str, title: &str| -> Game { serde_json::from_value(serde_json::json!({ "id": id, "source": "steam", "title": title })).unwrap() };
        let mut lib = vec![mk("steam:1", "Other")];
        if let Some(id) = p.steam_app_ids.first() {
            lib.push(mk(&format!("steam:{id}"), "Whatever"));
            assert_eq!(installed_native("eve-frontier", "EVE Frontier", &lib).unwrap().id, format!("steam:{id}"));
        }
        let lib = vec![mk("steam:9", "E.V.E. frontier")];
        assert_eq!(installed_native("eve-frontier", "EVE Frontier", &lib).unwrap().id, "steam:9");
    }
}
