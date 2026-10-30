//! The data Home shows, from the same sources as the web launcher: the backend's library cache
//! (~/.cache/rare/library.json, written by library/cache.rs) and the live endpoints of live.ts.
//! In the full port the library crate is called directly; the cache is the same `Library` value.

use rare_render::Src;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Art {
    #[serde(default)]
    pub cover: Vec<String>,
    #[serde(default)]
    pub hero: Vec<String>,
    #[serde(default)]
    pub logo: Vec<String>,
    #[serde(default)]
    pub header: Vec<String>,
    #[serde(default)]
    pub icon: Vec<String>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: String,
    pub title: String,
    pub source: String,
    #[serde(default)]
    pub last_played: Option<i64>,
    #[serde(default)]
    pub playtime_minutes: Option<i64>,
    #[serde(default)]
    pub art: Art,
}

#[derive(Deserialize)]
struct Library {
    games: Vec<Game>,
}

#[derive(Deserialize)]
struct Stored {
    library: Library,
}

pub fn library_path() -> PathBuf {
    if let Ok(p) = std::env::var("RR_LIBRARY") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".cache/rare/library.json")
}

pub fn load_library() -> Vec<Game> {
    let p = library_path();
    match std::fs::read(&p).ok().and_then(|b| serde_json::from_slice::<Stored>(&b).ok()) {
        Some(s) => s.library.games,
        None => {
            log::warn!("no library at {}; using the bundled web games", p.display());
            ["act"]
                .iter()
                .map(|id| Game {
                    id: format!("web:{id}"),
                    title: "ACT".into(),
                    source: "web".into(),
                    last_played: None,
                    playtime_minutes: None,
                    art: Art {
                        cover: vec![format!("games/{id}/cover.jpg")],
                        hero: vec![format!("games/{id}/hero.jpg")],
                        logo: vec![format!("games/{id}/logo.svg")],
                        ..Default::default()
                    },
                })
                .collect()
        }
    }
}

macro_rules! bundled {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_bytes!(concat!("../../assets/", $name)) as &[u8])),*]
    };
}

/// Art the web launcher serves from public/ (web games' covers, logos).
static BUNDLED: &[(&str, &[u8])] = bundled!["games/act/cover.jpg", "games/act/hero.jpg", "games/act/logo.svg"];

pub fn src(s: &str) -> Option<Src> {
    if s.starts_with("http://") || s.starts_with("https://") {
        return Some(Src::Url(s.to_string()));
    }
    if s.starts_with('/') {
        return Some(Src::File(PathBuf::from(s)));
    }
    BUNDLED.iter().find(|(n, _)| *n == s).map(|(n, b)| Src::Static(n, b))
}

/// The first source of a list we can load (local files first: no network on the first frame).
pub fn first(list: &[String]) -> Option<Src> {
    list.iter().filter(|s| !s.starts_with("http")).find_map(|s| src(s)).or_else(|| list.iter().find_map(|s| src(s)))
}

pub fn source_label(s: &str) -> &'static str {
    match s {
        "steam" => "Steam",
        "epic" => "Epic Games",
        "gog" => "GOG",
        "itch" => "itch.io",
        "local" => "Local",
        "web" => "Web",
        "apps" => "Emulators",
        "rare" => "Rare Store",
        _ => "",
    }
}

pub fn fmt_ago(t: Option<i64>) -> String {
    let Some(t) = t.filter(|t| *t > 0) else { return "Never played".into() };
    let now = unsafe { libc::time(std::ptr::null_mut()) } as i64;
    let s = now - t;
    if s < 3600 {
        return "Played just now".into();
    }
    if s < 86400 {
        return format!("Played {} h ago", (s as f64 / 3600.0).round());
    }
    if s < 86400 * 2 {
        return "Played yesterday".into();
    }
    if s < 86400 * 30 {
        return format!("Played {} days ago", (s as f64 / 86400.0).round());
    }
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let tt = t as libc::time_t;
    // SAFETY: localtime_r fills tm.
    unsafe { libc::localtime_r(&tt, &mut tm) };
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("Played {} {}", M[tm.tm_mon.clamp(0, 11) as usize], tm.tm_mday)
}

pub fn fmt_playtime(m: Option<i64>) -> Option<String> {
    let m = m.filter(|m| *m > 0)?;
    Some(if m < 60 { format!("{m} min played") } else { format!("{} h played", (m as f64 / 60.0).round()) })
}

#[derive(Clone, Debug, Default)]
pub struct Live {
    pub sui_usd: Option<f64>,
    pub sui_change: Option<f64>,
    pub tps: Option<i64>,
    pub gas: Option<u64>,
}

/// the Trade API's token price and rare.market's status, as live.ts polls them.
pub fn fetch_live() -> Live {
    let mut l = Live::default();
    let get = |url: &str| -> Option<serde_json::Value> { ureq::get(url).header("User-Agent", "RareLauncher/0.1").call().ok()?.body_mut().read_json().ok() };
    if let Some(v) = get("https://rare.market/v1/trade/ext/token/0x2::sui::SUI") {
        l.sui_usd = v["priceUsd"].as_f64();
        l.sui_change = v["change24h"].as_f64();
    }
    if let Some(v) = get("https://rare.market/api/v1/status") {
        l.tps = v["tps"].as_i64();
        l.sui_usd = l.sui_usd.or(v["suiUsd"].as_f64());
    }
    // The launcher reads it over gRPC (sui/chain.ts); the native port uses sui-rpc. Here: GraphQL.
    let body = r#"{"query":"{ epoch { referenceGasPrice } }"}"#;
    if let Some(v) = ureq::post("https://graphql.mainnet.sui.io/graphql").header("Content-Type", "application/json").send(body).ok().and_then(|mut r| r.body_mut().read_json::<serde_json::Value>().ok()) {
        l.gas = v["data"]["epoch"]["referenceGasPrice"].as_str().and_then(|s| s.parse().ok());
    }
    l
}

/// "$0.922": the Trade API's price rule, three significant digits.
pub fn fmt_price(v: f64) -> String {
    if v >= 100.0 {
        format!("${v:.0}")
    } else if v >= 10.0 {
        format!("${v:.1}")
    } else if v >= 1.0 {
        format!("${v:.2}")
    } else {
        format!("${v:.3}")
    }
}

pub fn fmt_change(v: f64) -> String {
    format!("{}{:.2}%", if v >= 0.0 { "+" } else { "\u{2212}" }, v.abs())
}
