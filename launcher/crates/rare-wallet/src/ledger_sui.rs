//! The Sui Ledger app protocol over raw APDUs: what @mysten/ledgerjs-hw-app-sui 0.9.1 does
//! (BIP32 payloads, the chunked block protocol, getPublicKey, signTransaction with clear-signing
//! objects, getVersion), plus Rare Wallet's own layer from src/wallet/ledger/{sign,errors,device,
//! index}.ts: the Sui app check, key checks before signing, signature serialization and
//! verification, plain error codes and messages, and the session that waits while the device is
//! locked or the Sui app is closed.
//!
//! Transport-agnostic: anything that carries one APDU and returns the response with its status
//! word (`Apdu`) works, the Rust USB/BLE transport (`ledger::LedgerState`, feature `hw`) or the
//! test device in `fake`.

use crate::keys::{address_of, personal_message_digest, tx_digest, FLAG_ED25519};
use crate::WalletError;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use sui_sdk_types::{Input, Transaction, TransactionKind};

// ---------------------------------------------------------------- errors

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerCode {
    NoDevice,
    Locked,
    AppClosed,
    WrongApp,
    Rejected,
    BlindSigning,
    Disconnected,
    Busy,
    Mismatch,
    Timeout,
    Cancelled,
    UpdateApp,
    Udev,
    BleOff,
    BlePairing,
    Unknown,
}

impl LedgerCode {
    pub const ALL: [LedgerCode; 16] = [
        LedgerCode::NoDevice,
        LedgerCode::Locked,
        LedgerCode::AppClosed,
        LedgerCode::WrongApp,
        LedgerCode::Rejected,
        LedgerCode::BlindSigning,
        LedgerCode::Disconnected,
        LedgerCode::Busy,
        LedgerCode::Mismatch,
        LedgerCode::Timeout,
        LedgerCode::Cancelled,
        LedgerCode::UpdateApp,
        LedgerCode::Udev,
        LedgerCode::BleOff,
        LedgerCode::BlePairing,
        LedgerCode::Unknown,
    ];

    /// The TS code string.
    pub fn as_str(self) -> &'static str {
        match self {
            LedgerCode::NoDevice => "no-device",
            LedgerCode::Locked => "locked",
            LedgerCode::AppClosed => "app-closed",
            LedgerCode::WrongApp => "wrong-app",
            LedgerCode::Rejected => "rejected",
            LedgerCode::BlindSigning => "blind-signing",
            LedgerCode::Disconnected => "disconnected",
            LedgerCode::Busy => "busy",
            LedgerCode::Mismatch => "mismatch",
            LedgerCode::Timeout => "timeout",
            LedgerCode::Cancelled => "cancelled",
            LedgerCode::UpdateApp => "update-app",
            LedgerCode::Udev => "udev",
            LedgerCode::BleOff => "ble-off",
            LedgerCode::BlePairing => "ble-pairing",
            LedgerCode::Unknown => "unknown",
        }
    }

    /// LEDGER_MESSAGES in errors.ts.
    pub fn message(self) -> &'static str {
        match self {
            LedgerCode::NoDevice => "No Ledger found. Connect it with USB-C or turn on its Bluetooth, unlock it and open the Sui app.",
            LedgerCode::Locked => "Your Ledger is locked. Unlock it with your PIN and try again.",
            LedgerCode::AppClosed => "Open the Sui app on your Ledger and try again.",
            LedgerCode::WrongApp => "Another app is open on your Ledger. Close it, open the Sui app and try again.",
            LedgerCode::Rejected => "Rejected on your Ledger.",
            LedgerCode::BlindSigning => "Your Ledger can't show this transaction in full, so it needs blind signing. On your Ledger, open the Sui app, go to Settings and turn on Blind signing. Then try again.",
            LedgerCode::Disconnected => "Your Ledger disconnected. Reconnect it and try again.",
            LedgerCode::Busy => "Your Ledger is busy with another request. Finish or reject it on the device, close other apps using it, then try again.",
            LedgerCode::Mismatch => "This Ledger does not hold this account. Connect the Ledger you added it with.",
            LedgerCode::Timeout => "Your Ledger did not answer in time. Try again.",
            LedgerCode::Cancelled => "Cancelled. If your Ledger still shows the request, reject it there.",
            LedgerCode::UpdateApp => "Update the Sui app in Ledger Live and try again.",
            LedgerCode::Udev => "Rare OS needs permission to use your Ledger. Run this command once in a terminal, then reconnect your Ledger.",
            LedgerCode::BleOff => "Bluetooth is off. Turn it on and try again.",
            LedgerCode::BlePairing => "Pairing with your Ledger failed. Check that the code on your Ledger matches, approve it there and try again.",
            LedgerCode::Unknown => "Your Ledger did not respond. Reconnect it and try again.",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LedgerError {
    pub code: LedgerCode,
    pub message: String,
    /// status word from the device, when there was one
    pub status: Option<u16>,
    /// extra data from the transport; for "udev", `fix` is the one-line command to show
    pub data: Option<serde_json::Value>,
}

impl std::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LedgerError {}

impl LedgerError {
    pub fn new(code: LedgerCode) -> Self {
        LedgerError { code, message: code.message().into(), status: None, data: None }
    }
    pub fn with(code: LedgerCode, message: impl Into<String>) -> Self {
        LedgerError { code, message: message.into(), status: None, data: None }
    }
    /// The udev rule command for the "udev" error.
    pub fn fix(&self) -> Option<&str> {
        self.data.as_ref()?.get("fix")?.as_str()
    }

    /// A device status word.
    pub fn from_status(sw: u16) -> Self {
        const LOCKED: [u16; 3] = [0x5515, 0x6b0c, 0x6982];
        // 0x6985: CONDITIONS_OF_USE_NOT_SATISFIED (C apps), 0x6e04: UserCancelled (Rust SDK)
        const REJECTED: [u16; 2] = [0x6985, 0x6e04];
        // SyscallError::NotSupported: a transaction it can't clear sign while blind signing is off
        const BLIND: [u16; 1] = [0x6808];
        // CLA/INS not supported: the dashboard or another app is open
        const APP_CLOSED: [u16; 8] = [0x6e00, 0x6e01, 0x6d00, 0x6d02, 0x6511, 0x6a15, 0x6807, 0x6a83];
        let code = if LOCKED.contains(&sw) {
            LedgerCode::Locked
        } else if REJECTED.contains(&sw) {
            LedgerCode::Rejected
        } else if BLIND.contains(&sw) {
            LedgerCode::BlindSigning
        } else if APP_CLOSED.contains(&sw) {
            LedgerCode::AppClosed
        } else {
            return LedgerError {
                code: LedgerCode::Unknown,
                message: format!("Your Ledger returned an error (0x{sw:x}). Check the Sui app is open and up to date, then try again."),
                status: Some(sw),
                data: None,
            };
        };
        LedgerError { status: Some(sw), ..LedgerError::new(code) }
    }

    /// A transport error string ("code|message[|{json}]") from `ledger::LedgerState`.
    pub fn from_native(s: &str) -> Self {
        let e = WalletError::parse(s);
        let code = match e.code.as_str() {
            "udev" => LedgerCode::Udev,
            "ble-off" => LedgerCode::BleOff,
            "ble-pairing" => LedgerCode::BlePairing,
            "busy" => LedgerCode::Busy,
            "notfound" => LedgerCode::NoDevice,
            "disconnected" => LedgerCode::Disconnected,
            "timeout" => LedgerCode::Timeout,
            // the exchange was dropped because the device was closed (another conversation took over)
            "cancelled" => LedgerCode::Cancelled,
            _ => LedgerCode::Unknown,
        };
        LedgerError { data: e.data, ..LedgerError::new(code) }
    }
}

type R<T> = Result<T, LedgerError>;

// ---------------------------------------------------------------- APDUs

/// Carries one APDU; returns the response including its 2-byte status word.
pub trait Apdu {
    fn exchange(&mut self, apdu: &[u8]) -> R<Vec<u8>>;
}

/// @ledgerhq Transport.send: builds the APDU, strips a 0x9000 status word, maps any other.
pub fn send(t: &mut dyn Apdu, cla: u8, ins: u8, p1: u8, p2: u8, data: &[u8]) -> R<Vec<u8>> {
    if data.len() > 255 {
        return Err(LedgerError::with(LedgerCode::Unknown, "APDU data too long"));
    }
    let mut apdu = vec![cla, ins, p1, p2, data.len() as u8];
    apdu.extend_from_slice(data);
    let mut res = t.exchange(&apdu)?;
    if res.len() < 2 {
        return Err(LedgerError::new(LedgerCode::Unknown));
    }
    let sw = u16::from_be_bytes([res[res.len() - 2], res[res.len() - 1]]);
    if sw != 0x9000 {
        return Err(LedgerError::from_status(sw));
    }
    res.truncate(res.len() - 2);
    Ok(res)
}

/// "m/44'/784'/0'/0'/0'" -> count byte + u32le components (hardened with 0x80000000).
pub fn bip32_payload(path: &str) -> Vec<u8> {
    // splitPath in the TS client: components that don't parse as numbers are skipped
    let parts: Vec<u32> = path
        .split('/')
        .filter_map(|c| {
            let digits: String = c.chars().take_while(|ch| ch.is_ascii_digit()).collect();
            let n: u32 = digits.parse().ok()?;
            Some(if c.len() > 1 && c.ends_with('\'') { n.wrapping_add(0x8000_0000) } else { n })
        })
        .collect();
    let mut out = vec![parts.len() as u8];
    for p in parts {
        out.extend_from_slice(&p.to_le_bytes());
    }
    out
}

const CHUNK: usize = 180;

/// The Sui app's block protocol: each payload becomes a hash-linked list of 180-byte chunks; the
/// app asks for blocks by hash (GET_CHUNK), may store blocks (PUT_CHUNK), and streams the result.
pub fn send_chunks(t: &mut dyn Apdu, cla: u8, ins: u8, p1: u8, p2: u8, payloads: &[&[u8]]) -> R<Vec<u8>> {
    let mut data: HashMap<[u8; 32], Vec<u8>> = HashMap::new();
    let mut params = vec![0u8]; // START
    for p in payloads {
        let mut last = [0u8; 32];
        let chunks: Vec<&[u8]> = if p.is_empty() { vec![] } else { p.chunks(CHUNK).collect() };
        for c in chunks.iter().rev() {
            let mut linked = last.to_vec();
            linked.extend_from_slice(c);
            last = Sha256::digest(&linked).into();
            data.insert(last, linked);
        }
        params.extend_from_slice(&last);
    }
    let mut payload = params;
    let mut result = Vec::new();
    loop {
        let rv = send(t, cla, ins, p1, p2, &payload)?;
        let Some((&op, body)) = rv.split_first() else { return Err(LedgerError::new(LedgerCode::Unknown)) };
        match op {
            // RESULT_ACCUMULATING, RESULT_FINAL
            0 | 1 => {
                result.extend_from_slice(body);
                if op == 1 {
                    return Ok(result);
                }
                payload = vec![4]; // RESULT_ACCUMULATING_RESPONSE
            }
            // GET_CHUNK
            2 => {
                let key: Option<[u8; 32]> = body.try_into().ok();
                payload = match key.and_then(|k| data.get(&k)) {
                    Some(block) => {
                        let mut p = vec![1]; // GET_CHUNK_RESPONSE_SUCCESS
                        p.extend_from_slice(block);
                        p
                    }
                    None => vec![2], // GET_CHUNK_RESPONSE_FAILURE
                };
            }
            // PUT_CHUNK
            3 => {
                data.insert(Sha256::digest(body).into(), body.to_vec());
                payload = vec![3]; // PUT_CHUNK_RESPONSE
            }
            _ => return Err(LedgerError::with(LedgerCode::Unknown, "Unknown instruction returned from ledger")),
        }
    }
}

/// getPublicKey: (public key, address bytes when the app reports one).
pub fn get_public_key(t: &mut dyn Apdu, path: &str, display: bool) -> R<(Vec<u8>, Option<Vec<u8>>)> {
    let r = send_chunks(t, 0x00, if display { 0x01 } else { 0x02 }, 0, 0, &[&bip32_payload(path)])?;
    let n = *r.first().ok_or_else(|| LedgerError::new(LedgerCode::Unknown))? as usize;
    let pk = r.get(1..1 + n).ok_or_else(|| LedgerError::new(LedgerCode::Unknown))?.to_vec();
    let addr = if r.len() > n + 2 {
        let m = r[n + 1] as usize;
        r.get(n + 2..n + 2 + m).map(<[u8]>::to_vec)
    } else {
        None
    };
    Ok((pk, addr))
}

/// getVersion: (major, minor, patch).
pub fn get_version(t: &mut dyn Apdu) -> R<(u8, u8, u8)> {
    let r = send_chunks(t, 0x00, 0x00, 0, 0, &[&[0u8]])?;
    Ok((r.first().copied().unwrap_or(0), r.get(1).copied().unwrap_or(0), r.get(2).copied().unwrap_or(0)))
}

/// signTransaction: `payload` is the intent message the app hashes and signs.
pub fn sign_payload(t: &mut dyn Apdu, path: &str, payload: &[u8], objects: &[Vec<u8>]) -> R<Vec<u8>> {
    let mut txp = (payload.len() as u32).to_le_bytes().to_vec();
    txp.extend_from_slice(payload);
    let key = bip32_payload(path);
    let (major, _, _) = get_version(t)?;
    let mut list = Vec::new();
    if major > 0 && !objects.is_empty() {
        list.extend_from_slice(&(objects.len() as u32).to_le_bytes());
        for o in objects {
            list.extend_from_slice(&(o.len() as u32).to_le_bytes());
            list.extend_from_slice(o);
        }
    }
    let mut payloads: Vec<&[u8]> = vec![&txp, &key];
    if !list.is_empty() {
        payloads.push(&list);
    }
    send_chunks(t, 0x00, 0x03, 0, 0, &payloads)
}

// ---------------------------------------------------------------- Rare Wallet's layer (sign.ts)

/// intent (TransactionData, V0, Sui) || tx bytes. The app hashes them itself.
pub fn tx_signing_payload(tx_bytes: &[u8]) -> Vec<u8> {
    let mut v = vec![0, 0, 0];
    v.extend_from_slice(tx_bytes);
    v
}

/// intent (PersonalMessage) || BCS vector<u8>(message).
pub fn personal_message_signing_payload(message: &[u8]) -> Vec<u8> {
    let mut v = vec![3, 0, 0];
    v.extend_from_slice(&bcs::to_bytes(&message.to_vec()).expect("bytes serialize"));
    v
}

/// Device signature -> Sui serialized signature, base64 flag(0x00) || sig(64) || pubkey(32).
pub fn serialize_signature(signature: &[u8], public_key: &[u8]) -> R<String> {
    if signature.len() != 64 {
        return Err(LedgerError::with(LedgerCode::Unknown, "Your Ledger returned a malformed signature. Try again."));
    }
    let mut v = vec![FLAG_ED25519];
    v.extend_from_slice(signature);
    v.extend_from_slice(public_key);
    Ok(B64.encode(v))
}

/// A coin reservation ref: an address balance amount dressed as an object ref (its digest ends in
/// 20 bytes of 0xac), which the node turns into a coin; there is no object to read.
pub fn is_coin_reservation(r: &sui_sdk_types::ObjectReference) -> bool {
    r.digest().inner()[12..32] == [0xac; 20]
}

/// Object ids the Sui app wants as BCS for clear signing: gas coins + owned/immutable inputs,
/// once each, in that order. Coin reservations (address balance stand-ins with no object behind
/// them) are left out.
pub fn clear_signing_object_ids(tx_bytes: &[u8]) -> Vec<String> {
    let Ok(tx) = bcs::from_bytes::<Transaction>(tx_bytes) else { return vec![] };
    let real = |o: &&sui_sdk_types::ObjectReference| !is_coin_reservation(o);
    let mut out: Vec<String> = tx.gas_payment.objects.iter().filter(real).map(|o| o.object_id().to_string()).collect();
    if let TransactionKind::ProgrammableTransaction(p) = &tx.kind {
        for i in &p.inputs {
            if let Input::ImmutableOrOwned(r) = i {
                if !is_coin_reservation(r) {
                    out.push(r.object_id().to_string());
                }
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|id| seen.insert(id.clone()));
    out
}

/// GET_APP_AND_VERSION (CLA 0xb0, INS 0x01) response (status word stripped): (name, version).
pub fn parse_app_and_version(res: &[u8]) -> Option<(String, String)> {
    if res.len() < 3 || res[0] != 0x01 {
        return None;
    }
    let n = res[1] as usize;
    let name = String::from_utf8_lossy(res.get(2..2 + n)?).into_owned();
    let vl = res.get(2 + n).copied().unwrap_or(0) as usize;
    let version = String::from_utf8_lossy(res.get(3 + n..3 + n + vl).unwrap_or(&[])).into_owned();
    Some((name, version))
}

/// Err unless the open app is Sui. The dashboard reports itself as "BOLOS".
pub fn assert_sui_app(name: &str) -> R<()> {
    match name {
        "Sui" => Ok(()),
        "BOLOS" | "" => Err(LedgerError::new(LedgerCode::AppClosed)),
        other => Err(LedgerError::with(LedgerCode::WrongApp, format!("Your Ledger has the {other} app open. Close it, open the Sui app and try again."))),
    }
}

/// Which app is open; Err unless it is Sui. None when the device doesn't answer
/// GET_APP_AND_VERSION (older firmware: the Sui calls then report a closed app).
pub fn check_sui_app(t: &mut dyn Apdu) -> R<Option<(String, String)>> {
    let info = match send(t, 0xb0, 0x01, 0, 0, &[]) {
        Ok(res) => match parse_app_and_version(&res) {
            Some(i) => i,
            None => return Ok(None),
        },
        Err(e) if matches!(e.code, LedgerCode::Locked | LedgerCode::Disconnected | LedgerCode::Busy | LedgerCode::Udev | LedgerCode::NoDevice) => return Err(e),
        Err(_) => return Ok(None),
    };
    assert_sui_app(&info.0)?;
    Ok(Some(info))
}

/// Public key and derived address at `path`; refuses an app whose reported address disagrees.
pub fn read_account(t: &mut dyn Apdu, path: &str, display: bool) -> R<(Vec<u8>, String)> {
    let (pk, addr) = get_public_key(t, path, display)?;
    let derived = address_of(FLAG_ED25519, &pk).map_err(|_| LedgerError::new(LedgerCode::Unknown))?;
    if let Some(a) = addr.filter(|a| a.len() == 32) {
        if format!("0x{}", crate::keys::hex(&a)) != derived {
            return Err(LedgerError::with(LedgerCode::Unknown, "Your Ledger reported an address that does not match its key. Update the Sui app and try again."));
        }
    }
    Ok((pk, derived))
}

/// The device must hold `public_key` at `path` (checked without a prompt before every signature).
pub fn assert_key(t: &mut dyn Apdu, path: &str, public_key: &[u8]) -> R<()> {
    let (got, _) = get_public_key(t, path, false)?;
    if got != public_key {
        return Err(LedgerError::new(LedgerCode::Mismatch));
    }
    Ok(())
}

const MISMATCH: &str = "The signature from your Ledger does not match this account. Connect the Ledger you added it with.";

fn verify(public_key: &[u8], digest: &[u8; 32], serialized: &str) -> R<()> {
    use sui_crypto::Verifier as _;
    let sig = sui_sdk_types::UserSignature::from_base64(serialized).map_err(|_| LedgerError::with(LedgerCode::Mismatch, MISMATCH))?;
    let pk: [u8; 32] = public_key.try_into().map_err(|_| LedgerError::with(LedgerCode::Mismatch, MISMATCH))?;
    let vk = sui_crypto::ed25519::Ed25519VerifyingKey::new(&sui_sdk_types::Ed25519PublicKey::new(pk)).map_err(|_| LedgerError::with(LedgerCode::Mismatch, MISMATCH))?;
    vk.verify(digest, &sig).map_err(|_| LedgerError::with(LedgerCode::Mismatch, MISMATCH))
}

/// Signs transaction bytes as the sender. `objects` are the clear-signing objects' BCS (empty when
/// they couldn't be read: the app falls back to its own checks and may ask for blind signing).
/// The signature is verified against `public_key` before it is returned.
pub fn sign_transaction(t: &mut dyn Apdu, path: &str, public_key: &[u8], tx_bytes: &[u8], objects: &[Vec<u8>]) -> R<String> {
    assert_key(t, path, public_key)?;
    let sig = sign_payload(t, path, &tx_signing_payload(tx_bytes), objects)?;
    let out = serialize_signature(&sig, public_key)?;
    verify(public_key, &tx_digest(tx_bytes), &out)?;
    Ok(out)
}

pub fn sign_personal_message(t: &mut dyn Apdu, path: &str, public_key: &[u8], message: &[u8]) -> R<String> {
    assert_key(t, path, public_key)?;
    let sig = sign_payload(t, path, &personal_message_signing_payload(message), &[])?;
    let out = serialize_signature(&sig, public_key)?;
    verify(public_key, &personal_message_digest(message), &out)?;
    Ok(out)
}

// ---------------------------------------------------------------- sessions (device.ts, index.ts)

/// What the review sheet shows while a request waits on the device (prompt.ts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Connecting,
    Unlock,
    OpenApp,
    Confirm,
    Verify,
    Done,
}

impl Stage {
    /// LEDGER_STAGE_TEXT.
    pub fn text(self) -> &'static str {
        match self {
            Stage::Connecting => "Connecting to your Ledger",
            Stage::Unlock => "Unlock your Ledger",
            Stage::OpenApp => "Open the Sui app on your Ledger",
            Stage::Confirm => "Confirm on your Ledger",
            Stage::Verify => "Check the address on your Ledger",
            Stage::Done => "Done",
        }
    }
}

/// Opens a device for one conversation (the transport side: `ledger::LedgerState` or a fake).
pub trait LedgerPort {
    type Conn: Apdu;
    fn open(&mut self, id: &str) -> R<Self::Conn>;
}

/// How long to wait for "Unlock your Ledger" / "Open the Sui app" before giving up.
pub const APP_WAIT: Duration = Duration::from_secs(180);
pub const POLL: Duration = Duration::from_millis(1500);

/// One conversation: open, check the Sui app (`app_version` gets its version), run `f`. While
/// the device is locked or another app is open it reports `Unlock` / `OpenApp`, waits `poll`
/// and retries until `wait` runs out or `cancelled()` says stop.
pub fn session<P: LedgerPort, T, F: FnMut(&mut P::Conn, Option<String>) -> R<T>>(
    port: &mut P,
    id: &str,
    wait: Duration,
    poll: Duration,
    on_stage: &mut dyn FnMut(Stage),
    cancelled: &dyn Fn() -> bool,
    mut f: F,
) -> R<T> {
    let deadline = Instant::now() + wait;
    loop {
        if cancelled() {
            return Err(LedgerError::new(LedgerCode::Cancelled));
        }
        let attempt = (|| {
            let mut c = port.open(id)?;
            let info = check_sui_app(&mut c)?;
            f(&mut c, info.map(|i| i.1))
        })();
        match attempt {
            Ok(v) => return Ok(v),
            Err(e) => {
                let stage = match e.code {
                    LedgerCode::Locked => Stage::Unlock,
                    LedgerCode::AppClosed | LedgerCode::WrongApp => Stage::OpenApp,
                    _ => return Err(e),
                };
                if Instant::now() > deadline {
                    return Err(e);
                }
                on_stage(stage);
                let until = Instant::now() + poll;
                while Instant::now() < until {
                    if cancelled() {
                        return Err(LedgerError::new(LedgerCode::Cancelled));
                    }
                    std::thread::sleep(Duration::from_millis(20).min(poll));
                }
                on_stage(Stage::Connecting);
            }
        }
    }
}

/// A test device running a fake Sui app (port of ledger/_testkit.ts): keys from a phrase,
/// real Ed25519 signatures over blake2b-256 of the intent message.
pub mod fake {
    use super::*;
    use crate::keys::Keypair;

    pub const TEST_PHRASE: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

    #[derive(Debug, Clone, PartialEq)]
    pub struct SignCall {
        pub path: String,
        pub payload: Vec<u8>,
        pub objects: Vec<Vec<u8>>,
    }

    #[derive(Debug, Clone)]
    pub struct FakeLedger {
        pub app: String,
        pub locked: bool,
        pub reject: bool,
        pub blind_off: bool,
        pub phrase: String,
        pub signs: Vec<SignCall>,
        pub shown: Vec<String>,
        /// every APDU sent, hex
        pub apdus: Vec<String>,
        session: Option<Box<BlockSession>>,
    }

    type BlockSession = (u8, Vec<Vec<u8>>, Vec<[u8; 32]>, usize, Vec<u8>);

    impl Default for FakeLedger {
        fn default() -> Self {
            FakeLedger::new()
        }
    }

    impl FakeLedger {
        pub fn new() -> Self {
            FakeLedger { app: "Sui".into(), locked: false, reject: false, blind_off: false, phrase: TEST_PHRASE.into(), signs: vec![], shown: vec![], apdus: vec![], session: None }
        }
        pub fn app(mut self, app: &str) -> Self {
            self.app = app.into();
            self
        }
        pub fn locked(mut self) -> Self {
            self.locked = true;
            self
        }
        pub fn rejecting(mut self) -> Self {
            self.reject = true;
            self
        }
        pub fn blind_off(mut self) -> Self {
            self.blind_off = true;
            self
        }
        pub fn phrase(mut self, phrase: &str) -> Self {
            self.phrase = phrase.into();
            self
        }
    }

    fn u32le(b: &[u8], o: usize) -> u32 {
        u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
    }

    pub fn parse_bip32(p: &[u8]) -> String {
        let n = p[0] as usize;
        let parts: Vec<String> = (0..n)
            .map(|i| {
                let v = u32le(p, 1 + i * 4);
                if v >= 0x8000_0000 {
                    format!("{}'", v - 0x8000_0000)
                } else {
                    v.to_string()
                }
            })
            .collect();
        format!("m/{}", parts.join("/"))
    }

    pub fn device_key(phrase: &str, path: &str) -> Keypair {
        Keypair::from_mnemonic(phrase, crate::keys::path_index(path).expect("standard path")).unwrap()
    }

    fn ok(body: &[u8]) -> Vec<u8> {
        let mut v = body.to_vec();
        v.extend_from_slice(&[0x90, 0x00]);
        v
    }
    fn status(sw: u16) -> Vec<u8> {
        sw.to_be_bytes().to_vec()
    }

    impl FakeLedger {
        fn next(&mut self) -> Vec<u8> {
            let (ins, params, want, cur, _) = (**self.session.as_ref().unwrap()).clone();
            if cur < want.len() {
                let mut b = vec![0x02];
                b.extend_from_slice(&want[cur]);
                return ok(&b);
            }
            self.session = None;
            self.answer(ins, params)
        }

        fn answer(&mut self, ins: u8, params: Vec<Vec<u8>>) -> Vec<u8> {
            match ins {
                0x00 => ok(&[0x01, 1, 4, 0]),
                0x01 | 0x02 => {
                    let path = parse_bip32(&params[0]);
                    if ins == 0x01 {
                        if self.reject {
                            return status(0x6e04);
                        }
                        self.shown.push(path.clone());
                    }
                    let kp = device_key(&self.phrase, &path);
                    let pk = kp.public_key();
                    let addr = crate::keys::unhex(&kp.address()[2..]).unwrap();
                    let mut b = vec![0x01, pk.len() as u8];
                    b.extend_from_slice(&pk);
                    b.push(addr.len() as u8);
                    b.extend_from_slice(&addr);
                    ok(&b)
                }
                0x03 => {
                    let txp = &params[0];
                    let len = u32le(txp, 0) as usize;
                    let payload = txp[4..4 + len].to_vec();
                    let path = parse_bip32(&params[1]);
                    let mut objects = vec![];
                    if let Some(l) = params.get(2) {
                        let n = u32le(l, 0) as usize;
                        let mut o = 4;
                        for _ in 0..n {
                            let k = u32le(l, o) as usize;
                            objects.push(l[o + 4..o + 4 + k].to_vec());
                            o += 4 + k;
                        }
                    }
                    let no_objects = objects.is_empty();
                    self.signs.push(SignCall { path: path.clone(), payload: payload.clone(), objects });
                    if self.blind_off && payload[0] == 0 && no_objects {
                        return status(0x6808);
                    }
                    if self.reject {
                        return status(0x6e04);
                    }
                    let Keypair::Ed25519(k) = device_key(&self.phrase, &path) else { unreachable!() };
                    use blake2::digest::consts::U32;
                    use ed25519_dalek::Signer as _;
                    let digest = blake2::Blake2b::<U32>::digest(&payload);
                    let sig = k.sign(&digest).to_bytes();
                    let mut b = vec![0x01];
                    b.extend_from_slice(&sig);
                    ok(&b)
                }
                _ => status(0x6d00),
            }
        }
    }

    impl Apdu for FakeLedger {
        fn exchange(&mut self, apdu: &[u8]) -> R<Vec<u8>> {
            self.apdus.push(crate::keys::hex(apdu));
            let (cla, ins) = (apdu[0], apdu[1]);
            let data = apdu[5..5 + apdu[4] as usize].to_vec();
            if self.locked {
                return Ok(status(0x5515));
            }
            if cla == 0xb0 && ins == 0x01 {
                let mut b = vec![0x01, self.app.len() as u8];
                b.extend_from_slice(self.app.as_bytes());
                b.push(5);
                b.extend_from_slice(b"1.4.0");
                b.extend_from_slice(&[0x01, 0x00]);
                return Ok(ok(&b));
            }
            if self.app != "Sui" || cla != 0x00 {
                return Ok(status(0x6e00));
            }
            match data.first() {
                Some(0) => {
                    let want: Vec<[u8; 32]> = data[1..].chunks_exact(32).map(|c| c.try_into().unwrap()).collect();
                    self.session = Some(Box::new((ins, vec![], want, 0, vec![])));
                    Ok(self.next())
                }
                Some(1) if self.session.is_some() => {
                    let linked = &data[1..];
                    let s = self.session.as_mut().unwrap();
                    let expect = s.2[s.3];
                    if <[u8; 32]>::from(Sha256::digest(linked)) != expect {
                        return Ok(status(0x6a80));
                    }
                    s.4.extend_from_slice(&linked[32..]);
                    let next: [u8; 32] = linked[..32].try_into().unwrap();
                    if next == [0u8; 32] {
                        let acc = std::mem::take(&mut s.4);
                        s.1.push(acc);
                        s.3 += 1;
                    } else {
                        let i = s.3;
                        s.2[i] = next;
                    }
                    Ok(self.next())
                }
                _ => Ok(status(0x6a80)),
            }
        }
    }
}

#[cfg(test)]
mod tests;
