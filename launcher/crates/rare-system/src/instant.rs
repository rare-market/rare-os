//! The instant layer: the launcher's use of `rare-state` (data cache) and `rare-images` (image
//! pipeline), both in launcher/crates so the native UI can use them as they are.
//!
//! - Every rare.market read (`rare_get`) and the Trade API's read-only paths (`state_get`) go
//!   through the data cache: display data answers from memory or disk at once and revalidates
//!   behind (Policy below), and the page hears about keys that changed (`rare-state` event).
//! - `state_boot` hands the page what it used most recently in one call before its first render.
//! - `state_put` keeps state the page computes itself (`ui:` keys) for the next start.
//! - `rimg://localhost/<size>/<base64url(source)>` serves art (URLs and local files such as Steam's
//!   librarycache) resized, as WebP, from memory, disk or the network.
//! - At start, once the window is up, recently used data is revalidated and recently shown images
//!   are put in memory. Background fetches wait while a game has the screen.
//!
//! Caches live in ~/.cache/rare/state and ~/.cache/rare/img.

use rare_images::{Images, Size, Source};
use rare_state::{Freshness, Policy, Request, Store};
use serde_json::value::RawValue;
use std::path::{Path, PathBuf};
use std::time::Duration;
use crate::network::NetworkCfg;

#[derive(Clone)]
pub struct Instant {
    pub state: Store,
    pub images: Images,
}

const MARKET: &str = "https://rare.market";
const TRADE: &str = "https://rare.market/v1/trade";
/// Trade API paths that are plain reads (the rest of trade_api posts or needs auth).
const TRADE_READS: [&str; 7] = ["/ext/", "/token/", "/tokens?", "/perps?l=1", "/search?", "/leaderboard?", "/leaderboard/extras?"];
/// Older than this, display data is fetched before it is shown (with the cached copy if offline):
/// a launcher not opened for a day shows that day's prices, not last week's.
const MAX_STALE: Duration = Duration::from_secs(6 * 3600);

pub fn root() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join(".cache")).join("rare")
}

/// How long each kind of data may be shown before it is refetched. Data a transaction is built from
/// (an item's listing, offers, kiosks, transfer policies, the gas sponsor) is always fresh.
pub fn policy() -> Policy {
    Policy::new(Freshness::Fresh { ttl: Duration::from_secs(5), offline_ok: false })
        .rule("/v1/items/", Freshness::Fresh { ttl: Duration::from_secs(5), offline_ok: false })
        .rule("/v1/offers", Freshness::fresh_secs(0))
        .rule("/kiosks", Freshness::fresh_secs(0))
        .rule("/v1/policies/", Freshness::fresh_secs(30))
        .rule("/v1/sponsor", Freshness::fresh_secs(0))
        .rule("/v1/store/", Freshness::fresh_secs(10))
        .rule("/v1/status", Freshness::swr_secs(30))
        .rule("/v1/events", Freshness::swr_secs(20))
        .rule("/v1/collections", Freshness::swr_secs(60))
        .rule("/v1/accounts/", Freshness::swr_secs(30))
        .rule("/v1/search", Freshness::swr_secs(120))
        .rule("/v1/games", Freshness::swr_secs(300))
        .rule("/games/games.json", Freshness::swr_secs(300))
        .rule("/v1/home", Freshness::swr_secs(300))
        .rule("/v1/drops", Freshness::swr_secs(300))
        .rule("/launchpad/", Freshness::swr_secs(300))
        .rule("/ext/token/", Freshness::swr_secs(20))
        .rule("/ext/perps/candles", Freshness::swr_secs(300))
        .rule("/ext/portfolio/", Freshness::swr_secs(60))
        .rule("/ext/", Freshness::swr_secs(60))
        .rule("/tokens?", Freshness::swr_secs(30))
        .rule("/token/", Freshness::swr_secs(30))
        .rule("/perps?l=1", Freshness::swr_secs(30))
        .rule("/leaderboard", Freshness::swr_secs(120))
        .rule("/search?", Freshness::swr_secs(60))
}

/// Keys: `rare:<path>` (mainnet rare.market), `rare:<full url>` (testnet or an override, already
/// checked by network::market_url), `trade:<path>` (Trade reads). Anything else is local state.
fn resolve(key: &str) -> Option<Request> {
    let get = |url: String| Some(Request { url, headers: vec![("User-Agent".into(), "RareLauncher/0.1".into())] });
    if let Some(rest) = key.strip_prefix("rare:") {
        return if rest.starts_with('/') { get(format!("{MARKET}{rest}")) } else if rest.starts_with("http") { get(rest.to_string()) } else { None };
    }
    if let Some(path) = key.strip_prefix("trade:") {
        return trade_read(path).then(|| format!("{TRADE}{path}")).and_then(get);
    }
    None
}

pub fn market_path_ok(path: &str) -> bool {
    (path.starts_with("/v1/") || path.starts_with("/games/") || path == "/launchpad/launches.json") && !path.contains("..")
}

fn trade_read(path: &str) -> bool {
    TRADE_READS.iter().any(|p| path.starts_with(p)) && !path.contains("..") && !path.contains('#')
}

/// The cache key for a rare.market read under the current network.
pub fn market_key(cfg: &impl NetworkCfg, path: &str) -> String {
    if crate::network::is_testnet(cfg) {
        format!("rare:{}", crate::network::market_url(cfg, path))
    } else {
        format!("rare:{path}")
    }
}

/// Local files art may come from: the asset protocol's scope (tauri.conf.json), image files only.
fn file_allowed(p: &Path) -> bool {
    let image = p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "png" | "webp" | "gif" | "avif" | "bmp" | "svg" | "ico"));
    let home = dirs::home_dir();
    let roots = ["/home/", "/var/home/", "/run/media/", "/media/", "/mnt/", "/var/mnt/", "/usr/share/rare/"];
    image && (home.is_some_and(|h| p.starts_with(h)) || roots.iter().any(|r| p.starts_with(r)))
}

/// Opens both caches. Called before the window loads, so the first command already has them.
pub fn open() -> Instant {
    let dir = root();
    let t = std::time::Instant::now();
    let state = Store::open(rare_state::Config { dir: Some(dir.join("state")), ..Default::default() }, policy(), resolve).unwrap_or_else(|e| {
        eprintln!("[instant] state cache on disk unavailable ({e}), memory only");
        Store::open(rare_state::Config::default(), policy(), resolve).expect("memory store")
    });
    let mut icfg = rare_images::Config::new(dir.join("img"));
    icfg.allow_file = Some(std::sync::Arc::new(file_allowed));
    let images = Images::open(icfg).unwrap_or_else(|e| {
        eprintln!("[instant] image cache dir unavailable ({e}), using /tmp");
        Images::open(rare_images::Config::new(std::env::temp_dir().join("rare-img"))).expect("image cache")
    });
    let s = state.stats();
    eprintln!("[startup] instant: {} cached responses ({} KB) loaded in {} ms", s.entries, s.bytes / 1024, t.elapsed().as_millis());
    Instant { state, images }
}

/// Hooks the caches to the front end: `on_change` hears keys that changed (the Tauri shell emits `rare-state` to its main window), the game gate,
/// start-up warming. `gaming` says whether a game has the screen (background fetches wait while
/// one does); `launcher_front` whether the launcher is in front; `since_start_ms` is for the log.
pub fn start(inst: &Instant, on_change: impl Fn(&str) + Send + Sync + 'static, gaming: impl Fn() -> bool + Send + Sync + Clone + 'static, launcher_front: impl Fn() -> bool + Send + Sync + 'static, since_start: impl Fn() -> String + Send + 'static) {
    inst.state.subscribe(move |key, _| {
        on_change(key);
    });
    let g = gaming.clone();
    inst.state.set_gate(move || !(g() && !launcher_front()));
    let (state, images) = (inst.state.clone(), inst.images.clone());
    std::thread::Builder::new()
        .name("instant-warm".into())
        .spawn(move || {
            // Images first (disk to memory only): the first screens draw from memory. Data after the
            // first frame and the library scan have had the network and the cores.
            std::thread::sleep(Duration::from_millis(400));
            let n = images.warm(320);
            std::thread::sleep(Duration::from_millis(1100));
            let m = if gaming() { 0 } else { state.warm(Duration::from_secs(3 * 86_400), 600) };
            eprintln!("[instant] warming {n} images, revalidating {m} responses at {} ms", since_start());
            loop {
                std::thread::sleep(Duration::from_secs(30));
                images.save_hot();
            }
        })
        .ok();
}

/// A display-data read: the policy, except that data older than MAX_STALE is fetched first.
pub fn read(state: &Store, key: &str) -> Result<Box<RawValue>, String> {
    let f = state.policy_for(key);
    let f = match (f, state.peek(key)) {
        (Freshness::Swr { .. }, Some(h)) if h.age() > MAX_STALE => Freshness::Fresh { ttl: Duration::ZERO, offline_ok: true },
        _ => f,
    };
    let hit = state.get_with(key, f)?;
    RawValue::from_string(hit.body.to_string()).map_err(|e| e.to_string())
}

// ---------- commands (blocking; a front end runs them where it likes) ----------

/// A cached read by key: `rare:<path>` or `trade:<read-only Trade path>` (fetched by policy) or
/// `ui:<name>` (what the page put). A memory hit answers at once; a miss waits for the network.
pub fn state_get(inst: &Instant, cfg: &impl NetworkCfg, key: String) -> Result<Box<RawValue>, String> {
    let state = &inst.state;
    let key = if let Some(path) = key.strip_prefix("rare:") {
        if !market_path_ok(path) {
            return Err("path not allowed".into());
        }
        market_key(cfg, path)
    } else if let Some(path) = key.strip_prefix("trade:") {
        if !trade_read(path) {
            return Err("path not allowed".into());
        }
        key
    } else if key.starts_with("ui:") {
        let hit = state.peek(&key).ok_or("not cached")?;
        return RawValue::from_string(hit.body.to_string()).map_err(|e| e.to_string());
    } else {
        return Err("unknown key".into());
    };
    read(state, &key)
}

/// True when `state_get` can answer on the calling thread (a fresh enough memory hit under a
/// stale-while-revalidate policy), so a front end only hops to a blocking thread on a miss.
pub fn state_get_is_instant(inst: &Instant, cfg: &impl NetworkCfg, key: &str) -> bool {
    let key = match key.strip_prefix("rare:") {
        Some(path) if market_path_ok(path) => market_key(cfg, path),
        Some(_) => return false,
        None => key.to_string(),
    };
    let state = &inst.state;
    state.peek(&key).is_some_and(|h| h.age() < MAX_STALE) && matches!(state.policy_for(&key), Freshness::Swr { .. })
}

/// What the page used most recently (rare:, trade: and ui: keys, up to 2 MB), as
/// `{ key: { at, v } }`, for it to render its first frame from.
pub fn state_boot(inst: &Instant) -> Result<Box<RawValue>, String> {
    let s = inst.state.hot_json("", Duration::from_secs(7 * 86_400), 2 << 20);
    RawValue::from_string(s).map_err(|e| e.to_string())
}

/// Keeps page state (`ui:` keys) for the next start.
pub fn state_put(inst: &Instant, key: String, value: Box<RawValue>) -> Result<(), String> {
    if !key.starts_with("ui:") || key.len() > 200 {
        return Err("only ui: keys".into());
    }
    if value.get().len() > 1 << 20 {
        return Err("too big".into());
    }
    inst.state.put(&key, value.get().to_string())
}

/// Makes images ready in memory ahead of a screen or a focus move: rimg:// URLs (or their paths).
pub fn img_preload(inst: &Instant, urls: Vec<String>) {
    let items: Vec<(Source, Size)> = urls.iter().take(200).filter_map(|u| rare_images::parse_uri_path(rimg_path(u))).collect();
    inst.images.preload(items);
}

#[derive(serde::Serialize)]
pub struct InstantStats {
    state: String,
    images: String,
}

pub fn instant_stats(i: &Instant) -> InstantStats {
    InstantStats { state: format!("{:?}", i.state.stats()), images: format!("{:?}", i.images.stats()) }
}

// ---------- rimg:// ----------

pub fn rimg_path(u: &str) -> &str {
    u.strip_prefix("rimg://localhost").or_else(|| u.strip_prefix("http://rimg.localhost")).unwrap_or(u)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_resolve_only_to_allowed_reads() {
        assert_eq!(resolve("rare:/v1/games").unwrap().url, "https://rare.market/v1/games");
        assert_eq!(resolve("rare:https://rare.market/v1/status?network=testnet").unwrap().url, "https://rare.market/v1/status?network=testnet");
        assert_eq!(resolve("trade:/ext/token/0x2::sui::SUI").unwrap().url, "https://rare.market/v1/trade/ext/token/0x2::sui::SUI");
        assert!(resolve("trade:/swap/build").is_none());
        assert!(resolve("trade:/users/0x1").is_none());
        assert!(resolve("ui:wallet").is_none());
        assert!(!market_path_ok("/v1/../admin"));
    }

    #[test]
    fn transaction_inputs_are_never_served_stale() {
        let p = policy();
        for k in ["rare:/v1/items/0x1", "rare:/v1/offers?item=0x1&limit=10", "rare:/v1/accounts/0x1/kiosks", "rare:/v1/sponsor/status?address=0x1", "rare:/v1/unknown"] {
            assert!(matches!(p.for_key(k), Freshness::Fresh { offline_ok: false, .. }), "{k}");
        }
        for k in ["rare:/v1/games", "rare:/v1/collections?window=24h", "rare:/v1/accounts/0x1/items?limit=60", "trade:/ext/token/0x2::sui::SUI"] {
            assert!(matches!(p.for_key(k), Freshness::Swr { .. }), "{k}");
        }
    }

    #[test]
    fn art_files_only_from_allowed_places() {
        assert!(file_allowed(Path::new("/home/deck/.local/share/Steam/appcache/librarycache/570/library_600x900.jpg")));
        assert!(file_allowed(Path::new("/usr/share/rare/art/x.webp")));
        assert!(!file_allowed(Path::new("/etc/shadow")));
        assert!(!file_allowed(Path::new("/home/deck/.ssh/id_ed25519")));
    }

    #[test]
    fn rimg_urls_parse_on_every_platform_form() {
        let p = rare_images::uri_path("https://x.example/a.png", Size::Tile);
        for u in [format!("rimg://localhost/{p}"), format!("http://rimg.localhost/{p}"), format!("/{p}")] {
            assert!(rare_images::parse_uri_path(rimg_path(&u)).is_some(), "{u}");
        }
    }
}
