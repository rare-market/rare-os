//! Ask Rare in the launcher: the conversation the Rare menu's Ask tab shows, the owner's model
//! (Settings > Ask Rare, the API key sealed in Rare Wallet's vault), push to talk, and the bridge
//! that runs the model's tool calls on the UI thread through the launcher's own flows (`run`).
//! Network apps reach the same tools over MCP (rare-localapi's /v1/mcp asks `api_tool`).
//!
//! The model never signs: buying, selling, listing, sending and swapping only open Rare Wallet's
//! review sheet, which the player confirms on the device.

pub mod run;

use crate::state::State;
use rare_agent::agent::{self, Event, Exec, Outcome};
use rare_agent::provider::{self, Kind};
use rare_agent::{voice, Call, Msg, Part};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- settings

/// Settings > Ask Rare, from launcher.json (`agent*` fields). The key is not here: it is sealed in
/// Rare Wallet's vault (`agent:<provider>`); `agentKeys` only lists which providers have one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Setup {
    pub kind: Option<Kind>,
    pub model: Option<String>,
    pub url: Option<String>,
    /// Speak to Rare (push to talk; downloads the speech model on first use).
    pub voice: bool,
    /// A picture of the game goes with questions asked over a game.
    pub shots: bool,
    /// Holding RC talks to Rare.
    pub hold: bool,
    pub keys: Vec<String>,
}

pub fn setup() -> Setup {
    use crate::features::config::{get, get_bool, get_str};
    if std::env::var("RR_ASK_MOCK").is_ok() {
        // Shots: a set-up Ask Rare with the scripted model.
        return Setup { kind: Some(Kind::Claude), voice: true, hold: true, shots: std::env::var("RR_ASK_SHOT").is_ok(), keys: vec!["claude".into()], ..Default::default() };
    }
    Setup {
        kind: get_str("agentProvider").as_deref().and_then(Kind::from_id),
        model: get_str("agentModel").filter(|m| !m.trim().is_empty()),
        url: get_str("agentUrl").filter(|m| !m.trim().is_empty()),
        voice: get_bool("agentVoice"),
        shots: get_bool("agentShots"),
        hold: get_bool("agentHold"),
        keys: get("agentKeys").and_then(|v| v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())).unwrap_or_default(),
    }
}

fn key_name(k: Kind) -> String {
    format!("agent:{}", k.id())
}

/// The key in memory once read from the vault (provider id, key). Never logged.
static KEY: Mutex<Option<(String, String)>> = Mutex::new(None);

pub enum KeyState {
    NotNeeded,
    Ready(String),
    Missing,
    Locked,
}

fn key_state(k: Kind) -> KeyState {
    if !k.takes_key() {
        return KeyState::NotNeeded;
    }
    if let Some((p, key)) = KEY.lock().unwrap().as_ref() {
        if p == k.id() {
            return KeyState::Ready(key.clone());
        }
    }
    let Some(w) = crate::screens::wallet::service::wallet() else { return KeyState::Missing };
    match w.keystore.sealed(&key_name(k)) {
        Ok(Some(v)) => {
            *KEY.lock().unwrap() = Some((k.id().to_string(), v.to_string()));
            KeyState::Ready(v.to_string())
        }
        Ok(None) => KeyState::Missing,
        Err(_) => KeyState::Locked,
    }
}

/// Saves (Some) or removes (None) the key for `k`: asks for the PIN when the vault is locked, then
/// seals it. Runs on its own thread; the answer is a toast.
pub fn save_key(k: Kind, value: Option<String>) {
    std::thread::Builder::new()
        .name("ask-key".into())
        .spawn(move || {
            let Some(w) = crate::screens::wallet::service::wallet() else {
                crate::features::bus::note("Rare Wallet isn't available.", true);
                return;
            };
            if !w.keystore.has_vault().unwrap_or(false) {
                crate::features::bus::note("Add an account with a PIN in Rare Wallet first. Your key is sealed with it.", true);
                return;
            }
            let ok = crate::screens::wallet::service::ask_unlock("Enter your PIN to save your API key in Rare Wallet").recv().unwrap_or(false);
            if !ok {
                return;
            }
            let v = value.as_deref().map(str::trim).filter(|v| !v.is_empty());
            match w.keystore.set_sealed(&key_name(k), v) {
                Ok(()) => {
                    *KEY.lock().unwrap() = v.map(|v| (k.id().to_string(), v.to_string()));
                    let mut keys = setup().keys;
                    keys.retain(|x| x != k.id());
                    if v.is_some() {
                        keys.push(k.id().to_string());
                    }
                    crate::features::config::set(&[("agentKeys", serde_json::json!(keys))]);
                    crate::features::bus::note(if v.is_some() { "Key saved in Rare Wallet" } else { "Key removed" }, false);
                    crate::screens::settings::live::touch();
                }
                Err(e) => crate::features::bus::note(format!("Couldn't save the key: {e}"), true),
            }
        })
        .ok();
}

/// The model config for a call, after the key (asking for the PIN when the vault is locked).
fn config(s: &Setup) -> Result<provider::Config, String> {
    let kind = s.kind.ok_or("Set up Ask Rare in Settings > Ask Rare first.")?;
    let key = match key_state(kind) {
        KeyState::NotNeeded | KeyState::Missing => None,
        KeyState::Ready(k) => Some(k),
        KeyState::Locked => {
            let ok = crate::screens::wallet::service::ask_unlock("Enter your PIN to use your API key").recv_timeout(Duration::from_secs(180)).unwrap_or(false);
            if !ok {
                return Err("Unlock Rare Wallet to use your API key.".into());
            }
            match key_state(kind) {
                KeyState::Ready(k) => Some(k),
                _ => None,
            }
        }
    };
    let c = provider::Config::new(kind, s.model.as_deref(), s.url.as_deref(), key);
    if let Some(m) = c.missing() {
        return Err(m.into());
    }
    Ok(c)
}

/// The model to ask: the owner's, or the scripted one for shots (`RR_ASK_MOCK`).
fn model_for(s: &Setup, history: &[Msg]) -> Result<Box<dyn rare_agent::Model>, String> {
    if std::env::var("RR_ASK_MOCK").is_ok() {
        let q = history.iter().rev().find_map(|m| if let Msg::User(p) = m { p.iter().find_map(|x| if let Part::Text(t) = x { Some(t.to_lowercase()) } else { None }) } else { None }).unwrap_or_default();
        return Ok(Box::new(mock_for(&q)));
    }
    Ok(config(s)?.model())
}

/// What the scripted model does for a question (fixtures and shots).
pub fn mock_for(q: &str) -> rare_agent::mock::Mock {
    use rare_agent::mock::{Mock, Step};
    use serde_json::json;
    if q.contains("send") {
        return Mock::new(vec![Step::calls("", vec![("wallet_send", json!({ "to": "sam.sui", "amount": "2.5" }))]), Step::text("The review sheet is open: 2.5 SUI to sam.sui. Check it and confirm on the device.")]);
    }
    if q.contains("boss") {
        return Mock::new(vec![Step::text("That is the Bone Hydra. Stay at the edge of the arena, break the green heads first, and dash through the slam when the floor glows. Use your cast on the middle head while it recovers.")]);
    }
    Mock::new(vec![Step::calls("", vec![("search_games", json!({ "query": "hades" }))]), Step::calls("", vec![("launch_game", json!({ "id": "steam:1145360" }))]), Step::text("Hades is starting. Have fun.")])
}

/// Settings' Test button: a one-word round trip, as a toast.
pub fn test() {
    std::thread::Builder::new()
        .name("ask-test".into())
        .spawn(|| {
            let r = config(&setup()).and_then(|c| provider::test(&c).map(|w| (c.kind.name(), c.model.clone(), w)));
            match r {
                Ok((p, m, w)) => crate::features::bus::note(format!("{p} answered with {m}: \"{w}\""), false),
                Err(e) => crate::features::bus::note(e, true),
            }
        })
        .ok();
}

// ---------------------------------------------------------------- the conversation

#[derive(Clone, Debug, PartialEq)]
pub enum StepState {
    Running,
    Done(String),
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    You { text: String, shot: bool },
    Answer(String),
    Step { id: String, label: String, state: StepState },
    Note { text: String, error: bool },
}

/// What the Ask tab draws.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub lines: Vec<Line>,
    pub busy: bool,
    pub listening: bool,
    /// Speech is being turned into text.
    pub hearing: bool,
    /// The speech model is downloading (0..1).
    pub download: Option<f32>,
}

#[derive(Default)]
struct Session {
    view: View,
    history: Vec<Msg>,
    cancel: Option<Arc<AtomicBool>>,
    rec: Option<(voice::Recorder, Source)>,
    /// The next words start a new answer block (after tools ran).
    fresh: bool,
}

/// Where push to talk came from (which release ends it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Menu,
    Rc,
}

static S: Mutex<Option<Session>> = Mutex::new(None);
static VER: AtomicU64 = AtomicU64::new(1);
/// The Rare menu's Ask tab is on screen (answers then are not toasted).
static SHOWN: AtomicBool = AtomicBool::new(false);

fn with<R>(f: impl FnOnce(&mut Session) -> R) -> R {
    let mut g = S.lock().unwrap();
    let r = f(g.get_or_insert_with(Session::default));
    VER.fetch_add(1, Ordering::SeqCst);
    crate::features::bus::wake();
    r
}

pub fn version() -> u64 {
    VER.load(Ordering::SeqCst)
}

pub fn view() -> View {
    S.lock().unwrap().as_ref().map(|s| s.view.clone()).unwrap_or_default()
}

pub fn set_shown(on: bool) {
    SHOWN.store(on, Ordering::SeqCst);
}

/// The fixtures' conversation (screenshots).
pub fn set_view(v: View) {
    with(|s| s.view = v);
}

/// What the app tells the model about now: the game in front and the account.
#[derive(Clone, Debug, Default)]
pub struct Context {
    pub game: Option<String>,
    pub account: Option<String>,
    /// The still of the game under the Rare menu (the in-game helper's picture).
    pub frame: Option<PathBuf>,
}

static CTX: Mutex<Option<Context>> = Mutex::new(None);

pub fn set_context(c: Context) {
    *CTX.lock().unwrap() = Some(c);
}

fn context() -> Context {
    CTX.lock().unwrap().clone().unwrap_or_default()
}

/// The Ask tab's picture switch can be used: over a game with a still.
pub fn can_shoot() -> bool {
    context().frame.is_some()
}

fn system_prompt(c: &Context) -> String {
    rare_agent::agent::system_prompt(c.game.as_deref(), c.account.as_deref(), &chrono_like_now())
}

/// "2026-10-27 21:40 UTC" without a date crate.
fn chrono_like_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, (rem % 3600) / 60)
}

/// A picture of the game for the model: the still, at most 1280 x 800, as JPEG.
fn picture(p: &std::path::Path) -> Option<Part> {
    use base64::Engine;
    let img = image::ImageReader::open(p).ok()?.with_guessed_format().ok()?.decode().ok()?;
    let img = if img.width() > 1280 || img.height() > 800 { img.resize(1280, 800, image::imageops::FilterType::Triangle) } else { img };
    let mut buf = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 80).encode_image(&img.to_rgb8()).ok()?;
    Some(Part::Image { media: "image/jpeg".into(), data: base64::engine::general_purpose::STANDARD.encode(buf) })
}

/// Asks the model `text`. `shot`: send a picture of the game with it (the switch, over a game).
pub fn ask(text: &str, shot: bool) {
    let text = text.trim().chars().take(2000).collect::<String>();
    if text.is_empty() {
        return;
    }
    let ctx = context();
    let shot_path = if shot { ctx.frame.clone() } else { None };
    let cancel = Arc::new(AtomicBool::new(false));
    let started = with(|s| {
        if s.view.busy {
            return false;
        }
        s.view.busy = true;
        s.view.lines.push(Line::You { text: text.clone(), shot: shot_path.is_some() });
        s.cancel = Some(cancel.clone());
        true
    });
    if !started {
        return;
    }
    std::thread::Builder::new()
        .name("ask-rare".into())
        .spawn(move || {
            let mut history = with(|s| std::mem::take(&mut s.history));
            let mut parts = vec![Part::Text(text)];
            if let Some(p) = shot_path.as_deref().and_then(picture) {
                parts.push(p);
            }
            history.push(Msg::User(parts));
            let r = model_for(&setup(), &history).and_then(|model| {
                let tools = rare_agent::tools::catalog();
                let sys = system_prompt(&ctx);
                agent::run(model.as_ref(), &sys, &mut history, &tools, &Bridge, &mut |e| emit(e), &cancel)
            });
            let shown = SHOWN.load(Ordering::SeqCst);
            let answer = with(|s| {
                s.history = history;
                s.view.busy = false;
                s.cancel = None;
                // Steps still marked running did not finish.
                for l in s.view.lines.iter_mut() {
                    if let Line::Step { state, .. } = l {
                        if *state == StepState::Running {
                            *state = StepState::Failed("Stopped".into());
                        }
                    }
                }
                if let Err(e) = &r {
                    let cancelled = e == rare_agent::sse::CANCELLED;
                    s.view.lines.push(Line::Note { text: e.clone(), error: !cancelled });
                }
                s.view.lines.iter().rev().find_map(|l| if let Line::Answer(a) = l { Some(a.clone()) } else { None })
            });
            // Asked from RC with the menu closed: the answer as a toast.
            if !shown {
                match (&r, answer) {
                    (Ok(()), Some(a)) => crate::features::bus::note(first_sentence(&a), false),
                    (Err(e), _) if e != rare_agent::sse::CANCELLED => crate::features::bus::note(e.clone(), true),
                    _ => {}
                }
            }
        })
        .ok();
}

fn first_sentence(s: &str) -> String {
    let t = s.trim();
    let end = t.find(". ").map(|i| i + 1).unwrap_or(t.len());
    let mut out: String = t[..end].chars().take(140).collect();
    if out.len() < t[..end].len() {
        out.push_str("...");
    }
    out
}

fn emit(e: Event) {
    with(|s| {
        let lines = &mut s.view.lines;
        match e {
            Event::Text(t) => match lines.last_mut() {
                Some(Line::Answer(a)) if !s.fresh => a.push_str(&t),
                _ => {
                    let t = t.trim_start().to_string();
                    if !t.is_empty() {
                        lines.push(Line::Answer(t));
                        s.fresh = false;
                    }
                }
            },
            Event::Turn => s.fresh = true,
            Event::Redo => {
                if matches!(lines.last(), Some(Line::Answer(_))) {
                    lines.pop();
                }
            }
            Event::ToolStart { id, label, .. } => lines.push(Line::Step { id, label, state: StepState::Running }),
            Event::ToolDone { id, ok, summary } => {
                if let Some(Line::Step { state, .. }) = lines.iter_mut().rev().find(|l| matches!(l, Line::Step { id: i, .. } if *i == id)) {
                    *state = if ok { StepState::Done(summary) } else { StepState::Failed(summary) };
                }
            }
        }
        // Long conversations keep the newest 60 lines.
        if lines.len() > 60 {
            let n = lines.len() - 60;
            lines.drain(..n);
        }
    });
}

/// Stop: the model call and any tool still waiting end.
pub fn stop() {
    with(|s| {
        if let Some(c) = &s.cancel {
            c.store(true, Ordering::SeqCst);
        }
    });
}

/// New chat: the model forgets the conversation.
pub fn clear() {
    with(|s| {
        if s.view.busy {
            return;
        }
        s.view.lines.clear();
        s.history.clear();
    });
}

// ---------------------------------------------------------------- tools on the UI thread

type Job = (Call, Sender<Outcome>);
static JOBS: Mutex<Vec<Job>> = Mutex::new(Vec::new());

/// The model's tool calls go to the UI thread (`poll`) and wait for their answer there.
struct Bridge;

impl Exec for Bridge {
    fn run(&self, call: &Call) -> Outcome {
        let (tx, rx) = channel();
        JOBS.lock().unwrap().push((call.clone(), tx));
        crate::features::bus::wake();
        rx.recv_timeout(Duration::from_secs(90)).unwrap_or_else(|_| Outcome::err("Rare didn't answer in time."))
    }
}

/// What the app should do around a tool: close the Rare menu so a sheet shows, or leave the game
/// for a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiReq {
    CloseMenu,
    Leave,
    /// Start a game: over a game, through the Switcher's own leave-and-launch.
    Launch(String),
    /// Open the Rare menu on Ask (RC held).
    OpenAsk,
}

static UI: Mutex<Vec<UiReq>> = Mutex::new(Vec::new());

pub fn request_ui(r: UiReq) {
    let mut q = UI.lock().unwrap();
    if !q.contains(&r) {
        q.push(r);
    }
    drop(q);
    crate::features::bus::wake();
}

pub fn take_ui() -> Vec<UiReq> {
    std::mem::take(&mut *UI.lock().unwrap())
}

/// The next Rare menu opens on Ask (set when RC is held, or a sheet the agent opened closes).
static OPEN_ON_ASK: AtomicBool = AtomicBool::new(false);

pub fn open_on_ask(on: bool) {
    OPEN_ON_ASK.store(on, Ordering::SeqCst);
}

pub fn take_open_on_ask() -> bool {
    OPEN_ON_ASK.swap(false, Ordering::SeqCst)
}

/// Every update: the tools waiting, the recording's limit.
pub fn poll(st: &mut State, fx: &mut crate::page::Fx, _now: f64) {
    let jobs = std::mem::take(&mut *JOBS.lock().unwrap());
    for (call, tx) in jobs {
        run::run(st, fx, &call, Box::new(move |o| {
            let _ = tx.send(o);
        }));
    }
    let long = S.lock().unwrap().as_ref().and_then(|s| s.rec.as_ref().map(|(r, _)| r.elapsed() > voice::MAX_RECORD)).unwrap_or(false);
    if long {
        stop_listen();
    }
}

/// A Local API network app's tool call (MCP): runs it like Ask Rare does, answers request `id`.
pub fn api_tool(st: &mut State, fx: &mut crate::page::Fx, id: u64, params: &serde_json::Value) {
    let call = Call { id: format!("mcp-{id}"), name: params["name"].as_str().unwrap_or("").to_string(), args: params["args"].clone() };
    run::run(
        st,
        fx,
        &call,
        Box::new(move |o| {
            if o.ok {
                rare_localapi::localapi_reply(id, true, o.data);
            } else {
                rare_localapi::localapi_reply(id, false, serde_json::json!({ "code": "failed", "message": o.summary }));
            }
        }),
    );
}

// ---------------------------------------------------------------- push to talk

fn speech_dir() -> PathBuf {
    rare_core::paths::data_dir().join("speech")
}

fn rec_dir() -> PathBuf {
    rare_core::paths::cache_dir().join("ask")
}

static DOWNLOADING: AtomicBool = AtomicBool::new(false);

/// Starts listening (a hold of X on the Ask tab, or RC). The first time, the speech model downloads.
pub fn listen(from: Source) {
    let s = setup();
    if !s.voice {
        note("Turn on Speak to Rare in Settings > Ask Rare to talk.", false);
        return;
    }
    if view().busy || S.lock().unwrap().as_ref().is_some_and(|s| s.rec.is_some()) {
        return;
    }
    if !voice::have_model(&speech_dir()) {
        download_model();
        return;
    }
    match voice::Recorder::start(&rec_dir()) {
        Ok(r) => with(|s| {
            s.rec = Some((r, from));
            s.view.listening = true;
        }),
        Err(e) => note(&e, true),
    }
}

/// Downloads the speech model once (190 MB), with progress on the Ask tab.
pub fn download_model() {
    if DOWNLOADING.swap(true, Ordering::SeqCst) {
        return;
    }
    with(|s| s.view.download = Some(0.0));
    std::thread::Builder::new()
        .name("ask-speech-model".into())
        .spawn(|| {
            let last = Mutex::new(Instant::now());
            let r = voice::download(&speech_dir(), &|done, total| {
                let mut l = last.lock().unwrap();
                if l.elapsed() > Duration::from_millis(250) {
                    *l = Instant::now();
                    with(|s| s.view.download = Some(done as f32 / total.max(1) as f32));
                }
            });
            DOWNLOADING.store(false, Ordering::SeqCst);
            with(|s| s.view.download = None);
            match r {
                Ok(_) => note("Speech is ready. Hold X to talk.", false),
                Err(e) => note(&e, true),
            }
        })
        .ok();
}

pub fn listening() -> bool {
    S.lock().unwrap().as_ref().is_some_and(|s| s.rec.is_some())
}

/// Stops listening and asks what was said.
pub fn stop_listen() {
    let Some((rec, _)) = with(|s| {
        s.view.listening = false;
        s.rec.take()
    }) else {
        return;
    };
    let held = rec.elapsed();
    with(|s| s.view.hearing = true);
    std::thread::Builder::new()
        .name("ask-hear".into())
        .spawn(move || {
            let r = rec.stop().and_then(|pcm| voice::transcribe(&voice::model_path(&speech_dir()), &pcm));
            with(|s| s.view.hearing = false);
            match r {
                Ok(t) if !t.trim().is_empty() => ask(&t, setup().shots && can_shoot()),
                Ok(_) if held < Duration::from_millis(400) => {}
                Ok(_) => note("I didn't catch that. Hold X and speak.", false),
                Err(e) => note(&e, true),
            }
        })
        .ok();
}

/// A button went up: the end of a hold that started listening.
pub fn released(pad: rare_render::input::Pad) {
    let from = S.lock().unwrap().as_ref().and_then(|s| s.rec.as_ref().map(|(r, f)| (*f, r.elapsed())));
    if let Some((Source::Menu, held)) = from {
        // A quick tap keeps listening (tap X again to send); a hold sends on release.
        if pad == rare_render::input::Pad::X && held > Duration::from_millis(350) {
            stop_listen();
        }
    }
}

/// RC went up.
pub fn rc_released() {
    if S.lock().unwrap().as_ref().is_some_and(|s| matches!(s.rec, Some((_, Source::Rc)))) {
        stop_listen();
    }
}

/// RC held: Ask Rare listens, the menu opens on Ask. False when Ask Rare isn't set up for it.
pub fn rc_hold() -> bool {
    let s = setup();
    if !(s.hold && s.voice && s.kind.is_some()) {
        return false;
    }
    open_on_ask(true);
    request_ui(UiReq::OpenAsk);
    listen(Source::Rc);
    true
}

fn note(text: &str, error: bool) {
    with(|s| s.view.lines.push(Line::Note { text: text.to_string(), error }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_names_the_rules_and_the_game() {
        let p = system_prompt(&Context { game: Some("Hades".into()), account: Some("ghost".into()), frame: None });
        assert!(p.contains("In front: Hades") && p.contains("account: ghost"));
        assert!(p.contains("only open a review sheet"));
        let now = chrono_like_now();
        assert!(now.starts_with("20") && now.ends_with("UTC") && now.len() == 20, "{now}");
    }

    #[test]
    fn first_sentence_for_the_toast() {
        assert_eq!(first_sentence("Hades is starting. Have fun."), "Hades is starting.");
        assert_eq!(first_sentence("Done"), "Done");
    }

    #[test]
    fn streamed_text_joins_one_answer_and_steps_finish() {
        with(|s| *s = Session::default());
        emit(Event::Text("Starting ".into()));
        emit(Event::Text("Hades.".into()));
        emit(Event::ToolStart { id: "a".into(), name: "launch_game".into(), label: "Starting steam:1".into() });
        emit(Event::ToolDone { id: "a".into(), ok: true, summary: "Started Hades".into() });
        emit(Event::Text(" Done.".into()));
        let v = view();
        assert_eq!(v.lines[0], Line::Answer("Starting Hades.".into()));
        assert_eq!(v.lines[1], Line::Step { id: "a".into(), label: "Starting steam:1".into(), state: StepState::Done("Started Hades".into()) });
        assert_eq!(v.lines[2], Line::Answer("Done.".into()));
    }
}
