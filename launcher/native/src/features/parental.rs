//! Parental controls: a parent's PIN (4 to 6 digits, argon2id with a random salt, never stored
//! plain) and per-profile rules (a daily play-time limit, allowed hours, a maximum age rating),
//! kept in ~/.config/rare/parental.json. Play time is counted for every game Rare runs, per day
//! and per profile (parental-usage.json, so it survives restarts).
//!
//! What asks for the PIN (when one is set):
//! - starting a game over the day's limit, outside the allowed hours or above the age rating
//!   (`gate_launch`; the PIN lets that one session through),
//! - approving a transaction in Rare Wallet's review sheet (`sign::host`, so every purchase and
//!   every signer is covered at one place),
//! - changing the System, Network, Accounts, Developer and Parental controls sections of Settings.
//!
//! The PIN sheet itself is drawn by the review host (`screens::wallet::sign::host`), which sits
//! over every sheet: `ask` queues a request and answers on a channel. Once entered, the PIN stays
//! unlocked for `UNLOCK_SECS`.
//!
//! The profile is the selected Rare Wallet account, or "default" without one. A profile with no
//! rules of its own follows "default".

use crate::state::{State, Tone};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

/// How long an entered PIN stays good.
pub const UNLOCK_SECS: f64 = 300.0;
/// The warning before the time runs out.
pub const WARN_SECS: i64 = 600;
/// Reminders after the time ran out, while the game keeps running.
pub const REMIND_SECS: f64 = 900.0;
pub const PIN_MIN: usize = 4;
pub const PIN_MAX: usize = 6;
/// Wrong tries before a wait.
const MAX_TRIES: u32 = 5;

/// Daily limits in minutes (None: off), and their labels.
pub const DAILY: [Option<u32>; 7] = [None, Some(30), Some(60), Some(90), Some(120), Some(180), Some(240)];
pub const DAILY_LABELS: [&str; 7] = ["Off", "30 min", "1 h", "1.5 h", "2 h", "3 h", "4 h"];
/// Maximum age ratings (None: off), and their labels.
pub const AGES: [Option<u8>; 4] = [None, Some(7), Some(12), Some(16)];
pub const AGE_LABELS: [&str; 4] = ["Off", "Up to 7", "Up to 12", "Up to 16"];
/// Settings sections that need the PIN to change.
pub const LOCKED_SECTIONS: [&str; 5] = ["system", "network", "accounts", "developer", "parental"];

// ---------------------------------------------------------------- the rules

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Rules {
    /// Minutes a day (None: no limit).
    #[serde(default)]
    pub daily_min: Option<u32>,
    /// Allowed hours, from and to (0..=24, to may be smaller: past midnight). None: any time.
    #[serde(default)]
    pub hours: Option<(u8, u8)>,
    /// The oldest age rating allowed (None: off).
    #[serde(default)]
    pub max_age: Option<u8>,
}

impl Rules {
    pub fn any(&self) -> bool {
        self.daily_min.is_some() || self.hours.is_some() || self.max_age.is_some()
    }
}

/// "8:00".
pub fn hour_text(h: u8) -> String {
    format!("{h}:00")
}

/// Whether `minute` (of the day) falls in the allowed hours.
pub fn in_window(from: u8, to: u8, minute: u32) -> bool {
    let (f, t) = (from as u32 * 60, to as u32 * 60);
    if f == t {
        return true;
    }
    if f < t {
        minute >= f && minute < t
    } else {
        minute >= f || minute < t
    }
}

/// Seconds until the allowed hours end (None: outside them, or any time).
pub fn window_left(from: u8, to: u8, minute: u32, sec: u32) -> Option<i64> {
    if from == to || !in_window(from, to, minute) {
        return None;
    }
    let now = (minute * 60 + sec) as i64;
    let end = to as i64 * 3600;
    Some(if end > now { end - now } else { end + 86_400 - now })
}

/// Seconds of play left today under the daily limit (None: no limit). Never below 0.
pub fn budget_left(daily_min: Option<u32>, used_secs: u64) -> Option<i64> {
    daily_min.map(|m| (m as i64 * 60 - used_secs as i64).max(0))
}

/// Why a game may not start now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    UsedUp,
    Hours(u8, u8),
    Rating(u8),
}

impl Block {
    pub fn title(&self) -> String {
        match self {
            Block::UsedUp => "Today's play time is used up".into(),
            Block::Hours(f, t) => format!("Playing is allowed from {} to {}", hour_text(*f), hour_text(*t)),
            Block::Rating(_) => "This game needs a parent's PIN".into(),
        }
    }

    pub fn reason(&self) -> String {
        match self {
            Block::Rating(age) => format!("It's rated for ages {age} and up. A parent's PIN lets it start."),
            _ => "A parent's PIN lets this game start.".into(),
        }
    }
}

/// The rule a launch breaks, if any. `rating`: the game's age rating when the store has one.
pub fn block_for(r: &Rules, used_secs: u64, minute: u32, rating: Option<u8>) -> Option<Block> {
    if let Some((f, t)) = r.hours {
        if !in_window(f, t, minute) {
            return Some(Block::Hours(f, t));
        }
    }
    if budget_left(r.daily_min, used_secs) == Some(0) {
        return Some(Block::UsedUp);
    }
    match (r.max_age, rating) {
        (Some(max), Some(age)) if age > max => Some(Block::Rating(age)),
        _ => None,
    }
}

/// Seconds left before a rule stops play (the smaller of the day's budget and the allowed hours),
/// and whether it is the budget.
pub fn play_left(r: &Rules, used_secs: u64, minute: u32, sec: u32) -> Option<(i64, bool)> {
    let budget = budget_left(r.daily_min, used_secs).map(|s| (s, true));
    let window = match r.hours {
        Some((f, t)) if !in_window(f, t, minute) => Some((0, false)),
        Some((f, t)) => window_left(f, t, minute, sec).map(|s| (s, false)),
        None => None,
    };
    match (budget, window) {
        (Some(b), Some(w)) => Some(if w.0 < b.0 { w } else { b }),
        (b, w) => b.or(w),
    }
}

/// "45 min", "1 h 20 min".
pub fn played_text(secs: u64) -> String {
    let m = secs / 60;
    match (m / 60, m % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

// ---------------------------------------------------------------- the PIN

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PinHash {
    pub algo: String,
    pub salt: String,
    pub hash: String,
}

pub fn valid_pin(pin: &str) -> bool {
    (PIN_MIN..=PIN_MAX).contains(&pin.len()) && pin.bytes().all(|b| b.is_ascii_digit())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    (0..s.len()).step_by(2).map(|i| s.get(i..i + 2).and_then(|p| u8::from_str_radix(p, 16).ok())).collect()
}

fn argon(pin: &str, salt: &[u8]) -> Option<[u8; 32]> {
    use argon2::{Algorithm, Argon2, Params, Version};
    let p = Params::new(19_456, 2, 1, Some(32)).ok()?;
    let mut out = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, p).hash_password_into(pin.as_bytes(), salt, &mut out).ok()?;
    Some(out)
}

fn random_salt() -> [u8; 16] {
    use std::io::Read;
    let mut s = [0u8; 16];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut s)).is_err() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let seed = t ^ ((std::process::id() as u128) << 64);
        s.copy_from_slice(&seed.to_le_bytes());
    }
    s
}

/// A new salted hash of `pin`.
pub fn hash_pin(pin: &str) -> PinHash {
    let salt = random_salt();
    let h = argon(pin, &salt).unwrap_or([0; 32]);
    PinHash { algo: "argon2id".into(), salt: hex(&salt), hash: hex(&h) }
}

/// Whether `pin` matches.
pub fn verify_pin(h: &PinHash, pin: &str) -> bool {
    let (Some(salt), Some(want)) = (unhex(&h.salt), unhex(&h.hash)) else { return false };
    let Some(got) = argon(pin, &salt) else { return false };
    want.len() == got.len() && want.iter().zip(got.iter()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0
}

// ---------------------------------------------------------------- the files

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct Saved {
    #[serde(default)]
    pin: Option<PinHash>,
    #[serde(default)]
    profiles: BTreeMap<String, Rules>,
}

/// Seconds played, per profile, per local day ("2026-10-27").
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct Usage {
    #[serde(default)]
    days: BTreeMap<String, BTreeMap<String, u64>>,
}

fn dir() -> PathBuf {
    rare_core::paths::config_dir()
}

fn read<T: for<'de> Deserialize<'de> + Default>(name: &str) -> T {
    std::fs::read(dir().join(name)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write<T: Serialize>(name: &str, v: &T) {
    let path = dir().join(name);
    let _ = std::fs::create_dir_all(dir());
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_vec_pretty(v).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
    }
}

fn fixture() -> bool {
    std::env::var("RR_FIXTURE").is_ok()
}

// ---------------------------------------------------------------- local time

/// The local day ("2026-10-27"), minute of the day and second of the minute.
pub fn local_now() -> (String, u32, u32) {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let tt = t as libc::time_t;
    // SAFETY: localtime_r fills tm.
    unsafe { libc::localtime_r(&tt, &mut tm) };
    (format!("{:04}-{:02}-{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday), (tm.tm_hour * 60 + tm.tm_min) as u32, tm.tm_sec.clamp(0, 59) as u32)
}

// ---------------------------------------------------------------- the PIN sheet's queue

/// What the PIN sheet is asked.
pub enum Ask {
    /// Enter the PIN: `title` on top, `reason` under it. Answers true when it matched.
    Check { title: String, reason: String, tx: Sender<bool> },
    /// Pick a new PIN (twice). Answers true once it is saved.
    Set { tx: Sender<bool> },
}

#[derive(Default)]
struct Inner {
    loaded: bool,
    saved: Saved,
    usage: Usage,
    profile: (String, String),
    unlocked_until: f64,
    failures: u32,
    wait_until: f64,
    queue: Vec<Ask>,
    /// A launch waiting for the PIN.
    pending: Option<(String, Receiver<bool>)>,
    /// Games the PIN let through: id, granted at, seen running.
    passes: HashMap<String, (f64, bool)>,
    last_play: Option<f64>,
    frac: f64,
    unsaved: f64,
    /// Today's notices: (profile, day), warned, ended at.
    noticed: ((String, String), bool, Option<f64>),
    /// Steam app id to age rating (None: the store has none).
    ratings: HashMap<u32, Option<u8>>,
    fetching: HashSet<u32>,
    ratings_loaded: bool,
    ver: u64,
}

static INNER: Mutex<Option<Inner>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Inner) -> R) -> R {
    let mut g = INNER.lock().unwrap_or_else(|e| e.into_inner());
    let me = g.get_or_insert_with(Inner::default);
    if !me.loaded {
        me.loaded = true;
        if !fixture() {
            me.saved = read("parental.json");
            me.usage = read("parental-usage.json");
        }
        me.profile = ("default".into(), "This device".into());
        // Shots: RR_PARENTAL=sheet puts the PIN sheet up for a launch over the limit.
        if fixture() && std::env::var("RR_PARENTAL").as_deref() == Ok("sheet") {
            let (tx, _) = channel();
            me.queue.push(Ask::Check { title: Block::UsedUp.title(), reason: Block::UsedUp.reason(), tx });
        }
    }
    f(me)
}

fn save(me: &Inner) {
    if !fixture() {
        write("parental.json", &me.saved);
    }
}

fn save_usage(me: &mut Inner) {
    me.unsaved = 0.0;
    // keep two weeks
    for days in me.usage.days.values_mut() {
        while days.len() > 14 {
            let k = days.keys().next().cloned().unwrap_or_default();
            days.remove(&k);
        }
    }
    if !fixture() {
        write("parental-usage.json", &me.usage);
    }
}

/// Moves whenever the PIN, the rules or the unlock change (Settings redraws its section).
pub fn version() -> u64 {
    with(|me| me.ver)
}

pub fn pin_set() -> bool {
    with(|me| me.saved.pin.is_some())
}

/// The PIN is set and was not entered in the last few minutes.
pub fn needs_pin(now: f64) -> bool {
    with(|me| me.saved.pin.is_some() && now >= me.unlocked_until)
}

/// A Settings section the PIN guards right now.
pub fn section_locked(key: &str, now: f64) -> bool {
    LOCKED_SECTIONS.contains(&key) && needs_pin(now)
}

/// Asks for the PIN on the sheet. Answers true at once when no PIN is set or it is unlocked.
pub fn ask(title: &str, reason: &str, now: f64) -> Receiver<bool> {
    let (tx, rx) = channel();
    if !needs_pin(now) {
        let _ = tx.send(true);
    } else {
        with(|me| me.queue.push(Ask::Check { title: title.into(), reason: reason.into(), tx }));
    }
    rx
}

/// Asks for a new PIN (Set PIN, Change PIN).
pub fn ask_set() -> Receiver<bool> {
    let (tx, rx) = channel();
    with(|me| me.queue.push(Ask::Set { tx }));
    rx
}

/// The oldest waiting request (the sheet's host shows it).
pub fn take_ask() -> Option<Ask> {
    with(|me| (!me.queue.is_empty()).then(|| me.queue.remove(0)))
}

/// Checks an entered PIN. Ok(true) matched (and unlocks), Ok(false) wrong, Err(secs) a wait.
pub fn try_pin(pin: &str, now: f64) -> Result<bool, f64> {
    with(|me| {
        if now < me.wait_until {
            return Err(me.wait_until - now);
        }
        let Some(h) = me.saved.pin.clone() else { return Ok(true) };
        if verify_pin(&h, pin) {
            me.failures = 0;
            me.unlocked_until = now + UNLOCK_SECS;
            me.ver += 1;
            Ok(true)
        } else {
            me.failures += 1;
            if me.failures >= MAX_TRIES {
                let extra = (me.failures - MAX_TRIES).min(4);
                me.wait_until = now + 30.0 * (1u32 << extra) as f64;
                return Err(me.wait_until - now);
            }
            Ok(false)
        }
    })
}

/// Tries left before a wait.
pub fn tries_left() -> u32 {
    with(|me| MAX_TRIES.saturating_sub(me.failures).max(1))
}

/// Saves a new PIN and unlocks.
pub fn set_pin(pin: &str, now: f64) {
    let h = hash_pin(pin);
    with(|me| {
        me.saved.pin = Some(h);
        me.unlocked_until = now + UNLOCK_SECS;
        me.failures = 0;
        me.ver += 1;
        save(me);
    });
}

/// Turns parental controls off (the rules stay for next time).
pub fn clear_pin() {
    with(|me| {
        me.saved.pin = None;
        me.passes.clear();
        me.ver += 1;
        save(me);
    });
}

/// The current profile's key and label.
pub fn profile() -> (String, String) {
    with(|me| me.profile.clone())
}

fn rules_of(me: &Inner) -> Rules {
    me.saved.profiles.get(&me.profile.0).or_else(|| me.saved.profiles.get("default")).cloned().unwrap_or_default()
}

/// The current profile's rules.
pub fn rules() -> Rules {
    with(|me| rules_of(me))
}

/// Saves the current profile's rules (and makes them the default when there is none yet).
pub fn set_rules(r: Rules) {
    with(|me| {
        let key = me.profile.0.clone();
        if !me.saved.profiles.contains_key("default") {
            me.saved.profiles.insert("default".into(), r.clone());
        }
        me.saved.profiles.insert(key, r);
        me.ver += 1;
        save(me);
    });
}

fn used_of(me: &Inner, day: &str) -> u64 {
    me.usage.days.get(&me.profile.0).and_then(|d| d.get(day)).copied().unwrap_or(0)
}

/// Seconds played today on this profile.
pub fn played_today() -> u64 {
    let (day, _, _) = local_now();
    with(|me| used_of(me, &day))
}

// ---------------------------------------------------------------- age ratings (Steam store)

fn steam_appid(id: &str) -> Option<u32> {
    id.strip_prefix("steam:")?.parse().ok()
}

/// ESRB letters as ages.
fn esrb_age(r: &str) -> Option<u8> {
    Some(match r.to_ascii_lowercase().as_str() {
        "ec" => 3,
        "e" => 6,
        "e10" | "e10+" => 10,
        "t" => 13,
        "m" => 17,
        "ao" => 18,
        _ => return None,
    })
}

/// The age a Steam store `appdetails` answer rates the game for (the highest of required_age,
/// ESRB and PEGI). None: no rating.
pub fn rating_of(data: &serde_json::Value) -> Option<u8> {
    let num = |v: &serde_json::Value| v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().trim_end_matches('+').parse().ok()));
    let mut age: Option<u8> = data.get("required_age").and_then(num).filter(|a| *a > 0).map(|a| a.min(21) as u8);
    let r = data.get("ratings");
    if let Some(e) = r.and_then(|r| r.get("esrb")).and_then(|e| e.get("rating")).and_then(|x| x.as_str()).and_then(esrb_age) {
        age = Some(age.map_or(e, |a| a.max(e)));
    }
    if let Some(p) = r.and_then(|r| r.get("pegi")).and_then(|e| e.get("rating")).and_then(num) {
        let p = p.min(21) as u8;
        age = Some(age.map_or(p, |a| a.max(p)));
    }
    age
}

fn fetch_rating(appid: u32) -> Result<Option<u8>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(10))).http_status_as_error(false).build().into();
    let url = format!("https://store.steampowered.com/api/appdetails?appids={appid}&cc=us&l=en");
    let mut resp = agent.get(&url).header("User-Agent", "RareLauncher/0.1").call().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let text = resp.body_mut().with_config().limit(4 * 1024 * 1024).read_to_string().map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let entry = v.get(appid.to_string()).ok_or("no entry")?;
    if entry.get("success").and_then(|s| s.as_bool()) != Some(true) {
        return Ok(None);
    }
    Ok(entry.get("data").and_then(rating_of))
}

fn load_ratings(me: &mut Inner) {
    if me.ratings_loaded {
        return;
    }
    me.ratings_loaded = true;
    if fixture() {
        return;
    }
    let m: BTreeMap<String, Option<u8>> = read("parental-ratings.json");
    me.ratings.extend(m.into_iter().filter_map(|(k, v)| Some((k.parse().ok()?, v))));
}

/// Reads the ratings the cache does not have yet, one by one on a worker.
fn prefetch(me: &mut Inner, ids: Vec<u32>) {
    let want: Vec<u32> = ids.into_iter().filter(|a| !me.ratings.contains_key(a) && !me.fetching.contains(a)).collect();
    if want.is_empty() || fixture() {
        return;
    }
    me.fetching.extend(want.iter().copied());
    std::thread::Builder::new()
        .name("rare-ratings".into())
        .spawn(move || {
            for a in want {
                let r = fetch_rating(a);
                with(|me| {
                    me.fetching.remove(&a);
                    match r {
                        Ok(age) => {
                            me.ratings.insert(a, age);
                            let m: BTreeMap<String, Option<u8>> = me.ratings.iter().map(|(k, v)| (k.to_string(), *v)).collect();
                            write("parental-ratings.json", &m);
                        }
                        Err(e) => log::info!("parental: rating for {a}: {e}"),
                    }
                });
                std::thread::sleep(std::time::Duration::from_millis(400));
            }
        })
        .ok();
}

/// The game's age rating when known.
pub fn rating(id: &str) -> Option<u8> {
    let a = steam_appid(id)?;
    with(|me| {
        load_ratings(me);
        me.ratings.get(&a).copied().flatten()
    })
}

// ---------------------------------------------------------------- enforcement

fn is_game(st: &State, id: &str) -> bool {
    !id.starts_with("app:") && st.game(id).is_some()
}

/// Before a game starts: true when a rule holds it for the PIN (the sheet is up; the launch runs
/// again once the PIN is in). False: start it.
pub fn gate_launch(st: &State, id: &str, now: f64) -> bool {
    if !is_game(st, id) || !pin_set() {
        return false;
    }
    let (day, minute, _) = local_now();
    let rating = rating(id);
    let block = with(|me| {
        if me.passes.contains_key(id) {
            return Ok(None);
        }
        // another launch waits for the PIN
        if me.pending.is_some() {
            return Err(());
        }
        Ok(block_for(&rules_of(me), used_of(me, &day), minute, rating))
    });
    let b = match block {
        Ok(Some(b)) => b,
        Ok(None) => return false,
        Err(()) => return true,
    };
    let rx = ask(&b.title(), &b.reason(), now);
    with(|me| me.pending = Some((id.to_string(), rx)));
    true
}

/// Device checks only (`RR_SIGN_CHECK=1`, never in fixtures): 3 s after start, asks Rare Wallet to
/// sign a short message through the real review sheet, so the PIN step can be tried on a device.
/// Nothing is signed unless someone approves it and enters the PIN.
fn dev_sign_check(now: f64) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    let _ = now;
    if fixture() || std::env::var("RR_SIGN_CHECK").is_err() {
        return;
    }
    ONCE.call_once(|| {
        let _ = std::thread::Builder::new().name("rare-sign-check".into()).spawn(|| {
            use rare_wallet::sign::{OriginKind, SignOptions, SignOrigin};
            std::thread::sleep(std::time::Duration::from_secs(3));
            let Some(w) = crate::screens::wallet::service::wallet() else { return log::warn!("sign check: no wallet") };
            let o = SignOptions { origin: Some(SignOrigin::new("Rare device check", OriginKind::Wallet)), title: Some("Sign in".into()), ..Default::default() };
            let r = w.signing.sign_personal_message(b"Rare parental controls check".to_vec(), o).and_then(|t| t.wait());
            log::info!("sign check: {:?}", r.map(|_| "signed"));
        });
    });
}

fn notify(st: &mut State, title: &str, body: &str, now: f64) {
    log::info!("parental: notice \"{title}\"");
    st.toast_for(format!("{title}. {body}"), Tone::Info, now, 6.0);
    rare_system::hud::hud_toast(title.to_string(), Some(body.to_string()));
}

/// Every update: the profile, launches the PIN let through, play time, the notices over a game.
pub fn poll(st: &mut State, fx: &mut crate::page::Fx, now: f64) {
    dev_sign_check(now);
    let profile = st.account.as_ref().map(|a| (a.address.clone(), a.label.clone())).unwrap_or_else(|| ("default".into(), "This device".into()));
    let running: Vec<String> = st.running.iter().filter(|r| is_game(st, r)).cloned().collect();
    let launch = with(|me| {
        if me.profile != profile {
            me.profile = profile;
            me.ver += 1;
        }
        // a launch the PIN answered
        let mut launch = None;
        if let Some((id, rx)) = me.pending.take() {
            match rx.try_recv() {
                Ok(true) => {
                    me.passes.insert(id.clone(), (now, false));
                    launch = Some(id);
                }
                Ok(false) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
                Err(std::sync::mpsc::TryRecvError::Empty) => me.pending = Some((id, rx)),
            }
        }
        // passes last one session
        me.passes.retain(|id, (at, seen)| {
            let on = running.contains(id);
            if on {
                *seen = true;
            }
            if *seen {
                on
            } else {
                now - *at < 120.0
            }
        });
        launch
    });
    if let Some(id) = launch {
        fx.run(crate::act::Act::Launch(id));
    }
    play(st, &running, now);
}

fn play(st: &mut State, running: &[String], now: f64) {
    let (day, minute, sec) = local_now();
    let mut say: Option<(String, String)> = None;
    with(|me| {
        let playing = !running.is_empty();
        let last = me.last_play.replace(now);
        if !playing {
            me.last_play = None;
            if me.unsaved > 0.0 {
                save_usage(me);
            }
            return;
        }
        let dt = last.map(|l| (now - l).clamp(0.0, 60.0)).unwrap_or(0.0);
        if dt > 0.0 {
            let key = me.profile.0.clone();
            me.frac += dt;
            let whole = me.frac.floor();
            me.frac -= whole;
            *me.usage.days.entry(key).or_default().entry(day.clone()).or_insert(0) += whole as u64;
            me.unsaved += dt;
            if me.unsaved >= 30.0 {
                save_usage(me);
            }
        }
        if me.saved.pin.is_none() {
            return;
        }
        let rules = rules_of(me);
        if rules.daily_min.is_none() && rules.hours.is_none() {
            return;
        }
        // a session the PIN let through gets no notices
        if running.iter().all(|r| me.passes.contains_key(r)) {
            return;
        }
        let tag = (me.profile.0.clone(), day.clone());
        if me.noticed.0 != tag {
            me.noticed = (tag, false, None);
        }
        let Some((left, budget)) = play_left(&rules, used_of(me, &day), minute, sec) else { return };
        let ends = |up: bool| match (budget, rules.hours) {
            (false, Some((f, t))) if up => format!("Playing is allowed from {} to {}. Save your game and take a break.", hour_text(f), hour_text(t)),
            (false, Some((_, t))) => format!("Playing time ends at {}. Save your game.", hour_text(t)),
            _ if up => "Today's play time is used up. Save your game and take a break.".into(),
            _ => "Today's play time ends soon. Save your game.".into(),
        };
        if left <= 0 {
            let due = me.noticed.2.map_or(true, |at| now - at >= REMIND_SECS);
            if due {
                me.noticed.2 = Some(now);
                me.noticed.1 = true;
                say = Some(("Time is up".into(), ends(true)));
            }
        } else if left <= WARN_SECS && !me.noticed.1 {
            me.noticed.1 = true;
            let mins = (left + 59) / 60;
            say = Some((format!("{mins} {} of play time left", if mins == 1 { "minute" } else { "minutes" }), ends(false)));
        }
    });
    if let Some((t, b)) = say {
        notify(st, &t, &b, now);
    }
    // ratings for the library while an age rule is on
    let max_age = with(|me| rules_of(me).max_age.is_some() && me.saved.pin.is_some());
    if max_age {
        let ids: Vec<u32> = st.games().iter().filter_map(|g| steam_appid(&g.id)).collect();
        with(|me| {
            load_ratings(me);
            prefetch(me, ids);
        });
    }
}

/// The next time play time needs counting (every 15 s while a game runs).
pub fn next_wake(st: &State, now: f64) -> Option<f64> {
    st.running.iter().any(|r| is_game(st, r)).then_some(now + 15.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_hash_and_check() {
        assert!(valid_pin("1234") && valid_pin("123456"));
        assert!(!valid_pin("123") && !valid_pin("1234567") && !valid_pin("12a4"));
        let h = hash_pin("4821");
        assert_eq!(h.algo, "argon2id");
        assert!(!h.hash.contains("4821") && h.salt.len() == 32 && h.hash.len() == 64);
        assert!(verify_pin(&h, "4821"));
        assert!(!verify_pin(&h, "4822"));
        assert!(!verify_pin(&h, ""));
        // a fresh salt each time
        assert_ne!(hash_pin("4821").hash, h.hash);
    }

    #[test]
    fn day_budget() {
        assert_eq!(budget_left(None, 9999), None);
        assert_eq!(budget_left(Some(60), 0), Some(3600));
        assert_eq!(budget_left(Some(60), 3000), Some(600));
        assert_eq!(budget_left(Some(30), 5000), Some(0));
        let r = Rules { daily_min: Some(60), hours: Some((8, 20)), max_age: Some(12) };
        // 10:00, 50 min played: fine
        assert_eq!(block_for(&r, 3000, 600, None), None);
        assert_eq!(block_for(&r, 3600, 600, None), Some(Block::UsedUp));
        assert_eq!(block_for(&r, 0, 7 * 60 + 59, None), Some(Block::Hours(8, 20)));
        assert_eq!(block_for(&r, 0, 20 * 60, None), Some(Block::Hours(8, 20)));
        assert_eq!(block_for(&r, 0, 600, Some(17)), Some(Block::Rating(17)));
        assert_eq!(block_for(&r, 0, 600, Some(10)), None);
        // 19:55 with 50 min of the budget left: the hours end first
        assert_eq!(play_left(&r, 600, 19 * 60 + 55, 0), Some((300, false)));
        // 10:00 with 5 min of budget left
        assert_eq!(play_left(&r, 3300, 600, 0), Some((300, true)));
        // past midnight windows
        assert!(in_window(20, 2, 23 * 60) && in_window(20, 2, 60) && !in_window(20, 2, 3 * 60));
        assert_eq!(window_left(20, 2, 23 * 60, 0), Some(3 * 3600));
        assert!(in_window(0, 24, 0) && in_window(0, 24, 1439));
        assert_eq!(Block::Hours(8, 20).title(), "Playing is allowed from 8:00 to 20:00");
        assert_eq!(played_text(45 * 60), "45 min");
        assert_eq!(played_text(80 * 60), "1 h 20 min");
    }

    #[test]
    fn steam_ratings() {
        let v = serde_json::json!({ "required_age": "0", "ratings": { "esrb": { "rating": "m" }, "pegi": { "rating": "18" } } });
        assert_eq!(rating_of(&v), Some(18));
        assert_eq!(rating_of(&serde_json::json!({ "required_age": 0 })), None);
        assert_eq!(rating_of(&serde_json::json!({ "required_age": 0, "ratings": { "esrb": { "rating": "e10" } } })), Some(10));
        assert_eq!(rating_of(&serde_json::json!({ "required_age": "17" })), Some(17));
    }
}
