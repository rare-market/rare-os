//! Game knowledge from rare.market's /v1/knowledge: HowLongToBeat times, Metacritic and Steam review
//! scores, Runs on SuiPlay reports, prices across stores and free games. rare.market fetches and
//! caches the outside sources (all key-free), so the device only talks to rare.market. Blocking
//! calls: run them from a worker (`Fx::spawn`). Answers are kept on disk in
//! ~/.cache/rare/knowledge.json so a revisit or a restart shows the last one at once.
//!
//! Fixtures (RR_FIXTURE) answer from samples unless RR_KNOW_LIVE=1; RR_KNOW picks the sample set
//! (`none`, `one`, `long`, default many) for the edge-case shots.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

const ORIGIN: &str = "https://rare.market";
const UA: &str = "RareLauncher/0.1";

// ---------------------------------------------------------------- types

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Hltb {
    pub id: u64,
    pub name: String,
    pub main: Option<f64>,
    pub extras: Option<f64>,
    pub completionist: Option<f64>,
    pub url: String,
    pub polled: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Score {
    pub metacritic: Option<u32>,
    pub metacritic_url: Option<String>,
    pub steam_pct: Option<u32>,
    pub steam_label: Option<String>,
    pub steam_count: Option<u64>,
}

/// The settings a report was played with (fps and refresh None: no limit, the screen's own).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct RunSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tdp: Option<f64>,
    pub fps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upscale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    pub refresh: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proton: Option<String>,
}

impl RunSettings {
    /// "Balanced, 40 fps, FSR at 800p, 40 Hz, GE-Proton9-20" (only what the report has).
    pub fn line(&self) -> String {
        let mut v: Vec<String> = Vec::new();
        if let Some(p) = &self.power {
            v.push(match (p.as_str(), self.tdp) {
                ("Custom", Some(w)) => format!("{} W", w.round()),
                _ => p.clone(),
            });
            v.push(match self.fps {
                Some(f) => format!("{f} fps"),
                None => "no frame limit".into(),
            });
        }
        match (self.upscale.as_deref(), self.size.as_deref()) {
            (Some(u), Some(s)) if u != "Off" => v.push(format!("{u} at {s}")),
            (_, Some(s)) if s != "1200p" && s != "Native" => v.push(s.to_string()),
            _ => {}
        }
        if let Some(r) = self.refresh {
            v.push(format!("{r} Hz"));
        }
        if let Some(p) = &self.proton {
            v.push(p.clone());
        }
        v.join(", ")
    }

    pub fn is_empty(&self) -> bool {
        self.power.is_none() && self.upscale.is_none() && self.size.is_none() && self.proton.is_none()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Best {
    pub settings: RunSettings,
    pub count: u32,
    pub from: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Recent {
    pub rating: String,
    pub settings: RunSettings,
    pub device: String,
    pub at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Runs {
    pub key: String,
    pub total: u32,
    pub great: u32,
    pub playable: u32,
    pub broken: u32,
    pub verdict: Option<String>,
    pub best: Option<Best>,
    pub recent: Vec<Recent>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct GameInfo {
    pub key: Option<String>,
    pub hltb: Option<Hltb>,
    pub score: Option<Score>,
    pub runs: Option<Runs>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Deal {
    pub store: String,
    pub price: f64,
    pub retail: f64,
    pub cut: u32,
    pub url: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Lowest {
    pub price: f64,
    pub at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Price {
    pub id: String,
    pub title: String,
    pub steam_app_id: Option<u32>,
    pub price: Option<f64>,
    pub retail: Option<f64>,
    pub cut: u32,
    pub store: Option<String>,
    pub url: Option<String>,
    pub lowest: Option<Lowest>,
    pub deals: Vec<Deal>,
    pub thumb: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct FreeGame {
    pub id: String,
    /// epic, gog, steam.
    pub store: String,
    /// free (to keep) or weekend (to play for a while).
    pub kind: String,
    pub title: String,
    pub image: Option<String>,
    pub url: String,
    pub start: Option<String>,
    pub end: Option<String>,
    pub upcoming: bool,
    pub worth: Option<String>,
}

impl FreeGame {
    pub fn store_name(&self) -> &'static str {
        store_name(&self.store)
    }
}

pub fn store_name(s: &str) -> &'static str {
    match s {
        "epic" => "Epic",
        "gog" => "GOG",
        _ => "Steam",
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Hit {
    pub id: String,
    pub title: String,
    pub steam_app_id: Option<u32>,
    pub price: Option<f64>,
    pub thumb: Option<String>,
}

// ---------------------------------------------------------------- keys

fn norm(t: &str) -> String {
    let mut out = String::new();
    let mut space = false;
    for c in t.chars().filter(|c| !matches!(c, '\u{2122}' | '\u{ae}' | '\u{a9}')).flat_map(|c| c.to_lowercase()) {
        if c.is_ascii_alphanumeric() {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        } else if c == '&' {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str("and");
            space = true;
        } else {
            space = true;
        }
    }
    out
}

/// The game key rare.market files Runs on SuiPlay under: the Steam app, else the title (as the
/// server's gameKey, without the edition trim it also does: the server trims again).
pub fn game_key(steam: Option<u32>, title: &str) -> Option<String> {
    if let Some(a) = steam.filter(|a| *a > 0 && *a < 0x8000_0000) {
        return Some(format!("steam:{a}"));
    }
    let t: String = bare(&norm(title)).chars().take(80).collect();
    (!t.is_empty()).then(|| format!("title:{}", t.trim()))
}

const EDITIONS: [&str; 16] = [
    " game of the year edition",
    " goty edition",
    " definitive edition",
    " complete edition",
    " deluxe edition",
    " ultimate edition",
    " gold edition",
    " standard edition",
    " enhanced edition",
    " directors cut",
    " director s cut",
    " anniversary edition",
    " special edition",
    " digital deluxe edition",
    " premium edition",
    " edition",
];

fn bare(n: &str) -> String {
    let mut s = n.to_string();
    for _ in 0..3 {
        match EDITIONS.iter().find(|e| s.ends_with(*e)) {
            Some(e) => s.truncate(s.len() - e.len()),
            None => break,
        }
    }
    s.trim().to_string()
}

/// A wishlist / price id: `steam:<appid>` or `t:<title>`.
pub fn price_id(steam: Option<u32>, title: &str) -> String {
    match steam.filter(|a| *a > 0 && *a < 0x8000_0000) {
        Some(a) => format!("steam:{a}"),
        None => format!("t:{title}"),
    }
}

fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

// ---------------------------------------------------------------- the network

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).http_status_as_error(false).build().into()
}

fn get_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    let mut r = agent().get(&format!("{ORIGIN}{path}")).header("User-Agent", UA).call().map_err(|e| e.to_string())?;
    let st = r.status().as_u16();
    if st != 200 {
        return Err(format!("rare.market {st}"));
    }
    r.body_mut().read_json().map_err(|e| e.to_string())
}

pub fn fixture() -> bool {
    std::env::var("RR_FIXTURE").is_ok() && std::env::var("RR_KNOW_LIVE").is_err()
}

/// RR_KNOW in fixtures: none, one, long, or many (default).
pub fn scenario() -> String {
    std::env::var("RR_KNOW").unwrap_or_else(|_| "many".into())
}

// ---------------------------------------------------------------- disk cache

#[derive(Serialize, Deserialize, Default)]
struct Disk {
    #[serde(default)]
    games: HashMap<String, (i64, GameInfo)>,
    #[serde(default)]
    free: Option<(i64, Vec<FreeGame>)>,
}

static DISK: Mutex<Option<Disk>> = Mutex::new(None);

fn disk_path() -> std::path::PathBuf {
    let base = std::env::var("XDG_CACHE_HOME").map(std::path::PathBuf::from).unwrap_or_else(|_| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache"));
    base.join("rare/knowledge.json")
}

fn with_disk<R>(f: impl FnOnce(&mut Disk) -> R) -> R {
    let mut g = DISK.lock().unwrap();
    let d = g.get_or_insert_with(|| if std::env::var("RR_FIXTURE").is_ok() { Disk::default() } else { super::userlib::read_json(&disk_path()) });
    f(d)
}

fn save_disk() {
    if std::env::var("RR_FIXTURE").is_ok() {
        return;
    }
    let g = DISK.lock().unwrap();
    if let Some(d) = g.as_ref() {
        super::userlib::write_json(&disk_path(), d);
    }
}

// ---------------------------------------------------------------- game info

/// The last answer for a game and when it came (unix secs).
pub fn cached_game(id: &str) -> Option<(i64, GameInfo)> {
    if fixture() {
        return Some((super::model::now_secs(), fixture_game(id)));
    }
    with_disk(|d| d.games.get(id).cloned())
}

/// HowLongToBeat, scores and Runs on SuiPlay for a library game (blocking).
pub fn game_info(id: &str, steam: Option<u32>, title: &str) -> Result<GameInfo, String> {
    if fixture() {
        return Ok(fixture_game(id));
    }
    let mut path = String::from("/v1/knowledge/game?");
    if let Some(a) = steam {
        path.push_str(&format!("steam={a}&"));
    }
    path.push_str(&format!("title={}", enc(title)));
    let info: GameInfo = get_json(&path)?;
    with_disk(|d| {
        d.games.insert(id.to_string(), (super::model::now_secs(), info.clone()));
        if d.games.len() > 600 {
            let oldest = d.games.iter().min_by_key(|(_, v)| v.0).map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                d.games.remove(&k);
            }
        }
    });
    save_disk();
    Ok(info)
}

/// Updates the kept answer's runs after a report.
pub fn put_runs(id: &str, runs: Runs) {
    with_disk(|d| {
        if let Some((_, g)) = d.games.get_mut(id) {
            g.runs = Some(runs);
        }
    });
    save_disk();
}

// ---------------------------------------------------------------- prices, search

/// Lowest prices for wishlist ids (blocking, batches of 40).
pub fn prices(ids: &[String]) -> Result<Vec<Price>, String> {
    if fixture() {
        return Ok(ids.iter().map(|id| fixture_price(id)).collect());
    }
    let mut out = Vec::new();
    for chunk in ids.chunks(40) {
        // Titles are encoded twice: once so their commas survive the split, once for the query.
        let list: Vec<String> = chunk.iter().map(|id| match id.strip_prefix("t:") { Some(t) => format!("t:{}", enc(&enc(t))), None => id.clone() }).collect();
        #[derive(Deserialize)]
        struct R {
            data: Vec<Price>,
        }
        let r: R = get_json(&format!("/v1/knowledge/prices?ids={}", list.join(",")))?;
        out.extend(r.data);
    }
    Ok(out)
}

pub fn search(q: &str) -> Result<Vec<Hit>, String> {
    if fixture() {
        return Ok(fixture_hits(q));
    }
    #[derive(Deserialize)]
    struct R {
        data: Vec<Hit>,
    }
    let r: R = get_json(&format!("/v1/knowledge/search?q={}", enc(q)))?;
    Ok(r.data)
}

// ---------------------------------------------------------------- Runs on SuiPlay

/// Sends this device's report (signed with the friends device key). Err when friends is off here.
pub fn report(key: &str, title: &str, rating: &str, settings: &RunSettings) -> Result<Runs, String> {
    if fixture() {
        let mut r = fixture_runs("many");
        r.total += 1;
        match rating {
            "great" => r.great += 1,
            "playable" => r.playable += 1,
            _ => r.broken += 1,
        }
        return Ok(r);
    }
    let body = serde_json::json!({ "key": key, "title": title, "rating": rating, "settings": settings, "device": device_name() });
    #[derive(Deserialize)]
    struct R {
        runs: Runs,
    }
    // No friends key yet: turn friends on first (one wallet signature), then send.
    let sent = crate::social::signed_post::<R>("/v1/knowledge/runs", &body).or_else(|| crate::social::link_device_key().ok().and_then(|_| crate::social::signed_post::<R>("/v1/knowledge/runs", &body)));
    let r: R = sent.ok_or_else(|| "Sign in with Rare Wallet to send reports.".to_string())??;
    Ok(r.runs)
}

/// The device's model ("SuiPlay0X1"), from the firmware's product name.
pub fn device_name() -> String {
    let p = std::fs::read_to_string("/sys/class/dmi/id/product_name").unwrap_or_default();
    let p = p.trim();
    if p.is_empty() {
        "SuiPlay0X1".into()
    } else {
        p.chars().filter(|c| c.is_ascii_alphanumeric() || " .+-_".contains(*c)).take(40).collect()
    }
}

/// This device's own reports, kept so the page says what you said.
#[derive(Serialize, Deserialize, Default)]
struct Mine {
    #[serde(default)]
    by_key: HashMap<String, (String, i64)>,
}

fn mine_path() -> std::path::PathBuf {
    super::userlib::state_dir().join("run-reports.json")
}

pub fn my_report(key: &str) -> Option<(String, i64)> {
    if fixture() {
        return (scenario() == "long").then(|| ("playable".to_string(), super::model::now_secs() - 86400 * 3));
    }
    let m: Mine = super::userlib::read_json(&mine_path());
    m.by_key.get(key).cloned()
}

pub fn set_my_report(key: &str, rating: &str) {
    if fixture() {
        return;
    }
    let mut m: Mine = super::userlib::read_json(&mine_path());
    m.by_key.insert(key.to_string(), (rating.to_string(), super::model::now_secs()));
    super::userlib::write_json(&mine_path(), &m);
}

// ---------------------------------------------------------------- free games (app level)

static FREE: Mutex<Option<Vec<FreeGame>>> = Mutex::new(None);
static FREE_VER: AtomicU64 = AtomicU64::new(0);
static FREE_AT: Mutex<f64> = Mutex::new(0.0);

pub fn free_ver() -> u64 {
    FREE_VER.load(Ordering::Relaxed)
}

/// The free games to show (None: never loaded yet). Ended ones are dropped as time passes.
pub fn free_games() -> Option<Vec<FreeGame>> {
    let mut g = FREE.lock().unwrap();
    if g.is_none() {
        if fixture() {
            *g = Some(fixture_free());
        } else if let Some((_, l)) = with_disk(|d| d.free.clone()) {
            *g = Some(l);
        }
    }
    let now = super::model::now_secs();
    g.clone().map(|l| l.into_iter().filter(|f| f.end.as_deref().and_then(parse_iso).map_or(true, |e| e > now)).collect())
}

/// Every update: the free list refreshes every 30 minutes while the launcher is in front.
pub fn poll(fx: &mut crate::page::Fx, now: f64) {
    if fixture() {
        return;
    }
    {
        let mut at = FREE_AT.lock().unwrap();
        if !super::keep::due(*at, 1800.0, now) {
            return;
        }
        *at = now;
    }
    fx.spawn(|_| {
        #[derive(Deserialize)]
        struct R {
            data: Vec<FreeGame>,
        }
        match get_json::<R>("/v1/knowledge/free") {
            Ok(r) => {
                with_disk(|d| d.free = Some((super::model::now_secs(), r.data.clone())));
                save_disk();
                *FREE.lock().unwrap() = Some(r.data);
                FREE_VER.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => log::warn!("free games: {e}"),
        }
        Box::new(|_, _| {})
    });
}

/// "2026-10-29T15:00:00.000Z" to unix secs.
pub fn parse_iso(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let n = |a: usize, l: usize| s.get(a..a + l)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0, 4)?, n(5, 2)?, n(8, 2)?, n(11, 2)?, n(14, 2)?, n(17, 2)?);
    // Days from the civil date (Howard Hinnant's algorithm).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (mo + if mo > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86400 + h * 3600 + mi * 60 + se)
}

/// "Free until Thu" / "Free from Thu" / "Free to keep" (a week out: the date).
pub fn free_line(f: &FreeGame, now: i64) -> String {
    let when = |t: i64| {
        let d = t - now;
        if (0..86400).contains(&d) {
            let mut tm: libc::tm = unsafe { std::mem::zeroed() };
            let tt = t as libc::time_t;
            // SAFETY: localtime_r fills tm.
            unsafe { libc::localtime_r(&tt, &mut tm) };
            let h = tm.tm_hour;
            format!("{}{}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { " am" } else { " pm" })
        } else {
            day_label(t, d < 6 * 86400)
        }
    };
    let weekend = f.kind == "weekend";
    match (f.upcoming, f.start.as_deref().and_then(parse_iso), f.end.as_deref().and_then(parse_iso)) {
        (true, Some(s), _) => format!("{} from {}", if weekend { "Play free" } else { "Free" }, when(s)),
        (_, _, Some(e)) => format!("{} until {}", if weekend { "Play free" } else { "Free" }, when(e)),
        _ => if weekend { "Play free this weekend".into() } else { "Free to keep".into() },
    }
}

/// "Thu" within a week (`short`), else "Nov 3", in local time.
pub fn day_label(t: i64, short: bool) -> String {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let tt = t as libc::time_t;
    // SAFETY: localtime_r fills tm.
    unsafe { libc::localtime_r(&tt, &mut tm) };
    const D: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    if short {
        D[tm.tm_wday.clamp(0, 6) as usize].to_string()
    } else {
        format!("{} {}", M[tm.tm_mon.clamp(0, 11) as usize], tm.tm_mday)
    }
}

// ---------------------------------------------------------------- fixtures

fn fixture_runs(sc: &str) -> Runs {
    let s = |p: &str, fps: Option<u32>, up: &str, size: &str, hz: Option<u32>, proton: &str| RunSettings { power: Some(p.into()), tdp: None, fps, upscale: Some(up.into()), size: Some(size.into()), refresh: hz, proton: Some(proton.into()) };
    match sc {
        "none" => Runs::default(),
        "one" => Runs { key: "steam:1145360".into(), total: 1, great: 0, playable: 1, broken: 0, verdict: Some("playable".into()), best: Some(Best { settings: s("Balanced", Some(30), "Off", "1200p", None, "Proton 9.0-4"), count: 1, from: "playable".into() }), recent: vec![] },
        "long" => Runs { key: "steam:1145360".into(), total: 12_408, great: 9_871, playable: 2_211, broken: 326, verdict: Some("great".into()), best: Some(Best { settings: RunSettings { power: Some("Custom".into()), tdp: Some(17.0), fps: Some(40), upscale: Some("FSR".into()), size: Some("800p".into()), refresh: Some(40), proton: Some("GE-Proton10-25-very-long-build-name".into()) }, count: 4_102, from: "great".into() }), recent: vec![] },
        _ => Runs { key: "steam:1145360".into(), total: 23, great: 17, playable: 5, broken: 1, verdict: Some("great".into()), best: Some(Best { settings: s("Balanced", Some(40), "FSR", "800p", Some(40), "GE-Proton9-20"), count: 9, from: "great".into() }), recent: vec![] },
    }
}

fn fixture_game(id: &str) -> GameInfo {
    let sc = scenario();
    if sc == "none" {
        return GameInfo { key: game_key(None, id), ..Default::default() };
    }
    let long = sc == "long";
    GameInfo {
        key: Some("steam:1145360".into()),
        hltb: Some(Hltb { id: 62941, name: "Hades".into(), main: Some(if long { 1234.5 } else { 23.6 }), extras: if sc == "one" { None } else { Some(48.6) }, completionist: if sc == "one" { None } else { Some(if long { 9876.4 } else { 95.3 }) }, url: "https://howlongtobeat.com/game/62941".into(), polled: 6511 }),
        score: Some(Score { metacritic: if sc == "one" { None } else { Some(93) }, metacritic_url: None, steam_pct: Some(97), steam_label: Some(if long { "Overwhelmingly Positive".into() } else { "Very Positive".into() }), steam_count: Some(if long { 12_345_678 } else { 287_552 }) }),
        runs: Some(fixture_runs(&sc)),
    }
}

fn fixture_free() -> Vec<FreeGame> {
    let now = super::model::now_secs();
    let iso = |t: i64| {
        let days = t.div_euclid(86400);
        let secs = t.rem_euclid(86400);
        // Civil from days (Hinnant).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:00.000Z", if m <= 2 { y + 1 } else { y }, m, d, secs / 3600, (secs % 3600) / 60)
    };
    let f = |id: &str, store: &str, kind: &str, title: &str, img: &str, end: Option<i64>, up: bool| FreeGame {
        id: id.into(),
        store: store.into(),
        kind: kind.into(),
        title: title.into(),
        image: Some(img.into()),
        url: format!("https://example.invalid/{id}"),
        start: up.then(|| iso(now + 2 * 86400)),
        end: end.map(iso),
        upcoming: up,
        worth: Some("$24.99".into()),
    };
    let steam = |a: u32| format!("https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/{a}/header.jpg");
    match scenario().as_str() {
        "none" => vec![],
        "one" => vec![f("epic:1", "epic", "free", "Hades", &steam(1145360), Some(now + 3 * 86400), false)],
        "long" => vec![
            f("epic:1", "epic", "free", "The Extraordinarily Long Title of a Game That Never Ends: Definitive Remastered Edition", &steam(1145360), Some(now + 5 * 3600), false),
            f("gog:2", "gog", "free", "Celeste", &steam(504230), None, false),
            f("steam-weekend:3", "steam", "weekend", "Hollow Knight", &steam(367520), Some(now + 2 * 86400), false),
            f("epic:4", "epic", "free", "Stardew Valley", &steam(413150), Some(now + 9 * 86400), true),
        ],
        _ => vec![
            f("epic:1", "epic", "free", "Hades", &steam(1145360), Some(now + 2 * 86400), false),
            f("epic:2", "epic", "free", "Celeste", &steam(504230), Some(now + 2 * 86400), false),
            f("gog:3", "gog", "free", "Stardew Valley", &steam(413150), Some(now + 5 * 86400), false),
            f("steam-weekend:4", "steam", "weekend", "Hollow Knight", &steam(367520), Some(now + 3 * 86400), false),
            f("epic:5", "epic", "free", "Dead Cells", &steam(588650), None, true),
        ],
    }
}

fn fixture_price(id: &str) -> Price {
    let n = id.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    let retail = [19.99, 24.99, 14.99, 59.99, 9.99][(n % 5) as usize];
    let cut = [0, 75, 50, 0, 90][(n / 5 % 5) as usize];
    let price = (retail * (100 - cut) as f64 / 100.0 * 100.0).round() / 100.0;
    let title = id.strip_prefix("t:").map(String::from).unwrap_or_else(|| format!("Game {id}"));
    Price {
        id: id.into(),
        title,
        steam_app_id: id.strip_prefix("steam:").and_then(|a| a.parse().ok()),
        price: Some(price),
        retail: Some(retail),
        cut,
        store: Some(["Steam", "GOG", "Humble Store", "Fanatical", "Epic Games Store"][(n / 25 % 5) as usize].into()),
        url: Some("https://example.invalid/deal".into()),
        lowest: Some(Lowest { price: (price * 0.8 * 100.0).round() / 100.0, at: "2025-06-01T00:00:00.000Z".into() }),
        deals: vec![],
        thumb: None,
    }
}

fn fixture_hits(q: &str) -> Vec<Hit> {
    let names = ["Hollow Knight", "Hollow Knight: Silksong", "Hades", "Celeste", "Stardew Valley"];
    let ids = [367520u32, 1030300, 1145360, 504230, 413150];
    names.iter().zip(ids).filter(|(n, _)| q.is_empty() || n.to_lowercase().contains(&q.to_lowercase())).map(|(n, a)| Hit { id: format!("steam:{a}"), title: n.to_string(), steam_app_id: Some(a), price: Some(14.99), thumb: None }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_dates() {
        assert_eq!(game_key(Some(620), "Portal 2").as_deref(), Some("steam:620"));
        assert_eq!(game_key(None, "Celeste\u{2122} Deluxe Edition").as_deref(), Some("title:celeste"));
        assert_eq!(game_key(None, "Ratchet & Clank").as_deref(), Some("title:ratchet and clank"));
        assert_eq!(parse_iso("2026-10-29T15:00:00.000Z"), Some(1_793_286_000));
        assert_eq!(enc("a b,c"), "a%20b%2Cc");
        let s = RunSettings { power: Some("Balanced".into()), fps: Some(40), upscale: Some("FSR".into()), size: Some("800p".into()), refresh: Some(40), proton: Some("GE-Proton9-20".into()), ..Default::default() };
        assert_eq!(s.line(), "Balanced, 40 fps, FSR at 800p, 40 Hz, GE-Proton9-20");
    }
}
