//! Friends and notices, live: rare-friends (the list, presence, parties, usernames) and rare-notify
//! (the SSE stream on rare.market's /v1/notify, toasts, the centre) on one background thread, with
//! the screens reading what it keeps. The UI thread never waits on the network: actions run on
//! short threads and the result lands in the state, then the app is woken.
//!
//! Device-key signing for friends comes from rare-wallet: the app installs a signer with
//! [`set_signer`] (and a recipient resolver with [`set_resolver`]); until then linking says so.

use crate::screens::friends::model::{FriendProfile, FriendsData, ItemCard, LibGame};
use rare_friends::join::{Invites, LibraryGame};
use rare_friends::live::{Activity, BeatInput, Live, Outcome};
use rare_friends::nav::FriendsNav;
use rare_friends::resolve::{Recipient, RecipientResolver};
use rare_friends::{Account, Friends, FriendsApi, FriendsStatus, Remote, WalletSigner};
use rare_market::net::{Call, LineStream, Net, NetError, Reply};
use rare_notify::notifier::NotifyHost;
use rare_notify::stream::{NotifyStream, StreamEvent, StreamOptions};
use rare_notify::Notifier;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

static SIGNER: OnceLock<Arc<dyn WalletSigner>> = OnceLock::new();
/// The running notifier, so Quick settings and Settings read and write the one set of notice prefs.
static LIVE: OnceLock<Arc<Social>> = OnceLock::new();
/// Do not disturb on a run without the live notifier (fixtures, the Mac preview).
static DND_LOCAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The running Friends and notices service (None before the app starts it, and in fixtures).
pub fn live() -> Option<Arc<Social>> {
    LIVE.get().cloned()
}

/// The notice prefs the notifier routes with (None without the live notifier).
pub fn prefs() -> Option<rare_notify::logic::Prefs> {
    LIVE.get().map(|s| s.with_store(|st| st.prefs.clone()))
}

/// Saves the notice prefs (persisted by the notifier's store) and moves the notice rev.
pub fn set_prefs(p: rare_notify::logic::Prefs) {
    if let Some(s) = LIVE.get() {
        DND_LOCAL.store(p.dnd, Ordering::Relaxed);
        s.with_store(|st| st.set_prefs(p));
        s.inner.bump();
    }
}

/// Do not disturb: one setting for Quick settings' tile and Settings > Notifications.
pub fn dnd() -> bool {
    prefs().map(|p| p.dnd).unwrap_or_else(|| DND_LOCAL.load(Ordering::Relaxed))
}

pub fn set_dnd(on: bool) {
    DND_LOCAL.store(on, Ordering::Relaxed);
    if let Some(mut p) = prefs() {
        p.dnd = on;
        set_prefs(p);
    }
}
static RESOLVER: OnceLock<Arc<dyn RecipientResolver>> = OnceLock::new();
/// When each account last signed in to rare.market by itself (one try per 10 minutes, process wide:
/// a rebuilt app must not sign in twice).
static AUTO_TRIED: Mutex<Option<std::collections::HashMap<String, i64>>> = Mutex::new(None);

/// Saves the picture on rare.market with this device's friends key: None when friends is off here
/// (the pick then goes up with the first heartbeat), else when rare.market stamped it, or why not.
pub fn save_avatar(id: &str) -> Option<Result<Option<i64>, String>> {
    let s = LIVE.get()?;
    let f = s.friends();
    let k = f.current_key()?;
    Some(f.api().save_avatar(&k, id).map_err(|e| e.to_string()))
}

/// This device's linked friends key for the account in use (phone control signs with it), or None.
pub fn device_key() -> Option<rare_friends::sign::DeviceKey> {
    LIVE.get()?.friends().current_key()
}

/// Turns friends on for the account in use (one wallet signature, or none for a phrase account), so phone
/// control has a key. Blocks: call it off the UI thread. Err says why it did not happen.
pub fn link_device_key() -> Result<(), String> {
    let Some(s) = LIVE.get() else { return Err("Friends is off in this run.".into()) };
    if s.friends().current_key().is_some() || s.friends().turn_on() {
        return Ok(());
    }
    Err(s.friends().read(|st| st.error.clone()).unwrap_or_else(|| "The sign-in to rare.market was cancelled.".into()))
}

/// The notify stream reconnects (with this device's key it carries the phone's commands).
pub fn refresh_stream() {
    if let Some(s) = LIVE.get() {
        s.inner.stream.refresh();
    }
}

/// A POST to rare.market signed with this device's friends key (Runs on SuiPlay reports): None
/// when friends is off here, else the answer or why not.
pub fn signed_post<T: serde::de::DeserializeOwned>(path: &str, body: &serde_json::Value) -> Option<Result<T, String>> {
    let s = LIVE.get()?;
    let k = s.friends().current_key()?;
    Some(rare_friends::api::send(&SharedNet, Some(&k), rare_market::net::Method::Post, path, Some(body), rare_friends::sign::now_ms()).map_err(|e| e.to_string()))
}

/// Any rare.market call signed with this device's friends key: None when friends is off here.
pub fn signed_send<T: serde::de::DeserializeOwned>(method: rare_market::net::Method, path: &str, body: Option<&serde_json::Value>) -> Option<Result<T, String>> {
    let s = LIVE.get()?;
    let k = s.friends().current_key()?;
    Some(rare_friends::api::send(&SharedNet, Some(&k), method, path, body, rare_friends::sign::now_ms()).map_err(|e| e.to_string()))
}

/// An unsigned rare.market call (public routes).
pub fn market_send<T: serde::de::DeserializeOwned>(method: rare_market::net::Method, path: &str, body: Option<&serde_json::Value>) -> Result<T, String> {
    rare_friends::api::send(&SharedNet, None, method, path, body, rare_friends::sign::now_ms()).map_err(|e| e.to_string())
}

/// A short toast from any thread (shown on the next frame); false when social is off.
pub fn toast_any(text: &str, error: bool) -> bool {
    LIVE.get().map(|s| s.inner.toast(text, error)).is_some()
}

/// Installs Rare Wallet's personal-message signer (`rare_wallet::adapters::FriendsWallet`).
pub fn set_signer(s: Arc<dyn WalletSigner>) {
    let _ = SIGNER.set(s);
}

/// Installs Rare Wallet's recipient resolver (`rare_wallet::adapters::FriendsRecipients`).
pub fn set_resolver(r: Arc<dyn RecipientResolver>) {
    let _ = RESOLVER.set(r);
}

struct SlotSigner;

impl WalletSigner for SlotSigner {
    fn sign_personal_message(&self, message: &[u8], prompt: &rare_friends::api::SignPrompt) -> rare_friends::api::Result<String> {
        match SIGNER.get() {
            Some(s) => s.sign_personal_message(message, prompt),
            None => Err(rare_friends::Error::Other("Signing for friends is not set up in this build yet.".into())),
        }
    }
}

struct SlotResolver;

impl RecipientResolver for SlotResolver {
    fn resolve_recipient(&self, text: &str) -> Recipient {
        if let Some(r) = RESOLVER.get() {
            return r.resolve_recipient(text);
        }
        let t = text.trim().to_lowercase();
        if t.starts_with("0x") && t.len() > 3 && t.len() <= 66 && t[2..].bytes().all(|c| c.is_ascii_hexdigit()) {
            let address = format!("0x{:0>64}", &t[2..]);
            return Recipient::Ready { address, suins: None };
        }
        Recipient::Missing { msg: "Name lookup is not available yet. Use a username or a 0x address.".into() }
    }
}

/// The shared rare.market net (the launcher's instant cache), as an `Arc<dyn Net>`.
struct SharedNet;

impl Net for SharedNet {
    fn read(&self, key: &str) -> Result<Arc<str>, NetError> {
        crate::screens::wallet::live::market_net().read(key)
    }
    fn refresh(&self, key: &str) -> Result<Arc<str>, NetError> {
        crate::screens::wallet::live::market_net().refresh(key)
    }
    fn peek(&self, key: &str) -> Option<Arc<str>> {
        crate::screens::wallet::live::market_net().peek(key)
    }
    fn put(&self, key: &str, json: &str) -> Result<(), NetError> {
        crate::screens::wallet::live::market_net().put(key, json)
    }
    fn call(&self, call: &Call) -> Result<Reply, NetError> {
        crate::screens::wallet::live::market_net().call(call)
    }
    fn stream(&self, call: &Call) -> Result<LineStream, NetError> {
        crate::screens::wallet::live::market_net().stream(call)
    }
    fn market_origin(&self) -> String {
        crate::screens::wallet::live::market_net().market_origin()
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// What the screens and the background thread pass each other besides friends and notices.
#[derive(Default)]
struct Misc {
    running: Vec<String>,
    library: Vec<LibraryGame>,
    games: Vec<LibGame>,
    toasts: Vec<(String, bool)>,
    code: Option<rare_friends::FriendCode>,
    items: Option<Vec<ItemCard>>,
    /// The account `items` belong to (they stay on screen while the next read runs).
    items_for: Option<String>,
    nav: FriendsNav,
    chat_loaded: Option<String>,
    /// The friend on screen: their profile (None while loading) and whose messages were loaded.
    profile: Option<(String, Option<FriendProfile>)>,
    dm_loaded: Option<String>,
    /// What an action finished off the UI thread wants the app to do (open a Steam lobby, start a game).
    pending: Vec<After>,
    rev: u64,
}

struct Host {
    inner: std::sync::Weak<Inner>,
}

impl NotifyHost for Host {
    fn app_toast(&self, line: &str) {
        if let Some(i) = self.inner.upgrade() {
            i.toast(line, false);
        }
    }
    fn launcher_focused(&self) -> bool {
        rare_launch::apps::front::launcher_front() && !rare_webgame::visible()
    }
    // Over a game or app the launcher's toast is behind it: the HUD draws it (low battery).
    fn hud_toast(&self, title: &str, body: Option<&str>) {
        rare_system::hud::hud_toast(title.to_string(), body.map(str::to_string));
    }
}

struct Inner {
    friends: Arc<Friends>,
    notifier: Mutex<Notifier>,
    misc: Mutex<Misc>,
    stream: NotifyStream,
    waker: Arc<dyn Fn() + Send + Sync>,
    rev: AtomicU64,
}

impl Inner {
    fn bump(&self) {
        self.rev.fetch_add(1, Ordering::SeqCst);
        (self.waker)();
    }
    fn toast(&self, text: &str, error: bool) {
        self.misc.lock().unwrap().toasts.push((text.to_string(), error));
        self.bump();
    }
}

pub struct Social {
    inner: Arc<Inner>,
}

/// The accounts on this device: (all addresses, the selected one with whether it can sign).
fn accounts() -> (Vec<String>, Option<Account>) {
    let (list, selected) = crate::screens::wallet::page::load_accounts();
    let addrs: Vec<String> = list.iter().map(|a| a.address.clone()).collect();
    let cur = selected.as_ref().and_then(|s| list.iter().find(|a| &a.address == s)).or(list.first());
    (addrs, cur.map(|a| Account { address: a.address.clone(), can_sign: a.kind != "watch" }))
}

impl Social {
    /// Starts the background thread. `waker` asks the app for a frame.
    pub fn start(waker: Arc<dyn Fn() + Send + Sync>) -> Arc<Social> {
        let net: Arc<dyn Net> = Arc::new(SharedNet);
        let data = rare_core::paths::data_dir();
        let keys: Arc<dyn rare_friends::storage::Storage> = Arc::new(rare_friends::storage::FileStorage::new(data.join("friends")));
        let cache: Arc<dyn rare_friends::storage::Storage> = Arc::new(rare_friends::storage::FileStorage::new(data.join("friends-cache")));
        let api: Arc<dyn FriendsApi> = Arc::new(Remote::new(net.clone(), Arc::new(SlotSigner)));
        let friends = Arc::new(Friends::new(api, keys, cache));
        let nstore: Arc<dyn rare_notify::storage::Storage> = Arc::new(rare_notify::storage::FileStorage::new(data.join("notify")));
        let device = rare_notify::device::device_id(&*nstore);
        let (stream, rx) = NotifyStream::channel(net, nstore.clone(), device, StreamOptions::default());
        {
            let f = friends.clone();
            stream.set_social_token_provider(Arc::new(move || f.stream_token()));
        }
        let inner = Arc::new(Inner { friends, notifier: Mutex::new(Notifier::new(nstore)), misc: Mutex::new(Misc::default()), stream, waker, rev: AtomicU64::new(1) });
        let me = Arc::new(Social { inner: inner.clone() });
        let _ = LIVE.set(me.clone());
        std::thread::Builder::new().name("rare-social".into()).spawn(move || run(inner, rx)).ok();
        me
    }

    /// Whether this device has active price alerts (the stream connects for them).
    pub fn set_has_alerts(&self, on: bool) {
        self.inner.stream.set_has_alerts(on);
    }

    /// Moves when anything the screens show changed.
    pub fn rev(&self) -> u64 {
        self.inner.rev.load(Ordering::SeqCst) + self.inner.friends.rev()
    }

    /// A notice the device raised (low battery, an OS update ready): the centre and a toast.
    pub fn notify_local(&self, n: rare_notify::LocalNotice) {
        let host = Host { inner: Arc::downgrade(&self.inner) };
        self.inner.notifier.lock().unwrap().notify_local(n, &host);
        self.inner.bump();
    }

    /// The notice store (toasts, the centre's list), under the lock for the length of `f`.
    pub fn with_store<R>(&self, f: impl FnOnce(&mut rare_notify::NotifyStore) -> R) -> R {
        f(&mut self.inner.notifier.lock().unwrap().store)
    }

    /// Timers: toasts that are up long enough go. True when the toast list changed.
    pub fn tick(&self) -> bool {
        let host = Host { inner: Arc::downgrade(&self.inner) };
        let mut n = self.inner.notifier.lock().unwrap();
        let before = n.store.toasts.len();
        n.tick(&host);
        n.store.toasts.len() != before
    }

    /// The app toasts waiting (text, is an error).
    pub fn take_toasts(&self) -> Vec<(String, bool)> {
        std::mem::take(&mut self.inner.misc.lock().unwrap().toasts)
    }

    /// What is running and what is installed, for presence and Join.
    pub fn set_library(&self, running: &[String], games: &[crate::data::model::Game]) {
        let mut m = self.inner.misc.lock().unwrap();
        let lib: Vec<LibraryGame> = games
            .iter()
            .map(|g| LibraryGame {
                id: g.id.clone(),
                source: g.source.clone(),
                title: g.title.clone(),
                launch: g.launch.as_ref().map(|l| rare_friends::join::Launch { kind: l.kind.clone(), url: l.url.clone(), appid: l.appid }),
            })
            .collect();
        let view: Vec<LibGame> = games
            .iter()
            .map(|g| LibGame {
                id: g.id.clone(),
                title: g.title.clone(),
                source: g.source.clone(),
                installed: g.installed,
                last_played: g.last_played.map(|t| t as f64),
                cover: g.art.cover.clone(),
                header: g.art.header.clone(),
                hero: g.art.hero.clone(),
            })
            .collect();
        if m.running != running || m.library != lib {
            m.running = running.to_vec();
            m.library = lib;
            m.games = view;
            m.rev += 1;
            drop(m);
            self.inner.bump();
        }
    }

    /// The friends screens' data now.
    pub fn data(&self) -> FriendsData {
        let st = self.inner.friends.snapshot();
        let m = self.inner.misc.lock().unwrap();
        let (my_username, my_name) = match st.address.as_deref() {
            Some(a) => {
                // Friends' own answer first, then what rare.market said (before the overview loads).
                let u = st.person_of(a).and_then(|p| p.username.clone()).or_else(|| crate::data::account::cached_username(a));
                let name = if st.person_of(a).is_some() { st.label_of(a) } else { u.clone().unwrap_or_else(|| st.label_of(a)) };
                (u, name)
            }
            None => (None, String::new()),
        };
        let sign_method = match (st.status, st.address.as_deref()) {
            (FriendsStatus::Unlinked | FriendsStatus::Linking, Some(a)) => Some(crate::screens::wallet::service::sign_method(a)),
            _ => None,
        };
        FriendsData { st, now_ms: now_ms(), library: m.games.clone(), running: m.running.clone(), my_username, my_name, code: m.code.clone(), items: m.items.clone(), profile: m.profile.clone(), sign_method }
    }

    /// What finished actions want the app to do.
    pub fn take_pending(&self) -> Vec<After> {
        std::mem::take(&mut self.inner.misc.lock().unwrap().pending)
    }

    /// The friend on the Friends screen changed (None: no friend shown). Their messages load and
    /// are marked read, and their rare.market profile loads once per visit.
    pub fn view_friend(&self, addr: Option<&str>) {
        let addr = addr.map(str::to_string);
        {
            let mut m = self.inner.misc.lock().unwrap();
            if m.dm_loaded == addr {
                return;
            }
            m.dm_loaded = addr.clone();
            m.profile = addr.as_ref().map(|a| (a.clone(), None));
        }
        self.inner.bump();
        // The profile does not wait for the messages: its own threads, the header (banner, counts,
        // games, trophies) as soon as /v1/profile answers, the items in place when they arrive.
        if let Some(a) = addr.clone() {
            let inner = self.inner.clone();
            let a2 = a.clone();
            std::thread::Builder::new()
                .name("rare-friend-profile".into())
                .spawn(move || {
                    let items = std::thread::spawn({
                        let a = a2.clone();
                        move || load_items(&a)
                    });
                    let p = load_profile_head(&a2);
                    let set = |p: FriendProfile| {
                        let mut m = inner.misc.lock().unwrap();
                        if m.dm_loaded.as_deref() == Some(a2.as_str()) {
                            // items that already arrived stay
                            let have = m.profile.as_ref().and_then(|(_, x)| x.as_ref()).and_then(|x| x.items.clone());
                            m.profile = Some((a2.clone(), Some(FriendProfile { items: p.items.clone().or(have), ..p })));
                        }
                        drop(m);
                        inner.bump();
                    };
                    set(p.clone());
                    let it = items.join().unwrap_or_default();
                    let mut m = inner.misc.lock().unwrap();
                    if m.dm_loaded.as_deref() == Some(a2.as_str()) {
                        let mut cur = m.profile.as_ref().and_then(|(_, x)| x.clone()).unwrap_or(p);
                        cur.items = Some(it);
                        m.profile = Some((a2.clone(), Some(cur)));
                    }
                    drop(m);
                    inner.bump();
                })
                .ok();
        }
        self.spawn(move |f, s| {
            f.open_dm(addr.as_deref());
            let Some(a) = addr else { return Ok(None) };
            f.load_dms(&a);
            s.0.bump();
            Ok(None)
        });
    }

    /// What a friend sent is read (replying from the Rare menu).
    pub fn mark_read(&self, addr: &str) {
        let a = addr.to_string();
        let open = self.inner.friends.read(|s| s.open_dm.clone());
        self.spawn(move |f, _| {
            f.open_dm(Some(&a));
            f.open_dm(open.as_deref());
            Ok(None)
        });
    }

    /// Sends a shared capture (its link) to a friend.
    pub fn send_capture(&self, addr: String, kind: String, url: String) {
        self.spawn(move |f, _| {
            let label = f.person_of(&addr).map(|p| rare_friends::display::shown(Some(&p), "your friend").primary).unwrap_or_else(|| "your friend".into());
            let attach = rare_friends::types::Attach { kind, url };
            f.send_dm(&addr, "", Some(&attach)).map_err(|e| e.to_string())?;
            Ok(Some(format!("Sent to {label}")))
        });
    }

    /// A friend's name as the screens show it.
    pub fn label_of(&self, addr: &str) -> String {
        self.inner.friends.read(|s| s.label_of(addr))
    }

    /// A notice or party to open on, set by a notice's action.
    pub fn open_friends(&self, target: Option<rare_friends::nav::FriendsTarget>) {
        self.inner.misc.lock().unwrap().nav.open(target);
        self.inner.bump();
    }

    pub fn take_target(&self) -> Option<rare_friends::nav::FriendsTarget> {
        self.inner.misc.lock().unwrap().nav.take()
    }

    pub fn friends(&self) -> &Arc<Friends> {
        &self.inner.friends
    }

    /// Runs `f` off the UI thread, then wakes the app. Failures become error toasts.
    pub fn spawn(&self, f: impl FnOnce(&Arc<Friends>, &Self2) -> Result<Option<String>, String> + Send + 'static) {
        let inner = self.inner.clone();
        std::thread::Builder::new()
            .name("rare-friends-act".into())
            .spawn(move || {
                let s2 = Self2(inner.clone());
                match f(&inner.friends, &s2) {
                    Ok(Some(t)) => inner.toast(&t, false),
                    Ok(None) => inner.bump(),
                    Err(e) => inner.toast(&e, true),
                }
            })
            .ok();
    }

    /// Adds a friend by what the add box holds (username, name.sui or 0x address).
    pub fn add_by_name(&self, raw: String) {
        self.spawn(move |f, _| {
            let r = rare_friends::resolve::resolve_target(&**f.api(), &SlotResolver, &raw).map_err(|e| e.to_string())?;
            match r {
                None => Ok(None),
                Some(rare_friends::resolve::Resolved::Err { msg }) => Err(msg),
                Some(rare_friends::resolve::Resolved::Ok { address, label, .. }) => {
                    f.graph(rare_friends::GraphOp::Request, &address).map_err(|e| e.to_string())?;
                    Ok(Some(format!("Request sent to {label}")))
                }
            }
        });
    }

    pub fn new_code(&self) {
        self.spawn(|f, s| {
            let c = f.make_code().map_err(|e| e.to_string())?;
            s.0.misc.lock().unwrap().code = Some(c);
            Ok(None)
        });
    }

    pub fn enter_code(&self, code: String) {
        self.spawn(move |f, _| {
            let r = f.redeem_code(code.trim()).map_err(|e| e.to_string())?;
            f.refresh();
            let label = f.person_of(&r.address).map(|p| rare_friends::display::shown(Some(&p), "your friend").primary).unwrap_or_else(|| "your friend".into());
            Ok(Some(format!("You and {label} are friends")))
        });
    }

    pub fn claim_username(&self, raw: String) {
        self.spawn(move |f, _| {
            let addr = f.read(|s| s.address.clone());
            let m = f.claim_username(addr.as_deref(), &raw).map_err(|e| e.to_string())?;
            crate::screens::settings::live::set_username(Some(m.username.clone()));
            // The Add pane's "You have not picked one yet" goes at once, not after the next overview.
            if let Some(a) = addr.as_deref() {
                crate::data::account::remember_username(a, Some(m.username.clone()));
            }
            f.refresh();
            Ok(Some(rare_friends::pick_username::saved_toast(&m)))
        });
    }

    /// Loads the chat for the party on screen once per party.
    pub fn open_chat(&self, id: &str) {
        let mut m = self.inner.misc.lock().unwrap();
        if m.chat_loaded.as_deref() == Some(id) {
            return;
        }
        m.chat_loaded = Some(id.to_string());
        drop(m);
        let id = id.to_string();
        self.spawn(move |f, _| {
            f.open_chat(Some(&id));
            f.load_messages(&id);
            Ok(None)
        });
    }

    pub fn set_items(&self, items: Option<Vec<ItemCard>>) {
        self.inner.misc.lock().unwrap().items = items;
        self.inner.bump();
    }

    /// Signs one account out of rare.market on this device (Settings > Accounts).
    pub fn sign_out_account(&self, address: String) {
        self.spawn(move |f, _| {
            f.sign_out_account(&address);
            crate::screens::settings::live::touch();
            Ok(Some("Signed out of rare.market on this device".into()))
        });
    }

    /// Reads the signed-in account's collectibles for "Send an item" (rare.market). Items that
    /// are staked or have no name are left out; the wallet's own check runs again on its page.
    /// The last list stays on screen while it is read again; another account's list does not.
    pub fn load_items(&self) {
        let addr = self.inner.friends.read(|st| st.address.clone());
        if self.inner.misc.lock().unwrap().items_for != addr {
            self.set_items(None);
        }
        self.spawn(|f, s| {
            let Some(addr) = f.read(|st| st.address.clone()) else {
                s.0.misc.lock().unwrap().items = Some(Vec::new());
                return Ok(None);
            };
            s.0.misc.lock().unwrap().items_for = Some(addr.clone());
            let net = crate::screens::wallet::live::market_net();
            let page: rare_market::Result<rare_market::api::Page<rare_market::api::ItemCard>> = rare_market::api::get(net, &format!("/v1/accounts/{}/items?limit=100", rare_market::api::enc(&addr)));
            match page {
                Ok(p) => {
                    s.0.misc.lock().unwrap().items = Some(cards_of(p.data));
                    Ok(None)
                }
                Err(e) => {
                    let mut m = s.0.misc.lock().unwrap();
                    if m.items.is_none() {
                        m.items = Some(Vec::new());
                    }
                    Err(format!("Could not load your items. {e}"))
                }
            }
        });
    }

    pub fn status(&self) -> FriendsStatus {
        self.inner.friends.read(|s| s.status)
    }
}

/// What the app does after a Friends action.
#[derive(Clone, Debug, PartialEq)]
pub enum After {
    Nothing,
    /// Open the Wallet tab.
    Wallet,
    /// Open the Wallet tab on this page (Send, Send collectibles), recipient filled in.
    WalletView(crate::screens::wallet::home::stack::WalletView),
    /// Start this library game.
    Launch(String),
    Toast(String, bool),
    /// Ask for text on the keyboard; the answer comes back through [`Social::answer`] with `tag`.
    Ask { tag: String, title: &'static str, placeholder: &'static str },
    /// Open the Share screen to send a capture to this friend.
    Share(String),
    /// Open a Steam link (a friend's lobby).
    SteamUrl(String),
}

impl Social {
    /// One `friends:...` action from the Friends screen.
    pub fn act(&self, a: &str) -> After {
        use rare_friends::GraphOp;
        let parts: Vec<&str> = a.splitn(4, ':').collect();
        let Some(&"friends") = parts.first() else { return After::Nothing };
        let arg = |i: usize| parts.get(i).copied().unwrap_or("");
        let list = |s: &str| s.split(',').filter(|x| !x.is_empty()).map(str::to_string).collect::<Vec<_>>();
        let inv = Invites::default();
        match arg(1) {
            "open-wallet" => After::Wallet,
            "turn-on" => {
                self.spawn(|f, _| {
                    f.turn_on();
                    Ok(None)
                });
                After::Nothing
            }
            "refresh" => {
                self.spawn(|f, _| {
                    f.refresh();
                    Ok(None)
                });
                After::Nothing
            }
            "turn-off" => {
                self.spawn(|f, _| {
                    f.turn_off();
                    Ok(Some("Friends is off on this device".into()))
                });
                After::Nothing
            }
            "privacy" => {
                let on = arg(3) == "true";
                let mut p = rare_friends::PrivacyPatch::default();
                match arg(2) {
                    "showPlaying" => p.show_playing = Some(on),
                    "appearOffline" => p.appear_offline = Some(on),
                    _ => p.visibility = Some(if on { rare_friends::Visibility::Everyone } else { rare_friends::Visibility::Friends }),
                }
                self.spawn(move |f, _| {
                    f.save_privacy(&p).map_err(|e| e.to_string())?;
                    log::info!("friends privacy: sent {p:?}, saved {:?}", f.snapshot().me.map(|m| m.settings));
                    Ok(None)
                });
                After::Nothing
            }
            "graph" => {
                let (op, who) = (arg(2).to_string(), arg(3).to_string());
                let op = match op.as_str() {
                    "accept" => GraphOp::Accept,
                    "decline" => GraphOp::Decline,
                    "cancel" => GraphOp::Cancel,
                    "remove" => GraphOp::Remove,
                    "block" => GraphOp::Block,
                    "unblock" => GraphOp::Unblock,
                    _ => GraphOp::Request,
                };
                self.spawn(move |f, _| {
                    let label = f.person_of(&who).map(|p| rare_friends::display::shown(Some(&p), "them").primary).unwrap_or_else(|| "them".into());
                    f.graph(op, &who).map_err(|e| e.to_string())?;
                    Ok(match op {
                        GraphOp::Accept => Some(format!("You and {label} are friends")),
                        GraphOp::Remove => Some(format!("{label} was removed")),
                        GraphOp::Block => Some(format!("{label} is blocked")),
                        _ => None,
                    })
                });
                After::Nothing
            }
            "new-code" => {
                self.new_code();
                After::Nothing
            }
            "load-items" => {
                self.load_items();
                After::Nothing
            }
            "send-coins" | "send-item" => {
                use crate::screens::wallet::home::stack::WalletView;
                let addr = arg(2);
                let to = self.inner.friends.read(|s| s.person_of(addr).and_then(|p| p.suins.clone())).unwrap_or_else(|| addr.to_string());
                let item = parts.get(3).copied().unwrap_or("");
                After::WalletView(if arg(1) == "send-coins" {
                    WalletView::Send { coin_type: None, to: Some(to) }
                } else {
                    WalletView::SendNfts { ids: vec![item.to_string()], to: Some(to) }
                })
            }
            "start-party" => {
                let invite = list(arg(2));
                let (running, library) = {
                    let m = self.inner.misc.lock().unwrap();
                    (m.running.clone(), m.library.clone())
                };
                self.spawn(move |f, _| {
                    let game = rare_friends::join::running_ref(&running, &library, None, &inv);
                    f.create_party(game.as_ref(), &invite).map_err(|e| e.to_string())?;
                    Ok(None)
                });
                After::Nothing
            }
            "invite" => {
                let (id, to) = (arg(2).to_string(), list(parts.get(3).copied().unwrap_or("")));
                self.spawn(move |f, _| f.invite_to_party(&id, &to).map(|_| Some("Invites sent".into())).map_err(|e| e.to_string()));
                After::Nothing
            }
            "join-party" => {
                let id = arg(2).to_string();
                self.spawn(move |f, _| f.join_party(&id).map(|_| None).map_err(|e| e.to_string()));
                After::Nothing
            }
            "leave-party" => {
                let id = arg(2).to_string();
                self.spawn(move |f, _| f.leave_party(&id).map(|_| None).map_err(|e| e.to_string()));
                After::Nothing
            }
            "set-game" => {
                let (id, gid) = (arg(2).to_string(), parts.get(3).copied().unwrap_or("").to_string());
                let library = self.inner.misc.lock().unwrap().library.clone();
                self.spawn(move |f, _| {
                    let g = library.iter().find(|g| g.id == gid).map(|g| rare_friends::join::game_ref(g, Some(&id), &inv));
                    f.set_party_game(&id, g.as_ref()).map(|_| None).map_err(|e| e.to_string())
                });
                After::Nothing
            }
            "join-game" | "party-join-game" => {
                let target = if arg(1) == "join-game" {
                    self.inner.friends.read(|s| s.friends.iter().find(|f| f.address == arg(2)).and_then(|f| f.presence.game.clone()))
                } else {
                    self.inner.friends.read(|s| s.parties.iter().find(|p| p.id == arg(2)).and_then(|p| p.game.clone()))
                };
                let Some(r) = target else { return After::Toast("They are not playing anything right now".into(), true) };
                self.join_steam_lobby_or(r)
            }
            "dm-write" => After::Ask { tag: format!("dm:{}", arg(2)), title: "Write a message", placeholder: "Up to 280 characters" },
            "send-capture" => After::Share(arg(2).to_string()),
            "dm-older" => {
                let a = arg(2).to_string();
                self.spawn(move |f, _| {
                    f.load_older_dms(&a);
                    Ok(None)
                });
                After::Nothing
            }
            "send-to" => {
                let (kind, url) = parts.get(3).and_then(|r| r.split_once(':')).unwrap_or(("", ""));
                if url.is_empty() {
                    return After::Nothing;
                }
                self.send_capture(arg(2).to_string(), kind.to_string(), url.to_string());
                After::Nothing
            }
            "write" => After::Ask { tag: format!("write:{}", arg(2)), title: "Message to your party", placeholder: "Up to 280 characters" },
            "add-by-name" => After::Ask { tag: "add".into(), title: "Add a friend", placeholder: "Username, name.sui or 0x..." },
            "pick-username" => After::Ask { tag: "username".into(), title: rare_friends::pick_username::TITLE, placeholder: rare_friends::pick_username::PLACEHOLDER },
            "enter-code" => After::Ask { tag: "code".into(), title: "Your friend's code", placeholder: "6 letters and numbers" },
            _ => After::Nothing,
        }
    }

    /// A Steam game whose player shares their Steam id: their lobby when Steam shows one (their
    /// Steam profile's join link), else the same as [`Social::join_ref`]. The lookup runs off the
    /// UI thread and its answer arrives through [`Social::take_pending`].
    pub fn join_steam_lobby_or(&self, r: rare_friends::types::GameRef) -> After {
        let (Some(appid), Some(steam_id)) = (r.appid, r.steam_id.clone().filter(|_| r.kind == rare_friends::types::GameKind::Steam)) else { return self.join_ref(r) };
        let known = r.lobby.clone();
        let fallback = self.join_ref(r.clone());
        let inner = self.inner.clone();
        std::thread::Builder::new()
            .name("rare-steam-lobby".into())
            .spawn(move || {
                let lobby = known.map(|l| format!("steam://joinlobby/{appid}/{l}/{steam_id}")).or_else(|| steam_lobby(&steam_id, appid));
                let next = match lobby {
                    Some(url) => After::SteamUrl(url),
                    None => fallback,
                };
                inner.misc.lock().unwrap().pending.push(next);
                inner.bump();
            })
            .ok();
        After::Toast(format!("Joining {}", r.title), false)
    }

    /// Join a game from its reference (a friend's or a party's), through the library.
    pub fn join_ref(&self, r: rare_friends::types::GameRef) -> After {
        let inv = Invites::default();
        let library = self.inner.misc.lock().unwrap().library.clone();
        let plan = rare_friends::join::join_plan(&r, &library, &inv);
        if let Some(t) = plan.toast(&r) {
            if t.error || !matches!(plan, rare_friends::join::JoinPlan::Launch { .. }) {
                return After::Toast(t.text, t.error);
            }
        }
        match plan {
            rare_friends::join::JoinPlan::Launch { game, .. } => After::Launch(game.id),
            _ => After::Nothing,
        }
    }

    /// The keyboard finished for an [`After::Ask`].
    pub fn answer(&self, tag: &str, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        match tag.split_once(':') {
            Some(("write", id)) => {
                let id = id.to_string();
                self.spawn(move |f, _| f.send_message(&id, &text).map(|_| None).map_err(|e| e.to_string()));
            }
            Some(("dm", addr)) => {
                let addr = addr.to_string();
                self.spawn(move |f, _| f.send_dm(&addr, &text, None).map(|_| None).map_err(|e| e.to_string()));
            }
            _ => match tag {
                "add" => self.add_by_name(text),
                "username" => self.claim_username(text),
                "code" => self.enter_code(text),
                _ => {}
            },
        }
    }
}

/// The part of the service an action gets.
pub struct Self2(Arc<Inner>);

impl Self2 {
    pub fn toast(&self, text: &str, error: bool) {
        self.0.toast(text, error);
    }
}

/// A friend's rare.market profile and some of their items. Parts that fail stay empty.
/// rare.market's profile of `addr` (banner, counts, games, trophies), items left to [`load_items`].
fn load_profile_head(addr: &str) -> FriendProfile {
    let net = crate::screens::wallet::live::market_net();
    rare_market::api::get(net, &format!("/v1/profile/{}", rare_market::api::enc(addr))).unwrap_or_default()
}

/// The first page of their collectibles.
fn load_items(addr: &str) -> Vec<ItemCard> {
    let net = crate::screens::wallet::live::market_net();
    let items: rare_market::Result<rare_market::api::Page<rare_market::api::ItemCard>> = rare_market::api::get(net, &format!("/v1/accounts/{}/items?limit=24", rare_market::api::enc(addr)));
    items.map(|pg| cards_of(pg.data)).unwrap_or_default()
}

/// The lobby a Steam player is in, from their Steam profile's join link (Steam shows it when the
/// game exposes a joinable lobby and the profile is public). None otherwise.
fn steam_lobby(steam_id: &str, appid: u64) -> Option<String> {
    if !steam_id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(4))).build().into();
    let html = agent.get(&format!("https://steamcommunity.com/profiles/{steam_id}")).call().ok()?.body_mut().read_to_string().ok()?;
    lobby_link(&html, appid, steam_id)
}

/// The first `steam://joinlobby/<appid>/<lobby>/<steamid>` link in a profile page for this game.
pub fn lobby_link(html: &str, appid: u64, steam_id: &str) -> Option<String> {
    let mut rest = html;
    while let Some(i) = rest.find("steam://joinlobby/") {
        rest = &rest[i + "steam://joinlobby/".len()..];
        let parts: Vec<&str> = rest.splitn(4, '/').take(3).collect();
        if parts.len() < 3 {
            return None;
        }
        let digits = |s: &str| s.bytes().take_while(|b| b.is_ascii_digit()).count();
        let (a, l, s) = (parts[0], parts[1], &parts[2][..digits(parts[2])]);
        if digits(a) == a.len() && digits(l) == l.len() && !l.is_empty() && a == appid.to_string() && s == steam_id {
            return Some(format!("steam://joinlobby/{a}/{l}/{s}"));
        }
    }
    None
}

/// The Steam account signed in on this device, as a SteamID64.
fn steam_id64() -> Option<String> {
    rare_library::steam::find_roots().into_iter().find_map(|r| rare_trophies::steam::users(&r.path).into_iter().find(|u| u.most_recent).map(|u| u.steam_id64().to_string()))
}

/// Steam achievements unlocked while a Steam game runs become Trophy notices (a small toast over
/// the game). The first read of a game is the baseline; only newer unlocks notify.
#[derive(Default)]
struct TrophyWatch {
    game: Option<(u32, std::collections::HashSet<String>)>,
    next: i64,
}

impl TrophyWatch {
    const EVERY_MS: i64 = 20_000;

    fn tick(&mut self, running: &[String], library: &[LibraryGame], now: i64) -> Vec<rare_notify::LocalNotice> {
        let appid = running.iter().find_map(|id| id.strip_prefix("steam:").and_then(|a| a.parse::<u32>().ok()));
        let Some(appid) = appid else {
            self.game = None;
            return Vec::new();
        };
        if now < self.next && self.game.as_ref().is_some_and(|(a, _)| *a == appid) {
            return Vec::new();
        }
        self.next = now + Self::EVERY_MS;
        let Some(g) = rare_trophies::steam::scan(Some(appid)).into_iter().find(|g| g.appid == appid) else {
            if self.game.as_ref().map(|(a, _)| *a) != Some(appid) {
                self.game = Some((appid, Default::default()));
            }
            return Vec::new();
        };
        let unlocked: std::collections::HashSet<String> = g.achievements.iter().filter(|a| a.unlocked).map(|a| a.api_name.clone()).collect();
        let fresh = match &self.game {
            Some((a, seen)) if *a == appid => g.achievements.iter().filter(|x| x.unlocked && !seen.contains(&x.api_name)).cloned().collect(),
            _ => Vec::new(),
        };
        self.game = Some((appid, unlocked));
        let title = library.iter().find(|l| l.id == format!("steam:{appid}")).map(|l| l.title.clone()).unwrap_or_else(|| g.name.clone());
        fresh
            .into_iter()
            .take(3)
            .map(|a| {
                let mut n = rare_notify::LocalNotice::new(rare_notify::NoticeKind::Trophy, format!("Trophy unlocked: {}", a.name), rare_notify::types::NoticeAction::Trophies);
                if !title.is_empty() {
                    n.body = Some(title.clone());
                }
                n.image_url = a.icon_url.clone();
                n
            })
            .collect()
    }
}

/// The picker's tiles from rare.market's items.
pub fn cards_of(items: Vec<rare_market::api::ItemCard>) -> Vec<ItemCard> {
    items.into_iter().filter(|i| i.staked.is_none()).map(|i| ItemCard { id: i.id, name: i.name, image_url: i.image_url, collection: i.collection.map(|c| c.name) }).collect()
}

fn to_local(l: &rare_friends::live::LocalNotice) -> Option<rare_notify::LocalNotice> {
    let action = serde_json::to_value(&l.action).ok().and_then(|v| serde_json::from_value(v).ok())?;
    let mut n = rare_notify::LocalNotice::new(rare_notify::NoticeKind::parse(l.kind), l.title.clone(), action);
    n.body = l.body.clone();
    n.image_url = l.image_url.clone();
    Some(n)
}

/// The background loop: the stream's events into the notifier, friends events into friends, and
/// what `Live` says is due (account sync, refresh, heartbeat, stream reconnect).
fn run(inner: Arc<Inner>, rx: std::sync::mpsc::Receiver<StreamEvent>) {
    let host = Host { inner: Arc::downgrade(&inner) };
    let mut live = Live::new(now_ms());
    let mut addrs: Vec<String> = Vec::new();
    let mut started = false;
    let mut seen = (0u64, 0usize);
    let mut wallet_seen = 0u64;
    let mut trophies = TrophyWatch::default();
    // The last renewal check (the sign-in by itself keeps its tries in AUTO_TRIED, process wide).
    let mut renew_at = 0i64;
    // The Steam account, read again every 5 minutes (a sign-in in Steam's own UI).
    let mut steam: (Option<String>, i64) = (None, 0);
    loop {
        // Events: wait briefly so timers still run.
        let mut changed = false;
        let mut first = true;
        loop {
            let ev = if first { rx.recv_timeout(Duration::from_millis(250)) } else { rx.try_recv().map_err(|e| match e {
                std::sync::mpsc::TryRecvError::Empty => std::sync::mpsc::RecvTimeoutError::Timeout,
                std::sync::mpsc::TryRecvError::Disconnected => std::sync::mpsc::RecvTimeoutError::Disconnected,
            }) };
            first = false;
            let ev = match ev {
                Ok(e) => e,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                Err(_) => return,
            };
            changed = true;
            if let StreamEvent::Notice(n) = &ev {
                crate::features::localapi::forward_notice(n);
            }
            let social = inner.notifier.lock().unwrap().handle(ev, &host);
            if let Some(s) = social {
                // Phone control (features/companion.rs): commands for this device ride on the same stream.
                if s.event == "device" {
                    crate::features::companion::on_stream(&s.data);
                    continue;
                }
                let Some(e) = rare_friends::SocialEvent::parse(&s.event, &s.data.to_string()) else { continue };
                match live.on_social(&inner.friends, &e, now_ms()) {
                    Some(Outcome::Notice(l)) => {
                        if let Some(n) = to_local(&l) {
                            inner.notifier.lock().unwrap().notify_local(n, &host);
                        }
                    }
                    Some(Outcome::Toast(t)) => inner.misc.lock().unwrap().toasts.push((t, false)),
                    None => {}
                }
            }
        }
        // Timers.
        let now = now_ms();
        let (running, library) = {
            let m = inner.misc.lock().unwrap();
            (m.running.clone(), m.library.clone())
        };
        inner.notifier.lock().unwrap().set_game_running(!running.is_empty(), &host);
        let due = live.tick(&inner.friends, &running, Activity::Active, now);
        // the wallet changed (an account created, imported, signed in or picked): sync now
        let wv = crate::screens::wallet::service::peek_version();
        let wallet_moved = wv != wallet_seen;
        wallet_seen = wv;
        if due.sync_account || wallet_moved {
            let (list, cur) = accounts();
            inner.friends.sync_account(cur.as_ref());
            if list != addrs || !started {
                addrs = list;
                if started {
                    inner.stream.set_accounts(&addrs);
                } else {
                    inner.stream.start(&addrs);
                    started = true;
                }
            }
        }
        if due.refresh {
            inner.friends.refresh();
        }
        // An account whose key is here (a phrase account) signs in to rare.market by itself: the
        // login message is not a transaction, so friends turns on with no prompt. Not after the
        // player turned it off or signed out; tried again every 10 minutes after a failure.
        let unlinked = inner.friends.read(|s| s.address.clone().filter(|_| s.status == rare_friends::FriendsStatus::Unlinked));
        if let Some(addr) = unlinked {
            let due = {
                let mut g = AUTO_TRIED.lock().unwrap();
                let tried = g.get_or_insert_with(Default::default);
                let due = tried.get(&addr).map_or(true, |t| now - t > 600_000);
                if due {
                    tried.insert(addr.clone(), now);
                }
                due
            };
            if due && !inner.friends.turned_off(&addr) {
                let inner2 = inner.clone();
                std::thread::Builder::new()
                    .name("rare-friends-signin".into())
                    .spawn(move || {
                        if crate::screens::wallet::service::signs_silently(&addr) && inner2.friends.read(|s| s.address.as_deref() == Some(addr.as_str())) {
                            log::info!("friends: signing in to rare.market by itself");
                            inner2.friends.turn_on();
                            inner2.bump();
                        }
                    })
                    .ok();
            }
        }
        // The sign-in renews itself well before it runs out (checked hourly).
        if now - renew_at > 3_600_000 && inner.friends.current_key().is_some() {
            renew_at = now;
            let inner2 = inner.clone();
            std::thread::Builder::new()
                .name("rare-friends-renew".into())
                .spawn(move || {
                    if inner2.friends.renew_if_due() {
                        log::info!("friends: rare.market sign-in renewed");
                    }
                })
                .ok();
        }
        for n in trophies.tick(&running, &library, now) {
            inner.notifier.lock().unwrap().notify_local(n, &host);
            changed = true;
        }
        if due.beat {
            let invites = Invites::default();
            if now - steam.1 > 300_000 {
                steam = (steam_id64(), now);
            }
            // The picked picture goes up with the heartbeat, so friends see it (friends/live.ts avatarNow).
            let pick = crate::data::account::current().and_then(|(a, _)| crate::data::live::picked(&a));
            let input = BeatInput { running: &running, library: &library, invites: &invites, focused: true, avatar: pick.as_ref().map(|p| (p.id.as_str(), p.image_url.as_str())), avatar_at: pick.as_ref().map(|p| p.at).filter(|a| *a > 0), steam_id: steam.0.as_deref() };
            let _ = live.beat(&inner.friends, &input, now);
        }
        if due.refresh_stream {
            inner.stream.refresh();
        }
        // The party on screen has its chat loaded.
        if let Some(id) = inner.friends.read(|s| s.active_party().map(|p| p.id.clone())) {
            let load = {
                let mut m = inner.misc.lock().unwrap();
                let fresh = m.chat_loaded.as_deref() != Some(id.as_str());
                if fresh {
                    m.chat_loaded = Some(id.clone());
                }
                fresh
            };
            if load {
                inner.friends.open_chat(Some(&id));
                inner.friends.load_messages(&id);
            }
        }
        let sig = (inner.friends.rev(), inner.notifier.lock().unwrap().store.toasts.len());
        if changed || sig != seen {
            seen = sig;
            inner.bump();
        }
    }
}

#[cfg(test)]
mod send_tests {
    use super::*;

    /// The exact `dm` event rare.market's stream sent in the live end-to-end run, through friends,
    /// the notice and the notifier: over a running game it goes to the HUD; with Do not disturb it
    /// waits in the list.
    #[test]
    fn a_live_dm_pops_over_a_game_and_waits_with_dnd() {
        use rare_friends::types::{Friend, Person, Presence};
        use rare_notify::notifier::NotifyHost;
        use std::cell::RefCell;
        let me = "0xd846c5d0bad0bb568ea38022e6354f79d52d021298521af0f95ee3a31e855aef";
        let b = "0xcc731e59c042348eaaf8da7be3504692bb07aed7b49979ae30aeb8b8f300258d";
        let data = format!(r#"{{"id":"6","from":"{b}","to":"{me}","text":"live?","attach":null,"at":1793164097791,"read":false}}"#);
        struct H(RefCell<Vec<String>>, i64);
        impl NotifyHost for H {
            fn now_ms(&self) -> i64 {
                self.1
            }
            fn launcher_focused(&self) -> bool {
                false
            }
            fn hud_toast(&self, title: &str, body: Option<&str>) {
                self.0.borrow_mut().push(format!("{title} | {}", body.unwrap_or("")));
            }
        }
        let run = |dnd: bool| {
            let mut st = rare_friends::FriendsState {
                status: rare_friends::FriendsStatus::Ready,
                address: Some(me.into()),
                friends: vec![Friend { person: Person { address: b.into(), name: Some("bee".into()), ..Person::default() }, since: 1, presence: Presence::default() }],
                ..Default::default()
            };
            let ev = rare_friends::SocialEvent::parse("dm", &data).expect("the stream's dm event");
            let applied = st.apply(&ev).expect("a message from a friend");
            assert_eq!(st.dm_unread(b), 1);
            let Some(Outcome::Notice(l)) = Live::new(0).outcome(&applied, 1_793_164_097_800) else { panic!("a notice") };
            let h = H(RefCell::new(Vec::new()), 1_793_164_097_900);
            let mut nf = Notifier::new(Arc::new(rare_notify::storage::MemStorage::default()));
            let mut p = nf.store.prefs.clone();
            p.dnd = dnd;
            nf.store.set_prefs(p);
            nf.set_game_running(true, &h);
            nf.notify_local(to_local(&l).unwrap(), &h);
            let shown = h.0.borrow().clone();
            shown
        };
        assert_eq!(run(false), vec!["bee: live? | Message".to_string()]);
        assert!(run(true).is_empty());
    }

    #[test]
    fn the_lobby_link_is_this_games_and_this_players() {
        let sid = "76561198000000001";
        let page = format!(r#"<a href="steam://joinlobby/570/1/2">x</a> <a class="btn" href="steam://joinlobby/730/109775241000000001/{sid}">Join Game</a>"#);
        assert_eq!(lobby_link(&page, 730, sid).as_deref(), Some("steam://joinlobby/730/109775241000000001/76561198000000001"));
        assert_eq!(lobby_link(&page, 440, sid), None);
        assert_eq!(lobby_link("no lobby here", 730, sid), None);
    }
    use rare_market::api::{CollectionRef, ItemCard as Card, Staked};

    #[test]
    fn picker_cards_skip_staked_and_keep_collection_name() {
        let a = Card { id: "0x1".into(), name: "Hero".into(), collection: Some(CollectionRef { name: "Pawtato".into(), ..Default::default() }), ..Default::default() };
        let b = Card { id: "0x2".into(), staked: Some(Staked::default()), ..Default::default() };
        let out = cards_of(vec![a, b]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].collection.as_deref(), Some("Pawtato"));
    }
}
