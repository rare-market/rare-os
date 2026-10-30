//! Passkey tests without hardware. `SoftCtap` is a tiny CTAP2 authenticator (P-256, resident
//! keys, built-in UV) behind the library's `Token` trait, so register + assert go through the
//! same CtapAuthenticator path as a USB key or a phone tunnel, including our vendored patches.

use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use openssl::bn::BigNumContext;
use openssl::ec::{EcGroup, EcKey, EcPoint, PointConversionForm};
use openssl::ecdsa::EcdsaSig;
use openssl::nid::Nid;
use openssl::pkey::Private;
use serde_cbor_2::Value;
use webauthn_authenticator_rs::ctap2::CtapAuthenticator;
use webauthn_authenticator_rs::error::{CtapError, WebauthnCError};
use webauthn_authenticator_rs::transport::Token;
use webauthn_authenticator_rs::ui::UiCallback;
use webauthn_rs_proto::AuthenticatorTransport;

use super::ui::{Event, Ui};
use super::webauthn::{self, AssertRequest, RegisterRequest};
use super::*;

// ------------------------------------------------------------------ soft authenticator

struct Cred {
    key: EcKey<Private>,
    user_id: Vec<u8>,
    rp_id: String,
    /// CredRandomWithUV: the HMAC key behind PRF / hmac-secret
    cred_random: [u8; 32],
}

/// Which PRF form the soft authenticator advertises in getInfo.
#[derive(Default, Clone, PartialEq)]
enum SoftExt {
    #[default]
    None,
    /// "prf" with outputs in the clear (a phone over hybrid)
    Prf,
    /// "hmac-secret" under these PIN/UV auth protocols (a USB key)
    Hmac(Vec<u32>),
}

#[derive(Default)]
struct SoftCtap {
    ext: SoftExt,
    /// authenticatorClientPIN key agreement key (made on first getKeyAgreement)
    ka_key: Option<EcKey<Private>>,
    /// the extensions map of the last makeCredential / getAssertion
    last_mc_ext: Option<Value>,
    last_ga_ext: Option<Value>,
    creds: Vec<(Vec<u8>, Cred)>,
    /// what the last MakeCredential asked for in its options map
    last_mc_options: BTreeMap<String, bool>,
    /// leave out `credential` (key 1) when the allow list has one entry, as CTAP2 permits
    omit_single_credential: bool,
    counter: u32,
}

impl std::fmt::Debug for SoftCtap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SoftCtap")
    }
}

fn group() -> EcGroup {
    EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap()
}

fn text(s: &str) -> Value {
    Value::Text(s.into())
}

fn cbor_map(v: Value) -> BTreeMap<Value, Value> {
    match v {
        Value::Map(m) => m,
        _ => panic!("not a map"),
    }
}

impl SoftCtap {
    fn handle(&mut self, cmd: u8, body: &[u8]) -> Result<Value, CtapError> {
        let req: BTreeMap<Value, Value> = if body.is_empty() { BTreeMap::new() } else { cbor_map(serde_cbor_2::from_slice(body).unwrap()) };
        let get = |k: i128| req.get(&Value::Integer(k)).cloned();
        match cmd {
            0x04 => {
                let mut m = BTreeMap::from([
                    (Value::Integer(1), Value::Array(vec![text("FIDO_2_0")])),
                    (Value::Integer(3), Value::Bytes(vec![0x5a; 16])),
                    (Value::Integer(4), Value::Map(BTreeMap::from([(text("rk"), Value::Bool(true)), (text("up"), Value::Bool(true)), (text("uv"), Value::Bool(true))]))),
                ]);
                match &self.ext {
                    SoftExt::None => {}
                    SoftExt::Prf => {
                        m.insert(Value::Integer(2), Value::Array(vec![text("devicePubKey"), text("prf")]));
                    }
                    SoftExt::Hmac(protocols) => {
                        m.insert(Value::Integer(2), Value::Array(vec![text("credProtect"), text("hmac-secret")]));
                        m.insert(Value::Integer(6), Value::Array(protocols.iter().map(|p| Value::Integer(*p as i128)).collect()));
                    }
                }
                Ok(Value::Map(m))
            }
            0x06 => {
                // authenticatorClientPIN: only getKeyAgreement (0x02)
                assert_eq!(get(2), Some(Value::Integer(2)), "only getKeyAgreement");
                let ka = self.ka_key.get_or_insert_with(|| EcKey::generate(&group()).unwrap());
                let mut ctx = BigNumContext::new().unwrap();
                let pt = ka.public_key().to_bytes(&group(), PointConversionForm::UNCOMPRESSED, &mut ctx).unwrap();
                let cose = Value::Map(BTreeMap::from([
                    (Value::Integer(1), Value::Integer(2)),
                    (Value::Integer(3), Value::Integer(-25)),
                    (Value::Integer(-1), Value::Integer(1)),
                    (Value::Integer(-2), Value::Bytes(pt[1..33].to_vec())),
                    (Value::Integer(-3), Value::Bytes(pt[33..65].to_vec())),
                ]));
                Ok(Value::Map(BTreeMap::from([(Value::Integer(1), cose)])))
            }
            0x01 => {
                let Some(Value::Bytes(cdh)) = get(1) else { return Err(CtapError::Ctap2MissingParameter) };
                assert_eq!(cdh.len(), 32);
                let rp = cbor_map(get(2).unwrap());
                let rp_id = match rp.get(&text("id")) {
                    Some(Value::Text(s)) => s.clone(),
                    _ => return Err(CtapError::Ctap2MissingParameter),
                };
                let user = cbor_map(get(3).unwrap());
                let Some(Value::Bytes(user_id)) = user.get(&text("id")).cloned() else { return Err(CtapError::Ctap2MissingParameter) };
                let algs = match get(4) {
                    Some(Value::Array(a)) => a,
                    _ => return Err(CtapError::Ctap2MissingParameter),
                };
                let es256 = algs.iter().any(|p| cbor_map(p.clone()).get(&text("alg")) == Some(&Value::Integer(-7)));
                if !es256 {
                    return Err(CtapError::Ctap2UnsupportedAlgorithm);
                }
                self.last_mc_options = match get(7) {
                    Some(Value::Map(m)) => m
                        .into_iter()
                        .filter_map(|(k, v)| match (k, v) {
                            (Value::Text(k), Value::Bool(b)) => Some((k, b)),
                            _ => None,
                        })
                        .collect(),
                    _ => BTreeMap::new(),
                };
                let key = EcKey::generate(&group()).unwrap();
                let mut ctx = BigNumContext::new().unwrap();
                let pt = key.public_key().to_bytes(&group(), PointConversionForm::UNCOMPRESSED, &mut ctx).unwrap();
                let cose = Value::Map(BTreeMap::from([
                    (Value::Integer(1), Value::Integer(2)),
                    (Value::Integer(3), Value::Integer(-7)),
                    (Value::Integer(-1), Value::Integer(1)),
                    (Value::Integer(-2), Value::Bytes(pt[1..33].to_vec())),
                    (Value::Integer(-3), Value::Bytes(pt[33..65].to_vec())),
                ]));
                let cred_id: Vec<u8> = (0..20).map(|i| i as u8 ^ self.creds.len() as u8 ^ 0xa5).collect();
                self.last_mc_ext = get(6);
                let ext = get(6).map(cbor_map).unwrap_or_default();
                let hmac_on = self.ext != SoftExt::None && ext.get(&text("hmac-secret")) == Some(&Value::Bool(true));
                let prf_on = self.ext == SoftExt::Prf && ext.contains_key(&text("prf"));
                let mut ad = webauthn::sha256(rp_id.as_bytes());
                ad.push(if hmac_on { 0xc5 } else { 0x45 }); // UP | UV | AT (| ED)
                ad.extend_from_slice(&0u32.to_be_bytes());
                ad.extend_from_slice(&[0x5a; 16]);
                ad.extend_from_slice(&(cred_id.len() as u16).to_be_bytes());
                ad.extend_from_slice(&cred_id);
                ad.extend_from_slice(&serde_cbor_2::to_vec(&cose).unwrap());
                if hmac_on {
                    ad.extend(serde_cbor_2::to_vec(&Value::Map(BTreeMap::from([(text("hmac-secret"), Value::Bool(true))]))).unwrap());
                }
                let mut cred_random = [0u8; 32];
                openssl::rand::rand_bytes(&mut cred_random).unwrap();
                self.creds.push((cred_id, Cred { key, user_id, rp_id, cred_random }));
                let mut out = BTreeMap::from([
                    (Value::Integer(1), text("none")),
                    (Value::Integer(2), Value::Bytes(ad)),
                    (Value::Integer(3), Value::Map(BTreeMap::new())),
                ]);
                if prf_on {
                    out.insert(Value::Integer(6), Value::Map(BTreeMap::from([(text("prf"), Value::Map(BTreeMap::from([(text("enabled"), Value::Bool(true))])))])));
                }
                Ok(Value::Map(out))
            }
            0x02 => {
                let Some(Value::Text(rp_id)) = get(1) else { return Err(CtapError::Ctap2MissingParameter) };
                let Some(Value::Bytes(cdh)) = get(2) else { return Err(CtapError::Ctap2MissingParameter) };
                let allow: Vec<Vec<u8>> = match get(3) {
                    Some(Value::Array(a)) => a
                        .into_iter()
                        .filter_map(|d| match cbor_map(d).get(&text("id")) {
                            Some(Value::Bytes(b)) => Some(b.clone()),
                            _ => None,
                        })
                        .collect(),
                    _ => vec![],
                };
                let found = self
                    .creds
                    .iter()
                    .rev()
                    .find(|(id, c)| c.rp_id == rp_id && (allow.is_empty() || allow.contains(id)))
                    .ok_or(CtapError::Ctap2NoCredentials)?;
                self.counter += 1;
                self.last_ga_ext = get(4);
                let ext = get(4).map(cbor_map).unwrap_or_default();
                let mut unsigned = None;
                let mut signed_ext = None;
                if let (SoftExt::Prf, Some(Value::Map(prf))) = (&self.ext, ext.get(&text("prf"))) {
                    let eval = cbor_map(prf.get(&text("eval")).cloned().unwrap());
                    let Some(Value::Bytes(salt)) = eval.get(&text("first")) else { return Err(CtapError::Ctap2InvalidCBOR) };
                    let out = soft_hmac(&found.1.cred_random, salt);
                    let results = Value::Map(BTreeMap::from([(text("first"), Value::Bytes(out))]));
                    unsigned = Some(Value::Map(BTreeMap::from([(text("prf"), Value::Map(BTreeMap::from([(text("results"), results)])))])));
                }
                if let (SoftExt::Hmac(_), Some(Value::Map(h))) = (&self.ext, ext.get(&text("hmac-secret"))) {
                    signed_ext = Some(soft_hmac_secret(self.ka_key.as_ref().expect("key agreement first"), h, &found.1.cred_random)?);
                }
                let mut ad = webauthn::sha256(rp_id.as_bytes());
                ad.push(if signed_ext.is_some() { 0x85 } else { 0x05 }); // UP | UV (| ED)
                ad.extend_from_slice(&self.counter.to_be_bytes());
                if let Some(e) = signed_ext {
                    ad.extend(serde_cbor_2::to_vec(&Value::Map(BTreeMap::from([(text("hmac-secret"), Value::Bytes(e))]))).unwrap());
                }
                let mut msg = ad.clone();
                msg.extend_from_slice(&cdh);
                let sig = EcdsaSig::sign(&webauthn::sha256(&msg), &found.1.key).unwrap().to_der().unwrap();
                let mut m = BTreeMap::from([(Value::Integer(2), Value::Bytes(ad)), (Value::Integer(3), Value::Bytes(sig))]);
                if !(self.omit_single_credential && allow.len() == 1) {
                    m.insert(Value::Integer(1), Value::Map(BTreeMap::from([(text("id"), Value::Bytes(found.0.clone())), (text("type"), text("public-key"))])));
                }
                // discoverable sign-in: the user handle comes back (the field our vendored patch reads)
                if allow.is_empty() {
                    m.insert(Value::Integer(4), Value::Map(BTreeMap::from([(text("id"), Value::Bytes(found.1.user_id.clone())), (text("name"), text("rare"))])));
                }
                if let Some(u) = unsigned {
                    m.insert(Value::Integer(8), u);
                }
                Ok(Value::Map(m))
            }
            _ => Err(CtapError::Ctap1InvalidCommand),
        }
    }
}

fn soft_hmac(key: &[u8], msg: &[u8]) -> Vec<u8> {
    let k = openssl::pkey::PKey::hmac(key).unwrap();
    let mut s = openssl::sign::Signer::new(openssl::hash::MessageDigest::sha256(), &k).unwrap();
    s.update(msg).unwrap();
    s.sign_to_vec().unwrap()
}

/// The authenticator side of hmac-secret (CTAP 2.1 §12.5), written separately from prf.rs:
/// ECDH with the platform key, KDF per protocol (OpenSSL's own HKDF for v2), check saltAuth,
/// decrypt the salt, HMAC it with CredRandom, encrypt the output back.
fn soft_hmac_secret(ka: &EcKey<Private>, h: &BTreeMap<Value, Value>, cred_random: &[u8]) -> Result<Vec<u8>, CtapError> {
    use openssl::pkey::PKey;
    use openssl::pkey_ctx::{HkdfMode, PkeyCtx};
    use openssl::symm::{Cipher, Crypter, Mode};
    let i = |k: i128| h.get(&Value::Integer(k)).cloned();
    let protocol = match i(4) {
        Some(Value::Integer(p)) => p as u32,
        _ => 1,
    };
    let peer = cbor_map(i(1).ok_or(CtapError::Ctap2MissingParameter)?);
    let (Some(Value::Bytes(x)), Some(Value::Bytes(y))) = (peer.get(&Value::Integer(-2)), peer.get(&Value::Integer(-3))) else { return Err(CtapError::Ctap2InvalidCBOR) };
    let Some(Value::Bytes(salt_enc)) = i(2) else { return Err(CtapError::Ctap2MissingParameter) };
    let Some(Value::Bytes(salt_auth)) = i(3) else { return Err(CtapError::Ctap2MissingParameter) };
    let peer = EcKey::from_public_key_affine_coordinates(&group(), &openssl::bn::BigNum::from_slice(x).unwrap(), &openssl::bn::BigNum::from_slice(y).unwrap()).unwrap();
    let mine = PKey::from_ec_key(ka.clone()).unwrap();
    let theirs = PKey::from_ec_key(peer).unwrap();
    let mut d = openssl::derive::Deriver::new(&mine).unwrap();
    d.set_peer(&theirs).unwrap();
    let z = d.derive_to_vec().unwrap();
    let hkdf = |info: &[u8]| {
        let mut c = PkeyCtx::new_id(openssl::pkey::Id::HKDF).unwrap();
        c.derive_init().unwrap();
        c.set_hkdf_mode(HkdfMode::EXTRACT_THEN_EXPAND).unwrap();
        c.set_hkdf_md(openssl::md::Md::sha256()).unwrap();
        c.set_hkdf_salt(&[0u8; 32]).unwrap();
        c.set_hkdf_key(&z).unwrap();
        c.add_hkdf_info(info).unwrap();
        let mut out = [0u8; 32];
        c.derive(Some(&mut out)).unwrap();
        out.to_vec()
    };
    let (mac_key, aes_key) = if protocol == 2 { (hkdf(b"CTAP2 HMAC key"), hkdf(b"CTAP2 AES key")) } else {
        let s = openssl::sha::sha256(&z).to_vec();
        (s.clone(), s)
    };
    let mut mac = soft_hmac(&mac_key, &salt_enc);
    if protocol == 1 {
        mac.truncate(16);
    }
    if mac != salt_auth {
        return Err(CtapError::Ctap2PinAuthInvalid);
    }
    let cbc = |mode: Mode, iv: &[u8], data: &[u8]| {
        let mut c = Crypter::new(Cipher::aes_256_cbc(), mode, &aes_key, Some(iv)).unwrap();
        c.pad(false);
        let mut out = vec![0; data.len() + 16];
        let n = c.update(data, &mut out).unwrap();
        let m = c.finalize(&mut out[n..]).unwrap();
        out.truncate(n + m);
        out
    };
    let (iv, ct) = if protocol == 2 { (salt_enc[..16].to_vec(), &salt_enc[16..]) } else { (vec![0; 16], &salt_enc[..]) };
    let salt = cbc(Mode::Decrypt, &iv, ct);
    assert_eq!(salt.len(), 32);
    let output = soft_hmac(cred_random, &salt);
    let out_iv = if protocol == 2 { vec![7u8; 16] } else { vec![0; 16] };
    let mut enc = if protocol == 2 { out_iv.clone() } else { vec![] };
    enc.extend(cbc(Mode::Encrypt, &out_iv, &output));
    Ok(enc)
}

/// A shared handle so a test can look at the token after the authenticator took ownership.
#[derive(Clone, Default)]
struct Soft(Arc<Mutex<SoftCtap>>);

impl std::fmt::Debug for Soft {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Soft")
    }
}

#[async_trait::async_trait]
impl Token for Soft {
    type Id = ();
    fn get_transport(&self) -> AuthenticatorTransport {
        AuthenticatorTransport::Usb
    }
    async fn transmit_raw<U: UiCallback>(&mut self, cbor: &[u8], ui: &U) -> Result<Vec<u8>, WebauthnCError> {
        if cbor[0] != 0x04 {
            ui.request_touch();
        }
        let v = self.0.lock().unwrap().handle(cbor[0], &cbor[1..]).map_err(WebauthnCError::Ctap)?;
        Ok(serde_cbor_2::to_vec(&v).unwrap())
    }
    async fn cancel(&mut self) -> Result<(), WebauthnCError> {
        Ok(())
    }
    async fn init(&mut self) -> Result<(), WebauthnCError> {
        Ok(())
    }
    async fn close(&mut self) -> Result<(), WebauthnCError> {
        Ok(())
    }
}

// ------------------------------------------------------------------ helpers

type Events = Arc<Mutex<Vec<Event>>>;

fn test_ui(via: Via) -> (Ui, Events, mpsc::Sender<Option<String>>) {
    let events = Arc::new(Mutex::new(vec![]));
    let ev = events.clone();
    let (tx, rx) = mpsc::channel();
    let ui = Ui::new(via, Arc::new(move |e| ev.lock().unwrap().push(e)), rx, Arc::new(AtomicBool::new(false)), Instant::now() + TIMEOUT);
    (ui, events, tx)
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap()
}

/// 0x01 || 32-byte Ed25519 device public key, as the JS side builds it.
fn user_handle() -> Vec<u8> {
    let mut u = vec![1u8];
    u.extend((0..32).map(|i| i as u8 * 7));
    u
}

fn verify(public_key_b64: &str, a: &webauthn::Assertion) -> bool {
    let pk = STANDARD.decode(public_key_b64).unwrap();
    let mut ctx = BigNumContext::new().unwrap();
    let point = EcPoint::from_bytes(&group(), &pk, &mut ctx).unwrap();
    let key = EcKey::from_public_key(&group(), &point).unwrap();
    let mut msg = STANDARD.decode(&a.authenticator_data).unwrap();
    msg.extend(webauthn::sha256(&STANDARD.decode(&a.client_data_json).unwrap()));
    let sig = EcdsaSig::from_der(&STANDARD.decode(&a.signature).unwrap()).unwrap();
    sig.verify(&webauthn::sha256(&msg), &key).unwrap()
}

fn registered(o: Outcome) -> webauthn::Registration {
    match o {
        Outcome::Registered(r) => r,
        _ => panic!("expected a registration"),
    }
}

fn asserted(o: Outcome) -> webauthn::Assertion {
    match o {
        Outcome::Asserted(a) => a,
        _ => panic!("expected an assertion"),
    }
}

// ------------------------------------------------------------------ full ceremonies (soft token)

#[test]
fn soft_register_then_discoverable_assert_verifies() {
    let soft = Soft::default();
    let (ui, events, _pin) = test_ui(Via::Usb);
    rt().block_on(async {
        let mut auth = CtapAuthenticator::new(soft.clone(), &ui).await.expect("soft token speaks CTAP2");
        let reg = registered(
            perform(
                &mut auth,
                &Op::Register(RegisterRequest { user_id: user_handle(), user_name: "Rare".into(), display_name: "Rare Wallet".into(), challenge: vec![9; 32] }),
                Via::Usb,
            )
            .unwrap(),
        );
        assert_eq!(reg.authenticator, "security-key");
        let pk = STANDARD.decode(&reg.public_key).unwrap();
        assert_eq!(pk.len(), 33);
        assert!(pk[0] == 2 || pk[0] == 3);
        // patch: the library now asks the key for a resident credential
        assert_eq!(soft.0.lock().unwrap().last_mc_options.get("rk"), Some(&true));

        // a Sui signing challenge: the 32-byte blake2b intent digest
        let digest: Vec<u8> = (0..32).map(|i| 255 - i as u8).collect();
        let a = asserted(perform(&mut auth, &Op::Assert(AssertRequest { challenge: digest.clone(), credential_ids: vec![], prf_input: None }), Via::Usb).unwrap());
        assert_eq!(a.credential_id, reg.credential_id);
        assert_eq!(a.user_handle.as_deref(), Some(STANDARD.encode(user_handle()).as_str()), "user handle comes back on discoverable sign-in");
        assert!(verify(&reg.public_key, &a), "signature verifies over authData || sha256(clientDataJSON)");

        let cd: serde_json::Value = serde_json::from_slice(&STANDARD.decode(&a.client_data_json).unwrap()).unwrap();
        assert_eq!(cd["type"], "webauthn.get");
        assert_eq!(cd["challenge"], URL_SAFE_NO_PAD.encode(&digest));
        assert_eq!(cd["origin"], "https://rare.market");
        let ad = STANDARD.decode(&a.authenticator_data).unwrap();
        assert_eq!(&ad[..32], &webauthn::sha256(b"rare.market")[..]);
    });
    let ev = events.lock().unwrap();
    assert!(ev.iter().any(|e| matches!(e, Event::Status { stage: "touch", .. })));
}

#[test]
fn soft_assert_with_allow_list_and_omitted_credential() {
    let soft = Soft::default();
    soft.0.lock().unwrap().omit_single_credential = true;
    let (ui, _events, _pin) = test_ui(Via::Phone);
    rt().block_on(async {
        let mut auth = CtapAuthenticator::new(soft.clone(), &ui).await.unwrap();
        let reg = registered(
            perform(&mut auth, &Op::Register(RegisterRequest { user_id: user_handle(), user_name: "a".into(), display_name: "".into(), challenge: vec![1; 32] }), Via::Phone).unwrap(),
        );
        assert_eq!(reg.authenticator, "phone");
        let id = STANDARD.decode(&reg.credential_id).unwrap();
        let a = asserted(perform(&mut auth, &Op::Assert(AssertRequest { challenge: vec![7; 32], credential_ids: vec![id], prf_input: None }), Via::Phone).unwrap());
        // patch: the 1-entry allow list stands in for the missing credential
        assert_eq!(a.credential_id, reg.credential_id);
        assert_eq!(a.user_handle, None);
        assert!(verify(&reg.public_key, &a));
    });
}

#[test]
fn soft_unknown_credential_maps_to_notfound() {
    let soft = Soft::default();
    let (ui, _e, _p) = test_ui(Via::Usb);
    rt().block_on(async {
        let mut auth = CtapAuthenticator::new(soft, &ui).await.unwrap();
        let err = perform(&mut auth, &Op::Assert(AssertRequest { challenge: vec![0; 32], credential_ids: vec![vec![1, 2, 3]], prf_input: None }), Via::Usb).unwrap_err();
        assert_eq!(err.code, "notfound");
    });
}

// ------------------------------------------------------------------ pure pieces

#[test]
fn cose_to_compressed_key() {
    let key = EcKey::generate(&group()).unwrap();
    let mut ctx = BigNumContext::new().unwrap();
    let un = key.public_key().to_bytes(&group(), PointConversionForm::UNCOMPRESSED, &mut ctx).unwrap();
    let co = key.public_key().to_bytes(&group(), PointConversionForm::COMPRESSED, &mut ctx).unwrap();
    let cose = |x: &[u8], y: &[u8], crv: i128| {
        Value::Map(BTreeMap::from([
            (Value::Integer(1), Value::Integer(2)),
            (Value::Integer(3), Value::Integer(-7)),
            (Value::Integer(-1), Value::Integer(crv)),
            (Value::Integer(-2), Value::Bytes(x.to_vec())),
            (Value::Integer(-3), Value::Bytes(y.to_vec())),
        ]))
    };
    assert_eq!(webauthn::compressed_from_cose(&cose(&un[1..33], &un[33..], 1)).unwrap().to_vec(), co);
    // wrong curve, off-curve point, wrong kty
    assert_eq!(webauthn::compressed_from_cose(&cose(&un[1..33], &un[33..], 2)).unwrap_err().code, "unsupported");
    let mut y = un[33..].to_vec();
    y[31] ^= 1;
    assert!(webauthn::compressed_from_cose(&cose(&un[1..33], &y, 1)).is_err());
    let okp = Value::Map(BTreeMap::from([(Value::Integer(1), Value::Integer(1)), (Value::Integer(3), Value::Integer(-8))]));
    assert!(webauthn::compressed_from_cose(&okp).is_err());
}

#[test]
fn attestation_parsing_checks_rp_and_reads_extensions_safely() {
    let soft = Soft::default();
    let mut t = soft.0.lock().unwrap();
    let mc = |rp: &str| {
        serde_cbor_2::to_vec(&Value::Map(BTreeMap::from([
            (Value::Integer(1), Value::Bytes(vec![0; 32])),
            (Value::Integer(2), Value::Map(BTreeMap::from([(text("id"), text(rp))]))),
            (Value::Integer(3), Value::Map(BTreeMap::from([(text("id"), Value::Bytes(vec![1]))]))),
            (Value::Integer(4), Value::Array(vec![Value::Map(BTreeMap::from([(text("alg"), Value::Integer(-7)), (text("type"), text("public-key"))]))])),
        ])))
        .unwrap()
    };
    let as_attestation = |v: Value| {
        let m = cbor_map(v);
        let ad = m.get(&Value::Integer(2)).unwrap().clone();
        serde_cbor_2::to_vec(&Value::Map(BTreeMap::from([(text("fmt"), text("none")), (text("authData"), ad), (text("attStmt"), Value::Map(BTreeMap::new()))]))).unwrap()
    };
    let good = as_attestation(t.handle(0x01, &mc("rare.market")).unwrap());
    let (id, pk, _) = webauthn::parse_attestation(&good).unwrap();
    assert_eq!(id.len(), 20);
    assert_eq!(pk.len(), 33);
    let other = as_attestation(t.handle(0x01, &mc("evil.example")).unwrap());
    assert!(webauthn::parse_attestation(&other).is_err());

    // extension data after the COSE key (flag ED) must not break parsing
    let m = cbor_map(serde_cbor_2::from_slice(&good).unwrap());
    let Some(Value::Bytes(mut ad)) = m.get(&text("authData")).cloned() else { panic!() };
    ad[32] |= 0x80;
    ad.extend(serde_cbor_2::to_vec(&Value::Map(BTreeMap::from([(text("credProtect"), Value::Integer(2))]))).unwrap());
    let (_, cid, _) = webauthn::parse_attested_credential(&ad).unwrap();
    assert_eq!(cid, id);
    assert!(webauthn::parse_attested_credential(&ad[..40]).is_err());
}

#[test]
fn client_data_json_is_browser_shaped() {
    let c = webauthn::client_data_json("webauthn.get", &[0xfb; 32]);
    let s = String::from_utf8(c).unwrap();
    assert!(s.starts_with(r#"{"type":"webauthn.get","challenge":""#));
    assert!(s.ends_with(r#"","origin":"https://rare.market","crossOrigin":false}"#));
    // base64url without padding, as browsers and the Sui verifier expect
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    let ch = v["challenge"].as_str().unwrap();
    assert!(ch.starts_with("-_v7") && !ch.contains('=') && !ch.contains('+') && !ch.contains('/'));
    assert_eq!(URL_SAFE_NO_PAD.decode(ch).unwrap(), vec![0xfb; 32]);
}

#[test]
fn options_mapping() {
    let r = RegisterRequest { user_id: user_handle(), user_name: "x".repeat(80), display_name: "".into(), challenge: vec![3; 32] };
    let o = webauthn::creation_options(&r);
    assert_eq!(o.rp.id, "rare.market");
    assert_eq!(o.pub_key_cred_params.len(), 1);
    assert_eq!(o.pub_key_cred_params[0].alg, -7);
    assert_eq!(o.user.id.as_slice(), &user_handle()[..]);
    assert_eq!(o.user.name.len(), 64);
    assert_eq!(o.user.display_name, o.user.name);
    let sel = o.authenticator_selection.unwrap();
    assert!(sel.require_resident_key);
    assert_eq!(sel.resident_key, Some(webauthn_rs_proto::ResidentKeyRequirement::Required));
    assert_eq!(sel.user_verification, webauthn_rs_proto::UserVerificationPolicy::Preferred);

    let a = webauthn::request_options(&AssertRequest { challenge: vec![1; 32], credential_ids: vec![vec![1], vec![2]], prf_input: None });
    assert_eq!(a.rp_id, "rare.market");
    assert_eq!(a.allow_credentials.len(), 2);
    assert_eq!(a.allow_credentials[1].id.as_slice(), &[2]);
    assert!(webauthn::request_options(&AssertRequest { challenge: vec![], credential_ids: vec![], prf_input: None }).allow_credentials.is_empty());
}

#[test]
fn error_mapping() {
    let code = |e: WebauthnCError| map_error(&e).code;
    assert_eq!(code(WebauthnCError::Cancelled), "cancelled");
    assert_eq!(code(WebauthnCError::Ctap(CtapError::Ctap2OperationDenied)), "cancelled");
    assert_eq!(code(WebauthnCError::Ctap(CtapError::Ctap2KeepAliveCancel)), "cancelled");
    assert_eq!(code(WebauthnCError::Ctap(CtapError::Ctap2UserActionTimeout)), "timeout");
    assert_eq!(code(WebauthnCError::Ctap(CtapError::Ctap2NoCredentials)), "notfound");
    assert_eq!(code(WebauthnCError::Ctap(CtapError::Ctap2PinInvalid)), "wrong-pin");
    assert_eq!(code(WebauthnCError::NoBluetoothAdapter), "ble-off");
    assert_eq!(code(WebauthnCError::BluetoothError("org.bluez.Error.NotReady: Resource Not Ready".into())), "ble-off");
    assert_eq!(code(WebauthnCError::BluetoothError("something else".into())), "error");
    assert_eq!(code(WebauthnCError::WebsocketError("dns".into())), "network");
    assert_eq!(code(WebauthnCError::NoSelectedToken), "notfound");
    assert_eq!(code(WebauthnCError::NotSupported), "unsupported");
    assert_eq!(code(WebauthnCError::UserVerificationRequired), "error");

    // the JS wire format
    assert_eq!(Fail::cancelled().to_js(), "cancelled|Passkey request cancelled");
    let u = udev_fail().to_js();
    assert!(u.starts_with("udev|") && u.contains(r#"|{"fix":"#), "{u}");
    let parsed: serde_json::Value = serde_json::from_str(&u[u.find("|{").unwrap() + 1..]).unwrap();
    assert!(parsed["fix"].as_str().unwrap().contains("ID_FIDO_TOKEN"));
}

#[test]
fn event_json_matches_native_ts() {
    let j = |e: Event| serde_json::to_value(e).unwrap();
    assert_eq!(j(Event::Qr { url: "FIDO:/123".into() }), serde_json::json!({"kind":"qr","url":"FIDO:/123"}));
    assert_eq!(j(super::ui::status("touch", "Touch")), serde_json::json!({"kind":"status","stage":"touch","message":"Touch"}));
    assert_eq!(j(Event::Pin { attempts_left: None }), serde_json::json!({"kind":"pin","attemptsLeft":null}));
    assert_eq!(j(Event::Error { code: "timeout".into(), message: "m".into() }), serde_json::json!({"kind":"error","code":"timeout","message":"m"}));

    let a = webauthn::Assertion { credential_id: "c".into(), authenticator_data: "a".into(), client_data_json: "j".into(), signature: "s".into(), user_handle: None, prf: Some("o".into()) };
    assert_eq!(
        serde_json::to_value(a).unwrap(),
        serde_json::json!({"credentialId":"c","authenticatorData":"a","clientDataJSON":"j","signature":"s","userHandle":null,"prf":"o"})
    );
    let r = webauthn::Registration { credential_id: "c".into(), public_key: "p".into(), authenticator: "phone".into(), prf: true };
    assert_eq!(serde_json::to_value(r).unwrap(), serde_json::json!({"credentialId":"c","publicKey":"p","authenticator":"phone","prf":true}));
}

#[test]
fn pin_prompt_waits_for_passkey_pin_and_dead_sessions_refuse() {
    let (ui, events, tx) = test_ui(Via::Usb);
    tx.send(Some("123456".into())).unwrap();
    assert_eq!(ui.request_pin().as_deref(), Some("123456"));
    assert!(matches!(events.lock().unwrap()[0], Event::Pin { attempts_left: None }));
    tx.send(None).unwrap();
    assert_eq!(ui.request_pin(), None, "null from passkey_pin cancels");

    let dead = Arc::new(AtomicBool::new(true));
    let (_tx2, rx2) = mpsc::channel();
    let seen = Arc::new(Mutex::new(0));
    let s2 = seen.clone();
    let quiet = Ui::new(Via::Phone, Arc::new(move |_| *s2.lock().unwrap() += 1), rx2, dead, Instant::now() + TIMEOUT);
    assert_eq!(quiet.request_pin(), None);
    quiet.cable_qr_code(webauthn_authenticator_rs::types::CableRequestType::GetAssertion, "FIDO:/1".into());
    assert_eq!(*seen.lock().unwrap(), 0, "an abandoned ceremony emits nothing");
}

#[test]
fn fido_descriptor_detection() {
    // Usage Page (FIDO Alliance 0xF1D0), Usage (CTAPHID 0x01), Collection (Application)
    assert!(hidraw::is_fido_descriptor(&[0x06, 0xD0, 0xF1, 0x09, 0x01, 0xA1, 0x01]));
    // Usage Page (Generic Desktop), Usage (Keyboard)
    assert!(!hidraw::is_fido_descriptor(&[0x05, 0x01, 0x09, 0x06, 0xA1, 0x01]));
    let fix = hidraw::udev_fix();
    assert!(!fix.contains('\n') && fix.contains("uaccess"));
}

// ------------------------------------------------------------------ caBLE QR code

/// caBLE base10: 7-byte little-endian chunks as 17 digits, a short tail as fewer digits.
fn base10_decode(s: &str) -> Vec<u8> {
    let mut out = vec![];
    let digits = s.as_bytes();
    let mut i = 0;
    while i < digits.len() {
        let n = (digits.len() - i).min(17);
        let bytes = match n {
            17 => 7,
            15 => 6,
            13 => 5,
            10 => 4,
            8 => 3,
            5 => 2,
            3 => 1,
            _ => panic!("bad base10 tail"),
        };
        let v: u64 = std::str::from_utf8(&digits[i..i + n]).unwrap().parse().unwrap();
        out.extend_from_slice(&v.to_le_bytes()[..bytes]);
        i += n;
    }
    out
}

/// Starts a real caBLE initiator and stops it at the QR code, which the library emits before it
/// touches Bluetooth (the test UI panics on the qr event and the panic is caught), then checks
/// the URL decodes the way a phone decodes it. Needs no Bluetooth and no network.
#[test]
fn cable_qr_url_shape() {
    let events = Arc::new(Mutex::new(vec![]));
    let ev = events.clone();
    let (_tx, rx) = mpsc::channel();
    let ui = Ui::new(
        Via::Phone,
        Arc::new(move |e: Event| {
            let stop = matches!(e, Event::Qr { .. });
            ev.lock().unwrap().push(e);
            if stop {
                panic!("stop at the QR code");
            }
        }),
        rx,
        Arc::new(AtomicBool::new(false)),
        Instant::now() + TIMEOUT,
    );
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        rt().block_on(webauthn_authenticator_rs::cable::connect_cable_authenticator(webauthn_authenticator_rs::types::CableRequestType::GetAssertion, &ui)).map(|_| ())
    }));
    assert!(r.is_err(), "stopped by the qr event");
    let ev = events.lock().unwrap();
    let url = ev
        .iter()
        .find_map(|e| match e {
            Event::Qr { url } => Some(url.clone()),
            _ => None,
        })
        .expect("a qr event");
    assert!(url.starts_with("FIDO:/"));
    let digits = &url["FIDO:/".len()..];
    assert!(digits.bytes().all(|b| b.is_ascii_digit()));
    let m = cbor_map(serde_cbor_2::from_slice(&base10_decode(digits)).unwrap());
    // 0: initiator's compressed P-256 identity key, 1: 16-byte QR secret, 5: request type
    assert!(matches!(m.get(&Value::Integer(0)), Some(Value::Bytes(b)) if b.len() == 33));
    assert!(matches!(m.get(&Value::Integer(1)), Some(Value::Bytes(b)) if b.len() == 16));
    assert_eq!(m.get(&Value::Integer(5)), Some(&text("ga")));
}

#[test]
fn tunnel_domains() {
    // The phone picks the relay: 0 is Google (Android), 1 is Apple (iOS).
    assert_eq!(webauthn_authenticator_rs::cable::get_domain(0).as_deref(), Some("cable.ua5v.com"));
    assert_eq!(webauthn_authenticator_rs::cable::get_domain(1).as_deref(), Some("cable.auth.com"));
}

#[test]
fn cancel_stops_an_endless_wait() {
    // e.g. waiting for a phone's Bluetooth advert that never comes
    let dead = Arc::new(AtomicBool::new(false));
    let d2 = dead.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        d2.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    let t = Instant::now();
    let r: Result<(), Fail> = rt().block_on(or_dead(&dead, std::future::pending()));
    assert_eq!(r.unwrap_err().code, "cancelled");
    assert!(t.elapsed() < std::time::Duration::from_secs(2));
}

// ------------------------------------------------------------------ PRF / hmac-secret

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

/// Vectors from python-fido2 1.2.0 (Yubico's reference CTAP client: PinProtocolV1/V2 and
/// `_prf_salt`), platform key d = 0x11..11, authenticator key d = 0x22..22, CredRandom 00..1f,
/// PRF input SHA-256("rare/cloud-save"), protocol 2 IV 64..73.
mod vectors {
    pub const AUTH_X: &str = "d65a93977caa3d1b081852ff57a79e465f1660577304baead505dd3a48589cf3";
    pub const AUTH_Y: &str = "50185e895372df6221ea3a137557e473fddb6755f05bd507c3c533fce9c91285";
    pub const PLAT_X: &str = "0217e617f0b6443928278f96999e69a23a4f2c152bdf6d6cdf66e5b80282d4ed";
    pub const PLAT_Y: &str = "194a7debcb97712d2dda3ca85aa8765a56f45fc758599652f2897c65306e5794";
    pub const PRF_INPUT: &str = "adb2fb0ddd91313471b538b6fb168c5a8e22d4d7caf6b842d2f6350ce1acc47d";
    pub const SALT: &str = "7f8501bdda5b0bbaeb9ca1bcebfdef8e2a5570d4e3c30f4239b442b73d2ac48c";
    pub const OUTPUT: &str = "1e0719fea9076fe1913f29a53b8834ba65c1060952c7e2e4862a32fe4b43167b";
    pub const V1_SHARED: &str = "98f5bf15dbc72627acd9a8ab61ce21349ea3496d79136adf08ad381cc9c0fa25";
    pub const V1_SALT_ENC: &str = "a850adbae84a78814315aa7470a6cf5df1448c0f691b0e854d8968bc7ecac354";
    pub const V1_SALT_AUTH: &str = "369ab16645f3d52a01e1f6502a8b9ce1";
    pub const V1_OUTPUT_ENC: &str = "0f0ee92fc0e1d5f3ccff7ad13b69fbbd81a6cb46c7505c4b03191a5d885a5da6";
    pub const V2_SHARED: &str = "abc3809533cb07b6ae481ea05b28ac708ffed9777147a09aabc8ab58e167ba0eaecad75245932defecf4cd0d93420fcd05d99403d09a49655fed76fa64c2cf82";
    pub const V2_SALT_ENC: &str = "6465666768696a6b6c6d6e6f7071727386138717dddab79ee497c70d9d25cd7856893ce5ed57018e28438ad03742f27c";
    pub const V2_SALT_AUTH: &str = "8bc29a37871164023d20a23b7ac27529048ec0f1cc2eae8637400bd493a290dd";
    pub const V2_OUTPUT_ENC: &str = "6465666768696a6b6c6d6e6f70717273620b95f2affd97e005cd87342f14c3f8a58a8dbee40569128c0d399db6b35e11";
}

#[test]
fn prf_salt_matches_webauthn() {
    use vectors::*;
    assert_eq!(hex(PRF_INPUT), webauthn::sha256(b"rare/cloud-save"));
    assert_eq!(prf::hmac_salt(&hex(PRF_INPUT)).to_vec(), hex(SALT));
    // and the PRF output is HMAC-SHA-256(CredRandom, salt)
    assert_eq!(soft_hmac(&(0..32).collect::<Vec<u8>>(), &hex(SALT)), hex(OUTPUT));
}

#[test]
fn hmac_secret_protocol_pieces_match_python_fido2() {
    use vectors::*;
    let platform = || {
        let d = openssl::bn::BigNum::from_slice(&[0x11; 32]).unwrap();
        let mut ctx = BigNumContext::new().unwrap();
        let mut pt = EcPoint::new(&group()).unwrap();
        pt.mul_generator2(&group(), &d, &mut ctx).unwrap();
        EcKey::from_private_components(&group(), &d, &pt).unwrap()
    };
    let salt: [u8; 32] = hex(SALT).try_into().unwrap();
    let iv: [u8; 16] = (100u8..116).collect::<Vec<_>>().try_into().unwrap();
    for (proto, shared, salt_enc, salt_auth, out_enc) in [(1, V1_SHARED, V1_SALT_ENC, V1_SALT_AUTH, V1_OUTPUT_ENC), (2, V2_SHARED, V2_SALT_ENC, V2_SALT_AUTH, V2_OUTPUT_ENC)] {
        let hs = prf::HmacSecret::with_key(proto, platform(), &hex(AUTH_X), &hex(AUTH_Y)).unwrap();
        assert_eq!(hs.shared(), &hex(shared)[..], "key agreement v{proto}");
        assert_eq!(hs.platform_xy(), (&hex(PLAT_X)[..], &hex(PLAT_Y)[..]));
        let input = cbor_map(hs.extension_input_with_iv(&salt, iv).unwrap());
        assert_eq!(input.get(&Value::Integer(2)), Some(&Value::Bytes(hex(salt_enc))), "saltEnc v{proto}");
        assert_eq!(input.get(&Value::Integer(3)), Some(&Value::Bytes(hex(salt_auth))), "saltAuth v{proto}");
        assert_eq!(input.contains_key(&Value::Integer(4)), proto == 2);
        let key = cbor_map(input.get(&Value::Integer(1)).cloned().unwrap());
        assert_eq!(key.get(&Value::Integer(-2)), Some(&Value::Bytes(hex(PLAT_X))));
        assert_eq!(hs.decrypt_output(&hex(out_enc)).unwrap().to_vec(), hex(OUTPUT), "output v{proto}");
    }
    // a key agreement point off the curve is refused
    let mut y = hex(AUTH_Y);
    y[31] ^= 1;
    assert!(prf::HmacSecret::new(2, &hex(AUTH_X), &y).is_err());
}

#[test]
fn prf_form_follows_chromium() {
    let e = |v: &[&str]| Some(v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(prf::form(e(&["devicePubKey", "prf"]).as_ref(), None), prf::Form::Prf);
    assert_eq!(prf::form(e(&["prf", "hmac-secret"]).as_ref(), Some(&vec![2, 1])), prf::Form::Prf);
    assert_eq!(prf::form(e(&["hmac-secret"]).as_ref(), Some(&vec![1, 2])), prf::Form::HmacSecret(2));
    assert_eq!(prf::form(e(&["hmac-secret"]).as_ref(), Some(&vec![1])), prf::Form::HmacSecret(1));
    // hybrid phones have no PIN/UV auth protocol: no hmac-secret without one
    assert_eq!(prf::form(e(&["hmac-secret"]).as_ref(), None), prf::Form::None);
    assert_eq!(prf::form(None, Some(&vec![2])), prf::Form::None);
}

struct PrfRun {
    /// registration said PRF is on
    enabled: bool,
    /// outputs (base64) for input A and input B
    a: Option<String>,
    b: Option<String>,
    cred_random: [u8; 32],
    soft: Soft,
}

/// Registers, then evaluates PRF for two inputs (input A twice, by id and discoverable).
fn soft_prf_run(ext: SoftExt, via: Via) -> PrfRun {
    let soft = Soft::default();
    soft.0.lock().unwrap().ext = ext;
    let (ui, _e, _p) = test_ui(via);
    let input_a: [u8; 32] = webauthn::sha256(b"rare/cloud-save").try_into().unwrap();
    let input_b: [u8; 32] = webauthn::sha256(b"rare/other").try_into().unwrap();
    let r = rt().block_on(async {
        let mut auth = CtapAuthenticator::new(soft.clone(), &ui).await.unwrap();
        let reg = registered(
            perform(&mut auth, &Op::Register(RegisterRequest { user_id: user_handle(), user_name: "a".into(), display_name: "a".into(), challenge: vec![1; 32] }), via).unwrap(),
        );
        let id = STANDARD.decode(&reg.credential_id).unwrap();
        let mut run = |input: Option<[u8; 32]>, ids: Vec<Vec<u8>>| {
            let a = asserted(perform(&mut auth, &Op::Assert(AssertRequest { challenge: vec![9; 32], credential_ids: ids, prf_input: input }), via).unwrap());
            assert!(verify(&reg.public_key, &a), "the signature still verifies with extensions");
            a.prf
        };
        let a1 = run(Some(input_a), vec![id.clone()]);
        let a2 = run(Some(input_a), vec![]); // discoverable too
        let b = run(Some(input_b), vec![id.clone()]);
        assert_eq!(run(None, vec![id]), None, "no input, no output");
        assert_eq!(a1, a2, "same input, same output");
        (reg.prf, a1, b)
    });
    let cred_random = soft.0.lock().unwrap().creds[0].1.cred_random;
    PrfRun { enabled: r.0, a: r.1, b: r.2, cred_random, soft }
}

#[test]
fn prf_over_hybrid_form() {
    let PrfRun { enabled, a, b, cred_random, soft } = soft_prf_run(SoftExt::Prf, Via::Phone);
    assert!(enabled);
    let a = STANDARD.decode(a.expect("prf output")).unwrap();
    assert_eq!(a.len(), 32);
    assert_ne!(Some(STANDARD.encode(&a)), b, "different inputs differ");
    // what the authenticator HMACed is the WebAuthn-hashed salt, so a browser gets the same value
    let salt = prf::hmac_salt(&webauthn::sha256(b"rare/cloud-save"));
    assert_eq!(a, soft_hmac(&cred_random, &salt));
    let t = soft.0.lock().unwrap();
    // the wire shape Chromium uses: MC {"prf": {}}, GA {"prf": {"eval": {"first": salt}}}
    assert_eq!(t.last_mc_ext, Some(Value::Map(BTreeMap::from([(text("prf"), Value::Map(BTreeMap::new()))]))));
    let ga = cbor_map(t.last_ga_ext.clone().unwrap_or(Value::Map(BTreeMap::new())));
    assert!(ga.is_empty(), "the last assertion had no PRF input");
}

#[test]
fn prf_over_usb_hmac_secret_v2_and_v1() {
    for protocols in [vec![2, 1], vec![1]] {
        let PrfRun { enabled, a, b, cred_random, soft } = soft_prf_run(SoftExt::Hmac(protocols.clone()), Via::Usb);
        assert!(enabled, "{protocols:?}");
        let a = STANDARD.decode(a.expect("prf output")).unwrap();
        let salt = prf::hmac_salt(&webauthn::sha256(b"rare/cloud-save"));
        assert_eq!(a, soft_hmac(&cred_random, &salt), "decrypted output, {protocols:?}");
        assert_ne!(Some(STANDARD.encode(&a)), b);
        let t = soft.0.lock().unwrap();
        assert_eq!(t.last_mc_ext, Some(Value::Map(BTreeMap::from([(text("hmac-secret"), Value::Bool(true))]))));
    }
}

#[test]
fn no_prf_support_gives_no_output() {
    let PrfRun { enabled, a, b, .. } = soft_prf_run(SoftExt::None, Via::Usb);
    assert!(!enabled);
    assert_eq!((a, b), (None, None));
    assert_eq!(prf::no_prf().code, "no-prf");
}
