//! Rare's data cache. Everything the UI shows from the network (the Sui Games catalog, market
//! collections, floors and activity, prices, portfolios, notices) and state the UI computes itself
//! (wallet balances, friends) lives here as JSON text under a string key:
//!
//! - in memory, so any screen gets data it has seen before in microseconds, never a loading state;
//! - on disk (one small file per key under `Config::dir`, written behind on a thread), so after a
//!   cold start the first frame already has it;
//! - kept fresh by stale-while-revalidate: a read answers at once with what is cached and, when it
//!   is older than the key's TTL ([`Policy`]), refetches it on a worker; listeners hear about keys
//!   whose content changed (304s and identical bodies are silent, so the UI never churns);
//! - warmed at start ([`Store::warm`]): every key the UI used recently is revalidated in order of
//!   use, so the second screen is as instant as the first;
//! - quiet while a game runs: background fetches ask a gate ([`Store::set_gate`]) first.
//!
//! The store knows nothing about Tauri, React or the native UI. Keys are opaque; HTTP keys become
//! requests through the host's resolver (`rare:/v1/games` -> `https://rare.market/v1/games`), which
//! is also where the host enforces its allowlist. Keys the resolver does not know are local state
//! ([`Store::put`]).

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// An HTTP GET the resolver makes of a key.
#[derive(Clone, Debug)]
pub struct Request {
    pub url: String,
    pub headers: Vec<(String, String)>,
}

impl Request {
    pub fn get(url: impl Into<String>) -> Self {
        Request { url: url.into(), headers: Vec::new() }
    }
}

/// Maps a key to its request, or None for keys that are local state only.
pub type Resolver = dyn Fn(&str) -> Option<Request> + Send + Sync;
/// Called with each key whose content changed (`None`: removed). Runs on a store thread.
pub type Listener = dyn Fn(&str, Option<&Hit>) + Send + Sync;
/// True when background fetches may run (false while a game has the screen).
pub type Gate = dyn Fn() -> bool + Send + Sync;

#[derive(Clone, Debug)]
pub struct Config {
    /// Where entries persist (`~/.cache/rare/state`). None keeps them in memory only.
    pub dir: Option<PathBuf>,
    /// Fetch threads. Foreground misses and revalidations go before background warming.
    pub workers: usize,
    /// Total body bytes kept; the least recently used entries go first.
    pub max_bytes: usize,
    /// Entries not read for this long are dropped.
    pub keep: Duration,
    pub timeout: Duration,
    pub user_agent: String,
    /// Largest response body accepted.
    pub max_body: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            dir: None,
            workers: 4,
            max_bytes: 192 << 20,
            keep: Duration::from_secs(21 * 86_400),
            timeout: Duration::from_secs(20),
            user_agent: "RareLauncher/0.1".into(),
            max_body: 32 << 20,
        }
    }
}

/// How a key is served.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freshness {
    /// Display data: answer with whatever is cached, however old, and refetch in the background
    /// once it is older than `ttl`. A miss fetches and waits.
    Swr { ttl: Duration },
    /// Data something acts on (a listing a purchase is built from): answer from the cache only when
    /// younger than `ttl`, else fetch and wait. With `offline_ok`, a failed fetch answers with the
    /// cached copy (marked stale) instead of the error.
    Fresh { ttl: Duration, offline_ok: bool },
}

impl Freshness {
    pub fn ttl(&self) -> Duration {
        match *self {
            Freshness::Swr { ttl } | Freshness::Fresh { ttl, .. } => ttl,
        }
    }
    pub const fn swr_secs(s: u64) -> Self {
        Freshness::Swr { ttl: Duration::from_secs(s) }
    }
    pub const fn fresh_secs(s: u64) -> Self {
        Freshness::Fresh { ttl: Duration::from_secs(s), offline_ok: false }
    }
}

/// Freshness by key: the first rule whose pattern the key contains wins.
#[derive(Clone, Debug)]
pub struct Policy {
    rules: Vec<(String, Freshness)>,
    default: Freshness,
}

impl Policy {
    pub fn new(default: Freshness) -> Self {
        Policy { rules: Vec::new(), default }
    }
    pub fn rule(mut self, pattern: impl Into<String>, f: Freshness) -> Self {
        self.rules.push((pattern.into(), f));
        self
    }
    pub fn for_key(&self, key: &str) -> Freshness {
        self.rules.iter().find(|(p, _)| key.contains(p.as_str())).map(|(_, f)| *f).unwrap_or(self.default)
    }
}

/// A cached answer.
#[derive(Clone, Debug)]
pub struct Hit {
    /// The JSON text, exactly as fetched or put.
    pub body: Arc<str>,
    /// When it was fetched (or put), ms since the epoch.
    pub fetched_ms: u64,
    /// Older than its TTL (a revalidation is on its way), or served from cache after a failed fetch.
    pub stale: bool,
}

impl Hit {
    pub fn age(&self) -> Duration {
        Duration::from_millis(now_ms().saturating_sub(self.fetched_ms))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub entries: usize,
    pub bytes: usize,
    pub hits: u64,
    pub misses: u64,
    pub fetches: u64,
    pub not_modified: u64,
    pub unchanged: u64,
    pub changed: u64,
    pub errors: u64,
    pub gated: u64,
}

#[derive(Clone)]
struct Entry {
    body: Arc<str>,
    fetched: u64,
    used: u64,
    /// `used` as last written to disk; a read only rewrites the file when this is an hour behind.
    saved_used: u64,
    etag: Option<String>,
    local: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Header {
    k: String,
    f: u64,
    u: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    e: Option<String>,
    #[serde(default)]
    l: bool,
}

#[derive(Default)]
struct Jobs {
    fg: VecDeque<String>,
    bg: VecDeque<String>,
    queued: HashSet<String>,
    stop: bool,
}

#[derive(Default)]
struct Dirty {
    keys: HashSet<String>,
    stop: bool,
}

struct Flight {
    result: Mutex<Option<Result<Hit, String>>>,
    cv: Condvar,
}

#[derive(Default)]
struct Counters {
    hits: AtomicU64,
    misses: AtomicU64,
    fetches: AtomicU64,
    not_modified: AtomicU64,
    unchanged: AtomicU64,
    changed: AtomicU64,
    errors: AtomicU64,
    gated: AtomicU64,
}

struct Inner {
    cfg: Config,
    resolver: Box<Resolver>,
    policy: RwLock<Policy>,
    map: RwLock<HashMap<String, Entry>>,
    jobs: Mutex<Jobs>,
    jobs_cv: Condvar,
    dirty: Mutex<Dirty>,
    dirty_cv: Condvar,
    inflight: Mutex<HashMap<String, Arc<Flight>>>,
    listeners: RwLock<Vec<Arc<Listener>>>,
    gate: RwLock<Option<Arc<Gate>>>,
    agent: ureq::Agent,
    n: Counters,
}

/// The data cache. Cheap to clone; all clones share one store.
#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

const HOUR_MS: u64 = 3_600_000;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// FNV-1a 64: file names for keys (the file's header holds the key itself).
fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Test runs only: `RARE_TEST_TOKEN` in the environment goes to rare.market as `x-rare-test`
/// (its own rate-limit bucket), so headless runs off the device never spend the home IP's budget.
/// `RR_NET_TRACE=1`: one stderr line per request (`net <ms> <status> <bytes> <url>`), for timing pages.
pub fn trace(t0: std::time::Instant, status: u16, bytes: usize, url: &str) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *ON.get_or_init(|| std::env::var_os("RR_NET_TRACE").is_some()) {
        eprintln!("net {:>5} ms {status} {bytes}B {url}", t0.elapsed().as_millis());
    }
}

pub fn test_token(url: &str) -> Option<String> {
    static T: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let t = T.get_or_init(|| std::env::var("RARE_TEST_TOKEN").ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()));
    let host = url.split("://").nth(1)?.split(['/', '?', ':']).next()?;
    (host == "rare.market" || host.ends_with(".rare.market")).then(|| t.clone()).flatten()
}

fn is_json(s: &str) -> bool {
    serde_json::from_str::<serde::de::IgnoredAny>(s).is_ok()
}

impl Store {
    /// Opens the store: loads what `cfg.dir` holds (dropping entries unused for `cfg.keep` and
    /// anything over `cfg.max_bytes`), then starts the fetch workers and the disk writer.
    pub fn open(cfg: Config, policy: Policy, resolver: impl Fn(&str) -> Option<Request> + Send + Sync + 'static) -> std::io::Result<Store> {
        let mut map = HashMap::new();
        if let Some(dir) = &cfg.dir {
            std::fs::create_dir_all(dir)?;
            map = load_dir(dir, cfg.keep);
        }
        let agent = ureq::AgentBuilder::new()
            .timeout(cfg.timeout)
            .user_agent(&cfg.user_agent)
            .max_idle_connections_per_host(4)
            .build();
        let inner = Arc::new(Inner {
            cfg,
            resolver: Box::new(resolver),
            policy: RwLock::new(policy),
            map: RwLock::new(map),
            jobs: Mutex::new(Jobs::default()),
            jobs_cv: Condvar::new(),
            dirty: Mutex::new(Dirty::default()),
            dirty_cv: Condvar::new(),
            inflight: Mutex::new(HashMap::new()),
            listeners: RwLock::new(Vec::new()),
            gate: RwLock::new(None),
            agent,
            n: Counters::default(),
        });
        let store = Store { inner };
        store.trim();
        for i in 0..store.inner.cfg.workers.max(1) {
            let s = store.clone();
            std::thread::Builder::new().name(format!("rare-state-{i}")).spawn(move || s.worker())?;
        }
        if store.inner.cfg.dir.is_some() {
            let s = store.clone();
            std::thread::Builder::new().name("rare-state-disk".into()).spawn(move || s.writer())?;
        }
        Ok(store)
    }

    // ---------- reads ----------

    /// Reads a key by its policy: cached data at once (revalidated behind when older than its TTL),
    /// or a fetch the caller waits on. Local keys (no request) answer only what was put.
    pub fn get(&self, key: &str) -> Result<Hit, String> {
        let f = self.inner.policy.read().unwrap().for_key(key);
        self.get_with(key, f)
    }

    /// [`Store::get`] with an explicit freshness.
    pub fn get_with(&self, key: &str, f: Freshness) -> Result<Hit, String> {
        let now = now_ms();
        let cached = self.touch(key, now);
        let ttl = f.ttl().as_millis() as u64;
        if let Some(h) = &cached {
            let old = now.saturating_sub(h.fetched_ms) > ttl;
            match f {
                Freshness::Swr { .. } => {
                    self.inner.n.hits.fetch_add(1, Ordering::Relaxed);
                    if old {
                        self.revalidate(key);
                    }
                    return Ok(Hit { stale: old, ..h.clone() });
                }
                Freshness::Fresh { .. } if !old => {
                    self.inner.n.hits.fetch_add(1, Ordering::Relaxed);
                    return Ok(h.clone());
                }
                Freshness::Fresh { .. } => {}
            }
        }
        self.inner.n.misses.fetch_add(1, Ordering::Relaxed);
        match self.fetch_shared(key) {
            Ok(h) => Ok(h),
            Err(e) => match (f, cached) {
                (Freshness::Fresh { offline_ok: true, .. }, Some(h)) => Ok(Hit { stale: true, ..h }),
                _ => Err(e),
            },
        }
    }

    /// What is cached for a key, without fetching, counting a use or revalidating.
    pub fn peek(&self, key: &str) -> Option<Hit> {
        let map = self.inner.map.read().unwrap();
        map.get(key).map(|e| Hit { body: e.body.clone(), fetched_ms: e.fetched, stale: false })
    }

    /// Fetches a key now and waits, whatever is cached.
    pub fn refresh(&self, key: &str) -> Result<Hit, String> {
        self.fetch_shared(key)
    }

    /// Queues a refetch of the key ahead of background work. No-op for local keys.
    pub fn revalidate(&self, key: &str) {
        self.enqueue(key, true);
    }

    // ---------- local state ----------

    /// Stores JSON the host computed (wallet balances, friends, the last screen's data). Listeners
    /// hear about it when the content changed.
    pub fn put(&self, key: &str, json: impl Into<String>) -> Result<(), String> {
        let body: String = json.into();
        if !is_json(&body) {
            return Err("not JSON".into());
        }
        self.store(key, body.into(), None, true);
        Ok(())
    }

    pub fn remove(&self, key: &str) {
        let had = self.inner.map.write().unwrap().remove(key).is_some();
        if had {
            self.mark_dirty(key);
            self.notify(key, None);
        }
    }

    // ---------- hooks ----------

    /// Calls `f` for every key whose content changes (a revalidation that brought new data, a put, a
    /// removal). Not for 304s or identical bodies.
    pub fn subscribe(&self, f: impl Fn(&str, Option<&Hit>) + Send + Sync + 'static) {
        self.inner.listeners.write().unwrap().push(Arc::new(f));
    }

    /// Background fetches (revalidations and warming) run only while `f` says so. Waited-on misses
    /// and refreshes always run: someone is looking at the screen that asked.
    pub fn set_gate(&self, f: impl Fn() -> bool + Send + Sync + 'static) {
        *self.inner.gate.write().unwrap() = Some(Arc::new(f));
    }

    pub fn set_policy(&self, p: Policy) {
        *self.inner.policy.write().unwrap() = p;
    }

    pub fn policy_for(&self, key: &str) -> Freshness {
        self.inner.policy.read().unwrap().for_key(key)
    }

    // ---------- start-up ----------

    /// Revalidates, in the background and most recently used first, every fetchable key read within
    /// `since` whose data is older than its TTL. Returns how many were queued.
    pub fn warm(&self, since: Duration, limit: usize) -> usize {
        let now = now_ms();
        let floor = now.saturating_sub(since.as_millis() as u64);
        let policy = self.inner.policy.read().unwrap().clone();
        let mut keys: Vec<(u64, String)> = {
            let map = self.inner.map.read().unwrap();
            map.iter()
                .filter(|(k, e)| !e.local && e.used >= floor && now.saturating_sub(e.fetched) > policy.for_key(k).ttl().as_millis() as u64)
                .map(|(k, e)| (e.used, k.clone()))
                .collect()
        };
        keys.sort_by(|a, b| b.0.cmp(&a.0));
        keys.truncate(limit);
        let n = keys.len();
        for (_, k) in keys {
            self.enqueue(&k, false);
        }
        n
    }

    /// The most recently used entries whose key starts with `prefix`, used within `since`, up to
    /// `max_bytes` of bodies: what a UI seeds itself with before its first frame.
    pub fn hot(&self, prefix: &str, since: Duration, max_bytes: usize) -> Vec<(String, Hit)> {
        let floor = now_ms().saturating_sub(since.as_millis() as u64);
        let map = self.inner.map.read().unwrap();
        let mut v: Vec<(&String, &Entry)> = map.iter().filter(|(k, e)| k.starts_with(prefix) && e.used >= floor).collect();
        v.sort_by(|a, b| b.1.used.cmp(&a.1.used));
        let mut total = 0;
        let mut out = Vec::new();
        for (k, e) in v {
            if total + e.body.len() > max_bytes {
                continue;
            }
            total += e.body.len();
            out.push((k.clone(), Hit { body: e.body.clone(), fetched_ms: e.fetched, stale: false }));
        }
        out
    }

    /// [`Store::hot`] as one JSON object, `{"<key>": {"at": <fetched ms>, "v": <body>}, ...}`,
    /// built without parsing the bodies.
    pub fn hot_json(&self, prefix: &str, since: Duration, max_bytes: usize) -> String {
        let hot = self.hot(prefix, since, max_bytes);
        let mut s = String::with_capacity(hot.iter().map(|(k, h)| k.len() + h.body.len() + 32).sum::<usize>() + 2);
        s.push('{');
        for (i, (k, h)) in hot.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&serde_json::to_string(k).unwrap_or_default());
            s.push_str(":{\"at\":");
            s.push_str(&h.fetched_ms.to_string());
            s.push_str(",\"v\":");
            s.push_str(&h.body);
            s.push('}');
        }
        s.push('}');
        s
    }

    /// Writes everything pending to disk now.
    pub fn flush(&self) {
        let keys: Vec<String> = std::mem::take(&mut self.inner.dirty.lock().unwrap().keys).into_iter().collect();
        self.write_keys(&keys);
    }

    pub fn stats(&self) -> Stats {
        let map = self.inner.map.read().unwrap();
        let n = &self.inner.n;
        Stats {
            entries: map.len(),
            bytes: map.values().map(|e| e.body.len()).sum(),
            hits: n.hits.load(Ordering::Relaxed),
            misses: n.misses.load(Ordering::Relaxed),
            fetches: n.fetches.load(Ordering::Relaxed),
            not_modified: n.not_modified.load(Ordering::Relaxed),
            unchanged: n.unchanged.load(Ordering::Relaxed),
            changed: n.changed.load(Ordering::Relaxed),
            errors: n.errors.load(Ordering::Relaxed),
            gated: n.gated.load(Ordering::Relaxed),
        }
    }

    /// Stops the workers and the writer after writing what is pending.
    pub fn shutdown(&self) {
        {
            let mut j = self.inner.jobs.lock().unwrap();
            j.stop = true;
        }
        self.inner.jobs_cv.notify_all();
        self.flush();
        self.inner.dirty.lock().unwrap().stop = true;
        self.inner.dirty_cv.notify_all();
    }

    // ---------- internals ----------

    /// The cached hit, counting the read (persisted at most hourly).
    fn touch(&self, key: &str, now: u64) -> Option<Hit> {
        let (hit, persist) = {
            let mut map = self.inner.map.write().unwrap();
            let e = map.get_mut(key)?;
            e.used = now;
            let persist = now.saturating_sub(e.saved_used) > HOUR_MS;
            (Hit { body: e.body.clone(), fetched_ms: e.fetched, stale: false }, persist)
        };
        if persist {
            self.mark_dirty(key);
        }
        Some(hit)
    }

    fn enqueue(&self, key: &str, fg: bool) {
        if (self.inner.resolver)(key).is_none() {
            return;
        }
        let mut j = self.inner.jobs.lock().unwrap();
        if j.queued.contains(key) {
            // Already waiting: a foreground ask moves it to the front.
            if fg {
                if let Some(i) = j.bg.iter().position(|k| k == key) {
                    j.bg.remove(i);
                    j.fg.push_back(key.to_string());
                }
            }
            return;
        }
        j.queued.insert(key.to_string());
        if fg {
            j.fg.push_back(key.to_string());
        } else {
            j.bg.push_back(key.to_string());
        }
        drop(j);
        self.inner.jobs_cv.notify_one();
    }

    fn worker(&self) {
        loop {
            let key = {
                let mut j = self.inner.jobs.lock().unwrap();
                loop {
                    if j.stop {
                        return;
                    }
                    if let Some(k) = j.fg.pop_front().or_else(|| j.bg.pop_front()) {
                        j.queued.remove(&k);
                        break k;
                    }
                    j = self.inner.jobs_cv.wait(j).unwrap();
                }
            };
            let open = self.inner.gate.read().unwrap().clone().map(|g| g()).unwrap_or(true);
            if !open {
                // A game has the screen. The key revalidates the next time something reads it.
                self.inner.n.gated.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            // Fetched meanwhile (a waited-on miss, another revalidation): nothing to do.
            let ttl = self.policy_for(&key).ttl().as_millis() as u64;
            if self.peek(&key).is_some_and(|h| now_ms().saturating_sub(h.fetched_ms) <= ttl) {
                continue;
            }
            let _ = self.fetch_shared(&key);
        }
    }

    /// One fetch per key at a time; concurrent callers wait for the same answer.
    fn fetch_shared(&self, key: &str) -> Result<Hit, String> {
        let (flight, leader) = {
            let mut inf = self.inner.inflight.lock().unwrap();
            match inf.get(key) {
                Some(f) => (f.clone(), false),
                None => {
                    let f = Arc::new(Flight { result: Mutex::new(None), cv: Condvar::new() });
                    inf.insert(key.to_string(), f.clone());
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
        let res = self.fetch(key);
        *flight.result.lock().unwrap() = Some(res.clone());
        flight.cv.notify_all();
        self.inner.inflight.lock().unwrap().remove(key);
        res
    }

    fn fetch(&self, key: &str) -> Result<Hit, String> {
        let Some(req) = (self.inner.resolver)(key) else {
            return self.peek(key).ok_or_else(|| "not cached".to_string());
        };
        self.inner.n.fetches.fetch_add(1, Ordering::Relaxed);
        let etag = self.inner.map.read().unwrap().get(key).and_then(|e| e.etag.clone());
        let mut r = self.inner.agent.get(&req.url);
        for (h, v) in &req.headers {
            r = r.set(h, v);
        }
        if let Some(t) = &etag {
            r = r.set("If-None-Match", t);
        }
        if let Some(t) = test_token(&req.url) {
            r = r.set("x-rare-test", &t);
        }
        let t0 = std::time::Instant::now();
        let r = r.call();
        trace(t0, match &r { Ok(x) => x.status(), Err(ureq::Error::Status(c, _)) => *c, Err(_) => 0 }, 0, &req.url);
        let resp = match r {
            Ok(resp) => resp,
            Err(ureq::Error::Status(code, _)) => {
                self.inner.n.errors.fetch_add(1, Ordering::Relaxed);
                return Err(format!("{}: status code {code}", req.url));
            }
            Err(e) => {
                self.inner.n.errors.fetch_add(1, Ordering::Relaxed);
                return Err(e.to_string());
            }
        };
        let now = now_ms();
        if resp.status() == 304 {
            self.inner.n.not_modified.fetch_add(1, Ordering::Relaxed);
            let mut map = self.inner.map.write().unwrap();
            if let Some(e) = map.get_mut(key) {
                e.fetched = now;
                let hit = Hit { body: e.body.clone(), fetched_ms: now, stale: false };
                drop(map);
                self.mark_dirty(key);
                return Ok(hit);
            }
            return Err("304 for a key with nothing cached".into());
        }
        let new_etag = resp.header("ETag").map(String::from);
        let mut body = String::new();
        if let Err(e) = resp.into_reader().take(self.inner.cfg.max_body).read_to_string(&mut body) {
            self.inner.n.errors.fetch_add(1, Ordering::Relaxed);
            return Err(e.to_string());
        }
        if !is_json(&body) {
            self.inner.n.errors.fetch_add(1, Ordering::Relaxed);
            return Err(format!("{}: not JSON", req.url));
        }
        Ok(self.store(key, body.into(), new_etag, false))
    }

    /// Saves a body; listeners hear about it only when it differs from what was there.
    fn store(&self, key: &str, body: Arc<str>, etag: Option<String>, local: bool) -> Hit {
        let now = now_ms();
        let changed = {
            let mut map = self.inner.map.write().unwrap();
            match map.get_mut(key) {
                Some(e) => {
                    let changed = *e.body != *body;
                    if changed {
                        e.body = body.clone();
                    }
                    e.fetched = now;
                    e.used = e.used.max(now);
                    e.etag = etag;
                    e.local = local;
                    changed
                }
                None => {
                    map.insert(key.to_string(), Entry { body: body.clone(), fetched: now, used: now, saved_used: 0, etag, local });
                    true
                }
            }
        };
        self.mark_dirty(key);
        let hit = Hit { body, fetched_ms: now, stale: false };
        if changed {
            self.inner.n.changed.fetch_add(1, Ordering::Relaxed);
            self.trim();
            self.notify(key, Some(&hit));
        } else {
            self.inner.n.unchanged.fetch_add(1, Ordering::Relaxed);
        }
        hit
    }

    fn notify(&self, key: &str, hit: Option<&Hit>) {
        let ls: Vec<Arc<Listener>> = self.inner.listeners.read().unwrap().clone();
        for l in ls {
            l(key, hit);
        }
    }

    /// Drops the least recently used entries while the bodies exceed `max_bytes`.
    fn trim(&self) {
        let mut gone = Vec::new();
        {
            let mut map = self.inner.map.write().unwrap();
            let mut total: usize = map.values().map(|e| e.body.len()).sum();
            if total <= self.inner.cfg.max_bytes {
                return;
            }
            let mut by_use: Vec<(u64, String, usize)> = map.iter().map(|(k, e)| (e.used, k.clone(), e.body.len())).collect();
            by_use.sort();
            for (_, k, len) in by_use {
                if total <= self.inner.cfg.max_bytes {
                    break;
                }
                map.remove(&k);
                total -= len;
                gone.push(k);
            }
        }
        for k in gone {
            self.mark_dirty(&k);
        }
    }

    fn mark_dirty(&self, key: &str) {
        if self.inner.cfg.dir.is_none() {
            return;
        }
        self.inner.dirty.lock().unwrap().keys.insert(key.to_string());
        self.inner.dirty_cv.notify_one();
    }

    fn writer(&self) {
        loop {
            {
                let mut d = self.inner.dirty.lock().unwrap();
                while d.keys.is_empty() && !d.stop {
                    d = self.inner.dirty_cv.wait(d).unwrap();
                }
                if d.stop {
                    return;
                }
            }
            // Collect a burst (a warm-up refreshes dozens of keys) into one pass.
            std::thread::sleep(Duration::from_millis(400));
            let keys: Vec<String> = std::mem::take(&mut self.inner.dirty.lock().unwrap().keys).into_iter().collect();
            self.write_keys(&keys);
        }
    }

    fn write_keys(&self, keys: &[String]) {
        let Some(dir) = &self.inner.cfg.dir else { return };
        for k in keys {
            let path = dir.join(format!("{:016x}.st", fnv(k)));
            let snapshot = {
                let mut map = self.inner.map.write().unwrap();
                map.get_mut(k).map(|e| {
                    e.saved_used = e.used;
                    (Header { k: k.clone(), f: e.fetched, u: e.used, e: e.etag.clone(), l: e.local }, e.body.clone())
                })
            };
            match snapshot {
                None => {
                    let _ = std::fs::remove_file(&path);
                }
                Some((h, body)) => {
                    let tmp = path.with_extension("tmp");
                    let ok = (|| -> std::io::Result<()> {
                        let mut f = std::fs::File::create(&tmp)?;
                        f.write_all(serde_json::to_string(&h).unwrap_or_default().as_bytes())?;
                        f.write_all(b"\n")?;
                        f.write_all(body.as_bytes())?;
                        std::fs::rename(&tmp, &path)
                    })();
                    if ok.is_err() {
                        let _ = std::fs::remove_file(&tmp);
                    }
                }
            }
        }
    }
}

/// Reads every entry file; broken files and entries unused for `keep` are deleted.
fn load_dir(dir: &std::path::Path, keep: Duration) -> HashMap<String, Entry> {
    let floor = now_ms().saturating_sub(keep.as_millis() as u64);
    let mut map = HashMap::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return map };
    for de in rd.flatten() {
        let path = de.path();
        match path.extension().and_then(|e| e.to_str()) {
            Some("st") => {}
            Some("tmp") => {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            _ => continue,
        }
        let parsed = std::fs::read_to_string(&path).ok().and_then(|s| {
            let (head, body) = s.split_once('\n')?;
            let h: Header = serde_json::from_str(head).ok()?;
            Some((h, body.to_string()))
        });
        match parsed {
            Some((h, body)) if h.u >= floor && !body.is_empty() => {
                map.insert(h.k, Entry { body: body.into(), fetched: h.f, used: h.u, saved_used: h.u, etag: h.e, local: h.l });
            }
            _ => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    map
}

#[cfg(test)]
mod tests;
