//! The out-of-process browser engine: a [`WebEngine`] that talks to a separate host process (the
//! CEF host, `rare-webgame-host`) over a Unix socket, one JSON object per line. The host owns the
//! window, an X11 window tagged with Rare's app id, so the focus keeper treats it like any other
//! game window; Rare's UI never loads web content itself.
//!
//! Launcher to host:
//! - `{"op":"open","label","title","url","initScript","dataDir","wallet"}`: opens (replacing any open window)
//! - `{"op":"eval","js"}`, `{"op":"show"}`, `{"op":"hide"}`, `{"op":"focus"}`, `{"op":"close"}`
//! - `{"op":"navigation","id","allow"}`: the answer to a navigation question
//!
//! Host to launcher:
//! - `{"ev":"opened"}`: the window is up
//! - `{"ev":"url","url"}`: the page's URL changed
//! - `{"ev":"navigation","id","url"}`: the page is about to navigate; the host waits for the answer
//!   (and lets it go ahead after [`NAV_WAIT`] with none)
//! - `{"ev":"closed"}`: the window is gone
//!
//! A host that exits counts as the window closing.
//!
//! [`Spawner`] starts a fresh host process for every window (`rare-webgame-host --socket <path>
//! --data-dir <dir> [--user-agent=<ua>] [--rare-stream]`, Chromium through CEF,
//! launcher/native/webhost) and drives it through a [`ProcessEngine`]; a closed window ends its
//! process.

use crate::engine::{EngineEvents, OpenSpec, WebEngine};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long the host holds a navigation for the launcher's answer.
pub const NAV_WAIT: Duration = Duration::from_millis(250);

#[derive(Default)]
struct State {
    open: bool,
    url: Option<String>,
    events: Option<Arc<dyn EngineEvents>>,
}

/// A web engine in another process, reached over `stream`.
pub struct ProcessEngine {
    out: Mutex<UnixStream>,
    state: Arc<Mutex<State>>,
}

impl ProcessEngine {
    /// Serves the host on the other end of `stream`. Reads its events on a thread of its own.
    pub fn new(stream: UnixStream) -> std::io::Result<ProcessEngine> {
        let read_half = stream.try_clone()?;
        let answer_half = stream.try_clone()?;
        let state: Arc<Mutex<State>> = Arc::default();
        let st = state.clone();
        std::thread::Builder::new().name("webgame-host".into()).spawn(move || {
            let mut answer = answer_half;
            for line in BufReader::new(read_half).lines() {
                let Ok(line) = line else { break };
                let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
                let events = st.lock().unwrap().events.clone();
                match msg["ev"].as_str() {
                    Some("opened") => st.lock().unwrap().open = true,
                    Some("url") => st.lock().unwrap().url = msg["url"].as_str().map(str::to_string),
                    Some("navigation") => {
                        let url = msg["url"].as_str().unwrap_or("");
                        let allow = events.map(|e| e.navigation(url)).unwrap_or(true);
                        let reply = format!("{}\n", json!({ "op": "navigation", "id": msg["id"], "allow": allow }));
                        if answer.write_all(reply.as_bytes()).is_err() {
                            break;
                        }
                    }
                    Some("closed") => {
                        let events = {
                            let mut s = st.lock().unwrap();
                            s.open = false;
                            s.url = None;
                            s.events.clone()
                        };
                        if let Some(e) = events {
                            e.closed();
                        }
                    }
                    _ => {}
                }
            }
            // The host went away: the window with it.
            let events = {
                let mut s = st.lock().unwrap();
                let was_open = s.open;
                s.open = false;
                s.url = None;
                was_open.then(|| s.events.clone()).flatten()
            };
            if let Some(e) = events {
                e.closed();
            }
        })?;
        Ok(ProcessEngine { out: Mutex::new(stream), state })
    }

    fn send(&self, msg: Value) -> Result<(), String> {
        // One write per line: formatting straight into the socket sends hundreds of tiny writes.
        let mut line = msg.to_string();
        line.push('\n');
        self.out.lock().unwrap().write_all(line.as_bytes()).map_err(|e| format!("The web game host is not running: {e}"))
    }
}

impl WebEngine for ProcessEngine {
    fn open(&self, spec: &OpenSpec, events: Arc<dyn EngineEvents>) -> Result<(), String> {
        self.state.lock().unwrap().events = Some(events);
        self.send(json!({
            "op": "open",
            "label": spec.label,
            "title": spec.title,
            "url": spec.url,
            "initScript": spec.init_script,
            "dataDir": spec.data_dir,
            "wallet": spec.wallet,
        }))?;
        // The window counts as open from the request: the launcher shows it and routes the pad to it
        // while the host is still building it (the same as a webview window that is being built).
        let mut s = self.state.lock().unwrap();
        s.open = true;
        s.url = Some(spec.url.clone());
        Ok(())
    }
    fn is_open(&self) -> bool {
        self.state.lock().unwrap().open
    }
    fn url(&self) -> Option<String> {
        self.state.lock().unwrap().url.clone()
    }
    fn eval(&self, js: &str) {
        let _ = self.send(json!({ "op": "eval", "js": js }));
    }
    fn show(&self) {
        let _ = self.send(json!({ "op": "show" }));
    }
    fn hide(&self) {
        let _ = self.send(json!({ "op": "hide" }));
    }
    fn focus(&self) {
        let _ = self.send(json!({ "op": "focus" }));
    }
    fn close(&self) {
        let _ = self.send(json!({ "op": "close" }));
        let mut s = self.state.lock().unwrap();
        s.open = false;
        s.url = None;
    }
}

/// How long a new host has to connect back.
const CONNECT_WAIT: Duration = Duration::from_secs(15);
/// How long a host has to exit after `close` before it is killed.
const EXIT_WAIT: Duration = Duration::from_secs(3);

/// A [`WebEngine`] that starts one host process per window (see the module docs).
pub struct Spawner {
    /// The host binary.
    pub bin: PathBuf,
    /// Extra environment for the host.
    pub env: Vec<(String, String)>,
    /// Where its socket goes (`$XDG_RUNTIME_DIR` normally).
    pub dir: PathBuf,
    cur: Mutex<Option<(Arc<ProcessEngine>, Arc<Mutex<Child>>)>>,
    seq: std::sync::atomic::AtomicU64,
}

impl Spawner {
    pub fn new(bin: PathBuf, env: Vec<(String, String)>, dir: PathBuf) -> Spawner {
        Spawner { bin, env, dir, cur: Mutex::new(None), seq: Default::default() }
    }

    fn engine(&self) -> Option<Arc<ProcessEngine>> {
        self.cur.lock().unwrap().as_ref().map(|c| c.0.clone())
    }

    /// Starts a host and waits for it to connect.
    fn start(&self, spec: &OpenSpec) -> Result<(ProcessEngine, Child), String> {
        let n = self.seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = self.dir.join(format!("rare-webgame-{}-{n}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).map_err(|e| format!("The web game window could not start: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let mut cmd = Command::new(&self.bin);
        cmd.arg("--socket").arg(&path);
        // The game's storage profile, known before the engine starts (CEF takes it at start).
        if let Some(d) = &spec.data_dir {
            cmd.arg("--data-dir").arg(d);
        }
        // Chromium reads its user agent from the command line; `--rare-stream` is the host's.
        if let Some(ua) = &spec.user_agent {
            cmd.arg(format!("--user-agent={ua}"));
        }
        if spec.stream {
            cmd.arg("--rare-stream");
        }
        let mut child = cmd
            .envs(self.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| format!("The web game window could not start ({}): {e}", self.bin.display()))?;
        let until = Instant::now() + CONNECT_WAIT;
        let stream = loop {
            match listener.accept() {
                Ok((s, _)) => break Ok(s),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => break Err(e.to_string()),
            }
            if let Ok(Some(status)) = child.try_wait() {
                break Err(format!("the host exited ({status})"));
            }
            if Instant::now() > until {
                let _ = child.kill();
                let _ = child.wait();
                break Err("the host did not answer".into());
            }
            std::thread::sleep(Duration::from_millis(15));
        };
        let _ = std::fs::remove_file(&path);
        let stream = stream.map_err(|e| format!("The web game window could not start: {e}"))?;
        stream.set_nonblocking(false).map_err(|e| e.to_string())?;
        Ok((ProcessEngine::new(stream).map_err(|e| e.to_string())?, child))
    }

    /// Ends the current host: `close`, then a kill if it is still there after [`EXIT_WAIT`].
    fn stop(&self) {
        let Some((engine, child)) = self.cur.lock().unwrap().take() else { return };
        engine.close();
        std::thread::spawn(move || {
            let until = Instant::now() + EXIT_WAIT;
            loop {
                let mut c = child.lock().unwrap();
                if !matches!(c.try_wait(), Ok(None)) {
                    return;
                }
                if Instant::now() > until {
                    let _ = c.kill();
                    let _ = c.wait();
                    return;
                }
                drop(c);
                std::thread::sleep(Duration::from_millis(50));
            }
        });
    }
}

impl WebEngine for Spawner {
    fn open(&self, spec: &OpenSpec, events: Arc<dyn EngineEvents>) -> Result<(), String> {
        self.stop();
        let (engine, child) = self.start(spec)?;
        let engine = Arc::new(engine);
        let child = Arc::new(Mutex::new(child));
        engine.open(spec, events)?;
        // The host is reaped when it exits, whoever ended it (the window closed, `stop`, a crash).
        {
            let child = child.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_millis(500));
                if !matches!(child.lock().unwrap().try_wait(), Ok(None)) {
                    return;
                }
            });
        }
        *self.cur.lock().unwrap() = Some((engine, child));
        Ok(())
    }
    fn is_open(&self) -> bool {
        self.engine().is_some_and(|e| e.is_open())
    }
    fn url(&self) -> Option<String> {
        self.engine()?.url()
    }
    fn eval(&self, js: &str) {
        if let Some(e) = self.engine() {
            e.eval(js);
        }
    }
    fn show(&self) {
        if let Some(e) = self.engine() {
            e.show();
        }
    }
    fn hide(&self) {
        if let Some(e) = self.engine() {
            e.hide();
        }
    }
    fn focus(&self) {
        if let Some(e) = self.engine() {
            e.focus();
        }
    }
    fn close(&self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Ev {
        closed: AtomicBool,
        blocked: Mutex<Vec<String>>,
    }
    impl EngineEvents for Ev {
        fn navigation(&self, url: &str) -> bool {
            let ok = !url.starts_with("rare-wallet:");
            if !ok {
                self.blocked.lock().unwrap().push(url.to_string());
            }
            ok
        }
        fn closed(&self) {
            self.closed.store(true, Ordering::SeqCst);
        }
    }

    fn spec() -> OpenSpec {
        OpenSpec { label: "webgame", title: "G".into(), url: "https://g.example/".into(), init_script: "window.x=1".into(), data_dir: None, wallet: false, user_agent: None, stream: false }
    }

    #[test]
    fn talks_to_a_host_over_the_socket() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let engine = ProcessEngine::new(ours).unwrap();
        let ev = Arc::new(Ev { closed: AtomicBool::new(false), blocked: Mutex::new(vec![]) });
        let host_in = BufReader::new(theirs.try_clone().unwrap());
        let mut host_out = theirs;
        let mut lines = host_in.lines();

        engine.open(&spec(), ev.clone()).unwrap();
        assert!(engine.is_open());
        let open: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        assert_eq!((open["op"].as_str(), open["url"].as_str(), open["initScript"].as_str()), (Some("open"), Some("https://g.example/"), Some("window.x=1")));
        assert_eq!(open["wallet"], json!(false), "app mode reaches the host");

        engine.eval("a()");
        engine.hide();
        let eval: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        assert_eq!(eval, json!({ "op": "eval", "js": "a()" }));
        assert_eq!(serde_json::from_str::<Value>(&lines.next().unwrap().unwrap()).unwrap()["op"], "hide");

        // The host asks about navigations and gets the launcher's answers.
        writeln!(host_out, "{}", json!({ "ev": "navigation", "id": 1, "url": "https://g.example/next" })).unwrap();
        writeln!(host_out, "{}", json!({ "ev": "navigation", "id": 2, "url": "rare-wallet://request/e30" })).unwrap();
        let a1: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        let a2: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        assert_eq!((a1["id"].clone(), a1["allow"].clone()), (json!(1), json!(true)));
        assert_eq!((a2["id"].clone(), a2["allow"].clone()), (json!(2), json!(false)));
        assert_eq!(ev.blocked.lock().unwrap().len(), 1);

        // The page's URL follows the host.
        writeln!(host_out, "{}", json!({ "ev": "url", "url": "https://g.example/next" })).unwrap();
        for _ in 0..100 {
            if engine.url().as_deref() == Some("https://g.example/next") {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(engine.url().as_deref(), Some("https://g.example/next"));

        // The host exiting closes the window.
        drop(host_out);
        drop(lines);
        for _ in 0..100 {
            if ev.closed.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ev.closed.load(Ordering::SeqCst) && !engine.is_open());
    }
}
