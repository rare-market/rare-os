//! Images: fetched, decoded, cropped and resized off the main thread to the exact pixel size they
//! are drawn at, premultiplied, then uploaded as one texture each (small ones share an atlas). The
//! main thread only uploads finished images; nothing it does waits on the network or a decode.
//!
//! Pipeline:
//! - a disk probe first, on the decode workers: the disk keeps compressed bytes, not pixels. A
//!   fetched rendition is kept as it came (`.src`, keyed by URL: a 20-40 KB WebP, shared by every
//!   size that asks for the same link); a source much larger than the slot (a 3840 px Steam hero
//!   on a 1920 px backdrop, a multi-MB original from the end of the chain, a big local file) is
//!   kept resized to the slot and re-encoded (`.px`, keyed by source, size and fit; JPEG when
//!   opaque, else PNG). A hit is decoded on the worker; raw RGBA (`.rgba`, the old format, ~15x
//!   larger) is dropped by the first trim;
//! - a miss goes to the network workers ([`NET_WORKERS`] requests in flight, over the app's
//!   fetcher: [`ImageLoader::set_fetcher`]; data: URIs are decoded in place);
//! - fetched bytes go back to the decode workers: PNG, JPEG, WebP, GIF and BMP (the first frame of
//!   an animation), ICO, and SVG rasterised by resvg at the drawn size (with system fonts and
//!   embedded raster images in any format the image crate reads).
//!
//! Every queue serves what is on screen first: paint asks for each image it draws
//! ([`ImageLoader::state`]), and pending images drawn in the latest frame jump ahead of the ones
//! that are only built (the next rail page, a list's tail), which load after, in request order.
//!
//! The disk cache is an LRU capped at [`ImageLoader::set_disk_cap`] (512 MB by default); GPU
//! textures are trimmed by [`ImageLoader::trim`]. A failure is logged once per source with its
//! reason, and [`ImageLoader::error`] gives the reason to callers that fall back. A link that
//! answered 4xx is remembered for [`GONE_FOR`], so other sizes and slots move on at once.
//!
//! First frame: images requested by a build (up to 1024 x 1024) whose bytes are on disk are
//! waited for in [`ImageLoader::poll`] for at most [`SETTLE`], so a revisited screen paints with
//! its art; [`ImageLoader::instant`] tells the scene not to fade those in.

use crate::gpu::{Gpu, GpuImage, TexId};
use std::cell::{Cell, RefCell};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum Src {
    File(PathBuf),
    /// http(s) URLs, and data: URIs (decoded in place, never fetched).
    Url(String),
    Static(&'static str, &'static [u8]),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fit {
    /// object-fit: cover, horizontally centred, `fy` (0..1) of the vertical overflow above.
    Cover { fy: f32 },
    /// object-fit: contain: the result is at most w x h and keeps its aspect.
    Contain,
}

#[derive(Clone, Debug)]
pub struct ImgReq {
    pub src: Src,
    pub w: u32,
    pub h: u32,
    pub fit: Fit,
}

impl ImgReq {
    pub fn key(&self) -> u64 {
        let mut h = DefaultHasher::new();
        self.src.hash(&mut h);
        if let Src::File(p) = &self.src {
            if let Ok(m) = std::fs::metadata(p) {
                m.len().hash(&mut h);
                m.modified().ok().hash(&mut h);
            }
        }
        self.w.hash(&mut h);
        self.h.hash(&mut h);
        match self.fit {
            Fit::Cover { fy } => (0u8, (fy * 1000.0) as i32).hash(&mut h),
            Fit::Contain => 1u8.hash(&mut h),
        }
        // Bump when the resize or premultiply changes.
        5u8.hash(&mut h);
        h.finish()
    }

    /// The request with its longest side at most [`MAX_SIDE`] (aspect kept): nothing is drawn
    /// larger than the screen, and a box sized in web px times rem (a 1920 px hero at rem 31)
    /// would otherwise decode at 3744 px.
    pub fn clamped(mut self) -> ImgReq {
        let m = self.w.max(self.h);
        if m > MAX_SIDE {
            let k = MAX_SIDE as f32 / m as f32;
            self.w = ((self.w as f32 * k).round() as u32).max(1);
            self.h = ((self.h as f32 * k).round() as u32).max(1);
        }
        self
    }

    /// Small enough that a disk hit is waited for on the first frame.
    fn small(&self) -> bool {
        self.w as u64 * self.h as u64 <= 1024 * 1024
    }
}

/// The disk key of a fetched link's bytes.
fn url_key(u: &str) -> u64 {
    let mut h = DefaultHasher::new();
    ("src", u).hash(&mut h);
    h.finish()
}

#[derive(Clone, Copy, Debug)]
pub enum ImgState {
    Pending,
    /// `tex` is the texture to bind, `uv` where the image sits in it (small images share an atlas).
    Ready { tex: TexId, w: u32, h: u32, uv: [f32; 4] },
    Failed,
}

/// The app's HTTP client: `(url, drawn width, drawn height)` to the response body. The default is a
/// plain ureq agent; the launcher sets one with HTTP/2 to rare.market.
pub type Fetcher = Arc<dyn Fn(&str, u32, u32) -> Result<Vec<u8>, String> + Send + Sync>;

/// Network requests in flight at once.
pub const NET_WORKERS: usize = 8;
/// Disk cache cap by default.
pub const DISK_CAP: u64 = 512 << 20;
/// Longest side an image is decoded at.
pub const MAX_SIDE: u32 = 2048;
/// How long a link that answered 4xx is not asked again.
pub const GONE_FOR: Duration = Duration::from_secs(600);
/// How long `poll` waits for disk hits of the images a build just asked for.
pub const SETTLE: Duration = Duration::from_millis(10);
/// A decode slower than this (ms) keeps a slot-sized copy (`.px`) whatever the source.
const SLOW_MS: f32 = 12.0;
/// Sources above this many pixels and over 4x the slot's are kept resized to the slot (`.px`),
/// not as fetched.
const BIG_SRC_PX: u64 = 300_000;
/// Never kept as fetched bytes (re-encoded at the slot's size instead, or not kept).
const SRC_MAX: usize = 4 << 20;

struct Done {
    key: u64,
    result: Result<(u32, u32, Vec<u8>), String>,
    ms: f32,
    cached: bool,
    /// Time on the network, ms (0 when not fetched).
    net_ms: f32,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct ImgStats {
    pub decoded: u32,
    pub from_disk: u32,
    pub failed: u32,
    pub fetched: u32,
    pub decode_ms_total: f32,
}

struct Job {
    key: u64,
    req: ImgReq,
    seq: u64,
    /// Fetched bytes waiting for a decode.
    bytes: Option<Vec<u8>>,
    net_ms: f32,
    /// Counted in `Shared::probes` until its disk probe is answered.
    settle: bool,
}

#[derive(Default)]
struct Queues {
    /// Disk probes (cheap), then fetched bytes to decode, on the decode workers.
    probe: Vec<Job>,
    decode: Vec<Job>,
    net: Vec<Job>,
    /// The frame a pending image was last drawn in (0: built, not drawn).
    prio: HashMap<u64, u64>,
    stop: bool,
}

impl Queues {
    /// The job to run next: drawn most recently first, then the oldest request.
    fn take(v: &mut Vec<Job>, prio: &HashMap<u64, u64>) -> Option<Job> {
        let i = (0..v.len()).max_by_key(|&i| (prio.get(&v[i].key).copied().unwrap_or(0), std::cmp::Reverse(v[i].seq)))?;
        Some(v.swap_remove(i))
    }

    fn best(v: &[Job], prio: &HashMap<u64, u64>) -> Option<(u64, std::cmp::Reverse<u64>)> {
        v.iter().map(|j| (prio.get(&j.key).copied().unwrap_or(0), std::cmp::Reverse(j.seq))).max()
    }
}

struct Shared {
    q: Mutex<Queues>,
    cpu: Condvar,
    net: Condvar,
    fetch: RwLock<Fetcher>,
    cache: Option<PathBuf>,
    cap: std::sync::atomic::AtomicU64,
    writes: std::sync::atomic::AtomicU32,
    done: Mutex<Sender<Done>>,
    waker: Arc<dyn Fn() + Send + Sync>,
    /// Links that answered 4xx (URL key to when, and the status).
    gone: Mutex<HashMap<u64, (Instant, u16)>>,
    /// Small images whose disk probe is not answered yet (see [`SETTLE`]).
    probes: std::sync::atomic::AtomicUsize,
}

impl Shared {
    fn finish(&self, d: Done) {
        let _ = self.done.lock().unwrap().send(d);
        (self.waker)();
    }

    fn probed(&self, job: &Job) {
        if job.settle {
            self.probes.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        }
    }

    fn disk_path(&self, key: u64, ext: &str) -> Option<PathBuf> {
        self.cache.as_ref().map(|d| d.join(format!("{:02x}", key >> 56)).join(format!("{key:016x}.{ext}")))
    }

    /// The disk cache's answer for a request: the slot-sized copy, else the link's bytes.
    fn probe(&self, job: &Job) -> Option<(u32, u32, Vec<u8>)> {
        if let Some(b) = self.disk_path(job.key, "px").as_deref().and_then(read_cached) {
            if let Ok(r) = decode(&b, &job.req) {
                return Some(r);
            }
        }
        let Src::Url(u) = &job.req.src else { return None };
        if u.starts_with("data:") {
            return None;
        }
        let p = self.disk_path(url_key(u), "src")?;
        let b = read_cached(&p)?;
        match decode(&b, &job.req) {
            Ok(r) => Some(r),
            Err(_) => {
                let _ = std::fs::remove_file(&p);
                None
            }
        }
    }

    /// Keeps what a decode used: a fetched link's bytes as they came when they are small or not
    /// much larger than the slot, else the slot-sized pixels re-encoded.
    fn keep(&self, job: &Job, bytes: &[u8], raw: &Raw, ms: f32) {
        // Decoding a large source again costs tens of ms (a 1024 px WebP behind a 480 px tile is
        // ~50 ms in image-webp, a 1280 px hero upscaled to the 1920 px backdrop ~80 ms, a busy
        // SVG as much); the slot-sized JPEG decodes in 1-10 ms.
        let slow = ms > SLOW_MS;
        let oversize = match raw {
            Raw::Img { img, src_px } => *src_px > BIG_SRC_PX && *src_px > 4 * img.width() as u64 * img.height() as u64,
            Raw::Svg(..) => false,
        };
        let fetched = job.bytes.is_some();
        let px = || match raw {
            Raw::Img { img, .. } => encode_px(img),
            Raw::Svg(w, h, px) => encode_px(&unpremultiplied(*w, *h, px)?),
        };
        let wrote = match &job.req.src {
            Src::Static(..) => false,
            _ if oversize || slow || (fetched && bytes.len() > SRC_MAX) => match (px(), self.disk_path(job.key, "px")) {
                (Some(b), Some(p)) => write_cached(&p, &b),
                _ => false,
            },
            Src::Url(u) if fetched && !u.starts_with("data:") => match self.disk_path(url_key(u), "src") {
                Some(p) => write_cached(&p, bytes),
                None => false,
            },
            _ => false,
        };
        if wrote {
            let n = self.writes.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if n % 256 == 255 {
                if let Some(d) = &self.cache {
                    trim_disk(d, self.cap.load(std::sync::atomic::Ordering::Relaxed));
                }
            }
        }
    }

    /// Decode worker: disk probes and decodes, most urgent first; a `probe_only` worker never
    /// takes a fetched image's decode, so disk hits are not stuck behind slow first decodes.
    fn cpu_worker(&self, probe_only: bool) {
        loop {
            let job = {
                let mut q = self.q.lock().unwrap();
                loop {
                    if q.stop {
                        return;
                    }
                    let (p, d) = (Queues::best(&q.probe, &q.prio), if probe_only { None } else { Queues::best(&q.decode, &q.prio) });
                    let from_probe = match (p, d) {
                        (None, None) => None,
                        (Some(_), None) => Some(true),
                        (None, Some(_)) => Some(false),
                        // Equal urgency: the probe first (a disk hit is a millisecond).
                        (Some(a), Some(b)) => Some(a.0 >= b.0),
                    };
                    if let Some(fp) = from_probe {
                        let Queues { probe, decode, prio, .. } = &mut *q;
                        let j = Queues::take(if fp { probe } else { decode }, prio);
                        if let Some(j) = &j {
                            prio.remove(&j.key);
                        }
                        break j.unwrap();
                    }
                    q = self.cpu.wait(q).unwrap();
                }
            };
            let t = Instant::now();
            if job.bytes.is_none() {
                if let Some(px) = self.probe(&job) {
                    self.finish(Done { key: job.key, result: Ok(px), ms: t.elapsed().as_secs_f32() * 1000.0, cached: true, net_ms: 0.0 });
                    self.probed(&job);
                    continue;
                }
            }
            let bytes = match &job.bytes {
                Some(_) => Ok(None),
                None => match &job.req.src {
                    Src::File(p) => std::fs::read(p).map(Some).map_err(|e| format!("{}: {e}", p.display())),
                    Src::Static(_, b) => Ok(Some(b.to_vec())),
                    Src::Url(u) if u.starts_with("data:") => data_uri(u).map(Some),
                    Src::Url(_) => {
                        // To the network workers.
                        let settle = job.settle;
                        let mut q = self.q.lock().unwrap();
                        q.net.push(Job { settle: false, ..job });
                        drop(q);
                        self.net.notify_one();
                        if settle {
                            self.probes.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
                        }
                        continue;
                    }
                },
            };
            let result = bytes.and_then(|own| {
                let b = own.as_deref().or(job.bytes.as_deref()).unwrap_or_default();
                let raw = decode_raw(b, &job.req)?;
                self.keep(&job, b, &raw, t.elapsed().as_secs_f32() * 1000.0);
                Ok(finish_raw(raw))
            });
            self.finish(Done { key: job.key, result, ms: t.elapsed().as_secs_f32() * 1000.0, cached: false, net_ms: job.net_ms });
            self.probed(&job);
        }
    }

    /// Network worker: one request at a time, the most urgent first.
    fn net_worker(&self) {
        loop {
            let job = {
                let mut q = self.q.lock().unwrap();
                loop {
                    if q.stop {
                        return;
                    }
                    let Queues { net, prio, .. } = &mut *q;
                    if let Some(j) = Queues::take(net, prio) {
                        break j;
                    }
                    q = self.net.wait(q).unwrap();
                }
            };
            let Src::Url(u) = &job.req.src else { continue };
            let uk = url_key(u);
            if let Some(code) = self.gone.lock().unwrap().get(&uk).filter(|(t, _)| t.elapsed() < GONE_FOR).map(|(_, c)| *c) {
                self.finish(Done { key: job.key, result: Err(format!("http status: {code}")), ms: 0.0, cached: false, net_ms: 0.0 });
                continue;
            }
            let fetch = self.fetch.read().unwrap().clone();
            let t = Instant::now();
            let r = fetch(u, job.req.w, job.req.h);
            let net_ms = t.elapsed().as_secs_f32() * 1000.0;
            if let Err(e) = &r {
                if let Some(code) = e.strip_prefix("http status: 4").and_then(|c| c.get(..2)).and_then(|c| c.parse::<u16>().ok()) {
                    let mut g = self.gone.lock().unwrap();
                    if g.len() > 4096 {
                        g.retain(|_, (t, _)| t.elapsed() < GONE_FOR);
                    }
                    g.insert(uk, (Instant::now(), 400 + code));
                }
            }
            match r {
                Ok(b) => {
                    let mut q = self.q.lock().unwrap();
                    q.decode.push(Job { bytes: Some(b), net_ms, ..job });
                    drop(q);
                    // All: the probe-only worker may be the one woken, and it leaves decodes.
                    self.cpu.notify_all();
                }
                Err(e) => self.finish(Done { key: job.key, result: Err(e), ms: 0.0, cached: false, net_ms }),
            }
        }
    }
}

pub struct ImageLoader {
    shared: Arc<Shared>,
    rx: Receiver<Done>,
    state: HashMap<u64, ImgState>,
    pub stats: ImgStats,
    /// Every request, so an evicted image can come back on its own.
    reqs: HashMap<u64, ImgReq>,
    uploaded: HashMap<u64, GpuImage>,
    /// When each image was last asked for (painted), seconds on `clock::secs`.
    used: HashMap<u64, Cell<f64>>,
    /// Images freed by `trim`, with when: asked for again, they reload (from the disk cache).
    evicted: HashMap<u64, f64>,
    seq: u64,
    frame: u64,
    /// Pending images paint drew since the last poll (they go to the front of the queues).
    drawn: RefCell<Vec<u64>>,
    errors: HashMap<u64, String>,
    /// Sources whose failure was logged (once each).
    logged: HashSet<u64>,
    /// Timing (RR_SHOT reports): when each image was first asked for and became ready, and the
    /// ones paint drew (on screen).
    asked: HashMap<u64, f64>,
    done_at: HashMap<u64, f64>,
    painted: RefCell<HashSet<u64>>,
    /// Images that came from the disk cache (shown without a fade).
    from_disk: HashSet<u64>,
    /// Small images were asked for since the last poll (it waits for their disk hits).
    fresh: bool,
}

impl ImageLoader {
    /// `threads` decode workers (plus [`NET_WORKERS`] network workers); `waker` is called from a
    /// worker when an image is ready; `cache_dir` keeps the resized pixels.
    pub fn new(threads: usize, waker: Arc<dyn Fn() + Send + Sync>, cache_dir: Option<PathBuf>) -> ImageLoader {
        let (done_tx, rx) = channel::<Done>();
        if let Some(d) = &cache_dir {
            let _ = std::fs::create_dir_all(d);
        }
        let shared = Arc::new(Shared {
            q: Mutex::new(Queues::default()),
            cpu: Condvar::new(),
            net: Condvar::new(),
            fetch: RwLock::new(default_fetcher()),
            cache: cache_dir.clone(),
            cap: std::sync::atomic::AtomicU64::new(DISK_CAP),
            writes: std::sync::atomic::AtomicU32::new(0),
            done: Mutex::new(done_tx),
            waker,
            gone: Mutex::new(HashMap::new()),
            probes: std::sync::atomic::AtomicUsize::new(0),
        });
        let threads = threads.max(1);
        for i in 0..threads {
            let s = shared.clone();
            let probe_only = threads >= 3 && i == 0;
            std::thread::Builder::new().name(format!("rr-img{i}")).spawn(move || s.cpu_worker(probe_only)).expect("image thread");
        }
        for i in 0..NET_WORKERS {
            let s = shared.clone();
            std::thread::Builder::new().name(format!("rr-net{i}")).spawn(move || s.net_worker()).expect("image net thread");
        }
        if let Some(d) = cache_dir {
            let cap = DISK_CAP;
            let _ = std::thread::Builder::new().name("rr-img-trim".into()).spawn(move || trim_disk(&d, cap));
        }
        ImageLoader {
            shared,
            rx,
            state: HashMap::new(),
            stats: ImgStats::default(),
            reqs: HashMap::new(),
            uploaded: HashMap::new(),
            used: HashMap::new(),
            evicted: HashMap::new(),
            seq: 0,
            frame: 1,
            drawn: RefCell::new(Vec::new()),
            errors: HashMap::new(),
            logged: HashSet::new(),
            asked: HashMap::new(),
            done_at: HashMap::new(),
            painted: RefCell::new(HashSet::new()),
            from_disk: HashSet::new(),
            fresh: false,
        }
    }

    /// The HTTP client the network workers use.
    pub fn set_fetcher(&mut self, f: Fetcher) {
        *self.shared.fetch.write().unwrap() = f;
    }

    /// The disk cache's size cap (least recently used files go first).
    pub fn set_disk_cap(&mut self, bytes: u64) {
        self.shared.cap.store(bytes, std::sync::atomic::Ordering::Relaxed);
    }

    fn enqueue(&mut self, key: u64, req: ImgReq) {
        self.seq += 1;
        // Waited for on the first frame: small pictures that may be on disk.
        let settle = req.small() && self.shared.cache.is_some() && !matches!(&req.src, Src::Static(..));
        if settle {
            self.fresh = true;
            self.shared.probes.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        let mut q = self.shared.q.lock().unwrap();
        q.probe.push(Job { key, req, seq: self.seq, bytes: None, net_ms: 0.0, settle });
        drop(q);
        self.shared.cpu.notify_one();
    }

    /// The image came from the disk cache (or was already on the GPU): show it without a fade.
    pub fn instant(&self, key: u64) -> bool {
        self.from_disk.contains(&key)
    }

    /// Queues the image unless it is known; returns its key. Sizes above [`MAX_SIDE`] are scaled
    /// down ([`ImgReq::clamped`]).
    pub fn request(&mut self, req: ImgReq) -> u64 {
        let req = req.clamped();
        let key = req.key();
        if !self.state.contains_key(&key) && !self.evicted.contains_key(&key) {
            self.state.insert(key, ImgState::Pending);
            self.used.insert(key, Cell::new(crate::clock::secs()));
            self.reqs.insert(key, req.clone());
            self.asked.entry(key).or_insert_with(crate::clock::secs);
            self.enqueue(key, req);
        }
        key
    }

    /// The state without counting as drawn (bookkeeping such as fades).
    pub fn peek(&self, key: u64) -> ImgState {
        if self.evicted.contains_key(&key) {
            return ImgState::Pending;
        }
        self.state.get(&key).copied().unwrap_or(ImgState::Failed)
    }

    /// Why the image failed, when it did.
    pub fn error(&self, key: u64) -> Option<&str> {
        self.errors.get(&key).map(String::as_str)
    }

    /// Time-to-image for the images paint drew: how many, the per-image latency (ask to ready)
    /// p50 / p90 / max, and the span from the first ask to the last one ready, in ms.
    pub fn timing_report(&self) -> String {
        let painted = self.painted.borrow();
        let mut lat: Vec<f64> = Vec::new();
        let (mut first, mut last, mut failed, mut pending) = (f64::MAX, 0.0f64, 0, 0);
        for k in painted.iter() {
            let Some(a) = self.asked.get(k) else { continue };
            first = first.min(*a);
            match (self.done_at.get(k), self.state.get(k)) {
                (Some(d), _) => {
                    lat.push((d - a) * 1000.0);
                    last = last.max(*d);
                }
                (None, Some(ImgState::Pending)) => pending += 1,
                _ => failed += 1,
            }
        }
        lat.sort_by(|a, b| a.total_cmp(b));
        let q = |p: f64| if lat.is_empty() { 0.0 } else { lat[((lat.len() - 1) as f64 * p).round() as usize] };
        let span = if last > 0.0 && first < f64::MAX { (last - first) * 1000.0 } else { 0.0 };
        format!(
            "image timing: {} on screen ready, {failed} links failed (the chain moved on), {pending} pending; per image p50 {:.0} ms, p90 {:.0} ms, max {:.0} ms; first ask to last ready {span:.0} ms; loader: {} decoded, {} from disk, {} fetched, {} failed",
            lat.len(),
            q(0.5),
            q(0.9),
            q(1.0),
            self.stats.decoded,
            self.stats.from_disk,
            self.stats.fetched,
            self.stats.failed
        )
    }

    /// The image's state; asking counts as drawing it (paint asks for every image it draws), which
    /// keeps it from `trim`, moves a pending one to the front of the queues and brings an evicted
    /// one back (Pending until it has reloaded).
    pub fn state(&self, key: u64) -> ImgState {
        if let Some(u) = self.used.get(&key) {
            u.set(crate::clock::secs());
        }
        if let Ok(mut p) = self.painted.try_borrow_mut() {
            p.insert(key);
        }
        if self.evicted.contains_key(&key) {
            return ImgState::Pending;
        }
        let s = self.state.get(&key).copied().unwrap_or(ImgState::Failed);
        if matches!(s, ImgState::Pending) {
            if let Ok(mut d) = self.drawn.try_borrow_mut() {
                d.push(key);
            }
        }
        s
    }

    /// Bytes of image pixels on the GPU.
    pub fn bytes(&self) -> u64 {
        self.uploaded.values().map(|g| g.w as u64 * g.h as u64 * 4).sum()
    }

    /// Frees the least recently drawn images, oldest first, until at most `max_bytes` remain;
    /// images drawn in the last `idle_secs` are kept whatever the total. An evicted image that is
    /// drawn again reloads by itself (from the disk cache, off the UI thread). Returns the bytes
    /// freed. Call it now and then (every few seconds, or after leaving a screen).
    pub fn trim(&mut self, gpu: &mut Gpu, max_bytes: u64, idle_secs: f64) -> u64 {
        let mut total = self.bytes();
        if total <= max_bytes {
            return 0;
        }
        let now = crate::clock::secs();
        let mut old: Vec<(f64, u64)> = self.uploaded.keys().map(|k| (self.used.get(k).map(|c| c.get()).unwrap_or(0.0), *k)).filter(|(t, _)| now - t >= idle_secs).collect();
        old.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut freed = 0;
        for (_, k) in old {
            if total <= max_bytes {
                break;
            }
            if let Some(g) = self.uploaded.remove(&k) {
                let b = g.w as u64 * g.h as u64 * 4;
                gpu.free_image(g);
                self.state.remove(&k);
                self.evicted.insert(k, now);
                total -= b;
                freed += b;
            }
        }
        freed
    }

    /// Uploads every finished image. True when something new is ready.
    pub fn poll(&mut self, gpu: &mut Gpu) -> bool {
        let mut any = false;
        // What the last frames drew goes first.
        self.frame += 1;
        let drawn = std::mem::take(&mut *self.drawn.borrow_mut());
        if !drawn.is_empty() {
            let mut q = self.shared.q.lock().unwrap();
            if q.prio.len() > 8192 {
                q.prio.clear();
            }
            for k in drawn {
                q.prio.insert(k, self.frame);
            }
        }
        // Evicted images asked for since: reload them.
        let back: Vec<u64> = self.evicted.iter().filter(|(k, at)| self.used.get(k).is_some_and(|u| u.get() > **at)).map(|(k, _)| *k).collect();
        for k in back {
            self.evicted.remove(&k);
            if let Some(req) = self.reqs.get(&k).cloned() {
                self.state.insert(k, ImgState::Pending);
                self.shared.q.lock().unwrap().prio.insert(k, self.frame);
                self.enqueue(k, req);
            }
        }
        // A build just asked for small pictures: their disk hits (a few ms on the decode workers)
        // land before this frame paints, so a revisited screen shows its art at once. Misses
        // leave the count as they go to the network, so this never waits on a fetch.
        let mut early: Vec<Done> = Vec::new();
        if std::mem::take(&mut self.fresh) {
            let until = Instant::now() + SETTLE;
            while self.shared.probes.load(std::sync::atomic::Ordering::Acquire) > 0 {
                let left = until.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                if let Ok(d) = self.rx.recv_timeout(left.min(Duration::from_millis(1))) {
                    early.push(d);
                }
            }
        }
        let mut early = early.into_iter();
        while let Some(d) = early.next().or_else(|| self.rx.try_recv().ok()) {
            any = true;
            if trace() {
                let now = crate::clock::secs();
                let asked = self.asked.get(&d.key).map(|a| (now - a) * 1000.0).unwrap_or(0.0);
                let drawn = self.painted.borrow().contains(&d.key);
                let what = self.reqs.get(&d.key).map(|r| format!("{}x{} {}", r.w, r.h, describe(&r.src))).unwrap_or_default();
                log::info!("img {} {asked:.0} ms after ask (net {:.0}, decode {:.0}){} {what}", if d.cached { "disk" } else if d.result.is_ok() { "made" } else { "FAIL" }, d.net_ms, d.ms, if drawn { " on screen" } else { "" });
            }
            match d.result {
                Ok((w, h, px)) => {
                    if let Some(old) = self.uploaded.remove(&d.key) {
                        gpu.free_image(old);
                    }
                    let g = gpu.upload_image(w, h, &px);
                    self.state.insert(d.key, ImgState::Ready { tex: g.tex, w, h, uv: g.uv });
                    self.done_at.entry(d.key).or_insert_with(crate::clock::secs);
                    self.uploaded.insert(d.key, g);
                    if d.cached {
                        self.from_disk.insert(d.key);
                        self.stats.from_disk += 1
                    } else {
                        if matches!(self.reqs.get(&d.key).map(|r| &r.src), Some(Src::Url(u)) if !u.starts_with("data:")) {
                            self.stats.fetched += 1;
                        }
                        self.stats.decoded += 1;
                        self.stats.decode_ms_total += d.ms;
                    }
                }
                Err(e) => {
                    let what = self.reqs.get(&d.key).map(|r| describe(&r.src)).unwrap_or_default();
                    let mut h = DefaultHasher::new();
                    what.hash(&mut h);
                    // A rendition rare.market has not made yet answers 404 at once: the next link
                    // of the chain is the normal path, not a failure worth a line.
                    let expected = what.contains("/img/src/") && e.contains("404");
                    if self.logged.insert(h.finish()) && !expected {
                        log::warn!("image {what}: {e}");
                    }
                    self.errors.insert(d.key, e);
                    self.state.insert(d.key, ImgState::Failed);
                    self.stats.failed += 1;
                }
            }
        }
        any
    }

    pub fn pending(&self) -> usize {
        self.state.values().filter(|s| matches!(s, ImgState::Pending)).count()
    }
}

impl Drop for ImageLoader {
    fn drop(&mut self) {
        self.shared.q.lock().unwrap().stop = true;
        self.shared.cpu.notify_all();
        self.shared.net.notify_all();
    }
}

/// RR_IMG_TRACE=1: a line per finished image (timings, source).
fn trace() -> bool {
    static T: OnceLock<bool> = OnceLock::new();
    *T.get_or_init(|| std::env::var_os("RR_IMG_TRACE").is_some())
}

/// A source for a log line: the URL (a data: URI by its type and length), or the path.
fn describe(src: &Src) -> String {
    match src {
        Src::Url(u) if u.starts_with("data:") => format!("{}... ({} bytes)", &u[..u.find(',').unwrap_or(u.len()).min(40)], u.len()),
        Src::Url(u) => u.chars().take(300).collect(),
        Src::File(p) => p.display().to_string(),
        Src::Static(n, _) => n.to_string(),
    }
}

// ---------------------------------------------------------------- the network

fn default_fetcher() -> Fetcher {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .timeout_connect(Some(Duration::from_secs(5)))
        .max_idle_connections_per_host(NET_WORKERS)
        .build()
        .into();
    Arc::new(move |u: &str, _w, _h| {
        let mut r = agent.get(u).header("User-Agent", "RareLauncher/0.1").header("Accept", ACCEPT).call().map_err(|e| e.to_string())?;
        r.body_mut().with_config().limit(32 << 20).read_to_vec().map_err(|e| e.to_string())
    })
}

/// What the decoders read (no AVIF: a server that negotiates sends WebP or JPEG instead).
pub const ACCEPT: &str = "image/webp,image/png,image/jpeg,image/gif,image/svg+xml,image/*;q=0.5";

/// The bytes of a data: URI (base64 or percent-encoded).
pub fn data_uri(u: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    let (head, body) = u.split_once(',').ok_or("data: URI without a comma")?;
    if head.to_ascii_lowercase().contains(";base64") {
        let clean: String = body.chars().filter(|c| !c.is_whitespace()).collect();
        let clean = percent_decode(&clean);
        let t = String::from_utf8_lossy(&clean);
        let t = t.trim_end_matches('=');
        base64::engine::general_purpose::STANDARD_NO_PAD
            .decode(t)
            .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(t))
            .map_err(|e| format!("data: URI: {e}"))
    } else {
        Ok(percent_decode(body))
    }
}

fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16)) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

// ---------------------------------------------------------------- the disk cache

fn read_cached(p: &Path) -> Option<Vec<u8>> {
    let b = std::fs::read(p).ok()?;
    if b.is_empty() {
        return None;
    }
    // Used: keep it from the trim (mtime is the LRU clock; at most one write an hour per file).
    if let Ok(f) = std::fs::File::options().write(true).open(p) {
        let old = f.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|e| e > Duration::from_secs(3600));
        if old {
            let _ = f.set_modified(SystemTime::now());
        }
    }
    Some(b)
}

fn write_cached(p: &Path, b: &[u8]) -> bool {
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = p.with_extension(format!("tmp{:x}", std::process::id()));
    if std::fs::write(&tmp, b).is_ok() {
        if std::fs::rename(&tmp, p).is_ok() {
            return true;
        }
        let _ = std::fs::remove_file(&tmp);
    }
    false
}

/// resvg's premultiplied pixels back to straight alpha (for the disk copy).
fn unpremultiplied(w: u32, h: u32, px: &[u8]) -> Option<image::RgbaImage> {
    let mut v = px.to_vec();
    for p in v.chunks_exact_mut(4) {
        let a = p[3] as u16;
        if a != 0 && a != 255 {
            for c in &mut p[..3] {
                *c = ((*c as u16 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    image::RgbaImage::from_raw(w, h, v)
}

/// Slot-sized pixels (straight alpha) for the disk: JPEG q90 when opaque, else PNG.
fn encode_px(img: &image::RgbaImage) -> Option<Vec<u8>> {
    use image::ImageEncoder;
    let (w, h) = img.dimensions();
    let mut out = Vec::new();
    if img.pixels().all(|p| p[3] == 255) {
        let rgb: Vec<u8> = img.as_raw().chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90).write_image(&rgb, w, h, image::ExtendedColorType::Rgb8).ok()?;
    } else {
        image::codecs::png::PngEncoder::new_with_quality(&mut out, image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::Adaptive)
            .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
            .ok()?;
    }
    Some(out)
}

/// Deletes the least recently used files until the cache is under 85% of `cap`. Only `.src` and
/// `.px` files count (winners.bin and other files next to them stay); `.rgba` files (the old raw
/// pixel cache) and stale temporaries are removed.
fn trim_disk(dir: &Path, cap: u64) {
    let mut files = Vec::new();
    let mut total = 0u64;
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut visit = |p: PathBuf| {
        let Ok(md) = std::fs::metadata(&p) else { return };
        if !md.is_file() {
            return;
        }
        let ext = p.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        if ext == "rgba" || (ext.starts_with("tmp") && md.modified().ok().and_then(|t| t.elapsed().ok()).is_some_and(|e| e > Duration::from_secs(600))) {
            let _ = std::fs::remove_file(&p);
            return;
        }
        if ext != "src" && ext != "px" {
            return;
        }
        total += md.len();
        files.push((md.modified().unwrap_or(SystemTime::UNIX_EPOCH), md.len(), p));
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Ok(rd2) = std::fs::read_dir(&p) {
                for f in rd2.flatten() {
                    visit(f.path());
                }
            }
        } else {
            visit(p);
        }
    }
    if total <= cap {
        return;
    }
    files.sort();
    for (_, len, p) in files {
        if total <= cap / 100 * 85 {
            break;
        }
        if std::fs::remove_file(&p).is_ok() {
            total -= len;
        }
    }
}

// ---------------------------------------------------------------- decoding

/// SVG by its text: an `<svg` element near the start (after a BOM, an XML declaration, comments or
/// a doctype).
fn is_svg(b: &[u8]) -> bool {
    let head = &b[..b.len().min(2048)];
    let t = String::from_utf8_lossy(head);
    let t = t.trim_start_matches('\u{feff}').trim_start();
    t.starts_with('<') && t.to_ascii_lowercase().contains("<svg")
}

fn is_avif(b: &[u8]) -> bool {
    b.len() >= 12 && &b[4..8] == b"ftyp" && matches!(&b[8..12], b"avif" | b"avis" | b"heic" | b"heix" | b"mif1" | b"msf1")
}

/// A decode before premultiplying: SVG comes out of resvg premultiplied; a raster image is shaped
/// to the slot with straight alpha (what the disk copy encodes), with the source's pixel count.
enum Raw {
    Svg(u32, u32, Vec<u8>),
    Img { img: image::RgbaImage, src_px: u64 },
}

fn decode_raw(bytes: &[u8], req: &ImgReq) -> Result<Raw, String> {
    if is_svg(bytes) {
        let (w, h, px) = svg(bytes, req)?;
        return Ok(Raw::Svg(w, h, px));
    }
    if is_avif(bytes) {
        return Err("AVIF/HEIF is not decoded here".into());
    }
    if bytes.starts_with(b"<") || bytes.is_empty() {
        return Err(format!("not an image ({} bytes of text)", bytes.len()));
    }
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?.to_rgba8();
    let src_px = img.width() as u64 * img.height() as u64;
    Ok(Raw::Img { img: shape(img, req), src_px })
}

fn finish_raw(raw: Raw) -> (u32, u32, Vec<u8>) {
    match raw {
        Raw::Svg(w, h, px) => (w, h, px),
        Raw::Img { img, .. } => {
            let (w, h) = img.dimensions();
            let mut px = img.into_raw();
            premultiply(&mut px);
            (w, h, px)
        }
    }
}

/// Fetched or read bytes to premultiplied RGBA at the request's size.
pub fn decode(bytes: &[u8], req: &ImgReq) -> Result<(u32, u32, Vec<u8>), String> {
    decode_raw(bytes, req).map(finish_raw)
}

/// Cropped and resized to the slot, straight alpha.
fn shape(img: image::RgbaImage, req: &ImgReq) -> image::RgbaImage {
    let (sw, sh) = img.dimensions();
    let (tw, th) = (req.w.max(1), req.h.max(1));
    // Already the slot's size (a disk copy made for it): as it is.
    if (sw, sh) == (tw, th) {
        return img;
    }
    match req.fit {
        Fit::Cover { fy } => {
            // Crop the source to the target aspect, then resize.
            let ta = tw as f32 / th as f32;
            let sa = sw as f32 / sh.max(1) as f32;
            let (cw, ch) = if sa > ta { (((sh as f32 * ta).round() as u32).max(1), sh) } else { (sw, ((sw as f32 / ta).round() as u32).max(1)) };
            let cx = (sw - cw.min(sw)) / 2;
            let cy = ((sh - ch.min(sh)) as f32 * fy).round() as u32;
            let crop = image::imageops::crop_imm(&img, cx, cy, cw.min(sw), ch.min(sh)).to_image();
            resize(&crop, tw, th)
        }
        Fit::Contain => {
            let k = (tw as f32 / sw.max(1) as f32).min(th as f32 / sh.max(1) as f32);
            let (w, h) = (((sw as f32 * k).round() as u32).max(1), ((sh as f32 * k).round() as u32).max(1));
            if (w, h) == (sw, sh) {
                return img;
            }
            resize(&img, w, h)
        }
    }
}

fn premultiply(px: &mut [u8]) {
    for p in px.chunks_exact_mut(4) {
        let a = p[3] as u16;
        if a != 255 {
            p[0] = ((p[0] as u16 * a + 127) / 255) as u8;
            p[1] = ((p[1] as u16 * a + 127) / 255) as u8;
            p[2] = ((p[2] as u16 * a + 127) / 255) as u8;
        }
    }
}

fn resize(img: &image::RgbaImage, w: u32, h: u32) -> image::RgbaImage {
    let (sw, sh) = img.dimensions();
    if sw == w && sh == h {
        return img.clone();
    }
    if sw >= w && sh >= h {
        // Area-averaging downscale: sharp and fast.
        image::imageops::thumbnail(img, w, h)
    } else if sw * 3 <= w && sh * 3 <= h {
        // Pixel art (a 72 px sprite on a 480 px tile): crisp blocks, not a blur.
        image::imageops::resize(img, w, h, image::imageops::FilterType::Nearest)
    } else {
        image::imageops::resize(img, w, h, image::imageops::FilterType::CatmullRom)
    }
}

/// System fonts for SVG text, loaded once (the first SVG with text pays for it).
fn svg_fonts() -> Arc<resvg::usvg::fontdb::Database> {
    static DB: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = resvg::usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    })
    .clone()
}

/// usvg's options: embedded rasters usvg does not take (BMP, ICO, TIFF: on-chain pixel art is
/// often BMP) become PNG first; no file or URL references; system fonts only when there is text.
fn svg_options(fonts: bool) -> resvg::usvg::Options<'static> {
    use resvg::usvg::{ImageHrefResolver, ImageKind};
    let default = ImageHrefResolver::default_data_resolver();
    let resolve_data = Box::new(move |mime: &str, data: Arc<Vec<u8>>, opts: &resvg::usvg::Options| {
        default(mime, data.clone(), opts).or_else(|| {
            let img = image::load_from_memory(&data).ok()?;
            let mut png = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
            Some(ImageKind::PNG(Arc::new(png)))
        })
    });
    let mut o = resvg::usvg::Options { image_href_resolver: ImageHrefResolver { resolve_data, resolve_string: Box::new(|_, _| None) }, ..Default::default() };
    o.fontdb = if fonts { svg_fonts() } else { Arc::new(resvg::usvg::fontdb::Database::new()) };
    o
}

fn svg(bytes: &[u8], req: &ImgReq) -> Result<(u32, u32, Vec<u8>), String> {
    let opts = svg_options(needs_fonts(bytes));
    let tree = resvg::usvg::Tree::from_data(bytes, &opts).map_err(|e| format!("svg: {e}"))?;
    let s = tree.size();
    let (tw, th) = (req.w.max(1) as f32, req.h.max(1) as f32);
    let k = match req.fit {
        Fit::Contain => (tw / s.width()).min(th / s.height()),
        Fit::Cover { .. } => (tw / s.width()).max(th / s.height()),
    };
    let (w, h) = match req.fit {
        Fit::Contain => ((s.width() * k).round().max(1.0) as u32, (s.height() * k).round().max(1.0) as u32),
        Fit::Cover { .. } => (req.w.max(1), req.h.max(1)),
    };
    let mut pm = resvg::tiny_skia::Pixmap::new(w, h).ok_or("svg: pixmap")?;
    let dx = (w as f32 - s.width() * k) / 2.0;
    let dy = (h as f32 - s.height() * k) / 2.0;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_row(k, 0.0, 0.0, k, dx, dy), &mut pm.as_mut());
    Ok((w, h, pm.take()))
}

/// Text in the SVG (system fonts are only loaded for those).
fn needs_fonts(b: &[u8]) -> bool {
    b.windows(5).any(|w| w == b"<text")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(w: u32) -> ImgReq {
        ImgReq { src: Src::Url(String::new()), w, h: w, fit: Fit::Cover { fy: 0.5 } }
    }

    #[test]
    fn svg_with_an_embedded_bmp() {
        // Dead Skull art: a pixelated BMP inside an SVG.
        let mut bmp = Vec::new();
        image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 30, 30, 255])).write_to(&mut std::io::Cursor::new(&mut bmp), image::ImageFormat::Bmp).unwrap();
        use base64::Engine;
        let svg = format!("<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 4 4'><image width='4' height='4' style='image-rendering:pixelated' href='data:image/bmp;base64,{}'/></svg>", base64::engine::general_purpose::STANDARD.encode(&bmp));
        let (w, h, px) = decode(svg.as_bytes(), &req(64)).unwrap();
        assert_eq!((w, h), (64, 64));
        assert_eq!(&px[(32 * 64 + 32) * 4..(32 * 64 + 32) * 4 + 4], &[200, 30, 30, 255]);
    }

    #[test]
    fn data_uris() {
        assert_eq!(data_uri("data:image/svg+xml;charset=utf-8,%3Csvg%3E").unwrap(), b"<svg>");
        assert_eq!(data_uri("data:image/png;base64,aGk=").unwrap(), b"hi");
        assert_eq!(data_uri("data:image/png;base64,aGk").unwrap(), b"hi");
        assert!(is_svg(b"\xef\xbb\xbf <?xml version='1.0'?><!-- x --><svg/>"));
        assert!(!is_svg(b"<html><body>404</body></html>"));
    }

    fn png(w: u32, h: u32, noisy: bool) -> Vec<u8> {
        let mut b = Vec::new();
        let img = image::RgbaImage::from_fn(w, h, |x, y| if noisy { image::Rgba([(x * 7 ^ y * 13) as u8, (x * y) as u8, (x + y * 3) as u8, 255]) } else { image::Rgba([10, 20, 30, 255]) });
        img.write_to(&mut std::io::Cursor::new(&mut b), image::ImageFormat::Png).unwrap();
        b
    }

    #[test]
    fn disk_keeps_compressed_bytes() {
        let dir = std::env::temp_dir().join(format!("rr-img-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let l = ImageLoader::new(1, Arc::new(|| {}), Some(dir.clone()));
        let s = &l.shared;
        // An old raw-pixel entry goes on the first trim.
        std::fs::create_dir_all(dir.join("ab")).unwrap();
        std::fs::write(dir.join("ab").join("x.rgba"), [0u8; 64]).unwrap();
        std::fs::write(dir.join("winners.bin"), [0u8; 16]).unwrap();
        trim_disk(&dir, DISK_CAP);
        assert!(!dir.join("ab").join("x.rgba").exists());
        assert!(dir.join("winners.bin").exists());

        // A small fetched rendition: kept as fetched, by URL, and shared by another size.
        let small = png(240, 240, false);
        let job = |u: &str, w: u32, b: Option<Vec<u8>>| {
            let req = ImgReq { src: Src::Url(u.into()), w, h: w, fit: Fit::Cover { fy: 0.5 } };
            Job { key: req.key(), req, seq: 0, bytes: b, net_ms: 0.0, settle: false }
        };
        let j = job("https://rare.market/img/src/240.webp?url=a", 120, Some(small.clone()));
        let raw = decode_raw(&small, &j.req).unwrap();
        s.keep(&j, &small, &raw, 0.0);
        let p = s.disk_path(url_key("https://rare.market/img/src/240.webp?url=a"), "src").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), small);
        let (w, h, _) = s.probe(&job("https://rare.market/img/src/240.webp?url=a", 90, None)).unwrap();
        assert_eq!((w, h), (90, 90));

        // A big original on a small slot: the slot-sized copy, re-encoded, not the original.
        let big = png(1600, 1600, true);
        let j = job("https://example.com/big.png", 200, Some(big.clone()));
        let raw = decode_raw(&big, &j.req).unwrap();
        s.keep(&j, &big, &raw, 0.0);
        assert!(s.disk_path(url_key("https://example.com/big.png"), "src").unwrap().metadata().is_err());
        let px = s.disk_path(j.key, "px").unwrap();
        assert!(px.metadata().unwrap().len() < 200 * 200 * 4 / 2);
        let (w, h, _) = s.probe(&job("https://example.com/big.png", 200, None)).unwrap();
        assert_eq!((w, h), (200, 200));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn requests_are_clamped() {
        let r = ImgReq { src: Src::Url(String::new()), w: 3744, h: 1209, fit: Fit::Cover { fy: 0.5 } }.clamped();
        assert_eq!((r.w, r.h), (2048, 661));
    }

    #[test]
    fn gif_first_frame() {
        let mut gif = Vec::new();
        image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 255, 255])).write_to(&mut std::io::Cursor::new(&mut gif), image::ImageFormat::Gif).unwrap();
        let (w, _, px) = decode(&gif, &req(16)).unwrap();
        assert_eq!(w, 16);
        assert_eq!(&px[0..4], &[0, 0, 255, 255]);
    }
}
