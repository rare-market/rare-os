//! Rare device attestation (docs/ATTEST.md).
//!
//! [`attest`] proves to rare.market's attestor that this device is a genuine SuiPlay running Rare
//! OS, and returns:
//! - a short-lived signed device claim ([`Attestation::token`], "rda1.…") any Rare service can
//!   check (trophies "verified on SuiPlay", for example);
//! - the Device Pass offer ([`PassOffer`]): a voucher for `device_pass::claim`, or the pass this
//!   account already holds.
//!
//! The steps: transient EK + AK in the TPM, POST /v1/device/attest/start (EK and AK public; the
//! attestor checks the EK certificate chain to AMD's root and answers a MakeCredential blob),
//! ActivateCredential (proves the AK sits in that TPM), a quote of PCR 0, 4 and 7 over the
//! attestor's nonce, then POST /v1/device/attest/finish. Blocking; a few seconds on the SuiPlay.
//! Nothing is kept on the device or in the TPM.

pub mod tpm;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use std::time::Duration;
pub use tpm::{status, TpmStatus};

/// Where the attestor answers.
#[derive(Clone, Debug)]
pub struct Endpoint {
    /// "https://rare.market" (or the testnet API base from Settings > Developer).
    pub base: String,
    /// Testnet: adds `?network=testnet` (the Device Pass registry on testnet).
    pub testnet: bool,
}

impl Endpoint {
    pub fn rare_market(testnet: bool) -> Endpoint {
        Endpoint { base: "https://rare.market".into(), testnet }
    }
    fn url(&self, step: &str) -> String {
        let mut u = format!("{}/v1/device/attest/{step}", self.base.trim_end_matches('/'));
        if self.testnet {
            u.push_str("?network=testnet");
        }
        u
    }
}

/// What to attest for.
#[derive(Clone, Debug)]
pub struct Request {
    /// The Rare account (Sui address) the claim and the pass are for.
    pub address: String,
    /// Device id audience for the claim token. None = "rare" (the id the Device Pass uses).
    pub aud: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attestation {
    /// Signed device claim, valid ~10 minutes. Check it with POST /v1/device/attest/verify or the
    /// attestor key (GET /v1/device/attest/key).
    pub token: String,
    /// This device's Rare id (hex): a salted hash of the TPM's EK, never the EK itself.
    pub device: String,
    /// "SuiPlay"
    pub model: String,
    /// Firmware build, e.g. "SuiPlay0X1 BIOS 2.19_S20 (2025-02-19)".
    pub firmware: String,
    /// "firmware" | "rare-os" | "rare-os-secure"
    pub level: String,
    pub secure_boot: bool,
    /// When Rare first saw this device (unix ms).
    pub first_seen_ms: u64,
    pub expires_ms: u64,
    pub pass: PassOffer,
}

/// The Device Pass side of an attestation.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum PassOffer {
    /// Claimable: call `device_pass::claim` with these, as the attested account.
    #[serde(rename_all = "camelCase")]
    New(Voucher),
    /// The device has a pass on another account: claiming moves it here (a rebind).
    #[serde(rename_all = "camelCase")]
    Rebind(Voucher),
    /// This account already holds the device's current pass.
    #[serde(rename_all = "camelCase")]
    Held { network: String, package: String, registry: String, number: u64, pass: String },
    /// No pass right now: "not_deployed" | "level" | "paused".
    #[serde(rename_all = "camelCase")]
    Unavailable { reason: String },
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Voucher {
    pub network: String,
    pub package: String,
    pub registry: String,
    /// hex, 32 bytes
    pub device: String,
    pub model: String,
    pub first_seen_ms: u64,
    /// decimal u64
    pub seq: String,
    pub expires_ms: u64,
    /// Ed25519 signature, hex
    pub signature: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AttestError {
    /// This device can't attest (no TPM, no access, no tools).
    Unavailable(TpmStatus),
    /// A TPM step failed.
    Tpm(String),
    /// The attestor said no: HTTP status, code ("unknown_firmware", "untrusted_tpm", ...), message.
    Refused { status: u16, code: String, message: String },
    /// Could not reach the attestor.
    Network(String),
}

impl AttestError {
    /// One plain sentence for the UI.
    pub fn message(&self) -> String {
        match self {
            AttestError::Unavailable(TpmStatus::NoTpm) => "This device has no TPM, so it can't be verified.".into(),
            AttestError::Unavailable(TpmStatus::NoAccess) => "Rare can't reach this device's TPM. Update Rare OS and try again.".into(),
            AttestError::Unavailable(TpmStatus::NoTools) => "This system is missing the TPM tools. Update Rare OS and try again.".into(),
            AttestError::Unavailable(TpmStatus::Ready) => "Try again.".into(),
            AttestError::Tpm(_) => "The TPM check didn't finish. Try again.".into(),
            AttestError::Refused { code, message, .. } => match code.as_str() {
                "unknown_firmware" => "This device's firmware isn't a known SuiPlay firmware.".into(),
                "untrusted_tpm" | "no_ek_cert" => "This device's TPM couldn't be verified.".into(),
                "rate_limited" => "Too many checks in the last hour. Try again later.".into(),
                _ => message.clone(),
            },
            AttestError::Network(_) => "Couldn't reach rare.market. Check your connection.".into(),
        }
    }
}

impl std::fmt::Display for AttestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttestError::Tpm(e) | AttestError::Network(e) => write!(f, "{}: {e}", self.message()),
            AttestError::Refused { status, code, message } => write!(f, "{status} {code}: {message}"),
            _ => f.write_str(&self.message()),
        }
    }
}

impl std::error::Error for AttestError {}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Started {
    session: String,
    credential: String,
    nonce: String,
    pcrs: Vec<u32>,
}

fn post<T: serde::de::DeserializeOwned>(url: &str, body: serde_json::Value) -> Result<T, AttestError> {
    let req = ureq::post(url).set("User-Agent", "RareLauncher/0.1").timeout(Duration::from_secs(25));
    match req.send_json(body) {
        Ok(r) => r.into_json().map_err(|e| AttestError::Network(e.to_string())),
        Err(ureq::Error::Status(status, r)) => {
            let v: serde_json::Value = r.into_json().unwrap_or_default();
            let e = &v["error"];
            Err(AttestError::Refused { status, code: e["code"].as_str().unwrap_or("error").into(), message: e["message"].as_str().unwrap_or("The attestor refused this device.").into() })
        }
        Err(e) => Err(AttestError::Network(e.to_string())),
    }
}

/// Runs the whole attestation. Blocking (a few seconds): call it off the UI thread.
pub fn attest(ep: &Endpoint, req: &Request) -> Result<Attestation, AttestError> {
    match tpm::status() {
        TpmStatus::Ready => {}
        s => return Err(AttestError::Unavailable(s)),
    }
    let s = tpm::Session::new().map_err(AttestError::Tpm)?;
    let (ek, ak) = s.keys().map_err(AttestError::Tpm)?;
    let mut start = serde_json::json!({ "ekPub": B64.encode(&ek), "akPub": B64.encode(&ak), "address": req.address, "network": if ep.testnet { "testnet" } else { "mainnet" } });
    if let Some(cert) = s.ek_cert() {
        start["ekCert"] = B64.encode(cert).into();
    }
    if let Some(aud) = &req.aud {
        start["aud"] = aud.clone().into();
    }
    let st: Started = post(&ep.url("start"), start)?;
    let cred = B64.decode(&st.credential).map_err(|e| AttestError::Network(e.to_string()))?;
    let secret = s.activate(&cred).map_err(AttestError::Tpm)?;
    let q = s.quote(&st.pcrs, &st.nonce).map_err(AttestError::Tpm)?;
    let pcrs: serde_json::Map<String, serde_json::Value> = q.pcrs.iter().map(|(i, v)| (i.to_string(), v.clone().into())).collect();
    let finish = serde_json::json!({ "session": st.session, "secret": B64.encode(&secret), "quote": B64.encode(&q.attest), "signature": B64.encode(&q.signature), "pcrs": pcrs });
    post(&ep.url("finish"), finish)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_answer_parses() {
        let j = r#"{"token":"rda1.x.y","device":"ab","model":"SuiPlay","firmware":"fw","level":"rare-os","secureBoot":false,"firstSeenMs":5,"expiresMs":9,
          "pass":{"status":"new","network":"testnet","package":"0x1","registry":"0x2","device":"ab","model":"SuiPlay","firstSeenMs":5,"seq":"0","expiresMs":9,"signature":"cd"}}"#;
        let a: Attestation = serde_json::from_str(j).unwrap();
        assert!(matches!(a.pass, PassOffer::New(ref v) if v.seq == "0" && v.registry == "0x2"));
        let held: PassOffer = serde_json::from_str(r#"{"status":"held","network":"testnet","package":"0x1","registry":"0x2","number":3,"pass":"0x9"}"#).unwrap();
        assert!(matches!(held, PassOffer::Held { number: 3, .. }));
        let un: PassOffer = serde_json::from_str(r#"{"status":"unavailable","reason":"level"}"#).unwrap();
        assert_eq!(un, PassOffer::Unavailable { reason: "level".into() });
    }

    #[test]
    fn testnet_urls_carry_the_network() {
        let ep = Endpoint { base: "https://rare.market/".into(), testnet: true };
        assert_eq!(ep.url("start"), "https://rare.market/v1/device/attest/start?network=testnet");
        assert_eq!(Endpoint::rare_market(false).url("finish"), "https://rare.market/v1/device/attest/finish");
    }
}
