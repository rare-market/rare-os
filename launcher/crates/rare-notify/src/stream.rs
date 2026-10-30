//! The live notice stream: one server-sent events connection for every Rare Wallet account (signed
//! in, hardware and watch-only) plus this launcher's device id, on its own thread.
//!
//! It resumes where it left off: the last notice id is kept in storage and every (re)connect asks
//! for `?after=<id>` and sends `Last-Event-ID`, so a launcher restart and a dropped connection both
//! pick up from there. Reconnects follow the web launcher's EventSource: a dropped or failed
//! connection retries after the server's `retry:` (3 s); a non-2xx answer backs off from 3 s,
//! doubling to a minute. A connection silent for 45 s (the server sends `: hb` every 15 s) counts
//! as dead. Whenever the accounts change, `recent` is fetched to fill the centre.
//!
//! Events go to a sink (usually a channel the UI thread drains into `Notifier::handle`). The sink
//! runs on the stream's threads, so it should hand the event off and return, not call back into
//! the stream.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rare_market::net::{Call, Net, NetError};
use serde_json::Value;

use crate::api;
use crate::logic::{max_id, norm_addrs, stream_query, uri_component};
use crate::sse::{SseEvent, SseParser};
use crate::storage::{read_value, write_value, Storage};
use crate::store::Status;
use crate::types::Notice;

const K_LAST: &str = "rare.notify.last";

/// The stream's first frame: the server's clock and the addresses it subscribed.
#[derive(Clone, Debug, PartialEq)]
pub struct Hello {
    pub now: i64,
    pub addrs: Vec<String>,
    pub social: Option<String>,
}

/// A friend event (`presence`, `friend`, `party`, `chat`), only on a connection with a valid
/// social token. The friends module handles it.
#[derive(Clone, Debug, PartialEq)]
pub struct SocialEvent {
    pub event: String,
    pub data: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StreamEvent {
    Status(Status),
    Hello(Hello),
    /// A notice from the live stream (boxed: it is much larger than the other events).
    Notice(Box<Notice>),
    /// `recent` answered (newest first): a backfill, not live.
    Recent(Vec<Notice>),
    Social(SocialEvent),
}

/// The signed token that adds friend events to the stream (none: no friend events). Called on the
/// stream's thread before each connection.
pub type SocialToken = Arc<dyn Fn() -> Option<String> + Send + Sync>;

type Sink = Arc<dyn Fn(StreamEvent) + Send + Sync>;

#[derive(Clone, Debug)]
pub struct StreamOptions {
    /// Wait after a dropped or failed connection, until the server sends its own `retry:`.
    pub retry: Duration,
    /// The first wait after a non-2xx answer, doubling to `max_backoff`.
    pub backoff: Duration,
    pub max_backoff: Duration,
    /// No bytes for this long (heartbeats included) is a dead connection.
    pub dead_after: Duration,
    /// The HTTP deadline of one connection; past it the stream reconnects and resumes.
    pub connection_deadline: Duration,
    /// Connect even with no accounts and no price alerts (the mock notices).
    pub without_accounts: bool,
}

impl Default for StreamOptions {
    fn default() -> Self {
        StreamOptions {
            retry: Duration::from_millis(3000),
            backoff: Duration::from_millis(3000),
            max_backoff: Duration::from_secs(60),
            dead_after: Duration::from_secs(45),
            connection_deadline: Duration::from_secs(3600),
            without_accounts: false,
        }
    }
}

enum Msg {
    Line(u64, String),
    End(u64),
    /// The connection this thread serves was replaced or stopped.
    Wake,
}

struct State {
    addrs: Vec<String>,
    device: String,
    has_alerts: bool,
    social: Option<SocialToken>,
    last: Option<String>,
    /// Bumped by every connect and stop; a thread whose generation is old exits.
    gen: u64,
    started: bool,
    /// A connection thread is running for the current generation.
    active: bool,
    wake: Option<Sender<Msg>>,
}

struct Shared {
    net: Arc<dyn Net>,
    storage: Arc<dyn Storage>,
    sink: Sink,
    opts: StreamOptions,
    st: Mutex<State>,
}

/// The stream client. Dropping it stops the stream.
pub struct NotifyStream {
    sh: Arc<Shared>,
}

impl NotifyStream {
    pub fn new(net: Arc<dyn Net>, storage: Arc<dyn Storage>, device: String, opts: StreamOptions, sink: impl Fn(StreamEvent) + Send + Sync + 'static) -> Self {
        let last = read_value(storage.as_ref(), K_LAST).and_then(|v| v.as_str().map(str::to_string)).filter(|v| crate::logic::is_id(v));
        let st = State { addrs: Vec::new(), device, has_alerts: false, social: None, last, gen: 0, started: false, active: false, wake: None };
        NotifyStream { sh: Arc::new(Shared { net, storage, sink: Arc::new(sink), opts, st: Mutex::new(st) }) }
    }

    /// A stream whose events arrive on a channel.
    pub fn channel(net: Arc<dyn Net>, storage: Arc<dyn Storage>, device: String, opts: StreamOptions) -> (Self, Receiver<StreamEvent>) {
        let (tx, rx) = mpsc::channel();
        (Self::new(net, storage, device, opts, move |e| drop(tx.send(e))), rx)
    }

    /// Starts once with the accounts' addresses: connects and fetches `recent`.
    pub fn start(&self, accounts: &[String]) {
        {
            let mut st = self.sh.st.lock().unwrap();
            if st.started {
                return;
            }
            st.started = true;
            st.addrs = norm_addrs(accounts);
        }
        self.connect();
        self.load_recent();
    }

    /// The accounts changed (added, removed, a watch address): re-subscribe right away. Nothing
    /// happens when the normalised list is the same.
    pub fn set_accounts(&self, accounts: &[String]) {
        let next = norm_addrs(accounts);
        let started = {
            let mut st = self.sh.st.lock().unwrap();
            if st.addrs == next {
                return;
            }
            st.addrs = next;
            st.started
        };
        if started {
            self.connect();
            self.load_recent();
        }
    }

    pub fn accounts(&self) -> Vec<String> {
        self.sh.st.lock().unwrap().addrs.clone()
    }

    /// Whether this device has active price alerts. A device with alerts but no accounts still
    /// needs the stream for the alerts themselves.
    pub fn set_has_alerts(&self, on: bool) {
        let go = {
            let mut st = self.sh.st.lock().unwrap();
            st.has_alerts = on;
            on && st.started && !st.active
        };
        if go {
            self.connect();
        }
    }

    /// The friends module's token provider (the next connection uses it; call `refresh` to apply
    /// it now).
    pub fn set_social_token_provider(&self, f: SocialToken) {
        self.sh.st.lock().unwrap().social = Some(f);
    }

    /// Reconnects now (friends linked or unlinked, the token due for renewal, a resume).
    pub fn refresh(&self) {
        if self.sh.st.lock().unwrap().started {
            self.connect();
        }
    }

    /// Fetches `recent` on a worker thread; the answer arrives as [`StreamEvent::Recent`].
    /// Offline (or not deployed yet) it says nothing: the stream fills in.
    pub fn load_recent(&self) {
        let sh = self.sh.clone();
        let (addrs, device) = {
            let st = sh.st.lock().unwrap();
            (st.addrs.clone(), st.device.clone())
        };
        std::thread::spawn(move || {
            if let Ok(list) = api::recent(sh.net.as_ref(), &addrs, &device, api::RECENT_LIMIT) {
                (sh.sink)(StreamEvent::Recent(list));
            }
        });
    }

    /// The resume point: the largest notice id the stream has delivered.
    pub fn last_id(&self) -> Option<String> {
        self.sh.st.lock().unwrap().last.clone()
    }

    pub fn stop(&self) {
        let was = {
            let mut st = self.sh.st.lock().unwrap();
            st.gen += 1;
            if let Some(w) = st.wake.take() {
                let _ = w.send(Msg::Wake);
            }
            st.active = false;
            std::mem::replace(&mut st.started, false)
        };
        if was {
            (self.sh.sink)(StreamEvent::Status(Status::Off));
        }
    }

    fn connect(&self) {
        let (tx, rx) = mpsc::channel();
        let (gen, on) = {
            let mut st = self.sh.st.lock().unwrap();
            st.gen += 1;
            if let Some(w) = st.wake.take() {
                let _ = w.send(Msg::Wake);
            }
            let on = !st.addrs.is_empty() || st.has_alerts || self.sh.opts.without_accounts;
            st.active = on;
            if on {
                st.wake = Some(tx.clone());
            }
            (st.gen, on)
        };
        if !on {
            (self.sh.sink)(StreamEvent::Status(Status::Off));
            return;
        }
        (self.sh.sink)(StreamEvent::Status(Status::Connecting));
        let sh = self.sh.clone();
        std::thread::spawn(move || sh.run(gen, tx, rx));
    }
}

impl Drop for NotifyStream {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Shared {
    fn current(&self, gen: u64) -> bool {
        self.st.lock().unwrap().gen == gen
    }

    /// Sends an event unless this thread's connection was replaced.
    fn emit(&self, gen: u64, e: StreamEvent) -> bool {
        if !self.current(gen) {
            return false;
        }
        (self.sink)(e);
        true
    }

    fn run(self: Arc<Self>, gen: u64, tx: Sender<Msg>, rx: Receiver<Msg>) {
        let mut backoff = self.opts.backoff;
        let mut retry = self.opts.retry;
        let mut attempt = 0u64;
        loop {
            if !self.current(gen) {
                return;
            }
            let provider = self.st.lock().unwrap().social.clone();
            let social = provider.and_then(|f| f());
            // A newer connect started while the token was being signed.
            if !self.current(gen) {
                return;
            }
            let call = self.call(social.as_deref());
            match self.net.stream(&call) {
                Err(NetError::Status(_)) | Err(NetError::NotAllowed(_)) => {
                    // A non-2xx answer: EventSource would give up; back off ourselves.
                    if !self.emit(gen, StreamEvent::Status(Status::Retrying)) || !wait(&rx, backoff) {
                        return;
                    }
                    backoff = (backoff * 2).min(self.opts.max_backoff);
                }
                Err(NetError::Network(_)) => {
                    if !self.emit(gen, StreamEvent::Status(Status::Retrying)) || !wait(&rx, retry) {
                        return;
                    }
                }
                Ok(lines) => {
                    attempt += 1;
                    backoff = self.opts.backoff;
                    if !self.emit(gen, StreamEvent::Status(Status::Live)) {
                        return;
                    }
                    let (a, txl) = (attempt, tx.clone());
                    std::thread::spawn(move || {
                        for l in lines {
                            let Ok(l) = l else { break };
                            if txl.send(Msg::Line(a, l)).is_err() {
                                return;
                            }
                        }
                        let _ = txl.send(Msg::End(a));
                    });
                    let mut parser = SseParser::new();
                    loop {
                        match rx.recv_timeout(self.opts.dead_after) {
                            Ok(Msg::Wake) | Err(RecvTimeoutError::Disconnected) => return,
                            Ok(Msg::Line(a, l)) if a == attempt => {
                                if let Some(ev) = parser.line(&l) {
                                    if !self.frame(gen, ev) {
                                        return;
                                    }
                                }
                                if let Some(r) = parser.retry() {
                                    retry = Duration::from_millis(r);
                                }
                            }
                            Ok(Msg::End(a)) if a == attempt => break,
                            // Lines from a connection already given up on.
                            Ok(_) => {}
                            Err(RecvTimeoutError::Timeout) => break,
                        }
                    }
                    if !self.emit(gen, StreamEvent::Status(Status::Retrying)) || !wait(&rx, retry) {
                        return;
                    }
                }
            }
        }
    }

    fn call(&self, social: Option<&str>) -> Call {
        let st = self.st.lock().unwrap();
        let mut query = stream_query(&st.addrs, &st.device, st.last.as_deref());
        if let Some(s) = social {
            query.push_str(&format!("&social={}", uri_component(s)));
        }
        let mut call = Call::get(api::stream_url(&self.net.market_origin(), &query)).timeout(self.opts.connection_deadline);
        if let Some(l) = &st.last {
            call = call.header("last-event-id", l.clone());
        }
        call
    }

    /// One complete frame; false when this connection was replaced.
    fn frame(&self, gen: u64, ev: SseEvent) -> bool {
        match ev.event.as_str() {
            "hello" => {
                let Ok(v) = serde_json::from_str::<Value>(&ev.data) else { return true };
                let h = Hello {
                    now: v.get("now").and_then(Value::as_f64).unwrap_or(0.0) as i64,
                    addrs: v.get("addrs").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
                    social: v.get("social").and_then(Value::as_str).map(str::to_string),
                };
                self.emit(gen, StreamEvent::Hello(h))
            }
            "notice" => {
                let Ok(mut n) = serde_json::from_str::<Notice>(&ev.data) else { return true };
                // The frame id is the notice id; trust it if the body lacks one.
                if n.id.is_empty() {
                    if let Some(id) = &ev.last_id {
                        n.id = id.clone();
                    }
                }
                if !self.current(gen) {
                    return false;
                }
                self.note_id(&n.id);
                self.emit(gen, StreamEvent::Notice(Box::new(n)))
            }
            "presence" | "friend" | "party" | "chat" | "dm" | "dm_read" | "device" => match serde_json::from_str::<Value>(&ev.data) {
                Ok(data) => self.emit(gen, StreamEvent::Social(SocialEvent { event: ev.event, data })),
                Err(_) => true,
            },
            _ => true,
        }
    }

    /// Moves the resume point forward (never back) and saves it.
    fn note_id(&self, id: &str) {
        let mut st = self.st.lock().unwrap();
        let next = max_id(st.last.as_deref(), Some(id));
        if next != st.last {
            st.last = next;
            write_value(self.storage.as_ref(), K_LAST, st.last.as_ref());
        }
    }
}

/// Sleeps for `d` unless the connection is replaced first (false).
fn wait(rx: &Receiver<Msg>, d: Duration) -> bool {
    let end = Instant::now() + d;
    loop {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return true;
        }
        match rx.recv_timeout(left) {
            Ok(Msg::Wake) | Err(RecvTimeoutError::Disconnected) => return false,
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => return true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemStorage;
    use rare_market::net::{Method, MockNet};

    fn opts() -> StreamOptions {
        StreamOptions {
            retry: Duration::from_millis(20),
            backoff: Duration::from_millis(20),
            max_backoff: Duration::from_millis(80),
            dead_after: Duration::from_millis(500),
            ..Default::default()
        }
    }

    const BASE: &str = "https://rare.market/v1/notify/stream";

    /// Events until `stop` says enough, or a timeout.
    fn collect(rx: &Receiver<StreamEvent>, mut stop: impl FnMut(&[StreamEvent]) -> bool) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        let end = Instant::now() + Duration::from_secs(5);
        while !stop(&out) {
            let left = end.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(e) => out.push(e),
                Err(_) => panic!("timed out; got {out:?}"),
            }
        }
        out
    }

    fn notices(ev: &[StreamEvent]) -> Vec<String> {
        ev.iter()
            .filter_map(|e| match e {
                StreamEvent::Notice(n) => Some(n.id.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn delivers_frames_and_resumes_after_the_last_id() {
        let net = MockNet::new();
        net.on_call(Method::Get, "https://rare.market/v1/notify/recent", 200, r#"{"data":[{"id":"3","kind":"sold","title":"r"}]}"#);
        net.on_stream(
            BASE,
            &[
                "retry: 20",
                "",
                "event: hello",
                r#"data: {"now":1800000000000,"addrs":["0xab"]}"#,
                "",
                ": hb",
                "id: 41",
                "event: notice",
                r#"data: {"kind":"sold","title":"a"}"#,
                "",
                "id: 42",
                "event: notice",
                r#"data: {"id":"42","kind":"sold","title":"b"}"#,
                "",
                "event: presence",
                r#"data: {"address":"0x9","state":"online"}"#,
                "",
                "event: notice",
                "data: {broken",
                "",
            ],
        );
        net.on_stream(BASE, &["id: 43", "event: notice", r#"data: {"id":"43","kind":"bought","title":"c"}"#, ""]);
        let storage: Arc<dyn Storage> = Arc::new(MemStorage::new());
        let (s, rx) = NotifyStream::channel(Arc::new(net.clone()), storage.clone(), "dev_1".into(), opts());
        s.start(&["0xAB".into()]);
        let ev = collect(&rx, |e| notices(e).len() == 3 && e.iter().any(|x| matches!(x, StreamEvent::Recent(_))));
        assert_eq!(notices(&ev), ["41", "42", "43"]);
        assert!(ev.contains(&StreamEvent::Hello(Hello { now: 1_800_000_000_000, addrs: vec!["0xab".into()], social: None })));
        assert!(ev.iter().any(|e| matches!(e, StreamEvent::Social(s) if s.event == "presence" && s.data["state"] == "online")));
        assert_eq!(ev[0], StreamEvent::Status(Status::Connecting));
        assert!(ev.contains(&StreamEvent::Status(Status::Live)));
        assert!(ev.contains(&StreamEvent::Status(Status::Retrying)));
        assert_eq!(s.last_id().as_deref(), Some("43"));
        assert_eq!(storage.get(K_LAST).as_deref(), Some("\"43\""));
        s.stop();
        let streams: Vec<Call> = net.calls().into_iter().filter(|c| c.url.starts_with(BASE)).collect();
        assert_eq!(streams[0].url, format!("{BASE}?addr=0xab&device=dev_1"));
        assert_eq!(streams[1].url, format!("{BASE}?addr=0xab&device=dev_1&after=42"));
        assert!(streams[1].headers.contains(&("last-event-id".into(), "42".into())));
    }

    #[test]
    fn a_restart_resumes_from_the_saved_id_with_the_social_token() {
        let net = MockNet::new();
        let storage: Arc<dyn Storage> = Arc::new(MemStorage::new());
        storage.set(K_LAST, Some("\"77\""));
        let (s, rx) = NotifyStream::channel(Arc::new(net.clone()), storage, "d".into(), opts());
        s.set_social_token_provider(Arc::new(|| Some("t+k/=".to_string())));
        s.start(&["0x1".into()]);
        collect(&rx, |e| e.iter().filter(|x| **x == StreamEvent::Status(Status::Retrying)).count() >= 2);
        s.stop();
        let c = net.calls().into_iter().find(|c| c.url.starts_with(BASE)).unwrap();
        assert_eq!(c.url, format!("{BASE}?addr=0x1&device=d&after=77&social=t%2Bk%2F%3D"));
    }

    #[test]
    fn is_off_without_accounts_or_alerts_and_connects_for_alerts() {
        let net = MockNet::new();
        let (s, rx) = NotifyStream::channel(Arc::new(net.clone()), Arc::new(MemStorage::new()), "d".into(), opts());
        s.start(&[]);
        let ev = collect(&rx, |e| !e.is_empty());
        assert_eq!(ev, [StreamEvent::Status(Status::Off)]);
        net.on_stream(BASE, &["event: hello", r#"data: {"now":5,"addrs":[]}"#, ""]);
        s.set_has_alerts(true);
        let ev = collect(&rx, |e| e.iter().any(|x| matches!(x, StreamEvent::Hello(_))));
        assert_eq!(ev[0], StreamEvent::Status(Status::Connecting));
        s.stop();
        assert!(net.calls().iter().any(|c| c.url.starts_with(BASE)));
    }

    #[test]
    fn same_accounts_do_not_reconnect_and_new_ones_do() {
        let net = MockNet::new();
        let (s, rx) = NotifyStream::channel(Arc::new(net.clone()), Arc::new(MemStorage::new()), "d".into(), StreamOptions { retry: Duration::from_secs(30), ..opts() });
        s.start(&["0x1".into()]);
        collect(&rx, |e| e.contains(&StreamEvent::Status(Status::Retrying)));
        s.set_accounts(&["0X1 ".into()]);
        s.set_accounts(&["0x1".into(), "0x2".into()]);
        collect(&rx, |e| e.contains(&StreamEvent::Status(Status::Retrying)));
        s.stop();
        let urls: Vec<String> = net.calls().into_iter().map(|c| c.url).filter(|u| u.starts_with(BASE)).collect();
        assert_eq!(urls, [format!("{BASE}?addr=0x1&device=d"), format!("{BASE}?addr=0x1%2C0x2&device=d")]);
    }
}
