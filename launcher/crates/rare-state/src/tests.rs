use super::*;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::sync::atomic::AtomicUsize;

/// A one-route JSON server: answers `body()` with an ETag of its text, 304 when If-None-Match
/// matches, and counts requests. `down` makes it refuse with a 503.
struct Server {
    port: u16,
    hits: Arc<AtomicUsize>,
    body: Arc<Mutex<String>>,
    down: Arc<std::sync::atomic::AtomicBool>,
    delay_ms: Arc<AtomicU64>,
}

fn server(initial: &str) -> Server {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let body = Arc::new(Mutex::new(initial.to_string()));
    let down = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let delay_ms = Arc::new(AtomicU64::new(0));
    let (h, b, d, dl) = (hits.clone(), body.clone(), down.clone(), delay_ms.clone());
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let (h, b, d, dl) = (h.clone(), b.clone(), d.clone(), dl.clone());
            std::thread::spawn(move || {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut inm = None;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("if-none-match:") {
                        inm = Some(v.trim().to_string());
                    }
                }
                h.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(dl.load(Ordering::SeqCst)));
                let mut s = s;
                if d.load(Ordering::SeqCst) {
                    let _ = s.write_all(b"HTTP/1.1 503 Down\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    return;
                }
                let body = b.lock().unwrap().clone();
                let tag = format!("\"{:x}\"", fnv(&body));
                if inm.as_deref() == Some(tag.as_str()) {
                    let _ = s.write_all(format!("HTTP/1.1 304 Not Modified\r\nETag: {tag}\r\nConnection: close\r\n\r\n").as_bytes());
                    return;
                }
                let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nETag: {tag}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes());
            });
        }
    });
    Server { port, hits, body, down, delay_ms }
}

fn open(dir: Option<PathBuf>, port: u16, policy: Policy) -> Store {
    let cfg = Config { dir, workers: 2, ..Config::default() };
    Store::open(cfg, policy, move |k: &str| k.strip_prefix("web:").map(|p| Request::get(format!("http://127.0.0.1:{port}{p}")))).unwrap()
}

fn wait_until(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..200 {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn miss_fetches_then_serves_from_memory() {
    let srv = server(r#"{"a":1}"#);
    let st = open(None, srv.port, Policy::new(Freshness::swr_secs(60)));
    assert_eq!(&*st.get("web:/x").unwrap().body, r#"{"a":1}"#);
    assert_eq!(&*st.get("web:/x").unwrap().body, r#"{"a":1}"#);
    assert_eq!(srv.hits.load(Ordering::SeqCst), 1);
    let s = st.stats();
    assert_eq!((s.misses, s.hits, s.fetches), (1, 1, 1));
}

#[test]
fn stale_answers_at_once_and_revalidates_behind() {
    let srv = server(r#"{"v":1}"#);
    let st = open(None, srv.port, Policy::new(Freshness::Swr { ttl: Duration::ZERO }));
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let s2 = seen.clone();
    st.subscribe(move |k, h| s2.lock().unwrap().push(format!("{k}={}", h.map(|h| h.body.to_string()).unwrap_or_default())));
    st.get("web:/p").unwrap();
    *srv.body.lock().unwrap() = r#"{"v":2}"#.into();
    srv.delay_ms.store(150, Ordering::SeqCst);
    let t = std::time::Instant::now();
    std::thread::sleep(Duration::from_millis(2));
    let h = st.get("web:/p").unwrap();
    assert!(t.elapsed() < Duration::from_millis(100), "a stale read must not wait on the network");
    assert!(h.stale);
    assert_eq!(&*h.body, r#"{"v":1}"#);
    assert!(wait_until(|| st.peek("web:/p").is_some_and(|h| &*h.body == r#"{"v":2}"#)));
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen, vec![r#"web:/p={"v":1}"#.to_string(), r#"web:/p={"v":2}"#.to_string()]);
}

#[test]
fn not_modified_and_identical_bodies_stay_quiet() {
    let srv = server(r#"{"same":true}"#);
    let st = open(None, srv.port, Policy::new(Freshness::Swr { ttl: Duration::ZERO }));
    let n = Arc::new(AtomicUsize::new(0));
    let n2 = n.clone();
    st.subscribe(move |_, _| {
        n2.fetch_add(1, Ordering::SeqCst);
    });
    st.get("web:/q").unwrap();
    std::thread::sleep(Duration::from_millis(2));
    st.refresh("web:/q").unwrap();
    assert_eq!(st.stats().not_modified, 1);
    assert_eq!(n.load(Ordering::SeqCst), 1, "only the first fill is a change");
}

#[test]
fn concurrent_misses_share_one_request() {
    let srv = server(r#"[1,2,3]"#);
    srv.delay_ms.store(100, Ordering::SeqCst);
    let st = open(None, srv.port, Policy::new(Freshness::swr_secs(60)));
    let ts: Vec<_> = (0..8)
        .map(|_| {
            let st = st.clone();
            std::thread::spawn(move || st.get("web:/c").unwrap().body.to_string())
        })
        .collect();
    for t in ts {
        assert_eq!(t.join().unwrap(), "[1,2,3]");
    }
    assert_eq!(srv.hits.load(Ordering::SeqCst), 1);
}

#[test]
fn fresh_policy_refetches_and_falls_back_offline_only_when_allowed() {
    let srv = server(r#"{"price":1}"#);
    let strict = Freshness::Fresh { ttl: Duration::ZERO, offline_ok: false };
    let lenient = Freshness::Fresh { ttl: Duration::ZERO, offline_ok: true };
    let st = open(None, srv.port, Policy::new(strict));
    st.get("web:/f").unwrap();
    std::thread::sleep(Duration::from_millis(2));
    st.get("web:/f").unwrap();
    assert_eq!(srv.hits.load(Ordering::SeqCst), 2, "Fresh with ttl 0 always asks");
    srv.down.store(true, Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(2));
    assert!(st.get_with("web:/f", strict).is_err());
    let h = st.get_with("web:/f", lenient).unwrap();
    assert!(h.stale);
    assert_eq!(&*h.body, r#"{"price":1}"#);
}

#[test]
fn bad_json_is_never_cached() {
    let srv = server("<html>oops</html>");
    let st = open(None, srv.port, Policy::new(Freshness::swr_secs(60)));
    assert!(st.get("web:/h").is_err());
    assert!(st.peek("web:/h").is_none());
}

#[test]
fn persists_across_restarts_with_used_order() {
    let dir = tempfile::tempdir().unwrap();
    let srv = server(r#"{"games":["a"]}"#);
    {
        let st = open(Some(dir.path().into()), srv.port, Policy::new(Freshness::swr_secs(60)));
        st.get("web:/games").unwrap();
        st.put("ui:wallet", r#"{"sui":"12.5"}"#).unwrap();
        assert!(st.put("ui:bad", "{nope").is_err());
        st.flush();
    }
    srv.down.store(true, Ordering::SeqCst);
    let st = open(Some(dir.path().into()), srv.port, Policy::new(Freshness::swr_secs(60)));
    // Cold start, network down: both answer from disk at once.
    assert_eq!(&*st.get("web:/games").unwrap().body, r#"{"games":["a"]}"#);
    assert_eq!(&*st.get("ui:wallet").unwrap().body, r#"{"sui":"12.5"}"#);
    let hot = st.hot_json("", Duration::from_secs(60), 1 << 20);
    let v: serde_json::Value = serde_json::from_str(&hot).unwrap();
    assert_eq!(v["ui:wallet"]["v"]["sui"], "12.5");
    assert!(v["web:/games"]["at"].as_u64().unwrap() > 0);
    // Only one prefix.
    let ui: serde_json::Value = serde_json::from_str(&st.hot_json("ui:", Duration::from_secs(60), 1 << 20)).unwrap();
    assert_eq!(ui.as_object().unwrap().len(), 1);
}

#[test]
fn removal_and_trim_reach_disk() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = Config { dir: Some(dir.path().into()), workers: 1, max_bytes: 40, ..Config::default() };
    let st = Store::open(cfg.clone(), Policy::new(Freshness::swr_secs(60)), |_: &str| None).unwrap();
    st.put("a", r#""aaaaaaaaaaaaaaaaaaaa""#).unwrap(); // 22 bytes
    std::thread::sleep(Duration::from_millis(3));
    st.put("b", r#""bbbbbbbbbbbbbbbbbbbb""#).unwrap(); // 44 in all: "a" goes
    assert!(st.peek("a").is_none());
    st.put("c", "1").unwrap();
    st.remove("c");
    st.flush();
    let files = std::fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(files, 1);
    let st2 = Store::open(cfg, Policy::new(Freshness::swr_secs(60)), |_: &str| None).unwrap();
    assert!(st2.peek("b").is_some() && st2.peek("a").is_none() && st2.peek("c").is_none());
}

#[test]
fn warm_revalidates_recent_stale_keys_and_respects_the_gate() {
    let srv = server(r#"{"n":1}"#);
    let st = open(None, srv.port, Policy::new(Freshness::Swr { ttl: Duration::ZERO }));
    st.get("web:/w1").unwrap();
    st.get("web:/w2").unwrap();
    st.put("ui:local", "true").unwrap();
    assert_eq!(srv.hits.load(Ordering::SeqCst), 2);
    let open_gate = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let g = open_gate.clone();
    st.set_gate(move || g.load(Ordering::SeqCst));
    std::thread::sleep(Duration::from_millis(2));
    assert_eq!(st.warm(Duration::from_secs(60), 10), 2, "local keys are not warmed");
    assert!(wait_until(|| st.stats().gated == 2));
    assert_eq!(srv.hits.load(Ordering::SeqCst), 2, "nothing fetched while a game runs");
    open_gate.store(true, Ordering::SeqCst);
    assert_eq!(st.warm(Duration::from_secs(60), 10), 2);
    assert!(wait_until(|| srv.hits.load(Ordering::SeqCst) == 4));
    // A foreground read with the gate closed still answers (from cache) at once.
    open_gate.store(false, Ordering::SeqCst);
    assert!(st.get("web:/w1").is_ok());
}

#[test]
fn policy_rules_match_in_order() {
    let p = Policy::new(Freshness::swr_secs(300)).rule("/v1/items/", Freshness::fresh_secs(5)).rule("/v1/", Freshness::swr_secs(30));
    assert_eq!(p.for_key("rare:/v1/items/0x1"), Freshness::fresh_secs(5));
    assert_eq!(p.for_key("rare:/v1/games"), Freshness::swr_secs(30));
    assert_eq!(p.for_key("rare:/games/games.json"), Freshness::swr_secs(300));
}

/// `cargo test --release -p rare-state -- --ignored --nocapture`: cold open and read costs.
#[test]
#[ignore]
fn bench_store() {
    let dir = tempfile::tempdir().unwrap();
    let body = format!("{{\"data\":[{}]}}", (0..60).map(|i| format!("{{\"id\":\"0x{i:064x}\",\"name\":\"Item {i}\",\"floorMist\":\"{}\"}}", i * 1_000_000)).collect::<Vec<_>>().join(","));
    {
        let st = Store::open(Config { dir: Some(dir.path().into()), ..Config::default() }, Policy::new(Freshness::swr_secs(60)), |_: &str| None).unwrap();
        for i in 0..600 {
            st.put(&format!("rare:/v1/collections/c{i}/items"), body.clone()).unwrap();
        }
        st.flush();
    }
    let t = std::time::Instant::now();
    let st = Store::open(Config { dir: Some(dir.path().into()), ..Config::default() }, Policy::new(Freshness::swr_secs(60)), |_: &str| None).unwrap();
    let open = t.elapsed();
    let t = std::time::Instant::now();
    for i in 0..10_000 {
        st.get(&format!("rare:/v1/collections/c{}/items", i % 600)).unwrap();
    }
    let read = t.elapsed() / 10_000;
    let t = std::time::Instant::now();
    let hot = st.hot_json("rare:", Duration::from_secs(3600), 2 << 20);
    let boot = t.elapsed();
    println!("open 600 entries ({} KB) {:.1} ms, read {:.2} us, boot snapshot {} KB in {:.2} ms", st.stats().bytes / 1024, open.as_secs_f64() * 1e3, read.as_secs_f64() * 1e6, hot.len() / 1024, boot.as_secs_f64() * 1e3);
}
