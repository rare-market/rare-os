//! Rare Wallet's signing pipeline. Port of src/wallet/sign.ts. Every signature in the launcher
//! goes through here: Buy in the Marketplace, sends from the wallet, Trade, web games. Each call
//! queues a request; the review sheet shows `requests()[0]` (what leaves, what arrives, the fee),
//! the person approves (PIN, phone tap, or hardware confirm as the account needs) and only then
//! is anything signed. Nothing is ever signed without a review.
//!
//! ```text
//! let t = signing.sign_and_execute_with_cover(ptb, Category::Trophy, SignOptions { title: Some("Claim".into()), ..Default::default() })?;
//! // the review sheet: signing.requests(), then signing.approve(id, None, &mut devices) or signing.reject(id, None)
//! let result = t.wait()?;  // digest once the network has it; Err(SignError::Rejected) when declined
//! ```
//!
//! Blocking: run it on a wallet worker thread. Chain work goes through `Chain` (rare-sui's gRPC
//! client in the app, a fake in tests), sponsorship through `sponsor::Sponsor`.

use crate::keystore::{AccountKind, AccountStore, Keystore, KeystoreError, RareAccount, Signer};
use crate::sponsor::{Category, FailReason, GameTx, Sponsor, SponsorApi, SponsorFail, Sponsorship};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rare_sui::ptb::Tx;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};

// ---------------------------------------------------------------- types

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginKind {
    Wallet,
    Market,
    Game,
}

/// Who asks: "Rare Marketplace", a game's title, or "Rare Wallet" for the wallet's own sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignOrigin {
    pub name: String,
    /// web origin for games
    pub url: Option<String>,
    pub icon: Option<String>,
    pub kind: OriginKind,
}

impl SignOrigin {
    pub fn wallet() -> Self {
        SignOrigin { name: "Rare Wallet".into(), url: None, icon: None, kind: OriginKind::Wallet }
    }
    pub fn new(name: &str, kind: OriginKind) -> Self {
        SignOrigin { name: name.into(), url: None, icon: None, kind }
    }
}

/// A transaction to review: built bytes (TransactionData BCS, e.g. from a web game) or an
/// unresolved PTB from a builder (resolved on the node at review time).
#[derive(Debug, Clone, PartialEq)]
pub enum TxInput {
    Bytes(Vec<u8>),
    Ptb(Tx),
    /// A PTB with gas the caller set (a web game's Transaction JSON with its gasData).
    PtbGas(Tx, rare_sui::tx::BuildOpts),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignKind {
    Transaction,
    Message,
}

/// Rare (or a game's budget) pays the fee: `tx` holds the sponsored bytes; Approve signs them and
/// rare.market submits.
#[derive(Debug, Clone, PartialEq)]
pub struct SponsoredSign {
    pub sponsor: String,
    pub category: Category,
    /// who pays, for the review sheet: "Rare", or the game whose gas budget does
    pub payer: Option<String>,
    /// the kind the sponsorship came from (asked again if it expires)
    pub kind: String,
    pub first: Sponsorship,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SignRequest {
    pub id: u64,
    pub kind: SignKind,
    /// the signing account (address)
    pub account: String,
    pub origin: SignOrigin,
    pub tx: Option<TxInput>,
    pub message: Option<Vec<u8>>,
    /// submit after signing
    pub execute: bool,
    pub dry_run: bool,
    /// title override for the sheet ("Buy Popkins #18736", "Send 5 SUI")
    pub title: Option<String>,
    pub sponsored: Option<SponsoredSign>,
    /// a line under the network fee ("<game>'s gas budget is used up, so you pay this fee.")
    pub fee_note: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignResult {
    /// base64 transaction bytes or message bytes
    pub bytes: String,
    pub signature: String,
    pub digest: Option<String>,
    /// "success" | "failure" after execution
    pub status: Option<String>,
    /// base64 BCS TransactionEffects after execution (Wallet Standard signAndExecuteTransaction)
    pub effects: Option<String>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SignError {
    /// The person declined ("You declined the request", or the reason given).
    Rejected(String),
    Keystore(KeystoreError),
    /// A device or passkey failure (code and message as the device layer reports them).
    Device { code: String, message: String },
    /// Building or the node.
    Chain(String),
    /// The sponsor said no at Approve (the request stays open; the sheet can retry or pay).
    Sponsor(SponsorFail),
    /// The network rejected the transaction (the request is finished).
    Failed(String),
    /// No such request (approved, rejected or never queued).
    Gone,
}

impl std::fmt::Display for SignError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignError::Rejected(m) | SignError::Chain(m) | SignError::Failed(m) => f.write_str(m),
            SignError::Keystore(e) => write!(f, "{e}"),
            SignError::Device { message, .. } => f.write_str(message),
            SignError::Sponsor(s) => f.write_str(&s.message),
            SignError::Gone => f.write_str("Request is gone"),
        }
    }
}

impl std::error::Error for SignError {}

impl From<KeystoreError> for SignError {
    fn from(e: KeystoreError) -> Self {
        SignError::Keystore(e)
    }
}

pub const DECLINED: &str = "You declined the request";

#[derive(Debug, Clone, Default)]
pub struct SignOptions {
    /// signing account (address); the selected account by default
    pub account: Option<String>,
    pub origin: Option<SignOrigin>,
    pub title: Option<String>,
    pub dry_run: bool,
    /// shown under the network fee in the review sheet
    pub fee_note: Option<String>,
}

// ---------------------------------------------------------------- the chain and devices

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Executed {
    pub digest: String,
    pub success: bool,
    /// base64 BCS effects
    pub effects: Option<String>,
}

/// What the pipeline needs from the node.
pub trait Chain: Send + Sync {
    /// Resolve inputs and gas (the sender pays unless `opts` names a sponsor) and serialize:
    /// TransactionData BCS.
    fn build(&self, tx: &Tx, sender: &str, opts: &rare_sui::tx::BuildOpts) -> Result<Vec<u8>, String>;
    /// The resolved transaction kind (base64), gas paid by `sponsor` when given.
    fn build_kind(&self, tx: &Tx, sender: &str, sponsor: Option<&str>) -> Result<String, String>;
    fn execute(&self, bytes: &[u8], signatures: &[String]) -> Result<Executed, String>;
    /// Simulate only (dry run): whether it would succeed.
    fn dry_run(&self, bytes: &[u8]) -> Result<bool, String>;
    /// SUI balance in MIST.
    fn sui_balance(&self, owner: &str) -> Result<u64, String>;
}

/// Signs for accounts whose key is elsewhere (Ledger, Keystone, the passkey on a phone): the app
/// runs that device's flow (with its prompt state) and returns the base64 serialized signature.
pub trait ExternalSigner {
    fn sign(&mut self, account: &RareAccount, public_key: &str, transaction: bool, bytes: &[u8]) -> Result<String, SignError>;
}

/// For tests and accounts that never need a device.
pub struct NoDevices;

impl ExternalSigner for NoDevices {
    fn sign(&mut self, account: &RareAccount, _: &str, _: bool, _: &[u8]) -> Result<String, SignError> {
        Err(KeystoreError::Unsupported(account.kind).into())
    }
}

// ---------------------------------------------------------------- the queue

/// The answer to one request, for its caller.
#[derive(Debug)]
pub struct Ticket {
    pub id: u64,
    rx: mpsc::Receiver<Result<SignResult, SignError>>,
}

impl Ticket {
    /// Waits for Approve (the result) or Cancel (`SignError::Rejected`).
    pub fn wait(self) -> Result<SignResult, SignError> {
        self.rx.recv().unwrap_or(Err(SignError::Gone))
    }
    pub fn try_get(&self) -> Option<Result<SignResult, SignError>> {
        self.rx.try_recv().ok()
    }
}

struct Pending {
    req: SignRequest,
    reply: mpsc::Sender<Result<SignResult, SignError>>,
}

pub struct Signing<S: AccountStore, C: Chain, A: SponsorApi> {
    pub keystore: Arc<Keystore<S>>,
    pub chain: C,
    pub sponsor: Arc<Sponsor<A>>,
    queue: Mutex<Vec<Pending>>,
    seq: AtomicU64,
    version: AtomicU64,
}

impl<S: AccountStore, C: Chain, A: SponsorApi> Signing<S, C, A> {
    pub fn new(keystore: Arc<Keystore<S>>, chain: C, sponsor: Arc<Sponsor<A>>) -> Self {
        Signing { keystore, chain, sponsor, queue: Mutex::default(), seq: AtomicU64::new(0), version: AtomicU64::new(0) }
    }

    /// Pending requests, oldest first. The review sheet shows the first.
    pub fn requests(&self) -> Vec<SignRequest> {
        self.queue.lock().unwrap().iter().map(|p| p.req.clone()).collect()
    }

    /// Moves on every queue change (for the UI to redraw).
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }

    fn enqueue(&self, mut req: SignRequest) -> Ticket {
        let (tx, rx) = mpsc::channel();
        req.id = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let id = req.id;
        self.queue.lock().unwrap().push(Pending { req, reply: tx });
        self.version.fetch_add(1, Ordering::SeqCst);
        Ticket { id, rx }
    }

    fn finish(&self, id: u64) -> Option<Pending> {
        let mut q = self.queue.lock().unwrap();
        let i = q.iter().position(|p| p.req.id == id)?;
        let p = q.remove(i);
        self.version.fetch_add(1, Ordering::SeqCst);
        Some(p)
    }

    fn signer_account(&self, address: Option<&str>) -> Result<String, SignError> {
        if let Some(a) = address {
            return Ok(a.to_string());
        }
        let cur = self.keystore.current()?.ok_or_else(|| SignError::Chain("Add a Rare Wallet account first".into()))?;
        if cur.kind == AccountKind::Watch {
            return Err(SignError::Chain("This account is watch only. Add a signing account to continue.".into()));
        }
        Ok(cur.address)
    }

    fn request(&self, kind: SignKind, account: String, o: &SignOptions, tx: Option<TxInput>, message: Option<Vec<u8>>, execute: bool) -> SignRequest {
        SignRequest {
            id: 0,
            kind,
            account,
            origin: o.origin.clone().unwrap_or_else(SignOrigin::wallet),
            tx,
            message,
            execute,
            dry_run: o.dry_run,
            title: o.title.clone(),
            sponsored: None,
            fee_note: o.fee_note.clone(),
        }
    }

    // ---- entry points

    /// Review, sign and submit. Resolves with the digest once the network has the transaction.
    pub fn sign_and_execute(&self, tx: TxInput, o: SignOptions) -> Result<Ticket, SignError> {
        let account = self.signer_account(o.account.as_deref())?;
        Ok(self.enqueue(self.request(SignKind::Transaction, account, &o, Some(tx), None, true)))
    }

    /// Review, sign and submit a transaction whose fee a sponsor covers. The review shows the
    /// sponsored bytes; rare.market submits them with both signatures. An expired sponsorship is
    /// asked for again once at Approve.
    pub fn sign_and_execute_sponsored(&self, kind: String, sponsorship: Sponsorship, category: Category, o: SignOptions, payer: Option<String>) -> Result<Ticket, SignError> {
        let account = self.signer_account(o.account.as_deref())?;
        let bytes = B64.decode(&sponsorship.bytes).map_err(|_| SignError::Chain("unreadable sponsored transaction".into()))?;
        let mut r = self.request(SignKind::Transaction, account, &o, Some(TxInput::Bytes(bytes)), None, true);
        r.sponsored = Some(SponsoredSign { sponsor: sponsorship.sponsor.clone(), category, payer, kind, first: sponsorship });
        Ok(self.enqueue(r))
    }

    /// A transaction a game asked for (a web game's signAndExecute, or a Local API sell, buy or
    /// claim): the game's gas budget pays when it covers it (category "game:<slug>"); otherwise
    /// `fallback` runs (default: the wallet pays), with a note when the budget is used up.
    pub fn sign_and_execute_for_game(
        &self,
        tx: TxInput,
        slug: Option<&str>,
        game_name: &str,
        mut o: SignOptions,
        fallback: Option<&dyn Fn(SignOptions) -> Result<Ticket, SignError>>,
    ) -> Result<Ticket, SignError> {
        let account = self.signer_account(o.account.as_deref())?;
        if let Some(slug) = slug {
            let asked = match &tx {
                TxInput::Bytes(b) => self.sponsor.sponsor_for_game(GameTx::Bytes(b), &account, slug),
                TxInput::Ptb(p) | TxInput::PtbGas(p, _) => {
                    let sponsor = self.sponsor.game_status(slug, &account).map(|s| s.address);
                    match self.chain.build_kind(p, &account, sponsor.as_deref()) {
                        Ok(kind) => self.sponsor.sponsor_for_game(GameTx::Kind(&kind), &account, slug),
                        Err(e) => Err(SponsorFail { reason: FailReason::Build, code: "build_failed".into(), message: e }),
                    }
                }
            };
            match asked {
                Ok((s, kind, category)) => {
                    o.account = Some(account);
                    return self.sign_and_execute_sponsored(kind, s, category, o, Some(game_name.to_string()));
                }
                Err(f) => {
                    if let Some(note) = crate::sponsor::game_fee_note(&f, game_name) {
                        o.fee_note = Some(note);
                    }
                }
            }
        }
        o.account = Some(account);
        match fallback {
            Some(f) => f(o),
            None => self.sign_and_execute(tx, o),
        }
    }

    /// For trophy claims, Walrus uploads and the other sponsored categories: Rare covers the fee
    /// when the wallet is short of SUI and rare.market offers `category` to it; otherwise, or when
    /// the sponsor says no, the wallet pays. Build `tx` without the gas coin.
    pub fn sign_and_execute_with_cover(&self, tx: Tx, category: Category, mut o: SignOptions) -> Result<Ticket, SignError> {
        let account = self.signer_account(o.account.as_deref())?;
        o.account = Some(account.clone());
        let offer = self.sponsor.offer(&account, &category, || self.chain.sui_balance(&account).ok(), None);
        if let Some(status) = offer {
            if let Ok(kind) = self.chain.build_kind(&tx, &account, Some(&status.address)) {
                if let Ok(s) = self.sponsor.sponsor_kind(&kind, &account, &category) {
                    return self.sign_and_execute_sponsored(kind, s, category, o, None);
                }
            }
        }
        self.sign_and_execute(TxInput::Ptb(tx), o)
    }

    /// Review and sign without submitting (games that submit themselves).
    pub fn sign_transaction(&self, tx: TxInput, o: SignOptions) -> Result<Ticket, SignError> {
        let account = self.signer_account(o.account.as_deref())?;
        Ok(self.enqueue(self.request(SignKind::Transaction, account, &o, Some(tx), None, false)))
    }

    pub fn sign_personal_message(&self, message: Vec<u8>, o: SignOptions) -> Result<Ticket, SignError> {
        let account = self.signer_account(o.account.as_deref())?;
        Ok(self.enqueue(self.request(SignKind::Message, account, &o, None, Some(message), false)))
    }

    // ---- the sheet

    /// Cancel.
    pub fn reject(&self, id: u64, reason: Option<&str>) {
        if let Some(p) = self.finish(id) {
            let _ = p.reply.send(Err(SignError::Rejected(reason.unwrap_or(DECLINED).to_string())));
        }
    }

    /// Transaction bytes for a request: a PTB is resolved on the node with the account as sender.
    pub fn request_bytes(&self, r: &SignRequest) -> Result<Vec<u8>, SignError> {
        match &r.tx {
            Some(TxInput::Bytes(b)) => Ok(b.clone()),
            Some(TxInput::Ptb(p)) => self.chain.build(p, &r.account, &Default::default()).map_err(SignError::Chain),
            Some(TxInput::PtbGas(p, opts)) => self.chain.build(p, &r.account, opts).map_err(SignError::Chain),
            None => Err(SignError::Chain("No transaction".into())),
        }
    }

    /// Approve, after the review and once the account can sign (vault unlocked when it needs
    /// it: the UI asks for the PIN first). `built` is the exact bytes the review simulated, when it
    /// built them. Device and passkey accounts sign through `devices`. Signer errors (wrong device,
    /// timeout, locked) leave the request open so the sheet can retry.
    pub fn approve(&self, id: u64, built: Option<Vec<u8>>, devices: &mut dyn ExternalSigner) -> Result<SignResult, SignError> {
        let r = self.queue.lock().unwrap().iter().find(|p| p.req.id == id).map(|p| p.req.clone()).ok_or(SignError::Gone)?;
        let signer = self.keystore.get_signer(&r.account)?;
        let mut sign = |bytes: &[u8], transaction: bool| -> Result<String, SignError> {
            match &signer {
                Signer::External { account, public_key } => devices.sign(account, public_key, transaction, bytes),
                local if transaction => local.sign_transaction(bytes).map_err(SignError::from),
                local => local.sign_personal_message(bytes).map(|(_, s)| s).map_err(SignError::from),
            }
        };

        let result = if r.kind == SignKind::Message {
            let msg = r.message.clone().unwrap_or_default();
            SignResult { bytes: B64.encode(&msg), signature: sign(&msg, false)?, ..Default::default() }
        } else if let (Some(sp), true, false) = (&r.sponsored, r.execute, r.dry_run) {
            // rare.market adds the sponsor's signature and submits
            match self.sponsor.run(&sp.kind, &r.account, &sp.category, Some(sp.first.clone()), |b| sign(b, true)) {
                Ok(out) => SignResult { bytes: out.bytes, signature: out.signature, digest: Some(out.digest), status: Some("success".into()), ..Default::default() },
                Err(Ok(f)) if f.reason == FailReason::Failed => {
                    let msg = if f.message.is_empty() { "The network rejected this transaction".to_string() } else { f.message.clone() };
                    if let Some(p) = self.finish(id) {
                        let _ = p.reply.send(Err(SignError::Failed(msg)));
                    }
                    return Ok(SignResult { bytes: sp.first.bytes.clone(), status: Some("failure".into()), ..Default::default() });
                }
                Err(Ok(f)) => return Err(SignError::Sponsor(f)),
                Err(Err(e)) => return Err(e),
            }
        } else {
            let bytes = match built {
                Some(b) => b,
                None => self.request_bytes(&r)?,
            };
            let signature = sign(&bytes, true)?;
            let mut res = SignResult { bytes: B64.encode(&bytes), signature: signature.clone(), ..Default::default() };
            if r.execute && !r.dry_run {
                let x = self.chain.execute(&bytes, &[signature]).map_err(SignError::Chain)?;
                res.digest = Some(x.digest);
                res.status = Some(if x.success { "success" } else { "failure" }.into());
                res.effects = x.effects;
                if !x.success {
                    if let Some(p) = self.finish(id) {
                        let _ = p.reply.send(Err(SignError::Failed("The network rejected this transaction".into())));
                    }
                    return Ok(res);
                }
            } else if r.execute && r.dry_run {
                let ok = self.chain.dry_run(&bytes).map_err(SignError::Chain)?;
                let data: sui_sdk_types::Transaction = bcs::from_bytes(&bytes).map_err(|e| SignError::Chain(e.to_string()))?;
                res.digest = Some(data.digest().to_string());
                res.status = Some(if ok { "success" } else { "failure" }.into());
                res.dry_run = true;
            }
            res
        };
        if let Some(p) = self.finish(id) {
            let _ = p.reply.send(Ok(result.clone()));
        }
        Ok(result)
    }
}

// ---------------------------------------------------------------- the node (rare-sui)

/// `Chain` over rare-sui's gRPC client, from a plain worker thread on `rt` (make the `Sui` inside
/// that runtime, e.g. under `rt.enter()`).
pub struct SuiChain {
    pub sui: rare_sui::grpc::Sui,
    pub rt: tokio::runtime::Handle,
}

impl Chain for SuiChain {
    fn build(&self, tx: &Tx, sender: &str, opts: &rare_sui::tx::BuildOpts) -> Result<Vec<u8>, String> {
        let mut t = tx.clone();
        if t.sender.is_none() {
            t.set_sender(sender);
        }
        let built = self.rt.block_on(rare_sui::tx::build(&self.sui, &t, opts)).map_err(|e| e.to_string())?;
        Ok(built.bytes)
    }

    fn build_kind(&self, tx: &Tx, sender: &str, sponsor: Option<&str>) -> Result<String, String> {
        let mut t = tx.clone();
        if t.sender.is_none() {
            t.set_sender(sender);
        }
        let opts = rare_sui::tx::BuildOpts { sponsor: sponsor.map(str::to_string), ..Default::default() };
        let built = self.rt.block_on(rare_sui::tx::build(&self.sui, &t, &opts)).map_err(|e| e.to_string())?;
        Ok(B64.encode(bcs::to_bytes(&built.data.kind).map_err(|e| e.to_string())?))
    }

    fn execute(&self, bytes: &[u8], signatures: &[String]) -> Result<Executed, String> {
        let data: sui_sdk_types::Transaction = bcs::from_bytes(bytes).map_err(|e| e.to_string())?;
        let sigs = signatures.iter().map(|s| sui_sdk_types::UserSignature::from_base64(s).map_err(|e| e.to_string())).collect::<Result<Vec<_>, _>>()?;
        let t = self.rt.block_on(self.sui.execute(&data, sigs)).map_err(|e| e.to_string())?;
        let effects = t.effects.as_ref();
        let success = effects.and_then(|e| e.status.as_ref()).and_then(|s| s.success).unwrap_or(false);
        let bcs = effects.and_then(|e| e.bcs.as_ref()).and_then(|b| b.value.as_ref()).map(|v| B64.encode(v));
        Ok(Executed { digest: t.digest.clone().unwrap_or_else(|| data.digest().to_string()), success, effects: bcs })
    }

    fn dry_run(&self, bytes: &[u8]) -> Result<bool, String> {
        let data: sui_sdk_types::Transaction = bcs::from_bytes(bytes).map_err(|e| e.to_string())?;
        let r = self.rt.block_on(self.sui.simulate(&data)).map_err(|e| e.to_string())?;
        Ok(r.transaction.as_ref().and_then(|t| t.effects.as_ref()).and_then(|e| e.status.as_ref()).and_then(|s| s.success).unwrap_or(false))
    }

    fn sui_balance(&self, owner: &str) -> Result<u64, String> {
        use rare_sui::grpc::ChainRead;
        let b = self.rt.block_on(self.sui.balance(owner, "0x2::sui::SUI")).map_err(|e| e.to_string())?;
        Ok(b.balance)
    }
}

#[cfg(test)]
mod tests;
