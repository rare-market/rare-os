//! Automatic cloud saves. Before a game starts Rare compares the saves on this device with the
//! newest cloud copy and brings the cloud copy down when only the cloud changed; after a session
//! it keeps a local copy and uploads when only this device changed. When both changed, the
//! player picks in the conflict sheet. Nothing Rare replaces is lost: the device copy goes into the
//! local history (last 10 per save set) and cloud copies stay as versions (last 5).
//!
//! What it syncs: emulator saves (the emulator's page, or ES-DE: every emulator), web games, and
//! PC games outside Steam, Epic and GOG (their stores sync themselves) whose save folders the
//! Ludusavi manifest knows, inside Rare's Proton prefix or the game's folder.
//!
//! Uploads are signed by the account's cloud saves writer (`crypto::writer_seed`): an address
//! derived from the same one-time wallet signature as the save key, funded once by the player, so
//! nothing asks for an approval after a session. The secret lives in ~/.local/share/rare/saves
//! (0600). Walrus space left by deleted old versions is used again.

use crate::data::model::Game;
use crate::screens::wallet::{live, service};
use crate::state::{Overlay, State, Tone};
use rare_sui::saves::{self, crypto, local, ludusavi, meta::{Backup, BackupMeta}, sync::{self, Base, GameSet, History, Plan, Scan, SyncState, Target}};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Fresh Walrus space for an automatic upload, in days.
const KEEP_DAYS: u32 = 365;
/// Space from a deleted version is used again when it lasts this many more epochs.
const REUSE_LEFT_EPOCHS: u64 = 6;
/// Saves larger than this are not kept or uploaded automatically.
const MAX_AUTO_BYTES: u64 = 256 << 20;
/// The overlay kind of the conflict sheet and the versions sheet.
pub const CONFLICT: &str = "saveconflict";
pub const VERSIONS: &str = "saveversions";
/// What one upload costs, roughly (write fee in WAL, gas and relay tip in SUI), for the funds line.
const UPLOAD_FROST: u64 = 10_000_000;
const UPLOAD_MIST: u64 = 6_000_000;
/// What "Add funds" sends from the wallet.
pub const FUND_FROST: u64 = 1_000_000_000;
pub const FUND_MIST: u64 = 200_000_000;

fn dir() -> PathBuf {
    std::env::var("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|_| local::home().unwrap_or_default().join(".local/share")).join("rare/saves")
}

/// The chain cloud saves use: mainnet, or testnet with RARE_SAVES_NETWORK=testnet (the end to end
/// check; the wallet stays on mainnet, so funding then happens outside Rare).
pub fn sui() -> Option<&'static rare_sui::Sui> {
    static TESTNET: OnceLock<Option<rare_sui::Sui>> = OnceLock::new();
    if std::env::var("RARE_SAVES_NETWORK").as_deref() == Ok("testnet") {
        return TESTNET
            .get_or_init(|| {
                let _g = live::rt().enter();
                rare_sui::Sui::new(rare_sui::Network::Testnet, rare_sui::network::DEFAULT_RPC_TESTNET).ok()
            })
            .as_ref();
    }
    live::sui()
}

fn testnet() -> bool {
    sui().is_some_and(|s| s.network() == rare_sui::Network::Testnet)
}

/// Epochs an automatic upload buys (RARE_SAVES_EPOCHS overrides it on testnet, to see renewal).
fn keep_epochs() -> u32 {
    if testnet() {
        if let Some(e) = std::env::var("RARE_SAVES_EPOCHS").ok().and_then(|v| v.parse().ok()) {
            return e;
        }
    }
    saves::meta::epochs_for(KEEP_DAYS, testnet())
}

fn history() -> History {
    History::new(dir().join("history"))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[derive(Default, Serialize, Deserialize)]
struct Auto {
    on: bool,
    account: String,
    /// base64 of the 32 deriveSecret bytes
    secret: String,
}

struct Keys {
    key: crypto::SaveKey,
    writer: rare_wallet::keys::Keypair,
    writer_addr: String,
    account: String,
}

impl Keys {
    fn from_secret(secret: &[u8], account: &str) -> Result<Keys, String> {
        let key = crypto::derive_save_key(secret, account).map_err(|e| e.to_string())?;
        let seed = crypto::writer_seed(secret, account).map_err(|e| e.to_string())?;
        let writer = rare_wallet::keys::Keypair::from_bytes(rare_wallet::keys::FLAG_ED25519, &seed).map_err(|e| e.to_string())?;
        Ok(Keys { writer_addr: writer.address(), key, writer, account: account.to_string() })
    }
}

/// The writer as rare-sui's signer: builds, signs with its own key, executes. No review sheet:
/// it only ever signs the cloud saves transactions this file builds.
struct Writer(Arc<Keys>);

impl rare_sui::signer::TxSigner for Writer {
    fn address(&self) -> String {
        self.0.writer_addr.clone()
    }
    fn sign_personal_message(&self, _m: &[u8], _t: &str) -> rare_sui::BoxFut<'_, String> {
        Box::pin(async { Err(rare_sui::Error::invalid("not used")) })
    }
    fn sign_and_execute(&self, tx: rare_sui::ptb::Tx, _title: &str) -> rare_sui::BoxFut<'_, String> {
        Box::pin(async move {
            let sui = sui().ok_or_else(|| rare_sui::Error::invalid("no network"))?;
            let k = self.0.clone();
            rare_sui::tx::execute_signed(sui, &tx, &self.0.writer_addr, &move |b| k.writer.sign_transaction(b)).await
        })
    }
}

/// The conflict sheet's question: one or more save sets of a game changed on both sides.
#[derive(Clone, Debug)]
pub struct Conflict {
    pub game: String,
    pub title: String,
    /// launch the game once it is answered
    pub launch: bool,
    pub sets: Vec<ConflictSet>,
}

#[derive(Clone, Debug)]
pub struct ConflictSet {
    pub target: Target,
    pub local: Scan,
    pub cloud: Backup,
}

/// One kept copy, for the versions sheet.
#[derive(Clone, Debug, PartialEq)]
pub struct VersionRow {
    pub set: String,
    pub set_label: String,
    /// unix ms
    pub created: i64,
    pub bytes: u64,
    pub reason: String,
    pub local: Option<i64>,
    pub cloud: Option<String>,
    pub current: bool,
}

/// What the game page shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GameView {
    /// None until the first look finished
    pub known: bool,
    pub sets: usize,
    /// newest saves on this device, unix ms
    pub modified: Option<i64>,
    pub bytes: u64,
    /// all sets match the cloud
    pub synced: bool,
    /// when the cloud copy was last brought in line (unix ms)
    pub synced_at: Option<i64>,
    pub conflict: bool,
    pub pending: bool,
    pub versions: Vec<VersionRow>,
}

#[derive(Default)]
struct Inner {
    loaded: bool,
    on: bool,
    keys: Option<Arc<Keys>>,
    cloud: Vec<Backup>,
    cloud_at: f64,
    refreshing: bool,
    funds: Option<(u64, u64)>,
    busy: HashMap<String, &'static str>,
    errors: HashMap<String, String>,
    games: HashMap<String, GameView>,
    conflict: Option<Conflict>,
    cleared: HashSet<String>,
    out: Vec<Out>,
    index: Option<Arc<ludusavi::Index>>,
    unlocking: bool,
    sheet: Option<String>,
    sync: Option<SyncState>,
    /// uploads that failed: game, ES-DE or not, attempts, next try (clock secs)
    retry: HashMap<String, (Game, bool, u32, f64)>,
    /// data::keep's epoch last seen (moves when the network comes back or the device wakes)
    net_epoch: u64,
    /// next renewal check (clock secs)
    renew_at: f64,
}

/// Seconds before the next try after `attempts` failed uploads: 1, 2, 4 ... minutes, at most an hour.
pub fn backoff(attempts: u32) -> f64 {
    (60.0 * 2f64.powi(attempts.saturating_sub(1).min(6) as i32)).min(3600.0)
}

enum Out {
    Launch(String),
    Conflict,
    Note(String, bool),
}

static INNER: OnceLock<Mutex<Inner>> = OnceLock::new();
static VER: AtomicU64 = AtomicU64::new(0);
static INDEX_BUSY: AtomicBool = AtomicBool::new(false);
/// One sync job at a time (they share sync.json and the history).
static JOB: Mutex<()> = Mutex::new(());

fn inner() -> std::sync::MutexGuard<'static, Inner> {
    INNER.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

fn bump() {
    VER.fetch_add(1, Ordering::Relaxed);
    super::bus::wake();
}

fn with<T>(f: impl FnOnce(&mut Inner) -> T) -> T {
    let r = f(&mut inner());
    bump();
    r
}

pub fn ver() -> u64 {
    VER.load(Ordering::Relaxed)
}

fn spawn(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new().name("rare-cloudsync".into()).spawn(f).ok();
}

fn fixture() -> bool {
    std::env::var("RR_FIXTURE").is_ok()
}

// ---------------------------------------------------------------- state on disk

fn load_auto() -> Auto {
    std::fs::read(dir().join("auto.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_auto(a: &Auto) -> Result<(), String> {
    use std::io::Write;
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let p = dir().join("auto.json");
    let tmp = dir().join("auto.json.tmp");
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let mut f = o.open(&tmp).map_err(|e| e.to_string())?;
    f.write_all(&serde_json::to_vec(a).unwrap_or_default()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

fn sync_path() -> PathBuf {
    dir().join("sync.json")
}

fn cloud_path() -> PathBuf {
    dir().join("cloud.json")
}

fn base_of(set: &str) -> Option<Base> {
    let mut i = inner();
    if i.sync.is_none() {
        i.sync = Some(SyncState::load(&sync_path()));
    }
    i.sync.as_ref().and_then(|s| s.sets.get(set).cloned())
}

fn set_base(set: &str, b: Base) {
    let snapshot = {
        let mut i = inner();
        let s = i.sync.get_or_insert_with(|| SyncState::load(&sync_path()));
        s.sets.insert(set.to_string(), b);
        s.clone()
    };
    if let Err(e) = snapshot.save(&sync_path()) {
        log::warn!("cloudsync: save sync.json: {e}");
    }
}

// ---------------------------------------------------------------- boot and refresh

/// Once at start: the saved switch and keys, then the cloud list and the manifest in the background.
pub fn boot() {
    if fixture() {
        with(|i| {
            i.loaded = true;
            i.on = true;
        });
        return;
    }
    let a = load_auto();
    let keys = if a.on {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.decode(&a.secret).ok().and_then(|s| Keys::from_secret(&s, &a.account).map_err(|e| log::warn!("cloudsync keys: {e}")).ok()).map(Arc::new)
    } else {
        None
    };
    let cached: Vec<Backup> = std::fs::read(cloud_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    with(|i| {
        i.loaded = true;
        i.on = a.on && keys.is_some();
        i.keys = keys;
        i.cloud = cached;
    });
    if is_on() {
        spawn(|| {
            refresh_cloud(None);
        });
        ensure_index();
    }
}

pub fn is_on() -> bool {
    inner().on
}

/// The cloud list (both the writer's and the account's own backups), at most `timeout` waiting.
fn refresh_cloud(timeout: Option<std::time::Duration>) -> bool {
    let (keys, known) = {
        let mut i = inner();
        let Some(k) = i.keys.clone() else { return false };
        i.refreshing = true;
        (k, i.cloud.iter().map(|b| (b.blob_object_id.clone(), b.clone())).collect::<HashMap<_, _>>())
    };
    let Some(sui) = sui() else { return false };
    let owners = vec![keys.writer_addr.clone(), keys.account.clone()];
    let fut = async {
        let n = sui.network();
        let list = saves::walrus::list_backups_known(sui, n, &owners, &keys.key, &known).await;
        let funds = futures_balance(sui, &keys.writer_addr).await;
        (list, funds)
    };
    let r = live::rt().block_on(async {
        match timeout {
            Some(t) => tokio::time::timeout(t, fut).await.ok(),
            None => Some(fut.await),
        }
    });
    let ok = matches!(r, Some((Ok(_), _)));
    match r {
        Some((Ok(list), funds)) => {
            let _ = std::fs::create_dir_all(dir());
            let _ = std::fs::write(cloud_path(), serde_json::to_vec(&list).unwrap_or_default());
            with(|i| {
                i.cloud = list;
                i.cloud_at = rare_render::clock::secs();
                i.refreshing = false;
                if funds.is_some() {
                    i.funds = funds;
                }
            });
        }
        Some((Err(e), _)) => {
            log::warn!("cloudsync: list: {e}");
            with(|i| i.refreshing = false);
        }
        None => with(|i| i.refreshing = false),
    }
    ok
}

async fn futures_balance(sui: &rare_sui::Sui, owner: &str) -> Option<(u64, u64)> {
    use rare_sui::grpc::ChainRead;
    let wal = sui.balance(owner, saves::store::wal_type(sui.network())).await.ok()?.balance;
    let s = sui.balance(owner, "0x2::sui::SUI").await.ok()?.balance;
    Some((wal, s))
}

fn index_path() -> PathBuf {
    local::home().unwrap_or_default().join(".cache/rare/ludusavi.json")
}

/// The Ludusavi index: from the cache, fetched again weekly (only when the manifest changed).
fn ensure_index() {
    if inner().index.is_some() || INDEX_BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    spawn(|| {
        let p = index_path();
        let cached: Option<ludusavi::Index> = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice(&b).ok());
        let fresh = std::fs::metadata(&p).and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok()).is_some_and(|e| e.as_secs() < 7 * 86_400);
        if let Some(idx) = cached {
            with(|i| i.index = Some(Arc::new(idx)));
            if fresh {
                INDEX_BUSY.store(false, Ordering::SeqCst);
                return;
            }
        }
        let etag_p = p.with_extension("etag");
        let etag = std::fs::read_to_string(&etag_p).unwrap_or_default();
        let r = live::rt().block_on(async {
            let mut req = reqwest::Client::new().get(ludusavi::MANIFEST_URL).timeout(std::time::Duration::from_secs(120));
            if !etag.is_empty() && inner().index.is_some() {
                req = req.header("If-None-Match", etag.trim());
            }
            let r = req.send().await?;
            let tag = r.headers().get("etag").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
            if r.status() == reqwest::StatusCode::NOT_MODIFIED {
                return Ok::<_, reqwest::Error>(None);
            }
            let text = r.error_for_status()?.text().await?;
            Ok(Some((text, tag)))
        });
        match r {
            Ok(Some((text, tag))) => {
                let idx = ludusavi::parse(&text);
                if let Some(dir) = p.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(&p, serde_json::to_vec(&idx).unwrap_or_default());
                let _ = std::fs::write(&etag_p, tag);
                log::info!("cloudsync: Ludusavi index, {} games", idx.games.len());
                with(|i| i.index = Some(Arc::new(idx)));
            }
            Ok(None) => {
                // unchanged: touch the cache so the week starts again
                if let Ok(b) = std::fs::read(&p) {
                    let _ = std::fs::write(&p, b);
                }
            }
            Err(e) => log::warn!("cloudsync: manifest: {e}"),
        }
        INDEX_BUSY.store(false, Ordering::SeqCst);
    });
}

// ---------------------------------------------------------------- what a game syncs

/// Whether Rare syncs this game's saves itself (Steam, Epic and GOG sync their own).
pub fn eligible(g: &Game) -> bool {
    !matches!(g.source.as_str(), "steam" | "epic" | "gog") && !g.id.starts_with("app:")
}

fn prefix_of(id: &str) -> PathBuf {
    std::env::var("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|_| local::home().unwrap_or_default().join(".local/share")).join("rare/prefixes").join(id.replace(':', "-"))
}

/// The save sets of a game on this device, plus the ones only the cloud has (a new device).
fn targets_for(g: &Game, emulation_app: bool, index: Option<&ludusavi::Index>, cloud: &[Backup]) -> Vec<Target> {
    let mut out = Vec::new();
    let from_cloud = |pred: &dyn Fn(&BackupMeta) -> bool, out: &mut Vec<Target>| {
        for b in cloud {
            let m = &b.meta;
            if pred(m) && !out.iter().any(|t: &Target| t.id() == m.id) {
                if let Some(s) = local::set_from(&m.id, &m.rel, &m.emulator, &m.label, &m.kind) {
                    out.push(Target::Dir(s));
                }
            }
        }
    };
    if g.source == "web" {
        let safe: String = g.id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        let id = format!("webgame:{safe}");
        let rel = format!(".local/share/rare/webgames/{safe}");
        if let Some(s) = local::detect().unwrap_or_default().into_iter().find(|s| s.id == id).or_else(|| local::set_from(&id, &rel, "Web game", &g.title, "webgame")) {
            out.push(Target::Dir(local::SaveSet { label: g.title.clone(), ..s }));
        }
        return out;
    }
    // A ROM: its emulator's save sets, and EmuDeck's folder for it under ~/Emulation/saves.
    if g.source == "emu" {
        if let Some((name, folder)) = rare_emu::roms::save_owner(&g.id) {
            let deck = format!("EmuDeck {folder}");
            let want = |e: &str| e == name || e.eq_ignore_ascii_case(&deck);
            for s in local::detect().unwrap_or_default() {
                if s.kind == "emulator" && want(&s.emulator) {
                    out.push(Target::Dir(s));
                }
            }
            from_cloud(&|m| m.kind == "emulator" && want(&m.emulator), &mut out);
        }
        return out;
    }
    let names = local::emulator_names();
    let title = g.title.to_lowercase();
    let emu: Option<&str> = names.iter().copied().find(|n| title.contains(&n.to_lowercase()));
    if emulation_app || (g.source == "apps" && emu.is_some()) {
        let want = |e: &str| emulation_app || Some(e) == emu;
        for s in local::detect().unwrap_or_default() {
            if s.kind == "emulator" && want(&s.emulator) {
                out.push(Target::Dir(s));
            }
        }
        from_cloud(&|m| m.kind == "emulator" && want(&m.emulator), &mut out);
        return out;
    }
    let Some(idx) = index else { return out };
    let base = g.install_dir.as_ref().map(PathBuf::from);
    let Some(entry) = idx.find(&g.title, base.as_deref()) else { return out };
    // A non-Steam shortcut Steam runs through Proton keeps its prefix in compatdata/<appid>/pfx.
    let shortcut = g.launch.as_ref().filter(|l| l.kind == "steamShortcut").and_then(|l| l.gameid).and_then(|id| shortcut_prefix((id >> 32) as u32));
    let windows = shortcut.is_some() || g.launch.as_ref().is_some_and(|l| l.windows);
    let home = local::home().unwrap_or_default();
    let user = std::env::var("USER").unwrap_or_else(|_| "deck".into());
    let prefix = shortcut.or_else(|| windows.then(|| prefix_of(&g.id)));
    let cx = ludusavi::Ctx { home, prefix, base, windows, user };
    let roots = ludusavi::roots(entry, &cx);
    if !roots.is_empty() {
        out.push(Target::Game(GameSet { id: format!("game:{}", g.id), title: g.title.clone(), roots }));
    }
    out
}

/// The Proton prefix Steam made for a non-Steam shortcut (`steamapps/compatdata/<appid>/pfx`).
fn shortcut_prefix(appid: u32) -> Option<PathBuf> {
    rare_library::steam::find_roots().into_iter().map(|r| r.path.join("steamapps/compatdata").join(appid.to_string()).join("pfx")).find(|p| p.join("drive_c").is_dir())
}

fn meta_for(t: &Target, scan: &Scan) -> BackupMeta {
    let mut m = match t {
        Target::Dir(s) => BackupMeta { v: 1, id: s.id.clone(), rel: s.rel.clone(), emulator: s.emulator.clone(), label: s.label.clone(), kind: s.kind.clone(), ..Default::default() },
        Target::Game(g) => BackupMeta { v: 1, id: g.id.clone(), rel: String::new(), emulator: "PC".into(), label: g.title.clone(), kind: "game".into(), ..Default::default() },
    };
    m.files = scan.files;
    m.created = now_ms();
    m.fp = Some(scan.fp.clone());
    m.modified = scan.modified;
    m.how = Some("auto".into());
    m.raw = Some(scan.bytes);
    m
}

fn newest<'a>(cloud: &'a [Backup], set: &str) -> Option<&'a Backup> {
    cloud.iter().filter(|b| b.meta.id == set).max_by_key(|b| b.meta.created)
}

fn game_ctx(st: &State, g: &Game) -> bool {
    st.library.as_ref().and_then(|l| l.emulation_app.as_deref()) == Some(g.id.as_str())
}

// ---------------------------------------------------------------- the jobs (worker threads)

fn http() -> reqwest::Client {
    reqwest::Client::builder().timeout(std::time::Duration::from_secs(120)).build().unwrap_or_default()
}

/// Keeps the saves on this device in the history (unless the newest kept copy is the same) and
/// answers that copy. None when there are no saves.
fn keep_local(t: &Target, reason: &str) -> Result<Option<(Vec<u8>, Scan)>, String> {
    let scan = t.scan()?;
    if scan.files == 0 {
        return Ok(None);
    }
    if scan.bytes > MAX_AUTO_BYTES {
        return Err(format!("{} saves are too large to keep automatically", t.label()));
    }
    let h = history();
    if let Some(top) = h.list(t.id()).into_iter().next().filter(|v| v.fp == scan.fp) {
        if let Ok(data) = h.read(t.id(), top.created) {
            return Ok(Some((data, scan)));
        }
    }
    let (data, scan) = t.pack()?;
    h.add(t.id(), &data, &scan, reason, now_ms())?;
    Ok(Some((data, scan)))
}

fn download(keys: &Keys, t: &Target, b: &Backup) -> Result<Scan, String> {
    let sui = sui().ok_or("no network")?;
    let data = live::rt().block_on(saves::walrus::fetch_backup(&http(), sui.network(), &keys.key, b)).map_err(|e| e.to_string())?;
    keep_local(t, "download")?;
    t.restore(&local::home()?, &data)?;
    let scan = t.scan()?;
    history().add(t.id(), &data, &scan, "cloud", now_ms()).ok();
    set_base(t.id(), Base { fp: scan.fp.clone(), cloud: Some(b.blob_object_id.clone()), at: now_ms() });
    Ok(scan)
}

fn upload(keys: &Arc<Keys>, t: &Target, data: &[u8], scan: &Scan) -> Result<Backup, String> {
    if testnet() && std::env::var("RARE_SAVES_E2E_OFFLINE").is_ok() {
        return Err("network unreachable (end to end check)".into());
    }
    let sui = sui().ok_or("no network")?;
    let w = Writer(keys.clone());
    let epochs = keep_epochs();
    let b = live::rt().block_on(saves::upload(sui, &http(), &w, &keys.key, meta_for(t, scan), data, epochs, REUSE_LEFT_EPOCHS)).map_err(|e| e.to_string())?;
    set_base(t.id(), Base { fp: scan.fp.clone(), cloud: Some(b.blob_object_id.clone()), at: now_ms() });
    // Older automatic copies of this set go, leaving their space for the next upload.
    let old: Vec<String> = {
        let mut i = inner();
        i.cloud.insert(0, b.clone());
        let mine: Vec<&Backup> = sync::versions_of(&i.cloud, t.id()).into_iter().filter(|x| x.meta.how.as_deref() == Some("auto")).collect();
        mine.iter().skip(sync::KEEP_CLOUD).map(|x| x.blob_object_id.clone()).collect()
    };
    for id in old {
        match live::rt().block_on(saves::delete(sui, &w, &id)) {
            Ok(_) => with(|i| i.cloud.retain(|x| x.blob_object_id != id)),
            Err(e) => log::warn!("cloudsync: delete {id}: {e}"),
        }
    }
    let _ = std::fs::write(cloud_path(), serde_json::to_vec(&inner().cloud).unwrap_or_default());
    Ok(b)
}

fn no_funds(e: &str) -> bool {
    let e = e.to_lowercase();
    e.contains("insufficient") || e.contains("balance") || e.contains("no gas") || e.contains("gas coin") || e.contains("not enough")
}

/// Before a launch: bring the cloud copy down, or ask. Then launch.
fn job_before(g: Game, emulation_app: bool) {
    let _job = JOB.lock().unwrap_or_else(|e| e.into_inner());
    let id = g.id.clone();
    let keys = inner().keys.clone();
    let Some(keys) = keys else {
        with(|i| {
            i.busy.remove(&id);
            i.cleared.insert(id.clone());
            i.out.push(Out::Launch(id.clone()));
        });
        return;
    };
    let (cloud0, index0) = {
        let i = inner();
        (i.cloud.clone(), i.index.clone())
    };
    // Nothing of this game's here or in the cached list: a short look at the cloud only (another
    // device may have uploaded it since), so unknown games start without a noticeable wait.
    let first = index0.is_some() && targets_for(&g, emulation_app, index0.as_deref(), &cloud0).is_empty();
    let fresh = refresh_cloud(Some(std::time::Duration::from_secs(if first { 2 } else { 4 })));
    let (cloud, index) = {
        let i = inner();
        (i.cloud.clone(), i.index.clone())
    };
    let targets = targets_for(&g, emulation_app, index.as_deref(), &cloud);
    let mut conflicts = Vec::new();
    let mut notes = Vec::new();
    for t in &targets {
        let scan = match t.scan() {
            Ok(s) => s,
            Err(e) => {
                notes.push(e);
                continue;
            }
        };
        let base = base_of(t.id());
        let top = newest(&cloud, t.id());
        match sync::plan(&scan, base.as_ref(), top) {
            Plan::Download => {
                with(|i| {
                    i.busy.insert(id.clone(), "Getting your cloud save");
                });
                if let Err(e) = download(&keys, t, top.unwrap()) {
                    notes.push(format!("Couldn't get the cloud save for {}: {e}", t.label()));
                }
            }
            Plan::Conflict => conflicts.push(ConflictSet { target: t.clone(), local: scan, cloud: top.unwrap().clone() }),
            Plan::InSync => {
                let cloud_id = top.map(|b| b.blob_object_id.clone());
                if base.as_ref().map(|b| (&b.fp, &b.cloud)) != Some((&scan.fp, &cloud_id)) {
                    set_base(t.id(), Base { fp: scan.fp, cloud: cloud_id, at: now_ms() });
                }
            }
            Plan::Upload | Plan::Nothing => {}
        }
    }
    if !fresh && !targets.is_empty() {
        log::info!("cloudsync: started {id} with the cached cloud list");
    }
    with(|i| {
        i.busy.remove(&id);
        for n in notes {
            i.out.push(Out::Note(n, true));
        }
        if conflicts.is_empty() {
            i.cleared.insert(id.clone());
            i.out.push(Out::Launch(id.clone()));
        } else {
            i.conflict = Some(Conflict { game: id.clone(), title: g.title.clone(), launch: true, sets: conflicts });
            i.out.push(Out::Conflict);
        }
    });
    look(g, emulation_app);
}

/// After a session: keep a copy, upload what changed here.
fn job_after(g: Game, emulation_app: bool) {
    let _job = JOB.lock().unwrap_or_else(|e| e.into_inner());
    let id = g.id.clone();
    if is_on() {
        // Another device may have uploaded meanwhile: then this is a conflict, not an upload.
        refresh_cloud(Some(std::time::Duration::from_secs(10)));
    }
    let (keys, on, cloud, index) = {
        let i = inner();
        (i.keys.clone(), i.on, i.cloud.clone(), i.index.clone())
    };
    let targets = targets_for(&g, emulation_app, index.as_deref(), &cloud);
    let mut err: Option<String> = None;
    let mut retry = false;
    for t in &targets {
        let kept = match keep_local(t, "played") {
            Ok(k) => k,
            Err(e) => {
                err = Some(e);
                continue;
            }
        };
        let (Some(keys), true, Some((data, scan))) = (keys.as_ref(), on, kept) else { continue };
        let base = base_of(t.id());
        match sync::plan(&scan, base.as_ref(), newest(&cloud, t.id())) {
            Plan::Upload if inner().funds.is_some_and(|f| uploads_left(f) == 0) => {
                err = Some("Cloud saves need funds. Add WAL and SUI in Saves.".into());
            }
            Plan::Upload => {
                with(|i| {
                    i.busy.insert(id.clone(), "Saving to the cloud");
                });
                if let Err(e) = upload(keys, t, &data, &scan) {
                    log::warn!("cloudsync: upload {}: {e}", t.id());
                    if no_funds(&e) {
                        err = Some("Cloud saves need funds. Add WAL and SUI in Saves.".into());
                    } else {
                        err = Some("Couldn't upload yet. Rare tries again shortly.".into());
                        retry = true;
                    }
                }
            }
            Plan::InSync => set_base(t.id(), Base { fp: scan.fp.clone(), cloud: newest(&cloud, t.id()).map(|b| b.blob_object_id.clone()), at: now_ms() }),
            _ => {}
        }
    }
    let now = rare_render::clock::secs();
    with(|i| {
        i.busy.remove(&id);
        match err {
            Some(e) => {
                i.errors.insert(id.clone(), e);
            }
            None => {
                i.errors.remove(&id);
            }
        }
        if retry {
            let n = i.retry.get(&id).map_or(0, |r| r.2) + 1;
            i.retry.insert(id.clone(), (g.clone(), emulation_app, n, now + backoff(n)));
        } else {
            i.retry.remove(&id);
        }
    });
    if let Some(k) = keys {
        if let Some(sui) = sui() {
            let f = live::rt().block_on(futures_balance(sui, &k.writer_addr));
            with(|i| i.funds = f.or(i.funds));
        }
    }
    look(g, emulation_app);
}

/// The game page's facts: status and versions, from disk and the cached cloud list.
fn look(g: Game, emulation_app: bool) {
    let (cloud, index, on) = {
        let i = inner();
        (i.cloud.clone(), i.index.clone(), i.on)
    };
    let targets = targets_for(&g, emulation_app, index.as_deref(), &cloud);
    let mut v = GameView { known: true, sets: targets.len(), synced: on && !targets.is_empty(), ..Default::default() };
    let h = history();
    for t in &targets {
        let scan = t.scan().unwrap_or_default();
        v.bytes += scan.bytes;
        v.modified = v.modified.max(scan.modified);
        let base = base_of(t.id());
        let top = newest(&cloud, t.id());
        match sync::plan(&scan, base.as_ref(), top) {
            Plan::InSync | Plan::Nothing => {
                v.synced_at = v.synced_at.max(base.as_ref().map(|b| b.at)).max(top.map(|b| b.meta.created));
            }
            Plan::Conflict => {
                v.conflict = true;
                v.synced = false;
            }
            Plan::Upload | Plan::Download => {
                v.pending = true;
                v.synced = false;
            }
        }
        let label = match t {
            Target::Dir(s) if s.kind == "emulator" && targets.len() > 1 => format!("{} {}", s.emulator, s.label.to_lowercase()),
            _ => String::new(),
        };
        let locals = h.list(t.id());
        let clouds = sync::versions_of(&cloud, t.id());
        let mut rows: Vec<VersionRow> = Vec::new();
        for l in &locals {
            let c = clouds.iter().find(|c| c.meta.fp.as_deref() == Some(l.fp.as_str()));
            rows.push(VersionRow { set: t.id().to_string(), set_label: label.clone(), created: l.created, bytes: l.bytes, reason: l.reason.clone(), local: Some(l.created), cloud: c.map(|c| c.blob_object_id.clone()), current: l.fp == scan.fp });
        }
        for c in clouds {
            if rows.iter().any(|r| r.cloud.as_deref() == Some(c.blob_object_id.as_str())) {
                continue;
            }
            rows.push(VersionRow { set: t.id().to_string(), set_label: label.clone(), created: c.meta.created, bytes: c.meta.raw.unwrap_or(c.meta.bytes), reason: "cloud".into(), local: None, cloud: Some(c.blob_object_id.clone()), current: c.meta.fp.as_deref() == Some(scan.fp.as_str()) });
        }
        v.versions.extend(rows);
    }
    v.versions.sort_by_key(|r| std::cmp::Reverse(r.created));
    with(|i| {
        i.games.insert(g.id.clone(), v);
    });
}

// ---------------------------------------------------------------- the UI thread

/// Play pressed. True: start now. False: cloud saves look first and start it after (or ask).
pub fn before_launch(st: &mut State, g: &Game) -> bool {
    if fixture() || !eligible(g) {
        return true;
    }
    {
        let mut i = inner();
        if i.cleared.remove(&g.id) {
            return true;
        }
        if !i.on {
            return true;
        }
        if i.busy.contains_key(&g.id) {
            return false;
        }
        i.busy.insert(g.id.clone(), "Checking cloud saves");
    }
    bump();
    st.launching = Some(g.id.clone());
    let (g, emu) = (g.clone(), game_ctx(st, g));
    spawn(move || job_before(g, emu));
    false
}

/// The running list changed: games that stopped get their saves kept and uploaded.
pub fn on_running(st: &State, before: &[String], after: &[String]) {
    if fixture() {
        return;
    }
    for id in before.iter().filter(|id| !after.contains(id)) {
        let Some(g) = st.game(id).filter(|g| eligible(g)).cloned() else { continue };
        let emu = game_ctx(st, &g);
        spawn(move || job_after(g, emu));
    }
}

/// The game page asks for its facts (once per visit).
pub fn load(st: &State, g: &Game) {
    if fixture() || !eligible(g) {
        return;
    }
    if is_on() {
        ensure_index();
    }
    let (g, emu) = (g.clone(), game_ctx(st, g));
    spawn(move || look(g, emu));
}

pub fn game(id: &str) -> Option<GameView> {
    if fixture() {
        return Some(fixture_view(id));
    }
    inner().games.get(id).cloned()
}

pub fn busy(id: &str) -> Option<&'static str> {
    inner().busy.get(id).copied()
}

pub fn error(id: &str) -> Option<String> {
    inner().errors.get(id).cloned()
}

pub fn conflict() -> Option<Conflict> {
    if fixture() && inner().conflict.is_none() {
        return Some(fixture_conflict());
    }
    inner().conflict.clone()
}

pub fn funds() -> Option<(u64, u64)> {
    inner().funds
}

pub fn writer() -> Option<String> {
    inner().keys.as_ref().map(|k| k.writer_addr.clone())
}

pub fn unlocking() -> bool {
    inner().unlocking
}

/// About how many automatic uploads the writer's funds pay for.
pub fn uploads_left(f: (u64, u64)) -> u64 {
    (f.0 / UPLOAD_FROST).min(f.1 / UPLOAD_MIST)
}

/// Every update: launches that were waiting on cloud saves, the conflict sheet, notes.
pub fn poll(st: &mut State, now: f64) {
    let (out, stale) = {
        let mut i = inner();
        let stale = i.on && !i.refreshing && now - i.cloud_at > 600.0 && st.running.is_empty();
        if stale {
            i.refreshing = true;
        }
        (std::mem::take(&mut i.out), stale)
    };
    if stale {
        spawn(|| {
            refresh_cloud(None);
        });
    }
    // Failed uploads: on their backoff, or at once when the network comes back.
    let epoch = crate::data::keep::epoch();
    let due: Vec<(Game, bool)> = {
        let mut i = inner();
        if epoch != i.net_epoch {
            i.net_epoch = epoch;
            for r in i.retry.values_mut() {
                r.3 = r.3.min(now);
            }
        }
        let running = &st.running;
        let due: Vec<String> = i.retry.iter().filter(|(id, r)| r.3 <= now && !running.contains(id) && !i.busy.contains_key(*id)).map(|(id, _)| id.clone()).collect();
        due.iter()
            .filter_map(|id| {
                let r = i.retry.get_mut(id)?;
                r.3 = now + backoff(r.2 + 1);
                Some((r.0.clone(), r.1))
            })
            .collect()
    };
    for (g, emu) in due {
        spawn(move || job_after(g, emu));
    }
    // Keep the newest copy of every save set alive: checked every 6 hours.
    let renew = {
        let mut i = inner();
        let go = i.on && st.running.is_empty() && now >= i.renew_at && i.cloud_at > 0.0;
        if go {
            i.renew_at = now + 6.0 * 3600.0;
        }
        go
    };
    if renew {
        spawn(|| {
            let r = renew_now();
            if !r.is_empty() {
                log::info!("cloudsync: renewed {r:?}");
            }
        });
    }
    if !out.is_empty() {
        super::bus::wake();
    }
    for o in out {
        match o {
            Out::Launch(id) => st.retry = Some(id),
            Out::Conflict => {
                st.launching = None;
                st.open(Some(Overlay::Other(CONFLICT)));
            }
            Out::Note(t, err) => st.toast(t, if err { Tone::Error } else { Tone::Info }, now),
        }
    }
}

/// Renews (extends) the newest cloud copy of each save set the writer holds when it has
/// RENEW_WHEN epochs or fewer left, so it lasts about KEEP_DAYS again. Answers (set, epochs added).
pub fn renew_now() -> Vec<(String, u32)> {
    const RENEW_WHEN: u64 = 4;
    let _job = JOB.lock().unwrap_or_else(|e| e.into_inner());
    let (Some(keys), Some(sui)) = (inner().keys.clone(), sui()) else { return vec![] };
    refresh_cloud(Some(std::time::Duration::from_secs(20)));
    let cloud = inner().cloud.clone();
    let Ok(now) = live::rt().block_on(saves::walrus::current_epoch(sui, sui.network())) else { return vec![] };
    let want = u64::from(saves::meta::epochs_for(KEEP_DAYS, testnet()));
    let mut sets: Vec<&str> = cloud.iter().map(|b| b.meta.id.as_str()).collect();
    sets.sort();
    sets.dedup();
    let w = Writer(keys.clone());
    let mut done = Vec::new();
    for set in sets {
        let Some(b) = newest(&cloud, set) else { continue };
        // Only the writer's copies: the account's own manual backups need its approval.
        if b.meta.how.as_deref() != Some("auto") {
            continue;
        }
        let Some(end) = b.end_epoch else { continue };
        let add = saves::renewal(end, now, RENEW_WHEN, want);
        if add == 0 {
            continue;
        }
        match live::rt().block_on(saves::extend(sui, &w, b, add)) {
            Ok(_) => done.push((set.to_string(), add)),
            Err(e) => log::warn!("cloudsync: renew {set}: {e}"),
        }
    }
    if !done.is_empty() {
        drop(_job);
        refresh_cloud(Some(std::time::Duration::from_secs(20)));
    }
    done
}

/// Keeps the secret from `deriveSecret("cloud-saves")` on this device and turns automatic sync on.
pub fn install(secret: &[u8], account: &str) -> Result<String, String> {
    let keys = Keys::from_secret(secret, account)?;
    use base64::Engine;
    save_auto(&Auto { on: true, account: account.to_string(), secret: base64::engine::general_purpose::STANDARD.encode(secret) })?;
    let addr = keys.writer_addr.clone();
    with(|i| {
        i.loaded = true;
        i.on = true;
        i.keys = Some(Arc::new(keys));
        i.games.clear();
    });
    Ok(addr)
}

/// Turn on: one wallet signature makes the save key and the writer; kept on this device.
pub fn turn_on() {
    let Some(addr) = service::wallet().and_then(|w| w.keystore.current_signing_account().ok().flatten()).map(|a| a.address) else {
        super::bus::note("Add a Rare Wallet account you can sign with first", true);
        return;
    };
    if std::mem::replace(&mut inner().unlocking, true) {
        return;
    }
    bump();
    let a2 = addr.clone();
    let rx = service::work(move |w| {
        let mut auth = rare_wallet::passkey_account::Ctap { state: &w.passkey, sink: w.sink(), rt: w.rt.clone() };
        let via = *w.via.lock().unwrap();
        rare_wallet::secret::derive::derive_secret(&w.signing, &mut auth, via, crypto::SAVES_CONTEXT, Some(&a2), Some("Turn on cloud saves")).map(|s| s.to_vec()).map_err(|e| e.to_string())
    });
    spawn(move || {
        let r = rx.recv().unwrap_or_else(|_| Err("The wallet stopped. Try again.".into())).and_then(|r| r);
        match r.and_then(|secret| install(&secret, &addr)) {
            Ok(_) => {
                with(|i| i.unlocking = false);
                ensure_index();
                refresh_cloud(None);
                let broke = funds().is_none_or(|f| uploads_left(f) == 0);
                super::bus::note(if broke { "Cloud saves are on. Add funds so Rare can upload." } else { "Cloud saves are on" }, false);
            }
            Err(e) => {
                with(|i| i.unlocking = false);
                super::bus::note(e, true);
            }
        }
    });
}

/// Turn off: no more automatic syncs. The keys are forgotten on this device; cloud copies stay.
pub fn turn_off() {
    let _ = save_auto(&Auto::default());
    with(|i| {
        i.on = false;
        i.keys = None;
        i.games.clear();
    });
}

/// Sends WAL and SUI from the wallet to the writer (one review).
pub fn add_funds() {
    let (Some(writer), Some(acct)) = (writer(), inner().keys.as_ref().map(|k| k.account.clone())) else { return };
    spawn(move || {
        let mut tx = rare_sui::ptb::Tx::new();
        tx.set_sender(&acct);
        let s = tx.split_u64(rare_sui::ptb::Arg::Gas, FUND_MIST);
        let w = tx.coin_with_balance(saves::store::wal_type(sui().map(|s| s.network()).unwrap_or(rare_sui::Network::Mainnet)), FUND_FROST);
        tx.transfer_to(&[s, w], &writer);
        match service::send(rare_wallet::sign::TxInput::Ptb(tx), &acct, "Add funds for cloud saves") {
            Ok(_) => {
                if let Some(sui) = sui() {
                    let f = live::rt().block_on(futures_balance(sui, &writer));
                    with(|i| i.funds = f.or(i.funds));
                }
                super::bus::note("Funds added for cloud saves", false);
            }
            Err(e) => super::bus::note(e.to_string(), true),
        }
    });
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Keep {
    Device,
    Cloud,
}

/// The conflict sheet's answer. None: not now (the game does not start).
pub fn resolve(st: &mut State, keep: Option<Keep>) {
    let Some(c) = inner().conflict.take() else { return };
    bump();
    st.open(None);
    let Some(keep) = keep else { return };
    let Some(g) = st.game(&c.game).cloned() else { return };
    let emu = game_ctx(st, &g);
    with(|i| {
        i.busy.insert(c.game.clone(), if keep == Keep::Cloud { "Getting your cloud save" } else { "Saving to the cloud" });
    });
    if c.launch {
        st.launching = Some(c.game.clone());
    }
    spawn(move || resolve_job(c, keep, g, emu));
}

fn resolve_job(c: Conflict, keep: Keep, g: Game, emu: bool) {
    let _job = JOB.lock().unwrap_or_else(|e| e.into_inner());
    let keys = inner().keys.clone();
    let mut err = None;
    if let Some(keys) = keys {
        for s in &c.sets {
            let r = match keep {
                Keep::Cloud => download(&keys, &s.target, &s.cloud).map(|_| ()),
                Keep::Device => keep_local(&s.target, "conflict").and_then(|k| match k {
                    Some((data, scan)) => upload(&keys, &s.target, &data, &scan).map(|_| ()),
                    None => Ok(()),
                }),
            };
            if let Err(e) = r {
                err = Some(e);
            }
        }
    }
    with(|i| {
        i.busy.remove(&c.game);
        if let Some(e) = &err {
            i.out.push(Out::Note(if keep == Keep::Device && no_funds(e) { "Kept this device's save. Cloud saves need funds to upload it.".into() } else { format!("Couldn't finish: {e}") }, true));
        }
        if c.launch {
            i.cleared.insert(c.game.clone());
            i.out.push(Out::Launch(c.game.clone()));
        }
    });
    drop(_job);
    look(g, emu);
}

/// The game page's Resolve: the conflict sheet for a game whose saves changed on both sides.
pub fn ask_conflict(st: &mut State, id: &str) {
    let Some(g) = st.game(id).cloned() else { return };
    let emu = game_ctx(st, &g);
    spawn(move || {
        let (cloud, index) = {
            let i = inner();
            (i.cloud.clone(), i.index.clone())
        };
        let sets: Vec<ConflictSet> = targets_for(&g, emu, index.as_deref(), &cloud)
            .into_iter()
            .filter_map(|t| {
                let scan = t.scan().ok()?;
                let top = newest(&cloud, t.id())?;
                (sync::plan(&scan, base_of(t.id()).as_ref(), Some(top)) == Plan::Conflict).then(|| ConflictSet { local: scan, cloud: top.clone(), target: t })
            })
            .collect();
        if !sets.is_empty() {
            with(|i| {
                i.conflict = Some(Conflict { game: g.id.clone(), title: g.title.clone(), launch: false, sets });
                i.out.push(Out::Conflict);
            });
        }
    });
}

/// Opens the versions sheet for a game.
pub fn open_versions(st: &mut State, id: &str) {
    with(|i| i.sheet = Some(id.to_string()));
    st.open(Some(Overlay::Other(VERSIONS)));
}

pub fn sheet_game() -> Option<String> {
    inner().sheet.clone()
}

/// Puts a kept copy back (the current saves are kept first), then uploads it when cloud saves are on.
pub fn restore(st: &mut State, id: &str, row: &VersionRow) {
    let Some(g) = st.game(id).cloned() else { return };
    let emu = game_ctx(st, &g);
    let row = row.clone();
    with(|i| {
        i.busy.insert(id.to_string(), "Restoring");
    });
    spawn(move || restore_job(g, emu, row));
}

fn restore_job(g: Game, emu: bool, row: VersionRow) {
    let _job = JOB.lock().unwrap_or_else(|e| e.into_inner());
    let (cloud, index, keys, on) = {
        let i = inner();
        (i.cloud.clone(), i.index.clone(), i.keys.clone(), i.on)
    };
    let r = (|| -> Result<(), String> {
        let t = targets_for(&g, emu, index.as_deref(), &cloud).into_iter().find(|t| t.id() == row.set).ok_or("These saves are not on this device")?;
        let data = match (row.local, &row.cloud, &keys) {
            (Some(created), _, _) => history().read(&row.set, created)?,
            (None, Some(blob), Some(k)) => {
                let b = cloud.iter().find(|b| &b.blob_object_id == blob).ok_or("That cloud copy is gone")?;
                let sui = sui().ok_or("no network")?;
                live::rt().block_on(saves::walrus::fetch_backup(&http(), sui.network(), &k.key, b)).map_err(|e| e.to_string())?
            }
            _ => return Err("Turn on cloud saves to get this copy".into()),
        };
        keep_local(&t, "restore")?;
        t.restore(&local::home()?, &data)?;
        if let (Some(k), true) = (keys.as_ref(), on) {
            let (data, scan) = t.pack()?;
            history().add(t.id(), &data, &scan, "restored", now_ms()).ok();
            if let Err(e) = upload(k, &t, &data, &scan) {
                log::warn!("cloudsync: upload after restore: {e}");
            }
        }
        Ok(())
    })();
    with(|i| {
        i.busy.remove(&g.id);
        i.out.push(match &r {
            Ok(()) => Out::Note("Restored. The saves you had are kept in Versions.".into(), false),
            Err(e) => Out::Note(format!("Couldn't restore: {e}"), true),
        });
    });
    drop(_job);
    look(g, emu);
}

// ---------------------------------------------------------------- the end to end check

/// Handles for src/bin/cloudsaves_e2e.rs: the same jobs, run on the caller's thread, against
/// Walrus testnet (RARE_SAVES_NETWORK=testnet) in a throwaway home folder.
pub mod e2e {
    use super::*;

    /// What the jobs asked the UI to do: "launch:<id>", "conflict", "note:<text>".
    pub fn take() -> Vec<String> {
        std::mem::take(&mut inner().out)
            .into_iter()
            .map(|o| match o {
                Out::Launch(id) => format!("launch:{id}"),
                Out::Conflict => "conflict".into(),
                Out::Note(t, _) => format!("note:{t}"),
            })
            .collect()
    }
    pub fn before(g: &Game, emu: bool) -> Vec<String> {
        job_before(g.clone(), emu);
        take()
    }
    pub fn after(g: &Game, emu: bool) -> Vec<String> {
        job_after(g.clone(), emu);
        take()
    }
    pub fn conflict() -> Option<Conflict> {
        inner().conflict.clone()
    }
    pub fn resolve(g: &Game, emu: bool, keep: Keep) -> Vec<String> {
        let c = inner().conflict.take();
        if let Some(c) = c {
            resolve_job(c, keep, g.clone(), emu);
        }
        take()
    }
    pub fn restore(g: &Game, emu: bool, row: &VersionRow) -> Vec<String> {
        restore_job(g.clone(), emu, row.clone());
        take()
    }
    pub fn view(g: &Game, emu: bool) -> GameView {
        look(g.clone(), emu);
        game(&g.id).unwrap_or_default()
    }
    pub fn cloud() -> Vec<Backup> {
        refresh_cloud(Some(std::time::Duration::from_secs(30)));
        inner().cloud.clone()
    }
    pub fn error(id: &str) -> Option<String> {
        super::error(id)
    }
    /// (attempts, seconds until the next try)
    pub fn retry(id: &str) -> Option<(u32, f64)> {
        inner().retry.get(id).map(|r| (r.2, r.3 - rare_render::clock::secs()))
    }
    pub fn writer() -> Option<Box<dyn rare_sui::signer::TxSigner>> {
        inner().keys.clone().map(|k| Box::new(Writer(k)) as Box<dyn rare_sui::signer::TxSigner>)
    }
    pub fn save_key() -> Option<crypto::SaveKey> {
        inner().keys.as_ref().map(|k| k.key.clone())
    }
    /// The Ludusavi index, fetched now if needed (at most 3 minutes).
    pub fn index_blocking() -> bool {
        ensure_index();
        for _ in 0..360 {
            if inner().index.is_some() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        false
    }
    /// Another device: forget this one's sync points and cached list (HOME already switched).
    pub fn switch_device(secret: &[u8], account: &str) -> Result<String, String> {
        with(|i| {
            i.sync = None;
            i.cloud.clear();
            i.games.clear();
            i.retry.clear();
            i.errors.clear();
        });
        install(secret, account)
    }
    /// Runs the failed uploads now, as the app does when their time comes or the network is back.
    pub fn run_retries() -> Vec<String> {
        let due: Vec<(Game, bool)> = inner().retry.values().map(|r| (r.0.clone(), r.1)).collect();
        for (g, emu) in due {
            job_after(g, emu);
        }
        take()
    }
    pub fn renew() -> Vec<(String, u32)> {
        renew_now()
    }
    pub fn targets(g: &Game, emu: bool) -> Vec<Target> {
        let (cloud, index) = {
            let i = inner();
            (i.cloud.clone(), i.index.clone())
        };
        targets_for(g, emu, index.as_deref(), &cloud)
    }
}

// ---------------------------------------------------------------- fixtures (RR_FIXTURE shots)

fn fixture_view(id: &str) -> GameView {
    let now = now_ms();
    let day = 86_400_000;
    GameView {
        known: true,
        sets: 1,
        modified: Some(now - 3_600_000),
        bytes: 2_200_000,
        synced: true,
        synced_at: Some(now - 3_600_000),
        conflict: false,
        pending: false,
        versions: vec![
            VersionRow { set: format!("game:{id}"), set_label: String::new(), created: now - 3_600_000, bytes: 2_200_000, reason: "played".into(), local: Some(1), cloud: Some("0x1".into()), current: true },
            VersionRow { set: format!("game:{id}"), set_label: String::new(), created: now - day, bytes: 2_100_000, reason: "played".into(), local: Some(2), cloud: Some("0x2".into()), current: false },
            VersionRow { set: format!("game:{id}"), set_label: String::new(), created: now - 3 * day, bytes: 1_900_000, reason: "cloud".into(), local: None, cloud: Some("0x3".into()), current: false },
        ],
    }
}

fn fixture_conflict() -> Conflict {
    let now = now_ms();
    let set = GameSet { id: "game:x".into(), title: "Hollow Knight".into(), roots: vec![] };
    Conflict {
        game: "local:hollow".into(),
        title: "Hollow Knight".into(),
        launch: true,
        sets: vec![ConflictSet {
            target: Target::Game(set),
            local: Scan { files: 3, bytes: 2_200_000, modified: Some(now - 1_800_000), fp: "a".into() },
            cloud: Backup { blob_object_id: "0x1".into(), blob_id: "b".into(), size: 1, end_epoch: None, meta: BackupMeta { v: 1, id: "game:x".into(), bytes: 1_900_000, created: now - 27 * 3_600_000, modified: Some(now - 27 * 3_600_000), ..Default::default() } },
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_is_stable_per_account() {
        let a = Keys::from_secret(&[7; 32], "0xabc").unwrap();
        let b = Keys::from_secret(&[7; 32], "0xABC").unwrap();
        let c = Keys::from_secret(&[8; 32], "0xabc").unwrap();
        assert_eq!(a.writer_addr, b.writer_addr);
        assert_ne!(a.writer_addr, c.writer_addr);
        assert!(a.writer_addr.starts_with("0x") && a.writer_addr.len() == 66);
        assert_eq!(a.key.id, b.key.id);
    }

    #[test]
    fn stores_that_sync_themselves_are_left_alone() {
        let g = |source: &str, id: &str| Game { id: id.into(), source: source.into(), title: "X".into(), installed: true, install_dir: None, size_bytes: None, last_played: None, playtime_minutes: None, art: Default::default(), launch: None, origin: String::new(), foreign: false };
        assert!(!eligible(&g("steam", "steam:1")));
        assert!(!eligible(&g("gog", "gog:1")));
        assert!(eligible(&g("local", "local:x")));
        assert!(eligible(&g("web", "web:x")));
        assert!(eligible(&g("apps", "apps:esde")));
        assert_eq!(uploads_left((FUND_FROST, FUND_MIST)), 33);
        assert_eq!((backoff(1), backoff(2), backoff(3), backoff(20)), (60.0, 120.0, 240.0, 3600.0));
    }
}
