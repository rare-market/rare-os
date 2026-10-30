//! One trophy case for everything (features/trophies.rs draws it): Epic and GOG achievements next
//! to Steam's and RetroAchievements', Rare's own milestones for every game in the library, the
//! play log they come from, the trust rare.market gives each unlock, the showcase and Pin to chain.
//!
//! - Milestones (rare-trophies::milestones) are evaluated from the play log and the store's own
//!   playtime; the play log gets 30 s credits while a game runs (`track`).
//! - Every unlock is reported to rare.market (`sync`, signed with the friends key) with the play
//!   sessions around it. rare.market answers which ones count: verified (an attested Rare OS boot
//!   and plausible), unverified (no attestation, or older than Rare's play log) or flagged (an
//!   unlock with no session, a burst at one second). Only verified ones count toward the Rare
//!   score. RetroAchievements hardcore unlocks count as verified (rcheevos checks them).
//! - Pin to chain asks rare.market for an attestation for the one trophy and mints it through
//!   Rare Trophies' `mint_attested`, with Rare's trophy cover paying gas within its limits.

use crate::screens::trophies::model::{RarityFrom, RowState, SteamAchievement, SteamGame, TrophyRow, Trust};
use crate::screens::trophies::ui::Popup;
use crate::state::State;
use rare_market::net::Method;
use rare_trophies::{badge, case as cs, milestones as ms};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Mutex;

/// Case ids at or above this are games keyed by a hash of their library id (below: Steam appids).
pub const EXT_BASE: u32 = 0x8000_0000;

/// The case's u32 id for a library game: a Steam appid as it is, anything else hashed into
/// [EXT_BASE, 0xE000_0000) (RetroAchievements games sit above).
pub fn case_id(lib_id: &str) -> u32 {
    if let Some(n) = lib_id.strip_prefix("steam:").and_then(|a| a.parse::<u32>().ok()).filter(|n| *n < EXT_BASE) {
        return n;
    }
    EXT_BASE + badge::fnv(lib_id) % 0x6000_0000
}

fn now_s() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn bg(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new().name("rr-trophy-case".into()).spawn(f).ok();
}

#[derive(Default)]
struct Shared {
    ver: u64,
    /// Epic and GOG games: library id -> achievements (as the case's game type).
    ext: HashMap<String, SteamGame>,
    ext_loading: bool,
    ext_at: Option<std::time::Instant>,
    /// What each running Epic/GOG game had unlocked at the last look (for pop-ups).
    ext_known: HashMap<String, HashSet<String>>,
    ext_polled: f64,
    /// Clock seconds of the last play credit, and what was running then.
    credited: Option<f64>,
    seeded: bool,
    syncing: bool,
    sync_at: Option<std::time::Instant>,
    texts_asked: HashSet<String>,
    pinning: HashSet<String>,
    /// A game was running at the last tick (a session just ended when it stops).
    was_running: bool,
    /// Clock seconds of the last background report.
    bg_at: Option<f64>,
}

/// Background reports: 20 s after start, after every play session, and every 15 minutes.
pub const BG_EVERY: f64 = 900.0;

/// Whether a background report is due now (and the reason is used up), and when the next is.
pub fn sync_due(running: bool, now: f64) -> (bool, bool, f64) {
    with(|s| {
        let ended = s.was_running && !running;
        s.was_running = running;
        let timer = match s.bg_at {
            None => now >= 20.0,
            Some(t) => now - t >= BG_EVERY,
        };
        let due = ended || timer;
        if due {
            s.bg_at = Some(now);
        }
        let next = s.bg_at.map_or(20.0, |t| t + BG_EVERY);
        (due, ended, next)
    })
}

static S: Mutex<Option<Shared>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Shared) -> R) -> R {
    let mut g = S.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Shared::default))
}

fn changed() {
    with(|s| s.ver += 1);
    super::bus::wake();
}

pub fn ver() -> u64 {
    with(|s| s.ver)
}

// ---------------------------------------------------------------- the library's games

/// A library game Rare gives milestones to (not emulator frontends and other apps).
pub fn eligible(g: &crate::data::model::Game) -> bool {
    const STORES: [&str; 3] = ["android:com.aurora.store", "android:org.fdroid.fdroid", "android:com.android.vending"];
    g.source != "apps" && !g.id.starts_with("app:") && !STORES.contains(&g.id.as_str())
}

fn lib_games(st: &State) -> Vec<&crate::data::model::Game> {
    st.library.as_ref().map(|l| l.games.iter().chain(l.owned.iter()).filter(|g| eligible(g)).collect()).unwrap_or_default()
}

// ---------------------------------------------------------------- Epic and GOG

fn ext_to_case(g: rare_trophies::ExtGame) -> SteamGame {
    SteamGame {
        appid: case_id(&g.id),
        name: g.name,
        total: g.total,
        unlocked: g.unlocked,
        last_unlock: g.last_unlock.map(|t| t as i64),
        source: g.source.to_string(),
        achievements: g
            .achievements
            .into_iter()
            .map(|a| SteamAchievement {
                api_name: a.api_name,
                name: a.name,
                description: a.description,
                icon_url: a.icon_url,
                icon_gray_url: a.icon_gray_url,
                hidden: a.hidden,
                unlocked: a.unlocked,
                unlock_time: a.unlock_time.map(|t| t as i64),
                global_percent: a.global_percent.map(f64::from),
                hardcore: false,
            })
            .collect(),
    }
}

fn ext_fetch(id: &str, title: &str) -> Option<SteamGame> {
    let g = if let Some(app) = id.strip_prefix("epic:") {
        rare_trophies::epic::game(app)
    } else if let Some(p) = id.strip_prefix("gog:") {
        rare_trophies::gog::game(p, title)
    } else {
        None
    }?;
    Some(ext_to_case(g))
}

/// Reads the Epic and GOG games' achievements (signed-in stores only), every 30 minutes at most.
pub fn load_ext(st: &State) {
    if std::env::var("RR_FIXTURE").is_ok() {
        return;
    }
    let epic = rare_trophies::epic::account().is_some();
    let gog = rare_trophies::gog::refresh_token().is_some();
    if !epic && !gog {
        return;
    }
    let list: Vec<(String, String)> = lib_games(st)
        .into_iter()
        .filter(|g| (epic && g.id.starts_with("epic:")) || (gog && g.id.starts_with("gog:")))
        .filter(|g| g.installed || g.last_played.unwrap_or(0) > 0)
        .take(24)
        .map(|g| (g.id.clone(), g.title.clone()))
        .collect();
    let go = with(|s| {
        if s.ext_loading || list.is_empty() || s.ext_at.is_some_and(|t| t.elapsed().as_secs() < 1800) {
            return false;
        }
        s.ext_loading = true;
        true
    });
    if !go {
        return;
    }
    bg(move || {
        let mut got = HashMap::new();
        for (id, title) in &list {
            if let Some(g) = ext_fetch(id, title) {
                got.insert(id.clone(), g);
            }
        }
        with(|s| {
            for (id, g) in &got {
                s.ext_known.insert(id.clone(), g.achievements.iter().filter(|a| a.unlocked).map(|a| a.api_name.clone()).collect());
            }
            s.ext = got;
            s.ext_loading = false;
            s.ext_at = Some(std::time::Instant::now());
        });
        changed();
    });
}

/// Epic and GOG games for the case: (library id, achievements).
pub fn ext_games() -> Vec<(String, SteamGame)> {
    with(|s| s.ext.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
}

pub fn ext_loading() -> bool {
    with(|s| s.ext_loading)
}

// ---------------------------------------------------------------- milestones

/// The milestone row for the case.
pub fn milestone_row(m: &ms::Milestone, game: &str) -> TrophyRow {
    let kind = m.kind;
    TrophyRow {
        key: m.key.clone(),
        name: m.text.name.clone(),
        description: m.text.description.clone(),
        icon_url: Some(badge::data_uri(kind, game)),
        icon_gray_url: None,
        masked: false,
        state: if m.unlocked { RowState::Unlocked } else { RowState::Locked },
        at: m.unlocked_at.map(|t| t * 1000),
        tier: Some(m.tier),
        points: Some(rare_trophies::score::points(m.tier)),
        rarity: None,
        rarity_from: None,
        def: None,
        minted: None,
        source: None,
        milestone: true,
        hardcore: false,
        trust: Trust::Pending,
        progress: m.progress.filter(|_| !m.unlocked),
        showcased: false,
        pinned: None,
    }
}

/// One library game's milestones (and the ones that just unlocked).
/// `first_unlock`: the store's earliest achievement unlock (unix seconds), which also proves a launch.
pub fn milestones(g: &crate::data::model::Game, source_complete: bool, first_unlock: Option<i64>, now: i64) -> (Vec<ms::Milestone>, Vec<ms::Milestone>) {
    let mut f = ms::facts(&g.id, g.playtime_minutes.map(|m| m.max(0) as u64), g.last_played.map(|t| t.max(0) as u64), source_complete);
    if let Some(t) = first_unlock.filter(|t| *t > 0) {
        f.launched = true;
        f.first_at = Some(f.first_at.map_or(t, |a| a.min(t)));
    }
    ms::evaluate(&g.id, &g.title, &f, now)
}

/// Trust, showcase, chain pins and Rare's rarity onto a game's rows.
pub fn apply_case(rows: &mut [TrophyRow], game: &str) {
    cs::with(|c| {
        for r in rows.iter_mut() {
            let id = cs::id(game, &r.key);
            if let Some(v) = c.trust.get(&id) {
                if !r.hardcore {
                    r.trust = match v.t.as_str() {
                        "verified" => Trust::Verified,
                        "flagged" => Trust::Flagged(v.why.clone().unwrap_or_else(|| "Didn't pass Rare's checks".into())),
                        _ => Trust::Unverified,
                    };
                }
            }
            r.showcased = c.showcase.iter().any(|p| p.game == game && p.key == r.key);
            r.pinned = c.chain.get(&id).cloned();
            if r.milestone {
                if let Some(p) = c.rarity.get(&id) {
                    r.rarity = Some(*p);
                    r.rarity_from = Some(RarityFrom::Rare);
                }
            }
        }
    });
}

// ---------------------------------------------------------------- play tracking

/// Credits play time to the running games every 30 s and evaluates their milestones; returns the
/// pop-ups for new ones, and the next wake (clock seconds).
pub fn track(st: &State, now: f64) -> (Vec<Popup>, Option<f64>) {
    if std::env::var("RR_FIXTURE").is_ok() || st.library.is_none() {
        return (vec![], None);
    }
    let t = now_s();
    // Once per run: what is already true is learned without pop-ups.
    if !with(|s| std::mem::replace(&mut s.seeded, true)) {
        let list: Vec<crate::data::model::Game> = lib_games(st).into_iter().cloned().collect();
        bg(move || {
            for g in &list {
                let _ = milestones(g, false, None, t);
            }
            changed();
        });
    }
    let running: Vec<String> = st.running.iter().filter(|id| st.game(id).is_some_and(eligible)).cloned().collect();
    let last = with(|s| s.credited);
    if running.is_empty() {
        with(|s| s.credited = None);
        return (vec![], None);
    }
    let Some(prev) = last else {
        with(|s| s.credited = Some(now));
        return (vec![], Some(now + 30.0));
    };
    if now - prev < 30.0 {
        return (vec![], Some(prev + 30.0));
    }
    let secs = ((now - prev).round() as u64).min(120);
    with(|s| s.credited = Some(now));
    ms::credit(&running, secs, t, ms::on_suiplay());
    ms::save();
    let mut pops = Vec::new();
    for id in &running {
        let Some(g) = st.game(id) else { continue };
        let (_, fresh) = milestones(g, false, None, t);
        for m in fresh {
            pops.push(Popup { minted: false, name: m.text.name.clone(), game: Some(g.title.clone()), tier: Some(m.tier), points: Some(rare_trophies::score::points(m.tier)), source: None, icon_url: Some(badge::data_uri(m.kind, &g.id)), more: 0 });
        }
    }
    // Epic and GOG don't tell anyone about unlocks: look every 2 minutes while one runs.
    let ext_running: Vec<(String, String)> = running.iter().filter(|id| id.starts_with("epic:") || id.starts_with("gog:")).filter_map(|id| st.game(id).map(|g| (id.clone(), g.title.clone()))).collect();
    if !ext_running.is_empty() && with(|s| now - s.ext_polled >= 120.0) {
        with(|s| s.ext_polled = now);
        bg(move || {
            for (id, title) in ext_running {
                let Some(g) = ext_fetch(&id, &title) else { continue };
                let new: Vec<SteamAchievement> = with(|s| {
                    let before = s.ext_known.get(&id).cloned();
                    let now_set: HashSet<String> = g.achievements.iter().filter(|a| a.unlocked).map(|a| a.api_name.clone()).collect();
                    s.ext_known.insert(id.clone(), now_set);
                    s.ext.insert(id.clone(), g.clone());
                    match before {
                        Some(b) => g.achievements.iter().filter(|a| a.unlocked && !b.contains(&a.api_name)).cloned().collect(),
                        None => vec![],
                    }
                });
                for a in new {
                    let tier = crate::screens::trophies::model::rarity_tier(a.global_percent);
                    super::trophies::push_popup_any(Popup { minted: false, name: a.name.clone(), game: Some(title.clone()), tier: Some(tier), points: Some(rare_trophies::score::points(tier)), source: None, icon_url: a.icon_url.clone(), more: 0 });
                }
                changed();
            }
        });
    }
    if !pops.is_empty() {
        changed();
        sync_soon();
    }
    (pops, Some(now + 30.0))
}

// ---------------------------------------------------------------- rare.market: names, trust, showcase

/// Asks rare.market for the milestone names written for these games (once per game).
pub fn fetch_texts(games: Vec<(String, String)>) {
    if std::env::var("RR_FIXTURE").is_ok() {
        return;
    }
    let want: Vec<(String, String)> = with(|s| games.into_iter().filter(|(id, _)| !ms::has_texts(id) && s.texts_asked.insert(id.clone())).take(60).collect());
    if want.is_empty() {
        return;
    }
    bg(move || {
        let body = json!({ "games": want.iter().map(|(id, title)| json!({ "game": id, "title": title })).collect::<Vec<_>>() });
        let Ok(v) = crate::social::market_send::<Value>(Method::Post, "/v1/trophies/milestones", Some(&body)) else { return };
        let Some(map) = v.get("games").and_then(Value::as_object) else { return };
        for (id, set) in map {
            let texts: BTreeMap<String, ms::Text> = set.as_object().map(|o| o.iter().filter_map(|(k, t)| Some((k.clone(), ms::Text { name: t.get("name")?.as_str()?.to_string(), description: t.get("description")?.as_str()?.to_string() }))).collect()).unwrap_or_default();
            if !texts.is_empty() {
                ms::set_texts(id, texts);
            }
        }
        changed();
    });
}

/// One game's unlocks as rare.market's report takes them.
#[derive(Clone, Debug)]
pub struct Report {
    pub game: String,
    pub name: String,
    pub source: String,
    pub unlocks: Vec<(String, Option<i64>, bool, u8)>,
}

fn report_hash(r: &Report, sessions: &[(i64, i64)]) -> u64 {
    crate::page::hash((&r.game, &r.unlocks, sessions))
}

/// This device's attestation for trophies (rare-attest, docs/ATTEST.md): the claim token while it is
/// valid (10 minutes), else a fresh one; None with why when the TPM can't be used here. rare.market
/// counts unlocks only from a device at the `rare-os` level or above.
fn device_claim(address: &str) -> (Option<String>, Option<String>) {
    static CLAIM: Mutex<Option<(String, String, u64)>> = Mutex::new(None);
    let now_ms = (now_s() as u64) * 1000;
    if let Some((a, t, exp)) = CLAIM.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        if a == address && exp > now_ms + 90_000 {
            return (Some(t), None);
        }
    }
    match rare_attest::status() {
        rare_attest::TpmStatus::Ready => {}
        rare_attest::TpmStatus::NoAccess => return (None, Some("Rare can't open this device's TPM yet".into())),
        _ => return (None, Some("this device has no TPM Rare can use".into())),
    }
    match rare_attest::attest(&rare_attest::Endpoint::rare_market(false), &rare_attest::Request { address: address.to_string(), aud: Some("trophies".into()) }) {
        Ok(a) => {
            *CLAIM.lock().unwrap_or_else(|e| e.into_inner()) = Some((address.to_string(), a.token.clone(), a.expires_ms));
            (Some(a.token), None)
        }
        Err(e) => (None, Some(e.message())),
    }
}

fn sync_soon() {
    with(|s| s.sync_at = None);
}

/// Sends the games whose unlocks changed since the last report (every 2 minutes at most) and keeps
/// rare.market's verdicts. Needs friends on (the device key signs it).
pub fn sync(reports: Vec<Report>, force: bool) {
    let showcase_changed = force;
    if std::env::var("RR_FIXTURE").is_ok() || crate::social::device_key().is_none() {
        return;
    }
    let go = with(|s| {
        if s.syncing || (!showcase_changed && s.sync_at.is_some_and(|t| t.elapsed().as_secs() < 120)) {
            return false;
        }
        s.syncing = true;
        s.sync_at = Some(std::time::Instant::now());
        true
    });
    if !go {
        return;
    }
    bg(move || {
        let sessions = |g: &str| ms::with_log(|l| l.games.get(g).map(|p| p.sessions.clone()).unwrap_or_default());
        // The account the reports are signed for (the friends key's).
        let address = crate::social::device_key().map(|k| rare_sui::ptb::norm_addr(&k.address)).unwrap_or_default();
        let (claim, why) = if address.is_empty() { (None, Some("no Rare account".to_string())) } else { device_claim(&address) };
        let device = json!({ "product": if ms::on_suiplay() { "SuiPlay0X1" } else { "other" }, "attest": claim, "why": why });
        let mut any = false;
        for r in &reports {
            let ses = sessions(&r.game);
            let h = report_hash(r, &ses);
            if cs::with(|c| c.sent.get(&r.game) == Some(&h)) {
                continue;
            }
            for chunk in r.unlocks.chunks(150) {
                let body = json!({
                    "device": device,
                    "game": r.game,
                    "name": r.name,
                    "source": r.source,
                    "sessions": ses.iter().rev().take(120).collect::<Vec<_>>(),
                    "unlocks": chunk.iter().map(|(k, at, hc, tier)| json!([k, at, hc, tier])).collect::<Vec<_>>(),
                });
                match crate::social::signed_send::<Value>(Method::Post, "/v1/trophies/reports", Some(&body)) {
                    Some(Ok(v)) => {
                        any = true;
                        keep_verdicts(&r.game, &v);
                    }
                    Some(Err(e)) => {
                        log::warn!("trophy report {}: {e}", r.game);
                        break;
                    }
                    None => break,
                }
            }
            cs::with(|c| {
                c.sent.insert(r.game.clone(), h);
            });
        }
        if any || showcase_changed {
            put_case();
        }
        cs::save();
        with(|s| s.syncing = false);
        changed();
    });
}

fn keep_verdicts(game: &str, v: &Value) {
    cs::with(|c| {
        if let Some(map) = v.get("trust").and_then(Value::as_object) {
            for (k, t) in map {
                let verdict = cs::Verdict { t: t.get("t").and_then(Value::as_str).unwrap_or("unverified").to_string(), why: t.get("why").and_then(Value::as_str).map(String::from) };
                c.trust.insert(cs::id(game, k), verdict);
            }
        }
        if let Some(map) = v.get("rarity").and_then(Value::as_object) {
            for (k, p) in map {
                if let Some(p) = p.as_f64() {
                    c.rarity.insert(cs::id(game, k), p);
                }
            }
        }
        if let Some(sc) = v.get("score").and_then(Value::as_u64) {
            c.score = Some(sc);
        }
        c.device_note = v.get("device").and_then(|d| d.get("note")).and_then(Value::as_str).map(String::from);
        c.synced_at = Some(now_s());
    });
}

/// The showcase on rare.market (/u/<address> shows it).
fn put_case() {
    let body = cs::with(|c| json!({ "showcase": c.showcase }));
    if let Some(Ok(v)) = crate::social::signed_send::<Value>(Method::Put, "/v1/trophies/case", Some(&body)) {
        if let Some(sc) = v.get("score").and_then(Value::as_u64) {
            cs::with(|c| c.score = Some(sc));
        }
    }
}

// ---------------------------------------------------------------- showcase, complete, pin

/// A trophy on or off the profile's showcase.
pub fn toggle_showcase(p: cs::Pin) -> bool {
    let on = cs::toggle_showcase(p);
    changed();
    bg(|| {
        if crate::social::device_key().is_some() {
            put_case();
            cs::save();
        }
    });
    on
}

pub fn set_complete(lib_id: &str, on: bool) {
    ms::set_marked(lib_id, on, now_s());
    changed();
    sync_soon();
}

pub fn pinning(game: &str, key: &str) -> bool {
    with(|s| s.pinning.contains(&cs::id(game, key)))
}

/// Pin to chain: rare.market attests the one trophy (after its checks), then Rare Wallet signs
/// `mint_attested` with Rare's trophy cover. Never automatic.
pub fn pin_to_chain(p: cs::Pin) {
    let id = cs::id(&p.game, &p.key);
    if !with(|s| s.pinning.insert(id.clone())) {
        return;
    }
    changed();
    bg(move || {
        let r = pin_blocking(&p);
        with(|s| s.pinning.remove(&id));
        match r {
            Ok(digest) => {
                cs::with(|c| {
                    c.chain.insert(id.clone(), digest);
                });
                cs::save();
                super::trophies::push_popup_any(Popup { minted: true, name: p.name.clone(), game: Some(p.game_name.clone()), tier: Some(p.tier), points: Some(rare_trophies::score::points(p.tier)), source: Some(1), icon_url: p.icon_url.clone(), more: 0 });
            }
            Err(e) if e == rare_market::inventory::notice::SIGN_REJECTED => super::bus::note("Pin cancelled", false),
            Err(e) => super::bus::note(format!("Couldn't pin it to chain. {e}"), true),
        }
        changed();
    });
}

fn pin_blocking(p: &cs::Pin) -> Result<String, String> {
    let cfg = rare_sui::network::trophy_config().ok_or("Pin to chain isn't set up in this build.")?;
    let acct = crate::sign::account().filter(|a| a.signer).ok_or(crate::sign::NO_SIGNER)?;
    let image = match ms::Kind::from_key(&p.key).filter(|_| p.source == "rare") {
        Some(k) => badge::url(k, &p.game),
        None => p.icon_url.clone().filter(|u| u.starts_with("https://")).unwrap_or_default(),
    };
    let body = json!({ "player": rare_sui::ptb::norm_addr(&acct.address), "game": p.game, "gameName": p.game_name, "key": p.key, "name": p.name, "description": p.description, "imageUrl": image, "tier": p.tier, "source": p.source, "unlockedAt": p.unlocked_at });
    let v: Value = match crate::social::signed_send(Method::Post, "/v1/trophies/pin", Some(&body)) {
        Some(r) => r?,
        None => {
            crate::social::link_device_key()?;
            crate::social::signed_send(Method::Post, "/v1/trophies/pin", Some(&body)).ok_or("Turn on friends to pin trophies")??
        }
    };
    let n = |k: &str| v.get(k).and_then(|x| x.as_str().map(String::from).or_else(|| x.as_u64().map(|n| n.to_string()))).and_then(|s| s.parse::<u64>().ok()).ok_or_else(|| format!("rare.market sent no {k}"));
    use base64::Engine;
    let signature = base64::engine::general_purpose::STANDARD.decode(v.get("signature").and_then(Value::as_str).unwrap_or_default()).map_err(|_| "The attestation signature is not readable".to_string())?;
    let game = v.get("game").and_then(Value::as_str).ok_or("rare.market sent no game")?.to_string();
    if rare_sui::ptb::norm_addr(v.get("registry").and_then(Value::as_str).unwrap_or_default()) != rare_sui::ptb::norm_addr(&cfg.registry) {
        return Err("The attestation is for a different registry".into());
    }
    let att = super::trophies::Attestation { trophy_id: n("trophyId")?, nonce: n("nonce")?, expires_ms: n("expiresMs")?, signature };
    let profile = super::trophies::profile_id_of(&acct.address)?;
    let tx = super::trophies::build_claim_tx(&cfg.package, &cfg.registry, &game, &acct.address, profile.as_deref(), &[att]);
    crate::sign::sign_for_game(tx, &acct.address, None, "Rare Trophies", &format!("Pin {} to chain", p.name), true)
}

// ---------------------------------------------------------------- fixture

/// Shots and tests: a play log, an Epic game's achievements, a showcase and rare.market's verdicts
/// in a scratch folder (never the real case).
pub fn seed_fixture() {
    let dir = std::env::temp_dir().join(format!("rare-trophies-fixture-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    ms::set_path(dir.join("plays.json"));
    cs::set_path(dir.join("case.json"));
    let t0 = 1_761_000_000;
    // Hades II: eight days in a row on the SuiPlay, three hours a day.
    for d in 0..8 {
        ms::credit(&["steam:1145350".to_string()], 3 * 3600, t0 + d * 86_400, true);
    }
    ms::credit(&["epic:Fortnite".to_string()], 2 * 3600, t0 + 3 * 86_400, true);
    ms::credit(&["steam:367520".to_string()], 600, t0 + 5 * 86_400, false);
    let mut texts = BTreeMap::new();
    for (k, n, dsc) in [("first", "First quest", "Started Hades II for the first time."), ("h10", "Ten hours in Hades II", "Played Hades II for 10 hours."), ("streak7", "A week of Hades II", "Played Hades II seven days in a row."), ("suiplay", "Hades II on SuiPlay", "Played Hades II on a SuiPlay0X1.")] {
        texts.insert(k.to_string(), ms::Text { name: n.into(), description: dsc.into() });
    }
    ms::set_texts("steam:1145350", texts);
    let ach = |k: &str, name: &str, unlocked: bool, pct: f64| SteamAchievement { api_name: k.into(), name: name.into(), description: format!("{name}: done in Alan Wake 2."), unlocked, unlock_time: unlocked.then_some(t0 + 3 * 86_400), global_percent: Some(pct), ..Default::default() };
    let epic = SteamGame {
        appid: case_id("epic:Fortnite"),
        name: "Alan Wake 2".into(),
        achievements: vec![ach("A1", "Return", true, 71.2), ach("A2", "Deerfest", true, 22.4), ach("A3", "Night Springs", false, 6.1), ach("A4", "Final Draft", false, 0.9)],
        total: 4,
        unlocked: 2,
        last_unlock: Some(t0 + 3 * 86_400),
        source: "epic".into(),
    };
    with(|s| {
        s.ext.insert("epic:Fortnite".into(), epic);
        s.seeded = true;
    });
    cs::with(|c| {
        let v = |t: &str, why: Option<&str>| cs::Verdict { t: t.into(), why: why.map(String::from) };
        for k in ["rare:first", "rare:h1", "rare:h10", "rare:streak7", "rare:suiplay"] {
            c.trust.insert(cs::id("steam:1145350", k), v("verified", None));
        }
        c.trust.insert(cs::id("steam:1145360", "ACH_0"), v("verified", None));
        c.trust.insert(cs::id("steam:1145360", "ACH_1"), v("verified", None));
        c.trust.insert(cs::id("steam:1145360", "ACH_2"), v("flagged", Some("5 unlocks at the same second")));
        c.trust.insert(cs::id("epic:Fortnite", "A1"), v("verified", None));
        c.trust.insert(cs::id("epic:Fortnite", "A2"), v("unverified", Some("Unlocked before Rare")));
        c.rarity.insert(cs::id("steam:1145350", "rare:streak7"), 8.4);
        c.rarity.insert(cs::id("steam:1145350", "rare:h10"), 41.0);
        c.synced_at = Some(t0);
        c.showcase = vec![
            cs::Pin { game: "steam:1145350".into(), game_name: "Hades II".into(), key: "rare:streak7".into(), name: "A week of Hades II".into(), description: "Played Hades II seven days in a row.".into(), icon_url: Some(badge::data_uri(ms::Kind::Streak7, "steam:1145350")), tier: 1, source: "rare".into(), unlocked_at: Some(t0) },
            cs::Pin { game: "epic:Fortnite".into(), game_name: "Alan Wake 2".into(), key: "A1".into(), name: "Return".into(), description: "Return: done in Alan Wake 2.".into(), icon_url: None, tier: 0, source: "epic".into(), unlocked_at: Some(t0) },
        ];
        c.chain.insert(cs::id("steam:1145350", "rare:suiplay"), "fixture".into());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_ids() {
        assert_eq!(case_id("steam:10180"), 10180);
        let s = case_id("steam:3000000000");
        assert!((EXT_BASE..0xE000_0000).contains(&s));
        let e = case_id("epic:Hades");
        assert!((EXT_BASE..0xE000_0000).contains(&e) && e != s);
        assert_eq!(case_id("epic:Hades"), e);
    }

    #[test]
    fn milestone_rows_look_right() {
        let m = ms::Milestone { kind: ms::Kind::Hours10, key: "rare:h10".into(), text: ms::template(ms::Kind::Hours10, "Hades"), tier: 1, unlocked: false, unlocked_at: None, progress: Some((4, 10)), done: None };
        let r = milestone_row(&m, "steam:1145360");
        assert_eq!((r.state, r.tier, r.points, r.milestone, r.progress), (RowState::Locked, Some(1), Some(30), true, Some((4, 10))));
        assert!(r.icon_url.unwrap().starts_with("data:image/svg+xml"));
    }
}

