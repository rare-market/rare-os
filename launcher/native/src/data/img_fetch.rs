//! The image loader the launcher runs: rare-render's ImageLoader with its disk cache under
//! ~/.cache/rare/img/native (512 MB LRU of compressed bytes: the renditions as fetched, ~20-40 KB
//! each, so it holds over ten thousand pictures) and an HTTP/2 client (one connection to
//! rare.market carries every image request; ~8 in flight).
//!
//! Each link gives way like on rare.market (components/ui/media.tsx): our own rendition (/img/src)
//! after 2.5 s, the wsrv front (/img/w) after 7 s, wsrv.nl after 5 s, anything else after 12 s, so
//! a slow link moves on to the next one of the chain instead of holding the tile. An AVIF or HEIF
//! answer (some hosts send nothing else) is converted to WebP by rare.market's front.

use rare_render::images::{Fetcher, ACCEPT};
use rare_render::ImageLoader;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// The disk cache: $RR_IMG_CACHE, else ~/.cache/rare/img/native.
pub fn cache_dir() -> Option<PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(find_cache_dir).clone()
}

fn find_cache_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("RR_IMG_CACHE") {
        return Some(PathBuf::from(d));
    }
    let base = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    // The cache before this one (raw pixels with no cap): gone, once.
    let legacy = base.join("rare-render").join("img");
    if legacy.is_dir() {
        let _ = std::thread::Builder::new().name("rr-img-legacy".into()).spawn(move || {
            let _ = std::fs::remove_dir_all(legacy);
        });
    }
    Some(base.join("rare").join("img").join("native"))
}

/// The launcher's loader: `threads` decode workers, the HTTP/2 fetcher, the disk cache.
pub fn loader(threads: usize, waker: Arc<dyn Fn() + Send + Sync>) -> ImageLoader {
    // Disk hits are decoded (WebP/JPEG), not read back as pixels: a third of the cores (5 on the
    // SuiPlay's 16 threads), never fewer than asked.
    let threads = threads.max(std::thread::available_parallelism().map(|n| n.get() / 3).unwrap_or(3).clamp(3, 6));
    let mut l = ImageLoader::new(threads, waker, cache_dir());
    if let Some(f) = fetcher() {
        l.set_fetcher(f);
    }
    l
}

fn runtime() -> Option<&'static tokio::runtime::Runtime> {
    static RT: OnceLock<Option<tokio::runtime::Runtime>> = OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("rr-img-http").enable_all().build().ok()).as_ref()
}

/// How long a link may take before the chain moves on (rare.market's SafeImg timings).
fn give_way(url: &str) -> Duration {
    if url.contains("rare.market/img/src/") {
        Duration::from_millis(2500)
    } else if url.contains("rare.market/img/w/") {
        Duration::from_secs(7)
    } else if url.starts_with("https://wsrv.nl/") {
        Duration::from_secs(5)
    } else {
        Duration::from_secs(12)
    }
}

fn ours(url: &str) -> bool {
    url.starts_with("https://rare.market/") || url.starts_with("https://wsrv.nl/")
}

fn is_avif(b: &[u8]) -> bool {
    b.len() >= 12 && &b[4..8] == b"ftyp" && matches!(&b[8..12], b"avif" | b"avis" | b"heic" | b"heix" | b"mif1" | b"msf1")
}

const MAX_BYTES: usize = 32 << 20;

async fn get(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let mut rq = client.get(url).timeout(give_way(url));
    if ours(url) {
        rq = rq.header("accept", ACCEPT);
        if let Some(t) = rare_market::net::test_token(url) {
            rq = rq.header("x-rare-test", t);
        }
    } else {
        // Originals as a browser would ask (some hosts refuse anything else).
        for (k, v) in rare_market::media::browser_headers(url) {
            rq = rq.header(k, if v.starts_with("image/avif") { ACCEPT.to_string() } else { v });
        }
    }
    let t0 = std::time::Instant::now();
    let resp = rq.send().await.map_err(|e| if e.is_timeout() { "timed out".to_string() } else { e.to_string() })?;
    let status = resp.status();
    rare_market::net::trace(t0, status.as_u16(), 0, url);
    if !status.is_success() {
        return Err(format!("http status: {}", status.as_u16()));
    }
    if resp.content_length().is_some_and(|n| n as usize > MAX_BYTES) {
        return Err("too large".into());
    }
    let b = resp.bytes().await.map_err(|e| if e.is_timeout() { "timed out".to_string() } else { e.to_string() })?;
    if b.len() > MAX_BYTES {
        return Err("too large".into());
    }
    Ok(b.to_vec())
}

fn fetcher() -> Option<Fetcher> {
    let rt = runtime()?;
    let client = reqwest::Client::builder()
        .user_agent("RareLauncher/0.1")
        .connect_timeout(Duration::from_secs(5))
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(8)
        .http2_adaptive_window(true)
        .build()
        .ok()?;
    Some(Arc::new(move |url: &str, w: u32, h: u32| {
        rt.block_on(async {
            let b = get(&client, url).await?;
            if is_avif(&b) && !ours(url) {
                // Decoded by rare.market's front as WebP at the drawn size.
                let px = w.max(h).max(64) as f64;
                return get(&client, &rare_market::media::wsrv_url(url, px, rare_market::media::Fit::Cover)).await;
            }
            Ok(b)
        })
    }))
}
