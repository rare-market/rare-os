//! Passkey accounts: what the passkey screens call. Port of src/wallet/passkey/index.ts (and the
//! dev-only soft authenticator, soft.ts, as `soft` for tests).
//!
//! A passkey account is a 1-of-2 Sui multisig of the person's passkey (secp256r1, rp rare.market)
//! and an Ed25519 device key sealed in the PIN vault (see `multisig`). The passkey's user handle
//! is 0x01 || the device public key, so a new device rebuilds the address from one sign-in: the
//! user handle gives the device key, and the passkey key is recovered from the assertion.
//!
//! The ceremony runs through an `Authenticator`: the CTAP2 side (`passkey::PasskeyState`,
//! feature `passkey`) or `soft::SoftAuthenticator`.

use crate::keys::{personal_message_digest, tx_digest, Keypair, FLAG_ED25519};
use crate::keystore::{AccountKind, AccountStore, Keystore, KeystoreError, PasskeyAccount, PasskeyMember, PinOptions, RareAccount};
use crate::multisig::{self, AssertionParts};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const RP_ID: &str = "rare.market";
/// First byte of the user handle: layout version (0x01 = device Ed25519 public key follows).
const USER_HANDLE_V1: u8 = 0x01;
/// 0x02: the same, for an account with more than one passkey (a phone and a security key):
/// its other members come from rare.market's registry.
const USER_HANDLE_V2: u8 = 0x02;

/// `PasskeyVia` in native.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Phone,
    Usb,
}

impl Via {
    pub fn as_str(self) -> &'static str {
        match self {
            Via::Phone => "phone",
            Via::Usb => "usb",
        }
    }
    fn authenticator_name(self) -> &'static str {
        match self {
            Via::Usb => "security-key",
            Via::Phone => "phone",
        }
    }
}

/// `PasskeyRegistration` in native.ts (base64 fields).
#[derive(Debug, Clone, PartialEq)]
pub struct Registration {
    pub credential_id: String,
    pub public_key: String,
    pub authenticator: String,
    pub prf: bool,
}

/// `PasskeyAssertion` in native.ts (base64 fields).
#[derive(Debug, Clone, PartialEq)]
pub struct Assertion {
    pub credential_id: String,
    pub authenticator_data: String,
    pub client_data_json: String,
    pub signature: String,
    pub user_handle: Option<String>,
    pub prf: Option<String>,
}

/// The WebAuthn client: errors are the "code|message" strings of `PasskeyNative`.
pub trait Authenticator {
    fn register(&mut self, user_id: &str, user_name: &str, display_name: &str, challenge: &str, via: Via) -> Result<Registration, String>;
    fn assert(&mut self, challenge: &str, credential_ids: &[String], via: Via, prf_salt: Option<&str>) -> Result<Assertion, String>;
    /// PRF output (base64 32 bytes) for a base64 32-byte input; "no-prf" when unsupported.
    fn prf(&mut self, credential_ids: &[String], salt: &str, via: Via) -> Result<String, String>;
}

/// Chain reads that tell candidate addresses apart (anything on chain = the real one).
pub trait History {
    fn has_history(&self, address: &str) -> bool;
    /// rare.market's by-passkey registry: the account registered for this passkey key and device
    /// key (see `passkey_registry`). Default: no registry, so history and the second touch decide.
    fn registered(&self, _passkey_public_key: &[u8], _device_key: &[u8]) -> Option<String> {
        None
    }
    /// The registry's entry with every passkey of the account in committee order (accounts with a
    /// phone and a security key). Default: the address alone, one passkey.
    fn registered_members(&self, passkey_public_key: &[u8], device_key: &[u8]) -> Option<(String, Vec<String>)> {
        self.registered(passkey_public_key, device_key).map(|a| (a, vec![B64.encode(passkey_public_key)]))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PasskeyError {
    Keystore(KeystoreError),
    /// from the authenticator ("cancelled", "timeout", "notfound", "no-prf", ...)
    Native(crate::WalletError),
    Invalid(String),
}

impl std::fmt::Display for PasskeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PasskeyError::Keystore(e) => write!(f, "{e}"),
            PasskeyError::Native(e) => f.write_str(&e.message),
            PasskeyError::Invalid(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for PasskeyError {}

impl From<KeystoreError> for PasskeyError {
    fn from(e: KeystoreError) -> Self {
        PasskeyError::Keystore(e)
    }
}

fn native(e: String) -> PasskeyError {
    PasskeyError::Native(crate::WalletError::parse(&e))
}

fn invalid(m: &str) -> PasskeyError {
    PasskeyError::Invalid(m.to_string())
}

type R<T> = Result<T, PasskeyError>;

fn b64url(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

fn random32() -> [u8; 32] {
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    b
}

// ---------------------------------------------------------------- helpers

/// user.id for a new passkey: 0x01 || the 32-byte Ed25519 device public key.
pub fn user_handle_for(device_public_key: &[u8]) -> Vec<u8> {
    let mut u = vec![USER_HANDLE_V1];
    u.extend_from_slice(device_public_key);
    u
}

/// user.id for the passkeys of an account with more than one: 0x02 || the device public key.
pub fn user_handle_multi(device_public_key: &[u8]) -> Vec<u8> {
    let mut u = vec![USER_HANDLE_V2];
    u.extend_from_slice(device_public_key);
    u
}

/// The device public key inside a Rare user handle, or None for a passkey Rare did not make.
pub fn device_key_from_user_handle(user_handle: Option<&str>) -> Option<Vec<u8>> {
    let u = B64.decode(user_handle?).ok()?;
    (u.len() == 33 && (u[0] == USER_HANDLE_V1 || u[0] == USER_HANDLE_V2)).then(|| u[1..].to_vec())
}

/// Whether a Rare user handle belongs to an account with more than one passkey.
pub fn user_handle_is_multi(user_handle: Option<&str>) -> bool {
    user_handle.and_then(|h| B64.decode(h).ok()).is_some_and(|u| u.len() == 33 && u[0] == USER_HANDLE_V2)
}

/// Checks an assertion is ours (rare.market, our challenge, a get) and returns every compressed
/// public key its signature could have come from (P-256 recovery: usually two).
pub fn recover_passkey_keys(a: &Assertion, challenge: &[u8]) -> R<Vec<[u8; 33]>> {
    let auth_data = B64.decode(&a.authenticator_data).map_err(|_| invalid("Could not read the passkey signature"))?;
    let cdj_bytes = B64.decode(&a.client_data_json).map_err(|_| invalid("Could not read the passkey signature"))?;
    let cdj: serde_json::Value = serde_json::from_slice(&cdj_bytes).map_err(|_| invalid("The passkey answered a different request"))?;
    if cdj.get("type").and_then(|t| t.as_str()) != Some("webauthn.get") || cdj.get("challenge").and_then(|t| t.as_str()) != Some(b64url(challenge).as_str()) {
        return Err(invalid("The passkey answered a different request"));
    }
    if auth_data.len() < 37 || auth_data[..32] != Sha256::digest(RP_ID.as_bytes())[..] {
        return Err(invalid("This passkey is not a Rare passkey"));
    }
    let mut msg = auth_data.clone();
    msg.extend_from_slice(&Sha256::digest(&cdj_bytes));
    let hash = Sha256::digest(&msg);
    let der = B64.decode(&a.signature).map_err(|_| invalid("Could not read the passkey signature"))?;
    let sig = p256::ecdsa::Signature::from_der(&der).map_err(|_| invalid("Could not read the passkey signature"))?;
    let mut out: Vec<[u8; 33]> = Vec::new();
    for bit in 0..4u8 {
        let Some(id) = ecdsa::RecoveryId::from_byte(bit) else { continue };
        if let Ok(vk) = p256::ecdsa::VerifyingKey::recover_from_prehash(&hash, &sig, id) {
            let pk: [u8; 33] = vk.to_encoded_point(true).as_bytes().try_into().expect("33 bytes");
            if !out.contains(&pk) {
                out.push(pk);
            }
        }
    }
    if out.is_empty() {
        return Err(invalid("Could not read the passkey signature"));
    }
    Ok(out)
}

// ---------------------------------------------------------------- the service

/// Make a passkey and the account. The vault must be unlocked, or `pin` creates it.
pub fn create_account<S: AccountStore>(ks: &Keystore<S>, auth: &mut dyn Authenticator, via: Via, label: Option<&str>, pin: Option<PinOptions<'_>>) -> R<RareAccount> {
    // check before the ceremony, so a phone never ends up with a passkey for an account we drop
    if ks.has_vault()? {
        if !ks.is_unlocked()? {
            return Err(KeystoreError::Locked.into());
        }
    } else if pin.is_none() {
        return Err(KeystoreError::NeedsPassword.into());
    }
    let seed = Zeroizing::new(random32());
    let device = Keypair::from_bytes(FLAG_ED25519, &seed[..]).expect("32 bytes");
    let device_pk = device.public_key();
    let name = label.map(str::trim).filter(|l| !l.is_empty()).unwrap_or("Rare Wallet");
    let reg = auth.register(&B64.encode(user_handle_for(&device_pk)), name, name, &B64.encode(random32()), via).map_err(native)?;
    let secret = device.to_suiprivkey();
    let authenticator = if reg.authenticator.is_empty() { via.authenticator_name().to_string() } else { reg.authenticator.clone() };
    let acc = ks.add_passkey(PasskeyAccount {
        credential_id: &reg.credential_id,
        public_key: &reg.public_key,
        device_key: &B64.encode(&device_pk),
        device_secret: Some(&secret),
        label,
        authenticator: Some(&authenticator),
        pin,
        backups: &[],
    })?;
    ks.select(&acc.address)?;
    Ok(acc)
}

/// "Create account with PIN": this SuiPlay's device key behind `pin` (a new key and PIN, unless a
/// PIN account here already uses the key: then `pin` must open it), then the phone passkey as the
/// backup member, allowed to unlock the device after five wrong PINs. Answers the account and the
/// open device key, for the caller to sign rare.market's registration with and drop.
///
/// `backups` are made in order (a phone, a security key, or both): each is one ceremony, and
/// `on_step(i)` runs before ceremony i so the screen can say which one is next.
pub fn create_pin_account<S: AccountStore>(ks: &Keystore<S>, auth: &mut dyn Authenticator, backups: &[Via], label: Option<&str>, pin: &str, on_step: &mut dyn FnMut(usize)) -> R<(RareAccount, Keypair)> {
    let pd = ks.pin_device().ok_or_else(|| invalid("PIN accounts are not available here"))?;
    if backups.is_empty() || backups.len() > multisig::MAX_PASSKEYS {
        return Err(invalid("Add a phone or a security key"));
    }
    let vault = |e: String| PasskeyError::Keystore(KeystoreError::Vault(crate::WalletError::parse(&e)));
    let kp = if ks.pin_accounts()?.is_empty() { pd.create(pin, None).map_err(vault)? } else { pd.open(pin).map_err(vault)? };
    let name = label.map(str::trim).filter(|l| !l.is_empty()).unwrap_or("Rare Wallet");
    let handle = if backups.len() > 1 { user_handle_multi(&kp.public_key()) } else { user_handle_for(&kp.public_key()) };
    let mut members: Vec<PasskeyMember> = Vec::new();
    for (i, via) in backups.iter().enumerate() {
        on_step(i);
        let reg = auth.register(&B64.encode(&handle), name, name, &B64.encode(random32()), *via).map_err(native)?;
        if members.iter().any(|m| m.public_key == reg.public_key) {
            return Err(invalid("That passkey is already in this account. Use a different phone or key."));
        }
        let authenticator = if reg.authenticator.is_empty() { via.authenticator_name().to_string() } else { reg.authenticator.clone() };
        members.push(PasskeyMember { credential_id: reg.credential_id, public_key: reg.public_key, authenticator: Some(authenticator) });
    }
    let acc = ks.add_pin_account(&members, label)?;
    for m in &members {
        pd.add_unlocker(pin, &m.public_key).map_err(vault)?;
    }
    ks.select(&acc.address)?;
    Ok((acc, kp))
}

/// The account's passkeys a ceremony over `via` may answer with (the phone's for Phone, the
/// security key's for Usb; all of them when none is marked).
pub fn credentials_for(account: &RareAccount, via: Via) -> Vec<PasskeyMember> {
    let Some(info) = account.passkey.as_ref() else { return Vec::new() };
    let all: Vec<PasskeyMember> = info.passkeys().into_iter().filter(|m| !m.credential_id.is_empty()).collect();
    let want = via.authenticator_name();
    let some: Vec<PasskeyMember> = all.iter().filter(|m| m.authenticator.as_deref() == Some(want)).cloned().collect();
    if some.is_empty() {
        all
    } else {
        some
    }
}

/// After five wrong PINs: the account's phone passkey signs a fresh challenge from the PIN guard,
/// which resets the count. Answers the token `Keystore::reset_pin` takes with the new PIN.
pub fn unlock_with_phone<S: AccountStore>(ks: &Keystore<S>, account: &RareAccount, auth: &mut dyn Authenticator, via: Via) -> R<String> {
    let pd = ks.pin_device().ok_or_else(|| invalid("PIN accounts are not available here"))?;
    let info = account.passkey.as_ref().filter(|p| p.is_pin()).ok_or_else(|| invalid("Not a PIN account"))?;
    let vault = |e: String| PasskeyError::Keystore(KeystoreError::Vault(crate::WalletError::parse(&e)));
    let _ = info;
    let challenge = pd.guard.challenge().map_err(vault)?;
    let ids: Vec<String> = credentials_for(account, via).into_iter().map(|m| m.credential_id).collect();
    let a = auth.assert(&challenge, &ids, via, None).map_err(native)?;
    pd.guard.unlock(&challenge, &a.authenticator_data, &a.client_data_json, &a.signature).map_err(vault)
}

/// The new PIN after `unlock_with_phone`, and the device key armed for the signature waiting.
pub fn reset_pin<S: AccountStore>(ks: &Keystore<S>, address: &str, token: &str, new_pin: &str) -> R<()> {
    let pd = ks.pin_device().ok_or_else(|| invalid("PIN accounts are not available here"))?;
    let kp = pd.reset_pin(token, new_pin).map_err(|e| PasskeyError::Keystore(KeystoreError::Vault(crate::WalletError::parse(&e))))?;
    ks.arm_key(address, kp)?;
    Ok(())
}

/// PIN signing back on a new SuiPlay: the 12 recovery words give the same device key, the
/// backup (phone or security key) signs in once so the account's members are known (skipped when
/// this SuiPlay already lists the account), and the account signs with a new PIN here again at
/// the same address. Every passkey of the account may unlock the device after 5 wrong PINs.
pub fn restore_pin_account<S: AccountStore>(ks: &Keystore<S>, auth: &mut dyn Authenticator, history: &dyn History, via: Via, words: &str, pin: &str) -> R<RareAccount> {
    let pd = ks.pin_device().ok_or_else(|| invalid("PIN accounts are not available here"))?;
    let words = crate::keys::normalize_mnemonic(words);
    if !crate::keys::is_valid_mnemonic(&words) {
        return Err(invalid("Those words are not a recovery phrase"));
    }
    let dk = B64.encode(Keypair::from_mnemonic(&words, 0).map_err(|e| invalid(&e.to_string()))?.public_key());
    // one device key per SuiPlay: another PIN account's key here stays
    let others = ks.pin_accounts()?;
    if others.iter().any(|a| a.passkey.as_ref().is_some_and(|p| p.device_key != dk)) {
        return Err(invalid("This SuiPlay already has a PIN account with other words. Remove it first, or move its things to the restored account later."));
    }
    let known = ks.list_accounts()?.into_iter().find(|a| a.passkey.as_ref().is_some_and(|p| p.device_key == dk));
    let acc = match known {
        Some(a) => a,
        None => {
            let a = sign_in(ks, auth, history, via, None)?;
            if a.passkey.as_ref().map(|p| p.device_key.as_str()) != Some(dk.as_str()) {
                return Err(invalid("Those words belong to another account than this passkey. Check the words, or use the backup of that account."));
            }
            a
        }
    };
    let vault = |e: String| PasskeyError::Keystore(KeystoreError::Vault(crate::WalletError::parse(&e)));
    if others.is_empty() || pd.public_key().as_deref() != Some(dk.as_str()) {
        pd.create(pin, Some(&words)).map_err(vault)?;
    } else {
        pd.open(pin).map_err(vault)?;
    }
    let acc = ks.adopt_pin(&acc.address)?;
    for m in acc.passkey.as_ref().map(|p| p.passkeys()).unwrap_or_default() {
        pd.add_unlocker(pin, &m.public_key).map_err(vault)?;
    }
    ks.select(&acc.address)?;
    Ok(acc)
}

/// Sign in with an existing Rare passkey, on this device or a new one. One discoverable
/// assertion gives the device key (user handle) and two or so candidate passkey keys; the account
/// is the candidate already known here, or the one whose device key is in the vault, or the one
/// rare.market's registry knows, or the only one with on-chain history. Only when none of that decides does the passkey sign a second time
/// (same credential), and the key common to both signatures wins.
pub fn sign_in<S: AccountStore>(ks: &Keystore<S>, auth: &mut dyn Authenticator, history: &dyn History, via: Via, label: Option<&str>) -> R<RareAccount> {
    let c1 = random32();
    let a1 = auth.assert(&B64.encode(c1), &[], via, None).map_err(native)?;
    let device_key = device_key_from_user_handle(a1.user_handle.as_deref()).ok_or_else(|| invalid("This passkey was not made in Rare Wallet. Pick your Rare passkey."))?;
    let device_key_b64 = B64.encode(&device_key);

    let accounts = ks.list_accounts()?;
    if let Some(same) = accounts
        .iter()
        .find(|a| a.kind == AccountKind::Passkey && a.passkey.as_ref().is_some_and(|p| p.device_key == device_key_b64 && p.passkeys().iter().any(|m| m.credential_id == a1.credential_id)))
    {
        ks.select(&same.address)?;
        return Ok(same.clone());
    }
    if user_handle_is_multi(a1.user_handle.as_deref()) {
        return sign_in_multi(ks, &a1, &c1, &device_key, history, via, label);
    }

    let candidates: Vec<([u8; 33], String)> = recover_passkey_keys(&a1, &c1)?
        .into_iter()
        .map(|pk| {
            let c = multisig::passkey_multisig(&pk, &device_key).map_err(|e| PasskeyError::Invalid(e.0))?;
            Ok((pk, multisig::multisig_address(&c)))
        })
        .collect::<R<_>>()?;
    if let Some((_, addr)) = candidates.iter().find(|(_, addr)| accounts.iter().any(|a| &a.address == addr)) {
        ks.select(addr)?;
        return Ok(accounts.into_iter().find(|a| &a.address == addr).expect("known"));
    }

    let mut pick = (candidates.len() == 1).then(|| candidates[0].clone());
    if pick.is_none() {
        // the vault outlived the account list (UI data cleared): its device key names the account
        let with_key: Vec<_> = candidates.iter().filter(|(_, a)| ks.has_device_key(a)).cloned().collect();
        if with_key.len() == 1 {
            pick = with_key.into_iter().next();
        }
    }
    if pick.is_none() {
        // rare.market knows the pair (the website or another launcher registered it): one touch
        let hits: Vec<_> = candidates.iter().filter(|(pk, a)| history.registered(pk, &device_key).is_some_and(|r| r.eq_ignore_ascii_case(a))).cloned().collect();
        if hits.len() == 1 {
            pick = hits.into_iter().next();
        }
    }
    if pick.is_none() {
        let used: Vec<_> = candidates.iter().filter(|(_, a)| history.has_history(a)).cloned().collect();
        if used.len() == 1 {
            pick = used.into_iter().next();
        }
    }
    let pick = match pick {
        Some(p) => p,
        None => {
            let c2 = random32();
            let a2 = auth.assert(&B64.encode(c2), std::slice::from_ref(&a1.credential_id), via, None).map_err(native)?;
            let second = recover_passkey_keys(&a2, &c2)?;
            candidates.iter().find(|(pk, _)| second.contains(pk)).cloned().ok_or_else(|| invalid("Could not identify the passkey account"))?
        }
    };

    let acc = ks.add_passkey(PasskeyAccount {
        credential_id: &a1.credential_id,
        public_key: &B64.encode(pick.0),
        device_key: &device_key_b64,
        device_secret: None,
        label,
        authenticator: Some(via.authenticator_name()),
        pin: None,
        backups: &[],
    })?;
    ks.select(&acc.address)?;
    Ok(acc)
}

/// Sign in to an account with more than one passkey: the registry knows its members.
fn sign_in_multi<S: AccountStore>(ks: &Keystore<S>, a1: &Assertion, c1: &[u8], device_key: &[u8], history: &dyn History, via: Via, label: Option<&str>) -> R<RareAccount> {
    let dk_b64 = B64.encode(device_key);
    let mut hits: Vec<(String, Vec<String>)> = Vec::new();
    for pk in recover_passkey_keys(a1, c1)? {
        let pk_b64 = B64.encode(pk);
        let Some((address, members)) = history.registered_members(&pk, device_key) else { continue };
        if !members.contains(&pk_b64) {
            continue;
        }
        let keys: Vec<&str> = members.iter().map(String::as_str).collect();
        let Ok(c) = multisig::rare_committee_b64(&keys, &dk_b64) else { continue };
        if multisig::multisig_address(&c).eq_ignore_ascii_case(&address) && !hits.iter().any(|(a, _)| a == &address) {
            hits.push((address, members));
        }
    }
    let (address, members) = match hits.len() {
        1 => hits.remove(0),
        _ => return Err(invalid("Rare could not look up this account on rare.market. Check the connection and try again.")),
    };
    if let Some(known) = ks.list_accounts()?.into_iter().find(|a| a.address.eq_ignore_ascii_case(&address)) {
        ks.select(&known.address)?;
        return Ok(known);
    }
    let touched = B64.encode(recover_passkey_keys(a1, c1)?.into_iter().find(|pk| members.contains(&B64.encode(pk))).expect("a hit"));
    let member = |pk: &String| PasskeyMember { credential_id: if *pk == touched { a1.credential_id.clone() } else { String::new() }, public_key: pk.clone(), authenticator: (*pk == touched).then(|| via.authenticator_name().to_string()) };
    let all: Vec<PasskeyMember> = members.iter().map(member).collect();
    let acc = ks.add_passkey(PasskeyAccount {
        credential_id: &all[0].credential_id,
        public_key: &all[0].public_key,
        device_key: &dk_b64,
        device_secret: None,
        label,
        authenticator: all[0].authenticator.as_deref(),
        pin: None,
        backups: &all[1..],
    })?;
    ks.select(&acc.address)?;
    Ok(acc)
}

/// Signs as the account's passkey member (one phone tap or key touch per signature); returns
/// the account's multisig signature (base64).
pub fn sign_with_passkey(account: &RareAccount, auth: &mut dyn Authenticator, via: Via, transaction: bool, bytes: &[u8]) -> R<String> {
    let info = account.passkey.as_ref().ok_or_else(|| invalid("Not a passkey account"))?;
    let committee = info.committee().map_err(|e| PasskeyError::Invalid(e.0))?;
    let digest = if transaction { tx_digest(bytes) } else { personal_message_digest(bytes) };
    let members = credentials_for(account, via);
    let ids: Vec<String> = members.iter().map(|m| m.credential_id.clone()).collect();
    let a = auth.assert(&B64.encode(digest), &ids, via, None).map_err(native)?;
    let dec = |s: &str| B64.decode(s).map_err(|_| invalid("Could not read the passkey signature"));
    let (ad, cdj, der) = (dec(&a.authenticator_data)?, dec(&a.client_data_json)?, dec(&a.signature)?);
    // the member that signed: by credential id, else by the key its signature recovers to
    let signer = match info.passkeys().into_iter().find(|m| !m.credential_id.is_empty() && m.credential_id == a.credential_id) {
        Some(m) => m.public_key,
        None => {
            let cands = recover_passkey_keys(&a, &digest)?;
            info.passkeys().into_iter().find(|m| cands.iter().any(|c| B64.encode(c) == m.public_key)).map(|m| m.public_key).ok_or_else(|| invalid("That passkey is not part of this account"))?
        }
    };
    let pk = dec(&signer)?;
    let member = multisig::passkey_signature(&pk, &digest, &AssertionParts { authenticator_data: &ad, client_data_json: &cdj, signature_der: &der }).map_err(|e| PasskeyError::Invalid(e.0))?;
    multisig::combine_member(&committee, &member).map_err(|e| PasskeyError::Invalid(e.0))
}

/// The passkey's PRF output for a 32-byte WebAuthn PRF input (Rare uses SHA-256("rare/" + context)).
pub fn passkey_prf(account: &RareAccount, auth: &mut dyn Authenticator, via: Via, input: &[u8]) -> R<Zeroizing<Vec<u8>>> {
    let info = account.passkey.as_ref().ok_or_else(|| invalid("Not a passkey account"))?;
    if input.len() != 32 {
        return Err(invalid("The PRF input must be 32 bytes"));
    }
    let out = Zeroizing::new(B64.decode(auth.prf(std::slice::from_ref(&info.credential_id), &B64.encode(input), via).map_err(native)?).unwrap_or_default());
    if out.len() != 32 {
        return Err(invalid("The passkey sent a bad PRF output"));
    }
    Ok(out)
}

/// The CTAP2 side as an `Authenticator`, from a plain worker thread on `rt`.
#[cfg(feature = "passkey")]
pub struct Ctap<'a> {
    pub state: &'a crate::passkey::PasskeyState,
    pub sink: crate::Sink,
    pub rt: tokio::runtime::Handle,
}

#[cfg(feature = "passkey")]
impl Authenticator for Ctap<'_> {
    fn register(&mut self, user_id: &str, user_name: &str, display_name: &str, challenge: &str, via: Via) -> Result<Registration, String> {
        let r = self.rt.block_on(self.state.register(&self.sink, user_id, user_name.into(), display_name.into(), challenge, via.as_str()))?;
        Ok(Registration { credential_id: r.credential_id, public_key: r.public_key, authenticator: r.authenticator, prf: r.prf })
    }
    fn assert(&mut self, challenge: &str, credential_ids: &[String], via: Via, prf_salt: Option<&str>) -> Result<Assertion, String> {
        let a = self.rt.block_on(self.state.assert(&self.sink, challenge, credential_ids, via.as_str(), prf_salt))?;
        Ok(Assertion { credential_id: a.credential_id, authenticator_data: a.authenticator_data, client_data_json: a.client_data_json, signature: a.signature, user_handle: a.user_handle, prf: a.prf })
    }
    fn prf(&mut self, credential_ids: &[String], salt: &str, via: Via) -> Result<String, String> {
        self.rt.block_on(self.state.prf(&self.sink, credential_ids, salt, via.as_str()))
    }
}

/// A software authenticator (port of soft.ts) for tests: a real P-256 key per credential, DER
/// signatures over authenticatorData || SHA-256(clientDataJSON), browser-shaped clientDataJSON,
/// PRF as HMAC-SHA-256(credential secret, SHA-256("WebAuthn PRF" || 0x00 || input)).
pub mod soft {
    use super::*;
    use hmac::{Hmac, Mac};

    pub const SOFT_ORIGIN: &str = "https://rare.market";

    struct Cred {
        id: Vec<u8>,
        sk: p256::ecdsa::SigningKey,
        user_id: String,
        counter: u32,
        prf_key: [u8; 32],
    }

    #[derive(Default)]
    pub struct SoftAuthenticator {
        creds: Vec<Cred>,
        /// every assert call's credential ids, and every prf call's
        pub asserts: Vec<Vec<String>>,
        pub prfs: Vec<(Vec<String>, Via)>,
        pub registers: usize,
    }

    /// The salt an authenticator HMACs for WebAuthn PRF input `input`.
    pub fn prf_salt_for(input: &[u8]) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(b"WebAuthn PRF");
        h.update([0u8]);
        h.update(input);
        h.finalize().into()
    }

    /// clientDataJSON the way a browser writes it.
    pub fn client_data_json(ty: &str, challenge: &[u8]) -> Vec<u8> {
        format!(r#"{{"type":"{ty}","challenge":"{}","origin":"{SOFT_ORIGIN}","crossOrigin":false}}"#, b64url(challenge)).into_bytes()
    }

    impl SoftAuthenticator {
        pub fn new() -> Self {
            Self::default()
        }
        /// The same credentials behind another device's view (a phone shared by two SuiPlays).
        pub fn assert_raw(&mut self, challenge: &str, credential_ids: &[String], prf_salt: Option<&str>) -> Result<Assertion, String> {
            let idx = if credential_ids.is_empty() {
                // discoverable: the newest passkey (a phone would let the person pick)
                self.creds.len().checked_sub(1)
            } else {
                self.creds.iter().position(|c| credential_ids.contains(&B64.encode(&c.id)))
            };
            let Some(i) = idx else { return Err(crate::err("notfound", "No Rare passkey on this phone or key")) };
            let cred = &mut self.creds[i];
            cred.counter += 1;
            let mut auth_data = Sha256::digest(RP_ID.as_bytes()).to_vec();
            auth_data.push(0x05); // user present + user verified
            auth_data.extend_from_slice(&cred.counter.to_be_bytes());
            let cdj = client_data_json("webauthn.get", &B64.decode(challenge).map_err(|_| crate::err("error", "bad challenge"))?);
            let mut msg = auth_data.clone();
            msg.extend_from_slice(&Sha256::digest(&cdj));
            let sig: p256::ecdsa::Signature = p256::ecdsa::signature::Signer::sign(&cred.sk, &msg);
            let prf = match prf_salt {
                Some(s) => {
                    let input = B64.decode(s).map_err(|_| crate::err("error", "bad salt"))?;
                    let mut mac = Hmac::<Sha256>::new_from_slice(&cred.prf_key).expect("any key");
                    mac.update(&prf_salt_for(&input));
                    Some(B64.encode(mac.finalize().into_bytes()))
                }
                None => None,
            };
            Ok(Assertion {
                credential_id: B64.encode(&cred.id),
                authenticator_data: B64.encode(&auth_data),
                client_data_json: B64.encode(&cdj),
                signature: B64.encode(sig.to_der().as_bytes()),
                user_handle: if credential_ids.is_empty() { Some(cred.user_id.clone()) } else { None },
                prf,
            })
        }
    }

    impl Authenticator for SoftAuthenticator {
        fn register(&mut self, user_id: &str, _user_name: &str, _display_name: &str, _challenge: &str, via: Via) -> Result<Registration, String> {
            self.registers += 1;
            let sk = p256::ecdsa::SigningKey::random(&mut OsRng);
            let mut id = vec![0u8; 16];
            OsRng.fill_bytes(&mut id);
            let pk = sk.verifying_key().to_encoded_point(true).as_bytes().to_vec();
            let reg = Registration { credential_id: B64.encode(&id), public_key: B64.encode(pk), authenticator: via.authenticator_name().into(), prf: true };
            self.creds.push(Cred { id, sk, user_id: user_id.into(), counter: 0, prf_key: random32() });
            Ok(reg)
        }
        fn assert(&mut self, challenge: &str, credential_ids: &[String], _via: Via, prf_salt: Option<&str>) -> Result<Assertion, String> {
            self.asserts.push(credential_ids.to_vec());
            self.assert_raw(challenge, credential_ids, prf_salt)
        }
        fn prf(&mut self, credential_ids: &[String], salt: &str, via: Via) -> Result<String, String> {
            self.prfs.push((credential_ids.to_vec(), via));
            let a = self.assert_raw(&B64.encode(random32()), credential_ids, Some(salt))?;
            a.prf.ok_or_else(|| crate::err("no-prf", "This passkey cannot protect saved data."))
        }
    }
}

#[cfg(test)]
mod tests;
