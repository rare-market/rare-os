//! Rare's image pipeline. Every picture the UI shows (game covers and heroes from the Steam library
//! cache, Sui Games art, collectible images) goes through here once:
//!
//! - fetched once (HTTP, or read from a local file such as Steam's `librarycache`);
//! - resized to the size the UI draws it at ([`Size`]: thumb, tile, card, hero) and stored on disk
//!   as lossy WebP, so a 3840 px hero never reaches the web engine or the GPU at full size again;
//! - kept as encoded bytes in an in-memory LRU, so a screen that comes back gets its images without
//!   touching the disk;
//! - preloaded: [`Images::preload`] for what a screen is about to show, [`Images::warm`] at start
//!   for what the last session showed most recently.
//!
//! Animated GIF and WebP, and formats this crate does not decode (AVIF, SVG), pass through
//! untouched but are still cached. Requests for the same picture share one fetch, and a source that
//! failed is not retried for a couple of minutes.
//!
//! No Tauri here: the launcher serves [`Images::get`] over its `rimg://` protocol
//! ([`uri_path`] / [`parse_uri_path`] are the URL format) and a native UI can call it directly.

use base64::Engine;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// The sizes the UI draws art at, by width in physical pixels (height follows the aspect).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Size {
    /// Small item art: rows, lists, rails of collectibles (256 px).
    Thumb,
    /// Library and Home cover tiles, 2:3 (400 px).
    Tile,
    /// Wide cards and capsules (720 px).
    Card,
    /// Full-width page art (1920 px, the SuiPlay's width).
    Hero,
    /// The original bytes, cached but untouched.
    Orig,
    /// Any width, snapped up to the next of [`Size::BUCKETS`] so variants stay few.
    W(u32),
}

impl Size {
    pub const BUCKETS: [u32; 12] = [64, 128, 192, 256, 400, 512, 720, 960, 1280, 1600, 1920, 2560];

    pub fn width(self) -> Option<u32> {
        match self {
            Size::Thumb => Some(256),
            Size::Tile => Some(400),
            Size::Card => Some(720),
            Size::Hero => Some(1920),
            Size::Orig => None,
            Size::W(n) => Some(Self::snap(n)),
        }
    }

    fn snap(n: u32) -> u32 {
        Self::BUCKETS.iter().copied().find(|b| *b >= n).unwrap_or(2560)
    }

    pub fn name(self) -> String {
        match self {
            Size::Thumb => "thumb".into(),
            Size::Tile => "tile".into(),
            Size::Card => "card".into(),
            Size::Hero => "hero".into(),
            Size::Orig => "orig".into(),
            Size::W(n) => format!("w{}", Self::snap(n)),
        }
    }

    pub fn parse(s: &str) -> Option<Size> {
        Some(match s {
            "thumb" => Size::Thumb,
            "tile" => Size::Tile,
            "card" => Size::Card,
            "hero" => Size::Hero,
            "orig" => Size::Orig,
            _ => Size::W(Self::snap(s.strip_prefix('w')?.parse().ok()?)),
        })
    }
}

/// Where a picture comes from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    Url(String),
    File(PathBuf),
}

impl Source {
    /// `http(s)://...` is a URL, an absolute path a file; anything else is not a source.
    pub fn parse(s: &str) -> Option<Source> {
        if s.starts_with("https://") || s.starts_with("http://") {
            Some(Source::Url(s.to_string()))
        } else if s.starts_with('/') {
            Some(Source::File(PathBuf::from(s)))
        } else {
            None
        }
    }

    pub fn as_string(&self) -> String {
        match self {
            Source::Url(u) => u.clone(),
            Source::File(p) => p.to_string_lossy().into_owned(),
        }
    }
}

/// `<size>/<base64url(source)>`: the path part of a `rimg://localhost/...` URL.
pub fn uri_path(source: &str, size: Size) -> String {
    format!("{}/{}", size.name(), base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(source.as_bytes()))
}

/// The inverse of [`uri_path`]; a leading `/` and a query string are ignored.
pub fn parse_uri_path(path: &str) -> Option<(Source, Size)> {
    let path = path.trim_start_matches('/');
    let path = path.split(['?', '#']).next()?;
    let (size, b64) = path.split_once('/')?;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(b64.trim_end_matches('=')).ok()?;
    Some((Source::parse(std::str::from_utf8(&raw).ok()?)?, Size::parse(size)?))
}

/// Encoded image bytes ready to hand to a web engine or a decoder.
#[derive(Clone, Debug)]
pub struct Image {
    pub bytes: Arc<[u8]>,
    pub mime: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The file is not there, or the server said 404.
    NotFound,
    /// A file outside what the host allows.
    NotAllowed,
    /// Network or server trouble (retry later).
    Network(String),
    /// The bytes are not a picture.
    NotImage,
    Io(String),
}

impl Error {
    /// An HTTP status for a protocol handler.
    pub fn status(&self) -> u16 {
        match self {
            Error::NotFound => 404,
            Error::NotAllowed => 403,
            Error::Network(_) => 502,
            Error::NotImage => 415,
            Error::Io(_) => 500,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotFound => f.write_str("not found"),
            Error::NotAllowed => f.write_str("not allowed"),
            Error::Network(e) => write!(f, "network: {e}"),
            Error::NotImage => f.write_str("not an image"),
            Error::Io(e) => write!(f, "io: {e}"),
        }
    }
}

impl std::error::Error for Error {}

pub type FileFilter = dyn Fn(&Path) -> bool + Send + Sync;

#[derive(Clone)]
pub struct Config {
    /// Disk cache (`~/.cache/rare/img`).
    pub dir: PathBuf,
    /// Encoded bytes kept in memory.
    pub mem_bytes: usize,
    /// Disk cache size; the least recently used files go at open.
    pub disk_bytes: u64,
    /// Background (preload) threads. Foreground gets run on the caller's thread.
    pub workers: usize,
    /// WebP quality, 0-100.
    pub quality: f32,
    pub timeout: Duration,
    pub user_agent: String,
    pub max_download: u64,
    /// Which local files may be read. None allows any absolute path.
    pub allow_file: Option<Arc<FileFilter>>,
}

impl Config {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Config {
            dir: dir.into(),
            mem_bytes: 320 << 20,
            disk_bytes: 3 << 30,
            workers: 2,
            quality: 82.0,
            timeout: Duration::from_secs(20),
            user_agent: "RareLauncher/0.1".into(),
            max_download: 48 << 20,
            allow_file: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub mem_entries: usize,
    pub mem_bytes: usize,
    pub mem_hits: u64,
    pub disk_hits: u64,
    pub produced: u64,
    pub passthrough: u64,
    pub fetches: u64,
    pub failures: u64,
    /// Total ms spent decoding, resizing and encoding.
    pub produce_ms: u64,
}

// ---------- memory LRU ----------

struct Lru {
    map: HashMap<u64, (Image, u64)>,
    order: BTreeMap<u64, u64>,
    tick: u64,
    bytes: usize,
    cap: usize,
}

impl Lru {
    fn get(&mut self, k: u64) -> Option<Image> {
        let (img, t) = self.map.get_mut(&k)?;
        self.order.remove(t);
        self.tick += 1;
        *t = self.tick;
        self.order.insert(self.tick, k);
        Some(img.clone())
    }

    fn put(&mut self, k: u64, img: Image) {
        if img.bytes.len() > self.cap / 4 {
            return;
        }
        if let Some((old, t)) = self.map.remove(&k) {
            self.order.remove(&t);
            self.bytes -= old.bytes.len();
        }
        self.tick += 1;
        self.bytes += img.bytes.len();
        self.order.insert(self.tick, k);
        self.map.insert(k, (img, self.tick));
        while self.bytes > self.cap {
            let Some((_, victim)) = self.order.pop_first() else { break };
            if let Some((v, _)) = self.map.remove(&victim) {
                self.bytes -= v.bytes.len();
            }
        }
    }
}

// ---------- the pipeline ----------

struct Flight {
    result: Mutex<Option<Result<Image, Error>>>,
    cv: Condvar,
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<(Source, Size)>,
    queued: HashSet<u64>,
    stop: bool,
}

#[derive(Default)]
struct Counters {
    mem_hits: AtomicU64,
    disk_hits: AtomicU64,
    produced: AtomicU64,
    passthrough: AtomicU64,
    fetches: AtomicU64,
    failures: AtomicU64,
    produce_ms: AtomicU64,
}

struct Inner {
    cfg: Config,
    mem: Mutex<Lru>,
    inflight: Mutex<HashMap<u64, Arc<Flight>>>,
    failed: Mutex<HashMap<u64, Instant>>,
    queue: Mutex<Queue>,
    queue_cv: Condvar,
    /// Most recently served pictures, newest last, persisted for [`Images::warm`].
    hot: Mutex<(VecDeque<String>, bool)>,
    agent: ureq::Agent,
    n: Counters,
}

/// The image pipeline. Cheap to clone; clones share one cache.
#[derive(Clone)]
pub struct Images {
    inner: Arc<Inner>,
}

const RETRY_FAILED: Duration = Duration::from_secs(120);
const HOT_MAX: usize = 800;

fn fnv(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for p in parts {
        for b in *p {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// The content type of encoded image bytes, from their signature.
pub fn sniff(b: &[u8]) -> Option<&'static str> {
    if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if b.len() >= 12 && &b[4..8] == b"ftyp" && (&b[8..12] == b"avif" || &b[8..12] == b"avis") {
        return Some("image/avif");
    }
    if b.starts_with(b"BM") {
        return Some("image/bmp");
    }
    let head = String::from_utf8_lossy(&b[..b.len().min(256)]).to_ascii_lowercase();
    let head = head.trim_start_matches('\u{feff}').trim_start();
    if head.starts_with("<svg") || (head.starts_with("<?xml") && head.contains("<svg")) {
        return Some("image/svg+xml");
    }
    None
}

/// An animated WebP (VP8X with the animation flag) or GIF with more than one frame: kept as is.
fn animated(b: &[u8], mime: &str) -> bool {
    match mime {
        "image/webp" => b.len() > 20 && &b[12..16] == b"VP8X" && b[20] & 0x02 != 0,
        "image/gif" => b.windows(3).filter(|w| *w == [0x00, 0x21, 0xF9]).count() > 1,
        _ => false,
    }
}

impl Images {
    /// Opens the cache in `cfg.dir` (trimming it to `cfg.disk_bytes` in the background) and starts
    /// the preload threads.
    pub fn open(cfg: Config) -> std::io::Result<Images> {
        std::fs::create_dir_all(cfg.dir.join("v1"))?;
        let agent = ureq::AgentBuilder::new().timeout(cfg.timeout).user_agent(&cfg.user_agent).max_idle_connections_per_host(6).build();
        let hot = read_hot(&cfg.dir);
        let inner = Arc::new(Inner {
            mem: Mutex::new(Lru { map: HashMap::new(), order: BTreeMap::new(), tick: 0, bytes: 0, cap: cfg.mem_bytes }),
            inflight: Mutex::new(HashMap::new()),
            failed: Mutex::new(HashMap::new()),
            queue: Mutex::new(Queue::default()),
            queue_cv: Condvar::new(),
            hot: Mutex::new((hot, false)),
            agent,
            n: Counters::default(),
            cfg,
        });
        let imgs = Images { inner };
        for i in 0..imgs.inner.cfg.workers.max(1) {
            let s = imgs.clone();
            std::thread::Builder::new().name(format!("rare-img-{i}")).spawn(move || s.worker())?;
        }
        let (dir, cap) = (imgs.inner.cfg.dir.join("v1"), imgs.inner.cfg.disk_bytes);
        std::thread::Builder::new().name("rare-img-trim".into()).spawn(move || trim_disk(&dir, cap))?;
        Ok(imgs)
    }

    /// The picture at this size: from memory, else disk, else fetched and made now (the caller
    /// waits; concurrent callers for the same picture share the work).
    pub fn get(&self, src: &Source, size: Size) -> Result<Image, Error> {
        let key = self.key(src, size)?;
        self.note_hot(src, size);
        if let Some(img) = self.inner.mem.lock().unwrap().get(key) {
            self.inner.n.mem_hits.fetch_add(1, Ordering::Relaxed);
            return Ok(img);
        }
        self.load_or_make(key, src, size)
    }

    /// Memory or disk only, never the network. Puts a disk hit into memory.
    pub fn cached(&self, src: &Source, size: Size) -> Option<Image> {
        let key = self.key(src, size).ok()?;
        if let Some(img) = self.inner.mem.lock().unwrap().get(key) {
            return Some(img);
        }
        let img = self.from_disk(key)?;
        self.inner.mem.lock().unwrap().put(key, img.clone());
        Some(img)
    }

    /// True when the picture is in memory now (the next get costs no I/O).
    pub fn in_memory(&self, src: &Source, size: Size) -> bool {
        self.key(src, size).is_ok_and(|k| self.inner.mem.lock().unwrap().map.contains_key(&k))
    }

    /// Queues pictures to be made and held in memory, in order, on the background threads.
    pub fn preload(&self, items: impl IntoIterator<Item = (Source, Size)>) {
        let mut q = self.inner.queue.lock().unwrap();
        for (src, size) in items {
            let Ok(key) = self.key(&src, size) else { continue };
            if q.queued.contains(&key) || self.inner.mem.lock().unwrap().map.contains_key(&key) {
                continue;
            }
            q.queued.insert(key);
            q.jobs.push_back((src, size));
        }
        drop(q);
        self.inner.queue_cv.notify_all();
    }

    /// Preloads the `n` pictures served most recently in earlier sessions (newest first): the
    /// first screen of every tab is in memory before it is opened.
    pub fn warm(&self, n: usize) -> usize {
        let items: Vec<(Source, Size)> = {
            let hot = self.inner.hot.lock().unwrap();
            hot.0.iter().rev().take(n).filter_map(|l| {
                let (size, src) = l.split_once('|')?;
                Some((Source::parse(src)?, Size::parse(size)?))
            }).collect()
        };
        let len = items.len();
        self.preload(items);
        len
    }

    /// Writes the recently-served list for the next start's [`Images::warm`]. Cheap when unchanged.
    pub fn save_hot(&self) {
        let lines = {
            let mut hot = self.inner.hot.lock().unwrap();
            if !hot.1 {
                return;
            }
            hot.1 = false;
            hot.0.iter().cloned().collect::<Vec<_>>().join("\n")
        };
        let path = self.inner.cfg.dir.join("hot.txt");
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, lines).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    pub fn stats(&self) -> Stats {
        let mem = self.inner.mem.lock().unwrap();
        let n = &self.inner.n;
        Stats {
            mem_entries: mem.map.len(),
            mem_bytes: mem.bytes,
            mem_hits: n.mem_hits.load(Ordering::Relaxed),
            disk_hits: n.disk_hits.load(Ordering::Relaxed),
            produced: n.produced.load(Ordering::Relaxed),
            passthrough: n.passthrough.load(Ordering::Relaxed),
            fetches: n.fetches.load(Ordering::Relaxed),
            failures: n.failures.load(Ordering::Relaxed),
            produce_ms: n.produce_ms.load(Ordering::Relaxed),
        }
    }

    /// Stops the preload threads after saving the recently-served list.
    pub fn shutdown(&self) {
        self.save_hot();
        self.inner.queue.lock().unwrap().stop = true;
        self.inner.queue_cv.notify_all();
    }

    // ---------- internals ----------

    /// The cache key. A file's key includes its size and mtime, so a replaced cover is made again.
    fn key(&self, src: &Source, size: Size) -> Result<u64, Error> {
        let sz = size.name();
        match src {
            Source::Url(u) => Ok(fnv(&[b"u", u.as_bytes(), sz.as_bytes()])),
            Source::File(p) => {
                if !p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                    return Err(Error::NotAllowed);
                }
                if let Some(f) = &self.inner.cfg.allow_file {
                    if !f(p) {
                        return Err(Error::NotAllowed);
                    }
                }
                let md = std::fs::metadata(p).map_err(|_| Error::NotFound)?;
                let mt = md.modified().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
                Ok(fnv(&[b"f", p.as_os_str().as_encoded_bytes(), sz.as_bytes(), &mt.to_le_bytes(), &md.len().to_le_bytes()]))
            }
        }
    }

    fn note_hot(&self, src: &Source, size: Size) {
        let line = format!("{}|{}", size.name(), src.as_string());
        if line.contains('\n') {
            return;
        }
        let mut hot = self.inner.hot.lock().unwrap();
        if hot.0.back() == Some(&line) {
            return;
        }
        if let Some(i) = hot.0.iter().position(|l| *l == line) {
            hot.0.remove(i);
        }
        hot.0.push_back(line);
        while hot.0.len() > HOT_MAX {
            hot.0.pop_front();
        }
        hot.1 = true;
    }

    fn disk_path(&self, key: u64) -> PathBuf {
        self.inner.cfg.dir.join("v1").join(format!("{:02x}", key >> 56)).join(format!("{key:016x}"))
    }

    fn from_disk(&self, key: u64) -> Option<Image> {
        let path = self.disk_path(key);
        let bytes = std::fs::read(&path).ok()?;
        let mime = sniff(&bytes)?;
        self.inner.n.disk_hits.fetch_add(1, Ordering::Relaxed);
        // Used: keep it from being trimmed (at most one mtime write a day per file).
        if let Ok(f) = std::fs::File::options().write(true).open(&path) {
            let old = f.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|e| e > Duration::from_secs(86_400));
            if old {
                let _ = f.set_modified(SystemTime::now());
            }
        }
        Some(Image { bytes: bytes.into(), mime })
    }

    fn load_or_make(&self, key: u64, src: &Source, size: Size) -> Result<Image, Error> {
        let (flight, leader) = {
            let mut inf = self.inner.inflight.lock().unwrap();
            match inf.get(&key) {
                Some(f) => (f.clone(), false),
                None => {
                    let f = Arc::new(Flight { result: Mutex::new(None), cv: Condvar::new() });
                    inf.insert(key, f.clone());
                    (f, true)
                }
            }
        };
        if !leader {
            let mut r = flight.result.lock().unwrap();
            while r.is_none() {
                r = flight.cv.wait(r).unwrap();
            }
            return r.clone().unwrap();
        }
        let res = match self.from_disk(key) {
            Some(img) => Ok(img),
            None => self.make(key, src, size),
        };
        if let Ok(img) = &res {
            self.inner.mem.lock().unwrap().put(key, img.clone());
        }
        *flight.result.lock().unwrap() = Some(res.clone());
        flight.cv.notify_all();
        self.inner.inflight.lock().unwrap().remove(&key);
        res
    }

    fn make(&self, key: u64, src: &Source, size: Size) -> Result<Image, Error> {
        if let Some(t) = self.inner.failed.lock().unwrap().get(&key) {
            if t.elapsed() < RETRY_FAILED {
                return Err(Error::NotFound);
            }
        }
        let res = self.fetch(src).and_then(|raw| self.shape(raw, size));
        match &res {
            Ok(img) => {
                let path = self.disk_path(key);
                let _ = std::fs::create_dir_all(path.parent().unwrap());
                let tmp = path.with_extension("tmp");
                let ok = std::fs::File::create(&tmp).and_then(|mut f| f.write_all(&img.bytes)).and_then(|_| std::fs::rename(&tmp, &path));
                if ok.is_err() {
                    let _ = std::fs::remove_file(&tmp);
                }
            }
            Err(Error::NotAllowed) => {}
            Err(_) => {
                self.inner.n.failures.fetch_add(1, Ordering::Relaxed);
                let mut f = self.inner.failed.lock().unwrap();
                if f.len() > 4096 {
                    f.retain(|_, t| t.elapsed() < RETRY_FAILED);
                }
                f.insert(key, Instant::now());
            }
        }
        res
    }

    fn fetch(&self, src: &Source) -> Result<Vec<u8>, Error> {
        match src {
            Source::File(p) => std::fs::read(p).map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { Error::NotFound } else { Error::Io(e.to_string()) }),
            Source::Url(u) => {
                self.inner.n.fetches.fetch_add(1, Ordering::Relaxed);
                let resp = match self.inner.agent.get(u).set("Accept", "image/webp,image/avif,image/*;q=0.8").call() {
                    Ok(r) => r,
                    Err(ureq::Error::Status(404 | 410, _)) => return Err(Error::NotFound),
                    Err(e) => return Err(Error::Network(e.to_string())),
                };
                let mut buf = Vec::new();
                resp.into_reader().take(self.inner.cfg.max_download).read_to_end(&mut buf).map_err(|e| Error::Network(e.to_string()))?;
                Ok(buf)
            }
        }
    }

    /// Resizes and re-encodes `raw` for `size`, or passes it through when that would not help.
    fn shape(&self, raw: Vec<u8>, size: Size) -> Result<Image, Error> {
        let mime = sniff(&raw).ok_or(Error::NotImage)?;
        let pass = |raw: Vec<u8>| {
            self.inner.n.passthrough.fetch_add(1, Ordering::Relaxed);
            Ok(Image { bytes: raw.into(), mime })
        };
        let Some(target) = size.width() else { return pass(raw) };
        let format = match mime {
            "image/webp" => image::ImageFormat::WebP,
            "image/jpeg" => image::ImageFormat::Jpeg,
            "image/png" => image::ImageFormat::Png,
            "image/gif" => image::ImageFormat::Gif,
            "image/bmp" => image::ImageFormat::Bmp,
            _ => return pass(raw),
        };
        if animated(&raw, mime) {
            return pass(raw);
        }
        let t = Instant::now();
        let Ok(img) = image::load_from_memory_with_format(&raw, format) else {
            // Bytes that say they are an image but do not decode: let the web engine try.
            return pass(raw);
        };
        let (w, h) = (img.width(), img.height());
        // Already small and already compact: keep the original.
        if w <= target && matches!(mime, "image/webp" | "image/jpeg") {
            return pass(raw);
        }
        let img = if w > target {
            let nh = ((h as f64) * (target as f64) / (w as f64)).round().max(1.0) as u32;
            if w >= target * 2 {
                // Area averaging: fast and clean for big reductions (a 3840 hero to 1920).
                image::DynamicImage::ImageRgba8(image::imageops::thumbnail(&img, target, nh))
            } else {
                img.resize_exact(target, nh, image::imageops::FilterType::Triangle)
            }
        } else {
            img
        };
        let alpha = img.color().has_alpha() && img.to_rgba8().pixels().any(|p| p.0[3] < 255);
        let (w, h) = (img.width(), img.height());
        let mut cfg = webp::WebPConfig::new().map_err(|_| Error::Io("webp config".into()))?;
        cfg.quality = self.inner.cfg.quality;
        cfg.method = 3;
        cfg.alpha_compression = 1;
        let mem = if alpha {
            let rgba = img.to_rgba8();
            webp::Encoder::from_rgba(&rgba, w, h).encode_advanced(&cfg)
        } else {
            let rgb = img.to_rgb8();
            webp::Encoder::from_rgb(&rgb, w, h).encode_advanced(&cfg)
        }
        .map_err(|e| Error::Io(format!("webp: {e:?}")))?;
        self.inner.n.produced.fetch_add(1, Ordering::Relaxed);
        self.inner.n.produce_ms.fetch_add(t.elapsed().as_millis() as u64, Ordering::Relaxed);
        Ok(Image { bytes: mem.to_vec().into(), mime: "image/webp" })
    }

    fn worker(&self) {
        loop {
            let job = {
                let mut q = self.inner.queue.lock().unwrap();
                loop {
                    if q.stop {
                        return;
                    }
                    if let Some(j) = q.jobs.pop_front() {
                        break j;
                    }
                    q = self.inner.queue_cv.wait(q).unwrap();
                }
            };
            let (src, size) = job;
            if let Ok(key) = self.key(&src, size) {
                if !self.inner.mem.lock().unwrap().map.contains_key(&key) {
                    let _ = self.load_or_make(key, &src, size);
                }
                self.inner.queue.lock().unwrap().queued.remove(&key);
            }
        }
    }
}

fn read_hot(dir: &Path) -> VecDeque<String> {
    std::fs::read_to_string(dir.join("hot.txt")).map(|s| s.lines().filter(|l| !l.is_empty()).map(String::from).collect()).unwrap_or_default()
}

/// Deletes the least recently used files until the cache is under 90% of `cap`.
fn trim_disk(dir: &Path, cap: u64) {
    let mut files = Vec::new();
    let mut total = 0u64;
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for sub in rd.flatten() {
        let Ok(rd2) = std::fs::read_dir(sub.path()) else { continue };
        for f in rd2.flatten() {
            let Ok(md) = f.metadata() else { continue };
            if f.path().extension().is_some_and(|e| e == "tmp") {
                let _ = std::fs::remove_file(f.path());
                continue;
            }
            total += md.len();
            files.push((md.modified().unwrap_or(SystemTime::UNIX_EPOCH), md.len(), f.path()));
        }
    }
    if total <= cap {
        return;
    }
    files.sort();
    for (_, len, p) in files {
        if total <= cap / 10 * 9 {
            break;
        }
        if std::fs::remove_file(&p).is_ok() {
            total -= len;
        }
    }
}

#[cfg(test)]
mod tests;
