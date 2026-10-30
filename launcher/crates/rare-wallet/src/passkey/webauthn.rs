//! The browser half of WebAuthn that WebKitGTK cannot do for us: building clientDataJSON and the
//! CTAP2 options, and reading the credential out of an attestation. Pure functions, no hardware.

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use base64urlsafedata::Base64UrlSafeData;
use openssl::bn::BigNumContext;
use openssl::ec::{EcGroup, EcPoint, PointConversionForm};
use openssl::nid::Nid;
use serde::Serialize;
use serde_cbor_2::Value;
use webauthn_rs_proto::{
    AllowCredentials, AttestationConveyancePreference, AuthenticatorSelectionCriteria, PubKeyCredParams,
    PublicKeyCredentialCreationOptions, PublicKeyCredentialRequestOptions, RelyingParty, ResidentKeyRequirement, User,
    UserVerificationPolicy,
};

use super::Fail;

/// Every Rare passkey belongs to rare.market, so the same passkey works on any SuiPlay and,
/// later, on rare.market itself.
pub const RP_ID: &str = "rare.market";
pub const RP_NAME: &str = "Rare";
pub const ORIGIN: &str = "https://rare.market";
/// COSE ES256 (ECDSA P-256 + SHA-256), the only curve Sui passkeys use.
pub const ES256: i64 = -7;

pub struct RegisterRequest {
    pub user_id: Vec<u8>,
    pub user_name: String,
    pub display_name: String,
    pub challenge: Vec<u8>,
}

pub struct AssertRequest {
    pub challenge: Vec<u8>,
    /// empty: discoverable credential (the authenticator picks, and returns the user handle)
    pub credential_ids: Vec<Vec<u8>>,
    /// WebAuthn PRF input (`prf.eval.first`) to evaluate along with the assertion
    pub prf_input: Option<[u8; 32]>,
}

/// `PasskeyRegistration` in native.ts.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Registration {
    pub credential_id: String,
    pub public_key: String,
    pub authenticator: String,
    /// the passkey can evaluate PRF (WebAuthn prf / CTAP hmac-secret)
    pub prf: bool,
}

/// `PasskeyAssertion` in native.ts.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Assertion {
    pub credential_id: String,
    pub authenticator_data: String,
    #[serde(rename = "clientDataJSON")]
    pub client_data_json: String,
    pub signature: String,
    pub user_handle: Option<String>,
    /// base64 32-byte PRF output when a PRF input was given and the passkey supports it
    pub prf: Option<String>,
}

pub fn b64(b: &[u8]) -> String {
    STANDARD.encode(b)
}

pub fn unb64(field: &str, s: &str) -> Result<Vec<u8>, Fail> {
    STANDARD.decode(s.trim()).map_err(|_| Fail::new("error", format!("Bad {field}")))
}

/// clientDataJSON the way browsers write it. The authenticator signs sha256 of these exact
/// bytes, and Sui checks `type` and `challenge` (base64url of the intent digest) in them.
pub fn client_data_json(ty: &str, challenge: &[u8]) -> Vec<u8> {
    format!(
        r#"{{"type":"{ty}","challenge":"{}","origin":"{ORIGIN}","crossOrigin":false}}"#,
        URL_SAFE_NO_PAD.encode(challenge)
    )
    .into_bytes()
}

pub fn sha256(b: &[u8]) -> Vec<u8> {
    openssl::sha::sha256(b).to_vec()
}

/// CTAP2 caps user names at 64 bytes; cut on a char boundary.
fn clamp(s: &str) -> String {
    let s = s.trim();
    if s.len() <= 64 {
        return s.to_string();
    }
    let mut end = 64;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

pub fn creation_options(r: &RegisterRequest) -> PublicKeyCredentialCreationOptions {
    let name = clamp(if r.user_name.trim().is_empty() { "Rare Wallet" } else { &r.user_name });
    let display = clamp(if r.display_name.trim().is_empty() { &name } else { &r.display_name });
    PublicKeyCredentialCreationOptions {
        rp: RelyingParty { name: RP_NAME.into(), id: RP_ID.into() },
        user: User { id: Base64UrlSafeData::from(r.user_id.clone()), name, display_name: display },
        challenge: Base64UrlSafeData::from(r.challenge.clone()),
        pub_key_cred_params: vec![PubKeyCredParams { type_: "public-key".into(), alg: ES256 }],
        timeout: Some(super::TIMEOUT.as_millis() as u32),
        exclude_credentials: None,
        authenticator_selection: Some(AuthenticatorSelectionCriteria {
            authenticator_attachment: None,
            resident_key: Some(ResidentKeyRequirement::Required),
            require_resident_key: true,
            user_verification: UserVerificationPolicy::Preferred,
        }),
        hints: None,
        attestation: Some(AttestationConveyancePreference::None),
        attestation_formats: None,
        extensions: None,
    }
}

pub fn request_options(r: &AssertRequest) -> PublicKeyCredentialRequestOptions {
    PublicKeyCredentialRequestOptions {
        challenge: Base64UrlSafeData::from(r.challenge.clone()),
        timeout: Some(super::TIMEOUT.as_millis() as u32),
        rp_id: RP_ID.into(),
        allow_credentials: r
            .credential_ids
            .iter()
            .map(|id| AllowCredentials { type_: "public-key".into(), id: Base64UrlSafeData::from(id.clone()), transports: None })
            .collect(),
        user_verification: UserVerificationPolicy::Preferred,
        hints: None,
        extensions: None,
    }
}

/// Credential id, compressed 33-byte P-256 public key, raw authenticatorData.
pub type Attested = (Vec<u8>, [u8; 33], Vec<u8>);

/// From the attestation object (CBOR map with "authData"): credential id, the compressed
/// 33-byte P-256 public key, and the raw authenticatorData.
pub fn parse_attestation(attestation_object: &[u8]) -> Result<Attested, Fail> {
    let bad = |m: &str| Fail::new("error", format!("Unexpected passkey response ({m})"));
    let v: Value = serde_cbor_2::from_slice(attestation_object).map_err(|_| bad("cbor"))?;
    let Value::Map(m) = v else { return Err(bad("not a map")) };
    let auth_data = match m.get(&Value::Text("authData".into())) {
        Some(Value::Bytes(b)) => b.clone(),
        _ => return Err(bad("no authData")),
    };
    let (rp_hash, cred_id, cose) = parse_attested_credential(&auth_data)?;
    if rp_hash != sha256(RP_ID.as_bytes()) {
        return Err(bad("rp id"));
    }
    let pk = compressed_from_cose(&cose)?;
    Ok((cred_id, pk, auth_data))
}

/// authenticatorData = rpIdHash(32) flags(1) signCount(4) [aaguid(16) len(2) credId coseKey] [ext]
pub fn parse_attested_credential(auth_data: &[u8]) -> Result<(Vec<u8>, Vec<u8>, Value), Fail> {
    let bad = |m: &str| Fail::new("error", format!("Unexpected passkey response ({m})"));
    if auth_data.len() < 37 + 18 {
        return Err(bad("short authData"));
    }
    let flags = auth_data[32];
    if flags & 0x40 == 0 {
        return Err(bad("no credential"));
    }
    let len = u16::from_be_bytes([auth_data[53], auth_data[54]]) as usize;
    let start = 55;
    if auth_data.len() < start + len + 1 {
        return Err(bad("short credential"));
    }
    let cred_id = auth_data[start..start + len].to_vec();
    // The COSE key is followed by extensions when flag ED is set, so read one CBOR item only.
    let mut de = serde_cbor_2::Deserializer::from_slice(&auth_data[start + len..]);
    let cose: Value = serde::Deserialize::deserialize(&mut de).map_err(|_| bad("cose"))?;
    Ok((auth_data[..32].to_vec(), cred_id, cose))
}

/// COSE_Key {1: 2 (EC2), 3: -7, -1: 1 (P-256), -2: x, -3: y} -> 0x02/0x03 || x, checked on-curve.
pub fn compressed_from_cose(cose: &Value) -> Result<[u8; 33], Fail> {
    let bad = |m: &str| Fail::new("unsupported", format!("This passkey type is not supported ({m})"));
    let Value::Map(m) = cose else { return Err(bad("key")) };
    let int = |k: i128| m.get(&Value::Integer(k));
    if int(1) != Some(&Value::Integer(2)) {
        return Err(bad("not EC2"));
    }
    if let Some(alg) = int(3) {
        if alg != &Value::Integer(ES256 as i128) {
            return Err(bad("not ES256"));
        }
    }
    if int(-1) != Some(&Value::Integer(1)) {
        return Err(bad("not P-256"));
    }
    let (Some(Value::Bytes(x)), Some(Value::Bytes(y))) = (int(-2), int(-3)) else { return Err(bad("coordinates")) };
    if x.len() != 32 || y.len() != 32 {
        return Err(bad("coordinates"));
    }
    let mut uncompressed = Vec::with_capacity(65);
    uncompressed.push(0x04);
    uncompressed.extend_from_slice(x);
    uncompressed.extend_from_slice(y);
    compress_p256(&uncompressed).ok_or_else(|| bad("not on curve"))
}

/// Uncompressed SEC1 point -> compressed, via OpenSSL so an off-curve point is rejected.
pub fn compress_p256(uncompressed: &[u8]) -> Option<[u8; 33]> {
    let group = EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).ok()?;
    let mut ctx = BigNumContext::new().ok()?;
    let p = EcPoint::from_bytes(&group, uncompressed, &mut ctx).ok()?;
    let c = p.to_bytes(&group, PointConversionForm::COMPRESSED, &mut ctx).ok()?;
    c.try_into().ok()
}
