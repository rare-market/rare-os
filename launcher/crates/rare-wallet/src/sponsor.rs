//! Gas sponsorship from rare.market ("Rare covers the fee"). Port of src/wallet/sponsor.ts.
//!
//! ```text
//! 1. GET  /v1/sponsor/status?address=   what's on offer for this address (cached ~30 s)
//! 2. POST /v1/sponsor                    send the transaction kind; get back the full transaction
//!                                        (gas owner = sponsor) and the sponsor's signature
//! 3. the person signs those exact bytes in Rare Wallet's review
//! 4. POST /v1/sponsor/execute            the server adds the sponsor signature and submits
//! ```
//!
//! Game budgets (category "game:<slug>", rare.market api/src/sponsor/game.ts): a studio funds a
//! budget and its players' fees come out of it, for what its policy covers. The launcher asks for
//! every transaction a web game has Rare Wallet sign and execute, and for Local API sell, buy and
//! claim from a game (`sponsor_for_game`); any "no" (503, 429, 403, budget_exhausted) means the
//! player pays as before, and the review sheet says who pays (`game_fee_note`).
//!
//! Every expected "no" comes back as a typed `SponsorFail`, never an `Err`, so callers fall back
//! to the wallet paying its own fee. A kind that touches the gas coin is never sent.

use crate::keys::normalize_address;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Mutex;
use sui_sdk_types::{Argument, Command, Transaction, TransactionKind};

// ---------------------------------------------------------------- categories

/// Rare's own categories (Rare's budget and limits), or a game's budget ("game:<slug>").
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Category {
    Rescue,
    Trophy,
    Trade,
    Walrus,
    /// Claiming the Rare Device Pass (rare.market api SPONSOR_DEVICE_TARGETS).
    Device,
    Game(String),
}

impl Category {
    pub fn as_str(&self) -> String {
        match self {
            Category::Rescue => "rescue".into(),
            Category::Trophy => "trophy".into(),
            Category::Trade => "trade".into(),
            Category::Walrus => "walrus".into(),
            Category::Device => "device".into(),
            Category::Game(slug) => format!("game:{slug}"),
        }
    }
    fn key(&self) -> &str {
        match self {
            Category::Rescue => "rescue",
            Category::Trophy => "trophy",
            Category::Trade => "trade",
            Category::Walrus => "walrus",
            Category::Device => "device",
            Category::Game(_) => "",
        }
    }
}

/// A catalog slug the API accepts: [a-z0-9][a-z0-9-]{0,63}.
pub fn valid_slug(slug: &str) -> bool {
    let b = slug.as_bytes();
    !b.is_empty() && b.len() <= 64 && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// "game:<slug>" for a slug the API accepts, else None.
pub fn game_category(slug: Option<&str>) -> Option<Category> {
    slug.filter(|s| valid_slug(s)).map(|s| Category::Game(s.to_string()))
}

// ---------------------------------------------------------------- status

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameYou {
    pub today_tx: u64,
    pub today_mist: String,
    pub eligible: bool,
}

/// A game's gas budget in /v1/sponsor/status?game= (the API's SponsorStatus.game).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameGasStatus {
    pub slug: String,
    /// the game's budget pays its players' fees right now
    pub covered: bool,
    pub available_mist: Option<String>,
    pub paused: bool,
    pub player_daily_tx: Option<u64>,
    pub player_daily_mist: Option<String>,
    pub you: Option<GameYou>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SponsorYou {
    pub sponsored: u64,
    pub gas_mist: String,
    pub trade_used: bool,
    pub eligible: Option<BTreeMap<String, bool>>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SponsorStatus {
    pub enabled: bool,
    pub funded: bool,
    /// the sponsor wallet (gas owner of sponsored transactions)
    pub address: String,
    pub balance_mist: Option<String>,
    pub daily_budget_mist: Option<String>,
    pub spent_today_mist: Option<String>,
    pub remaining_today_mist: Option<String>,
    pub categories: BTreeMap<String, bool>,
    pub max_user_sui_mist: Option<String>,
    pub you: Option<SponsorYou>,
    /// with ?game=: that game's budget
    pub game: Option<GameGasStatus>,
}

/// JS `BigInt(s)`: trimmed decimal, "" is 0; None when it would throw.
fn big(s: &str) -> Option<i128> {
    let t = s.trim();
    if t.is_empty() {
        return Some(0);
    }
    t.parse().ok()
}

/// Digits only, else 0 (the TS `big` for amounts in answers).
fn digits(v: &Value) -> u64 {
    let s = text(v);
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
        s.parse().unwrap_or(u64::MAX)
    } else {
        0
    }
}

/// JS `String(v ?? "")`.
fn text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// 0.01 SUI: the fee assumed when there's no estimate.
pub const DEFAULT_FEE_MIST: u64 = 10_000_000;
/// What a wallet needs on top of the estimate before it's "enough SUI for the fee".
const MARGIN_MIST: u64 = 5_000_000;
const STATUS_MS: u64 = 30_000;
/// Less than this before a sponsorship's expiry and a fresh one is asked for.
const EXPIRY_SLACK_MS: u64 = 5_000;

/// True when the wallet's SUI won't cover the fee (estimate plus a margin; 0.01 SUI without one).
pub fn needs_sponsor(sui_balance_mist: u64, estimated_gas_mist: Option<u64>) -> bool {
    let est = estimated_gas_mist.filter(|e| *e > 0).unwrap_or(DEFAULT_FEE_MIST);
    let margin = (est / 2).max(MARGIN_MIST);
    (sui_balance_mist as u128) < est as u128 + margin as u128
}

/// Whether the status offers this category to this address right now.
pub fn offers(s: Option<&SponsorStatus>, category: &Category) -> bool {
    let Some(s) = s else { return false };
    if !s.enabled || !s.funded {
        return false;
    }
    let key = category.key();
    if !s.categories.get(key).copied().unwrap_or(false) {
        return false;
    }
    if let Some(e) = s.you.as_ref().and_then(|y| y.eligible.as_ref()) {
        if !e.get(key).copied().unwrap_or(false) {
            return false;
        }
    }
    if *category == Category::Trade && s.you.as_ref().is_some_and(|y| y.trade_used) {
        return false;
    }
    // missing or unparsable budget figure: let the server decide
    if let Some(r) = s.remaining_today_mist.as_deref().and_then(big) {
        if r <= 0 {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------- results

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailReason {
    /// not funded, disabled, or the status said no
    Unavailable,
    Busy,
    /// policy said no; message says why
    Refused,
    /// the game's gas budget is used up (budget_exhausted)
    Exhausted,
    /// the game's budget doesn't cover this player now (status said no; nothing was sent)
    NotCovered,
    Limited,
    Simulation,
    /// the kind uses the gas coin: never sent
    GasCoin,
    /// the kind could not be read or built here
    Build,
    /// the server's answer did not match what was asked
    Mismatch,
    Network,
    Expired,
    AlreadyExecuted,
    Failed,
}

impl FailReason {
    /// The TS reason string.
    pub fn as_str(self) -> &'static str {
        match self {
            FailReason::Unavailable => "unavailable",
            FailReason::Busy => "busy",
            FailReason::Refused => "refused",
            FailReason::Exhausted => "exhausted",
            FailReason::NotCovered => "not-covered",
            FailReason::Limited => "limited",
            FailReason::Simulation => "simulation",
            FailReason::GasCoin => "gas-coin",
            FailReason::Build => "build",
            FailReason::Mismatch => "mismatch",
            FailReason::Network => "network",
            FailReason::Expired => "expired",
            FailReason::AlreadyExecuted => "already-executed",
            FailReason::Failed => "failed",
        }
    }
    fn plain(self) -> Option<&'static str> {
        Some(match self {
            FailReason::Unavailable => "Rare can't cover fees right now.",
            FailReason::Busy => "Rare's fee cover is busy. Try again in a moment.",
            FailReason::Limited => "Rare has covered as many fees as it can for now.",
            FailReason::Exhausted => "This game's gas budget is used up.",
            FailReason::Simulation => "This would fail right now.",
            FailReason::Expired => "The fee cover ran out of time. Try again.",
            FailReason::Network => "Couldn't reach rare.market.",
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SponsorFail {
    pub reason: FailReason,
    pub code: String,
    pub message: String,
}

fn fail(reason: FailReason, code: &str, message: impl Into<String>) -> SponsorFail {
    SponsorFail { reason, code: code.into(), message: message.into() }
}

fn plain_fail(reason: FailReason, code: &str) -> SponsorFail {
    fail(reason, code, reason.plain().unwrap_or(""))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sponsorship {
    /// base64 TransactionData, gas owner = sponsor: the person signs exactly these
    pub bytes: String,
    pub sponsor_signature: String,
    pub digest: String,
    pub sponsor: String,
    pub category: String,
    pub gas_budget: u64,
    pub estimated_gas_mist: u64,
    /// ms epoch; ask again after this
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Executed {
    pub digest: String,
    pub gas_used_mist: u64,
}

/// Executed with the bytes the person signed, their signature and the sponsor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SponsoredRun {
    pub digest: String,
    pub gas_used_mist: u64,
    pub bytes: String,
    pub signature: String,
    pub sponsor: String,
}

// ---------------------------------------------------------------- the gas coin check

fn arg_is_gas(a: &Argument) -> bool {
    matches!(a, Argument::Gas)
}

/// Any command that uses the gas coin.
pub fn uses_gas_coin(kind: &TransactionKind) -> bool {
    let TransactionKind::ProgrammableTransaction(p) = kind else { return false };
    p.commands.iter().any(|c| match c {
        Command::MoveCall(m) => m.arguments.iter().any(arg_is_gas),
        Command::TransferObjects(t) => t.objects.iter().any(arg_is_gas) || arg_is_gas(&t.address),
        Command::SplitCoins(s) => arg_is_gas(&s.coin) || s.amounts.iter().any(arg_is_gas),
        Command::MergeCoins(m) => arg_is_gas(&m.coin) || m.coins_to_merge.iter().any(arg_is_gas),
        Command::MakeMoveVector(v) => v.elements.iter().any(arg_is_gas),
        Command::Upgrade(u) => arg_is_gas(&u.ticket),
        Command::Publish(_) => false,
        // a command this crate version doesn't know: assume the worst
        #[allow(unreachable_patterns)]
        _ => true,
    })
}

fn gas_coin_fail() -> SponsorFail {
    fail(FailReason::GasCoin, "gas_coin", "This transaction pays from the gas coin, so Rare can't cover its fee.")
}

/// Checks kind bytes (base64) before they are sent: readable, programmable, no gas coin.
pub fn check_kind(kind_b64: &str) -> Result<(), SponsorFail> {
    let raw = B64.decode(kind_b64).map_err(|_| fail(FailReason::Build, "build_failed", "unreadable transaction kind"))?;
    let kind: TransactionKind = bcs::from_bytes(&raw).map_err(|e| fail(FailReason::Build, "build_failed", e.to_string()))?;
    if uses_gas_coin(&kind) {
        return Err(gas_coin_fail());
    }
    Ok(())
}

/// Kind bytes (base64) of full TransactionData bytes a game built, refusing any that touch the
/// gas coin.
pub fn kind_of_bytes(bytes: &[u8]) -> Result<String, SponsorFail> {
    let tx: Transaction = bcs::from_bytes(bytes).map_err(|e| fail(FailReason::Build, "build_failed", e.to_string()))?;
    if uses_gas_coin(&tx.kind) {
        return Err(fail(FailReason::GasCoin, "gas_coin", "This transaction pays from the gas coin, so its fee can't be covered."));
    }
    Ok(B64.encode(bcs::to_bytes(&tx.kind).expect("kind serializes")))
}

/// The sponsored bytes match the kind, the sender and the sponsor the server named.
fn check_sponsored(bytes: &str, kind: &str, sender: &str, sponsor: &str) -> Option<String> {
    let raw = match B64.decode(bytes) {
        Ok(r) => r,
        Err(_) => return Some("unreadable transaction".into()),
    };
    let tx: Transaction = match bcs::from_bytes(&raw) {
        Ok(t) => t,
        Err(e) => return Some(e.to_string()),
    };
    let norm = |a: &str| normalize_address(a).unwrap_or_default();
    let (sender, sponsor) = (norm(sender), norm(sponsor));
    if tx.sender.to_string() != sender {
        return Some("sender changed".into());
    }
    let owner = tx.gas_payment.owner.to_string();
    if owner != sponsor {
        return Some("gas owner is not the sponsor".into());
    }
    if owner == sender {
        return Some("gas owner is the sender".into());
    }
    if B64.encode(bcs::to_bytes(&tx.kind).expect("kind serializes")) != kind {
        return Some("transaction changed".into());
    }
    None
}

fn error_parts(body: &Value) -> (String, String) {
    match body.get("error") {
        Some(Value::Object(o)) => (o.get("code").and_then(Value::as_str).unwrap_or("error").into(), o.get("message").and_then(Value::as_str).unwrap_or("").into()),
        Some(Value::String(s)) if !s.is_empty() => (s.clone(), s.clone()),
        _ => ("error".into(), String::new()),
    }
}

fn reason_for(status: u16, code: &str) -> FailReason {
    match () {
        _ if code == "budget_exhausted" => FailReason::Exhausted,
        _ if code == "sponsor_busy" => FailReason::Busy,
        _ if code == "sponsor_unavailable" || status == 503 => FailReason::Unavailable,
        _ if code == "not_sponsored" || status == 403 => FailReason::Refused,
        _ if status == 429 => FailReason::Limited,
        _ if code == "simulation_failed" || status == 422 => FailReason::Simulation,
        _ if code == "sponsorship_expired" || status == 410 => FailReason::Expired,
        _ if code == "already_executed" || status == 409 => FailReason::AlreadyExecuted,
        _ => FailReason::Failed,
    }
}

// ---------------------------------------------------------------- the API

/// rare.market, through the launcher's rare_get / rare_post proxy (the API sends no CORS
/// headers), or a test double. Blocking: call from a worker thread.
pub trait SponsorApi: Send + Sync {
    /// GET rare.market{path}; Err on network errors and non-2xx.
    fn get(&self, path: &str) -> Result<Value, String>;
    /// POST JSON to rare.market{path}; any status comes back, network failures are Err.
    fn post(&self, path: &str, body: &Value) -> Result<(u16, Value), String>;
    /// ms since the epoch
    fn now_ms(&self) -> u64;
}

/// Sponsor client with the status caches and the sponsor wallets seen so far.
pub struct Sponsor<A: SponsorApi> {
    pub api: A,
    status: Mutex<HashMap<String, (u64, Option<SponsorStatus>)>>,
    game_status: Mutex<HashMap<String, (u64, Option<SponsorStatus>)>>,
    known: Mutex<BTreeSet<String>>,
}

/// What a game asked to be signed and executed.
pub enum GameTx<'a> {
    /// base64 kind bytes the launcher built
    Kind(&'a str),
    /// full TransactionData bytes a web game built (its gas data is dropped when the game pays)
    Bytes(&'a [u8]),
}

fn encode_component(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

impl<A: SponsorApi> Sponsor<A> {
    pub fn new(api: A) -> Self {
        Sponsor { api, status: Mutex::default(), game_status: Mutex::default(), known: Mutex::default() }
    }

    fn fetch_status(&self, path: &str) -> Option<SponsorStatus> {
        let r = self.api.get(path).ok()?;
        let d = r.get("data")?;
        d.get("address")?.as_str()?;
        let s: SponsorStatus = serde_json::from_value(d.clone()).ok()?;
        if let Some(a) = normalize_address(&s.address) {
            self.known.lock().unwrap().insert(a);
        }
        Some(s)
    }

    /// Sponsor status for an address, cached ~30 s. None when the API can't be reached.
    pub fn status(&self, address: &str) -> Option<SponsorStatus> {
        let key = normalize_address(address).unwrap_or_else(|| address.to_string());
        let now = self.api.now_ms();
        if let Some((at, v)) = self.status.lock().unwrap().get(&key) {
            if now.saturating_sub(*at) < STATUS_MS {
                return v.clone();
            }
        }
        let v = self.fetch_status(&format!("/v1/sponsor/status?address={key}"));
        self.status.lock().unwrap().insert(key, (now, v.clone()));
        v
    }

    /// Forget cached status (after a sponsored transaction, so "first trade used" shows).
    pub fn clear_status(&self, address: Option<&str>) {
        let mut c = self.status.lock().unwrap();
        match address {
            Some(a) => {
                c.remove(&normalize_address(a).unwrap_or_else(|| a.to_string()));
            }
            None => c.clear(),
        }
    }

    /// Sponsor wallets seen in status answers: a gas owner among these is "covered by Rare".
    pub fn known_sponsors(&self) -> Vec<String> {
        self.known.lock().unwrap().iter().cloned().collect()
    }

    /// Should Rare offer to cover the fee? Only when the wallet is short of SUI for it and the
    /// sponsor offers this category to this address. `balance` runs only when needed.
    pub fn offer(&self, address: &str, category: &Category, balance: impl FnOnce() -> Option<u64>, estimated_gas_mist: Option<u64>) -> Option<SponsorStatus> {
        let bal = balance()?;
        if !needs_sponsor(bal, estimated_gas_mist) {
            return None;
        }
        let s = self.status(address);
        if !offers(s.as_ref(), category) {
            return None;
        }
        let s = s?;
        // the server sponsors only wallets that hold less than this
        if let Some(max) = s.max_user_sui_mist.as_deref().filter(|m| !m.is_empty()).and_then(big) {
            if bal as i128 > max {
                return None;
            }
        }
        Some(s)
    }

    /// Ask rare.market to pay the fee for kind bytes (base64, checked here for the gas coin).
    pub fn sponsor_kind(&self, kind_b64: &str, sender: &str, category: &Category) -> Result<Sponsorship, SponsorFail> {
        check_kind(kind_b64)?;
        self.sponsor_kind_bytes(kind_b64, sender, category)
    }

    /// As `sponsor_kind`, for kind bytes already checked.
    pub fn sponsor_kind_bytes(&self, kind_b64: &str, sender: &str, category: &Category) -> Result<Sponsorship, SponsorFail> {
        let sender_n = normalize_address(sender).unwrap_or_else(|| sender.to_string());
        let cat = category.as_str();
        let (status, body) = self
            .api
            .post("/v1/sponsor", &json!({ "sender": sender_n, "kind": kind_b64, "category": cat }))
            .map_err(|_| plain_fail(FailReason::Network, "network"))?;
        let data = body.get("data").filter(|d| !d.is_null());
        let Some(s) = data.filter(|_| status == 200) else {
            let (code, message) = error_parts(&body);
            let reason = reason_for(status, &code);
            let msg = if reason == FailReason::Refused && !message.is_empty() {
                message
            } else {
                reason.plain().map(str::to_string).unwrap_or(if message.is_empty() { format!("Sponsor error ({status})") } else { message })
            };
            return Err(fail(reason, &code, msg));
        };
        let field = |k: &str| text(s.get(k).unwrap_or(&Value::Null));
        let (bytes, sponsor) = (field("bytes"), field("sponsor"));
        let bad = if bytes.is_empty() || sponsor.is_empty() || field("sponsorSignature").is_empty() || field("digest").is_empty() {
            Some("incomplete answer".to_string())
        } else {
            check_sponsored(&bytes, kind_b64, sender, &sponsor)
        };
        if let Some(bad) = bad {
            return Err(fail(FailReason::Mismatch, "mismatch", format!("The sponsored transaction didn't match: {bad}.")));
        }
        if let Some(a) = normalize_address(&sponsor) {
            self.known.lock().unwrap().insert(a);
        }
        let expires = s.get("expiresAt").and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|t| t.trim().parse().ok()))).filter(|n| *n != 0.0 && n.is_finite());
        Ok(Sponsorship {
            bytes,
            sponsor_signature: field("sponsorSignature"),
            digest: field("digest"),
            sponsor,
            category: s.get("category").filter(|v| !v.is_null()).map(text).unwrap_or(cat),
            gas_budget: digits(s.get("gasBudget").unwrap_or(&Value::Null)),
            estimated_gas_mist: digits(s.get("estimatedGasMist").unwrap_or(&Value::Null)),
            expires_at: expires.map(|n| n.max(0.0) as u64).unwrap_or_else(|| self.api.now_ms() + 110_000),
        })
    }

    /// Hand the person's signature over the sponsored bytes to rare.market, which submits.
    pub fn execute(&self, digest: &str, user_signature: &str) -> Result<Executed, SponsorFail> {
        let (status, body) = self
            .api
            .post("/v1/sponsor/execute", &json!({ "digest": digest, "signature": user_signature }))
            .map_err(|_| plain_fail(FailReason::Network, "network"))?;
        let Some(x) = body.get("data").filter(|d| !d.is_null() && status == 200) else {
            let (code, message) = error_parts(&body);
            let reason = reason_for(status, &code);
            let msg = reason.plain().map(str::to_string).unwrap_or(if message.is_empty() { format!("Sponsor error ({status})") } else { message });
            return Err(fail(reason, &code, msg));
        };
        let truthy = |v: Option<&Value>| match v {
            None | Some(Value::Null) | Some(Value::Bool(false)) => false,
            Some(Value::String(s)) => !s.is_empty(),
            Some(Value::Number(n)) => n.as_f64() != Some(0.0),
            _ => true,
        };
        if !truthy(x.get("ok")) {
            let err = x.get("error").map(text).filter(|e| !e.is_empty()).unwrap_or_else(|| "The network rejected this transaction.".into());
            return Err(fail(FailReason::Failed, "execution_failed", err));
        }
        Ok(Executed {
            digest: x.get("digest").filter(|d| !d.is_null()).map(text).unwrap_or_else(|| digest.to_string()),
            gas_used_mist: digits(x.get("gasUsedMist").unwrap_or(&Value::Null)),
        })
    }

    /// Sponsor, sign, execute. An expired sponsorship (before signing or at execute) is asked
    /// for again once. Signer errors are returned as they are (`Err(Err(e))`), sponsor "no"s as
    /// `Err(Ok(fail))`.
    pub fn run<E>(
        &self,
        kind_b64: &str,
        sender: &str,
        category: &Category,
        first: Option<Sponsorship>,
        mut sign: impl FnMut(&[u8]) -> Result<String, E>,
    ) -> Result<SponsoredRun, Result<SponsorFail, E>> {
        let mut s = first;
        for attempt in 0..2 {
            let cur = match s.take() {
                Some(c) if c.expires_at.saturating_sub(self.api.now_ms()) >= EXPIRY_SLACK_MS => c,
                _ => self.sponsor_kind(kind_b64, sender, category).map_err(Ok)?,
            };
            let raw = B64.decode(&cur.bytes).map_err(|_| Ok(fail(FailReason::Mismatch, "mismatch", "unreadable transaction")))?;
            let signature = sign(&raw).map_err(Err)?;
            match self.execute(&cur.digest, &signature) {
                Ok(e) => {
                    self.clear_status(Some(sender));
                    return Ok(SponsoredRun { digest: e.digest, gas_used_mist: e.gas_used_mist, bytes: cur.bytes, signature, sponsor: cur.sponsor });
                }
                Err(f) if f.reason == FailReason::Expired && attempt == 0 => {}
                Err(f) => return Err(Ok(f)),
            }
        }
        Err(Ok(plain_fail(FailReason::Expired, "sponsorship_expired")))
    }

    // ---------------------------------------------------------------- game budgets

    /// Sponsor status with a game's budget for an address, cached ~30 s.
    pub fn game_status(&self, slug: &str, address: &str) -> Option<SponsorStatus> {
        let addr = normalize_address(address).unwrap_or_else(|| address.to_string());
        let key = format!("{slug}|{addr}");
        let now = self.api.now_ms();
        if let Some((at, v)) = self.game_status.lock().unwrap().get(&key) {
            if now.saturating_sub(*at) < STATUS_MS {
                return v.clone();
            }
        }
        let v = self.fetch_status(&format!("/v1/sponsor/status?address={addr}&game={}", encode_component(slug)));
        self.game_status.lock().unwrap().insert(key, (now, v.clone()));
        v
    }

    pub fn clear_game_status(&self) {
        self.game_status.lock().unwrap().clear();
    }

    /// Ask the game's budget to pay. Checks the game's status first (cached) and asks only when
    /// it would cover this player. Returns the sponsorship and the kind it came from (kept to ask
    /// again on expiry).
    pub fn sponsor_for_game(&self, tx: GameTx<'_>, sender: &str, slug: &str) -> Result<(Sponsorship, String, Category), SponsorFail> {
        let category = game_category(Some(slug)).ok_or_else(|| fail(FailReason::NotCovered, "not_covered", "This game doesn't cover fees."))?;
        if let Some(no) = game_not_covered(self.game_status(slug, sender).as_ref(), slug) {
            return Err(no);
        }
        let (kind, r) = match tx {
            GameTx::Bytes(b) => {
                let kind = kind_of_bytes(b)?;
                let r = self.sponsor_kind_bytes(&kind, sender, &category);
                (kind, r)
            }
            GameTx::Kind(k) => (k.to_string(), self.sponsor_kind(k, sender, &category)),
        };
        match r {
            Ok(s) => Ok((s, kind, category)),
            Err(f) => {
                // the budget may have changed: look again next time
                if matches!(f.reason, FailReason::Exhausted | FailReason::Limited) {
                    self.clear_game_status();
                }
                Err(f)
            }
        }
    }
}

/// Why a game's budget won't pay, from its status; None when it would (the server still decides).
pub fn game_not_covered(s: Option<&SponsorStatus>, slug: &str) -> Option<SponsorFail> {
    let Some(s) = s else { return Some(plain_fail(FailReason::Network, "network")) };
    if !s.enabled || !s.funded {
        return Some(plain_fail(FailReason::Unavailable, "sponsor_unavailable"));
    }
    let Some(g) = s.game.as_ref().filter(|g| g.slug == slug) else {
        return Some(fail(FailReason::NotCovered, "not_covered", "This game doesn't cover fees."));
    };
    // no figure: let the server decide
    let available = match g.available_mist.as_deref() {
        Some(a) => big(a),
        None => None,
    };
    if available.is_some_and(|a| a <= 0) {
        return Some(plain_fail(FailReason::Exhausted, "budget_exhausted"));
    }
    if g.paused || !g.covered {
        return Some(fail(FailReason::NotCovered, "not_covered", "This game doesn't cover fees right now."));
    }
    if g.you.as_ref().is_some_and(|y| !y.eligible) {
        return Some(fail(FailReason::Limited, "player_limit", "This game has covered as many of your fees as it can today."));
    }
    None
}

/// What the review sheet says when a game's budget didn't pay and the player does. None: say nothing.
pub fn game_fee_note(f: &SponsorFail, game: &str) -> Option<String> {
    match f.reason {
        FailReason::Exhausted => Some(format!("{game}'s gas budget is used up, so you pay this fee.")),
        FailReason::Limited => Some(format!("{game} has covered as many of your fees as it can today, so you pay this one.")),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
