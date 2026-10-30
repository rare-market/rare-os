//! Rare Wallet behind the native screens: one keystore (the same `accounts.json` and vault the web
//! launcher uses), one signing queue with the review sheet in front of it (`sign::host`), the
//! devices (Ledger, Keystone, passkey) and the wallet's side of the other crates' traits:
//!
//! - [`tx_signer`]: `rare_sui::signer::TxSigner` (cloud saves, anything paid from the wallet)
//! - [`executor`]: `rare_market::inventory::trade::Executor` (sell, list, cancel; Rare covers gas
//!   when the wallet is short and the sponsor offers it)
//! - [`friends_signer`] / [`friends_recipients`]: rare-friends' `WalletSigner` and `RecipientResolver`
//! - [`trade_sign`] / [`identity_maker`]: Trade's sign requests and the swap API's device identity
//!
//! Every entry point queues a request and blocks until the person answers the review sheet: call
//! them from a worker thread (`work`), never the UI thread. Cancel answers "You declined the
//! request" (`rare_wallet::sign::DECLINED`).

use rare_wallet::devices::{DeviceStage, Devices};
use rare_wallet::keystone::KeystoneState;
use rare_wallet::keystore::{AccountKind, DeviceVault, FileStore, Keystore, RareAccount};
use rare_wallet::ledger::LedgerState;
use rare_wallet::passkey::PasskeyState;
use rare_wallet::passkey_account::Via;
use rare_wallet::sign::{OriginKind, SignError, SignOptions, SignOrigin, Signing, SuiChain, TxInput};
use rare_wallet::sponsor::{Category, Sponsor, SponsorApi};
use rare_wallet::vault::{default_dir, Kdf, Vault};
use rare_wallet::WalletEvent;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex, OnceLock};

/// rare.market's sponsor endpoints over the launcher's net (no cache: `Net::call`).
pub struct MarketSponsor;

impl SponsorApi for MarketSponsor {
    fn get(&self, path: &str) -> Result<Value, String> {
        use rare_market::net::{Call, Net};
        let net = super::live::market_net();
        let r = net.call(&Call::get(format!("{}{path}", net.market_origin()))).map_err(|e| e.to_string())?;
        if !r.ok() {
            return Err(format!("HTTP {}", r.status));
        }
        Ok(r.json())
    }
    fn post(&self, path: &str, body: &Value) -> Result<(u16, Value), String> {
        use rare_market::net::{Call, Net};
        let net = super::live::market_net();
        let call = Call::post(format!("{}{path}", net.market_origin()), body.to_string()).header("content-type", "application/json");
        let r = net.call(&call).map_err(|e| e.to_string())?;
        Ok((r.status, r.json()))
    }
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
    }
}

pub type Ks = Keystore<FileStore>;
pub type Sign = Signing<FileStore, SuiChain, MarketSponsor>;

pub struct Wallet {
    pub keystore: Arc<Ks>,
    pub signing: Arc<Sign>,
    pub sui: rare_sui::Sui,
    pub rt: tokio::runtime::Handle,
    pub ledger: LedgerState,
    pub keystone: KeystoneState,
    pub passkey: PasskeyState,
    /// What a device shows while it signs (the review sheet's footer).
    pub stage: Mutex<Option<DeviceStage>>,
    /// The last passkey ceremony event (QR, status, PIN, error), for the sheets that run one.
    pub passkey_event: Mutex<Option<rare_wallet::passkey::Event>>,
    /// Why a passkey ceremony asks again (a second touch to tell two possible accounts apart).
    pub passkey_note: Mutex<Option<String>>,
    /// The 6-digit code BlueZ shows while pairing a Ledger.
    pub ledger_pairing: Mutex<Option<String>>,
    /// The review sheet's Cancel while a device works.
    pub cancelled: Arc<AtomicBool>,
    /// Phone (QR) or a USB security key for passkey ceremonies.
    pub via: Mutex<Via>,
    /// SuiNS lookups for the recipient field, cached five minutes.
    pub names: rare_sui::names::NameCache,
    /// The optional PIN for big sends (Settings > Accounts).
    pub send_pin: rare_wallet::send_pin::SendPin,
    ver: AtomicU64,
}

static WALLET: OnceLock<Option<Wallet>> = OnceLock::new();

/// The wallet; None only when the gRPC client could not be made (a bad RPC URL).
pub fn wallet() -> Option<&'static Wallet> {
    WALLET
        .get_or_init(|| {
            let sui = super::live::sui()?.clone();
            let rt = super::live::rt().handle().clone();
            let dir = default_dir();
            let vault = Arc::new(Vault::new(dir.clone(), true, Kdf::DEFAULT));
            // phrase accounts made here live in the device vault: they open at login, no PIN
            // PIN accounts: this SuiPlay's device key behind the PIN guard (root service, else sealed here)
            let keystore = Arc::new(Keystore::new(FileStore::new(dir.clone()), vault).with_device(DeviceVault::at(&dir, true)).with_pin(rare_wallet::pin_device::PinDevice::at(&dir)));
            let chain = SuiChain { sui: sui.clone(), rt: rt.clone() };
            let signing = Arc::new(Signing::new(keystore.clone(), chain, Arc::new(Sponsor::new(MarketSponsor))));
            Some(Wallet {
                keystore,
                signing,
                sui,
                rt,
                ledger: LedgerState::default(),
                keystone: KeystoneState::default(),
                passkey: PasskeyState::default(),
                stage: Mutex::new(None),
                passkey_event: Mutex::new(None),
                passkey_note: Mutex::new(None),
                ledger_pairing: Mutex::new(None),
                cancelled: Arc::new(AtomicBool::new(false)),
                via: Mutex::new(Via::Phone),
                names: rare_sui::names::NameCache::default(),
                send_pin: rare_wallet::send_pin::SendPin::new(default_dir()),
                ver: AtomicU64::new(0),
            })
        })
        .as_ref()
}

impl Wallet {
    /// Moves on every change a screen shows (accounts, lock state, device stages, passkey events).
    pub fn version(&self) -> u64 {
        self.ver.load(Ordering::SeqCst) + self.signing.version()
    }

    pub fn bump(&self) {
        self.ver.fetch_add(1, Ordering::SeqCst);
    }

    /// Where the wallet's events go: the passkey ceremony and Ledger pairing land in the state the
    /// sheets read.
    pub fn sink(&'static self) -> rare_wallet::Sink {
        Arc::new(move |e: WalletEvent| {
            match e {
                WalletEvent::LedgerPairing { passkey } => *self.ledger_pairing.lock().unwrap() = Some(passkey),
                WalletEvent::Passkey(p) => *self.passkey_event.lock().unwrap() = Some(p),
            }
            self.bump();
        })
    }

    pub fn accounts(&self) -> (Vec<RareAccount>, Option<String>) {
        match self.keystore.meta() {
            Ok(m) => (m.accounts, m.selected),
            Err(e) => {
                log::warn!("wallet accounts: {e}");
                (Vec::new(), None)
            }
        }
    }

    /// The devices for one signature (blocking; the wallet worker thread).
    pub fn devices(&'static self) -> Devices<'static> {
        self.cancelled.store(false, Ordering::SeqCst);
        *self.stage.lock().unwrap() = None;
        *self.passkey_event.lock().unwrap() = None;
        *self.passkey_note.lock().unwrap() = None;
        Devices {
            ledger: &self.ledger,
            keystone: &self.keystone,
            passkey: &self.passkey,
            rt: self.rt.clone(),
            sink: self.sink(),
            via: *self.via.lock().unwrap(),
            on_stage: Box::new(move |s| {
                *self.stage.lock().unwrap() = Some(s);
                self.bump();
            }),
            cancelled: self.cancelled.clone(),
            objects: None,
            current: None,
        }
    }

    /// The recipient field's name service (SuiNS both ways).
    pub fn names(&'static self) -> rare_wallet::recipient::ChainNames<'static> {
        rare_wallet::recipient::ChainNames { cache: &self.names, chain: &self.sui }
    }

    /// Stops a device or passkey ceremony in progress (the sheet's Cancel).
    pub fn cancel_devices(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.passkey.cancel();
    }
}

/// Why the passkey asks again, while it does (None otherwise, and before the wallet started).
pub fn passkey_note() -> Option<String> {
    WALLET.get().and_then(|w| w.as_ref()).and_then(|w| w.passkey_note.lock().unwrap().clone())
}

/// The wallet's version without starting it (0 until something used the wallet).
pub fn peek_version() -> u64 {
    WALLET.get().and_then(|w| w.as_ref()).map(|w| w.version()).unwrap_or(0)
}

/// Runs `f` on a wallet worker thread; the answer arrives on the channel (pages drain it in `tick`).
pub fn work<T: Send + 'static>(f: impl FnOnce(&'static Wallet) -> T + Send + 'static) -> Receiver<Result<T, String>> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("rare-wallet".into())
        .spawn(move || {
            let r = match wallet() {
                Some(w) => Ok(f(w)),
                None => Err("Rare Wallet can't reach the network right now".to_string()),
            };
            let _ = tx.send(r);
        })
        .ok();
    rx
}

/// Takes a finished answer off a channel, if any.
pub fn poll<T>(rx: &mut Option<Receiver<Result<T, String>>>) -> Option<Result<T, String>> {
    let r = rx.as_ref()?.try_recv();
    match r {
        Ok(v) => {
            *rx = None;
            Some(v)
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => None,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            *rx = None;
            Some(Err("The wallet stopped. Try again.".into()))
        }
    }
}

// ---------------------------------------------------------------- unlock

static UNLOCKS: Mutex<Vec<(String, std::sync::mpsc::Sender<bool>)>> = Mutex::new(Vec::new());

/// Asks for the PIN (or passphrase) on the unlock sheet with `reason` ("Enter your PIN to add an
/// account"); answers true once the vault is unlocked, false when the person cancels. Answers true
/// at once when nothing needs unlocking. Call from the UI thread; drain the channel in `tick`.
pub fn ask_unlock(reason: &str) -> Receiver<bool> {
    let (tx, rx) = channel();
    let needed = wallet().is_some_and(|w| w.keystore.has_vault().unwrap_or(false) && !w.keystore.is_unlocked().unwrap_or(false));
    if !needed {
        let _ = tx.send(true);
    } else {
        UNLOCKS.lock().unwrap().push((reason.to_string(), tx));
        if let Some(w) = wallet() {
            w.bump();
        }
    }
    rx
}

static PIN_TASKS: Mutex<Vec<crate::screens::wallet::sign::pin_tasks::Task>> = Mutex::new(Vec::new());

/// Change PIN or Show recovery words for this SuiPlay's PIN (Settings > Accounts); the review
/// host shows it when no request is on screen.
pub fn ask_pin_task(t: crate::screens::wallet::sign::pin_tasks::Task) {
    PIN_TASKS.lock().unwrap().push(t);
    if let Some(w) = wallet() {
        w.bump();
    }
}

pub(crate) fn take_pin_task() -> Option<crate::screens::wallet::sign::pin_tasks::Task> {
    let mut q = PIN_TASKS.lock().unwrap();
    (!q.is_empty()).then(|| q.remove(0))
}

/// Whether this SuiPlay has a PIN (a PIN account's device key).
pub fn has_device_pin() -> bool {
    wallet().is_some_and(|w| !w.keystore.pin_accounts().unwrap_or_default().is_empty())
}

/// The oldest waiting unlock request (the review host shows it).
pub(crate) fn take_unlock() -> Option<(String, std::sync::mpsc::Sender<bool>)> {
    let mut q = UNLOCKS.lock().unwrap();
    (!q.is_empty()).then(|| q.remove(0))
}

// ---------------------------------------------------------------- the other crates' traits

/// `rare_sui::signer::TxSigner` for the current account.
pub struct WalletTxSigner {
    pub account: String,
}

impl rare_sui::signer::TxSigner for WalletTxSigner {
    fn address(&self) -> String {
        self.account.clone()
    }
    fn sign_personal_message(&self, message: &[u8], title: &str) -> rare_sui::BoxFut<'_, String> {
        let (msg, title) = (message.to_vec(), title.to_string());
        Box::pin(async move {
            let w = wallet().ok_or_else(|| rare_sui::Error::invalid("Rare Wallet is not available"))?;
            let o = SignOptions { account: Some(self.account.clone()), title: Some(title), ..Default::default() };
            let s = w.signing.clone();
            let r = tokio::task::spawn_blocking(move || s.sign_personal_message(msg, o).and_then(|t| t.wait())).await.map_err(|e| rare_sui::Error::invalid(e.to_string()))?;
            r.map(|r| r.signature).map_err(|e| rare_sui::Error::invalid(e.to_string()))
        })
    }
    fn sign_and_execute(&self, tx: rare_sui::ptb::Tx, title: &str) -> rare_sui::BoxFut<'_, String> {
        let title = title.to_string();
        Box::pin(async move {
            let w = wallet().ok_or_else(|| rare_sui::Error::invalid("Rare Wallet is not available"))?;
            let o = SignOptions { account: Some(self.account.clone()), title: Some(title), ..Default::default() };
            let s = w.signing.clone();
            let r = tokio::task::spawn_blocking(move || s.sign_and_execute_with_cover(tx, Category::Walrus, o).and_then(|t| t.wait())).await.map_err(|e| rare_sui::Error::invalid(e.to_string()))?;
            if r.is_ok() {
                crate::data::keep::nudge();
            }
            r.map(|r| r.digest.unwrap_or_default()).map_err(|e| rare_sui::Error::invalid(e.to_string()))
        })
    }
}

/// The TxSigner for the selected account (None without a signing account).
pub fn tx_signer() -> Option<WalletTxSigner> {
    let w = wallet()?;
    let a = w.keystore.current_signing_account().ok()??;
    Some(WalletTxSigner { account: a.address })
}

/// `rare_market::inventory::trade::Executor`: the Marketplace's sell, list and cancel.
pub struct WalletExecutor;

impl rare_market::inventory::trade::Executor for WalletExecutor {
    fn sign_and_execute(&self, tx: rare_sui::ptb::Tx, account: &str, origin: &rare_market::inventory::trade::Origin, title: &str) -> rare_market::Result<rare_market::inventory::trade::ExecStatus> {
        use rare_market::inventory::trade::ExecStatus;
        let w = wallet().ok_or_else(|| rare_market::Error::new("Rare Wallet is not available"))?;
        let kind = if origin.kind == "market" { OriginKind::Market } else { OriginKind::Wallet };
        let o = SignOptions { account: Some(account.to_string()), origin: Some(SignOrigin::new(origin.name, kind)), title: Some(title.to_string()), ..Default::default() };
        match w.signing.sign_and_execute_with_cover(tx, Category::Trade, o).and_then(|t| t.wait()) {
            Ok(r) if r.status.as_deref() == Some("failure") => Ok(ExecStatus::Failure),
            Ok(_) => {
                crate::data::keep::nudge();
                Ok(ExecStatus::Success)
            }
            Err(SignError::Rejected(_)) => Err(rare_market::Error::new(rare_market::inventory::notice::SIGN_REJECTED)),
            Err(SignError::Failed(_)) => Ok(ExecStatus::Failure),
            Err(e) => Err(rare_market::Error::new(e.to_string())),
        }
    }
    fn sponsored(&self, sender: &str) -> bool {
        let Some(w) = wallet() else { return false };
        let sui = || rare_wallet::sign::Chain::sui_balance(&w.signing.chain, sender).ok();
        w.signing.sponsor.offer(sender, &Category::Trade, sui, None).is_some()
    }
}

pub fn executor() -> Arc<dyn rare_market::inventory::trade::Executor> {
    Arc::new(WalletExecutor)
}

/// rare-friends' signer (link friends, claim a username), through the review sheet.
pub fn friends_signer() -> Option<rare_wallet::adapters::FriendsWallet<FileStore, SuiChain, MarketSponsor>> {
    Some(rare_wallet::adapters::FriendsWallet(wallet()?.signing.clone()))
}

/// rare-friends' recipient field (names over the wallet's gRPC client).
pub fn friends_recipients() -> Option<rare_wallet::adapters::FriendsRecipients<rare_wallet::recipient::ChainNames<'static>>> {
    let w = wallet()?;
    Some(rare_wallet::adapters::FriendsRecipients { names: rare_wallet::recipient::ChainNames { cache: &w.names, chain: &w.sui }, rt: w.rt.clone() })
}

/// The swap API's throwaway identity (a fresh Ed25519 key each time).
pub fn identity_maker() -> rare_trade::swap::IdentityMaker {
    rare_wallet::adapters::identity_maker()
}

/// A Trade sign request through the review sheet (blocks until answered). Rare covers the fee
/// when the wallet is short of SUI and the sponsor offers trades.
pub fn trade_sign(req: &rare_trade::wallet::SignRequest, reader: Option<&dyn rare_trade::chain::ChainReader>) -> rare_trade::wallet::SignOutcome {
    let Some(w) = wallet() else { return rare_trade::wallet::SignOutcome::Failed("Rare Wallet is not available".into()) };
    let out = rare_wallet::adapters::trade_sign(&w.signing, req, None, reader);
    if matches!(out, rare_trade::wallet::SignOutcome::Sent { dry_run: false, .. }) {
        crate::data::keep::nudge();
    }
    out
}

/// The wallet's own sends (Send, Send collectibles): review, sign, submit. Blocks.
pub fn send(tx: TxInput, account: &str, title: &str) -> Result<rare_wallet::sign::SignResult, SignError> {
    let w = wallet().ok_or_else(|| SignError::Chain("Rare Wallet is not available".into()))?;
    let o = SignOptions { account: Some(account.to_string()), origin: Some(SignOrigin::wallet()), title: Some(title.to_string()), ..Default::default() };
    let r = w.signing.sign_and_execute(tx, o)?.wait();
    if r.is_ok() {
        crate::data::keep::nudge();
    }
    r
}

// ---------------------------------------------------------------- installing the hooks

/// Friends' signer, made when first used (the wallet starts on demand).
struct LazyFriendsSigner;

impl rare_friends::WalletSigner for LazyFriendsSigner {
    fn sign_personal_message(&self, message: &[u8], prompt: &rare_friends::api::SignPrompt) -> rare_friends::api::Result<String> {
        // rare.market sign-in (friends, presence, messages, profile) is a message, not a
        // transaction: an account whose key is here and open signs it with no sheet.
        if let Some(sig) = wallet().and_then(|w| sign_silently(w, &prompt.account, message)) {
            return Ok(sig);
        }
        match friends_signer() {
            Some(s) => s.sign_personal_message(message, prompt),
            None => Err(rare_friends::Error::Other("Rare Wallet can't reach the network right now".into())),
        }
    }
}

/// Friends' recipient field (usernames aside): 0x addresses and SuiNS names.
struct LazyFriendsResolver;

impl rare_friends::resolve::RecipientResolver for LazyFriendsResolver {
    fn resolve_recipient(&self, text: &str) -> rare_friends::resolve::Recipient {
        match friends_recipients() {
            Some(r) => r.resolve_recipient(text),
            None => rare_friends::resolve::Recipient::Error { msg: "Name lookup is not available right now".into() },
        }
    }
}

/// A personal-message signature with no sheet, for an account whose key is on this device and
/// open: a phrase or imported key in the device vault, or in the PIN vault while it is unlocked.
/// None for every other account (passkey, Ledger, Keystone, locked): they go through the sheet.
fn sign_silently(w: &Wallet, address: &str, message: &[u8]) -> Option<String> {
    let acc = w.keystore.meta().ok()?.accounts.into_iter().find(|a| a.address.eq_ignore_ascii_case(address))?;
    if acc.kind != AccountKind::Native || w.keystore.needs_unlock(&acc.address).unwrap_or(true) {
        return None;
    }
    let signer = w.keystore.get_signer(&acc.address).ok()?;
    signer.sign_personal_message(message).ok().map(|(_, s)| s)
}

/// How each kind of account approves a signature, in plain words (Friends' gate, the review sheet).
pub const METHOD_SILENT: &str = "This account is on this SuiPlay, so it signs in by itself. Nothing to approve.";
pub const METHOD_PIN: &str = "You approve with your Rare Wallet PIN.";
pub const METHOD_PASSKEY_BOTH: &str = "You approve with your PIN on this SuiPlay, or with the passkey on your phone: Rare shows a QR code to scan.";
pub const METHOD_PASSKEY_PHONE: &str = "You approve with your passkey: Rare shows a QR code, you scan it with your phone and confirm with your face or fingerprint. Keep Bluetooth on.";
pub const METHOD_PIN_ACCOUNT: &str = "You approve with your PIN on this SuiPlay. Your backup (phone or security key) can always sign too.";
pub const METHOD_LEDGER: &str = "You approve on your Ledger. Connect it and open the Sui app.";
pub const METHOD_KEYSTONE: &str = "You approve on your Keystone over a USB-C cable.";

/// How `address` approves a signature now.
pub fn sign_method(address: &str) -> String {
    let Some(w) = wallet() else { return METHOD_PIN.into() };
    let Some(a) = w.keystore.meta().ok().and_then(|m| m.accounts.into_iter().find(|a| a.address.eq_ignore_ascii_case(address))) else { return String::new() };
    let locked = w.keystore.needs_unlock(&a.address).unwrap_or(true);
    match a.kind {
        AccountKind::Native if !locked => METHOD_SILENT,
        AccountKind::Native => METHOD_PIN,
        AccountKind::Passkey if w.keystore.is_pin_account(&a.address) => METHOD_PIN_ACCOUNT,
        AccountKind::Passkey if a.passkey.as_ref().and_then(|p| p.local_key) != Some(false) && w.keystore.has_vault().unwrap_or(false) => METHOD_PASSKEY_BOTH,
        AccountKind::Passkey => METHOD_PASSKEY_PHONE,
        AccountKind::Ledger => METHOD_LEDGER,
        AccountKind::Keystone => METHOD_KEYSTONE,
        _ => "",
    }
    .into()
}

/// Whether `address` signs rare.market sign-ins by itself (see `sign_silently`): Friends turns on
/// for it with no prompt.
pub fn signs_silently(address: &str) -> bool {
    let Some(w) = wallet() else { return false };
    w.keystore.meta().ok().and_then(|m| m.accounts.into_iter().find(|a| a.address.eq_ignore_ascii_case(address))).is_some_and(|a| a.kind == AccountKind::Native && !w.keystore.needs_unlock(&a.address).unwrap_or(true))
}

/// Opens the device vault with this device's sealed key (about a second on the TPM), off the UI
/// thread, so phrase accounts are ready to sign before anything asks.
pub fn open_device_vault() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    let mut first = false;
    ONCE.call_once(|| first = true);
    if !first {
        return;
    }
    std::thread::Builder::new()
        .name("rare-wallet-open".into())
        .spawn(|| {
            let Some(w) = wallet() else { return };
            match w.keystore.open_device() {
                Ok(true) => {
                    log::info!("device vault open");
                    w.bump();
                }
                Ok(false) => {}
                Err(e) => log::warn!("device vault: {e}"),
            }
        })
        .ok();
}

/// Hands Rare Wallet to the screens that sign elsewhere (Friends). Called once at start; golden
/// images (RR_FIXTURE) keep their stand-ins.
pub fn install() {
    if std::env::var("RR_FIXTURE").is_ok() {
        return;
    }
    open_device_vault();
    crate::social::set_signer(Arc::new(LazyFriendsSigner));
    crate::social::set_resolver(Arc::new(LazyFriendsResolver));
}

// ---------------------------------------------------------------- rare.market's passkey registry

/// UTC ISO 8601 with milliseconds (JS `new Date().toISOString()`).
pub fn iso_now() -> String {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    iso_of(ms)
}

pub fn iso_of(ms: i64) -> String {
    let (days, rem) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    // civil from days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3_600_000, rem / 60_000 % 60, rem / 1000 % 60, rem % 1000)
}

/// The message rare.market's registry checks (rare.market api/src/passkeys.ts).
pub fn passkey_register_message(address: &str, passkey: &str, device_key: &str, issued: &str) -> String {
    format!("Register a Rare account passkey\n\nAddress: {address}\nPasskey: {passkey}\nDevice key: {device_key}\nIssued: {issued}")
}

/// The same for an account with more than one passkey: every member, in committee order.
pub fn passkey_register_message_members(address: &str, passkey: &str, device_key: &str, members: &[String], issued: &str) -> String {
    if members.len() <= 1 {
        return passkey_register_message(address, passkey, device_key, issued);
    }
    format!("Register a Rare account passkey\n\nAddress: {address}\nPasskey: {passkey}\nDevice key: {device_key}\nMembers: {}\nIssued: {issued}", members.join(","))
}

/// After a PIN account is made: register every passkey of it with rare.market, signed by the
/// device key the PIN just opened (then dropped), so the website signs in with one touch of any
/// backup and finds an account with a phone and a security key at all. Best effort.
pub fn register_pin_account(account: &RareAccount, device: rare_wallet::keys::Keypair) {
    let Some(info) = account.passkey.as_ref() else { return };
    let Ok(committee) = info.committee() else { return };
    let Ok(signer) = rare_wallet::multisig::MultisigDeviceSigner::new(committee, device) else { return };
    let members: Vec<String> = info.passkeys().into_iter().map(|m| m.public_key).collect();
    for pk in &members {
        let message = passkey_register_message_members(&account.address, pk, &info.device_key, &members, &iso_now());
        let Ok(signature) = signer.sign_personal_message(message.as_bytes()) else { return };
        let mut passkey = serde_json::json!({ "publicKey": pk, "deviceKey": info.device_key });
        if members.len() > 1 {
            passkey["members"] = serde_json::json!(members);
        }
        let body = serde_json::json!({ "address": account.address, "message": message, "signature": signature, "passkey": passkey });
        match MarketSponsor.post("/v1/accounts/passkey", &body) {
            Ok((s, _)) if (200..300).contains(&s) => log::info!("PIN account passkey registered with rare.market"),
            Ok((s, v)) => log::warn!("PIN account register: HTTP {s} {v}"),
            Err(e) => log::warn!("PIN account register: {e}"),
        }
    }
}

/// After a passkey account is made here: tell rare.market which passkey and device key make the
/// address (signed by the device member, no phone), so the first sign-in on the website takes one
/// touch. Best effort: a failure is logged and the website falls back to its second touch.
pub fn register_passkey(w: &Wallet, address: &str) {
    let Ok(Some(acc)) = w.keystore.meta().map(|m| m.accounts.into_iter().find(|a| a.address == address)) else { return };
    let Some(pk) = acc.passkey.as_ref() else { return };
    let Ok(signer) = w.keystore.get_signer(address) else { return };
    let message = passkey_register_message(address, &pk.public_key, &pk.device_key, &iso_now());
    let Ok((_, signature)) = signer.sign_personal_message(message.as_bytes()) else { return };
    let body = serde_json::json!({ "address": address, "message": message, "signature": signature, "passkey": { "publicKey": pk.public_key, "deviceKey": pk.device_key } });
    match MarketSponsor.post("/v1/accounts/passkey", &body) {
        Ok((s, _)) if (200..300).contains(&s) => log::info!("passkey registered with rare.market"),
        Ok((s, v)) => log::warn!("passkey register: HTTP {s} {v}"),
        Err(e) => log::warn!("passkey register: {e}"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn iso_dates_like_js() {
        assert_eq!(super::iso_of(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(super::iso_of(1_772_582_400_123), "2026-03-04T00:00:00.123Z");
        assert_eq!(super::iso_of(951_782_400_000 + 3_723_004), "2000-02-29T01:02:03.004Z");
    }
}
