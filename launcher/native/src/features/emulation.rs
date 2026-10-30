//! Emulation in Rare (rare-emu): the one-press setup, the BIOS check and retro trophies, as pages
//! over the Library (Library, Emulation), plus the pieces other screens show: the setup strip at
//! the top of the Emulation tab, a ROM's panel on its game page, retro games in the trophy case,
//! and unlock pop-ups over a running game.
//!
//! Retro trophies need no account: the device asks rare.market for a token with its friends key
//! (`rare_emu::retro`), emulators unlock against rare.market, and the unlocks are the Rare
//! account's trophies.
//!
//! Everything slow runs on a worker; the results land in one store the pages read (never fetched
//! on enter). With RR_FIXTURE the store is filled with sample data (RR_EMU picks a variant:
//! `setup` a setup in progress, `ready` everything installed, `signedout` retro trophies off,
//! `none` no ROMs).

use crate::act::Act;
use crate::data::model::Game;
use crate::kit::{self, s, tok, Btn as B, BtnIcon, DotTone, Ty, Ui, T};
use crate::page::{hash, Acts, Btn, Feedback, Fx, Page};
use crate::state::{Overlay, State, Tone};
use rare_emu::bios::{self, SystemBios};
use rare_emu::retro::{self, GameProgress};
use rare_emu::setup::Status;
use rare_render::focus::hints::{hint, Hint, Key};
use rare_render::focus::{Focusable, Group};
use rare_render::view::{key, El, Motion};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

pub const SETUP: &str = "emusetup";
pub const BIOS: &str = "emubios";
pub const RA: &str = "emura";

// ---------------------------------------------------------------- the store

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetupRun {
    pub step: String,
    pub name: String,
    pub progress: Option<f32>,
    /// Emulators finished so far: name and error.
    pub done: Vec<(String, Option<String>)>,
}

/// Retro trophies on this device: the name emulators show and hardcore.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RetroView {
    pub user: String,
    pub hardcore: bool,
}

#[derive(Default)]
struct Store {
    ver: u64,
    fixture: bool,
    status: Option<Status>,
    status_loading: bool,
    /// Which emulators to set up (games per emulator, sizes) and the controller settings check.
    choices: Option<Vec<rare_emu::setup::Choice>>,
    controls: Option<Vec<rare_emu::controls::Review>>,
    controls_busy: bool,
    setup: Option<SetupRun>,
    setup_error: Option<String>,
    bios: Option<Vec<SystemBios>>,
    bios_loading: bool,
    retro: Option<RetroView>,
    retro_loaded: bool,
    retro_busy: bool,
    retro_error: Option<String>,
    /// The account the token was last checked for, and when (app seconds).
    retro_checked: Option<(Option<String>, f64)>,
    /// When a token was last asked for without a prompt (app seconds).
    retro_tried: f64,
    played: Option<Vec<retro::Played>>,
    played_loading: bool,
    /// A ROM's achievements (None: no set for it, or a disc game not played yet).
    progress: HashMap<String, Option<GameProgress>>,
    loading: HashSet<String>,
    /// Retro games opened in the trophy case.
    by_game: HashMap<u32, GameProgress>,
    seen: HashSet<u64>,
    poll_at: f64,
    polling: bool,
    /// Unix ms the running ROM started.
    session_start: i64,
    /// ROMs whose game the emulator's session named.
    mapped: HashSet<String>,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut g = STORE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(|| {
        let mut s = Store::default();
        if std::env::var("RR_FIXTURE").is_ok() {
            seed_fixture(&mut s);
        }
        s
    }))
}

fn changed(s: &mut Store) {
    s.ver += 1;
    super::bus::wake();
}

pub fn ver() -> u64 {
    with(|s| s.ver)
}

fn bg(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new().name("rr-emu".into()).spawn(f).ok();
}

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// "Just now", "5 min ago", "3 h ago", "Yesterday", "4 days ago", "Oct 3".
pub fn ago(t: i64) -> String {
    let d = now_secs() - t;
    if d < 60 {
        "Just now".into()
    } else if d < 3600 {
        format!("{} min ago", d / 60)
    } else if d < 86400 {
        format!("{} h ago", d / 3600)
    } else if d < 2 * 86400 {
        "Yesterday".into()
    } else if d < 7 * 86400 {
        format!("{} days ago", d / 86400)
    } else {
        crate::screens::emulators::date_text(Some(t)).rsplit_once(',').map(|(a, _)| a.to_string()).unwrap_or_default()
    }
}

// ---------------------------------------------------------------- loads

pub fn status() -> Option<Status> {
    with(|s| s.status.clone())
}

pub fn refresh_status() {
    if with(|s| s.fixture || std::mem::replace(&mut s.status_loading, true)) {
        return;
    }
    bg(|| {
        let st = rare_emu::setup::status();
        let choices = rare_emu::setup::choices();
        let controls = rare_emu::controls::review(&rare_emu::roms::home());
        with(|s| {
            s.status = Some(st);
            s.choices = Some(choices);
            s.controls = Some(controls);
            s.status_loading = false;
            changed(s);
        });
    });
}

pub fn choices() -> Option<Vec<rare_emu::setup::Choice>> {
    with(|s| s.choices.clone())
}

pub fn controls_review() -> Option<Vec<rare_emu::controls::Review>> {
    with(|s| s.controls.clone())
}

/// Rare's controller profiles over the player's own settings for these emulators (after they said
/// yes on the setup page); their files stay next to them as .before-rare.
pub fn replace_controls(ids: Vec<String>) {
    if with(|s| std::mem::replace(&mut s.controls_busy, true)) {
        return;
    }
    let fixture = with(|s| s.fixture);
    bg(move || {
        if !fixture {
            let w = rare_emu::controls::replace(&rare_emu::roms::home(), &ids);
            log::info!("controls replaced: {w:?}");
        }
        with(|s| {
            s.controls_busy = false;
            if let Some(r) = s.controls.as_mut() {
                for x in r.iter_mut().filter(|x| ids.contains(&x.id)) {
                    x.state = rare_emu::controls::State::Current;
                    x.own.clear();
                }
            }
            changed(s);
        });
        if !fixture {
            refresh_status();
        }
    });
}

/// The status once, for screens that show it (the Library's Emulation tab).
pub fn want_status() {
    if with(|s| s.status.is_none()) {
        refresh_status();
    }
}

pub fn setup_run() -> Option<SetupRun> {
    with(|s| s.setup.clone())
}

/// Starts the setup (all emulators, or `only` these).
pub fn start_setup(only: Option<Vec<String>>) {
    let go = with(|s| {
        if s.setup.is_some() {
            return false;
        }
        s.setup = Some(SetupRun { step: "folders".into(), name: "Folders".into(), ..Default::default() });
        s.setup_error = None;
        changed(s);
        !s.fixture
    });
    if !go {
        return;
    }
    bg(move || {
        let ctx = super::bus::ctx();
        let ids: Option<Vec<&str>> = only.as_ref().map(|v| v.iter().map(String::as_str).collect());
        if let Err(e) = rare_emu::setup::run(&ctx, ids.as_deref()) {
            log::warn!("emulation setup: {e}");
        }
    });
}

/// `emu-setup` events from rare-emu's setup.
pub fn on_setup_event(st: &mut State, fx: &mut Fx, p: &serde_json::Value, now: f64) {
    let g = |k: &str| p.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let step = g("step");
    let done = p.get("done").and_then(|v| v.as_bool()).unwrap_or(false);
    let err = p.get("error").and_then(|v| v.as_str()).map(String::from);
    let progress = p.get("progress").and_then(|v| v.as_f64()).map(|v| v as f32);
    let finished = with(|s| {
        let run = s.setup.get_or_insert_with(SetupRun::default);
        if step == "done" {
            s.setup = None;
            s.setup_error = err.clone();
            changed(s);
            return true;
        }
        if step == "install" && done {
            let name = g("name");
            run.done.retain(|(n, _)| *n != name);
            run.done.push((name, err.clone()));
        }
        run.step = step.clone();
        run.name = g("name");
        run.progress = progress;
        changed(s);
        false
    });
    if finished {
        refresh_status();
        with(|s| s.bios = None);
        fx.run(Act::Scan);
        match err {
            None => st.toast("Emulation is set up", Tone::Info, now),
            Some(e) => st.toast(format!("Setup finished with problems: {e}"), Tone::Error, now),
        }
    }
}

pub fn bios() -> Option<Vec<SystemBios>> {
    with(|s| s.bios.clone())
}

pub fn refresh_bios(st: &State) {
    let mut systems: Vec<&'static str> = st.games().iter().filter(|g| g.source == "emu").filter_map(|g| rare_emu::roms::system_of(&g.id)).map(|s| s.id).collect();
    systems.dedup();
    if with(|s| s.fixture || std::mem::replace(&mut s.bios_loading, true)) {
        return;
    }
    bg(move || {
        let r = bios::check(&systems);
        with(|s| {
            s.bios = Some(r);
            s.bios_loading = false;
            changed(s);
        });
    });
}

// ---------------------------------------------------------------- retro trophies

pub fn retro_view() -> Option<RetroView> {
    with(|s| {
        if !s.retro_loaded && !s.fixture {
            s.retro_loaded = true;
            s.retro = retro::sign().map(|a| RetroView { user: a.user, hardcore: a.hardcore });
        }
        s.retro.clone()
    })
}

fn set_retro(a: Option<retro::Sign>) {
    with(|s| {
        s.retro = a.map(|a| RetroView { user: a.user, hardcore: a.hardcore });
        s.retro_loaded = true;
        s.played = None;
        s.progress.clear();
        s.by_game.clear();
        changed(s);
    });
}

/// A token for this device's friends key (blocking).
fn issue_token(k: &rare_friends::sign::DeviceKey) -> Result<serde_json::Value, String> {
    let net = crate::screens::wallet::live::market_net();
    rare_friends::api::send::<serde_json::Value, serde_json::Value>(net, Some(k), rare_market::net::Method::Post, "/v1/retro/token", Some(&serde_json::json!({})), rare_friends::sign::now_ms()).map_err(|e| e.to_string())
}

/// Gets this device a token when it has none, or has one for another account. `link`: also turn
/// friends on first when the device has no key (the player pressed Turn on; it can ask the wallet
/// for one signature). Blocking.
fn ensure_token(link: bool) -> Result<retro::Sign, String> {
    let address = crate::data::account::current().map(|(a, _)| a).ok_or("Make a Rare Wallet account first.")?;
    if let Some(s) = retro::sign().filter(|s| s.address == address) {
        return Ok(s);
    }
    let key = match crate::social::device_key() {
        Some(k) => k,
        None if link => {
            crate::social::link_device_key()?;
            crate::social::device_key().ok_or("Friends couldn't turn on here.")?
        }
        None => return Err("Turn on retro trophies first.".into()),
    };
    let v = issue_token(&key)?;
    let s = retro::from_token_answer(&v, &address, retro::hardcore()).ok_or("rare.market didn't answer with a token.")?;
    retro::save(Some(&s))?;
    Ok(s)
}

/// Keeps the device's token current: checked when the account changes, and every 30 s while there
/// is none (friends signs in by itself a little after start), with no prompt (Turn on is the player's).
fn check_token(now: f64) {
    // The account list is read from disk: at most every 10 s.
    if with(|s| s.fixture || s.retro_checked.as_ref().is_some_and(|(_, at)| now - at < 10.0)) {
        return;
    }
    let acct = crate::data::account::current().map(|(a, _)| a);
    let go = with(|s| {
        if s.fixture || s.retro_busy {
            return false;
        }
        let due = match &s.retro_checked {
            Some((a, _)) => *a != acct || now - s.retro_tried > 30.0,
            None => true,
        };
        s.retro_checked = Some((acct.clone(), now));
        if due {
            s.retro_tried = now;
        }
        due
    });
    if !go || acct.is_none() || retro::sign().is_some_and(|s| Some(&s.address) == acct.as_ref()) {
        return;
    }
    bg(|| match ensure_token(false) {
        Ok(s) => set_retro(Some(s)),
        Err(e) => log::info!("retro trophies: {e}"),
    });
}

/// Turn on: friends on if needed, then the token.
pub fn retro_turn_on() {
    let fixture = with(|s| {
        s.retro_busy = true;
        s.retro_error = None;
        changed(s);
        s.fixture
    });
    if fixture {
        with(|s| {
            s.retro_busy = false;
            s.retro = Some(RetroView { user: "pixelpilot".into(), hardcore: true });
            changed(s);
        });
        return;
    }
    bg(move || {
        let r = ensure_token(true);
        with(|s| s.retro_busy = false);
        match r {
            Ok(a) => {
                set_retro(Some(a));
                super::bus::note("Retro trophies are on", false);
            }
            Err(e) => with(|s| {
                s.retro_error = Some(e);
                changed(s);
            }),
        }
    });
}

pub fn retro_set_hardcore(on: bool) {
    if with(|s| s.fixture) {
        with(|s| {
            if let Some(r) = s.retro.as_mut() {
                r.hardcore = on;
            }
            changed(s);
        });
        return;
    }
    match retro::set_hardcore(on) {
        Ok(()) => set_retro(retro::sign()),
        Err(e) => super::bus::note(e, true),
    }
}

/// A read with the token; a token rare.market no longer takes is dropped and asked for again.
fn with_sign<T>(f: impl Fn(&retro::Sign) -> Result<T, String>) -> Result<T, String> {
    let s = retro::sign().ok_or("off")?;
    match f(&s) {
        Err(e) if retro::is_unauthorized(&e) => {
            let _ = retro::save(None);
            let s = ensure_token(false)?;
            set_retro(Some(s.clone()));
            f(&s)
        }
        r => r,
    }
}

/// The account's retro games (the trophy case), loaded once per sign-in and after unlocks.
pub fn played() -> Option<Vec<retro::Played>> {
    retro_view()?;
    let load = with(|s| s.retro.is_some() && s.played.is_none() && !s.played_loading && !s.fixture);
    if load {
        with(|s| s.played_loading = true);
        bg(|| {
            let r = with_sign(|a| retro::played(a));
            with(|s| {
                s.played_loading = false;
                s.played = Some(r.unwrap_or_default());
                changed(s);
            });
        });
    }
    with(|s| s.played.clone())
}

/// A ROM's achievements: None while loading or off, Some(None) when there is no set for it (or a
/// disc game not played yet).
pub fn progress(id: &str) -> Option<Option<GameProgress>> {
    let (have, go) = with(|s| (s.progress.get(id).cloned(), s.retro.is_some() && !s.progress.contains_key(id) && !s.loading.contains(id) && !s.fixture));
    if go {
        let id = id.to_string();
        with(|s| s.loading.insert(id.clone()));
        bg(move || {
            let r = match rare_emu::roms::find(&id) {
                Some(rom) => with_sign(|a| retro::progress_for(a, &rom)).unwrap_or_else(|e| {
                    log::warn!("retro trophies {id}: {e}");
                    None
                }),
                None => None,
            };
            with(|s| {
                s.loading.remove(&id);
                if let Some(p) = &r {
                    s.by_game.insert(p.game_id, p.clone());
                }
                s.progress.insert(id, r);
                changed(s);
            });
        });
    }
    have
}

/// Trophy case ids for retro games sit above Steam's.
pub const CASE_BASE: u32 = 0xE000_0000;

pub fn case_game(appid: u32) -> Option<u32> {
    (appid >= CASE_BASE).then(|| appid - CASE_BASE)
}

/// The trophy case opened a retro game: its achievements, once.
pub fn case_open(appid: u32) {
    let Some(game) = case_game(appid) else { return };
    if with(|s| s.by_game.contains_key(&game) || s.fixture) {
        return;
    }
    bg(move || {
        if let Ok(Some(p)) = with_sign(|a| retro::progress(a, game)) {
            with(|s| {
                s.by_game.insert(game, p);
                changed(s);
            });
        }
    });
}

/// "Super Mario Bros. · NES"
fn game_line(title: &str, console: u32) -> String {
    let c = retro::console_short(console);
    let t = retro::display_title(title);
    if c.is_empty() { t } else { format!("{t} \u{b7} {c}") }
}

/// Retro games for the trophy case: (case id, name, achievements, header art). Source "retro":
/// Rare trophies from classic games, the same rows as Steam's.
pub fn case_games() -> Vec<(u32, String, crate::screens::trophies::model::SteamGame, Vec<String>)> {
    use crate::screens::trophies::model::{SteamAchievement, SteamGame};
    let list = played().unwrap_or_default();
    with(|s| {
        let hardcore_on = s.retro.as_ref().is_some_and(|r| r.hardcore);
        list.iter()
            .filter(|p| p.total > 0)
            .map(|p| {
                let prog = s.by_game.get(&p.game_id);
                let achievements: Vec<SteamAchievement> = prog
                    .map(|g| {
                        g.achievements
                            .iter()
                            .map(|a| SteamAchievement {
                                api_name: a.id.to_string(),
                                name: a.title.clone(),
                                description: a.description.clone(),
                                icon_url: Some(a.badge_url.clone()).filter(|u| !u.is_empty()),
                                icon_gray_url: Some(a.locked_url.clone()).filter(|u| !u.is_empty()),
                                hidden: false,
                                unlocked: a.earned_at.is_some(),
                                unlock_time: a.earned_at.map(|t| t / 1000),
                                // RetroAchievements' share: the hardcore one for a hardcore unlock (and for
                                // locked rows when the player plays hardcore).
                                global_percent: (if a.hardcore || (a.earned_at.is_none() && hardcore_on) { a.rarity_hardcore.or(a.rarity) } else { a.rarity }).map(f64::from),
                                hardcore: a.hardcore && a.earned_at.is_some(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let game = SteamGame { appid: CASE_BASE + p.game_id, name: retro::display_title(&p.title), achievements, total: p.total, unlocked: p.earned, last_unlock: p.last_unlock_at.map(|t| t / 1000), source: "retro".into() };
                (CASE_BASE + p.game_id, game_line(&p.title, p.console_id), game, vec![p.icon_url.clone()].into_iter().filter(|u| !u.is_empty()).collect())
            })
            .collect()
    })
}

// ---------------------------------------------------------------- unlocks over the game

/// The badge as pixels for the HUD (96 px square).
fn badge_icon(url: &str) -> Option<rare_system::hud::Icon> {
    let bytes = retro::fetch_picture(url)?;
    let img = image::load_from_memory(&bytes).ok()?.resize_exact(96, 96, image::imageops::FilterType::Triangle).to_rgba8();
    Some(rare_system::hud::Icon { w: 96, h: 96, rgba: img.into_raw() })
}

/// While a ROM runs with retro trophies on: new unlocks every 3 s, over the game (top right, with
/// the badge) and in the trophy pop-ups when Rare is in front. Also notes which game the
/// emulator started a ROM as (disc games are hashed only by the emulator).
pub fn poll(st: &State, now: f64) {
    check_token(now);
    let rom = st.running.iter().find(|r| r.starts_with("emu:")).cloned();
    if rom.is_some() {
        retro_view();
    }
    let due = with(|s| {
        let Some(_) = &rom else {
            s.session_start = 0;
            return false;
        };
        if s.session_start == 0 {
            s.session_start = now_secs() * 1000;
            s.poll_at = now + 3.0;
            return false;
        }
        if s.retro.is_none() || s.polling || now < s.poll_at || s.fixture {
            return false;
        }
        s.polling = true;
        s.poll_at = now + 3.0;
        true
    });
    if !due {
        return;
    }
    let rom = rom.unwrap_or_default();
    bg(move || {
        let since = with(|s| s.session_start) - 60_000;
        let r = with_sign(|a| retro::recent(a, since));
        let fresh: Vec<retro::Unlock> = with(|s| {
            s.polling = false;
            r.unwrap_or_default().into_iter().rev().filter(|u| s.seen.insert(u.id)).collect()
        });
        let map = with(|s| !s.mapped.contains(&rom)) && retro::known_game(&rom).is_none();
        if map {
            // The game the emulator's session started, once it has.
            if let Ok(list) = with_sign(|a| retro::played(a)) {
                let start = with(|s| s.session_start);
                if let Some(g) = list.iter().find(|g| g.last_played_at >= start - 5_000) {
                    retro::remember_game(&rom, g.game_id);
                    with(|s| {
                        s.mapped.insert(rom.clone());
                        s.progress.remove(&rom);
                        changed(s);
                    });
                }
            }
        }
        if fresh.is_empty() {
            return;
        }
        for u in &fresh {
            let body = format!("{} \u{b7} {} {}", u.description, u.points, if u.points == 1 { "point" } else { "points" });
            if !rare_system::host::get().launcher_front() {
                rare_system::hud::toast_icon(&u.title, Some(&body), badge_icon(&u.badge_url));
            }
            super::trophies::push_external(u.title.clone(), retro::display_title(&u.game), u.points, Some(u.badge_url.clone()).filter(|b| !b.is_empty()));
        }
        with(|s| {
            s.progress.clear();
            s.by_game.clear();
            s.played = None;
            changed(s);
        });
    });
}

// ---------------------------------------------------------------- the Library's Emulation tab

/// The strip over the Emulation tab: what the setup needs, and the ways in (Set up, BIOS,
/// Retro trophies).
pub fn library_head(ui: &mut Ui, acts: &mut Acts, def: bool) -> El {
    want_status();
    let status = status();
    let run = setup_run();
    let (title, line) = match (&run, &status) {
        (Some(r), _) => ("Setting up emulation".to_string(), run_line(r)),
        (None, Some(s)) if s.ready => ("Emulation is ready".into(), format!("{} emulators. ROMs go in {}", s.emulators.len(), crate::data::model::pretty_path(&s.folders.first().map(|f| f.roms.clone()).unwrap_or_default()))),
        (None, Some(s)) => {
            let missing = s.emulators.iter().filter(|e| e.installed.is_none()).count();
            ("Set up emulation".into(), if missing == s.emulators.len() { "Installs RetroArch and the standalone emulators from Flathub, and makes the ROM folders on this device and your SD card.".into() } else { format!("{missing} of {} emulators to install. Rare installs them from Flathub and makes the ROM folders.", s.emulators.len()) })
        }
        (None, None) => ("Emulation".into(), "Checking what is installed".into()),
    };
    let ready = status.as_ref().is_some_and(|s| s.ready) && run.is_none();
    let setup_label = if run.is_some() { "Setting up" } else if ready { "Emulators" } else { "Set up" };
    let mut setup_b = B::new(setup_label, acts.tag(Act::Open(Overlay::Other(SETUP)))).sm().def(def).icon(BtnIcon::Lucide(if ready { "gamepad-2" } else { "download" }, 1.0, false));
    if !ready && run.is_none() {
        setup_b = setup_b.primary();
    }
    let bios_missing = bios().map(|b| b.iter().filter(|s| s.has_games && !s.ready).count()).unwrap_or(0);
    let bios_label = if bios_missing > 0 { format!("BIOS ({bios_missing} missing)") } else { "BIOS".into() };
    let ra_label = if retro_view().is_some() { "Retro trophies: on" } else { "Retro trophies" };
    let buttons = vec![
        kit::button(ui, setup_b),
        kit::button(ui, B::new(bios_label, acts.tag(Act::Open(Overlay::Other(BIOS)))).sm().icon(BtnIcon::Lucide("cpu", 1.0, false))),
        kit::button(ui, B::new(ra_label, acts.tag(Act::Open(Overlay::Other(RA)))).sm().icon(BtnIcon::Lucide("trophy", 1.0, false))),
    ];
    let mut text = ui.bx(s().col().grow().min0().gap(0.2)).child(ui.text(title, T::new(Ty::Body).w(600).trunc())).child(ui.text(line, T::new(Ty::Small).c(tok::MUTED).trunc()));
    if let Some(r) = &run {
        text = text.child(ui.bx(s().mt(0.3)).child(super::downloads::bar(ui, None, r.progress.unwrap_or(0.0), r.progress.is_none())));
    }
    kit::panel(ui, s().row().items_center().gap(1.1).px(1.25).py(0.9).mb(1.0))
        .child(kit::icon(ui, "gamepad-2", 1.5, tok::ICON2))
        .child(text)
        .child(ui.bx(s().row().items_center().gap(0.6)).group(Group::new("emu-head", [0.0; 4])).kids(buttons))
}

fn run_line(r: &SetupRun) -> String {
    let pct = r.progress.map(|p| format!(" {}%", (p * 100.0).floor() as u32)).unwrap_or_default();
    match r.step.as_str() {
        "folders" => "Making the ROM and BIOS folders".into(),
        "install" => format!("Installing {}{pct}", r.name),
        "cores" => format!("Getting RetroArch's cores{pct}"),
        _ => "Working".into(),
    }
}

// ---------------------------------------------------------------- a ROM's game page

/// The ROM's system and emulator, and its achievements.
pub fn game_panel(ui: &mut Ui, acts: &mut Acts, g: &Game) -> Option<El> {
    let sys = rare_emu::roms::system_of(&g.id)?;
    let (emu, inst) = rare_emu::roms::runner_of(sys, &rare_emu::roms::home());
    let line = match inst {
        Some(_) => format!("{} \u{b7} plays in {}", sys.name, emu.name),
        None => format!("{} \u{b7} needs {}, set it up in Library, Emulation", sys.name, emu.name),
    };
    let mut col = ui.bx(s().col().gap(0.6).max_w(52.0)).child(ui.text(line, T::new(Ty::Small).c(tok::fg(0.8)).trunc()));
    if sys.ra == 0 {
        return Some(col);
    }
    match retro_view() {
        None => {
            let b = kit::button(ui, B::new("Turn on", acts.tag(Act::Open(Overlay::Other(RA)))).sm());
            col = col.child(ui.bx(s().row().items_center().gap(0.75)).child(ui.text_s(s().shrink().min0(), "Turn on retro trophies to earn Rare trophies in this game.", T::new(Ty::Small).c(tok::MUTED).trunc())).child(b));
        }
        Some(_) => match progress(&g.id) {
            None => col = col.child(ui.text("Looking up trophies", T::new(Ty::Small).c(tok::MUTED))),
            Some(None) => {
                let hashed = rare_emu::roms::find(&g.id).is_some_and(|r| retro::rom_hash(&r).is_some());
                let msg = if hashed { "No trophies for this game yet." } else { "Trophies show here after you play it once." };
                col = col.child(ui.text(msg, T::new(Ty::Small).c(tok::MUTED)));
            }
            Some(Some(p)) => {
                let earned = p.earned();
                let pts: u32 = p.achievements.iter().filter(|a| a.earned_at.is_some()).map(|a| a.points).sum();
                let total_pts: u32 = p.achievements.iter().map(|a| a.points).sum();
                let head = format!("{earned} of {} trophies \u{b7} {pts} of {total_pts} points", p.achievements.len());
                let badges: Vec<El> = p
                    .achievements
                    .iter()
                    .take(12)
                    .map(|a| {
                        let url = if a.earned_at.is_some() { &a.badge_url } else { &a.locked_url };
                        let src: Vec<String> = Some(url.clone()).filter(|u| !u.is_empty()).into_iter().collect();
                        let pic = kit::art(ui, s().size(2.6), &src, 2.6, 2.6, kit::ArtFit::Cover(0.5), None);
                        let b = ui.bx(s().size(2.6)).bg(tok::L3).radius(ui.r(0.35)).clip().child(pic);
                        (if a.earned_at.is_none() { b.opacity(0.55) } else { b }).key(key(&a.id))
                    })
                    .collect();
                let more = p.achievements.len().saturating_sub(12);
                let mut strip = ui.bx(s().row().items_center().gap(0.45)).kids(badges);
                if more > 0 {
                    strip = strip.child(ui.text(format!("+{more}"), T::new(Ty::Small).c(tok::MUTED)));
                }
                let open = kit::button(ui, B::new("Trophies", acts.tag(Act::Trophies(Some(CASE_BASE + p.game_id)))).sm().icon(BtnIcon::Lucide("trophy", 1.0, false)));
                col = col.child(ui.bx(s().row().items_center().gap(0.75)).child(ui.text_s(s().shrink().min0(), head, T::new(Ty::Small).w(500).trunc())).child(open)).child(strip);
            }
        },
    }
    Some(col)
}

// ---------------------------------------------------------------- pages

fn page(ui: &mut Ui, icon: &str, title: &str, note: &str, k: &str) -> El {
    let head = ui
        .bx(s().row().items_center().gap(0.75).h(2.5).mb(1.25))
        .child(kit::icon(ui, icon, 1.5, tok::ICON2))
        .child(ui.text(title, T::new(Ty::Title).w(700)))
        .child(ui.text_s(s().ml_auto().shrink().min0(), note.to_string(), T::new(Ty::Small).c(tok::MUTED).trunc()));
    ui.bx(s().col().fill().px(2.5).py(2.0)).bg(tok::BG).enter(Motion::FADE).scrolls_y(key(k)).child(head)
}

/// A row: title, sub line, a right side; focusable when `tag` is given.
fn line(ui: &mut Ui, title: &str, sub: &str, right: Option<El>, tag: Option<(u32, &str)>, def: bool) -> El {
    let col = ui.bx(s().col().grow().min0().gap(0.2)).child(ui.text(title, T::new(Ty::Body).w(600).trunc())).child(ui.text(if sub.is_empty() { " " } else { sub }, T::new(Ty::Small).c(tok::MUTED).trunc()));
    let el = ui.bx(s().row().items_center().gap(1.0).h(4.0).wp(1.0).px(1.0)).radius(ui.r(0.5)).child(col).maybe(right);
    match tag {
        Some((t, h)) => kit::nav(kit::row_look(el), t, Some(h), None, def),
        None => el,
    }
}

fn state_word(ui: &mut Ui, text: &str, tone: DotTone) -> El {
    ui.bx(s().row().items_center().gap(0.45)).child(kit::dot(ui, s(), tone, false)).child(ui.text(text.to_string(), T::new(Ty::Small).c(tok::fg(0.85))))
}

/// Library, Emulation, Set up: which emulators (ticked for the systems the player has games for,
/// with their sizes), Set up / All, the controller settings check, RetroArch's cores, the folders.
#[derive(Default)]
pub struct SetupPage {
    ids: Vec<String>,
    /// Ticked emulators; None until the choice is read (then the ones with games).
    picked: Option<Vec<String>>,
    /// Emulators whose own controller settings the next press of "Use Rare's controls" replaces.
    confirm: Option<Vec<String>>,
}

fn choice_sub(c: &rare_emu::setup::Choice, fallback: &str) -> String {
    let mut s = if c.plays.is_empty() { fallback.to_string() } else { c.plays.clone() };
    if !c.installed {
        s.push_str(&format!("  \u{b7}  {} MB", c.size_mb));
    }
    match c.roms {
        0 => {}
        1 => s.push_str("  \u{b7}  1 game"),
        n => s.push_str(&format!("  \u{b7}  {n} games")),
    }
    s
}

impl Page for SetupPage {
    fn stamp(&self, _st: &State) -> u64 {
        hash((ver(), &self.picked, &self.confirm))
    }

    fn view(&mut self, ui: &mut Ui, _st: &State, acts: &mut Acts) -> El {
        want_status();
        let status = status();
        let run = setup_run();
        let choices = choices().unwrap_or_default();
        if self.picked.is_none() && !choices.is_empty() {
            self.picked = Some(choices.iter().filter(|c| c.picked).map(|c| c.id.clone()).collect());
        }
        let picked = self.picked.clone().unwrap_or_default();
        let note = match (&run, &status) {
            (Some(r), _) => run_line(r),
            (None, Some(s)) if s.ready => "Everything is installed".into(),
            (None, Some(s)) => format!("{} of {} installed", s.emulators.iter().filter(|e| e.installed.is_some()).count(), s.emulators.len()),
            _ => String::new(),
        };
        let mut p = page(ui, "gamepad-2", "Emulation", &note, "emu-setup");
        let Some(status) = status else { return p };
        if !status.flatpak {
            p = p.child(kit::empty(ui, s().mb(1.0), "Flatpak isn't on this system", Some("Rare installs emulators with Flatpak. Emulators you put in Applications as AppImages still work."), vec![]));
        }
        // Set up the ticked ones, or all of them.
        let missing: Vec<String> = status.emulators.iter().filter(|e| e.installed.is_none()).map(|e| e.id.clone()).collect();
        let todo: Vec<String> = picked.iter().filter(|id| missing.contains(id)).cloned().collect();
        let label = if run.is_some() {
            "Setting up".to_string()
        } else if missing.is_empty() {
            if status.cores_missing > 0 { "Install the cores".into() } else { "Check again".into() }
        } else {
            match todo.len() {
                0 => "Set up".to_string(),
                1 => "Set up 1 emulator".into(),
                n => format!("Set up {n} emulators"),
            }
        };
        let mut b = B::new(label, acts.tag(Act::Local(1))).lg().def(true).disabled(run.is_some() || (!missing.is_empty() && todo.is_empty()));
        if !todo.is_empty() || (missing.is_empty() && status.cores_missing > 0) {
            b = b.primary();
        }
        let mut top = ui.bx(s().row().items_center().gap(1.0).mb(1.5)).child(kit::button(ui, b));
        if !missing.is_empty() {
            top = top.child(kit::button(ui, B::new("All", acts.tag(Act::Local(2))).lg().disabled(run.is_some())));
        }
        let size = if missing.is_empty() {
            String::new()
        } else if todo.is_empty() {
            "Tick the systems you play, or set up everything.".into()
        } else {
            rare_emu::setup::size_text(&choices, &todo)
        };
        top = top.child(ui.text_s(s().shrink().min0(), size, T::new(Ty::Small).c(tok::MUTED).trunc()));
        p = p.child(top);
        if let Some(e) = with(|s| s.setup_error.clone()) {
            p = p.child(ui.text_s(s().mb(1.0), e, T::new(Ty::Small).c(tok::NEG).wrap()));
        }
        // Emulators: A ticks one that is not installed.
        self.ids = status.emulators.iter().map(|e| e.id.clone()).collect();
        let mut rows = Vec::new();
        for (i, e) in status.emulators.iter().enumerate() {
            let busy = run.as_ref().is_some_and(|r| r.step == "install" && r.name == e.name);
            let failed = run.as_ref().and_then(|r| r.done.iter().find(|(n, _)| *n == e.name).and_then(|(_, err)| err.clone()));
            let right = match (&e.installed, busy, &failed) {
                (_, true, _) => state_word(ui, &format!("Installing{}", run.as_ref().and_then(|r| r.progress).map(|p| format!(" {}%", (p * 100.0).floor() as u32)).unwrap_or_default()), DotTone::Primary),
                (Some(k), _, _) => state_word(ui, if k == "flatpak" { "Installed" } else { "Installed (AppImage)" }, DotTone::Positive),
                (None, _, Some(_)) => state_word(ui, "Didn't install", DotTone::Negative),
                (None, _, None) => kit::toggle(ui, picked.contains(&e.id)),
            };
            let sub = match (&failed, choices.iter().find(|c| c.id == e.id)) {
                (Some(f), _) => f.clone(),
                (None, Some(c)) => choice_sub(c, &e.plays),
                (None, None) => e.plays.clone(),
            };
            let tag = acts.tag(Act::Local(100 + i as u32));
            let hint = if e.installed.is_some() { "Installed" } else if picked.contains(&e.id) { "Untick" } else { "Tick" };
            rows.push(line(ui, &e.name, &sub, Some(right), Some((tag, hint)), false).key(key(&e.id)));
        }
        p = p.child(kit::section_head(ui, s().mb(0.6), "Emulators", Some("Ticked for the systems you have games for"), None));
        p = p.child(ui.bx(s().col().gap(0.25).mb(1.5)).group(Group::new("emu-setup-list", [0.0; 4]).remember()).kids(rows));
        // Controls: Rare's profiles, and the player's own settings it leaves alone until asked.
        if let Some(rev) = controls_review() {
            let own: Vec<&rare_emu::controls::Review> = rev.iter().filter(|r| r.state == rare_emu::controls::State::Own).collect();
            p = p.child(kit::section_head(ui, s().mb(0.6), "Controls", Some("Every button set for the SuiPlay, hotkeys in the Rare menu"), None));
            if own.is_empty() {
                let cw = state_word(ui, "Ready", DotTone::Positive);
                let names = rev.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", ");
                let sub = if names.is_empty() { "Set when an emulator is installed".to_string() } else { names };
                p = p.child(ui.bx(s().mb(1.5)).child(line(ui, "Rare's controls", &sub, Some(cw), None, false)));
            } else {
                let ids: Vec<String> = own.iter().map(|r| r.id.clone()).collect();
                let names = own.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", ");
                let asking = self.confirm.as_ref() == Some(&ids);
                let label = if with(|s| s.controls_busy) { "Replacing" } else if asking { "Replace my settings" } else { "Use Rare's controls" };
                let btn = kit::button(ui, B::new(label, acts.tag(Act::Local(3))).sm());
                let sub = if asking {
                    format!("{names}: press again to replace them. Yours stay next to them as .before-rare files.")
                } else {
                    format!("{names}: your own settings, kept as they are")
                };
                p = p.child(ui.bx(s().mb(1.5)).child(line(ui, "Your own controller settings", &sub, Some(btn), None, false)));
            }
        }
        // RetroArch's cores.
        let cores = if status.cores_missing == 0 { format!("All {} cores installed", status.cores) } else { format!("{} of {} to download from libretro", status.cores_missing, status.cores) };
        p = p.child(kit::section_head(ui, s().mb(0.6), "RetroArch cores", Some("NES, SNES, Game Boy, Genesis, arcade and more"), None));
        let cw = state_word(ui, if status.cores_missing == 0 { "Ready" } else { "Missing" }, if status.cores_missing == 0 { DotTone::Positive } else { DotTone::Muted });
        let cl = line(ui, "Cores", &cores, Some(cw), None, false);
        p = p.child(ui.bx(s().mb(1.5)).child(cl));
        // Folders.
        p = p.child(kit::section_head(ui, s().mb(0.6), "ROM folders", Some("Copy games you own into the folder for their system"), None));
        let mut fl = Vec::new();
        for f in &status.folders {
            let sub = format!("{}  \u{b7}  BIOS: {}", crate::data::model::pretty_path(&f.roms), crate::data::model::pretty_path(&f.bios));
            let right = state_word(ui, if f.exists { "Ready" } else { "Made by setup" }, if f.exists { DotTone::Positive } else { DotTone::Muted });
            fl.push(line(ui, &f.label, &sub, Some(right), None, false));
        }
        p = p.child(ui.bx(s().col().gap(0.25).mb(1.5)).kids(fl));
        p.child(ui.text_s(s().max_w(56.0), "Saves and states are kept under Emulation/saves, which Rare's cloud saves back up. Rare never downloads games or BIOS files.", T::new(Ty::Small).c(tok::MUTED).wrap()))
    }

    fn act(&mut self, st: &mut State, fx: &mut Fx, code: u32) {
        let missing: Vec<String> = status().map(|s| s.emulators.iter().filter(|e| e.installed.is_none()).map(|e| e.id.clone()).collect()).unwrap_or_default();
        match code {
            1 => {
                let picked = self.picked.clone().unwrap_or_default();
                let todo: Vec<String> = picked.into_iter().filter(|id| missing.contains(id)).collect();
                if missing.is_empty() || !todo.is_empty() {
                    fx.fb(Feedback::Select);
                    // Nothing missing: check again (cores, folders, first-run settings and controls).
                    start_setup(if missing.is_empty() { Some(vec!["retroarch".into()]) } else { Some(todo) });
                }
            }
            2 => {
                fx.fb(Feedback::Select);
                start_setup(None);
            }
            3 => {
                let own: Vec<String> = controls_review().unwrap_or_default().into_iter().filter(|r| r.state == rare_emu::controls::State::Own).map(|r| r.id).collect();
                if own.is_empty() {
                    return;
                }
                fx.fb(Feedback::Select);
                if self.confirm.as_ref() == Some(&own) {
                    self.confirm = None;
                    replace_controls(own);
                } else {
                    self.confirm = Some(own);
                }
            }
            c if c >= 100 => {
                if let Some(id) = self.ids.get((c - 100) as usize) {
                    if missing.contains(id) {
                        fx.fb(Feedback::Select);
                        let p = self.picked.get_or_insert_with(Vec::new);
                        if let Some(k) = p.iter().position(|x| x == id) {
                            p.remove(k);
                        } else {
                            p.push(id.clone());
                        }
                    } else {
                        // Installed: nothing to pick. Say so instead of a row that seems dead.
                        let name = status().and_then(|s| s.emulators.iter().find(|e| &e.id == id).map(|e| e.name.clone())).unwrap_or_else(|| id.clone());
                        st.toast(format!("{name} is installed. Set up checks it again."), crate::state::Tone::Info, rare_render::clock::secs());
                    }
                }
            }
            _ => {}
        }
    }

    fn tick(&mut self, _st: &mut State, _fx: &mut Fx, now: f64) -> Option<f64> {
        setup_run().is_some().then_some(now + 0.5)
    }
}

/// Library, Emulation, BIOS: per system, the files it needs and what the folder has.
#[derive(Default)]
pub struct BiosPage {
    asked: bool,
}

impl Page for BiosPage {
    fn stamp(&self, st: &State) -> u64 {
        hash((ver(), st.ver.library))
    }

    fn view(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El {
        if !self.asked {
            self.asked = true;
            refresh_bios(st);
        }
        let list = bios();
        let folder = list.as_ref().and_then(|l| l.first()).map(|b| crate::data::model::pretty_path(&b.folder)).unwrap_or_default();
        let note = list.as_ref().map(|l| format!("{} of {} systems ready", l.iter().filter(|b| b.ready).count(), l.len())).unwrap_or_default();
        let mut p = page(ui, "cpu", "BIOS files", &note, "emu-bios");
        p = p.child(ui.text_s(s().mb(1.25).max_w(60.0), format!("Some systems need files from the console itself. Copy the ones you dumped from your own consoles into {folder} (or Emulation/bios on your SD card). Rare checks each file against the known good copies."), T::new(Ty::Small).c(tok::fg(0.8)).wrap()));
        let Some(list) = list else {
            return p.child(ui.text("Checking", T::new(Ty::Small).c(tok::MUTED)));
        };
        let mut cards = Vec::new();
        for (i, b) in list.iter().enumerate() {
            let (word, tone) = if b.ready { ("Ready", DotTone::Positive) } else if b.entries.iter().any(|e| e.state == bios::State::Wrong) { ("Wrong file", DotTone::Attention) } else { ("Missing", DotTone::Muted) };
            let head = ui
                .bx(s().row().items_center().gap(0.75))
                .child(ui.text_s(s().shrink().min0(), b.name.clone(), T::new(Ty::Body).w(600).trunc()))
                .child(ui.text_s(s().shrink(), if b.has_games { "You have games for it".to_string() } else { String::new() }, T::new(Ty::Small).c(tok::MUTED).trunc()))
                .child(ui.bx(s().ml_auto()).child(state_word(ui, word, tone)));
            let mut col = ui.bx(s().col().gap(0.35).wp(1.0).px(1.0).py(0.85)).child(head);
            for e in &b.entries {
                let st_word = match e.state {
                    bios::State::Found => "Found",
                    bios::State::Missing => "Missing",
                    bios::State::Wrong => "Not the right file",
                };
                let mut what = format!("{}  \u{b7}  {}", e.name, e.note);
                if !e.or.is_empty() {
                    what.push_str(&format!("  \u{b7}  or {}", e.or.join(", ")));
                }
                if !e.required {
                    what.push_str("  \u{b7}  optional");
                }
                let c = match e.state {
                    bios::State::Found => tok::fg(0.85),
                    bios::State::Wrong => tok::ATTN,
                    bios::State::Missing => tok::MUTED,
                };
                col = col.child(ui.bx(s().row().items_center().gap(0.75)).child(ui.text_s(s().grow().min0(), what, T::new(Ty::Small).c(tok::fg(0.75)).trunc())).child(ui.text_s(s().shrink(), st_word, T::new(Ty::Small).w(500).c(c))));
            }
            if !b.hint.is_empty() {
                col = col.child(ui.text_s(s().wp(1.0).min0(), b.hint.clone(), T::new(Ty::Small).c(tok::MUTED).trunc()));
            }
            let card = col.bg(tok::L1).radius(ui.r(0.6));
            cards.push(kit::nav(kit::row_look(card), acts.tag(Act::None), None, None, i == 0).key(key(&b.system)));
        }
        p.child(ui.bx(s().col().gap(0.5)).group(Group::new("emu-bios", [0.0; 4]).remember()).kids(cards))
    }

    fn pad(&mut self, st: &mut State, fx: &mut Fx, b: Btn, _cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        if b == Btn::Y {
            with(|s| s.bios = None);
            refresh_bios(st);
            fx.fb(Feedback::Select);
            return true;
        }
        false
    }

    fn hints(&self, _st: &State) -> Vec<Hint> {
        vec![hint(Key::Y, "Check again")]
    }
}

/// Library, Emulation, Retro trophies: classic games earn Rare trophies with no other account.
/// Turn on (when this device has no friends key yet), hardcore, the games played.
#[derive(Default)]
pub struct RetroPage;

const ACT_TURN_ON: u32 = 4;
const ACT_HARDCORE: u32 = 5;

impl Page for RetroPage {
    fn stamp(&self, _st: &State) -> u64 {
        hash(ver())
    }

    fn view(&mut self, ui: &mut Ui, _st: &State, acts: &mut Acts) -> El {
        let view = retro_view();
        let (busy, err) = with(|s| (s.retro_busy, s.retro_error.clone()));
        let note = view.as_ref().map(|v| format!("Emulators show you as {}", v.user)).unwrap_or_default();
        let mut p = page(ui, "trophy", "Retro trophies", &note, "emu-ra");
        p = p.child(ui.text_s(s().mb(1.25).max_w(60.0), "Classic games earn Rare trophies as you play, with no other account. Achievement sets come from RetroAchievements; your unlocks stay with your Rare account.", T::new(Ty::Small).c(tok::fg(0.8)).wrap()));
        match view {
            None => {
                let off = state_word(ui, "Off", DotTone::Muted);
                let rows = vec![line(ui, "Retro trophies", "Off on this device. Turning them on lets this device act for your Rare account.", Some(off), None, false).key(key("state"))];
                p = p.child(ui.bx(s().col().gap(0.25).mb(1.25).max_w(56.0)).kids(rows));
                let on = kit::button(ui, B::new(if busy { "Turning on" } else { "Turn on" }, acts.tag(Act::Local(ACT_TURN_ON))).primary().lg().disabled(busy).def(true));
                p = p.child(ui.bx(s().row()).child(on));
            }
            Some(v) => {
                let hc = kit::toggle(ui, v.hardcore);
                let on = state_word(ui, "On", DotTone::Positive);
                let rows = vec![
                    line(ui, "Hardcore", "No save state loads, rewind or slow motion. Unlocks are marked hardcore.", Some(hc), Some((acts.tag(Act::Local(ACT_HARDCORE)), "Toggle")), true).key(key("hc")),
                    line(ui, "Works in", "RetroArch, DuckStation, PCSX2, PPSSPP and Dolphin", Some(on), None, false).key(key("tok")),
                ];
                p = p.child(ui.bx(s().col().gap(0.25).mb(1.5).max_w(56.0)).group(Group::new("emu-ra-rows", [0.0; 4])).kids(rows));
                match played() {
                    Some(list) if !list.is_empty() => {
                        p = p.child(kit::section_head(ui, s().mb(0.6), "Recently played", None, None));
                        let mut rows = Vec::new();
                        for g in list.iter().take(8) {
                            let fb = ui.bx(s().size(2.75).items_center().justify_center()).bg(tok::L3).child(kit::icon(ui, "trophy", 1.2, tok::ICON2));
                            let srcs: Vec<String> = Some(g.icon_url.clone()).filter(|i| !i.is_empty()).into_iter().collect();
                            let icon = ui.bx(s().size(2.75)).radius(ui.r(0.35)).clip().child(kit::art(ui, s().size(2.75), &srcs, 2.75, 2.75, kit::ArtFit::Cover(0.5), Some(fb)));
                            let console = retro::console_name(g.console_id);
                            let sub = if console.is_empty() { format!("{} of {}", g.earned, g.total) } else { format!("{console} \u{b7} {} of {}", g.earned, g.total) };
                            let el = ui.bx(s().row().items_center().gap(0.9).h(3.75).px(1.0)).child(icon).child(ui.bx(s().col().grow().min0()).child(ui.text(retro::display_title(&g.title), T::new(Ty::Body).w(500).trunc())).child(ui.text(sub, T::new(Ty::Small).c(tok::MUTED).trunc())));
                            rows.push(kit::nav(kit::row_look(el.radius(ui.r(0.5))), acts.tag(Act::Trophies(Some(CASE_BASE + g.game_id))), Some("Trophies"), None, false).key(key(&g.game_id)));
                        }
                        p = p.child(ui.bx(s().col().gap(0.25).max_w(56.0)).group(Group::new("emu-ra-games", [0.0; 4])).kids(rows));
                    }
                    Some(_) => p = p.child(ui.text_s(s().max_w(56.0), "Play a game from Library, Emulation and its trophies show here.", T::new(Ty::Small).c(tok::MUTED).wrap())),
                    None => {}
                }
            }
        }
        if let Some(e) = err {
            p = p.child(ui.text_s(s().mt(1.0).max_w(56.0), e, T::new(Ty::Small).c(tok::NEG).wrap()));
        }
        p
    }

    fn act(&mut self, _st: &mut State, fx: &mut Fx, code: u32) {
        match code {
            ACT_TURN_ON => {
                fx.fb(Feedback::Select);
                retro_turn_on();
            }
            ACT_HARDCORE => {
                fx.fb(Feedback::Toggle);
                retro_set_hardcore(!retro_view().is_some_and(|v| v.hardcore));
            }
            _ => {}
        }
    }

    fn tick(&mut self, _st: &mut State, _fx: &mut Fx, now: f64) -> Option<f64> {
        with(|s| s.retro_busy).then_some(now + 0.5)
    }
}

// ---------------------------------------------------------------- fixture

fn seed_fixture(s: &mut Store) {
    use rare_emu::setup::{EmuStatus, Folder};
    s.fixture = true;
    let variant = std::env::var("RR_EMU").unwrap_or_default();
    let installed = |id: &str| match variant.as_str() {
        "ready" | "setup" => Some("flatpak".to_string()),
        _ => matches!(id, "retroarch" | "pcsx2" | "dolphin" | "ppsspp").then(|| "flatpak".to_string()),
    };
    let emulators: Vec<EmuStatus> = rare_emu::catalog::EMULATORS.iter().map(|e| EmuStatus { id: e.id.into(), name: e.name.into(), plays: e.plays.into(), installed: if variant == "setup" && matches!(e.id, "rpcs3" | "xemu" | "ryujinx") { None } else { installed(e.id) } }).collect();
    let ready = emulators.iter().all(|e| e.installed.is_some());
    let cores = rare_emu::systems::cores().len();
    s.status = Some(Status {
        flatpak: true,
        ready: ready && variant != "setup",
        emulators,
        cores,
        cores_missing: if variant == "ready" { 0 } else { 6 },
        folders: vec![
            Folder { label: "This device".into(), roms: "/var/home/rare/Emulation/roms".into(), bios: "/var/home/rare/Emulation/bios".into(), exists: true },
            Folder { label: "SD card".into(), roms: "/run/media/rare/SD/Emulation/roms".into(), bios: "/run/media/rare/SD/Emulation/bios".into(), exists: variant == "ready" },
        ],
    });
    s.choices = Some(
        rare_emu::catalog::EMULATORS
            .iter()
            .map(|e| {
                let roms = match e.id {
                    "retroarch" => 41,
                    "dolphin" => 3,
                    "duckstation" => 2,
                    "rpcs3" => 1,
                    _ => 0,
                };
                let inst = s.status.as_ref().is_some_and(|st| st.emulators.iter().any(|x| x.id == e.id && x.installed.is_some()));
                rare_emu::setup::Choice { id: e.id.into(), name: e.name.into(), plays: e.plays.into(), size_mb: e.size_mb, installed: inst, roms, picked: !inst && roms > 0 }
            })
            .collect(),
    );
    let rv = |id: &str, name: &str, own: bool| rare_emu::controls::Review { id: id.into(), name: name.into(), state: if own { rare_emu::controls::State::Own } else { rare_emu::controls::State::Current }, own: if own { vec![format!("/var/home/rare/.var/app/{id}/settings.ini")] } else { vec![] } };
    s.controls = Some(match variant.as_str() {
        "ready" => vec![rv("retroarch", "RetroArch", false), rv("dolphin", "Dolphin", false), rv("pcsx2", "PCSX2", false)],
        _ => vec![rv("dolphin", "Dolphin", true), rv("pcsx2", "PCSX2", true), rv("ppsspp", "PPSSPP", false)],
    });
    if variant == "setup" {
        s.setup = Some(SetupRun { step: "install".into(), name: "RPCS3".into(), progress: Some(0.45), done: vec![("RetroArch".into(), None), ("xemu".into(), Some("No remote refs found for app.xemu.xemu".into()))] });
    }
    let e = |name: &str, note: &str, state: bios::State, required: bool, or: &[&str]| bios::Entry { name: name.into(), note: note.into(), state, required, path: None, or: or.iter().map(|x| x.to_string()).collect() };
    let sysb = |id: &str, name: &str, ready: bool, has: bool, entries: Vec<bios::Entry>, hint: &str| SystemBios { system: id.into(), name: name.into(), ready, entries, hint: hint.into(), folder: "/var/home/rare/Emulation/bios".into(), has_games: has };
    s.bios = Some(vec![
        sysb("psx", "PlayStation", true, true, vec![e("scph5501.bin", "North America", bios::State::Found, true, &[])], "One BIOS from your own console plays games from every region."),
        sysb("ps2", "PlayStation 2", false, true, vec![e("PS2 BIOS (.bin)", "Any region, from your own console", bios::State::Missing, true, &[])], "Any PS2 BIOS dump works; PCSX2 lists every one it finds."),
        sysb("saturn", "Sega Saturn", false, true, vec![e("mpr-17933.bin", "North America and Europe", bios::State::Wrong, true, &[])], ""),
        sysb("segacd", "Sega CD", false, false, vec![e("bios_CD_U.bin", "North America", bios::State::Missing, true, &["bios_CD_E.bin", "bios_CD_J.bin"])], "Each region's games want their own BIOS."),
        sysb("gba", "Game Boy Advance", true, true, vec![e("gba_bios.bin", "Boot logo and better accuracy", bios::State::Missing, false, &[])], "Games play without it."),
        sysb("dreamcast", "Dreamcast", false, false, vec![e("dc/dc_boot.bin", "Boot ROM", bios::State::Missing, true, &[]), e("dc/dc_flash.bin", "Flash ROM", bios::State::Missing, false, &[])], "They go in a dc folder inside the BIOS folder."),
    ]);
    if variant != "signedout" {
        s.retro = Some(RetroView { user: "pixelpilot".into(), hardcore: true });
        s.retro_loaded = true;
        let t0 = 1_800_000_000_000i64;
        s.played = Some(vec![
            retro::Played { game_id: 1, title: "~Homebrew~ Micro Mages".into(), console_id: 7, icon_url: String::new(), total: 24, earned: 9, earned_hardcore: 9, last_played_at: t0, last_unlock_at: Some(t0) },
            retro::Played { game_id: 2, title: "~Homebrew~ Tobu Tobu Girl Deluxe".into(), console_id: 6, icon_url: String::new(), total: 18, earned: 18, earned_hardcore: 18, last_played_at: t0 - 86_400_000, last_unlock_at: Some(t0 - 86_400_000) },
        ]);
        let ach = |i: u64, title: &str, pts: u32, earned: bool| retro::Achievement { id: i, title: title.into(), description: format!("{title}: something you did."), points: pts, badge: String::new(), badge_url: String::new(), locked_url: String::new(), earned_at: earned.then_some(t0 - i as i64 * 3_600_000), hardcore: true, rarity: Some((70.0 - i as f32 * 2.9).max(0.8)), rarity_hardcore: Some((40.0 - i as f32 * 1.6).max(0.4)) };
        let prog = GameProgress { game_id: 1, title: "~Homebrew~ Micro Mages".into(), console_id: 7, icon_url: String::new(), achievements: (0..24).map(|i| ach(i, &format!("Tower floor {}", i + 1), [5, 10, 25][i as usize % 3], i < 9)).collect() };
        s.by_game.insert(1, prog.clone());
        s.progress.insert("emu:nes/Micro Mages.nes".into(), Some(prog));
        s.progress.insert("emu:snes/Super Bat Puncher (World).sfc".into(), None);
    }
}

/// The fixture's ROMs (Library, Emulation): homebrew names, one very long one; RR_EMU=many gives
/// hundreds across every system, `none` gives none.
pub fn mock_games() -> Vec<Game> {
    let variant = std::env::var("RR_EMU").unwrap_or_default();
    if variant == "none" {
        return vec![];
    }
    let mk = |folder: &str, file: &str, title: &str| Game {
        id: format!("emu:{folder}/{file}"),
        source: "emu".into(),
        title: title.into(),
        installed: true,
        install_dir: None,
        size_bytes: Some(4_194_304.0),
        last_played: None,
        playtime_minutes: None,
        art: Default::default(),
        launch: None,
        origin: format!("{} \u{b7} This device", rare_emu::systems::by_folder(folder).map(|s| s.name).unwrap_or("")),
        foreign: false,
    };
    if variant == "many" {
        let mut v = Vec::new();
        for (i, sys) in rare_emu::systems::SYSTEMS.iter().enumerate() {
            for n in 0..(3 + i % 9) {
                v.push(mk(sys.id, &format!("Homebrew {n}.{}", sys.exts.first().copied().unwrap_or("bin")), &format!("{} Homebrew {}", sys.short, n + 1)));
            }
        }
        return v;
    }
    vec![
        mk("nes", "Micro Mages.nes", "Micro Mages"),
        mk("nes", "Twin Dragons.nes", "Twin Dragons"),
        mk("snes", "Super Bat Puncher (World).sfc", "Super Bat Puncher"),
        mk("gb", "Dangan GB.gb", "Dangan GB"),
        mk("gbc", "Tobu Tobu Girl Deluxe.gbc", "Tobu Tobu Girl Deluxe"),
        mk("gba", "Anguna.gba", "Anguna"),
        mk("gba", "Kubo 3.gba", "Kubo 3"),
        mk("genesis", "Xenocrisis.md", "Xenocrisis"),
        mk("psx", "Big Adventure (USA) (Disc 1).chd", "Big Adventure"),
        mk("atari2600", "Sheep It Up.a26", "Sheep It Up"),
        mk("snes", "Long.sfc", "The Extraordinarily Long Adventures of the Very Small Knight and the Tower of Endless Staircases (USA) (Rev 2)"),
    ]
}
