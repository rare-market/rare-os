//! Accounts (rename, remove, add from the phrase) and the settings sheets (lock, auto-lock, change PIN, reveal phrase or key, connected sites, remove, reset).
//!
//! The pages are drawn by `page.rs` (pages/accounts.rs, pages/settings.rs); their effects land here.
//! Blocking keystore work runs on a wallet worker (`service::work`) and the answer comes back in
//! `manage_tick`. Secrets (the typed PIN, a revealed phrase or key) are never logged.

use crate::screens::wallet::home::lib::{secret_error, SecretFail};
use crate::screens::wallet::home::sheets::{self, ChangeMsg, ChangeOut, EntryMsg, EntryOut, LedgerCheck, RemoveAllMsg, RemoveMsg, RevealMsg, RevealWhat};
use crate::screens::wallet::home::stack::WalletView;
use crate::screens::wallet::markup::El as M;
use crate::screens::wallet::page::{self, WalletTab};
use crate::screens::wallet::pages::{accounts, settings};
use crate::screens::wallet::service::{self, poll};
use crate::screens::wallet::sign::host::ask_secret;
use crate::screens::wallet::types::SecretKind;
use crate::state::{State, Tone};
use rare_wallet::keystore::KeystoreError;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

const ASK_NAME: &str = "wallet:rename";
const ASK_PASS: &str = "wallet:manage:pass";
const ASK_REMOVE: &str = "wallet:manage:removeall";

type Rx<T> = Option<Receiver<Result<T, String>>>;

/// What a worker answers.
enum Done {
    Renamed(Result<(), String>),
    Added(Result<String, String>),
    Removed { addr: String, res: Result<(), String>, settings: bool },
    PinChanged(Result<(), SecretFail>),
    Secret(Result<String, SecretFail>),
    Wiped(Result<(), String>),
    Ledger(LedgerCheck),
}

/// What waits on the unlock sheet.
enum Then {
    AddPhrase(String),
    Remove { addr: String, settings: bool },
}

pub struct Flow {
    /// the focused account row at the last X or Y
    pub focused: Option<String>,
    /// the Accounts page's remove sheet
    rm: sheets::Remove,
    /// the label being renamed, by address
    renaming: Option<String>,
    pub vault: Option<settings::Vault>,
    pub origins: Vec<String>,
    /// the settings frame the vault and origins were read for
    settings_key: u64,
    prefs_loaded: bool,
    seen_ver: u64,
    last_press: Instant,
    unlock: Option<(Then, Receiver<bool>)>,
    rx: Rx<Done>,
}

impl Default for Flow {
    fn default() -> Self {
        Flow {
            focused: None,
            rm: sheets::Remove::default(),
            renaming: None,
            vault: None,
            origins: Vec::new(),
            settings_key: 0,
            prefs_loaded: false,
            seen_ver: 0,
            last_press: Instant::now(),
            unlock: None,
            rx: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    /// the Accounts page's remove sheet
    RemoveAcct(RemoveMsg),
}

// ---------------------------------------------------------------- helpers

/// Puts text on the system clipboard (wl-copy, xclip, xsel or pbcopy, whichever exists). Best effort.
pub fn copy_text(text: &str) {
    let text = text.to_string();
    std::thread::spawn(move || {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let tools: [(&str, &[&str]); 4] = [("wl-copy", &[]), ("xclip", &["-selection", "clipboard"]), ("xsel", &["--clipboard", "--input"]), ("pbcopy", &[])];
        for (cmd, args) in tools {
            let Ok(mut child) = Command::new(cmd).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else { continue };
            if let Some(mut si) = child.stdin.take() {
                let _ = si.write_all(text.as_bytes());
            }
            let _ = child.wait();
            return;
        }
    });
}

fn now() -> f64 {
    rare_render::clock::secs()
}

/// The vault's answer to a wrong or throttled secret, in the page's words.
pub fn fail_of(e: &KeystoreError) -> SecretFail {
    match e {
        KeystoreError::Vault(w) => {
            let ms = w.data.as_ref().and_then(|d| d.get("retryAfterMs")).and_then(|v| v.as_u64()).unwrap_or(0);
            match w.code.as_str() {
                "wrong" => SecretFail::Wrong { retry_after_ms: ms },
                "throttled" => SecretFail::Throttled { retry_after_ms: ms },
                "locked" => SecretFail::Locked,
                _ => SecretFail::Other(Some(w.message.clone())),
            }
        }
        KeystoreError::Locked => SecretFail::Locked,
        other => SecretFail::Other(Some(other.to_string())),
    }
}

fn vault_of(s: &rare_wallet::vault::VaultStatus) -> settings::Vault {
    settings::Vault { exists: s.exists, kind: if s.kind.as_deref() == Some("passphrase") { SecretKind::Passphrase } else { SecretKind::Pin }, backend: s.backend.clone() }
}

fn prefs_file() -> rare_wallet::prefs::PrefsFile {
    rare_wallet::prefs::PrefsFile::new(rare_wallet::vault::default_dir())
}

fn save_autolock(min: u32) {
    let f = prefs_file();
    let mut p = f.load();
    p.autolock_min = Some(min);
    if let Err(e) = f.save(&p) {
        log::warn!("wallet auto-lock: {e}");
    }
}

/// `approved_origins` raw value without `origin` (strings, `{origin}` objects, or an object keyed by origin).
fn without_origin(raw: &serde_json::Value, origin: &str) -> serde_json::Value {
    use serde_json::Value;
    let one = |x: &Value| match x {
        Value::String(s) => s == origin,
        Value::Object(o) => o.get("origin").and_then(Value::as_str) == Some(origin),
        _ => false,
    };
    match raw {
        Value::Array(a) => Value::Array(a.iter().filter(|x| !one(x)).cloned().collect()),
        Value::Object(o) => Value::Object(o.iter().filter(|(k, _)| k.as_str() != origin).map(|(k, v)| (k.clone(), v.clone())).collect()),
        other => other.clone(),
    }
}

fn remove_origin(origin: &str) -> Vec<String> {
    let f = prefs_file();
    let mut p = f.load();
    p.approved_origins = without_origin(&p.approved_origins, origin);
    if let Err(e) = f.save(&p) {
        log::warn!("wallet connected games: {e}");
    }
    rare_wallet::prefs::approved_origins(&p.approved_origins)
}

fn needs_vault(kind: &str) -> bool {
    kind == "native" || kind == "passkey"
}

impl WalletTab {
    fn mg(&mut self) -> &mut Flow {
        &mut self.flows.manage
    }

    fn manage_kind(&self) -> SecretKind {
        self.flows.manage.vault.as_ref().map(|v| v.kind).unwrap_or(SecretKind::Pin)
    }

    fn noun(&self) -> &'static str {
        if self.manage_kind() == SecretKind::Passphrase {
            "passphrase"
        } else {
            "PIN"
        }
    }

    /// Counts as the person being here (auto-lock restarts).
    pub(in crate::screens::wallet) fn manage_touch(&mut self) {
        self.flows.manage.last_press = Instant::now();
    }

    fn toast(&self, st: &mut State, text: impl Into<String>, tone: Tone) {
        st.toast(text, tone, now());
    }

    fn changed(&mut self) {
        if let Some(w) = service::wallet().filter(|_| !self.fixture) {
            w.bump();
        }
        self.ver += 1;
    }

    /// Wallet work on a worker; one at a time.
    fn job(&mut self, f: impl FnOnce(&'static service::Wallet) -> Done + Send + 'static) {
        if self.fixture || self.flows.manage.rx.is_some() {
            return;
        }
        self.flows.manage.rx = Some(service::work(f));
    }

    fn unlock_then(&mut self, then: Then, reason: &str) {
        if self.fixture || self.flows.manage.unlock.is_some() {
            return;
        }
        self.flows.manage.unlock = Some((then, service::ask_unlock(reason)));
    }

    // ------------------------------------------------------------ accounts page

    /// The Accounts page's remove sheet over the page, when open.
    pub(in crate::screens::wallet) fn manage_remove_sheet(&self) -> Option<M<page::Msg>> {
        let addr = self.accts.removing.as_ref()?;
        let a = self.accounts.iter().find(|a| &a.address == addr)?;
        Some(sheets::remove_view(a, &self.flows.manage.rm).map(&|m| page::Msg::Flow(super::Msg::Manage(Msg::RemoveAcct(m)))))
    }

    pub(in crate::screens::wallet) fn manage_accounts(&mut self, st: &mut State, m: accounts::Msg) {
        let cur = self.current().cloned();
        let focused = self.flows.manage.focused.take();
        let pr = accounts::Props { accounts: &self.accounts, current: cur.as_ref(), values: &self.acct_values, has_phrase: cur.as_ref().is_some_and(|c| c.hd.is_some()) };
        let eff = accounts::update(&pr, &mut self.accts, m, focused.as_deref());
        match eff {
            Some(accounts::Effect::Select(a)) => {
                if let Some(w) = service::wallet().filter(|_| !self.fixture) {
                    if let Err(e) = w.keystore.select(&a) {
                        log::warn!("wallet select: {e}");
                    }
                    w.bump();
                }
                self.selected = Some(a);
                crate::screens::wallet::home::page::account_changed(&mut self.home);
                self.stack.pop();
            }
            Some(accounts::Effect::Rename(a)) => {
                let label = self.accounts.iter().find(|x| x.address == a).map(|x| x.label.clone()).unwrap_or_default();
                self.flows.manage.renaming = Some(a);
                st.ask_text(ASK_NAME, "Account name", label, Some("Account name"));
            }
            Some(accounts::Effect::Copy(a)) => {
                copy_text(&a);
                self.toast(st, "Address copied", Tone::Info);
            }
            Some(accounts::Effect::AddFromPhrase) => match cur.and_then(|c| c.hd) {
                Some(hd) => {
                    if self.fixture {
                        self.accts.busy = false;
                    } else {
                        self.unlock_then(Then::AddPhrase(hd.phrase_id), "Enter your PIN to add an account");
                    }
                }
                None => self.accts.busy = false,
            },
            Some(accounts::Effect::Push(v)) => self.push(st, v),
            None => {
                if self.accts.removing.is_some() && !self.flows.manage.rm.busy {
                    self.flows.manage.rm = sheets::Remove::default();
                }
            }
        }
    }

    fn remove_account(&mut self, addr: String, settings: bool) {
        let kind = self.accounts.iter().find(|a| a.address == addr).map(|a| a.kind.clone()).unwrap_or_default();
        if needs_vault(&kind) {
            self.unlock_then(Then::Remove { addr, settings }, "Enter your PIN to remove this account");
        } else {
            self.start_remove(addr, settings);
        }
    }

    fn start_remove(&mut self, addr: String, settings: bool) {
        self.job(move |w| {
            let res = w.keystore.remove(&addr).map_err(|e| e.to_string());
            w.bump();
            Done::Removed { addr, res, settings }
        });
    }

    pub(in crate::screens::wallet) fn manage_handle(&mut self, st: &mut State, m: Msg) {
        let Msg::RemoveAcct(m) = m;
        match m {
            RemoveMsg::Cancel => {
                if !self.flows.manage.rm.busy {
                    self.accts.removing = None;
                    self.flows.manage.rm = sheets::Remove::default();
                }
            }
            RemoveMsg::Remove => {
                let Some(addr) = self.accts.removing.clone() else { return };
                if self.flows.manage.rm.busy {
                    return;
                }
                if self.fixture {
                    self.accts.removing = None;
                    self.toast(st, "Account removed", Tone::Info);
                    return;
                }
                self.flows.manage.rm = sheets::Remove { busy: true, error: None, sign_out: false };
                self.remove_account(addr, false);
            }
        }
    }

    // ------------------------------------------------------------ settings page

    pub(in crate::screens::wallet) fn manage_settings(&mut self, st: &mut State, m: settings::Msg) {
        let opening = matches!(m, settings::Msg::Open(_));
        let r = settings::update(&mut self.settings, m);
        // An account on this SuiPlay with no send PIN has no PIN to ask: its phrase or key shows
        // at once (the device key opens it). With the send PIN on, the sheet asks for that.
        if opening && !self.fixture {
            if let (Some(settings::Open::Reveal(what, _)), Some(a)) = (self.settings.open.as_ref(), self.current().cloned()) {
                let what = *what;
                if a.device && service::wallet().is_some_and(|w| !w.send_pin.is_set()) {
                    self.job(move |w| Done::Secret(reveal_device(w, what, &a.address).map_err(|e| fail_of(&e))));
                }
            }
        }
        match r {
            Some(settings::Effect::Lock) => {
                if let Some(w) = service::wallet().filter(|_| !self.fixture) {
                    w.keystore.lock();
                    w.bump();
                }
                self.toast(st, "Wallet locked", Tone::Info);
            }
            Some(settings::Effect::SetAutoLock(min)) => {
                if !self.fixture {
                    save_autolock(min);
                }
                self.manage_touch();
            }
            Some(settings::Effect::RemoveOrigin(o)) => {
                if !self.fixture {
                    self.flows.manage.origins = remove_origin(&o);
                } else {
                    self.flows.manage.origins.retain(|x| x != &o);
                }
            }
            Some(settings::Effect::Sheet(m)) => self.settings_sheet(st, m),
            None => {}
        }
    }

    fn open_entry(&mut self) -> Option<&mut sheets::Entry> {
        match self.settings.open.as_mut()? {
            settings::Open::Pin(s) => Some(&mut s.entry),
            settings::Open::Reveal(_, s) => Some(&mut s.entry),
            _ => None,
        }
    }

    fn settings_sheet(&mut self, st: &mut State, m: settings::Msg) {
        use settings::{Msg as S, Open};
        let kind = self.manage_kind();
        let account = self.current().cloned();
        match m {
            S::Pin(ChangeMsg::Entry(em)) => {
                let Some(Open::Pin(s)) = self.settings.open.as_mut() else { return };
                match sheets::change_update(kind, s, ChangeMsg::Entry(em)) {
                    Some(ChangeOut::AskPassphrase) => ask_secret(st, ASK_PASS, "Passphrase"),
                    Some(ChangeOut::Change { cur, next }) => {
                        let k = if kind == SecretKind::Passphrase { "passphrase" } else { "pin" };
                        if self.fixture {
                            return;
                        }
                        self.job(move |w| Done::PinChanged(w.keystore.change_secret(&cur, &next, k).map_err(|e| fail_of(&e))));
                    }
                    Some(ChangeOut::Close) | None => {}
                }
            }
            S::Reveal(RevealMsg::Entry(em)) => {
                let what = match self.settings.open.as_ref() {
                    Some(Open::Reveal(w, _)) => *w,
                    _ => return,
                };
                let Some(Open::Reveal(_, s)) = self.settings.open.as_mut() else { return };
                match sheets::entry_update(kind, &mut s.entry, em) {
                    Some(EntryOut::AskPassphrase) => ask_secret(st, ASK_PASS, "Passphrase"),
                    Some(EntryOut::Submit(secret)) => {
                        let Some(a) = account else { return };
                        if self.fixture {
                            if let Some(Open::Reveal(_, s)) = self.settings.open.as_mut() {
                                sheets::entry_done(&mut s.entry, None);
                            }
                            return;
                        }
                        self.job(move |w| {
                            if a.device {
                                if let Err(e) = w.send_pin.check(&secret) {
                                    return Done::Secret(Err(fail_of(&rare_wallet::keystore::KeystoreError::Vault(rare_wallet::WalletError::parse(&e)))));
                                }
                                return Done::Secret(reveal_device(w, what, &a.address).map_err(|e| fail_of(&e)));
                            }
                            let r = match what {
                                RevealWhat::Phrase => w.keystore.reveal_mnemonic(&secret, &a.address).map(|z| z.to_string()),
                                RevealWhat::Key => w.keystore.reveal_private_key(&secret, &a.address).map(|z| z.to_string()),
                            };
                            Done::Secret(r.map_err(|e| fail_of(&e)))
                        });
                    }
                    None => {}
                }
            }
            S::Reveal(RevealMsg::Copy) => {
                if let Some(Open::Reveal(_, s)) = self.settings.open.as_mut() {
                    if let Some(k) = &s.secret {
                        copy_text(k);
                        s.copied = true;
                    }
                }
            }
            S::Hw(sheets::HelpMsg::Check) => {
                if matches!(self.settings.open, Some(Open::Hw(Some(LedgerCheck::Busy), _))) {
                    return;
                }
                self.settings.open = Some(Open::Hw(Some(LedgerCheck::Busy), false));
                if self.fixture {
                    self.settings.open = Some(Open::Hw(Some(LedgerCheck::Error { message: "No Ledger found. Plug it in and unlock it.".into(), fix: None }), false));
                    return;
                }
                self.job(|w| {
                    let r = w.rt.block_on(rare_wallet::ledger::LedgerState::list(false, 0));
                    Done::Ledger(match r {
                        Ok(d) if !d.is_empty() => LedgerCheck::Ok { found: d[0].name.clone() },
                        Ok(_) => LedgerCheck::Error { message: "No Ledger found. Plug it in and unlock it.".into(), fix: None },
                        Err(e) => {
                            let we = rare_wallet::WalletError::parse(&e);
                            let fix = we.data.as_ref().and_then(|d| d.get("fix")).and_then(|f| f.as_str()).map(str::to_string);
                            LedgerCheck::Error { message: we.message, fix }
                        }
                    })
                });
            }
            S::Hw(sheets::HelpMsg::CopyFix(f)) => {
                copy_text(&f);
                if let Some(Open::Hw(_, copied)) = self.settings.open.as_mut() {
                    *copied = true;
                }
            }
            S::Remove(RemoveMsg::Remove) => {
                let Some(a) = account else { return };
                let Some(Open::Remove(s)) = self.settings.open.as_mut() else { return };
                if s.busy {
                    return;
                }
                *s = sheets::Remove { busy: true, error: None, sign_out: s.sign_out };
                if self.fixture {
                    self.settings.open = None;
                    self.toast(st, "Account removed", Tone::Info);
                    return;
                }
                self.remove_account(a.address, true);
            }
            S::RemoveAll(RemoveAllMsg::Type) => {
                let typed = match self.settings.open.as_ref() {
                    Some(Open::RemoveAll(s)) => s.typed.clone(),
                    _ => return,
                };
                st.ask_text(ASK_REMOVE, "Type REMOVE to confirm", typed, Some("REMOVE"));
            }
            S::RemoveAll(RemoveAllMsg::Typed(v)) => {
                if let Some(Open::RemoveAll(s)) = self.settings.open.as_mut() {
                    s.typed = v;
                }
            }
            S::RemoveAll(RemoveAllMsg::Wipe) => {
                let Some(Open::RemoveAll(s)) = self.settings.open.as_mut() else { return };
                if !s.ok() || s.busy {
                    return;
                }
                s.busy = true;
                s.error = None;
                if self.fixture {
                    self.settings.open = None;
                    return;
                }
                self.job(|w| {
                    let res = w.keystore.wipe().map_err(|e| e.to_string());
                    w.bump();
                    Done::Wiped(res)
                });
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------ tick

    pub(in crate::screens::wallet) fn manage_wired(_v: &WalletView) -> bool {
        false
    }

    pub(in crate::screens::wallet) fn manage_markup(&self, _v: &WalletView) -> Option<M<Msg>> {
        None
    }

    pub(in crate::screens::wallet) fn manage_tick(&mut self, st: &mut State) -> bool {
        // the keyboard's answers
        if let Some(v) = st.take_answer(ASK_NAME) {
            if let (Some(name), Some(addr)) = (v, self.mg().renaming.take()) {
                if !self.fixture {
                    self.job(move |w| Done::Renamed(w.keystore.rename(&addr, &name).map(|_| ()).map_err(|e| e.to_string())));
                }
            } else {
                self.mg().renaming = None;
            }
            self.ver += 1;
        }
        if let Some(v) = st.take_answer(ASK_PASS) {
            let kind = self.manage_kind();
            if let (Some(v), Some(e)) = (v, self.open_entry()) {
                sheets::entry_update(kind, e, EntryMsg::Typed(v));
            }
            self.ver += 1;
        }
        if let Some(v) = st.take_answer(ASK_REMOVE) {
            if let Some(v) = v {
                if let Some(settings::Open::RemoveAll(s)) = self.settings.open.as_mut() {
                    s.typed = v;
                }
            }
            self.ver += 1;
        }
        if self.fixture {
            return false;
        }
        let Some(w) = service::wallet() else { return false };

        // vault, origins and the saved auto-lock, read when settings opens
        let top = self.stack.top().clone();
        if !self.flows.manage.prefs_loaded {
            self.flows.manage.prefs_loaded = true;
            let p = prefs_file().load();
            self.settings.auto = settings::auto_lock(p.autolock_min.map(|m| m.to_string()).as_deref());
            self.flows.manage.vault = w.keystore.status().ok().map(|s| vault_of(&s));
            self.ver += 1;
        }
        let ver = w.version();
        if ver != self.flows.manage.seen_ver {
            // any wallet event (a signature, an unlock) is the person being here
            self.flows.manage.seen_ver = ver;
            self.manage_touch();
            self.flows.manage.vault = w.keystore.status().ok().map(|s| vault_of(&s));
            self.ver += 1;
        }
        if matches!(top.view, WalletView::Settings) && self.flows.manage.settings_key != top.key {
            self.flows.manage.settings_key = top.key;
            self.flows.manage.origins = rare_wallet::prefs::approved_origins(&prefs_file().load().approved_origins);
            self.flows.manage.vault = w.keystore.status().ok().map(|s| vault_of(&s));
            self.ver += 1;
        }

        // auto-lock after the idle minutes
        let min = self.settings.auto;
        if min > 0 && self.flows.manage.last_press.elapsed() >= Duration::from_secs(min as u64 * 60) {
            self.flows.manage.last_press = Instant::now();
            if w.keystore.has_vault().unwrap_or(false) && w.keystore.is_unlocked().unwrap_or(false) {
                w.keystore.lock();
                w.bump();
            }
        }

        // the unlock sheet's answer
        let mut go = None;
        if let Some((_, rx)) = self.flows.manage.unlock.as_ref() {
            match rx.try_recv() {
                Ok(ok) => go = Some(ok),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => go = Some(false),
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(ok) = go {
            let (then, _) = self.flows.manage.unlock.take().expect("waiting");
            match (then, ok) {
                (Then::AddPhrase(id), true) => self.job(move |w| {
                    let r = w.keystore.derive_next(Some(&id), None).and_then(|a| w.keystore.select(&a.address).map(|_| a.address)).map_err(|e| e.to_string());
                    w.bump();
                    Done::Added(r)
                }),
                (Then::AddPhrase(_), false) => self.accts.busy = false,
                (Then::Remove { addr, settings }, true) => self.start_remove(addr, settings),
                (Then::Remove { settings, .. }, false) => {
                    if settings {
                        if let Some(settings::Open::Remove(s)) = self.settings.open.as_mut() {
                            s.busy = false;
                        }
                    } else {
                        self.flows.manage.rm.busy = false;
                    }
                }
            }
            self.ver += 1;
        }

        // a worker's answer
        if let Some(r) = poll(&mut self.flows.manage.rx) {
            self.manage_done(st, r);
            self.ver += 1;
        }
        self.flows.manage.rx.is_some() || self.flows.manage.unlock.is_some()
    }

    fn manage_done(&mut self, st: &mut State, r: Result<Done, String>) {
        let d = match r {
            Ok(d) => d,
            Err(e) => {
                self.accts.busy = false;
                self.toast(st, e, Tone::Error);
                return;
            }
        };
        let noun = self.noun();
        match d {
            Done::Renamed(Ok(())) => {
                self.changed();
            }
            Done::Renamed(Err(e)) => {
                log::warn!("wallet rename: {e}");
                self.toast(st, "Couldn't rename the account.", Tone::Error);
            }
            Done::Added(Ok(addr)) => {
                self.accts.busy = false;
                self.selected = Some(addr);
                self.changed();
                self.toast(st, "Account added", Tone::Info);
            }
            Done::Added(Err(e)) => {
                self.accts.busy = false;
                log::warn!("wallet add account: {e}");
                self.toast(st, "Couldn't add the account.", Tone::Error);
            }
            Done::Removed { addr, res: Ok(()), settings } => {
                let _ = addr;
                self.accts.removing = None;
                self.flows.manage.rm = sheets::Remove::default();
                let signed_out = settings && matches!(&self.settings.open, Some(settings::Open::Remove(s)) if s.sign_out);
                if settings {
                    self.settings.open = None;
                }
                self.toast(st, if signed_out { "Signed out" } else { "Account removed" }, Tone::Info);
                self.loaded_for = None;
                self.changed();
                // re-read now so an empty wallet goes back to the welcome page
                self.wallet_ver = u64::MAX;
                self.sync_wallet();
                if self.accounts.is_empty() {
                    self.stack.reset(Vec::new());
                }
            }
            Done::Removed { res: Err(e), settings, .. } => {
                let msg = if e.len() < 140 { e } else { "Could not remove it".into() };
                if settings {
                    if let Some(settings::Open::Remove(s)) = self.settings.open.as_mut() {
                        *s = sheets::Remove { busy: false, error: Some(msg), sign_out: s.sign_out };
                    }
                } else {
                    self.flows.manage.rm = sheets::Remove { busy: false, error: Some(msg), sign_out: false };
                }
            }
            Done::PinChanged(res) => {
                if let Some(settings::Open::Pin(s)) = self.settings.open.as_mut() {
                    sheets::change_result(s, res.map_err(|f| secret_error(&f, noun)));
                }
                self.changed();
            }
            Done::Secret(res) => {
                if let Some(settings::Open::Reveal(_, s)) = self.settings.open.as_mut() {
                    match res {
                        Ok(secret) => {
                            sheets::entry_done(&mut s.entry, None);
                            s.secret = Some(secret);
                        }
                        Err(f) => sheets::entry_done(&mut s.entry, Some(secret_error(&f, noun))),
                    }
                }
            }
            Done::Wiped(Ok(())) => {
                self.settings.open = None;
                self.stack.reset(Vec::new());
                self.loaded_for = None;
                self.flows.manage.vault = None;
                self.changed();
                self.wallet_ver = u64::MAX;
                self.sync_wallet();
                self.toast(st, "All wallets removed from this device", Tone::Info);
            }
            Done::Wiped(Err(e)) => {
                if let Some(settings::Open::RemoveAll(s)) = self.settings.open.as_mut() {
                    s.busy = false;
                    s.error = Some(if e.len() < 140 { e } else { "Could not remove the wallets".into() });
                }
            }
            Done::Ledger(c) => {
                if let Some(settings::Open::Hw(res, _)) = self.settings.open.as_mut() {
                    *res = Some(c);
                }
            }
        }
    }
}

/// The phrase or key of an account in the device vault (no vault PIN).
fn reveal_device(w: &service::Wallet, what: RevealWhat, address: &str) -> Result<String, rare_wallet::keystore::KeystoreError> {
    match what {
        RevealWhat::Phrase => w.keystore.reveal_device_mnemonic(address).map(|z| z.to_string()),
        RevealWhat::Key => w.keystore.reveal_device_private_key(address).map(|z| z.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rare_wallet::keystore::{Keystore, MemoryStore};
    use rare_wallet::vault::{Kdf, Vault};
    use rare_wallet::WalletError;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn vault_errors_become_plain_messages() {
        let e = KeystoreError::Vault(WalletError { code: "wrong".into(), message: "x".into(), data: Some(json!({"retryAfterMs": 30000})) });
        assert_eq!(secret_error(&fail_of(&e), "PIN"), "Wrong PIN. Wait 30 seconds and try again.");
        assert_eq!(fail_of(&KeystoreError::Locked), SecretFail::Locked);
    }

    #[test]
    fn origins_come_out_of_every_stored_shape() {
        let a = json!(["https://a.com", {"origin": "https://b.com", "at": 1}]);
        assert_eq!(rare_wallet::prefs::approved_origins(&without_origin(&a, "https://a.com")), ["https://b.com"]);
        assert_eq!(rare_wallet::prefs::approved_origins(&without_origin(&a, "https://b.com")), ["https://a.com"]);
        let o = json!({"https://a.com": true, "https://b.com": true});
        assert_eq!(rare_wallet::prefs::approved_origins(&without_origin(&o, "https://a.com")), ["https://b.com"]);
    }

    #[test]
    fn rename_remove_change_and_reveal_on_a_temp_vault() {
        let dir = std::env::temp_dir().join(format!("rare-manage-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ks = Keystore::new(MemoryStore::default(), Arc::new(Vault::new(dir.clone(), false, Kdf { m: 8, t: 1, p: 1 })));
        let w = ks.add_watch("0x00000000000000000000000000000000000000000000000000000000000000aa", Some("Demo")).unwrap();
        assert_eq!(ks.rename(&w.address, "  Renamed ").unwrap().label, "Renamed");
        assert!(ks.rename(&w.address, "  ").is_err());
        let a = ks.add_phrase(None, Some("Main"), Some(rare_wallet::keystore::PinOptions { secret: "123456", kind: "pin" }), None).unwrap();
        assert!(ks.reveal_mnemonic("123456", &a.address).unwrap().split_whitespace().count() >= 12);
        assert_eq!(secret_error(&fail_of(&ks.reveal_mnemonic("000000", &a.address).unwrap_err()), "PIN"), "Wrong PIN. Try again.");
        ks.change_secret("123456", "654321", "pin").unwrap();
        assert!(ks.reveal_private_key("654321", &a.address).unwrap().starts_with("suiprivkey"));
        ks.remove(&w.address).unwrap();
        assert_eq!(ks.list_accounts().unwrap().len(), 1);
        ks.wipe().unwrap();
        assert!(ks.list_accounts().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
