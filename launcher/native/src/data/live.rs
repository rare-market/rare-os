//! Live Sui data for the shell (lib/live.ts): the SUI price and its 24 h line, network status,
//! gas, top collections and the latest sales. Polled on a worker thread on live.ts's schedule;
//! paused while a game runs. `fixture()` is a fixed set for golden images.

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CollectionRow {
    pub rank: u32,
    pub slug: String,
    pub name: String,
    pub image_url: Option<String>,
    pub verified: bool,
    pub floor_mist: Option<u128>,
    pub floor_change_pct: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Sale {
    pub id: String,
    /// The item sold and its collection's slug: where the row opens.
    pub item_id: Option<String>,
    pub collection_slug: Option<String>,
    pub item_name: Option<String>,
    pub item_image: Option<String>,
    pub collection_name: Option<String>,
    pub collection_image: Option<String>,
    pub price_mist: Option<u128>,
    /// ms since the epoch
    pub at_ms: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Live {
    pub sui_usd: Option<f64>,
    pub sui_change: Option<f64>,
    /// (t ms, close)
    pub sui_line: Vec<(f64, f64)>,
    pub tps: Option<i64>,
    pub degraded: bool,
    pub gas: Option<u64>,
    pub collections: Option<Vec<CollectionRow>>,
    pub sales: Option<Vec<Sale>>,
    /// Ids of sales that arrived since the last poll (one-time tint).
    #[serde(skip)]
    pub fresh: Vec<String>,
    /// The On Sui shelf's minting-now tiles (rare-sui SuiLive); None until the first read.
    pub mints: Option<Vec<SuiMint>>,
    /// The signed-in account as rare-sui reads it: name, picture, Sui games owned.
    pub profile: Option<ProfileLive>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProfileLive {
    pub address: String,
    pub name: String,
    /// The Rare username and the SuiNS name on their own, so the top bar can put the wallet's
    /// account name (which changes the moment it is renamed) between them.
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub suins: Option<String>,
    pub avatar: Option<String>,
    /// The profile banner (rare.market's answer, else the account's default).
    #[serde(default)]
    pub banner: Option<String>,
    pub owned_games: Option<usize>,
    pub total_usd: Option<f64>,
    pub change_usd: Option<f64>,
}

use rare_sui::grpc::{ChainRead, Sui};
use rare_sui::live::{LiveDeps, Mint as SuiMint, SuiLive};
use rare_sui::market::{self, HttpMarket, MarketApi, Page};
use rare_sui::ownership::{OwnershipStore, SuiGame as OwnGame};
use rare_sui::profile::{AvatarPick, Current, Persist, ProfileStore};
use rare_sui::shell_live::{Kept, ShellLive, ACTIVE, KEPT_KEY};
use rare_sui::NetworkConfig;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// A game has the screen: the shell's live numbers stop (live.ts pauseLive). Set by `State`.
static GAME_RUNNING: AtomicBool = AtomicBool::new(false);

/// The last SUI price the shell read (bits of an f64; 0 = none), for screens that need a price
/// for SUI when the wallet holds none (Send's USD line).
static SUI_USD: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn sui_usd() -> Option<f64> {
    let b = SUI_USD.load(Ordering::Relaxed);
    (b != 0).then(|| f64::from_bits(b)).or_else(|| kept().and_then(|l| l.sui_usd)).filter(|v| *v > 0.0)
}

pub fn set_game_running(on: bool) {
    GAME_RUNNING.store(on, Ordering::Release);
}

fn mist(v: &Option<String>) -> Option<u128> {
    v.as_deref().and_then(|s| s.parse().ok())
}

/// rare-sui's ShellLive and SuiLive state in the screens' shape.
fn map(shell: &ShellLive, sui: &SuiLive, profile: &ProfileStore, own: &OwnershipStore, prev: &Live) -> Live {
    let s = shell.snapshot();
    let k = s.kept;
    let su = sui.snapshot();
    let p = profile.snapshot();
    let o = own.snapshot();
    let mut l = prev.clone();
    // A value that is not read yet keeps the last one (from this run or the last), so nothing
    // goes blank and pops back in.
    l.sui_usd = k.sui_usd.or(su.sui_usd).or(prev.sui_usd);
    if let Some(v) = l.sui_usd {
        SUI_USD.store(v.to_bits(), Ordering::Relaxed);
    }
    l.sui_change = k.sui_change24h.or(prev.sui_change);
    if !k.sui_line.is_empty() {
        l.sui_line = k.sui_line.iter().map(|p| (p.t, p.v)).collect();
    }
    l.tps = k.tps.or(su.tps).map(|t| t.round() as i64).or(prev.tps);
    l.degraded = s.degraded || su.degraded;
    l.gas = su.gas_mist.or(prev.gas);
    let collections = k.collections.map(|c| {
        c.into_iter()
            .enumerate()
            .map(|(i, r)| CollectionRow {
                rank: if r.rank > 0.0 { r.rank as u32 } else { i as u32 + 1 },
                slug: r.slug,
                name: r.name,
                image_url: r.image_url,
                verified: r.verified,
                floor_mist: mist(&r.floor_mist),
                floor_change_pct: r.floor_change_pct,
            })
            .collect()
    });
    l.collections = collections.or_else(|| prev.collections.clone());
    let sales = k.sales.map(|v| {
        v.into_iter()
            .map(|e| {
                let it = e.item.as_ref();
                Sale {
                    id: e.id,
                    item_id: it.and_then(|i| i["id"].as_str()).map(Into::into),
                    collection_slug: e.collection.as_ref().map(|c| c.slug.clone()),
                    item_name: it.and_then(|i| i["name"].as_str()).map(Into::into),
                    item_image: it.and_then(|i| i["imageUrl"].as_str()).map(Into::into),
                    collection_name: e.collection.as_ref().map(|c| c.name.clone()),
                    collection_image: e.collection.as_ref().and_then(|c| c.image_url.clone()),
                    price_mist: mist(&e.price_mist),
                    at_ms: market::parse_iso_ms(&e.at),
                }
            })
            .collect()
    });
    l.sales = sales.or_else(|| prev.sales.clone());
    l.fresh = s.fresh;
    l.mints = su.mints.or_else(|| prev.mints.clone());
    let was = prev.profile.as_ref();
    // Before the profile store has the account, the last one stays (Home shows it only for the
    // current account).
    l.profile = match p.address.as_ref() {
        None => prev.profile.clone(),
        Some(a) => Some({
        let was = was.filter(|w| w.address.eq_ignore_ascii_case(a));
        let name = p.display_name();
        let named = |n: &str| !n.is_empty() && !n.starts_with("0x");
        ProfileLive {
            address: a.clone(),
            name: match was {
                Some(w) if !named(&name) && named(&w.name) => w.name.clone(),
                _ => name,
            },
            username: p.username.clone().or_else(|| if p.loading { was.and_then(|w| w.username.clone()) } else { None }),
            suins: p.name.clone().or_else(|| if p.loading { was.and_then(|w| w.suins.clone()) } else { None }),
            avatar: p.avatar.as_ref().map(|x| x.image_url.clone()).or(p.market_avatar.clone()).or_else(|| was.and_then(|w| w.avatar.clone())).or_else(|| Some(crate::avatars::url(crate::avatars::starter(a)))),
            // None until rare.market answers (no default first and the pick after it)
            banner: p.banner.clone().or_else(|| was.and_then(|w| w.banner.clone())),
            owned_games: (o.address.as_deref() == Some(a.as_str()) && o.at_ms > 0.0).then_some(o.by_slug.len()).or(was.and_then(|w| w.owned_games)),
            total_usd: su.portfolio_usd.or(was.and_then(|w| w.total_usd)),
            change_usd: su.change.map(|c| c.usd).or(was.and_then(|w| w.change_usd)),
        }
        }),
    };
    l
}

const KEPT_LIVE: &str = "home.live";

/// The shell's live data from the last run (None in golden runs or the first time).
pub fn kept() -> Option<Live> {
    crate::data::keep::load(KEPT_LIVE)
}

fn now_ms() -> f64 {
    rare_render::clock::realtime_ns() as f64 / 1_000_000.0
}

/// Runs forever on its own thread. Three loops that never wait on each other: the shell's numbers
/// (price, status, collections, sales: ShellLive every 20 s, its schedule decides which jobs are
/// due), the Sui strip and ownership (which need the gRPC connection), and the publisher that hands
/// each changed `Live` to `send` four times a second. The shell numbers are seeded from the disk
/// cache first so Home is full on the first frame, and saved back whenever they change.
pub fn run(init: Live, mut send: impl FnMut(Live) -> bool) {
    let cfg = NetworkConfig::mainnet();
    let api = HttpMarket::new(cfg.clone());
    let shell = ShellLive::default();
    let sui = SuiLive::default();
    let profile = ProfileStore::default();
    let own = OwnershipStore::default();
    if let Some(k) = crate::screens::wallet::live::market_net().store.peek(KEPT_KEY).and_then(|h| serde_json::from_str::<Kept>(&h.body).ok()) {
        shell.seed(k);
    }
    let stop = AtomicBool::new(false);
    std::thread::scope(|sc| {
        // Shell numbers: collections, sales, price, status. First fetch starts at once, in parallel.
        sc.spawn(|| {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
            let (mut last_tick, mut last_loop, mut first, mut was_running) = (0.0f64, now_ms(), true, false);
            while !stop.load(Ordering::Acquire) {
                let now = now_ms();
                let running = GAME_RUNNING.load(Ordering::Acquire);
                // The device slept: keep what is on screen and refresh at 3 s, 8 s and 20 s.
                if now - last_loop > 45_000.0 {
                    shell.resume(now);
                    first = true;
                }
                last_loop = now;
                if running != was_running {
                    was_running = running;
                    rt.block_on(shell.pause(&api, running, now));
                }
                rt.block_on(shell.resume_due(&api, now));
                if !running && now - last_tick >= ACTIVE.as_millis() as f64 - 500.0 {
                    last_tick = now;
                    let addr = crate::data::account::current().map(|(address, _)| address);
                    rt.block_on(async {
                        tokio::join!(shell.set_account(addr.as_deref()), shell.tick(&api, first, now));
                    });
                    first = false;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        });
        // The Sui strip and ownership: connect to the chain here, so a slow connection holds up
        // nothing on Home.
        sc.spawn(|| {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
            let chain: Option<Sui> = rt.block_on(async { Sui::current(&cfg).ok() });
            let none = NoChain;
            let chain: &dyn ChainRead = match &chain {
                Some(c) => c,
                None => &none,
            };
            let persist = DiskPersist;
            let mut games: Vec<OwnGame> = Vec::new();
            let (mut last_tick, mut last_loop, mut first) = (0.0f64, now_ms(), true);
            while !stop.load(Ordering::Acquire) {
                let now = now_ms();
                if now - last_loop > 45_000.0 {
                    first = true;
                }
                last_loop = now;
                // A pick on this device: put_pick saved it already; the profile reads it back.
                if PICK.lock().ok().and_then(|mut p| p.take()).is_some() {
                    profile.reload_pick(&persist);
                }
                if !GAME_RUNNING.load(Ordering::Acquire) && now - last_tick >= ACTIVE.as_millis() as f64 - 500.0 {
                    last_tick = now;
                    let cur = crate::data::account::current().map(|(address, label)| Current { address, label: Some(label) });
                    let addr = cur.as_ref().map(|c| c.address.clone());
                    rt.block_on(async {
                        if games.is_empty() {
                            if let Ok(p) = market::get_as::<Page<OwnGame>>(&api, "/v1/games").await {
                                games = p.data;
                            }
                        }
                        let deps = LiveDeps { api: &api, chain, persist: &persist, profile: &profile, current: cur.clone(), coins_usd: addr.as_deref().and_then(|a| shell.account_of_portfolio(a)), game_running: false };
                        tokio::join!(sui.tick(&deps, first, now), own.load(&api, chain, addr.as_deref(), &games, now));
                    });
                    first = false;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        });
        // Publisher (this thread).
        let mut prev = init;
        let mut kept = shell.kept();
        let mut kept_at = 0.0f64;
        let (mut saved, mut saved_at) = (prev.clone(), 0.0f64);
        loop {
            let l = map(&shell, &sui, &profile, &own, &prev);
            if l != prev {
                prev = l.clone();
                if !send(l) {
                    break;
                }
            }
            // Keep the shell numbers for the next start (not more than every 5 s).
            let now = now_ms();
            // The whole snapshot too (the On Sui shelf, the profile), at most every 10 s.
            if now - saved_at > 10_000.0 && prev != saved {
                crate::data::keep::save(KEPT_LIVE, &prev);
                saved = prev.clone();
                saved_at = now;
            }
            if now - kept_at > 5_000.0 {
                let k = shell.kept();
                if k != kept {
                    if let Ok(j) = serde_json::to_string(&k) {
                        let _ = crate::screens::wallet::live::market_net().store.put(KEPT_KEY, j);
                    }
                    kept = k;
                    kept_at = now;
                }
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        stop.store(true, Ordering::Release);
    });
}

/// rare-sui's profile storage on the launcher's state store, so the picked picture survives a restart
/// (the web keeps it in localStorage, per address).
pub struct DiskPersist;

impl Persist for DiskPersist {
    fn get(&self, key: &str) -> Option<String> {
        crate::screens::wallet::live::market_net().store.peek(key).map(|h| h.body.to_string())
    }
    fn put(&self, key: &str, value: String) {
        let _ = crate::screens::wallet::live::market_net().store.put(key, value);
    }
    fn remove(&self, key: &str) {
        crate::screens::wallet::live::market_net().store.remove(key);
    }
}

static PICK: std::sync::Mutex<Option<Option<rare_sui::market::ItemCard>>> = std::sync::Mutex::new(None);

/// Bumped on every pick on this device: what shows the account's picture rebuilds at once.
pub static PIC_VER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn put_pick(address: &str, pick: Option<&AvatarPick>) {
    let key = format!("rare.sui.avatar.{address}");
    match pick {
        Some(p) => DiskPersist.put(&key, serde_json::to_string(p).unwrap_or_default()),
        None => DiskPersist.remove(&key),
    }
    let c = pick.map(|p| rare_sui::market::ItemCard { id: p.id.clone(), name: p.name.clone(), image_url: Some(p.image_url.clone()), ..Default::default() });
    if let Ok(mut q) = PICK.lock() {
        *q = Some(c);
    }
    PIC_VER.fetch_add(1, Ordering::Relaxed);
}

/// The picture picked for the current account (profile.ts `pickAvatar`): kept here at once (the
/// chip, the profile and the wallet show it on the next frame), then saved on rare.market in the
/// background. A save rare.market refuses puts the previous picture back with a short message.
pub fn pick_avatar(id: &str, name: &str, image_url: &str) {
    let Some(a) = crate::data::account::current().map(|(a, _)| a) else { return };
    let prev = picked(&a);
    let pick = AvatarPick { id: id.into(), image_url: image_url.into(), name: name.into(), at: rare_sui::profile::now_ms() };
    put_pick(&a, Some(&pick));
    let id = id.to_string();
    std::thread::Builder::new()
        .name("rare-avatar-save".into())
        .spawn(move || match crate::social::save_avatar(&id) {
            // Stamped by rare.market: keep its time, so later comparisons use one clock.
            Some(Ok(at)) => {
                if let (Some(at), Some(mut cur)) = (at, picked(&a).filter(|p| p.id == id)) {
                    cur.at = at;
                    put_pick(&a, Some(&cur));
                }
            }
            Some(Err(_)) => {
                // Only when nothing newer was picked meanwhile.
                if picked(&a).is_some_and(|p| p.id == id) {
                    put_pick(&a, prev.as_ref());
                    crate::social::toast_any("Couldn't save your picture. Try again.", true);
                }
            }
            // Friends is off here: the pick goes up with the first heartbeat.
            None => {}
        })
        .ok();
}

/// A default picture as the current account's picture (just its id; no NFT needed).
pub fn pick_default(id: &str) {
    if let Some(d) = crate::avatars::get(id) {
        pick_avatar(d.id, d.name, &crate::avatars::url(d.id));
    }
}

/// An account's picture: the one picked on this device, else its default (rare.market hands out
/// the same default by address until the account picks one).
pub fn picture(address: &str) -> String {
    picked(address).map(|p| p.image_url).filter(|u| !u.is_empty()).unwrap_or_else(|| crate::avatars::url(crate::avatars::starter(address)))
}

/// The picture picked on this device for an address.
pub fn picked(address: &str) -> Option<AvatarPick> {
    serde_json::from_str(&DiskPersist.get(&format!("rare.sui.avatar.{address}"))?).ok()
}

/// With no gRPC client (a bad RPC setting): reads that need the chain answer "unsupported".
struct NoChain;
impl ChainRead for NoChain {}

#[allow(dead_code)]
fn _types(_: SuiMint, _: &dyn MarketApi) {}

/// Fixed values for golden images (RR_FIXTURE): the numbers in docs/screenshots.
pub fn fixture(now_ms: f64) -> Live {
    let line: Vec<(f64, f64)> = (0..24).map(|i| (now_ms - (23 - i) as f64 * 3600_000.0, 0.914 + ((i * 7 % 11) as f64 - 5.0) * 0.002 - i as f64 * 0.0004)).collect();
    let cols = [("Popkins", "popkins", 3_200_000_000u128, 4.2), ("Warped Pioneers", "warped-pioneers", 41_500_000_000, -2.1), ("Pawtato Heroes", "pawtato-heroes", 980_000_000, 12.6), ("Rootlets", "rootlets", 88_000_000_000, 0.0), ("Fuddies", "fuddies", 15_250_000_000, -0.8)];
    Live {
        sui_usd: Some(0.907),
        sui_change: Some(-0.76),
        sui_line: line,
        tps: Some(113),
        degraded: false,
        gas: Some(100),
        collections: Some(cols.iter().enumerate().map(|(i, (n, s, f, c))| CollectionRow { rank: i as u32 + 1, slug: s.to_string(), name: n.to_string(), image_url: None, verified: i < 3, floor_mist: Some(*f), floor_change_pct: Some(*c) }).collect()),
        sales: Some(
            [("Popkins #7409", "Popkins", 3_400_000_000u128, 20.0), ("Warped Pioneer #112", "Warped Pioneers", 42_000_000_000, 95.0), ("Pawtato #3301", "Pawtato Heroes", 1_050_000_000, 240.0), ("Rootlet #88", "Rootlets", 90_000_000_000, 600.0), ("Fuddie #1203", "Fuddies", 15_000_000_000, 1800.0)]
                .iter()
                .enumerate()
                .map(|(i, (n, c, p, ago))| Sale { id: format!("s{i}"), item_id: None, collection_slug: None, item_name: Some(n.to_string()), item_image: None, collection_name: Some(c.to_string()), collection_image: None, price_mist: Some(*p), at_ms: Some(now_ms - ago * 1000.0) })
                .collect(),
        ),
        fresh: vec![],
        mints: Some(vec![SuiMint { key: "launch:demo".into(), name: "Rarelings".into(), image_url: None, sub: "1,204 of 5,000 minted".into(), href: "https://rare.market/launchpad/rarelings".into() }]),
        profile: None,
    }
}

