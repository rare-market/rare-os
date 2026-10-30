//! Create a wallet (new recovery phrase + PIN setup), import a phrase or a private key, watch an
//! address, add the next account from the phrase (Welcome's Derive), all against rare-wallet's
//! keystore. The pages' views and pure state live in `onboard/*` and `pages/watch.rs`; this file
//! performs their effects: the keyboard, the PIN gate, and the keystore work on a worker.

use crate::screens::wallet::home::lib::Hint;
use crate::screens::wallet::home::stack::{ImportWhat, WalletView};
use crate::screens::wallet::markup::El as M;
use crate::screens::wallet::onboard::flow::{plain_error, Failure, Gate};
use crate::screens::wallet::onboard::pin_setup::{self, PinOptions};
use crate::screens::wallet::onboard::{create_phrase, import};
use crate::screens::wallet::page::WalletTab;
use crate::screens::wallet::pages::watch;
use crate::screens::wallet::service;
use crate::screens::wallet::types::SecretKind;
use crate::state::{State, Tone};
use rare_wallet::keystore::{AccountStore, Keystore, PinOptions as KsPin};
use rare_wallet::recipient::{resolve_recipient, RecipientState};
use std::sync::mpsc::Receiver;

const K_WORD: &str = "wallet:onb:word";
const K_PASTE: &str = "wallet:onb:paste";
const K_KEY: &str = "wallet:onb:key";
const K_PASS: &str = "wallet:onb:pass";
const K_PASS2: &str = "wallet:onb:pass2";
const K_ADDR: &str = "wallet:onb:addr";
const K_NAME: &str = "wallet:onb:name";
const KEYS: [&str; 7] = [K_WORD, K_PASTE, K_KEY, K_PASS, K_PASS2, K_ADDR, K_NAME];

/// Whose answer a pending gate or job is for.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Who {
    Create,
    Import(ImportWhat),
    Derive,
    Watch,
}

/// Keystore work.
enum Job {
    Phrase { words: String, pin: Option<PinOptions> },
    Key { key: String, pin: Option<PinOptions> },
    Derive,
}

type Answer = Result<Result<String, Failure>, String>;

#[derive(Default)]
pub struct Flow {
    create: create_phrase::State,
    import: import::State,
    gate: Option<(Who, Receiver<bool>)>,
    work: Option<(Who, Receiver<Answer>)>,
    /// the word box the keyboard is open for
    ask_word: usize,
    /// open the next word box's keyboard at this time
    next_word: Option<(usize, f64)>,
    derive_busy: bool,
    derive_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Create(create_phrase::Msg),
    Import(import::Msg),
}

fn fail(e: impl std::fmt::Display) -> Failure {
    Failure::msg(&e.to_string())
}

fn ks_pin(p: &Option<PinOptions>) -> Option<KsPin<'_>> {
    p.as_ref().map(|p| KsPin { secret: &p.secret, kind: if p.kind == SecretKind::Pin { "pin" } else { "passphrase" } })
}

/// Adds the account, selects it, answers its address.
fn run_job<S: AccountStore>(ks: &Keystore<S>, job: Job) -> Result<String, Failure> {
    // No PIN given: the device vault (opens at login, signs with A alone).
    let device = ks.has_device_vault();
    let acc = match &job {
        Job::Phrase { words, pin: None } if device => ks.add_phrase_device(Some(words), None, None),
        Job::Key { key, pin: None } if device => ks.add_private_key_device(key, None),
        Job::Phrase { words, pin } => ks.add_phrase(Some(words), None, ks_pin(pin), None),
        Job::Key { key, pin } => ks.add_private_key(key, None, ks_pin(pin)),
        Job::Derive => ks.derive_next(None, None),
    }
    .map_err(fail)?;
    ks.select(&acc.address).map_err(fail)?;
    Ok(acc.address)
}

fn run_watch<S: AccountStore>(ks: &Keystore<S>, address: &str, label: Option<&str>) -> Result<String, Failure> {
    let acc = ks.add_watch(address, label).map_err(fail)?;
    ks.select(&acc.address).map_err(fail)?;
    Ok(acc.address)
}

fn rnd_source() -> impl FnMut(usize) -> usize {
    use std::hash::{BuildHasher, Hasher};
    let mut s = std::collections::hash_map::RandomState::new().build_hasher().finish();
    move |n| {
        // splitmix64
        s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) % n.max(1) as u64) as usize
    }
}

impl WalletTab {
    pub(in crate::screens::wallet) fn onboard_markup(&self, v: &WalletView) -> Option<M<Msg>> {
        let f = &self.flows.onboard;
        match v {
            WalletView::CreatePhrase => Some(create_phrase::view(&f.create).map(&Msg::Create)),
            WalletView::Import { what } => {
                let phrase = f.import.words.join(" ");
                let chk = import::Check {
                    phrase_ok: f.import.words.iter().all(|w| !w.is_empty()) && rare_wallet::keys::is_valid_mnemonic(&phrase),
                    key_ok: !f.import.key.trim().is_empty() && rare_wallet::keys::Keypair::from_secret(&f.import.key).is_ok(),
                };
                Some(import::view(*what, &f.import, &chk).map(&Msg::Import))
            }
            _ => None,
        }
    }

    pub(in crate::screens::wallet) fn onboard_wired(v: &WalletView) -> bool {
        matches!(v, WalletView::CreatePhrase | WalletView::Import { .. })
    }

    pub(in crate::screens::wallet) fn onboard_hints(&self, v: &WalletView) -> Option<Vec<Hint>> {
        match v {
            WalletView::CreatePhrase => Some(create_phrase::hints(&self.flows.onboard.create)),
            WalletView::Import { .. } => Some(import::hints(&self.flows.onboard.import)),
            _ => None,
        }
    }

    /// A fresh page state when one of these pages opens.
    pub(in crate::screens::wallet) fn onboard_reset(&mut self) {
        let f = &mut self.flows.onboard;
        f.create = Default::default();
        f.import = Default::default();
        f.next_word = None;
    }

    /// The Add account page's Derive busy state and error.
    pub(in crate::screens::wallet) fn derive_state(&self) -> (bool, Option<String>) {
        (self.flows.onboard.derive_busy, self.flows.onboard.derive_error.clone())
    }

    pub(in crate::screens::wallet) fn onboard_handle(&mut self, st: &mut State, m: Msg) {
        match m {
            Msg::Create(m) => self.create_msg(st, m),
            Msg::Import(m) => self.import_msg(st, m),
        }
    }

    // ---------------------------------------------------------------- create

    fn create_msg(&mut self, st: &mut State, m: create_phrase::Msg) {
        if let Some(e) = create_phrase::update(&mut self.flows.onboard.create, m) {
            self.create_effect(st, e);
        }
    }

    fn create_effect(&mut self, st: &mut State, e: create_phrase::Effect) {
        use create_phrase::Effect as E;
        match e {
            E::NewPhrase { bits } => {
                let w = rare_wallet::keys::new_mnemonic(bits);
                create_phrase::words(&mut self.flows.onboard.create, &w);
            }
            E::Quiz => {
                let words = self.flows.onboard.create.words.clone();
                let q = create_phrase::make_quiz(&words, &rare_wallet::keys::wordlist()[..], &mut rnd_source());
                create_phrase::quiz(&mut self.flows.onboard.create, q);
            }
            E::Gate => self.start_gate(Who::Create, "Enter your PIN to add this phrase"),
            E::Add { words, pin } => self.start_job(Who::Create, Job::Phrase { words, pin }),
            E::AskPass { confirm } => ask_pass(st, confirm),
        }
    }

    // ---------------------------------------------------------------- import

    fn import_what(&self) -> ImportWhat {
        match &self.stack.top().view {
            WalletView::Import { what } => *what,
            _ => ImportWhat::Phrase,
        }
    }

    fn import_msg(&mut self, st: &mut State, m: import::Msg) {
        let what = self.import_what();
        let wl = &rare_wallet::keys::wordlist()[..];
        if let Some(e) = import::update(what, &mut self.flows.onboard.import, m, wl) {
            self.import_effect(st, what, e);
        }
    }

    fn import_effect(&mut self, st: &mut State, what: ImportWhat, e: import::Effect) {
        use import::Effect as E;
        match e {
            E::AskWord(i) => {
                self.flows.onboard.ask_word = i;
                st.ask_text(K_WORD, format!("Word {}", i + 1), "", Some("First letters"));
            }
            E::AskPaste => st.ask_text(K_PASTE, "Paste all words", "", Some("12 or 24 words")),
            E::AskKey => crate::screens::wallet::sign::host::ask_secret(st, K_KEY, "Private key"),
            E::AskPass { confirm } => ask_pass(st, confirm),
            E::NextWord(i) => self.flows.onboard.next_word = Some((i, rare_render::clock::secs() + 0.12)),
            // Restore a PIN account: the words go to the PIN account flow (PIN, the backup's sign in)
            E::Gate if what == ImportWhat::Restore => {
                let words = zeroize::Zeroizing::new(self.flows.onboard.import.words.join(" "));
                for w in self.flows.onboard.import.words.iter_mut() {
                    zeroize::Zeroize::zeroize(w);
                }
                self.flows.pin_account = super::pin_account::Flow::restore(words, self.fixture);
                self.stack.pop();
                self.stack.push(WalletView::PinAccount, None);
                self.ver += 1;
            }
            E::Gate => self.start_gate(Who::Import(what), "Enter your PIN to add this wallet"),
            E::AddPhrase { words, pin } => self.start_job(Who::Import(what), Job::Phrase { words, pin }),
            E::AddKey { key, pin } => self.start_job(Who::Import(what), Job::Key { key, pin }),
        }
    }

    // ---------------------------------------------------------------- watch

    /// The Watch page's messages (page.rs routes `Msg::Watch` here).
    pub(in crate::screens::wallet) fn watch_msg(&mut self, st: &mut State, m: watch::Msg) {
        match watch::update(&mut self.watch, m) {
            Some(watch::Effect::Ask { prompt, value, placeholder }) => {
                let key = if prompt.starts_with("Address") { K_ADDR } else { K_NAME };
                st.ask_text(key, prompt, value, Some(placeholder));
            }
            Some(watch::Effect::Watch { input, name }) => self.start_watch(input, name),
            None => {}
        }
    }

    fn start_watch(&mut self, input: String, name: Option<String>) {
        if self.fixture {
            watch::done(&mut self.watch, Err(Failure::msg("Not available in the preview")));
            return;
        }
        let rx = service::work(move |w| -> Result<String, Failure> {
            let address = match w.rt.block_on(resolve_recipient(&w.names(), &input)) {
                RecipientState::Ready { address, .. } => address,
                RecipientState::Invalid { msg } | RecipientState::Missing { msg, .. } | RecipientState::Error { msg, .. } => return Err(Failure::msg(&msg)),
                _ => return Err(Failure::msg("Enter a 0x address or a .sui name")),
            };
            let label = name.or_else(|| (!input.starts_with("0x")).then(|| input.clone()));
            let r = run_watch(&w.keystore, &address, label.as_deref());
            w.bump();
            r
        });
        self.flows.onboard.work = Some((Who::Watch, rx));
    }

    // ---------------------------------------------------------------- derive

    /// Welcome's Derive and Accounts' "Add from phrase": unlock, derive the next account of the
    /// phrase on this device, select it, say so.
    pub(in crate::screens::wallet) fn derive_next_account(&mut self, st: &mut State) {
        let f = &mut self.flows.onboard;
        if f.derive_busy {
            return;
        }
        if self.fixture {
            st.toast("Account added", Tone::Info, rare_render::clock::secs());
            return;
        }
        f.derive_busy = true;
        f.derive_error = None;
        self.start_gate(Who::Derive, "Enter your PIN to add an account");
        self.ver += 1;
    }

    // ---------------------------------------------------------------- gate and jobs

    fn start_gate(&mut self, who: Who, reason: &str) {
        // New phrases and imports go to the device vault: no PIN to set or enter. The next account
        // of a phrase there needs none either.
        let device = self.fixture || service::wallet().is_some_and(|w| w.keystore.has_device_vault());
        let device_phrase = !self.fixture && service::wallet().is_some_and(|w| w.keystore.has_device_phrase().unwrap_or(false));
        if (matches!(who, Who::Create | Who::Import(_)) && device) || (who == Who::Derive && device_phrase) {
            self.deliver_gate(who, Gate::Ready);
            return;
        }
        let has = !self.fixture && service::wallet().is_some_and(|w| w.keystore.has_vault().unwrap_or(false));
        if matches!(who, Who::Create | Who::Import(_)) && !has {
            self.deliver_gate(who, Gate::Pin);
            return;
        }
        self.flows.onboard.gate = Some((who, service::ask_unlock(reason)));
    }

    fn deliver_gate(&mut self, who: Who, g: Gate) {
        match who {
            Who::Create => {
                if let Some(e) = create_phrase::gate(&mut self.flows.onboard.create, g) {
                    self.run_effect_no_osk(who, Some(e), None);
                }
            }
            Who::Import(what) => {
                if let Some(e) = import::gate(what, &mut self.flows.onboard.import, g) {
                    self.run_effect_no_osk(who, None, Some(e));
                }
            }
            Who::Derive => match g {
                Gate::Ready => self.start_job(Who::Derive, Job::Derive),
                _ => self.flows.onboard.derive_busy = false,
            },
            Who::Watch => {}
        }
    }

    /// Effects that start jobs (no keyboard needed).
    fn run_effect_no_osk(&mut self, who: Who, c: Option<create_phrase::Effect>, i: Option<import::Effect>) {
        match (who, c, i) {
            (Who::Create, Some(create_phrase::Effect::Add { words, pin }), _) => self.start_job(who, Job::Phrase { words, pin }),
            (Who::Import(_), _, Some(import::Effect::AddPhrase { words, pin })) => self.start_job(who, Job::Phrase { words, pin }),
            (Who::Import(_), _, Some(import::Effect::AddKey { key, pin })) => self.start_job(who, Job::Key { key, pin }),
            _ => {}
        }
    }

    fn start_job(&mut self, who: Who, job: Job) {
        if self.fixture {
            self.job_done(who, Ok(Err(Failure::msg("Not available in the preview"))));
            return;
        }
        let rx = service::work(move |w| {
            let r = run_job(&w.keystore, job);
            w.bump();
            r
        });
        self.flows.onboard.work = Some((who, rx));
    }

    fn job_done(&mut self, who: Who, r: Answer) {
        let res = match r {
            Ok(r) => r,
            Err(e) => Err(Failure::msg(&e)),
        };
        let ok = res.as_ref().ok().cloned();
        let now = rare_render::clock::secs();
        match who {
            Who::Create => create_phrase::added(&mut self.flows.onboard.create, res.map(|_| ())),
            Who::Import(_) => import::added(&mut self.flows.onboard.import, res.map(|_| ())),
            Who::Watch => watch::done(&mut self.watch, res.map(|_| ())),
            Who::Derive => {
                let f = &mut self.flows.onboard;
                f.derive_busy = false;
                if let Err(e) = res {
                    let msg = plain_error(&e, "Could not add the account");
                    f.derive_error = Some(msg);
                    PENDING_TOAST.with(|t| *t.borrow_mut() = Some((self.flows.onboard.derive_error.clone().unwrap_or_default(), Tone::Error, now)));
                } else {
                    PENDING_TOAST.with(|t| *t.borrow_mut() = Some(("Account added".into(), Tone::Info, now)));
                }
            }
        }
        if let Some(addr) = ok {
            self.finish_add(addr);
        }
        self.ver += 1;
    }

    /// An account was added: it is selected, go to the wallet home.
    fn finish_add(&mut self, address: String) {
        self.sync_wallet();
        self.selected = Some(address);
        self.stack.reset(Vec::new());
        crate::screens::wallet::home::page::account_changed(&mut self.home);
        self.watch = watch::State::default();
        let f = &mut self.flows.onboard;
        f.create = Default::default();
        f.import = Default::default();
        f.next_word = None;
        f.derive_error = None;
    }

    // ---------------------------------------------------------------- tick

    pub(in crate::screens::wallet) fn onboard_tick(&mut self, st: &mut State) -> bool {
        let now = rare_render::clock::secs();
        // gate answers
        if let Some((who, rx)) = self.flows.onboard.gate.take() {
            match rx.try_recv() {
                Ok(ok) => {
                    self.deliver_gate(who, if ok { Gate::Ready } else { Gate::Cancelled });
                    self.ver += 1;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => self.flows.onboard.gate = Some((who, rx)),
                Err(_) => {
                    self.deliver_gate(who, Gate::Cancelled);
                    self.ver += 1;
                }
            }
        }
        // keystore work
        if let Some((who, rx)) = self.flows.onboard.work.take() {
            let mut slot = Some(rx);
            match service::poll(&mut slot) {
                Some(r) => self.job_done(who, r),
                None => self.flows.onboard.work = slot.map(|rx| (who, rx)),
            }
        }
        if let Some((text, tone, at)) = PENDING_TOAST.with(|t| t.borrow_mut().take()) {
            st.toast(text, tone, at);
        }
        // keyboard answers
        let on_create = matches!(self.stack.top().view, WalletView::CreatePhrase);
        let on_import = matches!(self.stack.top().view, WalletView::Import { .. });
        if let Some(a) = st.take_answer(K_WORD) {
            if let Some(t) = a.filter(|_| on_import) {
                let i = self.flows.onboard.ask_word;
                self.import_msg(st, import::Msg::Typed(i, t));
                self.ver += 1;
            }
        }
        if let Some(a) = st.take_answer(K_PASTE) {
            if let Some(t) = a.filter(|_| on_import) {
                self.import_msg(st, import::Msg::Pasted(t));
                self.ver += 1;
            }
        }
        if let Some(a) = st.take_answer(K_KEY) {
            if let Some(t) = a.filter(|_| on_import) {
                self.import_msg(st, import::Msg::Key(t));
                self.ver += 1;
            }
        }
        for (key, confirm) in [(K_PASS, false), (K_PASS2, true)] {
            if let Some(Some(t)) = st.take_answer(key) {
                let m = if confirm { pin_setup::Msg::Pass2(t) } else { pin_setup::Msg::Pass(t) };
                if on_create {
                    self.create_msg(st, create_phrase::Msg::Pin(m));
                } else if on_import {
                    self.import_msg(st, import::Msg::Pin(m));
                }
                self.ver += 1;
            }
        }
        if let Some(Some(t)) = st.take_answer(K_ADDR) {
            self.watch_msg(st, watch::Msg::Address(t));
            self.ver += 1;
        }
        if let Some(Some(t)) = st.take_answer(K_NAME) {
            self.watch_msg(st, watch::Msg::Name(t));
            self.ver += 1;
        }
        if let Some((i, at)) = self.flows.onboard.next_word {
            if now >= at {
                self.flows.onboard.next_word = None;
                if on_import {
                    self.import_effect(st, self.import_what(), import::Effect::AskWord(i));
                }
            }
        }
        let f = &self.flows.onboard;
        f.gate.is_some() || f.work.is_some() || f.next_word.is_some() || KEYS.iter().any(|k| st.asking(k).is_some())
    }
}

fn ask_pass(st: &mut State, confirm: bool) {
    let (key, title) = if confirm { (K_PASS2, "Confirm passphrase") } else { (K_PASS, "Passphrase") };
    crate::screens::wallet::sign::host::ask_secret(st, key, title);
}

thread_local! {
    /// A toast job_done wants shown (it has no `State`); tick shows it.
    static PENDING_TOAST: std::cell::RefCell<Option<(String, Tone, f64)>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use rare_wallet::keystore::MemoryStore;
    use rare_wallet::vault::{Kdf, Vault};
    use std::sync::Arc;

    const PHRASE: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    #[test]
    fn create_import_watch_derive() {
        let dir = std::env::temp_dir().join(format!("rare-onboard-test-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let vault = Arc::new(Vault::new(dir.join("w"), false, Kdf { m: 2048, t: 2, p: 1 }));
        let ks = Keystore::new(MemoryStore::default(), vault);
        assert!(!ks.has_vault().unwrap());
        let pin = Some(PinOptions { kind: SecretKind::Pin, secret: "123456".into() });
        // a bad phrase is refused in plain words
        let bad = run_job(&ks, Job::Phrase { words: "abandon abandon".into(), pin: pin.clone() });
        assert!(bad.is_err());
        let a = run_job(&ks, Job::Phrase { words: PHRASE.into(), pin }).unwrap();
        assert!(ks.has_vault().unwrap());
        assert_eq!(ks.meta().unwrap().selected.as_deref(), Some(a.as_str()));
        let b = run_job(&ks, Job::Derive).unwrap();
        assert_ne!(a, b);
        let key = rare_wallet::keys::Keypair::from_mnemonic(&rare_wallet::keys::new_mnemonic(128), 0).unwrap().to_suiprivkey().to_string();
        let c = run_job(&ks, Job::Key { key, pin: None }).unwrap();
        let w = run_watch(&ks, &format!("0x{}", "ab".repeat(32)), Some("Friend")).unwrap();
        let accs = ks.meta().unwrap().accounts;
        assert_eq!(accs.len(), 4);
        assert!([&a, &b, &c, &w].iter().all(|x| accs.iter().any(|y| &&y.address == x)));
        assert!(run_watch(&ks, "nope", None).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
