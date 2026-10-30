//! Trophies on the native launcher: the case (Overlay::Other("trophies")), the game page entry,
//! the profile shelf and the unlock pop-ups. Local Steam achievements come from rare-trophies, the
//! chain side (registry, games, profile, minted trophies) is read over Sui gRPC the way
//! trophies/chain.ts does, and a claim is attest (rare.market) then one PTB signed through the
//! wallet's `TxSigner` (trophies/attest.ts).

use crate::act::Act;
use crate::page::{hash, Acts, Fx, Page};
use crate::screens::sui::profile::Profile;
use crate::screens::trophies::model::{case_summary, game_rows, summarize, ChainDef, ChainGame, ChainProfile, ChainTrophy, SteamAchievement, SteamGame};
use crate::screens::trophies::ui::{self, case_view, sort_games, CaseGame, CaseProps, CaseState, Msg, Popup};
use crate::screens::wallet::live;
use crate::screens::wallet::render::{build, Built};
use crate::state::State;
use rare_sui::grpc::{ChainRead, Include};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

/// Most attestations one transaction carries.
const MAX_PER_TX: usize = 25;
const CLAIM_STEAM_SOURCE: u8 = 1;

#[derive(Default)]
pub struct Shared {}

// ---------------------------------------------------------------- the store

#[derive(Clone, Debug, Default)]
pub struct Registry {
    pub id: String,
    pub paused: bool,
    pub issuer_key_set: bool,
    pub games_table: String,
    pub profiles_table: String,
}

#[derive(Clone, Debug)]
pub struct GameInfo {
    pub game: ChainGame,
    pub attested: bool,
    pub paused: bool,
}

struct Pop {
    id: u64,
    p: Popup,
}

#[derive(Default)]
struct Store {
    ver: u64,
    steam: Option<Vec<SteamGame>>,
    steam_loading: bool,
    /// The last run's Steam list was put in `steam` (once), so the case draws whole at once.
    seeded: bool,
    address: Option<String>,
    registry: Option<Registry>,
    profile: Option<ChainProfile>,
    profile_id: Option<String>,
    minted: Vec<ChainTrophy>,
    chain_loading: bool,
    chain_at: Option<std::time::Instant>,
    games: HashMap<u32, Option<GameInfo>>,
    claiming: Vec<(u32, Vec<String>)>,
    popups: VecDeque<Pop>,
    pop_seq: u64,
    shown_at: Option<f64>,
    /// the case was asked to open at this game
    open_appid: Option<u32>,
    fixture: bool,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut g = STORE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Store::default))
}

fn changed(s: &mut Store) {
    s.ver += 1;
    super::bus::wake();
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Asks the case to open at a game (None: the grid). Read once by `CasePage`.
pub fn set_open(appid: Option<u32>) {
    with(|s| s.open_appid = appid);
}

// ---------------------------------------------------------------- local Steam

fn steam_game(g: rare_trophies::steam::GameAchievements) -> SteamGame {
    SteamGame {
        appid: g.appid,
        name: g.name,
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
                global_percent: a.global_percent.map(|p| p as f64),
                hardcore: false,
            })
            .collect(),
        total: g.total,
        unlocked: g.unlocked,
        last_unlock: g.last_unlock.map(|t| t as i64),
        source: g.source.to_string(),
    }
}

fn bg(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new().name("rr-trophies".into()).spawn(f).ok();
}

/// The kept Steam list, 'trophies.steam': the case opens with the last run's games and fills in place.
const KEPT_STEAM: &str = "trophies.steam";

fn seed(s: &mut Store) {
    if s.seeded || s.fixture {
        return;
    }
    s.seeded = true;
    if s.steam.is_none() {
        s.steam = crate::data::keep::load::<Vec<SteamGame>>(KEPT_STEAM).filter(|l| !l.is_empty());
    }
}

pub fn load_steam() {
    if with(|s| {
        seed(s);
        s.fixture || std::mem::replace(&mut s.steam_loading, true)
    }) {
        return;
    }
    bg(|| {
        // What the files say first (cached percentages), then again once Steam's percentages are in.
        let list: Vec<SteamGame> = rare_trophies::trophies_steam_cached().into_iter().map(steam_game).collect();
        with(|s| {
            s.steam = Some(list);
            changed(s);
        });
        let list: Vec<SteamGame> = rare_trophies::trophies_steam().into_iter().map(steam_game).collect();
        crate::data::keep::save(KEPT_STEAM, &list);
        with(|s| {
            s.steam = Some(list);
            s.steam_loading = false;
            changed(s);
        });
    });
}

// ---------------------------------------------------------------- chain reads (trophies/chain.ts)

fn id_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if s.starts_with("0x") => Some(rare_sui::ptb::norm_addr(s)),
        Value::Object(o) => o.get("id").or_else(|| o.get("bytes")).and_then(id_of),
        _ => None,
    }
}

fn num(v: Option<&Value>) -> u64 {
    match v {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

fn string(v: Option<&Value>) -> String {
    v.and_then(|v| v.as_str()).unwrap_or_default().to_string()
}

fn boolean(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Bool(true))) || v.and_then(|v| v.as_str()) == Some("true")
}

fn tier(v: Option<&Value>) -> u8 {
    num(v).min(3) as u8
}

async fn read_objects(sui: &dyn ChainRead, ids: &[String]) -> Vec<Option<Value>> {
    let mut out = Vec::new();
    for chunk in ids.chunks(50) {
        match sui.objects(chunk, Include::JSON).await {
            Ok(list) => out.extend(list.into_iter().map(|r| r.ok().and_then(|o| o.json))),
            Err(_) => out.extend(chunk.iter().map(|_| None)),
        }
    }
    out
}

fn key_string(s: &str) -> Vec<u8> {
    rare_sui::ptb::bcs::string(s)
}

fn field_id(table: &str, kind: &str, key: &[u8]) -> Option<String> {
    let t = match kind {
        "string" => "0x1::string::String",
        "address" => "address",
        _ => "u64",
    };
    rare_sui::kiosk::dynamic_field_id(table, t, key).ok()
}

async fn read_registry(sui: &dyn ChainRead, registry: &str) -> Result<Registry, String> {
    let j = read_objects(sui, &[registry.to_string()]).await.into_iter().next().flatten().ok_or("Rare Trophies registry not found")?;
    let games = j.get("games").and_then(id_of);
    let profiles = j.get("profiles").and_then(id_of);
    let (Some(games_table), Some(profiles_table)) = (games, profiles) else { return Err("Unexpected registry shape".into()) };
    let key = j.get("issuer_key");
    let issuer_key_set = match key {
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::String(s)) => !s.is_empty(),
        _ => false,
    };
    Ok(Registry { id: rare_sui::ptb::norm_addr(registry), paused: boolean(j.get("paused")), issuer_key_set, games_table, profiles_table })
}

fn parse_def(trophy_id: u32, j: &Value) -> ChainDef {
    ChainDef {
        trophy_id,
        key: string(j.get("key")),
        name: string(j.get("name")),
        description: string(j.get("description")),
        image_url: string(j.get("image_url")),
        tier: tier(j.get("tier")),
        points: num(j.get("points")) as u32,
        hidden: boolean(j.get("hidden")),
        unlocked: num(j.get("unlocked")),
    }
}

async fn read_game(sui: &dyn ChainRead, id: &str, game_json: Option<Value>) -> Option<GameInfo> {
    let j = game_json?;
    let table = j.get("defs").and_then(id_of);
    let count = num(j.get("defs").and_then(|d| d.get("size"))) as usize;
    let ids: Vec<String> = match &table {
        Some(t) => (0..count).filter_map(|i| field_id(t, "u64", &(i as u64).to_le_bytes())).collect(),
        None => Vec::new(),
    };
    let fields = read_objects(sui, &ids).await;
    let defs = fields.iter().enumerate().filter_map(|(i, f)| f.as_ref().and_then(|f| f.get("value")).filter(|v| v.is_object()).map(|v| parse_def(i as u32, v))).collect();
    Some(GameInfo {
        game: ChainGame { id: rare_sui::ptb::norm_addr(id), name: string(j.get("name")), players: num(j.get("players")), defs },
        attested: boolean(j.get("attested")),
        paused: boolean(j.get("paused")),
    })
}

fn parse_trophy(object_id: &str, j: &Value) -> Option<ChainTrophy> {
    let game = j.get("game").and_then(id_of)?;
    Some(ChainTrophy {
        object_id: object_id.to_string(),
        game,
        trophy_id: num(j.get("trophy_id")) as u32,
        name: string(j.get("name")),
        tier: tier(j.get("tier")),
        points: num(j.get("points")) as u32,
        source: if num(j.get("source")) == 0 { 0 } else { 1 },
        number: num(j.get("number")),
        unlocked_at_ms: num(j.get("unlocked_at_ms")) as i64,
    })
}

/// Everything the case needs from chain for `address` (blocking; call off the UI thread).
async fn chain_load(address: &str, appids: &[u32]) -> Result<(Registry, Option<String>, Option<ChainProfile>, Vec<ChainTrophy>, HashMap<u32, Option<GameInfo>>), String> {
    let cfg = rare_sui::network::trophy_config().ok_or("Claiming isn't set up in this build.")?;
    let sui = live::sui().ok_or("No network")?;
    let registry = read_registry(sui, &cfg.registry).await?;
    let addr = rare_sui::ptb::norm_addr(address);
    let profile_field = field_id(&registry.profiles_table, "address", &rare_sui::ptb::bcs::address(&addr));
    let (profile_f, owned) = tokio::join!(
        async {
            match profile_field {
                Some(f) => read_objects(sui, &[f]).await.into_iter().next().flatten(),
                None => None,
            }
        },
        sui.owned_objects(&addr, Some(&rare_sui::network::trophy_type(&cfg.package)), 2000, Include::JSON)
    );
    let profile_id = profile_f.and_then(|f| f.get("value").and_then(id_of));
    let profile = match &profile_id {
        Some(p) => read_objects(sui, std::slice::from_ref(p)).await.into_iter().next().flatten().map(|j| ChainProfile {
            points: num(j.get("points")),
            trophies: num(j.get("trophies")),
            bronze: num(j.get("bronze")),
            silver: num(j.get("silver")),
            gold: num(j.get("gold")),
            platinum: num(j.get("platinum")),
            self_reported_points: num(j.get("self_reported_points")),
        }),
        None => None,
    };
    let minted: Vec<ChainTrophy> = owned.unwrap_or_default().iter().filter_map(|o| o.json.as_ref().and_then(|j| parse_trophy(&o.object_id, j))).collect();
    // Every Steam game with achievements: is it on Rare Trophies, and with which trophies.
    let fields: Vec<(u32, String)> = appids.iter().filter_map(|a| field_id(&registry.games_table, "string", &key_string(&format!("steam:{a}"))).map(|f| (*a, f))).collect();
    let field_json = read_objects(sui, &fields.iter().map(|(_, f)| f.clone()).collect::<Vec<_>>()).await;
    let ids: Vec<(u32, String)> = fields.iter().zip(field_json).filter_map(|((a, _), f)| f.and_then(|f| f.get("value").and_then(id_of)).map(|id| (*a, id))).collect();
    let game_json = read_objects(sui, &ids.iter().map(|(_, i)| i.clone()).collect::<Vec<_>>()).await;
    let mut games: HashMap<u32, Option<GameInfo>> = appids.iter().map(|a| (*a, None)).collect();
    for ((a, id), j) in ids.iter().zip(game_json) {
        games.insert(*a, read_game(sui, id, j).await);
    }
    Ok((registry, profile_id, profile, minted, games))
}

/// The player's trophy Profile id, or None when they have none yet (a Local API trophy claim
/// creates it in the same transaction). Blocking.
pub(super) fn profile_id_of(address: &str) -> Result<Option<String>, String> {
    let cfg = rare_sui::network::trophy_config().ok_or("Rare Trophies isn't live yet")?;
    let sui = live::sui().ok_or("No network")?;
    live::rt().block_on(async {
        let registry = read_registry(sui, &cfg.registry).await?;
        let addr = rare_sui::ptb::norm_addr(address);
        let Some(f) = field_id(&registry.profiles_table, "address", &rare_sui::ptb::bcs::address(&addr)) else { return Ok(None) };
        Ok(read_objects(sui, &[f]).await.into_iter().next().flatten().and_then(|f| f.get("value").and_then(id_of)))
    })
}

/// Loads the chain side for the current account once a minute (and when forced).
pub fn load_chain(force: bool) {
    let Some(address) = account_address() else {
        with(|s| {
            if s.address.is_some() {
                s.address = None;
                s.profile = None;
                s.profile_id = None;
                s.minted.clear();
                changed(s);
            }
        });
        return;
    };
    let go = with(|s| {
        if s.fixture || s.chain_loading || rare_sui::network::trophy_config().is_none() {
            return false;
        }
        if !force && s.address.as_deref() == Some(&address) && s.chain_at.is_some_and(|t| t.elapsed().as_secs() < 60) {
            return false;
        }
        if s.address.as_deref() != Some(&address) {
            s.profile = None;
            s.profile_id = None;
            s.minted.clear();
        }
        s.chain_loading = true;
        s.address = Some(address.clone());
        changed(s);
        true
    });
    if !go {
        return;
    }
    bg(move || {
        // Steam first: the games to look up.
        if with(|s| s.steam.is_none()) {
            with(|s| s.steam_loading = true);
            let list: Vec<SteamGame> = rare_trophies::trophies_steam().into_iter().map(steam_game).collect();
            with(|s| {
                s.steam = Some(list);
                s.steam_loading = false;
            });
        }
        let appids: Vec<u32> = with(|s| s.steam.as_ref().map(|l| l.iter().map(|g| g.appid).collect()).unwrap_or_default());
        let r = live::rt().block_on(chain_load(&address, &appids));
        with(|s| {
            s.chain_loading = false;
            match r {
                Ok((reg, pid, profile, minted, games)) => {
                    if s.address.as_deref() == Some(&address) {
                        s.registry = Some(reg);
                        s.profile_id = pid;
                        s.profile = profile;
                        s.minted = minted;
                        s.games = games;
                        s.chain_at = Some(std::time::Instant::now());
                    }
                }
                Err(e) => log::warn!("trophies chain: {e}"),
            }
            changed(s);
        });
    });
}

fn account_address() -> Option<String> {
    if std::env::var("RR_FIXTURE").is_ok() {
        return None;
    }
    let (accounts, selected) = crate::screens::wallet::page::load_accounts();
    selected.as_ref().and_then(|s| accounts.iter().find(|a| &a.address == s)).or(accounts.first()).map(|a| a.address.clone())
}

// ---------------------------------------------------------------- claim (trophies/attest.ts)

/// Where claiming stands for a game, or None when Claim works (`claimBlocker`). Empty when
/// claiming isn't open at all: then nothing about claiming is shown.
fn claim_blocker(s: &Store, appid: u32) -> Option<String> {
    if rare_sui::network::trophy_config().is_none() && !s.fixture {
        return Some(String::new());
    }
    if s.address.is_none() && !s.fixture {
        return Some("Add a Rare Wallet account to claim".into());
    }
    if let Some(r) = &s.registry {
        if r.paused {
            return Some("Claims are paused".into());
        }
        if !r.issuer_key_set {
            return Some(String::new());
        }
    }
    let Some(g) = s.games.get(&appid) else { return Some("Checking Rare Trophies".into()) };
    let Some(g) = g else { return Some("This game is not on Rare Trophies yet".into()) };
    if !g.attested {
        return Some("This game's studio issues its own trophies".into());
    }
    if g.paused {
        return Some("Claims for this game are paused".into());
    }
    None
}

pub(super) struct Attestation {
    pub(super) trophy_id: u64,
    pub(super) nonce: u64,
    pub(super) expires_ms: u64,
    pub(super) signature: Vec<u8>,
}

fn parse_attestation(reply: &Value, registry: &str, game: &str, trophy_id: u32, now: i64) -> Result<Attestation, String> {
    let status = reply.get("status").and_then(|s| s.as_u64()).unwrap_or(0);
    let body = reply.get("body").cloned().unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        let msg = body.get("error").or_else(|| body.get("message")).and_then(|v| v.as_str()).map(String::from);
        return Err(msg.unwrap_or_else(|| if status == 429 { "Too many claims right now. Try again in a minute.".into() } else { format!("The attestation service answered {status}") }));
    }
    let norm = |k: &str| body.get(k).and_then(|v| v.as_str()).map(rare_sui::ptb::norm_addr).unwrap_or_default();
    if norm("registry") != rare_sui::ptb::norm_addr(registry) {
        return Err("The attestation is for a different registry".into());
    }
    if norm("game") != rare_sui::ptb::norm_addr(game) {
        return Err("The attestation is for a different game".into());
    }
    let big = |k: &str, what: &str| -> Result<u64, String> {
        match body.get(k) {
            Some(Value::String(s)) if s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty() => s.parse().map_err(|_| format!("The attestation has no valid {what}")),
            Some(Value::Number(n)) => n.as_u64().ok_or_else(|| format!("The attestation has no valid {what}")),
            _ => Err(format!("The attestation has no valid {what}")),
        }
    };
    let tid = big("trophyId", "trophy id")?;
    if tid != trophy_id as u64 {
        return Err("The attestation is for a different trophy".into());
    }
    let expires_ms = big("expiresMs", "expiry")?;
    if expires_ms <= now as u64 {
        return Err("The attestation has expired. Try again.".into());
    }
    use base64::Engine;
    let signature = base64::engine::general_purpose::STANDARD.decode(body.get("signature").and_then(|v| v.as_str()).unwrap_or_default()).map_err(|_| "The attestation signature is not readable".to_string())?;
    if signature.len() != 64 {
        return Err("The attestation signature is not readable".into());
    }
    Ok(Attestation { trophy_id: tid, nonce: big("nonce", "nonce")?, expires_ms, signature })
}

pub(super) fn build_claim_tx(pkg: &str, registry: &str, game: &str, player: &str, profile_id: Option<&str>, atts: &[Attestation]) -> rare_sui::ptb::Tx {
    use rare_sui::ptb::{bcs, Tx};
    let mut tx = Tx::new();
    tx.set_sender(player);
    let reg = tx.object(registry);
    let g = tx.object(game);
    let profile = match profile_id {
        Some(p) => tx.object(p),
        None => {
            let p = tx.pure_address(player);
            tx.move_call(&format!("{pkg}::profile::create"), &[], &[reg, p])
        }
    };
    let clock = tx.object("0x6");
    for a in atts {
        let (t, n, e, sig) = (tx.pure_u64(a.trophy_id), tx.pure_u64(a.nonce), tx.pure_u64(a.expires_ms), tx.pure(bcs::bytes(&a.signature)));
        tx.move_call(&format!("{pkg}::game::mint_attested"), &[], &[reg, g, profile, t, n, e, sig, clock]);
    }
    if profile_id.is_none() {
        tx.move_call(&format!("{pkg}::profile::share"), &[], &[profile]);
    }
    tx
}

pub fn claim(appid: u32, keys: Vec<String>) {
    let ok = with(|s| {
        if s.claiming.iter().any(|(a, _)| *a == appid) || keys.is_empty() || claim_blocker(s, appid).is_some() {
            return false;
        }
        s.claiming.push((appid, keys.clone()));
        changed(s);
        true
    });
    if !ok {
        return;
    }
    bg(move || {
        let r = claim_blocking(appid, &keys);
        with(|s| {
            s.claiming.retain(|(a, _)| *a != appid);
            changed(s);
        });
        match r {
            Ok((claimed, skipped, game_name)) => {
                with(|s| {
                    for d in &claimed {
                        push_popup(s, Popup { minted: true, name: d.name.clone(), game: Some(game_name.clone()), tier: Some(d.tier), points: Some(d.points), source: Some(CLAIM_STEAM_SOURCE), icon_url: (!d.image_url.is_empty()).then(|| d.image_url.clone()), more: 0 });
                    }
                });
                if let Some((_, why)) = skipped.first() {
                    super::bus::note(format!("{} not claimed: {}", skipped.len(), why.to_lowercase()), false);
                }
                load_chain(true);
            }
            Err(e) if e.contains("declined") || e.contains("cancel") => super::bus::note("Claim cancelled", false),
            Err(e) => super::bus::note(format!("Couldn't claim the trophies. {e}"), true),
        }
    });
}

type Claimed = (Vec<ChainDef>, Vec<(String, String)>, String);

fn claim_blocking(appid: u32, keys: &[String]) -> Result<Claimed, String> {
    let cfg = rare_sui::network::trophy_config().ok_or("Claiming isn't set up in this build.")?;
    let (player, game, profile_id) = with(|s| (s.address.clone(), s.games.get(&appid).cloned().flatten(), s.profile_id.clone()));
    let player = player.ok_or("Add a Rare Wallet account to claim")?;
    let game = game.ok_or("This game is not on Rare Trophies yet")?;
    if !game.attested {
        return Err("This game does not take self-reported trophies".into());
    }
    if game.paused {
        return Err("Claims for this game are paused".into());
    }
    let fresh = rare_trophies::trophies_steam_app(appid).ok_or("Steam's files for this game are gone from this device")?;
    let mut skipped: Vec<(String, String)> = Vec::new();
    let mut picks: Vec<(rare_trophies::steam::Achievement, ChainDef)> = Vec::new();
    for k in keys {
        let a = fresh.achievements.iter().find(|x| &x.api_name == k);
        let def = game.game.defs.iter().find(|d| &d.key == k);
        match (a, def) {
            (Some(a), Some(d)) if a.unlocked && d.tier != 3 => picks.push((a.clone(), d.clone())),
            (Some(a), _) if !a.unlocked => skipped.push((k.clone(), "No longer unlocked on this device".into())),
            (None, _) => skipped.push((k.clone(), "No longer unlocked on this device".into())),
            (_, None) => skipped.push((k.clone(), "Not a trophy on Rare yet".into())),
            _ => skipped.push((k.clone(), "The platinum unlocks by itself".into())),
        }
    }
    picks.truncate(MAX_PER_TX);
    if picks.is_empty() {
        return Err(skipped.first().map(|s| s.1.clone()).unwrap_or_else(|| "Nothing to claim".into()));
    }
    let addr = rare_sui::ptb::norm_addr(&player);
    let mut atts = Vec::new();
    for (a, def) in &picks {
        let body = serde_json::json!({ "player": addr, "game": format!("steam:{appid}"), "achievement": a.api_name, "evidence": { "unlockTime": a.unlock_time, "steamId": fresh.steam_id } });
        let reply = rare_trophies::trophy_attest(body).map_err(|e| e.to_string())?;
        atts.push(parse_attestation(&reply, &cfg.registry, &game.game.id, def.trophy_id, now_ms())?);
    }
    let tx = build_claim_tx(&cfg.package, &cfg.registry, &game.game.id, &player, profile_id.as_deref(), &atts);
    let title = if picks.len() == 1 { format!("Claim {}", picks[0].1.name) } else { format!("Claim {} trophies", picks.len()) };
    let signer = super::signer::current().ok_or("Add a Rare Wallet account to claim")?;
    live::rt().block_on(signer.sign_and_execute(tx, &title)).map_err(|e| e.to_string())?;
    Ok((picks.into_iter().map(|p| p.1).collect(), skipped, game.game.name))
}

// ---------------------------------------------------------------- pop-ups

fn push_popup(s: &mut Store, p: Popup) {
    // A burst folds into the last card.
    if s.popups.len() >= 4 {
        if let Some(last) = s.popups.back_mut() {
            last.p.more += 1;
        }
    } else {
        s.pop_seq += 1;
        let id = s.pop_seq;
        s.popups.push_back(Pop { id, p });
    }
    changed(s);
}

/// A pop-up from any thread (Epic and GOG unlocks, milestones, a pin). Over a game it goes on
/// the HUD too (rare_system::hud: drawn over the game without taking its focus).
pub fn push_popup_any(p: Popup) {
    hud(&p);
    with(|s| push_popup(s, p));
}

/// The pop-up over whatever has the screen when it isn't the launcher: the title, then game,
/// tier and points, with the trophy's picture.
fn hud(p: &Popup) {
    if std::env::var("RR_FIXTURE").is_ok() || rare_system::host::get().launcher_front() {
        return;
    }
    let p = p.clone();
    bg(move || {
        let meta = [p.game.clone(), p.tier.map(|t| crate::screens::trophies::model::tier_name(t).to_string()), p.points.map(|x| format!("{x} points"))].into_iter().flatten().collect::<Vec<_>>().join(" \u{b7} ");
        let title = if p.minted { format!("On chain: {}", p.name) } else { p.name.clone() };
        rare_system::hud::toast_icon(&title, Some(&meta), p.icon_url.as_deref().and_then(hud_icon));
    });
}

/// A trophy picture for the HUD: Rare's badge (an SVG data URI) or a store's icon (https).
pub fn hud_icon(src: &str) -> Option<rare_system::hud::Icon> {
    const N: u32 = 96;
    if let Some(svg) = src.strip_prefix("data:image/svg+xml;utf8,") {
        let text = svg.replace("%3C", "<").replace("%3E", ">").replace("%23", "#").replace("%25", "%").replace('\'', "\"");
        let tree = resvg::usvg::Tree::from_str(&text, &resvg::usvg::Options::default()).ok()?;
        let mut pm = resvg::tiny_skia::Pixmap::new(N, N)?;
        let s = N as f32 / tree.size().width().max(1.0);
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(s, s), &mut pm.as_mut());
        // tiny-skia is premultiplied; the HUD takes straight alpha.
        let rgba = pm.pixels().iter().flat_map(|c| { let c = c.demultiply(); [c.red(), c.green(), c.blue(), c.alpha()] }).collect();
        return Some(rare_system::hud::Icon { w: N as usize, h: N as usize, rgba });
    }
    if !src.starts_with("https://") {
        return None;
    }
    let mut r = ureq::get(src).call().ok()?;
    let bytes = r.body_mut().with_config().limit(2 << 20).read_to_vec().ok()?;
    let img = image::load_from_memory(&bytes).ok()?.resize_exact(N, N, image::imageops::FilterType::Triangle).to_rgba8();
    Some(rare_system::hud::Icon { w: N as usize, h: N as usize, rgba: img.into_raw() })
}

/// An unlock from elsewhere (retro trophies from an emulator): a pop-up like a Steam one.
pub fn push_external(name: String, game: String, points: u32, icon_url: Option<String>) {
    with(|s| push_popup(s, Popup { minted: false, name, game: Some(game), tier: None, points: Some(points), source: None, icon_url, more: 0 }));
}

/// New local Steam unlocks (`steam-achievements`).
pub fn on_unlocks(st: &mut State, _fx: &mut Fx, payload: &Value, _now: f64) {
    let list = payload.get("unlocks").and_then(|u| u.as_array()).cloned().unwrap_or_default();
    for u in &list {
        let appid = num(u.get("appid")) as u32;
        let api = string(u.get("apiName"));
        let title = st.library.as_ref().and_then(|l| l.games.iter().find(|g| g.id == format!("steam:{appid}"))).map(|g| g.title.clone());
        let game_name = string(u.get("gameName"));
        let def = with(|s| s.games.get(&appid).and_then(|g| g.as_ref()).and_then(|g| g.game.defs.iter().find(|d| d.key == api)).cloned());
        let tier = def.as_ref().map(|d| d.tier).or_else(|| Some(crate::screens::trophies::model::rarity_tier(rare_trophies::rarity::cached(appid).and_then(|r| r.get(&api).map(|p| *p as f64)))));
        {
            push_popup_any(
                Popup {
                    minted: false,
                    name: string(u.get("name")),
                    // The library's title first: a schema can carry a placeholder name ("ValveTestApp10180").
                    game: Some(title.filter(|t| !t.is_empty()).unwrap_or_else(|| if game_name.is_empty() { "Steam".into() } else { game_name.clone() })),
                    tier,
                    points: def.as_ref().map(|d| d.points).or_else(|| tier.map(rare_trophies::score::points)),
                    source: None,
                    icon_url: u.get("iconUrl").and_then(|v| v.as_str()).map(String::from),
                    more: 0,
                },
            );
        }
    }
    if !list.is_empty() {
        with(|s| s.steam_loading = false);
        load_steam();
    }
}

/// Timers: the front pop-up shows 4.8 s and waits while a game runs. Returns the next wake.
pub fn tick(st: &State, now: f64) -> Option<f64> {
    let gaming = !st.running.is_empty();
    let (pops, track_wake) = super::trophy_case::track(st, now);
    for p in pops {
        push_popup_any(p);
    }
    let own = tick_popups(gaming, now);
    // Reports go to rare.market in the background too: after each session and on a timer.
    let (due, ended, next) = super::trophy_case::sync_due(gaming, now);
    if due && st.library.is_some() && std::env::var("RR_FIXTURE").is_err() {
        let d = case_data(st, ui::GameSort::Recent);
        super::trophy_case::sync(reports(&d), ended);
    }
    [own, track_wake, Some(next)].into_iter().flatten().reduce(f64::min)
}

fn tick_popups(gaming: bool, now: f64) -> Option<f64> {
    with(|s| {
        if s.popups.is_empty() {
            s.shown_at = None;
            return None;
        }
        if gaming {
            s.shown_at = None;
            return None;
        }
        match s.shown_at {
            None => {
                s.shown_at = Some(now);
                s.ver += 1;
                Some(now + ui::POPUP_MS as f64 / 1000.0)
            }
            Some(t) if now - t >= ui::POPUP_MS as f64 / 1000.0 => {
                s.popups.pop_front();
                s.shown_at = None;
                s.ver += 1;
                Some(now)
            }
            Some(t) => Some(t + ui::POPUP_MS as f64 / 1000.0),
        }
    })
}

/// What the toast chrome keys on: the front pop-up's id while it is up.
pub fn popup_stamp() -> u64 {
    with(|s| if s.shown_at.is_some() { s.popups.front().map(|p| p.id).unwrap_or(0) } else { 0 })
}

/// The pop-up card for the toast column (None when nothing is up).
pub fn popup_el(ui: &mut crate::kit::Ui, gaming: bool) -> Option<rare_render::view::El> {
    let p = with(|s| if s.shown_at.is_some() { s.popups.front().map(|p| p.p.clone()) } else { None })?;
    let m = ui::popup_view::<()>(Some(&p), gaming)?;
    let mut acts = Acts::default();
    let (el, _) = build(ui, &mut acts, &m);
    Some(el)
}

// ---------------------------------------------------------------- the case's data

pub struct CaseData {
    pub games: Vec<CaseGame>,
    pub summary: crate::screens::trophies::model::CaseSummary,
    pub address: Option<String>,
    pub loading: bool,
    pub chain_on: bool,
    pub claiming: Vec<(u32, Vec<String>)>,
}

pub fn case_data(st: &State, sort: ui::GameSort) -> CaseData {
    use super::trophy_case as tc;
    let ra = super::emulation::case_games();
    let mut d = case_data_steam(st, sort);
    for g in d.games.iter_mut() {
        g.key = format!("steam:{}", g.appid);
        g.lib_id = Some(g.key.clone());
    }
    for (appid, name, game, header) in ra {
        let rows = game_rows(Some(&game), None, &[]);
        let mut sum = summarize(&rows);
        if rows.is_empty() {
            // Not opened yet: the counts the list already has.
            sum.total = game.total;
            sum.unlocked = game.unlocked;
            sum.pct = if game.total > 0 { (game.unlocked as f64 / game.total as f64 * 100.0).round() as u32 } else { 0 };
            sum.last_at = game.last_unlock.map(|t| t * 1000);
        }
        let key = format!("retro:{}", appid - super::emulation::CASE_BASE);
        d.games.push(CaseGame { appid, name, steam: game, chain_players: None, on_chain: false, sum, rows, blocker: None, header, key, lib_id: None, milestones: vec![], marked: false });
    }
    for (lib, game) in tc::ext_games() {
        let rows = game_rows(Some(&game), None, &[]);
        let title = st.game(&lib).map(|g| g.title.clone()).unwrap_or_else(|| game.name.clone());
        let header = st.game(&lib).map(|g| crate::screens::trophies::ui::lib_header(g)).unwrap_or_default();
        d.games.push(CaseGame { appid: game.appid, name: title, sum: summarize(&rows), rows, steam: game, chain_players: None, on_chain: false, blocker: None, header, key: lib.clone(), lib_id: Some(lib), milestones: vec![], marked: false });
    }
    // Rare's milestones for every game in the library, joined to the game's achievements.
    let now = now_ms() / 1000;
    let mut texts_wanted = Vec::new();
    if let Some(lib) = st.library.as_ref() {
        for g in lib.games.iter().chain(lib.owned.iter()).filter(|g| tc::eligible(g)) {
            let cid = tc::case_id(&g.id);
            let at = d.games.iter().position(|c| c.appid == cid);
            let source_complete = at.is_some_and(|i| d.games[i].sum.total > 0 && d.games[i].sum.unlocked >= d.games[i].sum.total && !d.games[i].rows.is_empty());
            let first_unlock = at.and_then(|i| d.games[i].rows.iter().filter(|r| r.state != crate::screens::trophies::model::RowState::Locked).filter_map(|r| r.at.filter(|t| *t > 0)).min().map(|t| t / 1000).or(d.games[i].steam.last_unlock.filter(|_| d.games[i].steam.unlocked > 0)));
            let (list, fresh) = tc::milestones(g, source_complete, first_unlock, now);
            // Only while it runs: anything else just became known (an old Steam unlock read for the first time).
            for m in fresh.into_iter().filter(|_| st.running.contains(&g.id)) {
                push_popup_any(Popup { minted: false, name: m.text.name.clone(), game: Some(g.title.clone()), tier: Some(m.tier), points: Some(rare_trophies::score::points(m.tier)), source: None, icon_url: Some(rare_trophies::badge::data_uri(m.kind, &g.id)), more: 0 });
            }
            if !list.iter().any(|m| m.unlocked) && at.is_none() && !g.installed {
                continue;
            }
            texts_wanted.push((g.id.clone(), g.title.clone()));
            let rows: Vec<_> = list.iter().map(|m| tc::milestone_row(m, &g.id)).collect();
            let marked = rare_trophies::milestones::marked(&g.id);
            match at {
                Some(i) => {
                    d.games[i].milestones = rows;
                    d.games[i].marked = marked;
                    d.games[i].lib_id = Some(g.id.clone());
                    if d.games[i].header.is_empty() {
                        d.games[i].header = crate::screens::trophies::ui::lib_header(g);
                    }
                }
                None => {
                    let game = SteamGame { appid: cid, name: g.title.clone(), source: "rare".into(), ..Default::default() };
                    d.games.push(CaseGame { appid: cid, name: g.title.clone(), steam: game, chain_players: None, on_chain: false, sum: Default::default(), rows: vec![], blocker: None, header: crate::screens::trophies::ui::lib_header(g), key: g.id.clone(), lib_id: Some(g.id.clone()), milestones: rows, marked });
                }
            }
        }
    }
    tc::fetch_texts(texts_wanted);
    // Trust, showcase and pins, then the sums over both lists.
    for g in d.games.iter_mut() {
        tc::apply_case(&mut g.rows, &g.key);
        tc::apply_case(&mut g.milestones, &g.key);
        let all: Vec<_> = g.rows.iter().chain(g.milestones.iter()).cloned().collect();
        let mut sum = summarize(&all);
        if g.rows.is_empty() && g.steam.total > 0 {
            // RetroAchievements before the game's list is read.
            let ms = summarize(&g.milestones);
            sum.total = g.steam.total + ms.total;
            sum.unlocked = g.steam.unlocked + ms.unlocked;
            sum.pct = if sum.total > 0 { (sum.unlocked as f64 / sum.total as f64 * 100.0).round() as u32 } else { 0 };
            sum.last_at = sum.last_at.max(g.steam.last_unlock.map(|t| t * 1000));
        }
        // Keep the chain's numbers (minted, claimable) from the achievements alone.
        let own = summarize(&g.rows);
        sum.minted = own.minted;
        sum.claimable = own.claimable;
        sum.unminted = own.unminted;
        sum.points = own.points;
        g.sum = sum;
    }
    d.games.retain(|g| g.sum.total > 0);
    let lists: Vec<&[crate::screens::trophies::model::TrophyRow]> = d.games.iter().flat_map(|g| [g.rows.as_slice(), g.milestones.as_slice()]).collect();
    d.summary = crate::screens::trophies::model::with_score(d.summary, &lists);
    d.summary.games = d.games.iter().filter(|g| g.sum.unlocked > 0).count() as u64;
    d.loading = d.loading || tc::ext_loading();
    sort_games(&mut d.games, sort);
    d
}

/// What goes to rare.market's checks: every game's unlocks (key, unix seconds, hardcore, tier).
pub fn reports(d: &CaseData) -> Vec<super::trophy_case::Report> {
    d.games
        .iter()
        .filter(|g| g.sum.unlocked > 0)
        .map(|g| super::trophy_case::Report {
            game: g.key.clone(),
            name: g.name.clone(),
            source: g.steam.source.clone(),
            unlocks: g.rows.iter().chain(g.milestones.iter()).filter(|r| r.state != crate::screens::trophies::model::RowState::Locked).map(|r| (r.key.clone(), r.at.map(|t| t / 1000), r.hardcore, r.tier.unwrap_or(0))).collect(),
        })
        .collect()
}

fn case_data_steam(st: &State, sort: ui::GameSort) -> CaseData {
    with(|s| {
        seed(s);
        let steam = s.steam.clone().unwrap_or_default();
        let mut games: Vec<CaseGame> = steam
            .iter()
            .map(|g| {
                let info = s.games.get(&g.appid).cloned().flatten();
                let rows = game_rows(Some(g), info.as_ref().map(|i| &i.game), &s.minted);
                let title = st.library.as_ref().and_then(|l| l.games.iter().find(|x| x.id == format!("steam:{}", g.appid))).map(|x| x.title.clone());
                let name = title.filter(|t| !t.is_empty()).or_else(|| (!g.name.is_empty()).then(|| g.name.clone())).or_else(|| info.as_ref().map(|i| i.game.name.clone())).unwrap_or_else(|| format!("Steam app {}", g.appid));
                CaseGame {
                    appid: g.appid,
                    name,
                    steam: g.clone(),
                    chain_players: info.as_ref().map(|i| i.game.players),
                    on_chain: info.is_some(),
                    sum: summarize(&rows),
                    rows,
                    blocker: claim_blocker(s, g.appid),
                    header: ui::steam_header_sources(g.appid, &[]),
                    key: format!("steam:{}", g.appid),
                    lib_id: Some(format!("steam:{}", g.appid)),
                    milestones: vec![],
                    marked: false,
                }
            })
            .collect();
        // RetroAchievements games (features/emulation.rs), after the store lock is released below.
        sort_games(&mut games, sort);
        CaseData {
            summary: case_summary(&steam, s.profile.as_ref(), &s.minted),
            games,
            address: s.address.clone(),
            loading: s.steam_loading || s.chain_loading || s.steam.is_none(),
            chain_on: rare_sui::network::trophy_config().is_some() || s.fixture,
            claiming: s.claiming.clone(),
        }
    })
}

/// The running game's whole case for the Rare menu's Trophies tab: its store's achievements, then
/// Rare's own trophies. None while a Steam game's achievements are still being read; Some(empty)
/// when the game has none at all.
pub fn case_rows(st: &State, rid: &str) -> Option<Vec<crate::screens::trophies::model::TrophyRow>> {
    let cid = super::trophy_case::case_id(rid);
    if cid < super::trophy_case::EXT_BASE && with(|s| s.steam.is_none()) {
        load_steam();
        return None;
    }
    let d = case_data(st, ui::GameSort::Recent);
    let g = d.games.iter().find(|g| g.appid == cid || g.lib_id.as_deref() == Some(rid));
    Some(g.map(|g| g.rows.iter().chain(g.milestones.iter()).cloned().collect()).unwrap_or_default())
}

/// Rare's own trophies for a library game (the Rare menu adds them to a Steam game's list).
pub fn milestone_rows(st: &State, rid: &str) -> Vec<crate::screens::trophies::model::TrophyRow> {
    let Some(g) = st.game(rid).filter(|g| super::trophy_case::eligible(g)) else { return vec![] };
    let (list, _) = super::trophy_case::milestones(g, false, None, now_ms() / 1000);
    let mut rows: Vec<_> = list.iter().map(|m| super::trophy_case::milestone_row(m, &g.id)).collect();
    super::trophy_case::apply_case(&mut rows, &g.id);
    rows
}

/// One Steam game's trophy rows for the Rare menu: None while the local achievements are still
/// being read, Some(None) when the game has none.
pub fn rows_for(appid: u32) -> Option<Option<Vec<crate::screens::trophies::model::TrophyRow>>> {
    with(|s| {
        let steam = s.steam.as_ref()?;
        Some(steam.iter().find(|g| g.appid == appid).map(|g| {
            let info = s.games.get(&appid).cloned().flatten();
            game_rows(Some(g), info.as_ref().map(|i| &i.game), &s.minted)
        }))
    })
}

fn store_ver() -> u64 {
    with(|s| s.ver) ^ super::trophy_case::ver().wrapping_mul(0x9e37_79b9)
}

/// Starts the loads the case and the profile shelf need.
pub fn refresh() {
    if std::env::var("RR_FIXTURE").is_ok() && !with(|s| s.fixture) {
        with(|s| seed_fixture(s));
    }
    load_steam();
    load_chain(false);
}

/// Called once at app start: the watcher for new unlocks, and a first read.
pub fn start() {
    if std::env::var("RR_FIXTURE").is_ok() {
        with(|s| {
            seed_fixture(s);
        });
        return;
    }
    rare_trophies::watch(super::bus::ctx());
}

// ---------------------------------------------------------------- profile shelf

/// The profile's Trophies rail, mapped to the profile's messages; None without games.
pub fn shelf<M: 'static>(st: &State, map: &impl Fn(Option<u32>) -> M) -> Option<crate::screens::wallet::markup::El<M>> {
    let d = case_data(st, ui::GameSort::Recent);
    let now = now_ms();
    let show = showcase(&d);
    ui::profile_shelf(&d.games, &d.summary, &show, now, 0).map(|e| {
        e.map(&|m| match m {
            Msg::Open(a) => map(a),
            _ => map(None),
        })
    })
}

pub fn shelf_ver() -> u64 {
    store_ver()
}

// ---------------------------------------------------------------- showcase and trophy sheets

/// The showcase as the case draws it (each slot opens its game when it is in the case).
pub fn showcase(d: &CaseData) -> Vec<ui::ShowPin> {
    rare_trophies::case::with(|c| c.showcase.clone())
        .into_iter()
        .map(|p| ui::ShowPin { appid: d.games.iter().find(|g| g.key == p.game).map(|g| g.appid), name: p.name, game: p.game_name, icon_url: p.icon_url, tier: p.tier })
        .collect()
}

fn pin_of(g: &CaseGame, r: &crate::screens::trophies::model::TrophyRow) -> rare_trophies::case::Pin {
    let source = if r.milestone { "rare".to_string() } else { g.steam.source.replace("stats", "steam").replace("librarycache", "steam") };
    rare_trophies::case::Pin {
        game: g.key.clone(),
        game_name: g.name.clone(),
        key: r.key.clone(),
        name: r.name.clone(),
        description: r.description.clone(),
        icon_url: r.icon_url.clone().filter(|u| u.starts_with("https://")).or_else(|| {
            // Rare's badge: rare.market's copy, so the profile page shows the same art.
            rare_trophies::milestones::Kind::from_key(&r.key).filter(|_| r.milestone).map(|k| rare_trophies::badge::url(k, &g.key))
        }),
        tier: r.tier.unwrap_or(0),
        source,
        unlocked_at: r.at.map(|t| t / 1000),
    }
}

/// A on an unlocked trophy: its sheet.
fn row_sheet(st: &mut State, appid: u32, key: &str) {
    use crate::state::{OptionItem, OptionsSpec};
    use crate::screens::trophies::model::Trust;
    let d = case_data(st, ui::GameSort::Recent);
    let Some(g) = d.games.iter().find(|g| g.appid == appid) else { return };
    let Some(r) = g.row(key) else { return };
    let code = |what: &str| Act::Legacy(format!("trophy:{what}:{appid}:{key}"));
    let mut items = vec![OptionItem { label: if r.showcased { "Remove from your showcase".into() } else { "Show on your profile".into() }, on: None, act: code("show") }];
    let can_pin = rare_sui::network::trophy_config().is_some() && r.pinned.is_none() && !matches!(r.trust, Trust::Flagged(_));
    if can_pin {
        items.push(OptionItem { label: "Pin to chain".into(), on: None, act: code("chain") });
    }
    let trust = match &r.trust {
        Trust::Verified => "Verified on SuiPlay: it counts toward your Rare score.".to_string(),
        Trust::Flagged(why) => format!("Not counted: {why}."),
        Trust::Unverified => "Not verified, so it doesn't count toward your Rare score.".to_string(),
        Trust::Pending => "Not checked by rare.market yet.".to_string(),
    };
    let chain = if r.pinned.is_some() { " It is on chain." } else if can_pin { " Pin to chain mints it in your wallet as a self-reported trophy; Rare covers the fee within its limits." } else { "" };
    st.options = Some(OptionsSpec { title: r.name.clone(), sub: Some(format!("{} · {}. {trust}{chain}", g.name, rare_trophies::score::tier_name(r.tier.unwrap_or(0)))), items });
}

/// The sheet's codes: `trophy:show:<appid>:<key>`, `trophy:chain:<appid>:<key>`.
pub fn on_code(st: &mut State, code: &str) {
    let mut parts = code.splitn(4, ':');
    let (_, what, appid, key) = (parts.next(), parts.next().unwrap_or(""), parts.next().and_then(|a| a.parse::<u32>().ok()), parts.next().unwrap_or(""));
    let Some(appid) = appid else { return };
    let d = case_data(st, ui::GameSort::Recent);
    let Some(g) = d.games.iter().find(|g| g.appid == appid) else { return };
    let Some(r) = g.row(key) else { return };
    let pin = pin_of(g, r);
    st.options = None;
    match what {
        "show" => {
            let on = super::trophy_case::toggle_showcase(pin);
            super::bus::note(if on { "On your profile" } else { "Removed from your profile" }, false);
        }
        "chain" => super::trophy_case::pin_to_chain(pin),
        _ => {}
    }
    with(changed);
}

// ---------------------------------------------------------------- the case page

pub struct CasePage {
    cs: CaseState,
    built: Built<Msg>,
    ver: u64,
    kicked: bool,
}

impl Default for CasePage {
    fn default() -> Self {
        let appid = with(|s| s.open_appid.take());
        if let Some(a) = appid {
            super::emulation::case_open(a);
        }
        refresh();
        CasePage { cs: CaseState::new(appid), built: Built::default(), ver: 0, kicked: false }
    }
}

impl Page for CasePage {
    fn stamp(&self, st: &State) -> u64 {
        hash((self.ver, store_ver(), format!("{:?}", self.cs), st.ver.library, super::emulation::ver()))
    }

    fn view(&mut self, ui_: &mut crate::kit::Ui, st: &State, acts: &mut Acts) -> rare_render::view::El {
        let d = case_data(st, self.cs.sort);
        if !std::mem::replace(&mut self.kicked, true) {
            super::trophy_case::load_ext(st);
        }
        super::trophy_case::sync(reports(&d), false);
        // The heading names the account as the rest of Rare does (username, SuiNS, label), not 0x...
        let pl = st.live.profile.as_ref().filter(|p| d.address.as_deref().is_some_and(|a| p.address.eq_ignore_ascii_case(a)));
        let acct = st.account.as_ref().filter(|a| d.address.as_deref().is_some_and(|x| a.address.eq_ignore_ascii_case(x)));
        let username = pl.and_then(|p| p.username.clone()).or_else(|| acct.and_then(|a| a.username.clone())).or_else(|| d.address.as_deref().and_then(crate::data::account::cached_username));
        let prof = Profile { address: d.address.clone(), username, name: pl.and_then(|p| p.suins.clone()), label: acct.map(|a| a.label.clone()), ..Default::default() };
        let show = showcase(&d);
        let (note, checked) = rare_trophies::case::with(|c| (c.device_note.clone(), c.synced_at.is_some()));
        let pinning: Vec<String> = d.games.iter().flat_map(|g| g.rows.iter().chain(g.milestones.iter()).filter(|r| super::trophy_case::pinning(&g.key, &r.key)).map(|r| rare_trophies::case::id(&g.key, &r.key)).collect::<Vec<_>>()).collect();
        let pr = CaseProps { games: &d.games, showcase: &show, device_note: note.as_deref(), checked, pinning: &pinning, summary: &d.summary, prof: &prof, loading: d.loading, chain_on: d.chain_on, claiming: &d.claiming, now: now_ms(), tz_min: 0 };
        let mut m = case_view(&pr, &self.cs);
        m.class = m.class.replace("fixed inset-0 z-30", "h-full w-full");
        let (el, built) = build(ui_, acts, &m);
        self.built = built;
        el
    }

    fn pad(&mut self, st: &mut State, _fx: &mut Fx, b: crate::page::Btn, _cur: Option<(&rare_render::focus::Focusable, Option<&Act>)>) -> bool {
        use crate::page::Btn;
        let msg = match b {
            Btn::B => Msg::Back,
            Btn::X => Msg::Next,
            _ => return false,
        };
        if ui::case_update(&mut self.cs, &msg) == Some(true) {
            st.close();
        }
        self.ver += 1;
        true
    }

    fn act(&mut self, st: &mut State, _fx: &mut Fx, code: u32) {
        let Some(m) = self.built.msg(code) else { return };
        match &m {
            Msg::Claim(appid, keys) => claim(*appid, keys.clone()),
            Msg::Row(appid, key) => row_sheet(st, *appid, key),
            Msg::Locked(how) => st.toast(how.clone(), crate::state::Tone::Info, rare_render::clock::secs()),
            Msg::Complete(appid) => {
                let d = case_data(st, self.cs.sort);
                if let Some(lib) = d.games.iter().find(|g| g.appid == *appid).and_then(|g| g.lib_id.clone()) {
                    let on = !rare_trophies::milestones::marked(&lib);
                    super::trophy_case::set_complete(&lib, on);
                    super::bus::note(if on { "Marked complete" } else { "No longer marked complete" }, false);
                }
                self.ver += 1;
            }
            other => {
                ui::case_update(&mut self.cs, other);
                if let Some(a) = self.cs.view {
                    super::emulation::case_open(a);
                }
                self.ver += 1;
            }
        }
    }

    /// X sorts the games, or filters one game's trophies (TrophyCase.tsx useHints).
    fn hints(&self, _st: &State) -> Vec<rare_render::focus::hints::Hint> {
        vec![rare_render::focus::hints::hint(rare_render::focus::hints::Key::X, &ui::case_hint(&self.cs))]
    }

    fn tick(&mut self, _st: &mut State, _fx: &mut Fx, now: f64) -> Option<f64> {
        let busy = with(|s| s.steam_loading || s.chain_loading || !s.claiming.is_empty()) || super::trophy_case::ext_loading();
        busy.then_some(now + 0.5)
    }
}

// ---------------------------------------------------------------- fixture

fn seed_fixture(s: &mut Store) {
    let ach = |k: &str, name: &str, unlocked: bool, t: i64| SteamAchievement { api_name: k.into(), name: name.into(), description: format!("{name}: a thing you did in the game."), unlocked, unlock_time: unlocked.then_some(t), global_percent: Some(40.0 - t as f64 % 30.0), ..Default::default() };
    let t0 = 1_761_000_000;
    let mk = |appid: u32, name: &str, n: usize, done: usize| SteamGame {
        appid,
        name: name.into(),
        achievements: (0..n).map(|i| ach(&format!("ACH_{i}"), &format!("Trophy {}", i + 1), i < done, t0 + i as i64 * 86_400)).collect(),
        total: n as u32,
        unlocked: done as u32,
        last_unlock: Some(t0 + done as i64 * 86_400),
        source: "stats".into(),
    };
    s.steam = Some(vec![mk(1145360, "Hades", 8, 5), mk(367520, "Hollow Knight", 12, 3), mk(1966720, "Lethal Company", 6, 0)]);
    s.fixture = true;
    s.address = Some("0x1".into());
    super::trophy_case::seed_fixture();
    let def = |i: u32, tier: u8, pts: u32| ChainDef { trophy_id: i, key: format!("ACH_{i}"), name: format!("Trophy {}", i + 1), tier, points: pts, unlocked: 30, ..Default::default() };
    s.games.insert(1145360, Some(GameInfo { game: ChainGame { id: "0xaa".into(), name: "Hades".into(), players: 100, defs: (0..8).map(|i| def(i, (i % 3) as u8, 10 * (i % 3 + 1))).collect() }, attested: true, paused: false }));
    s.minted = vec![ChainTrophy { object_id: "0x01".into(), game: "0xaa".into(), trophy_id: 0, name: "Trophy 1".into(), tier: 0, points: 10, source: 1, number: 12, unlocked_at_ms: (t0 as i64) * 1000 }];
    s.ver += 1;
}
