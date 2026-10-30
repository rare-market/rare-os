//! Swaps through the Trade API's DEX router, the way wallet clients do it (developer API
//! /swap): quote for the trader's wallet, then the unsigned transaction for that quote, which
//! Rare Wallet signs and submits. Nothing here signs a trade.
//!
//! The API wants a key. The launcher gets a free one on its own: a device-local identity key (holds
//! nothing, never signs a trade) answers the API's sign-in challenge once, and the key is kept on
//! the device. Trades still go out from the person's own wallet (`owner`). The identity's signing
//! (Ed25519 over a Sui personal message) is `rare-wallet`'s; this crate asks for it through
//! [`Identity`].

use crate::amount::{normalize_type, SLIPPAGE_BPS};
use crate::http::{error_of, trade_call, Reply, Result, TradeApiError};
use crate::jsnum::{js_string, to_fixed, value_number};
use crate::kv::Kv;
use crate::tx::UnsignedTx;
use rare_market::net::{Method, Net};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq)]
pub struct SwapQuote {
    pub id: String,
    pub coin_in: String,
    pub coin_out: String,
    pub amount_in: u128,
    /// what the wallet receives after every fee, base units
    pub amount_out: u128,
    pub fee_bps: f64,
    pub hops: u32,
    /// exchanges on the route
    pub venues: Vec<String>,
    /// ms since the epoch
    pub expires_at: f64,
}

impl SwapQuote {
    /// Quotes are bound to the wallet and the moment they were asked for: one about to expire is
    /// asked again before building.
    pub fn reusable(&self, now_ms: u64) -> bool {
        self.expires_at - now_ms as f64 > 8_000.0
    }

    /// "2 hops via Cetus, FlowX": the route line under a quote.
    pub fn route_line(&self) -> String {
        let mut names: Vec<String> = Vec::new();
        for v in &self.venues {
            let n = venue_name(v);
            if !names.contains(&n) {
                names.push(n);
            }
        }
        names.truncate(3);
        let via = if self.venues.is_empty() { String::new() } else { format!(" via {}", names.join(", ")) };
        format!("{} {}{via}", self.hops, if self.hops == 1 { "hop" } else { "hops" })
    }

    /// "0.20%"
    pub fn fee_line(&self) -> String {
        format!("{}%", to_fixed(self.fee_bps / 100.0, 2))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SwapBuild {
    /// the unsigned transaction, sender set; gas is resolved when it is built
    pub tx: UnsignedTx,
    pub min_out: Option<u128>,
    pub gas_estimate_mist: Option<u128>,
    pub expires_at: f64,
}

/// Where the device's API key is kept.
pub trait KeyStore: Send + Sync {
    fn get(&self) -> Option<String>;
    fn set(&self, key: Option<&str>);
}

const STORE_KEY: &str = "rare.trade.apikey";

/// The API key in the device's key-value store.
pub struct KvKeys(pub Arc<dyn Kv>);

impl KeyStore for KvKeys {
    fn get(&self) -> Option<String> {
        self.0.get(STORE_KEY).filter(|k| !k.is_empty())
    }
    fn set(&self, key: Option<&str>) {
        match key {
            Some(k) => self.0.set(STORE_KEY, k),
            None => self.0.remove(STORE_KEY),
        }
    }
}

/// The key held in memory only (tests, private sessions: a new key next time).
#[derive(Default)]
pub struct MemKeys(pub Mutex<Option<String>>);

impl MemKeys {
    pub fn with(k: Option<&str>) -> Self {
        MemKeys(Mutex::new(k.map(str::to_string)))
    }
}

impl KeyStore for MemKeys {
    fn get(&self) -> Option<String> {
        self.0.lock().unwrap().clone()
    }
    fn set(&self, key: Option<&str>) {
        *self.0.lock().unwrap() = key.map(str::to_string);
    }
}

/// A throwaway device identity that signs the API's sign-in challenge.
pub trait Identity: Send + Sync {
    /// Its Sui address.
    fn address(&self) -> String;
    /// A Sui personal-message signature (serialized, base64) over `message`.
    fn sign_personal_message(&self, message: &[u8]) -> Result<String>;
}

/// Makes a fresh identity each time a key is needed (the web launcher's `new Ed25519Keypair()`).
pub type IdentityMaker = Arc<dyn Fn() -> Result<Box<dyn Identity>> + Send + Sync>;

/// The swap client: the Trade API, where the key lives, and how to make an identity.
pub struct Swap {
    pub net: Arc<dyn Net>,
    pub keys: Arc<dyn KeyStore>,
    pub identity: IdentityMaker,
    /// ms since the epoch (a fixed clock in tests).
    pub clock: fn() -> u64,
    /// One key request at a time: concurrent callers wait for it and share the key.
    pending: Mutex<()>,
}

/// What to quote.
#[derive(Clone, Debug, PartialEq)]
pub struct QuoteParams {
    pub coin_in: String,
    pub coin_out: String,
    pub amount_in: u128,
    pub owner: Option<String>,
}

/// `x ?? default` for a JSON field: missing and null both fall through.
fn field<'a>(v: &'a Value, k: &str) -> Option<&'a Value> {
    v.get(k).filter(|x| !x.is_null())
}

/// `!x`: JSON falsy (null, false, 0, "").
fn falsy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => true,
        Some(Value::Number(n)) => n.as_f64() == Some(0.0),
        Some(Value::String(s)) => s.is_empty(),
        _ => false,
    }
}

/// `String(x)` of a JSON value (numbers as JS prints them).
fn js_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_u64().map(|u| u.to_string()).unwrap_or_else(|| js_string(n.as_f64().unwrap_or(f64::NAN))),
        Value::Null => "null".into(),
        other => other.to_string(),
    }
}

fn digits(s: &str) -> Option<u128> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

impl Swap {
    pub fn new(net: Arc<dyn Net>, keys: Arc<dyn KeyStore>, identity: IdentityMaker) -> Swap {
        Swap { net, keys, identity, clock: crate::timing::now_ms, pending: Mutex::new(()) }
    }

    fn call(&self, path: &str, body: &Value, auth: Option<&str>) -> Result<Reply> {
        trade_call(self.net.as_ref(), Method::Post, path, Some(body), auth)
    }

    /// The device's API key, created on first use.
    pub fn api_key(&self) -> Result<String> {
        if let Some(k) = self.keys.get() {
            return Ok(k);
        }
        let _one = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(k) = self.keys.get() {
            return Ok(k);
        }
        let id = (self.identity)()?;
        let address = id.address();
        let ch = self.call("/auth/challenge", &json!({ "address": address }), None)?;
        let data = field(&ch.body, "data");
        if ch.status != 200 || falsy(data) {
            return Err(error_of(ch.status, &ch.body));
        }
        let data = data.unwrap();
        let message = data.get("message").and_then(Value::as_str).unwrap_or_default();
        let challenge_id = data.get("challengeId").cloned().unwrap_or(Value::Null);
        let signature = id.sign_personal_message(message.as_bytes())?;
        let k = self.call(
            "/auth/keys",
            &json!({ "address": address, "challengeId": challenge_id, "signature": signature, "name": "Rare Launcher", "scopes": ["quote", "build"] }),
            None,
        )?;
        let key = k.body.pointer("/data/key").and_then(Value::as_str).filter(|s| !s.is_empty());
        let Some(key) = key.filter(|_| k.status == 200) else { return Err(error_of(k.status, &k.body)) };
        self.keys.set(Some(key));
        Ok(key.to_string())
    }

    /// POST with the device key; a key the API no longer knows is replaced once.
    fn keyed(&self, path: &str, body: &Value) -> Result<Value> {
        for attempt in 0..2 {
            let key = self.api_key()?;
            let r = self.call(path, body, Some(&key))?;
            if r.status == 401 && attempt == 0 {
                self.keys.set(None);
                continue;
            }
            let data = field(&r.body, "data");
            if r.status != 200 || falsy(data) {
                return Err(error_of(r.status, &r.body));
            }
            return Ok(data.unwrap().clone());
        }
        Err(TradeApiError::new(401, "authentication_required", "Could not sign in to the swap service."))
    }

    pub fn quote(&self, p: &QuoteParams) -> Result<SwapQuote> {
        if p.amount_in == 0 {
            return Err(TradeApiError::new(400, "invalid_amount", "Choose an amount first."));
        }
        let coin_in = normalize_type(&p.coin_in);
        let coin_out = normalize_type(&p.coin_out);
        let mut body = json!({ "coinIn": coin_in, "coinOut": coin_out, "amountIn": p.amount_in.to_string() });
        if let Some(o) = p.owner.as_deref().filter(|o| !o.is_empty()) {
            body["owner"] = Value::from(o);
        }
        let q = self.keyed("/swap/quote", &body)?;
        let out = field(&q, "amountOut").map(js_text).unwrap_or_default();
        let id = q.get("id").and_then(Value::as_str);
        let (Some(id), Some(amount_out)) = (id, digits(&out).filter(|a| *a > 0)) else {
            return Err(TradeApiError::new(502, "invalid_quote", "No route for this pair right now."));
        };
        let hops = field(&q, "hops").map(value_number).unwrap_or(1.0);
        Ok(SwapQuote {
            id: id.to_string(),
            coin_in,
            coin_out,
            amount_in: p.amount_in,
            amount_out,
            fee_bps: field(&q, "feeBps").map(value_number).unwrap_or(0.0),
            hops: if hops.is_finite() && hops >= 1.0 { hops as u32 } else { 1 },
            venues: route_venues(q.get("route").unwrap_or(&Value::Null)),
            expires_at: field(&q, "expiresAt").map(value_number).unwrap_or((self.clock)() as f64 + 30_000.0),
        })
    }

    /// The unsigned swap for a quote. The router dry-runs it for `owner` before answering.
    /// `sponsored`: Rare pays the fee, so the kind is used as it comes (no gas-coin fix-up: a
    /// sponsored transaction must never touch the gas coin, which belongs to the sponsor).
    pub fn build(&self, q: &SwapQuote, owner: &str, slippage_bps: u32, sponsored: bool) -> Result<SwapBuild> {
        let b = self.keyed("/swap/build", &json!({ "id": q.id, "owner": owner, "slippageBps": slippage_bps, "mode": "transaction" }))?;
        let Some(kind) = b.get("kind").and_then(Value::as_str).filter(|k| !k.is_empty()) else {
            return Err(TradeApiError::new(502, "invalid_build", "Could not build this swap."));
        };
        let sim = field(&b, "simulation").cloned().unwrap_or_else(|| json!({}));
        if sim.get("status").and_then(Value::as_str) == Some("failed") {
            return Err(TradeApiError::new(422, "simulation_failed", "This swap would fail right now. Get a new quote."));
        }
        let gas = field(&b, "gasEstimateMist").or_else(|| field(&sim, "gasEstimateMist"));
        Ok(SwapBuild {
            tx: swap_transaction(kind, owner, !sponsored),
            min_out: b.get("minOut").and_then(Value::as_str).and_then(digits),
            gas_estimate_mist: gas.map(js_text).as_deref().and_then(digits),
            expires_at: field(&b, "expiresAt").map(value_number).unwrap_or(q.expires_at),
        })
    }

    /// [`Swap::build`] at the default slippage.
    pub fn build_default(&self, q: &SwapQuote, owner: &str, sponsored: bool) -> Result<SwapBuild> {
        self.build(q, owner, SLIPPAGE_BPS, sponsored)
    }
}

/// A transaction kind from the router with the trader as sender, ready for the wallet.
pub fn swap_transaction(kind: &str, sender: &str, gas_from_spent_sui: bool) -> UnsignedTx {
    UnsignedTx::Kind { kind: kind.to_string(), sender: sender.to_string(), gas_from_spent_sui }
}

/// The exchanges in a router answer: its own hops, or the aggregator's providers.
pub fn route_venues(route: &Value) -> Vec<String> {
    let mut names: Vec<&Value> = Vec::new();
    for p in route.get("paths").and_then(Value::as_array).into_iter().flatten() {
        for h in p.get("hops").and_then(Value::as_array).into_iter().flatten() {
            names.push(h.get("venue").unwrap_or(&Value::Null));
        }
    }
    names.extend(route.get("providers").and_then(Value::as_array).into_iter().flatten());
    let mut out: Vec<String> = Vec::new();
    for n in names.into_iter().filter_map(Value::as_str).filter(|s| !s.is_empty()) {
        let l = n.to_lowercase();
        if !out.contains(&l) {
            out.push(l);
        }
    }
    out
}

/// Exchange names for the route line.
fn venue(k: &str) -> Option<&'static str> {
    Some(match k {
        "cetus" => "Cetus",
        "aftermath" => "Aftermath",
        "bluefin" => "Bluefin",
        "turbos" => "Turbos",
        "deepbook" | "deepbookv3" => "DeepBook",
        "flowx" | "flowxv3" => "FlowX",
        "kriya" | "kriyav3" => "Kriya",
        "momentum" => "Momentum",
        "haedal" => "Haedal",
        "obric" => "Obric",
        "steamm" => "Steamm",
        "magma" => "Magma",
        "bluemove" => "BlueMove",
        _ => return None,
    })
}

pub fn venue_name(v: &str) -> String {
    let k = v.to_lowercase();
    let mut base = match k.find(['_', '-']) {
        Some(i) => k[..i].to_string(),
        None => k.clone(),
    };
    // A trailing version ("v3") is not part of the name.
    let digits = base.bytes().rev().take_while(u8::is_ascii_digit).count();
    if digits > 0 && base.len() > digits && base.as_bytes()[base.len() - digits - 1] == b'v' {
        base.truncate(base.len() - digits - 1);
    }
    if venue(&k).is_none() && venue(&base).is_none() && base.len() > 4 {
        let (head, tail) = base.split_at(base.len() - 4);
        if tail == "dlmm" || tail == "clmm" {
            if let Some(name) = venue(head) {
                return format!("{name} {}", tail.to_uppercase());
            }
        }
    }
    if let Some(n) = venue(&k).or_else(|| venue(&base)) {
        return n.to_string();
    }
    let mut c = base.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    //! swap.test.ts (the signature check stays with rare-wallet's identity; here the fake identity
    //! records what it was asked to sign).
    use super::*;
    use crate::amount::{SUI_TYPE, USDC_TYPE};
    use rare_market::net::MockNet;

    const API: &str = "https://rare.market/v1/trade";

    fn owner() -> String {
        format!("0x{}", "a1".repeat(32))
    }

    struct FakeId {
        signed: Arc<Mutex<Vec<String>>>,
    }
    impl Identity for FakeId {
        fn address(&self) -> String {
            format!("0x{}", "1d".repeat(32))
        }
        fn sign_personal_message(&self, m: &[u8]) -> Result<String> {
            let m = String::from_utf8_lossy(m).to_string();
            self.signed.lock().unwrap().push(m.clone());
            Ok(format!("sig({m})"))
        }
    }

    fn swap(net: &MockNet, key: Option<&str>) -> (Swap, Arc<MemKeys>, Arc<Mutex<Vec<String>>>) {
        let keys = Arc::new(MemKeys::with(key));
        let signed = Arc::new(Mutex::new(Vec::new()));
        let s2 = signed.clone();
        let id: IdentityMaker = Arc::new(move || Ok(Box::new(FakeId { signed: s2.clone() }) as Box<dyn Identity>));
        let mut s = Swap::new(Arc::new(net.clone()), keys.clone(), id);
        s.clock = || 1_000;
        (s, keys, signed)
    }

    fn ok(data: Value) -> String {
        json!({ "version": "1", "data": data }).to_string()
    }

    fn body(c: &rare_market::net::Call) -> Value {
        serde_json::from_str(c.body.as_deref().unwrap_or("null")).unwrap()
    }

    fn auth(c: &rare_market::net::Call) -> Option<String> {
        c.headers.iter().find(|(k, _)| k == "authorization").map(|(_, v)| v.trim_start_matches("Bearer ").to_string())
    }

    #[test]
    fn signs_the_apis_challenge_with_the_device_identity_and_keeps_the_key() {
        let n = MockNet::new();
        let (s, keys, signed) = swap(&n, None);
        let addr = format!("0x{}", "1d".repeat(32));
        n.on_call(Method::Post, &format!("{API}/auth/challenge"), 200, ok(json!({ "challengeId": "c-1", "message": format!("Rare Launcher swap key for {addr}") })));
        n.on_call(Method::Post, &format!("{API}/auth/keys"), 200, ok(json!({ "key": "yk_test" })));
        assert_eq!(s.api_key().unwrap(), "yk_test");
        assert_eq!(keys.get().as_deref(), Some("yk_test"));
        // The signature is over the exact challenge text, for the identity's address.
        let calls = n.calls();
        assert_eq!(body(&calls[0]), json!({ "address": addr }));
        let b = body(&calls[1]);
        assert_eq!(signed.lock().unwrap().as_slice(), [format!("Rare Launcher swap key for {addr}")]);
        assert_eq!(b["signature"], format!("sig(Rare Launcher swap key for {addr})"));
        assert_eq!(b["address"], addr);
        assert_eq!(b["challengeId"], "c-1");
        assert_eq!(b["scopes"], json!(["quote", "build"]));
        // Second call uses the stored key.
        assert_eq!(s.api_key().unwrap(), "yk_test");
        assert_eq!(n.calls().len(), 2);
    }

    #[test]
    fn surfaces_the_apis_error_message() {
        let n = MockNet::new();
        n.on_call(Method::Post, &format!("{API}/auth/"), 429, json!({ "error": { "code": "challenge_limit", "message": "Wait before requesting another challenge." } }).to_string());
        let (s, _, _) = swap(&n, None);
        assert!(s.api_key().unwrap_err().message.contains("Too many requests"));
    }

    #[test]
    fn asks_for_the_traders_wallet_with_long_coin_types_and_maps_the_answer() {
        let n = MockNet::new();
        n.on_call(Method::Post, &format!("{API}/swap/quote"), 200, ok(json!({ "id": "mvpva0a1-lw", "amountOut": "90459", "feeBps": 20, "hops": 2, "route": { "providers": ["AFTERMATH", "FLOWXV3"] }, "expiresAt": 123 })));
        let (s, _, _) = swap(&n, Some("yk"));
        let q = s.quote(&QuoteParams { coin_in: "0x2::sui::SUI".into(), coin_out: USDC_TYPE.into(), amount_in: 100_000_000, owner: Some(owner()) }).unwrap();
        let c = &n.calls()[0];
        assert_eq!(c.url, format!("{API}/swap/quote"));
        assert_eq!(body(c), json!({ "coinIn": SUI_TYPE, "coinOut": USDC_TYPE, "amountIn": "100000000", "owner": owner() }));
        assert_eq!(auth(c).as_deref(), Some("yk"));
        assert_eq!(
            q,
            SwapQuote { id: "mvpva0a1-lw".into(), coin_in: SUI_TYPE.into(), coin_out: USDC_TYPE.into(), amount_in: 100_000_000, amount_out: 90_459, fee_bps: 20.0, hops: 2, venues: vec!["aftermath".into(), "flowxv3".into()], expires_at: 123.0 }
        );
    }

    #[test]
    fn replaces_a_key_the_api_no_longer_knows_once() {
        let n = MockNet::new();
        n.on_call(Method::Post, &format!("{API}/swap/quote"), 401, json!({ "error": { "message": "bad key" } }).to_string());
        n.on_call(Method::Post, &format!("{API}/swap/quote"), 200, ok(json!({ "id": "q1", "amountOut": "5" })));
        n.on_call(Method::Post, &format!("{API}/auth/challenge"), 200, ok(json!({ "challengeId": "c", "message": "m" })));
        n.on_call(Method::Post, &format!("{API}/auth/keys"), 200, ok(json!({ "key": "new" })));
        let (s, keys, _) = swap(&n, Some("old"));
        let q = s.quote(&QuoteParams { coin_in: SUI_TYPE.into(), coin_out: USDC_TYPE.into(), amount_in: 1, owner: Some(owner()) }).unwrap();
        assert_eq!(q.amount_out, 5);
        assert_eq!(q.expires_at, 31_000.0);
        assert_eq!(keys.get().as_deref(), Some("new"));
        assert_eq!(auth(n.calls().last().unwrap()).as_deref(), Some("new"));
    }

    #[test]
    fn refuses_empty_amounts_and_empty_routes() {
        let n = MockNet::new();
        n.on_call(Method::Post, &format!("{API}/swap/quote"), 200, ok(json!({ "id": "q", "amountOut": "0" })));
        let (s, _, _) = swap(&n, Some("k"));
        let p = |a| QuoteParams { coin_in: SUI_TYPE.into(), coin_out: USDC_TYPE.into(), amount_in: a, owner: Some(owner()) };
        assert!(s.quote(&p(0)).unwrap_err().message.contains("amount"));
        assert!(s.quote(&p(1)).unwrap_err().message.contains("No route"));
    }

    fn quote() -> SwapQuote {
        SwapQuote { id: "q-1".into(), coin_in: SUI_TYPE.into(), coin_out: USDC_TYPE.into(), amount_in: 1, amount_out: 2, fee_bps: 20.0, hops: 1, venues: vec![], expires_at: 0.0 }
    }

    /// A router kind: base64 the crate carries through untouched (rare-sui decodes it).
    const KIND: &str = "AAEAAQIDBAUGBwgJ";

    #[test]
    fn turns_the_routers_transaction_kind_into_the_traders_unsigned_transaction() {
        let n = MockNet::new();
        n.on_call(Method::Post, &format!("{API}/swap/build"), 200, ok(json!({ "ok": true, "kind": KIND, "minOut": "89533", "expiresAt": 99, "simulation": { "status": "succeeded", "gasEstimateMist": "4723677" } })));
        let (s, _, _) = swap(&n, Some("k"));
        let b = s.build(&quote(), &owner(), 100, false).unwrap();
        assert_eq!(body(&n.calls()[0]), json!({ "id": "q-1", "owner": owner(), "slippageBps": 100, "mode": "transaction" }));
        assert_eq!(b.tx, UnsignedTx::Kind { kind: KIND.into(), sender: owner(), gas_from_spent_sui: true });
        assert_eq!(b.tx.sender(), Some(owner().as_str()));
        assert_eq!(b.min_out, Some(89_533));
        assert_eq!(b.gas_estimate_mist, Some(4_723_677));
        assert_eq!(b.expires_at, 99.0);
        // Sponsored: the kind as it comes, the gas coin never touched.
        let b = s.build(&quote(), &owner(), 100, true).unwrap();
        assert_eq!(b.tx, UnsignedTx::Kind { kind: KIND.into(), sender: owner(), gas_from_spent_sui: false });
    }

    #[test]
    fn stops_when_the_routers_dry_run_failed() {
        let n = MockNet::new();
        n.on_call(Method::Post, &format!("{API}/swap/build"), 200, ok(json!({ "kind": KIND, "simulation": { "status": "failed" } })));
        let (s, _, _) = swap(&n, Some("k"));
        assert!(s.build(&quote(), &owner(), 100, false).unwrap_err().message.contains("would fail"));
    }

    #[test]
    fn explains_a_declined_build() {
        let n = MockNet::new();
        n.on_call(Method::Post, &format!("{API}/swap/build"), 422, json!({ "error": { "code": "build_declined", "message": "The swap could not pass its dry run. Request a new quote." } }).to_string());
        let (s, _, _) = swap(&n, Some("k"));
        assert!(s.build(&quote(), &owner(), 100, false).unwrap_err().message.contains("dry run"));
    }

    #[test]
    fn reads_venues_from_both_route_shapes() {
        assert_eq!(route_venues(&json!({ "paths": [{ "hops": [{ "venue": "cetus" }, { "venue": "turbos" }] }, { "hops": [{ "venue": "cetus" }] }] })), vec!["cetus", "turbos"]);
        assert!(route_venues(&Value::Null).is_empty());
    }

    #[test]
    fn sets_the_sender_on_a_bare_kind() {
        let tx = swap_transaction(KIND, &owner(), true);
        assert_eq!(tx.sender(), Some(owner().as_str()));
    }

    #[test]
    fn names_exchanges_plainly() {
        assert_eq!(venue_name("cetus_tide"), "Cetus");
        assert_eq!(venue_name("FLOWXV3"), "FlowX");
        assert_eq!(venue_name("deepbookv3"), "DeepBook");
        assert_eq!(venue_name("newdex"), "Newdex");
        assert_eq!(venue_name("cetusdlmm"), "Cetus DLMM");
    }

    #[test]
    fn route_and_fee_lines() {
        let mut q = quote();
        q.hops = 2;
        q.venues = vec!["cetus".into(), "cetus_tide".into(), "flowxv3".into(), "turbos".into(), "kriya".into()];
        assert_eq!(q.route_line(), "2 hops via Cetus, FlowX, Turbos");
        q.hops = 1;
        q.venues.clear();
        assert_eq!(q.route_line(), "1 hop");
        assert_eq!(q.fee_line(), "0.20%");
        q.expires_at = 10_000.0;
        assert!(q.reusable(1_999) && !q.reusable(2_000));
    }
}
