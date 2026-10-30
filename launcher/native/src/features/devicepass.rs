//! Rare Device Pass (Settings > About): one pass per genuine SuiPlay, kept by the player's Rare
//! account. Claim = attest this device with rare-attest (TPM, rare.market's attestor), build
//! `device_pass::claim` with the attestor's voucher, and sign it in Rare Wallet's review sheet
//! (Rare covers the fee when the sponsor offers device claims). After that, About shows the pass.
//!
//! The claim runs on the network Rare Wallet signs on (mainnet). It shows only once the package is
//! published there (rare_sui::network::device_pass_deployment); the testnet deployment is
//! exercised by the attestor e2e. Fixture runs read RR_DEVICE_PASS (none|idle|checking|signing|
//! error|held|moved) instead of the chain.

use rare_sui::grpc::{ChainRead, Include};
use rare_sui::network::{device_pass_deployment, TrophyConfig};
use rare_sui::Network;
use serde_json::Value;
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    /// rare-attest is talking to the TPM and the attestor.
    Checking,
    /// Rare Wallet's review sheet is up.
    Signing,
    Error(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PassInfo {
    pub id: String,
    pub number: u64,
    pub model: String,
    pub first_seen_ms: u64,
}

/// What About draws.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// The panel shows at all (published on the wallet's network).
    pub shown: bool,
    pub phase: Phase,
    /// This account's current pass for this device.
    pub pass: Option<PassInfo>,
    /// The account holds a pass whose device has moved on (a rebind elsewhere).
    pub stale: bool,
    /// A signing account exists.
    pub can_sign: bool,
}

#[derive(Default)]
struct Store {
    ver: u64,
    phase: Option<Phase>,
    pass: Option<PassInfo>,
    stale: bool,
    loaded_for: Option<String>,
    loading: bool,
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

/// Moves whenever the view changes (Settings rebuilds About).
pub fn version() -> u64 {
    with(|s| s.ver)
}

/// The network Rare Wallet signs on.
const NETWORK: Network = Network::Mainnet;

fn deployment() -> Option<TrophyConfig> {
    device_pass_deployment(NETWORK)
}

fn fixture() -> Option<View> {
    if std::env::var("RR_FIXTURE").is_err() {
        return None;
    }
    let held = PassInfo { id: "0x5".into(), number: 1287, model: "SuiPlay".into(), first_seen_ms: 1_793_203_200_000 };
    let base = View { shown: true, phase: Phase::Idle, pass: None, stale: false, can_sign: true };
    Some(match std::env::var("RR_DEVICE_PASS").as_deref() {
        Ok("idle") => base,
        Ok("checking") => View { phase: Phase::Checking, ..base },
        Ok("signing") => View { phase: Phase::Signing, ..base },
        Ok("error") => View { phase: Phase::Error("This device's firmware isn't a known SuiPlay firmware.".into()), ..base },
        Ok("nosigner") => View { can_sign: false, ..base },
        Ok("held") => View { pass: Some(held), ..base },
        Ok("moved") => View { stale: true, ..base },
        _ => View { shown: false, ..base },
    })
}

pub fn view() -> View {
    if let Some(v) = fixture() {
        return v;
    }
    let shown = deployment().is_some();
    let can_sign = crate::sign::account().is_some_and(|a| a.signer);
    with(|s| View { shown, phase: s.phase.clone().unwrap_or(Phase::Idle), pass: s.pass.clone(), stale: s.stale, can_sign })
}

fn bg(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new().name("rare-device-pass".into()).spawn(f).ok();
}

fn hex_bytes(v: &Value) -> Option<Vec<u8>> {
    use base64::Engine;
    match v {
        Value::Array(a) => a.iter().map(|x| x.as_u64().and_then(|n| u8::try_from(n).ok())).collect(),
        Value::String(s) => {
            let h = s.trim_start_matches("0x");
            if h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()) {
                (0..32).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).ok()).collect()
            } else {
                base64::engine::general_purpose::STANDARD.decode(s).ok()
            }
        }
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

/// The account's current pass (newest first), and whether it only holds stale ones.
async fn read_pass(sui: &dyn ChainRead, cfg: &TrophyConfig, owner: &str) -> Result<(Option<PassInfo>, bool), String> {
    let ty = format!("{}::device_pass::DevicePass", cfg.package);
    // JSON, not just Display: number, first_seen_ms and device come from the fields.
    let mut list = sui.owned_objects(owner, Some(&ty), 20, Include::JSON).await.map_err(|e| e.to_string())?;
    if list.is_empty() {
        return Ok((None, false));
    }
    // The registry's devices table, to tell the current pass from one whose device moved on.
    let reg = sui.objects(&[cfg.registry.clone()], Include::JSON).await.ok().and_then(|l| l.into_iter().next()).and_then(|r| r.ok()).and_then(|o| o.json);
    let table = reg.as_ref().and_then(|j| j.get("devices")).and_then(|d| d.get("id")).and_then(|v| v.as_str()).map(String::from);
    list.sort_by_key(|o| std::cmp::Reverse(o.json.as_ref().map(|j| num(j.get("claimed_ms"))).unwrap_or(0)));
    let mut stale = false;
    for o in &list {
        let Some(j) = o.json.as_ref() else { continue };
        let info = PassInfo { id: o.object_id.clone(), number: num(j.get("number")), model: j.get("model").and_then(|v| v.as_str()).unwrap_or("SuiPlay").to_string(), first_seen_ms: num(j.get("first_seen_ms")) };
        let current = match (table.as_deref(), j.get("device").and_then(hex_bytes)) {
            (Some(t), Some(dev)) => match rare_sui::kiosk::dynamic_field_id(t, "vector<u8>", &rare_sui::ptb::bcs::bytes(&dev)) {
                Ok(fid) => {
                    let rec = sui.objects(&[fid], Include::JSON).await.ok().and_then(|l| l.into_iter().next()).and_then(|r| r.ok()).and_then(|o| o.json);
                    let pass = rec.as_ref().and_then(|r| r.get("value")).and_then(|v| v.get("pass")).and_then(|p| p.as_str()).map(rare_sui::ptb::norm_addr);
                    pass.map_or(true, |p| p == rare_sui::ptb::norm_addr(&o.object_id))
                }
                Err(_) => true,
            },
            _ => true,
        };
        if current {
            return Ok((Some(info), false));
        }
        stale = true;
    }
    Ok((None, stale))
}

/// Reads the signing account's pass (once per account, or again with `force`).
pub fn load(force: bool) {
    if fixture().is_some() {
        return;
    }
    let Some(cfg) = deployment() else { return };
    let Some(acct) = crate::sign::account().filter(|a| a.signer) else { return };
    let go = with(|s| {
        if s.loading || (!force && s.loaded_for.as_deref() == Some(acct.address.as_str())) {
            return false;
        }
        s.loading = true;
        true
    });
    if !go {
        return;
    }
    bg(move || {
        let live = crate::screens::wallet::live::sui();
        let r = match live {
            Some(sui) => crate::screens::wallet::live::rt().block_on(read_pass(sui, &cfg, &acct.address)),
            None => Err("No network".into()),
        };
        with(|s| {
            s.loading = false;
            s.loaded_for = Some(acct.address.clone());
            if let Ok((pass, stale)) = r {
                s.pass = pass;
                s.stale = stale;
            }
            changed(s);
        });
    });
}

/// `device_pass::claim(registry, device, model, first_seen_ms, seq, expires_ms, signature, clock)`.
pub fn build_claim_tx(v: &rare_attest::Voucher, sender: &str) -> Result<rare_sui::ptb::Tx, String> {
    use rare_sui::ptb::{bcs, Tx};
    let dev = hex_bytes(&Value::String(v.device.clone())).filter(|d| d.len() == 32).ok_or("The attestor sent a bad device id")?;
    let sig: Vec<u8> = (0..v.signature.len() / 2).map(|i| u8::from_str_radix(v.signature.get(2 * i..2 * i + 2).unwrap_or("zz"), 16)).collect::<Result<_, _>>().map_err(|_| "The attestor sent a bad signature")?;
    if sig.len() != 64 {
        return Err("The attestor sent a bad signature".into());
    }
    let seq: u64 = v.seq.parse().map_err(|_| "The attestor sent a bad sequence")?;
    let mut tx = Tx::new();
    tx.set_sender(sender);
    let reg = tx.object(&v.registry);
    let d = tx.pure(bcs::bytes(&dev));
    let m = tx.pure_string(&v.model);
    let f = tx.pure_u64(v.first_seen_ms);
    let q = tx.pure_u64(seq);
    let e = tx.pure_u64(v.expires_ms);
    let s = tx.pure(bcs::bytes(&sig));
    let clock = tx.object("0x6");
    tx.move_call(&format!("{}::device_pass::claim", v.package), &[], &[reg, d, m, f, q, e, s, clock]);
    Ok(tx)
}

fn set_phase(p: Phase) {
    with(|s| {
        s.phase = Some(p);
        changed(s);
    });
}

/// Claim your Device Pass: attest, then Rare Wallet's review sheet. Runs on a worker thread.
pub fn claim() {
    if fixture().is_some() {
        return;
    }
    let busy = with(|s| matches!(s.phase, Some(Phase::Checking) | Some(Phase::Signing)));
    if busy {
        return;
    }
    let Some(cfg) = deployment() else { return };
    let Some(acct) = crate::sign::account().filter(|a| a.signer) else {
        set_phase(Phase::Error(crate::sign::NO_SIGNER.into()));
        return;
    };
    set_phase(Phase::Checking);
    bg(move || {
        let r = claim_blocking(&cfg, &acct.address);
        match r {
            Ok(()) => {
                set_phase(Phase::Idle);
                super::bus::note("Rare Pass claimed", false);
                load(true);
            }
            Err(e) if e == rare_market::inventory::notice::SIGN_REJECTED => set_phase(Phase::Idle),
            Err(e) => set_phase(Phase::Error(e)),
        }
    });
}

fn claim_blocking(cfg: &TrophyConfig, address: &str) -> Result<(), String> {
    let ep = rare_attest::Endpoint { base: rare_sui::network::MARKET.into(), testnet: cfg.network == Network::Testnet };
    let a = rare_attest::attest(&ep, &rare_attest::Request { address: address.to_string(), aud: None }).map_err(|e| e.message())?;
    let (v, title) = match a.pass {
        rare_attest::PassOffer::New(v) => (v, "Claim Rare Pass"),
        rare_attest::PassOffer::Rebind(v) => (v, "Move this device's Rare Pass here"),
        rare_attest::PassOffer::Held { .. } => return Ok(()),
        rare_attest::PassOffer::Unavailable { reason } => {
            return Err(match reason.as_str() {
                "level" => "This device isn't running a verified Rare OS build.".into(),
                "paused" => "Rare Pass claims are paused right now.".into(),
                _ => "Rare Pass isn't open on this network yet.".into(),
            })
        }
    };
    if rare_sui::ptb::norm_addr(&v.registry) != rare_sui::ptb::norm_addr(&cfg.registry) || rare_sui::ptb::norm_addr(&v.package) != rare_sui::ptb::norm_addr(&cfg.package) {
        return Err("The attestor answered for a different Rare Pass deployment.".into());
    }
    let tx = build_claim_tx(&v, address)?;
    set_phase(Phase::Signing);
    crate::sign::sign_device_pass(tx, address, title).map(|_| ())
}

/// "2026-10-28" (UTC) for unix ms.
pub fn date(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let z = secs.div_euclid(86400) + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_tx_calls_the_package_with_the_voucher() {
        let pkg = format!("0x{}", "23".repeat(32));
        let v = rare_attest::Voucher { network: "testnet".into(), package: pkg.clone(), registry: "0x068f".into(), device: "ab".repeat(32), model: "SuiPlay".into(), first_seen_ms: 5, seq: "1".into(), expires_ms: 9, signature: "cd".repeat(64) };
        let tx = build_claim_tx(&v, "0xbaa2").unwrap();
        assert_eq!(tx.targets(), vec![format!("{pkg}::device_pass::claim")]);
        let bad = rare_attest::Voucher { signature: "cd".repeat(10), ..v.clone() };
        assert!(build_claim_tx(&bad, "0xbaa2").is_err());
        let bad = rare_attest::Voucher { device: "ab".into(), ..v };
        assert!(build_claim_tx(&bad, "0xbaa2").is_err());
    }

    /// Live testnet read (cargo test -- --ignored): the two e2e accounts of 2026-10-28 have moved the
    /// owner's SuiPlay pass between them by rebinds, so exactly one holds it as current and the other
    /// holds a stale one.
    #[test]
    #[ignore]
    fn testnet_pass_reads() {
        let cfg = device_pass_deployment(Network::Testnet).unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let sui = {
            let _g = rt.enter();
            rare_sui::Sui::new(Network::Testnet, rare_sui::network::DEFAULT_RPC_TESTNET).unwrap()
        };
        let read = |a: &str| rt.block_on(read_pass(&sui, &cfg, a)).unwrap();
        let one = read("0xbaa2446cf617539546818b36de4ba53bd57c494716d5c8cf6eed3d0b92229206");
        let two = read("0x9716aa232df1b1fc81b664965674dcb20732ae8f17dad3c6704e00b5fb1dd447");
        let (cur, other) = if one.0.is_some() { (one, two) } else { (two, one) };
        let p = cur.0.unwrap();
        assert!(!cur.1);
        assert_eq!((p.number, p.model.as_str(), date(p.first_seen_ms).as_str()), (1, "SuiPlay", "2026-10-28"));
        assert_eq!(other, (None, true));
    }

    #[test]
    fn dates_are_utc_days() {
        assert_eq!(date(1_793_203_200_000), "2026-10-28");
        assert_eq!(date(0), "1970-01-01");
    }
}
