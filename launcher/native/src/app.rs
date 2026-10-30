//! The app: state, the shell's views, the screen on show, the overlay over it, the Options sheet
//! and the keyboard, the engine's focus `Nav` across all of them, and the input stack between them
//! (App.tsx's pad handler over nav/input.ts dispatch). main.rs owns the window (or the headless
//! GPU) and the frame loop and calls `input`, `update`, `frame` and `paint`.
//!
//! Each screen is a `Page` mounted in its own `View` under its own host: the page under the top
//! bar, an overlay in the page or sheet layer, Options in the sheet layer, the keyboard in the
//! modal layer. Views rebuild only when their stamp moves; after a layout every view registers with
//! Nav, Nav settles focus, and every view draws it (`View::set_focus`).

use crate::act::Act;
use crate::data::backend::Backend;
use crate::kit::controls::{LOAD_SX, LOAD_X};
use crate::kit::{s, tok, Arts, Ui};
use crate::page::{hash, Acts, Btn, Done, Feedback, Fx, Page};
use crate::screens::shell::boot::{Boot, Phase as BootPhase};
use crate::screens::shell::options::{game_options, Options, SCOPE as SCOPE_OPTIONS};
use crate::screens::shell::placeholder::Placeholder;
use crate::screens::shell::{self, clock_text};
use crate::state::{cancellable, Overlay, OskFor, OskReq, Phase, Start, State, Tab, Tone, TABS};
use rare_render::anim::Keys;
use rare_render::focus::hints::{compose, hint, Hint, Key, Stack};
use rare_render::focus::{Dir, Nav, Spot};
use rare_render::view::{key, El, View};
use rare_render::{clock, layer, DrawList, Gpu, Id, ImageLoader, Node, Scene, TextSys};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

pub const SCOPE_PAGE: u32 = 0;
pub const SCOPE_OVERLAY: u32 = 1;
/// The keyboard is above everything (the review sheet asks for a passphrase on it).
pub const SCOPE_OSK: u32 = 30;
/// The wallet's review sheet: over every page and sheet, under the keyboard.
pub const SCOPE_REVIEW: u32 = crate::screens::wallet::sign::host::SCOPE;

struct Slot {
    what: String,
    view: View,
    page: Box<dyn Page>,
    acts: Acts,
    scope: u32,
    /// Looping elements by name and the node they run on.
    loops: HashMap<&'static str, Id>,
}

impl Slot {
    fn new(what: String, host: Id, page: Box<dyn Page>, scope: u32) -> Slot {
        Slot { what, view: View::new(host), page, acts: Acts::default(), scope, loops: HashMap::new() }
    }
}

/// How the app starts (main.rs reads it from the environment).
#[derive(Clone, Debug, Default)]
pub struct Start0 {
    pub tab: Option<Tab>,
    pub open: Option<String>,
    pub boot_hold: bool,
    pub no_boot: bool,
    pub fixture: bool,
    pub running: Vec<String>,
}

/// A chrome view (top bar, footer, toasts, backdrop, hand-off): built from a closure, no tags.
struct Chrome {
    view: View,
    loops: HashMap<&'static str, Id>,
}

mod controls_glue;
mod alert_glue;
mod share_glue;
pub(crate) mod share_up;
mod social_glue;
mod journey_glue;
mod switcher_glue;
mod ingame_glue;
mod remap_glue;
mod keep_glue;
mod img_glue;
mod portal_glue;
mod support_glue;
mod agent_glue;
mod touch_glue;
pub use touch_glue::TouchPhase;
pub use switcher_glue::surface_up;

/// The first entry of a new send PIN while it is typed again (Settings > Accounts).
static SEND_PIN_FIRST: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub struct App {
    pub st: State,
    pub scene: Scene,
    pub text: TextSys,
    pub images: ImageLoader,
    pub nav: Nav,
    arts: Arts,
    /// When the image cache was last trimmed.
    last_trim: f64,
    fx: Fx,
    done_tx: Sender<Done>,
    done_rx: Receiver<Done>,
    waker: Arc<dyn Fn() + Send + Sync>,
    backdrop_host: Id,
    load_bar: Id,
    top: Chrome,
    footer: Chrome,
    toasts: Chrome,
    backdrop: Chrome,
    handoff: Chrome,
    page: Slot,
    overlay: Option<Slot>,
    options: Option<Slot>,
    osk: Option<Slot>,
    /// The wallet's review sheet, mounted for the app's lifetime (empty while nothing waits).
    review: Option<Slot>,
    boot: Option<Boot>,
    hints: Vec<Hint>,
    tab_spots: HashMap<Tab, Spot>,
    overlay_spot: Option<Spot>,
    /// Motion on (off for the first frame and golden images until the first input).
    animate: bool,
    clock: String,
    next_poll: f64,
    next_device: f64,
    next_pc: f64,
    next_account: f64,
    /// The wallet service's version Home's account was checked at.
    wallet_seen: u64,
    next_progress: f64,
    launching_until: f64,
    forced_running: Vec<String>,
    /// A deadline a page asked for.
    page_wake: Option<f64>,
    /// An overlay surface built on the scene directly (WS 13's Rare menu), and the root nodes it added.
    legacy: Option<(Box<dyn crate::screens::overlay::Screen>, Vec<Id>)>,
    /// The scene-built surface up is the Switcher (not the Rare menu).
    switcher_up: bool,
    /// The scene-built surface up is "Close a paused game?".
    paused_up: bool,
    /// The power sheet is the legacy surface (journey probe: overlay=power).
    power_up: bool,
    /// The game page's Move, waiting for Storage to have measured the games.
    pending_move: Option<String>,
    /// Rare over a running game (rare-overlay's session), while it is open.
    ingame: Option<ingame_glue::Ingame>,
    /// tools/journeys' probe.
    journey: journey_glue::Journey,
    /// A tab page built on the scene directly (WS 13's Settings, the Market tab), a Phase 4 page
    /// (`host::Page`: it may paint after the scene, the chart).
    legacy_page: Option<(Tab, Box<dyn crate::screens::market::host::Page>, Vec<Id>)>,
    /// Friends and notices, live (crate::social); None in fixtures.
    social: Option<Arc<crate::social::Social>>,
    share: Option<share_glue::ShareSlot>,
    notice_toasts: Option<crate::screens::notify::toast::ToastLayer>,
    social_seen: ((u64, u64), (u64, usize)),
    /// Price alerts: (first read asked, stream told there are active alerts).
    alerts_seen: (bool, bool),
    /// Settings on the device: the readings were started, the last version drawn, the next poll.
    settings_loaded: bool,
    settings_ver: u64,
    next_settings_poll: f64,
    /// The UI size changed: main rebuilds the app at the new size and comes back to Settings.
    pub rescale: bool,
    /// A full-page overlay covers the page: the page's loops (a breathing Live dot) are held at
    /// rest, so nothing hidden under it keeps the screen redrawing.
    page_covered: bool,
    fixture: bool,
    /// UI sounds and rumble (Settings > Controls and Sound).
    player: rare_render::feedback::Feedback,
    side_seen: (u64, u64),
    social_names: u64,
    /// What `keep_update` saw last (app-level data, `data::keep`).
    keep_seen: keep_glue::KeepSeen,
    /// Where focus was when Rare Wallet's review sheet came up: it goes back there when it closes.
    review_spot: Option<Option<Spot>>,
    /// Touch and mouse (touch_glue).
    touch: touch_glue::Touch,
}

fn make_page(tab: Tab) -> Box<dyn Page> {
    match tab {
        Tab::Home => Box::new(crate::screens::home::Home::default()),
        Tab::Library => Box::new(crate::screens::library::Library::default()),
        Tab::Games => Box::new(crate::screens::sui::page::SuiTab::default()),
        Tab::Market => Box::new(Placeholder { title: "", overlay: false }),
        Tab::Wallet => Box::new(crate::screens::wallet::page::WalletTab::default()),
        Tab::Friends => Box::new(Placeholder { title: "", overlay: false }),
        Tab::Settings => Box::new(Placeholder { title: "", overlay: false }),
    }
}

fn make_overlay(o: &Overlay) -> Box<dyn Page> {
    match o {
        Overlay::Game(id) => Box::new(crate::screens::game::GameDetail::new(id.clone())),
        Overlay::Search => Box::new(crate::screens::search::Search::default()),
        Overlay::Qam => Box::new(crate::screens::quick::QuickSettings::default()),
        Overlay::LaunchFail { .. } => Box::new(crate::screens::game::launch::LaunchFail),
        Overlay::Other("signin") => Box::new(crate::screens::signin::SignInPage::default()),
        Overlay::Other("profile") => Box::new(crate::screens::sui::profile_page::ProfilePage::default()),
        // Share mounts its own scene page over this one (share_glue); anything else unknown stays blank.
        Overlay::Other(k) => crate::features::overlay_page(k).unwrap_or_else(|| Box::new(Placeholder { title: "", overlay: true })),
    }
}

/// A host for one overlay's view: an in-flow, full-size node in its layer, removed whole when the
/// overlay closes. (Removing an absolutely positioned view root straight from a layer leaves it in
/// the layer's cached list of hoisted children and the next layout's rounding pass panics: a WS 1
/// engine report.)
fn wrapper(scene: &mut Scene, z: i16) -> Id {
    let layer = scene.layer(z);
    let rem = scene.rem;
    scene.add(layer, s().full().to_style(rem), Node::default())
}

fn drop_slot(scene: &mut Scene, mut slot: Slot) {
    let host = slot.view.host();
    slot.view.clear(scene);
    scene.remove(host);
}

/// The layer an overlay draws in (its web z-index).
fn overlay_layer(o: &Overlay) -> i16 {
    match o {
        Overlay::Qam | Overlay::LaunchFail { .. } | Overlay::Other("apisheet") | Overlay::Other(crate::features::webgame::CONNECT) | Overlay::Other(crate::features::uninstall::KIND) | Overlay::Other(crate::features::cloudsync::CONFLICT) | Overlay::Other(crate::features::cloudsync::VERSIONS) => layer::SHEET,
        _ => layer::PAGE,
    }
}

fn overlay_what(o: &Overlay) -> String {
    match o {
        Overlay::Game(id) => format!("game:{id}"),
        Overlay::LaunchFail { id, .. } => format!("launchfail:{id}"),
        o => o.kind().to_string(),
    }
}

/// Builds a slot's view if its stamp moved. Returns true when the scene needs a layout.
#[allow(clippy::too_many_arguments)]
fn build_slot(slot: &mut Slot, scene: &mut Scene, text: &mut TextSys, img: &mut ImageLoader, arts: &mut Arts, st: &State, now: f64, force: bool) -> bool {
    let Slot { view, page, acts, scope, loops, .. } = slot;
    if force {
        view.invalidate();
    }
    let stamp = hash((page.stamp(st), arts.version));
    let (rem, w, h) = (scene.rem, scene.w, scene.h);
    let mut started = Vec::new();
    let edits = view.update_stamped(scene, text, stamp, now, || {
        acts.clear();
        let mut ui = Ui::new(rem, w, h, img, arts, now);
        let el = page.view(&mut ui, st, acts);
        started = std::mem::take(&mut ui.loops);
        if *scope > 0 {
            el.scope(*scope)
        } else {
            el
        }
    });
    let Some(e) = edits else { return false };
    start_loops(view, scene, loops, &started, now);
    e.layout()
}

fn build_chrome(c: &mut Chrome, scene: &mut Scene, text: &mut TextSys, img: &mut ImageLoader, arts: &mut Arts, stamp: u64, now: f64, f: impl FnOnce(&mut Ui) -> El) -> bool {
    let (rem, w, h) = (scene.rem, scene.w, scene.h);
    let stamp = hash((stamp, arts.version));
    let mut started = Vec::new();
    let edits = c.view.update_stamped(scene, text, stamp, now, || {
        let mut ui = Ui::new(rem, w, h, img, arts, now);
        let el = f(&mut ui);
        started = std::mem::take(&mut ui.loops);
        el
    });
    let Some(e) = edits else { return false };
    start_loops(&c.view, scene, &mut c.loops, &started, now);
    e.layout()
}

/// Starts the loops a build asked for on nodes that are new (a rebuild keeps a running loop).
fn start_loops(view: &View, scene: &mut Scene, running: &mut HashMap<&'static str, Id>, started: &[(&'static str, &'static Keys, f32)], now: f64) {
    // Golden images hold loops at rest so the screen can settle.
    if std::env::var("RR_SHOT").is_ok() {
        return;
    }
    let mut next = HashMap::new();
    for (name, keys, period) in started {
        if let Some(id) = view.id(name) {
            if running.get(name) != Some(&id) {
                scene.n(id).opacity.start_loop(keys, *period, now);
            }
            next.insert(*name, id);
        }
    }
    *running = next;
}

/// Holds every loop of a view at rest (its value now goes to the loop's rest value at once).
fn hold_loops(scene: &mut Scene, running: &HashMap<&'static str, Id>) {
    for id in running.values() {
        let a = &mut scene.n(*id).opacity;
        if a.looping() {
            let rest = a.target();
            a.snap(rest);
        }
    }
}

/// Runs `$body` with `$v` bound to each mounted view (disjoint field borrows, so the body may use
/// the scene and the text system).
macro_rules! each_view {
    ($s:ident, $v:ident => $body:expr) => {{
        for $v in [&mut $s.top.view, &mut $s.footer.view, &mut $s.toasts.view, &mut $s.backdrop.view, &mut $s.handoff.view, &mut $s.page.view] {
            $body;
        }
        for slot in [$s.overlay.as_mut(), $s.options.as_mut(), $s.review.as_mut(), $s.osk.as_mut()].into_iter().flatten() {
            let $v = &mut slot.view;
            $body;
        }
    }};
}

impl App {
    pub fn new(mut scene: Scene, text: TextSys, mut images: ImageLoader, backend: Arc<dyn Backend>, waker: Arc<dyn Fn() + Send + Sync>, s0: Start0) -> App {
        let now = clock::secs();
        crate::features::bus::set_waker(waker.clone());
        crate::features::apps::init(backend.clone());
        crate::screens::wallet::service::install();
        let mut st = State::default();
        // Fixtures: RR_FRIENDS=mock|party|edge-<name> puts the web mock friends on Home (no live service).
        match std::env::var("RR_FRIENDS").as_deref() {
            Ok("mock") => st.friends = Some(Arc::new(crate::screens::friends::model::mock())),
            Ok("party") => st.friends = Some(Arc::new(crate::screens::friends::model::mock_party())),
            // edge-<name>: the Friends edge fixtures (edge-convo has a friend's profile open)
            Ok(e) if e.starts_with("edge-") => st.friends = Some(Arc::new(crate::screens::friends::model::edge(&e[5..]))),
            _ => {}
        }
        if let Some(t) = s0.tab {
            st.tab = t;
        }
        st.device = backend.device();
        st.ver.device += 1;
        if s0.fixture {
            st.live = crate::data::live::fixture(wall_ms());
            st.ver.live += 1;
        }
        if let Some(lib) = backend.cached_library() {
            st.set_library(lib);
        }
        st.running = s0.running.clone();
        st.favs = load_favs();
        st.user = crate::data::userlib::UserLib::load();
        st.played = crate::data::played::Played::load();
        if let Some(l) = st.library.as_mut() {
            l.split_cloud();
            st.played.apply(l, crate::data::model::now_secs());
        }
        // Renamed copies name their group (the device's game-custom.json; fixtures start with none).
        if !s0.fixture {
            st.renamed = rare_library::custom::load().into_iter().filter(|(_, c)| c.title.as_ref().is_some_and(|t| !t.trim().is_empty())).map(|(id, _)| id).collect();
        }
        st.regroup();
        st.note_played();
        if s0.fixture && std::env::var("RR_ACCOUNT").is_ok() {
            let mut a = crate::data::account::fixture();
            // Shots: RR_ACCOUNT_USD / RR_ACCOUNT_CHANGE ("none" leaves it out), RR_ACCOUNT_LABEL.
            let num = |k: &str, d: Option<f64>| std::env::var(k).ok().map_or(d, |v| v.parse().ok());
            a.total_usd = num("RR_ACCOUNT_USD", a.total_usd);
            a.change_usd = num("RR_ACCOUNT_CHANGE", a.change_usd);
            if let Ok(l) = std::env::var("RR_ACCOUNT_LABEL") {
                a.label = l;
            }
            st.account = Some(a);
        }
        // The last run's numbers at once (Home's card, the profile chip, the shelves); they
        // refresh in place.
        if !s0.fixture {
            st.account = crate::data::account::boot();
            if let Some(l) = crate::data::live::kept() {
                st.live = l;
            }
            st.ver.account += 1;
            st.ver.live += 1;
        }
        if !s0.fixture {
            crate::features::boot(&mut st);
        }

        // The frame: the art behind Home, the top bar, the page, the bottom bar, as one column.
        let rem = scene.rem;
        let root = scene.root;
        let mut rs = scene.style(root);
        rs.flex_direction = rare_render::taffy::FlexDirection::Column;
        scene.set_style(root, rs);
        scene.n(root).bg.snap(tok::BG);
        scene.ring_gap_color = tok::BG;
        scene.ring_color = tok::FG;
        let backdrop_host = scene.add(root, s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h(40.0).to_style(rem), Node::default().z(0));
        let top_host = scene.add(root, s().row().h(4.25).to_style(rem), Node::default().z(20));
        let main_host = scene.add(root, s().grow().col().to_style(rem), Node::default());
        let footer_host = scene.add(root, s().row().h(3.0).to_style(rem), Node::default().z(40));
        // The loading bar under the top bar: a 2 px strip whose segment slides (app.css .load-bar).
        let load_bar = scene.add(root, s().abs(Some(4.25), Some(0.0), None, Some(0.0)).h_px(2.0).to_style(rem), Node::default().z(30).clip().opacity(0.0));
        let seg = scene.add(load_bar, s().fill().to_style(rem), Node::default());
        let strip = scene.add(seg, s().abs(Some(-2.0 / rem), None, None, Some(0.0)).wp(1.0).h_px(6.0).to_style(rem), Node::bg(tok::fg(0.55)).origin(0.0, 0.5));
        let _ = (seg, strip);

        let toast_layer = scene.layer(layer::TOAST);
        let modal = scene.layer(layer::MODAL);
        let boot = (!s0.no_boot).then(|| Boot::build(&mut scene, &mut images, now, s0.boot_hold));
        let (done_tx, done_rx) = channel();
        let nav = Nav::new(scene.w, scene.h);
        let touch = touch_glue::Touch::new(scene.rem);
        let mut app = App {
            st,
            scene,
            text,
            images,
            nav,
            arts: Arts::default(),
            last_trim: 0.0,
            fx: Fx::new(backend),
            done_tx,
            done_rx,
            waker,
            backdrop_host,
            load_bar,
            top: Chrome { view: View::new(top_host), loops: HashMap::new() },
            footer: Chrome { view: View::new(footer_host), loops: HashMap::new() },
            toasts: Chrome { view: View::new(toast_layer), loops: HashMap::new() },
            backdrop: Chrome { view: View::new(backdrop_host), loops: HashMap::new() },
            handoff: Chrome { view: View::new(modal), loops: HashMap::new() },
            page: Slot::new("none".into(), main_host, Box::new(Placeholder { title: "", overlay: false }), SCOPE_PAGE),
            overlay: None,
            options: None,
            osk: None,
            review: None,
            boot,
            hints: Vec::new(),
            tab_spots: HashMap::new(),
            overlay_spot: None,
            animate: false,
            clock: clock_text(),
            next_poll: 0.0,
            next_device: now + 30.0,
            next_pc: 0.0,
            next_account: 0.0,
            wallet_seen: 0,
            next_progress: 0.0,
            launching_until: 0.0,
            forced_running: s0.running.clone(),
            page_wake: None,
            legacy: None,
            switcher_up: false,
            paused_up: false,
            power_up: false,
            pending_move: None,
            ingame: None,
            journey: journey_glue::Journey::new(),
            legacy_page: None,
            social: None,
            share: None,
            notice_toasts: None,
            social_seen: ((u64::MAX, 0), (0, usize::MAX)),
            alerts_seen: (false, false),
            settings_loaded: false,
            settings_ver: 0,
            next_settings_poll: 0.0,
            rescale: false,
            page_covered: false,
            fixture: s0.fixture,
            player: controls_glue::player(s0.fixture),
            side_seen: (u64::MAX, 0),
            social_names: u64::MAX,
            keep_seen: Default::default(),
            review_spot: None,
            touch,
        };
        if !s0.fixture {
            app.apply_controls();
            if std::env::var_os("RR_SHOT").is_none() {
                crate::side::set_waker(app.waker.clone());
                crate::side::start_share();
            }
        }
        app.social = social_glue::start(app.waker.clone());
        // The first scan (the cache filled the first frame when there was one).
        app.st.scanning = app.st.library.is_none();
        app.fx.spawn(|b| {
            let r = b.scan();
            Box::new(move |st: &mut State, now: f64| {
                st.scanning = false;
                match r {
                    Ok(l) => st.set_library(l),
                    Err(e) => {
                        log::warn!("scan: {e}");
                        if st.library.is_none() {
                            st.toast("Couldn't read your library. Scan again from the Library tab.", Tone::Error, now);
                        }
                    }
                }
            })
        });
        if !s0.fixture {
            app.start_live();
        }
        app.fx.spawn(|_| {
            let games: Vec<_> = crate::data::catalog::load().into_iter().filter(|g| g.active()).collect();
            Box::new(move |st: &mut State, _| {
                st.sui_games = games;
                st.ver.catalog += 1;
            })
        });
        if let Some(o) = s0.open.as_deref() {
            app.open_from_env(o);
        }
        app.flush_jobs();
        app
    }

    fn open_from_env(&mut self, o: &str) {
        let now = clock::secs();
        let (kind, rest) = o.split_once(':').unwrap_or((o, ""));
        match kind {
            "game" => self.st.open(Some(Overlay::Game(rest.to_string()))),
            "search" => {
                self.st.open(Some(Overlay::Search));
                match std::env::var("RR_SEARCH") {
                    Ok(q) => {
                        self.st.search = q;
                        self.st.ver.search += 1;
                    }
                    Err(_) => self.ask_search(),
                }
            }
            "qam" => self.st.open(Some(Overlay::Qam)),
            "launchfail" => {
                let mut p = rest.splitn(2, '|');
                let id = p.next().unwrap_or("").to_string();
                let reason = p.next().unwrap_or("quick").to_string();
                self.st.open(Some(Overlay::LaunchFail { id, reason, code: None }));
            }
            "options" => self.st.options = game_options(&self.st, rest),
            "uninstall" => crate::features::uninstall::open(&mut self.st, rest),
            // Cloud saves sheets (shots): saveversions:<game id>, saveconflict.
            "saveversions" => crate::features::cloudsync::open_versions(&mut self.st, rest),
            "saveconflict" => self.st.open(Some(Overlay::Other(crate::features::cloudsync::CONFLICT))),
            "osk" => self.st.osk = Some(OskReq::new("Sui address or SuiNS name to watch", "rare.sui", Some("0x... or name.sui"), OskFor::Nothing)),
            "handoff" => self.st.handoff_until = Some(now + 3600.0),
            "toast" => self.st.toast_for(rest.to_string(), Tone::Info, now, 3600.0),
            "signin" => self.st.open(Some(Overlay::Other("signin"))),
            // A Market page: market:collection:<slug>, market:item:<id>, ...
            "market" => self.show_market(rest.to_string()),
            // A Sui game's page in the Sui tab: sui:<slug>.
            "sui" => self.run(Act::SuiGame(rest.to_string())),
            "profile" => self.st.open(Some(Overlay::Other("profile"))),
            "share" => self.st.open(Some(Overlay::Other("share"))),
            // Rare over a running game as rare-overlay's event would bring it (shots):
            // ingame:<run id>[|menu|switcher|quick|power][|frame.png].
            "ingame" => {
                let mut p = rest.splitn(3, '|');
                let rid = p.next().unwrap_or("");
                let mode = p.next().filter(|m| !m.is_empty()).unwrap_or("menu");
                let frame = p.next().map(String::from);
                let payload = serde_json::json!({ "open": true, "gameId": (!rid.is_empty()).then_some(rid), "web": false, "frame": frame, "mechanism": "window", "mode": mode, "guest": false, "home": false });
                crate::features::bus::ctx().send(rare_core::Event::Other { name: "overlay".into(), payload });
            }
            // Game management (features/gamemgmt): gm-pick, gm-art:<game id>.
            "gm-pick" if rest == "folder" => crate::features::gamemgmt::picker::open(&mut self.st, crate::features::gamemgmt::picker::Mode::Folder),
            "gm-pick" => self.st.open(Some(Overlay::Other(crate::features::gamemgmt::PICK))),
            "gm-art" => crate::features::gamemgmt::artpage::open(&mut self.st, rest, rare_library::custom::ArtKind::Cover),
            "downloads" => self.st.open(Some(Overlay::Other(crate::features::downloads::KIND))),
            // trophies:<case id>: the case at a game; trophypop: an unlock pop-up (shots).
            "trophies" if !rest.is_empty() => {
                crate::features::trophies::set_open(rest.parse().ok());
                self.st.open(Some(Overlay::Other("trophies")));
            }
            "trophypop" => crate::features::trophies::push_popup_any(crate::screens::trophies::ui::Popup { minted: false, name: if rest.is_empty() { "A week of Hades II".into() } else { rest.to_string() }, game: Some("Hades II".into()), tier: Some(1), points: Some(30), source: None, icon_url: Some(rare_trophies::badge::data_uri(rare_trophies::milestones::Kind::Streak7, "steam:1145350")), more: 0 }),
            "firstrun" | "saves" | "trophies" | "apisheet" | "stats" | "year" | "wishlist" | "fan-curve" | "game-buttons" | "emusetup" | "emubios" | "emura" => self.st.open(Some(Overlay::Other(crate::features::KINDS.iter().find(|k| **k == kind).unwrap()))),
            _ => log::warn!("RR_OPEN: unknown {o}"),
        }
    }

    fn start_live(&mut self) {
        let tx = self.done_tx.clone();
        let waker = self.waker.clone();
        let init = self.st.live.clone();
        std::thread::Builder::new()
            .name("rr-live".into())
            .spawn(move || {
                crate::data::live::run(init, move |l| {
                    let done: Done = Box::new(move |st: &mut State, _| {
                        if st.live != l {
                            st.live = l;
                            st.ver.live += 1;
                        }
                    });
                    let ok = tx.send(done).is_ok();
                    waker();
                    ok
                });
            })
            .ok();
    }

    // ------------------------------------------------------------------ input

    /// One press, down the stack like nav/input.ts dispatch: the keyboard, the Options sheet, the
    /// overlay (or the page), the shell, then focus.
    pub fn input(&mut self, b: Btn, repeat: bool) {
        // A pad press: the focus ring is back (touch hid it).
        self.touch_pad();
        self.input_inner(b, repeat);
    }

    fn input_inner(&mut self, b: Btn, repeat: bool) {
        self.animate = true;
        crate::features::services::touch();
        self.journey_pad(b, repeat);
        if self.ingame_fixture_press(b) {
            self.after_input();
            return;
        }
        let setup = matches!(self.st.overlay, Some(Overlay::Other("firstrun" | "signin")));
        if matches!(b, Btn::GuideHold | Btn::ExitHold) {
            // The Rare button held, or View + Menu: the Switcher (App.tsx guidehold / exithold).
            if !setup && !self.switcher_up {
                self.open_switcher();
            }
            self.after_input();
            return;
        }
        // Rare Wallet's review sheet is over everything but the keyboard: no page under it (Share,
        // Settings, Market, Friends) sees a press while it is up. Friends and Settings ask for
        // rare.market sign-ins, so the sheet often sits over them.
        let review_up = crate::screens::wallet::sign::host::active() && self.st.osk.is_none();
        if self.share.is_some() && !review_up && self.legacy.is_none() && self.st.options.is_none() && self.st.osk.is_none() {
            self.share_pad(b);
            self.after_input();
            return;
        }
        // The keyboard or Rare Wallet's review sheet over the Rare menu (Ask Rare asks for text and
        // prepares reviews with the menu up): they take the presses, the menu waits under them.
        let modal = self.st.osk.is_some() || crate::screens::wallet::sign::host::active();
        if self.legacy.is_some() && modal && !matches!(b, Btn::Guide) {
            if !self.dispatch(b) {
                self.default_pad(b, repeat);
            }
            self.after_input();
            return;
        }
        if self.legacy.is_some() {
            self.legacy_pad(b);
            self.after_input();
            return;
        }
        if b == Btn::Guide {
            // The Rare button with the launcher in front: the Rare menu (App.tsx "guide"); nothing
            // during setup.
            if !setup {
                self.open_rare_menu();
            }
            self.after_input();
            return;
        }
        let page_y = b == Btn::Y && matches!(self.st.tab, Tab::Market | Tab::Settings);
        if self.legacy_page.is_some() && !review_up && self.st.overlay.is_none() && self.st.options.is_none() && self.st.osk.is_none() && (page_y || !matches!(b, Btn::Lb | Btn::Rb | Btn::Y | Btn::Qam)) && self.legacy_page_pad(b) {
            self.after_input();
            return;
        }
        if b == Btn::Start && !review_up && self.st.overlay.is_none() && self.st.options.is_none() && self.st.osk.is_none() && self.notice_start() {
            self.after_input();
            return;
        }
        // the review sheet takes every press: nothing behind it reacts (no tab switch, no Search)
        let review = crate::screens::wallet::sign::host::active() && self.st.osk.is_none();
        if !self.dispatch(b) && (review || !self.app_pad(b)) {
            self.default_pad(b, repeat);
        }
        self.after_input();
    }

    fn cur(&self) -> Option<(rare_render::focus::Focusable, Option<Act>)> {
        let it = self.nav.current_item()?.clone();
        let act = self.slot_of(it.scope).and_then(|s| s.acts.get(it.tag).cloned());
        Some((it, act))
    }

    fn slot_of(&self, scope: u32) -> Option<&Slot> {
        match scope {
            SCOPE_PAGE => Some(&self.page),
            // A page's own [data-nav-scope] (a Sui game's page) is scope 1 too: with no overlay up,
            // its presses are the page's.
            SCOPE_OVERLAY => self.overlay.as_ref().or(Some(&self.page)),
            SCOPE_OPTIONS => self.options.as_ref(),
            SCOPE_OSK => self.osk.as_ref(),
            SCOPE_REVIEW => self.review.as_ref(),
            // A page's own sheet with a focus scope of its own (the game page's Performance sheet,
            // 3): its presses belong to the page on top.
            _ => self.overlay.as_ref().or(Some(&self.page)),
        }
    }

    fn slot_mut(&mut self, scope: u32) -> Option<&mut Slot> {
        match scope {
            SCOPE_PAGE => Some(&mut self.page),
            SCOPE_OVERLAY => self.overlay.as_mut(),
            SCOPE_OPTIONS => self.options.as_mut(),
            SCOPE_OSK => self.osk.as_mut(),
            SCOPE_REVIEW => self.review.as_mut(),
            _ => self.overlay.as_mut().or(Some(&mut self.page)),
        }
    }

    fn dispatch(&mut self, b: Btn) -> bool {
        let cur = self.cur();
        let c = cur.as_ref().map(|(f, a)| (f, a.as_ref()));
        if let Some(s) = self.osk.as_mut() {
            s.page.pad(&mut self.st, &mut self.fx, b, c);
            return true;
        }
        if crate::screens::wallet::sign::host::active() {
            if let Some(s) = self.review.as_mut() {
                return s.page.pad(&mut self.st, &mut self.fx, b, c);
            }
        }
        if let Some(s) = self.options.as_mut() {
            return s.page.pad(&mut self.st, &mut self.fx, b, c);
        }
        match self.overlay.as_mut() {
            Some(s) => s.page.pad(&mut self.st, &mut self.fx, b, c),
            None => self.page.page.pad(&mut self.st, &mut self.fx, b, c),
        }
    }

    /// App.tsx's own handler.
    fn app_pad(&mut self, b: Btn) -> bool {
        let now = clock::secs();
        match b {
            // Setup's account pages show no tabs.
            Btn::Lb | Btn::Rb if crate::features::firstrun::wallet_from_setup() => true,
            Btn::Lb | Btn::Rb => {
                // LB/RB are always the launcher's tabs: a page or sheet over the tab closes first.
                self.st.close();
                let n = TABS.len();
                let j = match self.st.tab.index() {
                    None => {
                        if b == Btn::Rb {
                            0
                        } else {
                            n - 1
                        }
                    }
                    Some(i) => (i + if b == Btn::Rb { 1 } else { n - 1 }) % n,
                };
                self.st.set_tab(TABS[j].0, false);
                self.fx.fb(Feedback::Tab);
                true
            }
            Btn::B => {
                // A start still going after 15 s with no window: B cancels it.
                let slow = if self.st.overlay.is_none() { cancellable(&self.st.starts, now) } else { None };
                if let Some(id) = slow {
                    self.fx.spawn(move |be| {
                        let _ = be.quit(&id);
                        Box::new(|_: &mut State, _| {})
                    });
                    self.st.toast("Cancelled", Tone::Info, now);
                    self.fx.fb(Feedback::Back);
                    return true;
                }
                if self.st.back() {
                    self.fx.fb(Feedback::Back);
                }
                true
            }
            Btn::Qam => {
                let open = self.st.overlay == Some(Overlay::Qam);
                self.fx.fb(if open { Feedback::Close } else { Feedback::Open });
                self.st.open(if open { None } else { Some(Overlay::Qam) });
                true
            }
            // X on an app tile: its page, as the tile's Details hint says.
            Btn::X if self.st.overlay.is_none() && matches!(self.cur(), Some((_, Some(Act::OpenApp(_))))) => {
                if let Some((_, Some(Act::OpenApp(id)))) = self.cur() {
                    self.open_app(&id, true);
                }
                true
            }
            Btn::Start => {
                // Menu on an app tile: the app's page (Details).
                if self.st.overlay.is_none() {
                    if let Some((_, Some(Act::OpenApp(id)))) = self.cur() {
                        self.open_app(&id, true);
                        return true;
                    }
                }
                let id = match &self.st.overlay {
                    Some(Overlay::Game(id)) => Some(id.clone()),
                    Some(_) => return false,
                    None => self.nav.current_item().and_then(|it| self.slot_of(it.scope).and_then(|s| s.acts.game_of(it.tag)).map(String::from)),
                };
                let Some(spec) = id.and_then(|id| game_options(&self.st, &id)) else { return false };
                self.fx.fb(Feedback::Open);
                self.st.options = Some(spec);
                true
            }
            Btn::Y => {
                if self.st.overlay.is_some() || crate::features::firstrun::wallet_from_setup() {
                    return false;
                }
                self.fx.fb(Feedback::Open);
                self.st.open(Some(Overlay::Search));
                self.ask_search();
                true
            }
            _ => false,
        }
    }

    /// The Rare menu over the launcher (WS 13's RareMenu): Rare, Friends, Notifications.
    fn open_rare_menu(&mut self) {
        use crate::screens::overlay::menu::{GameInfo, MenuData, RareMenu};
        let now = clock::secs();
        self.st.close();
        self.st.options = None;
        // Over a running game, a web game or a webview app (rare-overlay's session): its still,
        // the Game tab first.
        let (session, rid, frame) = match self.ingame_session() {
            Some((rid, frame)) => (true, rid, frame),
            None => (false, None, None),
        };
        let (game, is_app, in_library) = match rid.as_deref() {
            Some(r) => self.menu_game(r),
            None => (GameInfo { title: String::new(), banner: None, logo: None }, false, false),
        };
        // Something runs under the launcher: Resume leads the Rare tab.
        let resume = if rid.is_none() { self.st.running.first().map(|r| self.st.run_name(r)) } else { None };
        let d = MenuData {
            session,
            over_game: rid.is_some(),
            rid,
            frame,
            guest: rare_overlay::guest(),
            game,
            is_app,
            in_library,
            // The header's account chip (the current Rare Wallet account).
            address: self.st.account.as_ref().map(|a| a.address.clone()).or_else(|| if self.fixture { None } else { crate::data::account::current().map(|c| c.0) }),
            name: self.st.account.as_ref().map(|a| a.display_name()).or_else(|| if self.fixture { None } else { crate::data::account::current().map(|(a, l)| crate::data::account::display_name(None, &l, &a)) }),
            signed_out: true,
            collections: false,
            trophies: false,
            items: None,
            unread: 0,
            trophies_count: 0,
            loading: false,
            perf: None,
            power_saved: false,
            started_ms: None,
            now_ms: wall_ms(),
            emu: vec![],
            running: !self.st.running.is_empty(),
            taking: None,
            invite: false,
            tab: (crate::features::agent::take_open_on_ask() || (self.fixture && std::env::var("RR_ASK_TAB").is_ok())).then_some(crate::screens::overlay::menu::model::MenuTab::Ask),
            colls: vec![],
            replay: if self.fixture { Some(30) } else { crate::features::capture::replay_on().then(crate::features::capture::replay_seconds) },
            discord: if self.fixture {
                None
            } else {
                let s = rare_discord::status();
                s.client.map(|_| (s.muted, s.deafened))
            },
            resume,
        };
        // What Ask Rare tells the model about now, and the still the in-game helper can send.
        crate::features::agent::set_context(crate::features::agent::Context {
            game: d.rid.as_ref().map(|_| d.game.title.clone()).filter(|t| !t.is_empty()),
            account: d.name.clone(),
            frame: match &d.frame {
                Some(rare_render::Src::File(p)) => Some(p.clone()),
                // A fixture still (shots): the picture switch shows; no picture is read.
                Some(_) if self.fixture => Some("fixture".into()),
                _ => None,
            },
        });
        let d = self.menu_game_data(d);
        // Opened right as a game started: its settings and rows are read now, not after a tick.
        self.game_perf_sync();
        let d = self.menu_game_extras(d);
        let (unread, invite) = self.menu_feed();
        let d = MenuData { unread: unread as u32, invite, signed_out: d.address.is_none(), ..d };
        // Shots: every tab of a Sui game over it (the widest tab strip).
        let d = if self.fixture && std::env::var("RR_MENU_ALLTABS").is_ok() { MenuData { collections: true, trophies: true, trophies_count: 7, unread: 3, ..d } } else { d };
        let root = self.scene.root;
        let before = self.scene.children(root);
        let screen: Box<dyn crate::screens::overlay::Screen> = {
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            Box::new(RareMenu::build(&mut cx, d, now))
        };
        let nodes: Vec<Id> = self.scene.children(root).into_iter().filter(|n| !before.contains(n)).collect();
        for n in &nodes {
            // Over the page, the bars and the sheets (the menu is a sheet-layer surface).
            self.scene.n(*n).z = layer::SHEET + 5;
        }
        self.fx.fb(Feedback::Open);
        self.legacy = Some((screen, nodes));
    }

    /// The sentences the open legacy screen wants toasted (a PC game sheet's failures).
    fn legacy_messages(&mut self, now: f64) {
        while let Some(m) = self.legacy.as_mut().and_then(|(s, _)| s.take_message()) {
            self.st.toast(m, Tone::Error, now);
        }
    }

    /// A PC game's sheet over Home (stream/sheets.tsx StreamAppSheet), on the device's rare-stream.
    fn open_pc_sheet(&mut self, uuid: String, app: u32, now: f64) {
        self.open_stream_sheet(crate::screens::stream::SheetKind::App(uuid, app), now);
    }

    /// The streaming sheets: a PC game's (Play, Wake and play, Resume, Quit) or the setup and pairing.
    fn open_stream_sheet(&mut self, kind: crate::screens::stream::SheetKind, now: f64) {
        use crate::screens::stream::{state::StreamState, StreamUi};
        let Some(api) = self.fx.backend.stream_api() else {
            self.st.toast("PC streaming isn't set up on this device yet.", Tone::Info, now);
            return;
        };
        let root = self.scene.root;
        let before = self.scene.children(root);
        let mut st = StreamState::new(api);
        st.load();
        let ui = {
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            let mut ui = StreamUi::new(&mut cx, st, None, Some(kind), now);
            ui.sim = false;
            // Moonlight missing: Rare gets it now, so the first stream never stops at a Get button.
            ui.need_moonlight(&mut cx, now);
            ui
        };
        let nodes: Vec<Id> = self.scene.children(root).into_iter().filter(|n| !before.contains(n)).collect();
        if !ui.has_sheet() {
            for n in nodes {
                self.scene.remove(n);
            }
            self.st.toast("That PC isn't answering.", Tone::Error, now);
            return;
        }
        for n in &nodes {
            self.scene.n(*n).z = layer::SHEET + 5;
        }
        self.fx.fb(Feedback::Open);
        self.legacy = Some((Box::new(ui), nodes));
    }

    fn close_legacy(&mut self) {
        crate::features::agent::set_shown(false);
        self.switcher_up = false;
        self.paused_up = false;
        self.power_up = false;
        if let Some((_, nodes)) = self.legacy.take() {
            for n in nodes {
                self.scene.remove(n);
            }
        }
    }

    fn legacy_pad(&mut self, b: Btn) {
        use rare_render::input::Pad;
        let now = clock::secs();
        if b == Btn::Guide && self.switcher_up {
            // The Rare button over the Switcher: the Rare menu.
            self.close_legacy();
            self.open_rare_menu();
            return;
        }
        if b == Btn::Guide {
            // The Rare button again: Home (over a game rare-overlay decides that itself).
            self.close_legacy();
            self.st.set_tab(Tab::Home, false);
            self.fx.fb(Feedback::Back);
            return;
        }
        let pad = match b {
            Btn::Up => Pad::Up,
            Btn::Down => Pad::Down,
            Btn::Left => Pad::Left,
            Btn::Right => Pad::Right,
            Btn::A => Pad::A,
            Btn::B => Pad::B,
            Btn::X => Pad::X,
            Btn::Y => Pad::Y,
            Btn::Lb => Pad::Lb,
            Btn::Rb => Pad::Rb,
            Btn::Lt => Pad::Lt,
            Btn::Rt => Pad::Rt,
            Btn::Start => Pad::Start,
            Btn::Select => Pad::Select,
            Btn::Qam => Pad::Qam,
            _ => return,
        };
        let out = {
            let Some((screen, _)) = self.legacy.as_mut() else { return };
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            screen.pad(&mut cx, pad, now)
        };
        self.legacy_out(out);
    }

    /// What the scene-built surface said after a press (or a touch on a slider).
    fn legacy_out(&mut self, out: crate::screens::overlay::Out) {
        use crate::screens::overlay::Out;
        let now = clock::secs();
        self.legacy_messages(now);
        if self.paused_up {
            if let Out::Act(a) = &out {
                let a = a.clone();
                self.close_legacy();
                self.paused_act(&a);
                return;
            }
        }
        if self.ingame_out(&out) {
            return;
        }
        match out {
            Out::None | Out::Moved => {}
            Out::Close => {
                self.close_legacy();
                self.fx.fb(Feedback::Back);
            }
            Out::Act(a) if self.ask_for_act(&a) => {}
            Out::Act(a) => {
                self.close_legacy();
                self.legacy_act(&a);
            }
        }
    }

    /// An `Out::Act` that is a question for the on-screen keyboard (askText in the web screens):
    /// opens it keyed by the act; the answer goes back through `Screen::answer`. False otherwise.
    fn ask_for_act(&mut self, a: &str) -> bool {
        let Some((title, placeholder)) = osk_for_act(a) else { return false };
        self.st.ask_text(a, title, "", Some(placeholder));
        self.fx.fb(Feedback::Open);
        true
    }

    /// Hands a keyboard answer nobody took on the page side to the scene-built screens, and keeps
    /// their live status line current while the keyboard is up for them.
    fn legacy_osk(&mut self, now: f64) {
        use crate::screens::overlay::Out;
        if let Some(OskReq { purpose: OskFor::Screen(k), value, .. }) = self.st.osk.as_ref() {
            let (k, v) = (k.clone(), value.clone());
            let mut line = None;
            if let Some((_, screen, _)) = self.legacy_page.as_mut() {
                line = screen.osk_status(&k, &v);
            }
            if line.is_none() {
                if let Some((screen, _)) = self.legacy.as_mut() {
                    line = screen.osk_status(&k, &v);
                }
            }
            if line.is_some() {
                self.st.set_osk_status(&k, line);
            }
        }
        let Some((key, value)) = self.st.osk_answer.take() else { return };
        let mut outs = Vec::new();
        {
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            if let Some((screen, _)) = self.legacy.as_mut() {
                outs.push(screen.answer(&mut cx, &key, value.clone(), now));
            } else if let Some((_, screen, _)) = self.legacy_page.as_mut() {
                outs.push(screen.answer(&mut cx, &key, value, now));
            }
        }
        for o in outs {
            match o {
                Out::Act(a) if self.ask_for_act(&a) => {}
                Out::Act(a) => log::info!("answer act: {a}"),
                _ => {}
            }
        }
    }

    /// A press for the scene-built tab page; false when it is not for it (B at the top goes back).
    fn legacy_page_pad(&mut self, b: Btn) -> bool {
        use rare_render::input::Pad;
        let now = clock::secs();
        let pad = match b {
            Btn::Up => Pad::Up,
            Btn::Down => Pad::Down,
            Btn::Left => Pad::Left,
            Btn::Right => Pad::Right,
            Btn::A => Pad::A,
            Btn::B => Pad::B,
            Btn::X => Pad::X,
            Btn::Y => Pad::Y,
            Btn::Lt => Pad::Lt,
            Btn::Rt => Pad::Rt,
            Btn::Start => Pad::Start,
            Btn::Select => Pad::Select,
            _ => return false,
        };
        let out = {
            let Some((_, screen, _)) = self.legacy_page.as_mut() else { return false };
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            screen.pad(&mut cx, pad, now)
        };
        self.legacy_page_out(out)
    }

    /// What the scene-built tab page said after a press (or a touch on a slider).
    fn legacy_page_out(&mut self, out: crate::screens::overlay::Out) -> bool {
        use crate::screens::overlay::Out;
        match out {
            Out::Moved => true,
            Out::None => false,
            Out::Close => {
                if self.st.back() {
                    self.fx.fb(Feedback::Back);
                }
                true
            }
            Out::Act(a) if self.ask_for_act(&a) => true,
            Out::Act(a) => {
                if a.starts_with("friends:") {
                    self.friends_act(&a);
                } else if !self.settings_act(&a, None) {
                    log::warn!("settings: nothing handles {a}");
                }
                true
            }
        }
    }

    /// Mounts the scene-built screen for tabs other workstreams have on main.
    fn mount_legacy_page(&mut self, tab: Tab, now: f64) {
        if let Some((_, _, nodes)) = self.legacy_page.take() {
            for n in nodes {
                self.scene.remove(n);
            }
        }
        if !matches!(tab, Tab::Settings | Tab::Market | Tab::Friends) {
            return;
        }
        let root = self.scene.root;
        let before = self.scene.children(root);
        let screen: Option<Box<dyn crate::screens::market::host::Page>> = {
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            match tab {
                Tab::Settings if self.fixture => Some(Box::new(ScreenPage(Box::new(crate::screens::settings::Settings::fixture_open(&mut cx, now)))) as Box<dyn crate::screens::market::host::Page>),
                Tab::Settings => {
                    use crate::screens::settings::{fixture_area, live, sections::SectionId, Settings};
                    live::enable();
                    if !self.settings_loaded {
                        self.settings_loaded = true;
                        let sources: Vec<(String, u32)> = self.st.library.as_ref().map(|l| l.sources.iter().filter(|s| s.available || s.count > 0).map(|s| (s.source.clone(), s.count)).collect()).unwrap_or_default();
                        live::load_all(sources, self.waker.clone());
                    }
                    let area = fixture_area(&mut cx);
                    Some(Box::new(ScreenPage(Box::new(Settings::live(&mut cx, area, SectionId::System, now)))) as Box<dyn crate::screens::market::host::Page>)
                }
                Tab::Friends => {
                    let main = crate::screens::trade::main_area(&mut cx);
                    Some(Box::new(crate::screens::friends::tab::FriendsTab::new(&mut cx, main, self.social.clone(), now)) as Box<dyn crate::screens::market::host::Page>)
                }
                _ => {
                    // The Market tab: Collections, Coins, Perps and Traders on live data, every act
                    // its pages emit handled by `screens::market::live`.
                    let main = crate::screens::trade::main_area(&mut cx);
                    Some(Box::new(crate::screens::market::live::LiveMarket::new(&mut cx, main, now)))
                }
            }
        };
        let nodes: Vec<Id> = self.scene.children(root).into_iter().filter(|n| !before.contains(n)).collect();
        match screen {
            Some(s) => self.legacy_page = Some((tab, s, nodes)),
            None => {
                for n in nodes {
                    self.scene.remove(n);
                }
            }
        }
    }

    /// What a Rare menu row does (`<tab>:<sid>`).
    fn legacy_act(&mut self, a: &str) {
        let now = clock::secs();
        if self.journey.probe.on() {
            self.journey_act(a);
        }
        let power = |fx: &mut Fx, action: &'static str| {
            fx.spawn(move |b| {
                let r = b.power(action);
                Box::new(move |st: &mut State, now| {
                    if let Err(e) = r {
                        st.toast(e, Tone::Error, now);
                    }
                })
            })
        };
        if self.menu_act(a) || self.switcher_act(a) {
            return;
        }
        if let Some((old, new)) = a.strip_prefix("close-and-play:").and_then(|r| r.split_once('\u{1f}')) {
            self.close_and_play(old.to_string(), new.to_string());
            return;
        }
        match a {
            _ if a.starts_with("toast:") && !a.starts_with("toast:error:") => self.st.toast(&a["toast:".len()..], Tone::Info, now),
            _ if a.starts_with("market:") => self.menu_market_act(a),
            _ if a.starts_with("android:") => {
                crate::features::android::act(&mut self.st, a, now);
            }
            "rare:home" => self.st.set_tab(Tab::Home, false),
            "rare:steam" | "open-steam" => self.open_steam(now),
            // The Rare button's first screen (overlay/tabs rare_tab) and its X and Y.
            "rare:resume" => {
                if let Some(id) = self.st.running.first().cloned() {
                    self.run(Act::Launch(id));
                }
            }
            "rare:quick" => {
                self.fx.fb(Feedback::Open);
                self.st.open(Some(Overlay::Qam));
            }
            "rare:screenshot" => {
                let wait = if self.ingame.is_some() {
                    self.ingame_resume();
                    450
                } else {
                    300
                };
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(wait));
                    crate::side::capture(false);
                });
            }
            "rare:share" => {
                self.ingame_leave();
                self.st.open(Some(Overlay::Other("share")));
            }
            "rare:friends" => {
                self.ingame_leave();
                self.st.set_tab(Tab::Friends, false);
            }
            "rare:search-settings" => {
                self.ingame_leave();
                self.st.close();
                self.st.set_tab(Tab::Settings, false);
                self.settings_act("settings:search", None);
            }
            "rare:power" => self.open_power_sheet(),
            "ask:setup" => {
                if self.ingame.is_some() {
                    self.ingame_act("home");
                }
                crate::screens::settings::open_section_next(crate::screens::settings::sections::SectionId::Ask);
                self.st.set_tab(Tab::Settings, false);
            }
            "stream:show-library" => self.st.set_tab(Tab::Library, false),
            "rare:settings" => {
                self.ingame_leave();
                self.st.set_tab(Tab::Settings, false);
            }
            "rare:profile" => {
                self.ingame_leave();
                self.st.open(Some(Overlay::Other("profile")));
            }
            "rare:downloads" | "downloads" => {
                self.ingame_leave();
                self.st.open(Some(Overlay::Other(crate::features::downloads::KIND)));
            }
            "rare:sleep" => power(&mut self.fx, "suspend"),
            "rare:restart" => power(&mut self.fx, "reboot"),
            "rare:poweroff" => power(&mut self.fx, "poweroff"),
            "rare:desktop" => power(&mut self.fx, "desktop"),
            // Quick settings' capture buttons. A screenshot waits for the panel (and the in-game
            // menu under it) to go, so it is of the game or the screen, not of Rare's panel.
            "qs:screenshot" => {
                self.st.open(None);
                let wait = if self.ingame.is_some() {
                    self.ingame_resume();
                    450
                } else {
                    300
                };
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(wait));
                    crate::side::capture(false);
                });
            }
            "qs:clip" => crate::side::capture(true),
            // The power sheet over the launcher (over a game ingame_act has them).
            "suspend" | "reboot" | "poweroff" => power(&mut self.fx, if a == "suspend" { "suspend" } else if a == "reboot" { "reboot" } else { "poweroff" }),
            "desktop" => power(&mut self.fx, "desktop"),
            // An app's page did the call itself (Apps model); it only closes.
            a if a.starts_with("app-open:") || a.starts_with("app-resume:") || a.starts_with("app-close:") => {}
            // A trophy row in the Rare menu is for reading (the web's rows have no action either).
            a if a.starts_with("trophies:") => {}
            a if a.starts_with("trophy:") => crate::features::trophies::on_code(&mut self.st, a),
            other => {
                if !self.settings_act(other, None) {
                    log::warn!("rare menu: nothing handles {other}");
                }
            }
        }
    }

    /// What a Settings row or sheet asks of the app. False when it is not a Settings action.
    fn settings_act(&mut self, a: &str, arg: Option<String>) -> bool {
        use crate::screens::settings::live;
        let now = clock::secs();
        let cfg = crate::screens::settings::config::load();
        let ask = |st: &mut State, title: &str, placeholder: &str, secret: bool, key: String, value: Option<String>| {
            st.osk = Some(OskReq { title: title.into(), value: value.unwrap_or_default(), placeholder: Some(placeholder.into()), secret, purpose: OskFor::Settings(key), status: None });
        };
        let command = |me: &mut App, cmd: &str, arg: &str| {
            if let Some((_, screen, _)) = me.legacy_page.as_mut() {
                let mut cx = crate::screens::overlay::Cx { scene: &mut me.scene, t: &mut me.text, img: &mut me.images };
                screen.command(&mut cx, cmd, arg, now);
            }
        };
        if self.remap_act(a) {
            return true;
        }
        match a {
            "settings:save" | "settings:saved" => self.apply_controls(),
            "settings:saved:ui-scale" => {
                self.apply_controls();
                self.rescale = true;
            }
            "settings:search" => ask(&mut self.st, "Search settings", "Brightness, Wi-Fi, volume", false, "query".into(), None),
            "storage:scan" => {
                self.run(Act::Scan);
                self.st.toast("Looking for games", Tone::Info, now);
            }
            "storage:add-folder" => self.open_folder_pick(),
            _ if a.starts_with("storage:") && self.storage_act(a) => {}
            "storage:grid-key" => ask(&mut self.st, "SteamGridDB API key", "", true, "gridkey".into(), cfg.steamgriddb_key.clone()),
            // Settings > Ask Rare. The key goes from the keyboard straight to Rare Wallet's vault.
            "ask:model" => {
                let kind = cfg.agent_provider.as_deref().and_then(rare_agent::provider::Kind::from_id);
                let ph = kind.map(|k| k.default_model()).filter(|m| !m.is_empty()).unwrap_or("Model name");
                ask(&mut self.st, "Model", ph, false, "askmodel".into(), cfg.agent_model.clone());
            }
            "ask:url" => {
                let kind = cfg.agent_provider.as_deref().and_then(rare_agent::provider::Kind::from_id);
                let ph = if kind == Some(rare_agent::provider::Kind::Ollama) { "http://192.168.0.20:11434" } else { "https://example.com/v1" };
                ask(&mut self.st, "Address", ph, false, "askurl".into(), cfg.agent_url.clone());
            }
            "ask:key" => ask(&mut self.st, "API key", "Paste or type your key", true, "askkey".into(), None),
            "ask:key-remove" => {
                if let Some(k) = cfg.agent_provider.as_deref().and_then(rare_agent::provider::Kind::from_id) {
                    crate::features::agent::save_key(k, None);
                }
            }
            "ask:test" => {
                self.st.toast("Asking your model...", Tone::Info, now);
                crate::features::agent::test();
            }
            "open:signin" => self.st.open(Some(Overlay::Other("signin"))),
            "devices:pair" => crate::features::companion::pair(),
            "devices:unpair" => crate::features::companion::unpair(),
            "tab:wallet" => self.st.set_tab(Tab::Wallet, false),
            "open:firstrun" => self.st.open(Some(Overlay::Other("firstrun"))),
            "open:share" => self.st.open(Some(Overlay::Other("share"))),
            "open:fan-curve" => self.st.open(Some(Overlay::Other(crate::features::fancurve::KIND))),
            "open:game-buttons" => self.st.open(Some(Overlay::Other(crate::features::gamebuttons::KIND))),
            "accounts:sign-out" => {
                crate::screens::wallet::page::open_sign_out();
                self.st.set_tab(Tab::Wallet, false);
            }
            // Settings > Accounts, per account: its rare.market sign-in, and which one is main.
            a if a.starts_with("accounts:signout:") => {
                let addr = a["accounts:signout:".len()..].to_string();
                match self.social.clone() {
                    Some(s) => s.sign_out_account(addr),
                    None => self.st.toast("Signing out needs the live launcher.", Tone::Info, now),
                }
            }
            a if a.starts_with("accounts:main:") => {
                let addr = a["accounts:main:".len()..].to_string();
                let _ = crate::screens::wallet::service::work(move |w| {
                    if let Err(e) = w.keystore.select(&addr) {
                        log::warn!("main account: {e}");
                    }
                    w.bump();
                    live::touch();
                });
                self.st.toast("Main account changed", Tone::Info, now);
            }
            "accounts:signin" => {
                if let Some(s) = self.social.clone() {
                    let _ = s.act("friends:turn-on");
                    self.st.set_tab(Tab::Friends, false);
                }
            }
            "accounts:pin:change" => crate::screens::wallet::service::ask_pin_task(crate::screens::wallet::sign::pin_tasks::Task::Change),
            "accounts:pin:words" => crate::screens::wallet::service::ask_pin_task(crate::screens::wallet::sign::pin_tasks::Task::Words),
            "accounts:send-pin:set" => ask(&mut self.st, "New PIN for big sends", "6 or more digits", true, "send-pin".into(), None),
            "accounts:send-pin:off" => ask(&mut self.st, "Enter your PIN to turn it off", "Your PIN", true, "send-pin-off".into(), None),
            // Steam signs in (and out, or switches) in its own big-screen mode; the Rare button comes back.
            "accounts:steam" => self.open_steam(now),
            "network:portal" => self.portal_sign_in(now),
            "accounts:pick-username" => {
                if self.social.is_none() {
                    self.st.toast("Usernames need the live launcher.", Tone::Info, now);
                } else {
                    // Settings' reading when it is loaded, else what Rare already knows (the profile's
                    // Change username opens this without Settings ever loading).
                    let current = live::get()
                        .and_then(|s| s.profile.username)
                        .or_else(|| self.st.live.profile.as_ref().and_then(|p| p.username.clone()))
                        .or_else(|| self.st.account.as_ref().and_then(|a| a.username.clone().or_else(|| crate::data::account::cached_username(&a.address))));
                    ask(&mut self.st, rare_friends::pick_username::TITLE, rare_friends::pick_username::PLACEHOLDER, false, "username".into(), current);
                }
            }
            "os:restart-open" | "os:restore-open" => self.open_os_sheet(a == "os:restart-open"),
            "support:tz-open" | "support:reset-open" => self.support_open(a),
            "ssh:ask-password" => ask(&mut self.st, crate::screens::settings::remote::ask::PASSWORD_TITLE, "At least 8 characters", true, "ssh-password".into(), None),
            "ssh:ask-key" => {
                // A key on the clipboard is filled in, so pasting from a laptop is one press.
                let clip = rare_system::ssh::clipboard_text().filter(|t| !rare_system::ssh::parse_keys(t).is_empty()).map(|t| rare_system::ssh::parse_keys(&t).join("\n"));
                ask(&mut self.st, crate::screens::settings::remote::ask::KEY_TITLE, "ssh-ed25519 AAAA...", false, "ssh-key".into(), clip);
            }
            "ssh:ask-github" => ask(&mut self.st, crate::screens::settings::remote::ask::GITHUB_TITLE, "octocat", false, "ssh-github".into(), None),
            "streamhost:ask-pin" => ask(&mut self.st, "The PIN Moonlight shows", "4 digits", false, "streamhost-pin".into(), None),
            _ if ["capture:", "discord:", "streamhost:", "nearby:"].iter().any(|p| a.starts_with(p)) => {
                if let Some((msg, err)) = crate::features::capture::act(a, None).filter(|m| !m.0.is_empty()) {
                    self.st.toast(&msg, if err { Tone::Error } else { Tone::Info }, now);
                }
            }
            _ if a.starts_with("parental:") => crate::screens::settings::parental::act(&mut self.st, a, now),
            _ if a.starts_with("settings:goto:") => command(self, "goto", &a["settings:goto:".len()..]),
            _ if a.starts_with("toast:error:") => self.st.toast(&a["toast:error:".len()..], Tone::Error, now),
            _ if a.starts_with("developer:endpoint:") => {
                let field = &a["developer:endpoint:".len()..];
                let title = match field {
                    "testnetRpc" => "Testnet RPC address",
                    "testnetApi" => "Testnet market API address",
                    _ => "Testnet gas sponsor address",
                };
                let current = match field {
                    "testnetRpc" => cfg.testnet_rpc.clone(),
                    "testnetApi" => cfg.testnet_api.clone(),
                    _ => cfg.testnet_sponsor.clone(),
                };
                ask(&mut self.st, title, "https://", false, format!("endpoint-{field}"), current);
            }
            _ if a.starts_with("network:join:1:") => {
                let ssid = &a["network:join:1:".len()..];
                ask(&mut self.st, &format!("Password for {ssid}"), "", true, format!("wifi:{ssid}"), None);
            }
            _ if a.starts_with("stores:") => return crate::features::stores::act(&mut self.st, a, now),
            _ if a.starts_with("open-web:") => self.run(Act::OpenWeb(a["open-web:".len()..].to_string())),
            // Fixtures (shots, the crawler) run no system commands: say what would run instead of
            // nothing, so a device-only control is not mistaken for a dead one.
            _ if self.fixture && live::owns(a) => self.st.toast("Runs on the device (fixture).", Tone::Info, now),
            _ if self.fixture => return false,
            _ => return live::run(a, arg, self.waker.clone()) == live::Ran::Started,
        }
        true
    }

    /// The folder picker for "Add folder" (Settings, Storage): the launcher's one picker
    /// (features/gamemgmt/picker.rs) in its folder mode.
    fn open_folder_pick(&mut self) {
        crate::features::gamemgmt::picker::open(&mut self.st, crate::features::gamemgmt::picker::Mode::Folder);
        self.fx.fb(Feedback::Open);
    }

    /// Settings, Storage: moves, the SD card, the list. False when it is not one of these.
    fn storage_act(&mut self, a: &str) -> bool {
        use crate::features::storage as fst;
        let now = clock::secs();
        let w = self.waker.clone();
        let rest = |p: &str| a.strip_prefix(p).map(str::to_string);
        if let Some(id) = rest("storage:move-game:") {
            // The game page's Move: Storage's move for that game, once its sizes are known.
            if !self.st.running.is_empty() {
                self.st.toast("Close the running game first, then move it.", Tone::Info, now);
            } else if fst::snapshot().items.is_some() {
                fst::start_move(&w, &id);
            } else {
                self.st.toast("Finding where it can go", Tone::Info, now);
                self.pending_move = Some(id);
                fst::measure(&w, now);
            }
        } else if let Some(t) = rest("storage:say:") {
            self.st.toast(t, Tone::Info, now);
        } else if let Some(k) = rest("storage:move:") {
            if !self.st.running.is_empty() {
                self.st.toast("Close the running game first, then move it.", Tone::Info, now);
            } else {
                fst::start_move(&w, &k);
            }
        } else if let Some(id) = rest("storage:uninstall:") {
            match self.st.game(&id) {
                Some(g) if crate::features::uninstall::can_uninstall(g) => crate::features::uninstall::open(&mut self.st, &id),
                Some(_) => self.st.toast("Steam owns this one. Remove it in Steam.", Tone::Info, now),
                None => self.st.toast("Rare is still reading this one. Try again in a moment.", Tone::Info, now),
            }
        } else if let Some(p) = rest("storage:delete-ask:") {
            let size = fst::snapshot().items.and_then(|v| v.into_iter().find(|i| i.key == format!("orphan:{p}"))).map(|i| crate::screens::settings::storage::size_text(i.bytes)).unwrap_or_default();
            let ask = crate::screens::settings::live::Ask {
                title: "Delete these leftover files?".into(),
                sub: size,
                body: "A game you removed left its Windows saves and settings here. If you reinstall it, saves that aren't in the cloud are gone.\nThis can't be undone.".into(),
                yes: "Delete".into(),
                act: format!("storage:delete:{p}"),
            };
            self.open_confirm_sheet(&ask);
        } else if let Some(p) = rest("storage:delete:") {
            fst::delete_prefix(&w, &p);
        } else if let Some(k) = rest("storage:clear:") {
            fst::clear_cache(&w, &k);
        } else if let Some(path) = rest("storage:folder:") {
            if let Err(e) = std::fs::create_dir_all(&path) {
                self.st.toast(format!("Couldn't make {path}: {e}"), Tone::Error, now);
                return true;
            }
            if let Some((_, screen, _)) = self.legacy_page.as_mut() {
                let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
                screen.command(&mut cx, "add-folder", &path, now);
            }
            self.run(Act::Scan);
        } else {
            match a {
                "storage:measure" => fst::measure(&w, now),
                "storage:redraw" => fst::redraw(),
                "storage:cancel" => fst::cancel(),
                "storage:setup" => {
                    if self.st.running.iter().any(|r| r.starts_with("steam:")) {
                        self.st.toast("Close the Steam game first. Steam restarts to add the card.", Tone::Info, now);
                    } else {
                        fst::start_set_up(&w);
                    }
                }
                "storage:format-ask" => {
                    let size = match fst::snapshot().sd {
                        rare_library::storage::SdCard::NotReady { size, .. } => crate::screens::settings::storage::size_text(size),
                        _ => fst::snapshot().drives.iter().find(|d| d.kind == rare_library::storage::DriveKind::Sd).map(|d| crate::screens::settings::storage::size_text(d.total)).unwrap_or_default(),
                    };
                    let ask = crate::screens::settings::live::Ask {
                        title: "Erase and format the SD card?".into(),
                        sub: if size.is_empty() { String::new() } else { format!("{size} card") },
                        body: "Everything on the card is deleted, and this can't be undone.\nRare tests the card, formats it for games, then adds it to Steam and to your Games folders. It takes a minute or two. Keep the card in until it's done.".into(),
                        yes: "Erase and format".into(),
                        act: "storage:format".into(),
                    };
                    self.open_confirm_sheet(&ask);
                }
                "storage:format" => {
                    if self.st.running.iter().any(|r| r.starts_with("steam:")) {
                        self.st.toast("Close the Steam game first. Steam restarts to add the card.", Tone::Info, now);
                    } else {
                        fst::start_format(&w);
                    }
                }
                _ => return false,
            }
        }
        true
    }

    /// The SD card slot and storage workers: toasts, a rescan after a move, favourites that follow
    /// a moved game, a new Games folder, and Storage redrawn when its numbers move.
    fn storage_tick(&mut self, now: f64) {
        use crate::features::storage as fst;
        if self.fixture {
            // Fixtures still show what Storage's buttons say (Measure again).
            for (t, err) in fst::take_out().toasts {
                self.st.toast(t, if err { Tone::Error } else { Tone::Info }, now);
            }
            return;
        }
        let redraw = fst::app_tick(&self.waker.clone(), now, self.st.ver.library);
        if self.pending_move.is_some() && fst::snapshot().items.is_some() {
            if let Some(id) = self.pending_move.take() {
                fst::start_move(&self.waker.clone(), &id);
            }
        }
        let o = fst::take_out();
        for (t, err) in o.toasts {
            self.st.toast(t, if err { Tone::Error } else { Tone::Info }, now);
        }
        if !o.renames.is_empty() {
            for (old, new) in &o.renames {
                for f in self.st.favs.iter_mut().filter(|f| *f == old) {
                    *f = new.clone();
                }
            }
            save_favs(&self.st.favs);
            self.st.ver.favs += 1;
        }
        for f in &o.folders {
            if let Some((_, screen, _)) = self.legacy_page.as_mut() {
                let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
                screen.command(&mut cx, "add-folder", f, now);
            } else {
                use crate::screens::settings::config;
                let old = config::load();
                let mut cfg = old.clone();
                if !cfg.game_folders.contains(f) {
                    cfg.game_folders.push(f.clone());
                    let _ = config::save_diff(&old, &cfg);
                }
            }
        }
        if o.rescan {
            self.run(Act::Scan);
        }
        if redraw && self.st.tab == Tab::Settings {
            if let Some((_, screen, _)) = self.legacy_page.as_mut() {
                let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
                screen.command(&mut cx, "refresh", "", now);
            }
        }
    }

    /// The restart and restore confirmations (OsSheets.tsx), over the page.
    fn open_os_sheet(&mut self, restart: bool) {
        use crate::screens::settings::{live, ossheets::OsSheet, update::OsUpdate};
        let now = clock::secs();
        let snap = live::get();
        let update = snap.as_ref().map(|s| s.system.update.clone()).unwrap_or_else(OsUpdate::default);
        let running = !self.st.running.is_empty();
        let root = self.scene.root;
        let before = self.scene.children(root);
        let screen: Box<dyn crate::screens::overlay::Screen> = {
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            if restart {
                Box::new(OsSheet::restart(&mut cx, &update, running, false, now))
            } else {
                Box::new(OsSheet::restore(&mut cx, false, now))
            }
        };
        let nodes: Vec<Id> = self.scene.children(root).into_iter().filter(|n| !before.contains(n)).collect();
        for n in &nodes {
            self.scene.n(*n).z = layer::SHEET + 5;
        }
        self.fx.fb(Feedback::Open);
        self.legacy = Some((screen, nodes));
    }

    /// A question with one yes (remove an SSH key, add keys from GitHub), over the page.
    fn open_confirm_sheet(&mut self, a: &crate::screens::settings::live::Ask) {
        use crate::screens::settings::ossheets::OsSheet;
        let now = clock::secs();
        let root = self.scene.root;
        let before = self.scene.children(root);
        let screen: Box<dyn crate::screens::overlay::Screen> = {
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            Box::new(OsSheet::confirm(&mut cx, &a.title, &a.sub, &a.body, &a.yes, &a.act, now))
        };
        let nodes: Vec<Id> = self.scene.children(root).into_iter().filter(|n| !before.contains(n)).collect();
        for n in &nodes {
            self.scene.n(*n).z = layer::SHEET + 5;
        }
        self.fx.fb(Feedback::Open);
        self.legacy = Some((screen, nodes));
    }

    /// Settings: what the keyboard returned, readings that landed, messages from workers.
    fn settings_tick(&mut self, now: f64) {
        use crate::screens::settings::live;
        if let Some(o) = self.st.osk.as_mut() {
            if matches!(&o.purpose, OskFor::Settings(k) if k == "username") {
                let line = rare_friends::username_rules::username_problem(&o.value);
                if o.status != line {
                    o.status = line;
                }
            }
        }
        if let Some((key, val)) = self.st.settings_input.take() {
            let v = val.unwrap_or_default();
            let cancelled = v.is_empty();
            let command = |me: &mut App, cmd: &str, arg: &str| {
                if let Some((_, screen, _)) = me.legacy_page.as_mut() {
                    let mut cx = crate::screens::overlay::Cx { scene: &mut me.scene, t: &mut me.text, img: &mut me.images };
                    screen.command(&mut cx, cmd, arg, now);
                }
            };
            match key.as_str() {
                "query" => command(self, "query", &v),
                "folder" if !cancelled => {
                    command(self, "add-folder", &v);
                    self.run(Act::Scan);
                }
                "gridkey" => command(self, "grid-key", &v),
                "askmodel" if !cancelled => command(self, "ask-model", &v),
                "askurl" if !cancelled => command(self, "ask-url", &v),
                "askkey" if !cancelled => {
                    if let Some(k) = crate::screens::settings::config::load().agent_provider.as_deref().and_then(rare_agent::provider::Kind::from_id) {
                        crate::features::agent::save_key(k, Some(v));
                    }
                }
                k if k.starts_with("endpoint-") => command(self, k, &v),
                "username" if !cancelled => match rare_friends::username_rules::username_problem(&v) {
                    Some(problem) => self.st.toast(problem, Tone::Error, now),
                    None => {
                        if let Some(social) = self.social.clone() {
                            social.claim_username(v);
                        }
                    }
                },
                "ssh-password" if !cancelled => {
                    self.settings_act("ssh:password", Some(v));
                }
                // The send PIN: typed twice, then saved; or typed to turn it off.
                "send-pin" if !cancelled => {
                    if v.len() < 6 || !v.bytes().all(|b| b.is_ascii_digit()) {
                        self.st.toast("Use at least 6 digits", Tone::Error, now);
                    } else {
                        *SEND_PIN_FIRST.lock().unwrap() = Some(v);
                        self.st.osk = Some(OskReq { title: "Enter it again".into(), value: String::new(), placeholder: Some("The same digits".into()), secret: true, purpose: OskFor::Settings("send-pin-2".into()), status: None });
                    }
                }
                "send-pin-2" if !cancelled => {
                    let first = SEND_PIN_FIRST.lock().unwrap().take();
                    if first.as_deref() != Some(v.as_str()) {
                        self.st.toast("The PINs did not match. Try again.", Tone::Error, now);
                    } else {
                        let _ = crate::screens::wallet::service::work(move |w| {
                            match w.send_pin.set(&v, rare_wallet::send_pin::DEFAULT_THRESHOLD_USD) {
                                Ok(()) => live::toast_pub("PIN for big sends is on", false),
                                Err(e) => live::toast_pub(rare_wallet::WalletError::parse(&e).message, true),
                            }
                            live::touch();
                        });
                    }
                }
                "send-pin-off" if !cancelled => {
                    let _ = crate::screens::wallet::service::work(move |w| {
                        match w.send_pin.clear(&v) {
                            Ok(()) => live::toast_pub("PIN for big sends is off", false),
                            Err(e) => live::toast_pub(rare_wallet::WalletError::parse(&e).message, true),
                        }
                        live::touch();
                    });
                }
                "ssh-key" if !cancelled => {
                    self.settings_act("ssh:keys", Some(v));
                }
                "ssh-github" if !cancelled => {
                    self.settings_act("ssh:github", Some(v));
                }
                "streamhost-pin" if !cancelled => {
                    crate::features::capture::act("streamhost:pin", Some(v));
                }
                k if k.starts_with("wifi:") && !cancelled => {
                    let ssid = k["wifi:".len()..].to_string();
                    self.settings_act(&format!("network:join:0:{ssid}"), Some(v));
                }
                _ => {}
            }
        }
        for a in live::take_asks() {
            self.open_confirm_sheet(&a);
        }
        self.storage_tick(now);
        self.support_tick();
        if live::take_ssh_changed() {
            // The top bar's SSH icon reads the device again now, not in half a minute.
            self.next_device = 0.0;
        }
        // Results of Settings actions started anywhere (the Rare menu, Quick settings, a sheet): shown
        // now, not held until Settings is next opened.
        for (text, err) in live::take_toasts() {
            self.st.toast(text, if err { Tone::Error } else { Tone::Info }, now);
        }
        if self.st.tab != Tab::Settings || self.legacy_page.is_none() {
            return;
        }
        if let Some((_, screen, _)) = self.legacy_page.as_mut() {
            if let Some(m) = screen.take_message() {
                self.st.toast(m, Tone::Error, now);
            }
        }
        // Accounts' Epic and GOG rows follow the stores too.
        let v = (live::version() + crate::features::parental::version() + crate::features::companion::version() + crate::features::devicepass::version()) ^ (rare_stores::accounts_version() << 32);
        if v != self.settings_ver {
            self.settings_ver = v;
            if let Some((_, screen, _)) = self.legacy_page.as_mut() {
                let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
                screen.command(&mut cx, "refresh", "", now);
            }
        }
        if !self.fixture && now >= self.next_settings_poll {
            self.next_settings_poll = now + 5.0;
            live::poll(self.waker.clone());
        }
    }

    pub fn ask_search(&mut self) {
        self.st.search.clear();
        self.st.ver.search += 1;
        self.st.osk = Some(OskReq::new("Search games, Sui games, collections and coins", "", Some("Type a name"), OskFor::Search));
    }

    fn default_pad(&mut self, b: Btn, repeat: bool) {
        let now = clock::secs();
        let dir = match b {
            Btn::Up => Some(Dir::Up),
            Btn::Down => Some(Dir::Down),
            Btn::Left => Some(Dir::Left),
            Btn::Right => Some(Dir::Right),
            _ => None,
        };
        if let Some(d) = dir {
            if self.nav.move_dir(d, now) {
                self.fx.fb(Feedback::Move);
            }
            return;
        }
        match b {
            Btn::Lt | Btn::Rt => {
                if self.nav.jump(b == Btn::Rt, now) {
                    self.fx.fb(Feedback::Move);
                }
            }
            Btn::A if !repeat => {
                let Some(id) = self.nav.activate(now) else { return };
                let Some(it) = self.nav.item(id).cloned() else { return };
                self.fx.fb(Feedback::Select);
                if it.scope == SCOPE_PAGE && it.tag == shell::DL_TAG {
                    self.run(Act::Open(Overlay::Other(crate::features::downloads::KIND)));
                    return;
                }
                if it.scope == SCOPE_PAGE && it.tag == shell::CHIP_TAG {
                    crate::screens::sui::profile_page::OPEN_PICKER.store(true, std::sync::atomic::Ordering::Relaxed);
                    self.st.open(Some(Overlay::Other("profile")));
                    return;
                }
                let act = self.slot_of(it.scope).and_then(|s| s.acts.get(it.tag).cloned());
                match act {
                    Some(Act::Local(code)) => {
                        let (st, fx) = (&mut self.st, &mut self.fx);
                        let slot = match it.scope {
                            SCOPE_PAGE => Some(&mut self.page),
                            SCOPE_OVERLAY => self.overlay.as_mut().or(Some(&mut self.page)),
                            SCOPE_OPTIONS => self.options.as_mut(),
                            SCOPE_OSK => self.osk.as_mut(),
                            SCOPE_REVIEW => self.review.as_mut(),
                            _ => self.overlay.as_mut().or(Some(&mut self.page)),
                        };
                        if let Some(s) = slot {
                            s.page.act(st, fx, code);
                        }
                    }
                    Some(a) => self.run(a),
                    None => {}
                }
            }
            _ => {}
        }
    }

    /// The shared acts.
    pub fn run(&mut self, a: Act) {
        let now = clock::secs();
        if self.journey.probe.on() {
            self.journey_act(&format!("{a:?}"));
        }
        match a {
            Act::None => {}
            // A View list's choice: the screen under the sheet handles it.
            Act::Local(code) => match self.overlay.as_mut() {
                Some(o) => o.page.act(&mut self.st, &mut self.fx, code),
                None => self.page.page.act(&mut self.st, &mut self.fx, code),
            },
            Act::HomeFriends(a) => self.home_friends_act(a),
            Act::Launch(id) => self.launch(&id),
            Act::PcSheet(uuid, app) => self.open_pc_sheet(uuid, app, now),
            Act::Market(target) => {
                self.fx.fb(Feedback::Open);
                self.show_market(target);
            }
            Act::SuiGame(slug) => {
                self.fx.fb(Feedback::Open);
                crate::screens::sui::page::open_game(slug);
                self.show_tab(Tab::Games);
            }
            Act::PcSetup => self.open_stream_sheet(crate::screens::stream::SheetKind::Setup, now),
            Act::StreamPlay(uuid, app) => {
                self.fx.fb(Feedback::Launch);
                self.fx.spawn(move |b| {
                    let r = b.stream_play(&uuid, app);
                    Box::new(move |st: &mut State, now| match r {
                        Ok(()) => {}
                        Err(e) if e == crate::data::backend::NOT_WIRED => st.toast("PC streaming isn't set up on this device yet.", Tone::Info, now),
                        Err(e) => st.toast(e, Tone::Error, now),
                    })
                });
            }
            Act::Retry(id) => {
                self.st.close();
                if !self.st.is_running(&id) {
                    self.launch(&id);
                    return;
                }
                // Close what is left of the run (a start with no window keeps running), wait for it
                // to be gone (Quit gives a program a few seconds), then start it again.
                self.fx.spawn(move |b| {
                    let _ = b.quit(&id);
                    for _ in 0..40 {
                        if !b.running().contains(&id) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(250));
                    }
                    let running = b.running();
                    Box::new(move |st: &mut State, _| {
                        st.set_running(running);
                        st.retry = Some(id);
                    })
                });
            }
            Act::OpenGame(id) => {
                if id.starts_with("steam:") {
                    // "When I pick a game": Steam warms up while the page is open.
                    self.fx.spawn(|b| {
                        if crate::data::backend::steam_mode() == "focus" {
                            b.steam_prewarm();
                        }
                        Box::new(|_: &mut State, _| {})
                    });
                }
                self.fx.fb(Feedback::Open);
                self.st.open(Some(Overlay::Game(id)));
            }
            Act::Tab(t) => {
                self.st.close();
                self.st.set_tab(t, false);
            }
            Act::Open(o) => self.st.open(Some(o)),
            Act::Legacy(a) => self.legacy_act(&a),
            a @ (Act::Install(_) | Act::InstallTo(..) | Act::Download(..)) => crate::features::downloads::run(&mut self.st, &mut self.fx, a, now),
            Act::OpenApp(id) => self.open_app(&id, false),
            Act::Close => self.st.close(),
            Act::Trophies(appid) => {
                crate::features::trophies::set_open(appid);
                self.st.open(Some(Overlay::Other("trophies")));
            }
            Act::Toast(t) => self.st.toast(t, Tone::Info, now),
            Act::ToggleFav(id) => {
                let on = self.st.toggle_fav(&id);
                self.st.toast(if on { "Added to favourites" } else { "Removed from favourites" }, Tone::Info, now);
            }
            Act::Collections(id) => {
                if let Some(spec) = crate::screens::shell::options::collection_options(&self.st, &id) {
                    self.fx.fb(Feedback::Open);
                    self.st.options = Some(spec);
                }
            }
            Act::CollectionToggle(name, id) => {
                let on = self.st.toggle_in(&name, &id);
                self.st.user_changed();
                self.st.toast(if on { format!("Added to {name}") } else { format!("Removed from {name}") }, Tone::Info, now);
            }
            Act::NewCollection(game) => crate::state::ask_collection(&mut self.st, None, game),
            Act::ToggleHidden(id) => {
                let hidden = self.st.toggle_hidden(&id);
                self.st.user_changed();
                let name = self.st.run_name(&id);
                self.st.toast(if hidden { format!("{name} is hidden. Find it under Hidden in the Library.") } else { format!("{name} is back in your library") }, Tone::Info, now);
                if hidden && matches!(&self.st.overlay, Some(Overlay::Game(o)) if self.st.copy_ids(&id).contains(o)) {
                    self.st.close();
                }
            }
            a @ (Act::SameGame(_) | Act::SameGameFind(..) | Act::MergeGames(..) | Act::NotSame(_) | Act::SplitCopy(_)) => crate::features::copies::run(&mut self.st, &mut self.fx, a, now),
            Act::QuitGame(id) => {
                if self.st.is_running(&id) {
                    self.fx.fb(Feedback::Open);
                    self.st.options = Some(crate::screens::shell::options::quit_options(&self.st, &id));
                }
            }
            Act::QuitNow(id) => {
                let name = self.st.run_name(&id);
                self.fx.fb(Feedback::Back);
                self.st.toast(format!("Closing {name}"), Tone::Info, now);
                self.fx.spawn(move |b| {
                    let r = b.quit(&id);
                    let running = b.running();
                    Box::new(move |st: &mut State, now| {
                        st.set_running(running);
                        if let Err(e) = r {
                            log::warn!("quit {id}: {e}");
                            st.toast(format!("Couldn't close {name}."), Tone::Error, now);
                        }
                    })
                });
            }
            Act::SteamBigPicture | Act::SteamGamePage(_) | Act::SteamStore(_) => {
                let (page, store) = match a {
                    Act::SteamGamePage(appid) => (Some(appid), false),
                    Act::SteamStore(appid) => (Some(appid), true),
                    _ => (None, false),
                };
                self.st.handoff_until = Some(now + 3.0);
                self.fx.spawn(move |b| {
                    let r = match page {
                        Some(a) if store => b.steam_store(a),
                        Some(a) => b.steam_game_page(a),
                        None => b.steam_big_picture(),
                    };
                    Box::new(move |st: &mut State, now| {
                        if let Err(e) = r {
                            log::warn!("steam: {e}");
                            st.handoff_until = None;
                            st.toast("Couldn't open Steam. Try again.", Tone::Error, now);
                        }
                    })
                });
            }
            Act::SteamAddLibrary(path) => self.fx.spawn(move |b| {
                let r = b.steam_add_library(&path);
                let lib = r.as_ref().ok().and_then(|_| b.scan().ok());
                Box::new(move |st: &mut State, now| match r {
                    Ok(()) => {
                        if let Some(l) = lib {
                            st.set_library(l);
                        }
                        st.toast("Added to Steam.", Tone::Info, now)
                    }
                    Err(e) => {
                        log::warn!("steam library: {e}");
                        st.toast(crate::screens::settings::live::failed("Couldn't add the folder to Steam.", &e, None), Tone::Error, now)
                    }
                })
            }),
            Act::Uninstall(id) => crate::features::uninstall::open(&mut self.st, &id),
            Act::AddWebGame { id, title, url, cover, hero } => {
                let r = crate::features::webgame::add(id.as_deref(), &title, &url, cover.as_deref(), hero.as_deref());
                match r {
                    Ok(_) => {
                        // The scan puts it in the library; the toast says so when it lands.
                        self.st.scanning = true;
                        self.fx.spawn(move |b| {
                            let r = b.scan();
                            Box::new(move |st: &mut State, now| {
                                st.scanning = false;
                                match r {
                                    Ok(l) => {
                                        st.set_library(l);
                                        st.toast(format!("{title} is in your library"), Tone::Info, now);
                                    }
                                    Err(_) => st.toast("Couldn't read your library. Scan again from the Library tab.", Tone::Error, now),
                                }
                            })
                        });
                    }
                    Err(e) => self.st.toast(crate::screens::settings::live::failed("Couldn't add the web game.", &e, None), Tone::Error, now),
                }
            }
            Act::OpenWeb(url) => {
                if let Err(e) = crate::features::webgame::open_external(&url) {
                    log::warn!("open web {url}: {e}");
                    self.st.toast("Couldn't open that page. Try again.", Tone::Error, now);
                }
            }
            Act::PlayWeb { id, title, url } => {
                let starting = !self.st.is_running(&id);
                self.fx.fb(Feedback::Launch);
                self.fx.spawn(move |b| {
                    // Running already: Play resumes it.
                    let r = if starting { b.play_web(&id, &title, &url) } else { b.launch(&id) };
                    let running = b.running();
                    Box::new(move |st: &mut State, now| match r {
                        Ok(()) => {
                            if starting {
                                st.toast(format!("Starting {title}"), Tone::Info, now);
                            }
                            st.set_running(running);
                        }
                        Err(e) => {
                            log::warn!("play web {id}: {e}");
                            st.toast(format!("{title} didn't start. Try again."), Tone::Error, now);
                        }
                    })
                });
            }
            Act::Scan => {
                self.st.scanning = true;
                self.fx.spawn(|b| {
                    let r = b.scan();
                    Box::new(move |st: &mut State, now| {
                        st.scanning = false;
                        match r {
                            Ok(l) => {
                                // Scan again says what it found, also when nothing changed.
                                let before = st.games().len();
                                st.set_library(l);
                                let found = st.games().len().saturating_sub(before);
                                st.toast(
                                    match found {
                                        0 => "Scan finished. No new games.".to_string(),
                                        1 => "Scan finished. 1 new game.".to_string(),
                                        n => format!("Scan finished. {n} new games."),
                                    },
                                    Tone::Info,
                                    now,
                                );
                            }
                            Err(_) => st.toast("Couldn't read your library. Scan again from the Library tab.", Tone::Error, now),
                        }
                    })
                });
            }
        }
    }

    /// The device woke up (lib/sleep.ts onResume): nothing on screen is cleared; the library,
    /// the account and an open Settings page read again. Live Sui data sees the gap itself
    /// (data/live.rs), the power settings come back through system::keep.
    fn on_resume(&mut self, now: f64) {
        crate::features::services::on_resume();
        self.quiet_scan();
        self.next_poll = now;
        self.next_account = now;
        self.next_settings_poll = now;
    }

    /// A rescan with no loading bar and no error toast (a game closed, a games folder changed).
    fn quiet_scan(&mut self) {
        self.fx.spawn(|b| {
            let r = b.scan();
            Box::new(move |st: &mut State, _| {
                if let Ok(l) = r {
                    st.set_library(l);
                }
            })
        });
    }

    /// store.ts launch: resume when it runs, else start it and say so.
    pub fn launch(&mut self, id: &str) {
        let now = clock::secs();
        // An app's failure sheet: Try again opens it the same way.
        if id.starts_with("app:") && crate::features::apps::retry(id) {
            return;
        }
        let Some(g) = self.st.game(id).cloned() else { return };
        // An Epic or GOG game that is not installed yet: the install sheet instead.
        if !g.installed && crate::screens::game::stores::managed(&g) {
            crate::features::stores::act(&mut self.st, &format!("stores:install:{id}"), now);
            return;
        }
        if self.st.is_running(id) {
            // Resume: the game's window comes back to the front.
            let id = id.to_string();
            self.fx.spawn(move |b| {
                let r = b.launch(&id);
                Box::new(move |st: &mut State, now| {
                    if let Err(e) = r {
                        st.toast(e, Tone::Error, now);
                    }
                })
            });
            return;
        }
        // One game at a time: another game running asks first (apps and PC streams may run beside one).
        if is_game_run(id) {
            if let Some(old) = self.st.running.iter().find(|r| is_game_run(r) && r.as_str() != id).cloned() {
                self.ask_close_and_play(&old, id);
                return;
            }
        }
        // Parental controls: over the day's limit, outside the hours or above the rating, the
        // PIN sheet comes up and the launch runs again once it is in.
        if crate::features::parental::gate_launch(&self.st, id, now) {
            return;
        }
        // Cloud saves: the newest save comes down first (or the player picks one); the launch
        // runs again once that is done.
        if !crate::features::cloudsync::before_launch(&mut self.st, &g) {
            return;
        }
        self.st.launching = Some(id.to_string());
        self.launching_until = now + 1.5;
        self.fx.fb(Feedback::Launch);
        let id = id.to_string();
        self.fx.spawn(move |b| {
            let r = b.launch(&id);
            let running = b.running();
            Box::new(move |st: &mut State, now| match r {
                Ok(()) => {
                    st.toast(format!("Starting {}", g.title), Tone::Info, now);
                    for r in &running {
                        st.starts.entry(r.clone()).or_insert(Start { phase: Phase::Steam, since: now });
                    }
                    st.ver.starts += 1;
                    st.set_running(running);
                    exit_tip_once(st, now);
                }
                Err(e) if e == crate::data::backend::NOT_WIRED => st.toast("Web games can't start on this device yet.", Tone::Info, now),
                Err(e) => {
                    log::warn!("launch {}: {e}", g.id);
                    st.toast(format!("{} didn't start. Try again.", g.title), Tone::Error, now);
                }
            })
        });
    }

    /// "Close <running> and play <new>?" over the page: Close and play, or keep playing.
    fn ask_close_and_play(&mut self, old: &str, new: &str) {
        use crate::screens::settings::ossheets::OsSheet;
        let now = clock::secs();
        let (a, b) = (self.st.run_name(old), self.st.run_name(new));
        let root = self.scene.root;
        let before = self.scene.children(root);
        let screen: Box<dyn crate::screens::overlay::Screen> = {
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            let body = format!("One game runs at a time. {a} closes first, the way its own Quit does; progress it has not saved is lost.");
            Box::new(OsSheet::confirm_with(&mut cx, &format!("Close {a} and play {b}?"), "", &body, "Close and play", &format!("Keep playing {a}"), &format!("close-and-play:{old}\u{1f}{new}"), now))
        };
        let nodes: Vec<Id> = self.scene.children(root).into_iter().filter(|n| !before.contains(n)).collect();
        for n in &nodes {
            self.scene.n(*n).z = layer::SHEET + 5;
        }
        self.fx.fb(Feedback::Open);
        self.legacy = Some((screen, nodes));
    }

    /// Close and play: the running game closes (Steam's own exit for Steam games, TERM then KILL
    /// for the rest), then the new one starts once the old one is gone.
    fn close_and_play(&mut self, old: String, new: String) {
        let now = clock::secs();
        let name = self.st.run_name(&old);
        self.st.toast(format!("Closing {name}"), Tone::Info, now);
        self.fx.spawn(move |b| {
            let r = b.quit(&old);
            // Wait for it to be gone (Steam and emulators take a few seconds to save and exit).
            let t0 = std::time::Instant::now();
            while r.is_ok() && b.running().iter().any(|x| *x == old) && t0.elapsed() < std::time::Duration::from_secs(15) {
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            let gone = !b.running().iter().any(|x| *x == old);
            let running = b.running();
            Box::new(move |st: &mut State, now| {
                st.set_running(running);
                if gone {
                    st.acts.push(Act::Launch(new));
                } else {
                    log::warn!("close and play: {old} still running ({r:?})");
                    st.toast(format!("Couldn't close {name}. Close it from the Rare menu, then play."), Tone::Error, now);
                }
            })
        });
    }

    fn after_input(&mut self) {
        let mut acts = std::mem::take(&mut self.fx.acts);
        acts.append(&mut self.st.acts);
        for a in acts {
            self.run(a);
        }
        self.flush_jobs();
    }

    fn flush_jobs(&mut self) {
        self.journey.jobs += self.fx.jobs.len() as u64;
        for job in self.fx.jobs.drain(..) {
            let be = self.fx.backend.clone();
            let tx = self.done_tx.clone();
            let waker = self.waker.clone();
            std::thread::Builder::new()
                .name("rr-job".into())
                .spawn(move || {
                    let done = job(be.as_ref());
                    let _ = tx.send(done);
                    waker();
                })
                .ok();
        }
        self.play_feedback(clock::secs());
    }

    // ------------------------------------------------------------------ update

    /// Applies results and timers, brings every view in line with the state, lays out, settles
    /// focus and the hints. Returns the next time a frame is needed.
    pub fn update(&mut self) -> Option<f64> {
        let now = clock::secs();
        let review_was = crate::screens::wallet::sign::host::active();
        while let Ok(done) = self.done_rx.try_recv() {
            done(&mut self.st, now);
        }
        // Acts a finished job queued (Close and play's launch).
        if !self.st.acts.is_empty() {
            for a in std::mem::take(&mut self.st.acts) {
                self.run(a);
            }
            self.flush_jobs();
        }
        crate::features::tick_state(&mut self.st);
        // Search results follow the keyboard as it types (B restores the term it opened on).
        if let Some(o) = self.st.osk.as_ref().filter(|o| matches!(o.purpose, OskFor::Search)) {
            if self.st.search_before.is_none() {
                self.st.search_before = Some(self.st.search.clone());
            }
            if o.value != self.st.search {
                self.st.search = o.value.clone();
                self.st.ver.search += 1;
            }
        } else {
            self.st.search_before = None;
        }
        for ev in crate::features::bus::drain() {
            if self.ingame_event(&ev, now) {
                continue;
            }
            if matches!(&ev, rare_core::Event::Other { name, .. } if name == "system-resume") {
                self.on_resume(now);
                continue;
            }
            on_event(&mut self.st, ev.clone(), now);
            crate::features::on_event(&mut self.st, &mut self.fx, ev, now);
        }
        if std::mem::take(&mut self.st.reopen_switcher) && self.switcher_up {
            self.open_switcher();
        }
        if let Some(id) = self.st.retry.take() {
            self.launch(&id);
        }
        if self.st.rescan_at.is_some_and(|t| t <= now) {
            self.st.rescan_at = None;
            self.quiet_scan();
        }
        if !self.st.running.is_empty() {
            self.st.played.heartbeat(crate::data::model::now_secs());
        }
        crate::features::poll(&mut self.st, &mut self.fx, now);
        if !self.forced_running.is_empty() && self.st.running != self.forced_running {
            let r = self.forced_running.clone();
            self.st.set_running(r);
        }
        self.timers(now);
        self.keep_update(now);
        self.portal_update(now);
        self.st.art_ready = self.arts.ready(&mut self.images, self.scene.rem, self.scene.w, &self.st.art_want);
        self.prefetch_art();
        let mut wake: Vec<Option<f64>> = vec![self.page_wake.take(), crate::features::tick(&self.st, now)];
        // Rare's play sessions are written down once a minute while a game runs.
        wake.push((!self.st.running.is_empty()).then_some(now + 60.0));
        for scope in [SCOPE_PAGE, SCOPE_OVERLAY, SCOPE_OPTIONS, SCOPE_REVIEW, SCOPE_OSK] {
            let (st, fx) = (&mut self.st, &mut self.fx);
            let slot = match scope {
                SCOPE_PAGE => Some(&mut self.page),
                SCOPE_OVERLAY => self.overlay.as_mut(),
                SCOPE_OPTIONS => self.options.as_mut(),
                SCOPE_REVIEW => self.review.as_mut(),
                _ => self.osk.as_mut(),
            };
            if let Some(s) = slot {
                wake.push(s.page.tick(st, fx, now));
            }
        }
        self.settings_tick(now);
        if let Some((_, screen, _)) = self.legacy_page.as_mut() {
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            screen.tick(&mut cx, now);
            wake.push(screen.next_deadline());
        }
        if self.st.tab == Tab::Settings && !self.fixture {
            wake.push(Some(self.next_settings_poll.max(now + 0.5)));
        }
        if let Some((screen, _)) = self.legacy.as_mut() {
            let mut cx = crate::screens::overlay::Cx { scene: &mut self.scene, t: &mut self.text, img: &mut self.images };
            screen.tick(&mut cx, now);
            wake.push(screen.next_deadline());
        }
        self.legacy_messages(now);
        if self.legacy.as_ref().is_some_and(|(s, _)| s.finished()) {
            self.close_legacy();
        }
        self.playing_marker();
        self.social_update(now, &mut wake);
        crate::features::services::set_setup(matches!(self.st.overlay, Some(Overlay::Other("firstrun" | "signin"))));
        crate::features::services::deliver(&mut self.st, self.social.as_deref(), now);
        self.share_sync(now);
        self.share_tick(now, &mut wake);
        self.side_update(now);
        self.legacy_osk(now);
        self.agent_update(now);
        self.ingame_tick();
        self.set_surface();
        wake.push(self.journey_poll());
        // An answer the asking page did not take this round goes stale.
        self.st.osk_answer = None;
        self.market_acts(now);
        self.alerts_update();
        self.reconcile(now);
        self.build_all(now, false);
        // The review sheet came up (focus moves into it in `settle`) or went (back to where it was).
        match (review_was, crate::screens::wallet::sign::host::active()) {
            (false, true) => self.review_spot = Some(self.nav.spot()),
            (true, false) => {
                if let Some(sp) = self.review_spot.take() {
                    self.nav.restore_later(sp, now);
                }
            }
            _ => {}
        }
        self.settle(now);
        // The focused element changed something a page draws itself (Home's strip): rebuild once.
        if self.nav.take_changed() {
            self.hints_changed();
            let cur = self.nav.current_item().cloned();
            let mut again = false;
            for scope in [SCOPE_PAGE, SCOPE_OVERLAY, SCOPE_OPTIONS, SCOPE_REVIEW, SCOPE_OSK] {
                if let Some(s) = self.slot_mut(scope) {
                    let mine = cur.as_ref().filter(|c| c.scope == scope);
                    if s.page.focused(mine, &s.acts, now) {
                        s.view.invalidate();
                        again = true;
                    }
                }
            }
            if cur.as_ref().is_some_and(|c| c.scope == SCOPE_PAGE) && self.page.page.focus_moved(&mut self.st, now) {
                self.page.view.invalidate();
                again = true;
            }
            if again {
                self.build_all(now, false);
                self.settle(now);
            }
        }
        // Scroll glides.
        self.nav.tick(now);
        let scrolls = self.nav.take_scrolls();
        if !scrolls.is_empty() {
            let (scene, nav) = (&mut self.scene, &mut self.nav);
            self.page.view.apply_scrolls(scene, nav, &scrolls);
            for s in [self.overlay.as_ref(), self.options.as_ref(), self.review.as_ref(), self.osk.as_ref()].into_iter().flatten() {
                s.view.apply_scrolls(scene, nav, &scrolls);
            }
        }
        // The hint bar follows focus and the screens.
        self.build_footer(now);
        // Exits finished, art that failed over.
        let mut gone = false;
        each_view!(self, v => gone |= v.gc(&mut self.scene, now));
        let _ = gone;
        if self.arts.poll(&mut self.images) {
            wake.push(Some(now));
        }
        // Pages that are not ticking (under another page, on a tab not shown, closed) keep
        // failing over and keep their winning links remembered.
        crate::kit::art::poll_hidden(&mut self.images, &self.arts);
        if let Some(b) = self.boot.as_mut() {
            let ready = self.st.library.is_some();
            let (t, f) = b.tick(&mut self.scene, ready, now, self.animate || true);
            wake.push(t);
            if let Some(f) = f {
                self.fx.fb(f);
            }
            if b.phase == BootPhase::Gone {
                self.boot = None;
            }
        }
        wake.push(self.st.toasts.iter().map(|t| t.until).reduce(f64::min));
        wake.push(self.st.handoff_until);
        wake.push(self.st.rescan_at);
        if !self.st.running.is_empty() {
            wake.push(Some(self.next_progress));
        }
        wake.push(Some(self.next_poll));
        wake.push(self.nav.next_wake());
        wake.push(self.touch_deadline());
        for v in self.views() {
            wake.push(v.next_deadline());
        }
        self.after_input();
        self.journey_tick();
        wake.into_iter().flatten().reduce(f64::min)
    }

    fn views(&self) -> Vec<&View> {
        let mut v = vec![&self.top.view, &self.footer.view, &self.toasts.view, &self.backdrop.view, &self.handoff.view, &self.page.view];
        for s in [self.overlay.as_ref(), self.options.as_ref(), self.review.as_ref(), self.osk.as_ref()].into_iter().flatten() {
            v.push(&s.view);
        }
        v
    }

    fn hints_changed(&mut self) {}

    fn timers(&mut self, now: f64) {
        self.st.toasts.retain(|t| t.until > now);
        if self.st.handoff_until.is_some_and(|t| t <= now) {
            self.st.handoff_until = None;
        }
        if self.st.launching.is_some() && now >= self.launching_until {
            self.st.launching = None;
        }
        // Running games (the backend's game-state events; polled for the mock) and progress.
        if now >= self.next_poll {
            self.next_poll = now + 1.0;
            if self.forced_running.is_empty() {
                self.fx.spawn(|b| {
                    let r = b.running();
                    Box::new(move |st: &mut State, _| {
                        st.set_running(r);
                        let running = st.running.clone();
                        st.starts.retain(|id, _| running.contains(id));
                    })
                });
            }
        }
        if !self.st.running.is_empty() && now >= self.next_progress {
            self.next_progress = now + 0.7;
            let ids = self.st.running.clone();
            self.fx.spawn(move |b| {
                let phases: Vec<(String, Phase)> = ids.iter().map(|id| (id.clone(), b.launch_phase(id))).collect();
                Box::new(move |st: &mut State, now| {
                    let mut changed = false;
                    let mut said: Vec<&'static str> = Vec::new();
                    for (id, ph) in phases {
                        let e = st.starts.entry(id).or_insert(Start { phase: ph, since: now });
                        if e.phase != ph && e.phase != Phase::Playing {
                            if let Phase::Answer(q) = ph {
                                said.push(q);
                            }
                            e.phase = ph;
                            changed = true;
                        }
                    }
                    // Steam asks the player something: said here first, then Steam's dialog shows.
                    for q in said {
                        st.toast_for(q, Tone::Info, now, 6.0);
                    }
                    if changed {
                        st.ver.starts += 1;
                    }
                })
            });
        }
        // The wallet's current account changed (created, imported, signed in, picked, removed):
        // Home, the Rare menu and Friends follow now, its value and items right after.
        if !self.fixture {
            let wv = crate::screens::wallet::service::peek_version();
            if wv != self.wallet_seen {
                self.wallet_seen = wv;
                if let Some(a) = crate::data::account::follow(self.st.account.as_ref(), crate::data::account::current()) {
                    self.st.account = a;
                    self.st.ver.account += 1;
                    self.next_account = now;
                }
            }
        }
        // Home's account, its value and items: every 60 s while nothing runs (fixtures: none).
        if now >= self.next_account && !self.fixture {
            self.next_account = now + 60.0;
            if self.st.running.is_empty() {
                self.fx.spawn(|_| {
                    let a = crate::data::account::load();
                    Box::new(move |st: &mut State, _| {
                        let a = crate::data::account::merge(st.account.as_ref(), a);
                        if st.account != a {
                            st.account = a;
                            st.ver.account += 1;
                        }
                    })
                });
            }
        }
        // The paired PC's games: every 30 s while nothing runs (stream/state.ts startStream).
        if now >= self.next_pc {
            self.next_pc = now + 30.0;
            if self.st.running.is_empty() {
                self.fx.spawn(|b| {
                    let pc = b.pc_shelf();
                    Box::new(move |st: &mut State, _| {
                        if st.pc != pc {
                            st.pc = pc;
                            st.ver.pc += 1;
                        }
                    })
                });
            }
        }
        // A game started or ended: the menu's performance line and emulator rows are read again.
        self.game_perf_sync();
        if now >= self.next_device {
            self.next_device = now + 15.0;
            let running = self.st.running.clone();
            self.fx.spawn(move |b| {
                let d = b.device();
                let perf = crate::state::GamePerf {
                    perf: b.perf(),
                    power: if running.is_empty() { None } else { b.game_power() },
                    emu: running.iter().flat_map(|id| b.emu_actions(id)).collect(),
                    running,
                };
                Box::new(move |st: &mut State, _| {
                    if st.running == perf.running {
                        st.game_perf = perf;
                    }
                    if st.device != d {
                        st.device = d;
                        st.ver.device += 1;
                    }
                })
            });
        }
    }

    /// The Rare menu's Game tab parts for the running set (performance settings that apply,
    /// emulator rows), at once when the set changed; a worker reads the live values after.
    pub(crate) fn game_perf_sync(&mut self) {
        if self.st.running == self.st.game_perf.running || self.fixture {
            return;
        }
        {
            self.st.game_perf.running = self.st.running.clone();
            self.next_device = 0.0;
            // The Performance line has the settings that apply to the game from its first moment.
            if let Some(rid) = self.st.running.last().cloned() {
                let (perf, power) = crate::data::backend::planned_perf(&rid, self.st.game_perf.perf.as_ref().or(self.st.qs.perf.as_ref()));
                self.st.game_perf.perf = perf;
                self.st.game_perf.power = power;
            }
            // Emulator rows too, so a Rare menu opened right away has them (a library lookup and
            // the emulator's config file; the device tick's worker reads them again).
            let b = self.fx.backend.clone();
            self.st.game_perf.emu = self.st.running.iter().flat_map(|id| b.emu_actions(id)).collect();
            // A Steam game's achievements, ready for the Rare menu's Trophies tab.
            if self.st.running.iter().any(|r| r.starts_with("steam:")) {
                crate::features::trophies::refresh();
            }
        }
    }

    /// Opens and closes the slots to match the state.
    fn reconcile(&mut self, now: f64) {
        // The page: keyed by tab (a switch remounts it, App.tsx `key={shown}`).
        let what = format!("{:?}", self.st.tab);
        if self.page.what != what {
            if self.st.overlay.is_none() {
                if let (Some(sp), Some(t)) = (self.nav.spot(), Tab::parse(&self.page.what.to_lowercase())) {
                    self.tab_spots.insert(t, sp);
                }
            }
            let host = self.page.view.host();
            self.page.view.clear(&mut self.scene);
            self.page = Slot::new(what, host, make_page(self.st.tab), SCOPE_PAGE);
            self.nav.restore_later(self.tab_spots.get(&self.st.tab).cloned(), now);
            self.scene.n(self.backdrop_host).hidden = self.st.tab != Tab::Home;
            let tab = self.st.tab;
            self.mount_legacy_page(tab, now);
        }
        // The overlay.
        let want = self.st.overlay.as_ref().map(overlay_what);
        if want != self.overlay.as_ref().map(|s| s.what.clone()) {
            let had = self.overlay.is_some();
            if let Some(old) = self.overlay.take() {
                drop_slot(&mut self.scene, old);
            }
            match self.st.overlay.clone() {
                Some(o) => {
                    if !had {
                        self.overlay_spot = self.nav.spot();
                    }
                    let host = wrapper(&mut self.scene, overlay_layer(&o));
                    self.overlay = Some(Slot::new(overlay_what(&o), host, make_overlay(&o), SCOPE_OVERLAY));
                }
                None => self.nav.restore_later(self.overlay_spot.take(), now),
            }
        }
        // The Options sheet.
        let want = self.st.options.as_ref().map(|o| o.title.clone());
        if want != self.options.as_ref().map(|s| s.what.clone()) {
            if let Some(old) = self.options.take() {
                drop_slot(&mut self.scene, old);
            }
            if let Some(t) = want {
                // Its own layer: a layer lays its wrappers out side by side, so a sheet sharing
                // one with Quick settings would land beside it, off screen.
                let host = wrapper(&mut self.scene, layer::SHEET + 1);
                self.options = Some(Slot::new(t, host, Box::new(Options), SCOPE_OPTIONS));
            }
        }
        // The wallet's review sheet: mounted once, before the keyboard (which draws over it).
        if self.review.is_none() {
            let host = wrapper(&mut self.scene, layer::MODAL);
            self.review = Some(Slot::new("review".into(), host, Box::new(crate::screens::wallet::sign::host::ReviewHost::default()), SCOPE_REVIEW));
        }
        // The keyboard.
        let want = self.st.osk.as_ref().map(|o| o.title.clone());
        if want != self.osk.as_ref().map(|s| s.what.clone()) {
            if let Some(old) = self.osk.take() {
                drop_slot(&mut self.scene, old);
            }
            if let Some(t) = want {
                // Its own layer over the review sheet's: two wrappers in one layer sit side by side
                // and the keyboard landed off screen (invisible, with the focus in it).
                let host = wrapper(&mut self.scene, layer::MODAL + 1);
                self.osk = Some(Slot::new(t, host, Box::new(crate::screens::osk::Osk::default()), SCOPE_OSK));
            }
        }
    }

    /// Builds every view whose stamp moved, then lays out once.
    fn build_all(&mut self, now: f64, force: bool) {
        let mut layout = false;
        let (scene, text, img, arts, st) = (&mut self.scene, &mut self.text, &mut self.images, &mut self.arts, &self.st);
        layout |= build_chrome(&mut self.top, scene, text, img, arts, hash((crate::data::live::PIC_VER.load(std::sync::atomic::Ordering::Relaxed), crate::features::firstrun::wallet_from_setup(), st.feat.downloads.ver, st.tab, st.ver.device, &self.clock, st.friends.as_ref().is_some_and(|f| f.st.badge() > 0), st.account.as_ref().map(|a| (&a.address, &a.label, &a.username, crate::data::account::cached_username(&a.address), a.total_usd.map(|v| v.to_bits()), a.change_usd.map(|v| v.to_bits()))), st.live.profile.as_ref().map(|p| (&p.name, &p.username, &p.suins, &p.avatar, p.total_usd.map(|v| v.to_bits()), p.change_usd.map(|v| v.to_bits()))))), now, |ui| shell::top_bar(ui, st, &self.clock));
        layout |= build_chrome(&mut self.backdrop, scene, text, img, arts, hash(&st.backdrop), now, |ui| shell::backdrop(ui, &st.backdrop));
        layout |= build_chrome(&mut self.toasts, scene, text, img, arts, hash((st.toasts.iter().map(|t| t.id).collect::<Vec<_>>(), crate::features::trophies::popup_stamp())), now, |ui| {
            let col = shell::toasts(ui, st);
            match crate::features::trophies::popup_el(ui, !st.running.is_empty()) {
                Some(p) => col.child(p),
                None => col,
            }
        });
        let handoff = st.handoff_until.is_some();
        layout |= build_chrome(&mut self.handoff, scene, text, img, arts, hash(handoff), now, |ui| if handoff { shell::handoff::handoff(ui) } else { ui.bx(s()) });
        // The page under a full-page overlay comes back with its loops running again.
        let covered = st.overlay.as_ref().is_some_and(|o| overlay_layer(o) == layer::PAGE);
        if self.page_covered && !covered {
            self.page.loops.clear();
            self.page.view.invalidate();
        }
        self.page_covered = covered;
        layout |= build_slot(&mut self.page, scene, text, img, arts, st, now, force);
        if covered {
            hold_loops(scene, &self.page.loops);
        }
        for slot in [self.overlay.as_mut(), self.options.as_mut(), self.review.as_mut(), self.osk.as_mut()].into_iter().flatten() {
            layout |= build_slot(slot, scene, text, img, arts, st, now, force);
        }
        // The loading bar under the top bar while the first scan runs.
        let on = if self.st.scanning { 1.0 } else { 0.0 };
        if self.scene.get(self.load_bar).opacity.target() != on {
            self.scene.n(self.load_bar).opacity.set(on, now, if self.animate { 0.2 } else { 0.0 });
            self.load_loop(on > 0.0, now);
        }
        if layout || self.scene.needs_layout() {
            self.scene.layout(&mut self.text);
        }
        each_view!(self, v => v.after_layout(&mut self.scene, now));
        each_view!(self, v => crate::kit::follow_tabs(v, &mut self.scene, now));
    }

    /// Starts or stops the loading bar's segment (nothing loops while it is hidden).
    fn load_loop(&mut self, on: bool, now: f64) {
        let Some(seg) = self.scene.children(self.load_bar).first().copied() else { return };
        let Some(strip) = self.scene.children(seg).first().copied() else { return };
        if on {
            let w = self.scene.w;
            let xs: &'static [(f32, f32)] = Box::leak(LOAD_X.frames.iter().map(|(t, v)| (*t, v * w)).collect::<Vec<_>>().into_boxed_slice());
            let kx: &'static Keys = Box::leak(Box::new(Keys { frames: xs, ease: LOAD_X.ease }));
            self.scene.n(seg).tx.start_loop(kx, 1.1, now);
            self.scene.n(strip).scale.start_loop(&LOAD_SX, 1.1, now);
        } else {
            self.scene.n(seg).tx.snap(0.0);
            self.scene.n(strip).scale.snap(1.0);
        }
    }

    /// Registers every focusable view with Nav, settles focus and draws it.
    fn settle(&mut self, now: f64) {
        self.nav.viewport = [self.scene.w, self.scene.h];
        self.nav.begin();
        self.page.view.register(&self.scene, &mut self.nav, now);
        // The top bar's profile chip sits in the page's scope (Up from the page's top row).
        // Not on a scene-built tab (Settings, Market, Friends): that page has its own focus, and
        // only one element may ever show focus.
        if self.legacy_page.is_none() {
            self.top.view.register(&self.scene, &mut self.nav, now);
        }
        for s in [self.overlay.as_ref(), self.options.as_ref(), self.review.as_ref(), self.osk.as_ref()].into_iter().flatten() {
            s.view.register(&self.scene, &mut self.nav, now);
        }
        self.nav.ensure(now);
        // A screen that wants focus somewhere now (a late default).
        for scope in [SCOPE_PAGE, SCOPE_OVERLAY, SCOPE_OPTIONS, SCOPE_REVIEW, SCOPE_OSK] {
            let cur = self.nav.current_item().cloned();
            let Some(s) = self.slot_mut(scope) else { continue };
            if let Some(tag) = s.page.take_focus(cur.as_ref().filter(|c| c.scope == scope)) {
                if let Some(id) = self.nav.items.iter().find(|i| i.scope == scope && i.tag == tag).map(|i| i.id) {
                    self.nav.focus(id, rare_render::focus::FocusOpts { scroll: true, smooth: false }, now);
                }
            }
        }
        let cur = self.nav.current();
        each_view!(self, v => v.set_focus(&mut self.scene, &mut self.text, cur, now));
    }

    /// The hint bar (Chrome.tsx HintBar) and the status cells.
    fn build_footer(&mut self, now: f64) {
        let st = &self.st;
        // First run keeps the tabs, Quick and Search shut: its page says what B and Y do.
        let setup = matches!(st.overlay, Some(Overlay::Other("firstrun")));
        let base: Vec<Hint> = match &st.overlay {
            _ if setup => Vec::new(),
            // Setup's account pages on the Wallet tab: no tabs or search there.
            None if crate::features::firstrun::wallet_from_setup() => vec![hint(Key::Quick, "Quick settings")],
            Some(_) => vec![hint(Key::LbRb, "Tabs")],
            None => vec![hint(Key::Y, "Search"), hint(Key::LbRb, "Tabs"), hint(Key::Quick, "Quick settings")],
        };
        let mut stack = Stack::default();
        stack.push(self.page.page.hints(st));
        if let Some((_, p, _)) = self.legacy_page.as_ref().filter(|_| st.overlay.is_none()) {
            stack.push(p.hints().into_iter().filter_map(|(k, l)| crate::screens::market::live::hint_key(k).map(|k| hint(k, &l))).collect());
        }
        if let Some(sh) = self.share.as_ref() {
            use crate::screens::market::host::Page;
            stack.push(sh.page.hints().into_iter().filter_map(|(k, l)| crate::screens::market::live::hint_key(k).map(|k| hint(k, &l))).collect());
        }
        for s in [self.overlay.as_ref(), self.options.as_ref(), self.review.as_ref()].into_iter().flatten() {
            let mut h = s.page.hints(st);
            // A sheet over it (Settings' time zone picker over setup) takes Y: the page under it
            // does not get to offer it ("Y Skip setup" did nothing there).
            if self.legacy.is_some() {
                h.retain(|x| !matches!(x.b, Key::Y | Key::X));
            }
            stack.push(h);
        }
        let can_back = !setup && (st.overlay.is_some() || !st.history.is_empty() || st.tab != Tab::Home);
        let mut over = if self.legacy.is_none() && st.overlay.is_none() && st.options.is_none() && st.osk.is_none() { self.legacy_page.as_ref().and_then(|(_, s, _)| s.bar()) } else { None };
        // Share keeps its own focus: the app's focused node (still the tile behind it) must not
        // put its A/X/Menu (Play, Details, Options) on the bar.
        if let Some(sh) = self.share.as_ref().filter(|_| !crate::screens::wallet::sign::host::active() && st.osk.is_none()) {
            use crate::screens::market::host::Page;
            let mut h: Vec<Hint> = sh.page.hints().into_iter().filter_map(|(k, l)| crate::screens::market::live::hint_key(k).map(|k| hint(k, &l))).collect();
            h.push(hint(Key::LbRb, "Tabs"));
            over = Some(h);
        }
        self.hints = compose(over.as_deref(), &base, &stack, &self.nav.focused_hints(), can_back);
        let hints = self.hints.clone();
        let stamp = hash((st.ver.live, st.ver.library, &st.running, hints.iter().map(|h| (h.b.name(), h.label.clone())).collect::<Vec<_>>()));
        let (scene, text, img, arts) = (&mut self.scene, &mut self.text, &mut self.images, &mut self.arts);
        if build_chrome(&mut self.footer, scene, text, img, arts, stamp, now, |ui| shell::footer(ui, st, &hints)) {
            self.scene.layout(&mut self.text);
            self.footer.view.after_layout(&mut self.scene, now);
        }
    }

    // ------------------------------------------------------------------ frame

    /// Image uploads and fades, the backdrop following Home's scroll. True while anything moves.
    pub fn frame(&mut self, gpu: &mut Gpu, now: f64) -> bool {
        self.images.poll(gpu);
        // The decoded art of games focused lately stays on the GPU (going back is instant); the
        // least recently drawn go when the pile passes 512 MB, never one drawn in the last 30 s.
        if now - self.last_trim > 3.0 {
            self.last_trim = now;
            self.images.trim(gpu, 512 << 20, 30.0);
        }
        self.text.upload(gpu);
        self.scene.sync_images(now, &self.images, 0.2);
        if self.st.tab == Tab::Home && self.st.overlay.is_none() {
            if let Some(page) = self.page.view.scroller(key("home")) {
                let y = self.scene.get(page).scroll_y.get(now);
                let k = (y / (self.scene.h * 0.55)).min(1.0);
                let n = self.scene.n(self.backdrop_host);
                n.opacity.snap(1.0 - k);
                n.ty.snap(-y * 0.25);
            }
        }
        if self.scene.needs_layout() {
            self.scene.layout(&mut self.text);
        }
        // Touch momentum and a long press coming due.
        self.touch_frame(now);
        // Sticky bars follow their page's scroll every frame (no lag behind a glide).
        crate::kit::pin_bar(&self.page.view, &mut self.scene, now);
        self.scene.animating(now) || self.nav.scrolling() || self.touch_busy()
    }

    /// Paints the scene; the page's charts draw in their paint slots, so sheets, overlays and
    /// toasts sit over them.
    pub fn paint(&mut self, now: f64, gpu: &mut Gpu, list: &mut DrawList) {
        // Painting rasterises the glyphs it meets for the first time; they go to the GPU right
        // after, before this frame is submitted. (They used to wait for the next frame's upload,
        // so the first frame of a screen showed only the glyphs earlier screens had used: text
        // flashed in on every first visit.) When the atlas filled up and was cleared while
        // painting, glyphs painted before the clear point at reused slots: paint once more.
        for pass in 0..2 {
            let resets = self.text.atlas_resets;
            self.paint_once(now, gpu, list);
            self.text.upload(gpu);
            if self.text.atlas_resets == resets {
                break;
            }
            if pass == 1 {
                log::warn!("text: the atlas filled up twice in one frame");
            }
        }
    }

    fn paint_once(&mut self, now: f64, gpu: &mut Gpu, list: &mut DrawList) {
        match self.legacy_page.as_mut() {
            Some((_, p, _)) => {
                p.prepare_paint(&self.scene, &mut self.text, gpu);
                self.scene.paint_with(now, &mut self.text, &self.images, list, &mut |sp, ts, out| p.paint_slot(sp, ts, out));
            }
            None => self.scene.paint(now, &mut self.text, &self.images, list),
        }
    }

    /// The year card's rect when its page asked for a picture of it (the main loop reads the frame
    /// back and crops it: features::stats::card_captured).
    pub fn take_card_shot(&mut self, now: f64) -> Option<[f32; 4]> {
        if !crate::features::stats::card_wanted() {
            return None;
        }
        let id = self.overlay.as_ref().and_then(|s| s.view.id(crate::features::stats::CARD)).or_else(|| self.page.view.id(crate::features::stats::CARD))?;
        Some(self.scene.window_rect(id, now))
    }

    /// What a page paints after the scene (Share's video frame); not while anything sits over it.
    pub fn paint_extra(&mut self, gpu: &mut Gpu, list: &mut DrawList) {
        if let Some(sh) = self.share.as_mut().filter(|_| self.st.options.is_none() && self.st.osk.is_none()) {
            use crate::screens::market::host::Page;
            sh.page.paint_extra(&self.scene, &mut self.text, gpu, list);
            self.text.upload(gpu);
        }
    }

    /// What the Market tab asked of the shell.
    fn market_acts(&mut self, now: f64) {
        use crate::screens::market::live::{drain, AppAct};
        for a in drain() {
            match a {
                AppAct::Toast(t) => self.st.toast(t, Tone::Info, now),
                AppAct::Error(t) => self.st.toast(t, Tone::Error, now),
                AppAct::OpenWeb(u) => self.run(Act::OpenWeb(u)),
                AppAct::Search => self.ask_search(),
                AppAct::Wallet => self.st.set_tab(Tab::Wallet, false),
                AppAct::WalletItem(id) => {
                    crate::screens::wallet::page::open_on(crate::screens::wallet::home::stack::WalletView::Nft { id });
                    self.st.set_tab(Tab::Wallet, false);
                }
                AppAct::WalletAdd => {
                    crate::screens::wallet::page::open_on(crate::screens::wallet::home::stack::WalletView::Add);
                    self.st.set_tab(Tab::Wallet, false);
                }
                AppAct::PriceAlert { coin_type, symbol, price } => self.open_price_alert(&coin_type, &symbol, price, now),
                AppAct::Osk { key, title, placeholder, value } => self.st.ask_text(key, title, value, placeholder.as_deref()),
            }
        }
    }

    /// Settled for a golden image: no motion, no image outstanding, boot gone (or held).
    pub fn settled(&self, now: f64) -> bool {
        !self.scene.animating(now) && !self.nav.scrolling() && !self.touch_busy() && self.images.pending() == 0 && self.arts.pending(&self.images) == 0 && self.views().iter().all(|v| v.next_deadline().is_none())
    }

    /// The minute changed.
    pub fn minute(&mut self) {
        self.clock = clock_text();
    }

    /// Text typed on a hardware keyboard (development): into the open on-screen keyboard.
    pub fn type_text(&mut self, s: &str) {
        if let Some(o) = self.st.osk.as_mut() {
            o.value.push_str(s);
        }
    }
}

/// The backend's events, on the UI thread (store.ts onGameState, onLaunchFailed, onLibraryChanged,
/// SteamHandoff.tsx).
pub fn on_event(st: &mut State, e: rare_core::Event, now: f64) {
    use rare_core::Event;
    match e {
        Event::LaunchFailed(f) => {
            st.starts.remove(&f.id);
            st.ver.starts += 1;
            if st.launching.as_deref() == Some(f.id.as_str()) {
                st.launching = None;
            }
            let name = st.run_name(&f.id);
            // A start that went wrong always says so: a crash is a toast, a game that didn't start
            // opens the sheet with Try again. First run and sign-in keep the screen.
            if f.reason == "crash" {
                st.toast_for(format!("{name} closed unexpectedly."), Tone::Error, now, 6.0);
            } else if matches!(st.overlay, Some(Overlay::Other("firstrun" | "signin"))) {
                st.toast_for(format!("{name} didn't start."), Tone::Error, now, 6.0);
            } else {
                st.options = None;
                st.open(Some(Overlay::LaunchFail { id: f.id, reason: f.reason.to_string(), code: f.code }));
            }
        }
        Event::GameState(r) => {
            let was = st.running.len();
            let gone = r.running.len() < was;
            let before = st.running.clone();
            st.set_running(r.running);
            // Games that stopped: cloud saves keep a copy and upload it.
            crate::features::cloudsync::on_running(st, &before, &st.running.clone());
            let running = st.running.clone();
            st.starts.retain(|id, _| running.contains(id));
            // A game just closed: playtime and last played changed.
            if gone {
                st.rescan_at = Some(now);
            }
        }
        Event::ProtonInstall(p) => crate::screens::game::panels::on_progress(&p),
        Event::SteamHandoff(ms) => st.handoff_until = Some(now + ms as f64 / 1000.0),
        Event::LibraryChanged => {
            // Steam touches its folders every few seconds while downloading: at most every 10 s.
            if st.rescan_at.is_none() {
                let wait = (10.0 - (now - st.last_scan)).max(1.5);
                st.rescan_at = Some(now + wait);
            }
        }
        other => log::debug!("event {}", other.name()),
    }
}

/// The keyboard questions screens ask through `Out::Act`: (title, placeholder). The asking screen
/// gets the text back through `Screen::answer` with the act as key. Friends' questions are not here:
/// `Social::act` asks them itself (`social:` keys) and takes the answer (the Friends tab has no
/// `answer`, so a keyboard opened here for them was dropped).
pub fn osk_for_act(a: &str) -> Option<(&'static str, &'static str)> {
    Some(match a {
        "alert:type" => ("Alert price", "Price in USD"),
        "items:price-other" => ("Price in SUI", "e.g. 12.5"),
        "stream:ask-address" => ("Add a PC", "192.168.0.20"),
        "support:reset-type" => ("Type RESET to reset this device", "RESET"),
        "ask:type" => ("Ask Rare", "Start Hades, turn on Do not disturb..."),
        _ => return None,
    })
}

/// Ms since the epoch.
pub fn wall_ms() -> f64 {
    (clock::realtime_ns() / 1_000_000) as f64
}

fn state_dir() -> std::path::PathBuf {
    let base = std::env::var("XDG_STATE_HOME").map(std::path::PathBuf::from).unwrap_or_else(|_| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state"));
    base.join("rare-native")
}

fn load_favs() -> Vec<String> {
    if std::env::var("RR_FIXTURE").is_ok() {
        return Vec::new();
    }
    std::fs::read(state_dir().join("favourites.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub(crate) fn save_favs(f: &[String]) {
    if std::env::var("RR_FIXTURE").is_ok() {
        return;
    }
    let d = state_dir();
    let _ = std::fs::create_dir_all(&d);
    let _ = std::fs::write(d.join("favourites.json"), serde_json::to_vec(f).unwrap_or_default());
}

/// The first game started on this device: how to get back out, once (store.ts exitTipOnce).
impl App {
    /// Steam's big-screen UI as the app "app:steam" (rare-launch `apps::steam`): Rare shows how to
    /// come back, then Steam has the screen and the Rare button brings Rare back. Over a game the
    /// menu or Quick settings go first.
    fn open_steam(&mut self, now: f64) {
        if self.journey.probe.on() {
            self.journey_act("SteamBigPicture");
        }
        let over = self.ingame.is_some();
        if over {
            self.ingame_closing();
        }
        self.st.handoff_until = Some(now + 3.0);
        self.fx.spawn(move |b| {
            if over {
                let _ = rare_overlay::close();
            }
            let r = b.steam_big_picture();
            Box::new(move |st: &mut State, now| {
                if let Err(e) = r {
                    log::warn!("steam: {e}");
                    st.handoff_until = None;
                    st.toast("Couldn't open Steam. Try again.", Tone::Error, now);
                }
            })
        });
    }
}

/// A run that counts as a game for "one game at a time": not an app, Steam's own screens, a PC
/// stream or an Android app.
fn is_game_run(id: &str) -> bool {
    !["app:", "stream:", "android:"].iter().any(|p| id.starts_with(p))
}

fn exit_tip_once(st: &mut State, now: f64) {
    if std::env::var("RR_FIXTURE").is_ok() {
        return;
    }
    let p = state_dir().join("exit-tip-shown");
    if p.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(state_dir());
    let _ = std::fs::write(&p, b"1");
    st.toast_for("Press the Rare button in any game for the Rare menu", Tone::Info, now, 8.0);
}

/// A scene-built Screen as a Phase 4 page (nothing painted after the scene, no hints of its own).
struct ScreenPage(Box<dyn crate::screens::overlay::Screen>);

impl crate::screens::overlay::Screen for ScreenPage {
    fn touch_focus(&mut self, cx: &mut crate::screens::overlay::Cx, x: f32, y: f32, now: f64) -> bool {
        self.0.touch_focus(cx, x, y, now)
    }
    fn touch_slide(&mut self, cx: &mut crate::screens::overlay::Cx, x: f32, now: f64) -> Option<crate::screens::overlay::Out> {
        self.0.touch_slide(cx, x, now)
    }
    fn touch_is_slider(&self, cx: &mut crate::screens::overlay::Cx, x: f32, y: f32, now: f64) -> bool {
        self.0.touch_is_slider(cx, x, y, now)
    }
    fn pad(&mut self, cx: &mut crate::screens::overlay::Cx, pad: rare_render::input::Pad, now: f64) -> crate::screens::overlay::Out {
        self.0.pad(cx, pad, now)
    }
    fn tick(&mut self, cx: &mut crate::screens::overlay::Cx, now: f64) {
        self.0.tick(cx, now)
    }
    fn next_deadline(&self) -> Option<f64> {
        self.0.next_deadline()
    }
    fn answer(&mut self, cx: &mut crate::screens::overlay::Cx, key: &str, value: Option<String>, now: f64) -> crate::screens::overlay::Out {
        self.0.answer(cx, key, value, now)
    }
    fn osk_status(&mut self, key: &str, value: &str) -> Option<String> {
        self.0.osk_status(key, value)
    }
    fn command(&mut self, cx: &mut crate::screens::overlay::Cx, cmd: &str, arg: &str, now: f64) {
        self.0.command(cx, cmd, arg, now)
    }
    fn take_message(&mut self) -> Option<String> {
        self.0.take_message()
    }
    fn bar(&self) -> Option<Vec<rare_render::focus::hints::Hint>> {
        self.0.bar()
    }
}

impl crate::screens::market::host::Page for ScreenPage {}
