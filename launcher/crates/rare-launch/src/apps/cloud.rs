//! Cloud games as Library tiles: Xbox Cloud Gaming's catalogue (Game Pass's cloud list, public)
//! and the GeForce NOW games the player owns in their stores (NVIDIA's public supported-games
//! list matched against Steam, Epic and GOG on this device). Each tile opens its app (catalog
//! `xcloud`, `geforcenow`) straight at the game's stream, with the app's sign-in.
//!
//! The lists are fetched at most once a day into ~/.cache/rare/cloud.json; the library scan only
//! reads that file, so scanning never waits on the network.

use rare_library::model::{Art, Game, LaunchSpec, Source};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const XBOX_APP: &str = "xcloud";
pub const GFN_APP: &str = "geforcenow";

/// Game Pass's "all cloud games" list (the one xbox.com/play shows under All games).
const XBOX_LIST: &str = "https://catalog.gamepass.com/sigls/v2?id=29a81209-df6f-41fd-a528-2ae6b91f719c&language=en-us&market=US";
const XBOX_PRODUCTS: &str = "https://displaycatalog.mp.microsoft.com/v7.0/products?market=US&languages=en-us&fieldsTemplate=browse&bigIds=";
/// Products per catalogue request (the service answers up to 200 ids at once).
const XBOX_BATCH: usize = 150;
const GFN_LIST: &str = "https://static.nvidiagrid.net/supported-public-game-list/locales/gfnpc-en-US.json";
/// How old the lists may get before a fetch.
const MAX_AGE: Duration = Duration::from_secs(24 * 3600);

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct XboxGame {
    /// Microsoft Store product id (9NPDN9R45JX4).
    pub id: String,
    pub title: String,
    pub cover: Option<String>,
    pub hero: Option<String>,
    pub header: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct GfnGame {
    /// GeForce NOW's game id (its deep links take it).
    pub id: u64,
    pub title: String,
    /// "Steam", "Epic", "GOG", "Ubisoft Connect", ...
    pub store: String,
    pub steam: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Catalogue {
    /// Unix seconds of the last good fetch.
    pub fetched: u64,
    pub xbox: Vec<XboxGame>,
    pub gfn: Vec<GfnGame>,
}

fn path() -> PathBuf {
    rare_core::paths::cache_dir().join("cloud.json")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn load() -> Option<Catalogue> {
    serde_json::from_slice(&std::fs::read(path()).ok()?).ok()
}

fn save(c: &Catalogue) {
    let p = path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = p.with_extension("json.tmp");
    if let Ok(b) = serde_json::to_vec(c) {
        if std::fs::write(&tmp, b).is_ok() {
            let _ = std::fs::rename(tmp, p);
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout_read(Duration::from_secs(60)).user_agent("RareLauncher/1").build()
}

fn get_json(a: &ureq::Agent, url: &str) -> Result<Value, String> {
    a.get(url).call().map_err(|e| e.to_string())?.into_json::<Value>().map_err(|e| e.to_string())
}

/// `//store-images...` to an https URL at `w` pixels wide.
fn ms_image(uri: &str, w: u32) -> String {
    let u = if uri.starts_with("//") { format!("https:{uri}") } else { uri.to_string() };
    format!("{u}?w={w}")
}

/// One product of the display catalogue's answer.
pub fn parse_xbox_product(p: &Value) -> Option<XboxGame> {
    let id = p["ProductId"].as_str()?.to_string();
    let lp = p["LocalizedProperties"].get(0)?;
    let title = lp["ProductTitle"].as_str()?.trim().to_string();
    let images = lp["Images"].as_array().cloned().unwrap_or_default();
    let pick = |purpose: &str, w: u32| images.iter().find(|i| i["ImagePurpose"].as_str() == Some(purpose)).and_then(|i| i["Uri"].as_str()).map(|u| ms_image(u, w));
    Some(XboxGame { id, title, cover: pick("Poster", 600), hero: pick("SuperHeroArt", 1920), header: pick("TitledHeroArt", 920).or_else(|| pick("BoxArt", 600)) })
}

fn fetch_xbox(a: &ureq::Agent) -> Result<Vec<XboxGame>, String> {
    let list = get_json(a, XBOX_LIST)?;
    let ids: Vec<String> = list.as_array().into_iter().flatten().filter_map(|v| v["id"].as_str().map(str::to_string)).collect();
    if ids.is_empty() {
        return Err("Xbox Cloud Gaming's list came back empty".into());
    }
    let mut by_id: HashMap<String, XboxGame> = HashMap::new();
    for chunk in ids.chunks(XBOX_BATCH) {
        let v = get_json(a, &format!("{XBOX_PRODUCTS}{}", chunk.join(",")))?;
        for p in v["Products"].as_array().into_iter().flatten() {
            if let Some(g) = parse_xbox_product(p) {
                by_id.insert(g.id.clone(), g);
            }
        }
    }
    // The list's own order (newest and featured first).
    Ok(ids.iter().filter_map(|i| by_id.remove(i)).collect())
}

/// NVIDIA's supported-games list.
pub fn parse_gfn(v: &Value) -> Vec<GfnGame> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| {
            let steam = g["steamUrl"].as_str().and_then(|u| u.trim_end_matches('/').rsplit('/').next()).and_then(|s| s.parse::<u32>().ok());
            Some(GfnGame { id: g["id"].as_u64()?, title: g["title"].as_str()?.trim().to_string(), store: g["store"].as_str().unwrap_or("").to_string(), steam })
        })
        .collect()
}

/// Fetches both lists and keeps them. A list that fails keeps its last good copy.
pub fn refresh() -> Result<Catalogue, String> {
    let a = agent();
    let old = load().unwrap_or_default();
    let xbox = fetch_xbox(&a);
    let gfn = get_json(&a, GFN_LIST).map(|v| parse_gfn(&v));
    if let (Err(x), Err(g)) = (&xbox, &gfn) {
        return Err(format!("{x}; {g}"));
    }
    let c = Catalogue { fetched: now(), xbox: xbox.unwrap_or(old.xbox), gfn: gfn.ok().filter(|g| !g.is_empty()).unwrap_or(old.gfn) };
    save(&c);
    Ok(c)
}

static FETCHING: AtomicBool = AtomicBool::new(false);

/// Fetches the lists in the background when they are missing or a day old; `done` runs after a
/// good fetch (the caller scans the library again). One fetch at a time.
pub fn refresh_if_stale(done: impl FnOnce() + Send + 'static) {
    let stale = load().map(|c| now().saturating_sub(c.fetched) > MAX_AGE.as_secs()).unwrap_or(true);
    if !stale || FETCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("cloud-games".into())
        .spawn(move || {
            let r = refresh();
            FETCHING.store(false, Ordering::SeqCst);
            match r {
                Ok(c) => {
                    rare_core::host::global().log(&format!("[cloud] {} Xbox Cloud Gaming games, {} GeForce NOW games", c.xbox.len(), c.gfn.len()));
                    done();
                }
                Err(e) => rare_core::host::global().log(&format!("[cloud] lists not fetched: {e}")),
            }
        })
        .ok();
}

/// What the player owns, for GeForce NOW: Steam app ids and normalised titles from every store.
#[derive(Debug, Default)]
pub struct Owned {
    pub steam: HashSet<u32>,
    pub titles: HashSet<String>,
}

/// Owned games on this device: the scanned library (installed games) plus what Steam's played
/// list and Heroic's Epic and GOG library caches know.
pub fn owned(library: &[Game]) -> Owned {
    let mut o = Owned::default();
    for g in library {
        if let LaunchSpec::Steam { appid } = g.launch {
            o.steam.insert(appid);
        }
        if !matches!(g.source, Source::Apps | Source::Web | Source::Cloud) {
            o.titles.insert(norm(&g.title));
        }
    }
    o.steam.extend(rare_library::steam::played_appids());
    o.titles.extend(rare_library::heroic::owned_titles().iter().map(|t| norm(t)));
    o
}

fn norm(t: &str) -> String {
    t.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The xbox.com/play path segment for a title ("Halo Infinite" to "halo-infinite").
fn slug(t: &str) -> String {
    let mut s = String::new();
    for c in t.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    s.trim_end_matches('-').to_string()
}

pub fn xbox_url(g: &XboxGame) -> String {
    format!("https://www.xbox.com/en-US/play/launch/{}/{}", slug(&g.title), g.id)
}

pub fn gfn_url(g: &GfnGame) -> String {
    format!("https://play.geforcenow.com/mall/#/deeplink?game-id={}", g.id)
}

fn steam_art(appid: u32) -> Art {
    let cdn = format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}");
    Art { cover: vec![format!("{cdn}/library_600x900.jpg")], hero: vec![format!("{cdn}/library_hero.jpg")], logo: vec![format!("{cdn}/logo.png")], header: vec![format!("{cdn}/header.jpg")], icon: vec![] }
}

fn tile(id: String, title: String, art: Art, app: &str, url: String, origin: &str) -> Game {
    Game {
        id,
        source: Source::Cloud,
        title,
        installed: true,
        install_dir: None,
        size_bytes: None,
        last_played: None,
        playtime_minutes: None,
        art,
        launch: LaunchSpec::Cloud { app: app.to_string(), url },
        origin: origin.to_string(),
        foreign: false,
    }
}

/// The Library's cloud tiles: every Xbox Cloud Gaming game, then the GeForce NOW games the
/// player owns (with the owned copy's art when the library has it). Ids are `cloud:xbox:<id>` and
/// `cloud:gfn:<id>`.
pub fn games(c: &Catalogue, owned: &Owned, library: &[Game]) -> Vec<Game> {
    let mut out: Vec<Game> = c
        .xbox
        .iter()
        .map(|g| {
            let art = Art { cover: g.cover.iter().cloned().collect(), hero: g.hero.iter().cloned().collect(), header: g.header.iter().cloned().collect(), ..Default::default() };
            tile(format!("cloud:xbox:{}", g.id), g.title.clone(), art, XBOX_APP, xbox_url(g), "Xbox Cloud Gaming")
        })
        .collect();
    let art_by_title: HashMap<String, &Art> = library.iter().filter(|g| !g.art.cover.is_empty()).map(|g| (norm(&g.title), &g.art)).collect();
    let mut seen = HashSet::new();
    for g in &c.gfn {
        let key = norm(&g.title);
        let mine = g.steam.is_some_and(|a| owned.steam.contains(&a)) || (g.steam.is_none() && owned.titles.contains(&key));
        // One tile per game: GeForce NOW lists a game once per store.
        if !mine || !seen.insert(key.clone()) {
            continue;
        }
        let art = art_by_title.get(&key).map(|a| (*a).clone()).or_else(|| g.steam.map(steam_art)).unwrap_or_default();
        out.push(tile(format!("cloud:gfn:{}", g.id), g.title.clone(), art, GFN_APP, gfn_url(g), &format!("GeForce NOW, from {}", if g.store.is_empty() { "your store" } else { &g.store })));
    }
    out
}

/// GeForce NOW tiles that are a Steam game: (tile id, Steam app id), so the library knows the
/// cloud copy and the Steam copy are one game (rare-library dupes.rs).
pub fn steam_keys() -> Vec<(String, u32)> {
    load().map(|c| c.gfn.iter().filter_map(|g| g.steam.map(|a| (format!("cloud:gfn:{}", g.id), a))).collect()).unwrap_or_default()
}

/// The cloud tiles from the kept lists (empty before the first fetch).
pub fn library_games(library: &[Game]) -> Vec<Game> {
    match load() {
        Some(c) => games(&c, &owned(library), library),
        None => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn xbox_products_and_links() {
        let p = json!({ "ProductId": "9NPDN9R45JX4", "LocalizedProperties": [{ "ProductTitle": "Halo Infinite: Campaign", "Images": [
            { "ImagePurpose": "Poster", "Uri": "//store-images.s-microsoft.com/image/apps.1.p" },
            { "ImagePurpose": "SuperHeroArt", "Uri": "//store-images.s-microsoft.com/image/apps.2.h" }
        ] }] });
        let g = parse_xbox_product(&p).unwrap();
        assert_eq!(g.title, "Halo Infinite: Campaign");
        assert_eq!(g.cover.as_deref(), Some("https://store-images.s-microsoft.com/image/apps.1.p?w=600"));
        assert_eq!(xbox_url(&g), "https://www.xbox.com/en-US/play/launch/halo-infinite-campaign/9NPDN9R45JX4");
    }

    #[test]
    fn gfn_only_owned_games_once() {
        let v = json!([
            { "id": 1, "title": "7 Days to Die", "steamUrl": "https://store.steampowered.com/app/251570", "store": "Steam" },
            { "id": 2, "title": "Absolute Drift", "steamUrl": "", "store": "Epic" },
            { "id": 3, "title": "Absolute Drift", "steamUrl": "", "store": "GOG" },
            { "id": 4, "title": "Not Mine", "steamUrl": "https://store.steampowered.com/app/9", "store": "Steam" }
        ]);
        let c = Catalogue { fetched: 1, xbox: vec![XboxGame { id: "X1".into(), title: "Forza".into(), ..Default::default() }], gfn: parse_gfn(&v) };
        assert_eq!(c.gfn[0].steam, Some(251570));
        let owned = Owned { steam: [251570].into(), titles: [norm("Absolute Drift")].into() };
        let g = games(&c, &owned, &[]);
        let ids: Vec<&str> = g.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, ["cloud:xbox:X1", "cloud:gfn:1", "cloud:gfn:2"]);
        assert!(g.iter().all(|g| g.source == Source::Cloud));
        let LaunchSpec::Cloud { app, url } = &g[1].launch else { panic!() };
        assert_eq!((app.as_str(), url.as_str()), (GFN_APP, "https://play.geforcenow.com/mall/#/deeplink?game-id=1"));
        assert!(g[1].art.cover[0].ends_with("/251570/library_600x900.jpg"));
    }
}
