//! rare-webgame-host: one web game's window, in a process of its own, on Chromium (CEF). The native
//! launcher starts it with `--socket <path> [--data-dir <dir>]` and drives it over that Unix
//! socket, one JSON object per line (the protocol is rare-webgame's `process.rs`):
//!
//! - launcher to host: `open`, `eval`, `show`, `hide`, `focus`, `close`, and `navigation` answers
//! - host to launcher: `opened`, `url`, `navigation` questions, `closed`
//!
//! The same binary is CEF's renderer, GPU and utility process (`--type=...`); only the browser
//! process talks to the launcher. The window is a frameless CEF Views window at the screen's size,
//! tagged with Rare's gamescope app id (`STEAM_GAME`) so gamescope focuses it like the launcher's
//! own; hiding it hands the screen back. The page gets no IPC: the open's init script (controller
//! profile, Local API grant, webgame.js) runs in the main frame before the page's own scripts,
//! wallet requests are `rare-wallet:` navigations the launcher takes, and controller input arrives
//! as scripts the launcher evaluates. WebGPU is on (Vulkan), with GPU rasterisation and zero-copy.
//! `--rare-stream` (cloud gaming) trades WebGPU for VA-API video decode on GL; `--user-agent=<ua>`
//! is Chromium's own switch. A pop-up (a sign-in) in an app-mode window gets the same init script,
//! and launcher scripts (controller input) go to it while it is open.
//!
//! Env: `RARE_CEF_DIR` the CEF runtime's resources (default: next to this binary, `cef/`),
//! `RARE_WEBHOST_WINDOWED=WxH` a plain window, `RARE_WEBHOST_LOG=1` log every message,
//! `RARE_WEBHOST_NO_SANDBOX=1` without Chromium's sandbox (hosts without user namespaces),
//! `RARE_WEBHOST_FLAGS="--a --b=c"` extra Chromium switches.

use cef::*;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long a navigation waits for the launcher's answer before it goes ahead (process.rs NAV_WAIT).
const NAV_WAIT: Duration = Duration::from_millis(250);

/// Rare's gamescope app id (overlay/gamescope.rs RARE_APPID).
#[cfg(target_os = "linux")]
const RARE_APPID: u32 = 1380012613;

/// The init script's key in the browser's extra info (browser process to renderer).
const INIT_KEY: &str = "rareInit";

fn log_on() -> bool {
    std::env::var_os("RARE_WEBHOST_LOG").is_some()
}

fn log(msg: &str) {
    eprintln!("[webgame-host] {msg}");
}

fn trunc(s: &str) -> String {
    match s.char_indices().nth(200) {
        Some((i, _)) => format!("{}...", &s[..i]),
        None => s.to_string(),
    }
}

fn started() -> Instant {
    static T: OnceLock<Instant> = OnceLock::new();
    *T.get_or_init(Instant::now)
}

fn ms() -> u128 {
    started().elapsed().as_millis()
}

fn arg(name: &str) -> Option<String> {
    let mut args = std::env::args().skip(1);
    let flag = format!("--{name}");
    let eq = format!("--{name}=");
    while let Some(a) = args.next() {
        if a == flag {
            return args.next();
        }
        if let Some(v) = a.strip_prefix(&eq) {
            return Some(v.to_string());
        }
    }
    None
}

fn windowed() -> Option<(i32, i32)> {
    let v = std::env::var("RARE_WEBHOST_WINDOWED").ok()?;
    let (a, b) = v.split_once('x')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

fn s(v: &str) -> CefString {
    CefString::from(v)
}

// ---------------------------------------------------------------- the launcher's socket

static OUT: OnceLock<Mutex<UnixStream>> = OnceLock::new();
/// Navigations waiting for the launcher's answer.
static WAITING: OnceLock<Mutex<HashMap<u64, Sender<bool>>>> = OnceLock::new();
static SEQ: AtomicU64 = AtomicU64::new(0);
static CLOSED_SENT: AtomicBool = AtomicBool::new(false);

fn send(v: Value) {
    if log_on() {
        log(&format!("-> {}", trunc(&v.to_string())));
    }
    if let Some(out) = OUT.get() {
        // One write per line (formatting into the socket would send it in pieces).
        let line = format!("{v}\n");
        let _ = out.lock().unwrap().write_all(line.as_bytes());
    }
}

fn send_closed() {
    if !CLOSED_SENT.swap(true, Ordering::SeqCst) {
        send(json!({ "ev": "closed" }));
    }
}

fn waiting() -> &'static Mutex<HashMap<u64, Sender<bool>>> {
    WAITING.get_or_init(Default::default)
}

/// A navigation: `rare-wallet:` is the launcher's and never loads; web pages and `rare:` links wait
/// for its answer (it may open them in Rare instead); everything else goes ahead.
fn ask_navigation(url: &str) -> bool {
    let id = SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    if url.starts_with("rare-wallet:") {
        send(json!({ "ev": "navigation", "id": id, "url": url }));
        return false;
    }
    if !(url.starts_with("http:") || url.starts_with("https:") || url.starts_with("rare:")) {
        return true;
    }
    let (tx, rx) = channel();
    waiting().lock().unwrap().insert(id, tx);
    send(json!({ "ev": "navigation", "id": id, "url": url }));
    let allow = rx.recv_timeout(NAV_WAIT).unwrap_or(true);
    waiting().lock().unwrap().remove(&id);
    allow
}

// ---------------------------------------------------------------- the UI thread's state

struct Open {
    title: String,
    url: String,
    init_script: String,
    /// Rare Wallet is in the page (a web game): its pop-ups get no init script.
    wallet: bool,
}

#[derive(Default)]
struct Ui {
    ready: bool,
    /// An open that arrived before CEF was ready.
    pending: Option<Open>,
    window: Option<Window>,
    browser: Option<Browser>,
    /// A pop-up over the page (a sign-in window).
    popup: Option<Browser>,
    /// The open's init script, for pop-ups of app-mode windows (None for web games).
    popup_init: Option<String>,
    opened_at: Option<Instant>,
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui::default());
}

fn parse(msg: &Value) -> Option<Op> {
    let st = |k: &str| msg.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    Some(match msg.get("op")?.as_str()? {
        "open" => Op::Open(Open { title: st("title"), url: st("url"), init_script: st("initScript"), wallet: msg.get("wallet").and_then(Value::as_bool).unwrap_or(true) }),
        "eval" => Op::Eval(st("js")),
        "show" => Op::Show,
        "hide" => Op::Hide,
        "focus" => Op::Focus,
        "close" | "gone" => Op::Close,
        _ => return None,
    })
}

enum Op {
    Open(Open),
    Eval(String),
    Show,
    Hide,
    Focus,
    Close,
}

/// Runs a launcher op on the UI thread.
fn run_op(op: Op) {
    match op {
        Op::Open(o) => UI.with(|u| {
            let ready = u.borrow().ready;
            if ready {
                open(o);
            } else {
                u.borrow_mut().pending = Some(o);
            }
        }),
        Op::Eval(js) => UI.with(|u| {
            let u = u.borrow();
            // A sign-in pop-up over the page takes the controller while it is open.
            if let Some(f) = u.popup.as_ref().or(u.browser.as_ref()).and_then(|b| b.main_frame()) {
                f.execute_java_script(Some(&s(&js)), Some(&s("")), 0);
            }
        }),
        Op::Show => UI.with(|u| {
            let u = u.borrow();
            if let Some(w) = &u.window {
                w.show();
                w.request_focus();
            }
            if let Some(h) = u.browser.as_ref().and_then(|b| b.host()) {
                h.set_focus(1);
            }
        }),
        Op::Hide => UI.with(|u| {
            if let Some(w) = &u.borrow().window {
                w.hide();
            }
        }),
        Op::Focus => UI.with(|u| {
            let u = u.borrow();
            if let Some(w) = &u.window {
                w.request_focus();
            }
            if let Some(h) = u.browser.as_ref().and_then(|b| b.host()) {
                h.set_focus(1);
            }
        }),
        Op::Close => UI.with(|u| {
            let browser = u.borrow().browser.clone();
            match browser.and_then(|b| b.host()) {
                Some(h) => h.close_browser(1),
                None => quit_message_loop(),
            }
        }),
    }
}

wrap_task! {
    struct OpTask {
        op: Arc<Mutex<Option<Op>>>,
    }

    impl Task {
        fn execute(&self) {
            if let Some(op) = self.op.lock().unwrap().take() {
                run_op(op);
            }
        }
    }
}

fn post(op: Op) {
    let mut task = OpTask::new(Arc::new(Mutex::new(Some(op))));
    post_task(ThreadId::UI, Some(&mut task));
}

/// `STEAM_GAME` on our X11 window, the way the launcher's window is tagged (render platform.rs).
#[cfg(target_os = "linux")]
fn tag(xid: u64) -> String {
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
    use x11rb::wrapper::ConnectionExt as _;
    if xid == 0 {
        return "no X11 window".into();
    }
    let r = (|| -> Result<(), String> {
        let (conn, _) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let atom = conn.intern_atom(false, b"STEAM_GAME").map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?.atom;
        conn.change_property32(PropMode::REPLACE, xid as u32, atom, AtomEnum::CARDINAL, &[RARE_APPID]).map_err(|e| e.to_string())?;
        x11rb::connection::Connection::flush(&conn).map_err(|e| e.to_string())?;
        conn.sync().map_err(|e| e.to_string())
    })();
    match r {
        Ok(()) => format!("x11 window {xid:#x}: STEAM_GAME={RARE_APPID}"),
        Err(e) => format!("x11 window {xid:#x}: STEAM_GAME failed: {e}"),
    }
}

#[cfg(not(target_os = "linux"))]
fn tag(_xid: u64) -> String {
    "not linux".into()
}

// ---------------------------------------------------------------- the window and the browser

wrap_window_delegate! {
    struct GameWindow {
        browser_view: RefCell<Option<BrowserView>>,
        popup: bool,
    }

    impl ViewDelegate {}

    impl PanelDelegate {}

    impl WindowDelegate {
        fn on_window_created(&self, window: Option<&mut Window>) {
            let (Some(window), Some(view)) = (window, self.browser_view.borrow().clone()) else { return };
            let mut v = View::from(&view);
            window.add_child_view(Some(&mut v));
            if windowed().is_none() {
                window.set_fullscreen(1);
            }
            window.show();
            log(&tag(window.window_handle() as u64));
            if self.popup {
                // A sign-in window over the game (Google for zkLogin games); it closes itself.
                log("pop-up window open");
                return;
            }
            UI.with(|u| u.borrow_mut().window = Some(window.clone()));
            if let Some(t) = UI.with(|u| u.borrow().opened_at) {
                log(&format!("window up {} ms after the open", t.elapsed().as_millis()));
            }
            send(json!({ "ev": "opened" }));
        }

        fn on_window_destroyed(&self, _window: Option<&mut Window>) {
            *self.browser_view.borrow_mut() = None;
        }

        fn can_close(&self, _window: Option<&mut Window>) -> i32 {
            let view = self.browser_view.borrow();
            match view.as_ref().and_then(|v| v.browser()).and_then(|b| b.host()) {
                Some(h) => h.try_close_browser(),
                None => 1,
            }
        }

        fn initial_bounds(&self, _window: Option<&mut Window>) -> Rect {
            // No window manager under gamescope's Xwayland applies fullscreen: take the screen's size.
            if let Some((w, h)) = windowed() {
                return Rect { x: 0, y: 0, width: w, height: h };
            }
            display_get_primary().map(|d| d.bounds()).unwrap_or(Rect { x: 0, y: 0, width: 1280, height: 800 })
        }

        fn is_frameless(&self, _window: Option<&mut Window>) -> i32 {
            windowed().is_none() as i32
        }

        fn window_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::ALLOY
        }
    }
}

wrap_browser_view_delegate! {
    struct GameView {}

    impl ViewDelegate {}

    impl BrowserViewDelegate {
        fn browser_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::ALLOY
        }

        fn on_popup_browser_view_created(&self, _view: Option<&mut BrowserView>, popup: Option<&mut BrowserView>, _is_devtools: i32) -> i32 {
            let mut delegate = GameWindow::new(RefCell::new(popup.cloned()), true);
            window_create_top_level(Some(&mut delegate));
            1
        }
    }
}

wrap_client! {
    struct GameClient {}

    impl Client {
        fn request_handler(&self) -> Option<RequestHandler> {
            Some(GameRequests::new())
        }
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(GameLife::new())
        }
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(GameDisplay::new())
        }
        fn load_handler(&self) -> Option<LoadHandler> {
            Some(GameLoad::new())
        }
    }
}

wrap_request_handler! {
    struct GameRequests {}

    impl RequestHandler {
        fn on_before_browse(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _user_gesture: i32,
            _is_redirect: i32,
        ) -> i32 {
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            let main = frame.map(|f| f.is_main() != 0).unwrap_or(false);
            if !main {
                // Frames inside the game never reach the wallet; their own navigations go ahead.
                return url.starts_with("rare-wallet:") as i32;
            }
            // 1 cancels.
            (!ask_navigation(&url)) as i32
        }

        fn on_render_process_terminated(&self, _browser: Option<&mut Browser>, status: TerminationStatus, error_code: i32, error_string: Option<&CefString>) {
            log(&format!("the page's process ended ({status:?}, {error_code}): {}", error_string.map(|e| e.to_string()).unwrap_or_default()));
        }
    }
}

wrap_life_span_handler! {
    struct GameLife {}

    impl LifeSpanHandler {
        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: i32,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            user_gesture: i32,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut i32>,
        ) -> i32 {
            // A pop-up the player opened (a sign-in: zkLogin games open Google's, GeForce NOW
            // NVIDIA's) gets a window of its own over the game; anything else stays out.
            let url = target_url.map(|u| u.to_string()).unwrap_or_default();
            let ok = user_gesture != 0 && url.starts_with("https://");
            log(&format!("pop-up {}: {}", if ok { "opened" } else { "refused" }, trunc(&url)));
            // An app's pop-up gets the controller runtime too (focus mode off the app's hosts), so
            // the pad and the on-screen keyboard work on the sign-in page. Never a web game's.
            if ok {
                if let (Some(init), Some(x)) = (UI.with(|u| u.borrow().popup_init.clone()), extra_info) {
                    let mut d = dictionary_value_create();
                    if let Some(dv) = d.as_mut() {
                        dv.set_string(Some(&s(INIT_KEY)), Some(&s(&init)));
                    }
                    *x = d;
                }
            }
            (!ok) as i32
        }

        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let Some(b) = browser else { return };
            UI.with(|u| {
                let mut u = u.borrow_mut();
                if u.browser.is_none() {
                    u.browser = Some(b.clone());
                } else {
                    u.popup = Some(b.clone());
                }
            });
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            let id = browser.map(|b| b.identifier());
            let main = UI.with(|u| {
                let mut u = u.borrow_mut();
                let main = u.browser.as_ref().map(|b| b.identifier()) == id;
                if main {
                    u.browser = None;
                    u.window = None;
                }
                if u.popup.as_ref().map(|b| b.identifier()) == id {
                    u.popup = None;
                }
                main
            });
            // A pop-up closing leaves the game running.
            if main {
                quit_message_loop();
            }
        }
    }
}

wrap_display_handler! {
    struct GameDisplay {}

    impl DisplayHandler {
        fn on_address_change(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            if frame.map(|f| f.is_main() != 0).unwrap_or(false) {
                send(json!({ "ev": "url", "url": url.map(|u| u.to_string()).unwrap_or_default() }));
            }
        }
    }
}

wrap_load_handler! {
    struct GameLoad {}

    impl LoadHandler {
        fn on_load_error(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, error_code: Errorcode, error_text: Option<&CefString>, failed_url: Option<&CefString>) {
            let url = failed_url.map(|u| u.to_string()).unwrap_or_default();
            if frame.map(|f| f.is_main() != 0).unwrap_or(false) && !url.starts_with("rare-wallet:") {
                log(&format!("load failed ({error_code:?} {}): {}", error_text.map(|e| e.to_string()).unwrap_or_default(), trunc(&url)));
            }
        }

        fn on_load_end(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, status: i32) {
            if frame.map(|f| f.is_main() != 0).unwrap_or(false) {
                if let Some(t) = UI.with(|u| u.borrow().opened_at) {
                    log(&format!("page loaded ({status}) {} ms after the open", t.elapsed().as_millis()));
                }
            }
        }
    }
}

/// Opens the game: a frameless window with one browser view (UI thread, CEF ready).
fn open(o: Open) {
    UI.with(|u| {
        let mut u = u.borrow_mut();
        u.opened_at = Some(Instant::now());
        u.popup_init = (!o.wallet).then(|| o.init_script.clone());
    });
    let mut client = GameClient::new();
    let settings = BrowserSettings { background_color: 0xff00_0000, ..Default::default() };
    // The init script rides to the renderer with the browser (RenderApp::on_browser_created).
    let mut extra = dictionary_value_create();
    if let Some(d) = extra.as_mut() {
        d.set_string(Some(&s(INIT_KEY)), Some(&s(&o.init_script)));
    }
    let mut view_delegate = GameView::new();
    let view = browser_view_create(Some(&mut client), Some(&s(&o.url)), Some(&settings), extra.as_mut(), None, Some(&mut view_delegate));
    let mut window_delegate = GameWindow::new(RefCell::new(view), false);
    if let Some(w) = window_create_top_level(Some(&mut window_delegate)) {
        w.set_title(Some(&s(&o.title)));
    }
}

// ---------------------------------------------------------------- the app (every process)

// Init scripts by browser id (renderer processes).
thread_local! {
    static INIT: RefCell<HashMap<i32, String>> = RefCell::new(HashMap::new());
}

wrap_render_process_handler! {
    struct GameRenderer {}

    impl RenderProcessHandler {
        fn on_browser_created(&self, browser: Option<&mut Browser>, extra_info: Option<&mut DictionaryValue>) {
            let (Some(b), Some(x)) = (browser, extra_info) else { return };
            if x.has_key(Some(&s(INIT_KEY))) != 0 {
                let script = CefString::from(&x.string(Some(&s(INIT_KEY)))).to_string();
                INIT.with(|i| i.borrow_mut().insert(b.identifier(), script));
            }
        }

        fn on_browser_destroyed(&self, browser: Option<&mut Browser>) {
            if let Some(b) = browser {
                INIT.with(|i| i.borrow_mut().remove(&b.identifier()));
            }
        }

        fn on_context_created(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, context: Option<&mut V8Context>) {
            // Before the page's own scripts: the controller profile, the Local API grant, webgame.js.
            let (Some(b), Some(f), Some(c)) = (browser, frame, context) else { return };
            if f.is_main() == 0 {
                return;
            }
            let Some(script) = INIT.with(|i| i.borrow().get(&b.identifier()).cloned()) else { return };
            let mut ret = None;
            let mut exc = None;
            if c.eval(Some(&s(&script)), Some(&s("rare://webgame.js")), 1, Some(&mut ret), Some(&mut exc)) == 0 {
                let msg = exc.map(|e| CefString::from(&e.message()).to_string()).unwrap_or_default();
                eprintln!("[webgame-host] init script failed: {msg}");
            }
        }
    }
}

wrap_browser_process_handler! {
    struct GameBrowserProcess {}

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            log(&format!("CEF ready {} ms after start", ms()));
            let pending = UI.with(|u| {
                let mut u = u.borrow_mut();
                u.ready = true;
                u.pending.take()
            });
            if let Some(o) = pending {
                open(o);
            }
        }
    }
}

/// The host runs a cloud game stream (`--rare-stream`).
fn stream_mode() -> bool {
    std::env::args().any(|a| a == "--rare-stream")
}

/// VA-API video decode (and encode) on the 780M's VCN. Chromium's Linux decoder runs through GL, so
/// a stream's window uses ANGLE on GL; the cloud services send H.264 or H.264, H.265 and AV1
/// (GeForce NOW), and this CEF build has no software H.264, so the GPU decodes them.
const VAAPI_FEATURES: &str = "AcceleratedVideoDecodeLinuxGL,AcceleratedVideoDecodeLinuxZeroCopyGL,AcceleratedVideoEncoder,VaapiVideoDecoder,VaapiIgnoreDriverChecks";

/// Chromium's switches for a game: WebGPU on Vulkan, GPU rasterisation, zero-copy, X11 (gamescope's
/// Xwayland), sound without a click first, no keyring and no browser background traffic. A cloud
/// game stream: ANGLE on GL with VA-API decode instead of WebGPU on Vulkan.
fn game_switches() -> Vec<(String, Option<String>)> {
    let mut v: Vec<(String, Option<String>)> = vec![
        // Cookies are encrypted with a key from the desktop keyring (Secret Service over D-Bus)
        // otherwise: in a session without one every navigation waits for it forever.
        ("password-store".into(), Some("basic".into())),
        ("disable-background-networking".into(), None),
        ("disable-component-update".into(), None),
        ("disable-spell-checking".into(), None),
        ("no-first-run".into(), None),
        ("ozone-platform".into(), Some("x11".into())),
        ("enable-unsafe-webgpu".into(), None),
        ("enable-features".into(), Some("Vulkan,VulkanFromANGLE,DefaultANGLEVulkan,WebGPUService,UnsafeWebGPU".into())),
        ("use-angle".into(), Some("vulkan".into())),
        ("use-vulkan".into(), None),
        ("enable-gpu-rasterization".into(), None),
        ("enable-zero-copy".into(), None),
        ("ignore-gpu-blocklist".into(), None),
        ("autoplay-policy".into(), Some("no-user-gesture-required".into())),
        ("disable-background-timer-throttling".into(), None),
        ("disable-renderer-backgrounding".into(), None),
        ("hide-scrollbars".into(), None),
    ];
    if stream_mode() {
        v.retain(|(k, _)| !matches!(k.as_str(), "enable-unsafe-webgpu" | "enable-features" | "use-angle" | "use-vulkan"));
        v.push(("use-angle".into(), Some("gl".into())));
        v.push(("enable-features".into(), Some(VAAPI_FEATURES.into())));
    }
    if let Ok(extra) = std::env::var("RARE_WEBHOST_FLAGS") {
        for f in extra.split_whitespace() {
            let f = f.trim_start_matches("--");
            match f.split_once('=') {
                Some((k, val)) => {
                    v.retain(|(n, _)| n != k);
                    v.push((k.into(), Some(val.into())));
                }
                None => v.push((f.into(), None)),
            }
        }
    }
    v
}

wrap_app! {
    struct GameApp {}

    impl App {
        fn on_before_command_line_processing(&self, process_type: Option<&CefString>, command_line: Option<&mut CommandLine>) {
            let browser = process_type.map(|p| p.to_string().is_empty()).unwrap_or(true);
            let Some(cl) = command_line else { return };
            if browser {
                for (k, val) in game_switches() {
                    match val {
                        Some(val) => cl.append_switch_with_value(Some(&s(&k)), Some(&s(&val))),
                        None => cl.append_switch(Some(&s(&k))),
                    }
                }
            }
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(GameBrowserProcess::new())
        }

        fn render_process_handler(&self) -> Option<RenderProcessHandler> {
            Some(GameRenderer::new())
        }
    }
}

/// The CEF runtime's resources (icudtl.dat, *.pak, locales/, v8 snapshot).
fn cef_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("RARE_CEF_DIR") {
        return PathBuf::from(d);
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let here = exe.parent().map(|d| d.join("cef")).unwrap_or_default();
    if here.join("icudtl.dat").exists() {
        return here;
    }
    PathBuf::from("/usr/lib/rare/cef")
}

fn main() {
    let _ = started();
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let args = cef::args::Args::new();
    let mut app = GameApp::new();
    // CEF's subprocesses (renderer, GPU, utility) run here and exit.
    let code = execute_process(Some(args.as_main_args()), Some(&mut app), std::ptr::null_mut());
    if code >= 0 {
        std::process::exit(code);
    }

    let Some(sock) = arg("socket").or_else(|| std::env::var("RARE_WEBHOST_SOCKET").ok()) else {
        log("usage: rare-webgame-host --socket <path> [--data-dir <dir>]");
        std::process::exit(2);
    };
    let stream = match UnixStream::connect(&sock) {
        Ok(st) => st,
        Err(e) => {
            log(&format!("connect {sock}: {e}"));
            std::process::exit(2);
        }
    };
    let _ = OUT.set(Mutex::new(stream.try_clone().expect("socket")));

    // One storage profile per game (cookies, localStorage, IndexedDB).
    let data = arg("data-dir").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join(format!("rare-webgame-{}", std::process::id())));
    let _ = std::fs::create_dir_all(&data);
    let res = cef_dir();
    let exe = std::env::current_exe().unwrap_or_default();
    let settings = Settings {
        no_sandbox: std::env::var_os("RARE_WEBHOST_NO_SANDBOX").is_some() as i32,
        browser_subprocess_path: s(&exe.to_string_lossy()),
        root_cache_path: s(&data.to_string_lossy()),
        cache_path: s(&data.to_string_lossy()),
        persist_session_cookies: 1,
        resources_dir_path: s(&res.to_string_lossy()),
        locales_dir_path: s(&res.join("locales").to_string_lossy()),
        log_severity: match std::env::var("RARE_WEBHOST_LOG").as_deref() {
            Ok("2") => LogSeverity::VERBOSE,
            Ok(_) => LogSeverity::INFO,
            Err(_) => LogSeverity::ERROR,
        },
        background_color: 0xff00_0000,
        ..Default::default()
    };
    if initialize(Some(args.as_main_args()), Some(&settings), Some(&mut app), std::ptr::null_mut()) != 1 {
        log("CEF did not start");
        send_closed();
        std::process::exit(1);
    }

    // The launcher's lines: navigation answers go straight to the waiting handler (the UI thread
    // is blocked in it), everything else to the UI thread. The launcher going away closes the game.
    std::thread::Builder::new()
        .name("webhost-in".into())
        .spawn(move || {
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
                if log_on() {
                    log(&format!("<- {}", trunc(&line)));
                }
                if msg.get("op").and_then(Value::as_str) == Some("navigation") {
                    let id = msg.get("id").and_then(Value::as_u64).unwrap_or(0);
                    let allow = msg.get("allow").and_then(Value::as_bool).unwrap_or(true);
                    if let Some(tx) = waiting().lock().unwrap().remove(&id) {
                        let _ = tx.send(allow);
                    }
                    continue;
                }
                if let Some(op) = parse(&msg) {
                    post(op);
                }
            }
            post(Op::Close);
        })
        .expect("reader thread");

    run_message_loop();
    shutdown();
    send_closed();
}
