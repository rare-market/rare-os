//! Passkey ceremonies for Rare Wallet. WebKitGTK has no platform authenticator on Linux, so the
//! launcher is the WebAuthn client itself: CTAP2 to a phone over hybrid/caBLE (QR code, a BLE
//! beacon from the phone, then a websocket tunnel through Google's or Apple's relay) or to a USB
//! security key over CTAP HID. The relying party is always rare.market.
//!
//! Each ceremony runs on its own thread with its own tokio runtime: the CTAP library blocks
//! inside `perform_register` / `perform_auth`, and must not stall Tauri's async workers.
//! Cancel and the 2 minute timeout answer JS at once; an abandoned ceremony thread goes quiet
//! (no events, PIN prompts refused) and ends when the authenticator gives up.

mod hidraw;
pub mod prf;
pub mod ui;
pub mod webauthn;
#[cfg(test)]
mod tests;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use webauthn_authenticator_rs::ctap2::{select_one_device, CtapAuthenticator};
use webauthn_authenticator_rs::error::{CtapError, WebauthnCError};
use webauthn_authenticator_rs::transport::{Token, TokenEvent, Transport};
use webauthn_authenticator_rs::types::CableRequestType;

use self::ui::{status, Ui};
pub use self::ui::Event;
use crate::{Sink, WalletEvent};
use self::webauthn::{b64, unb64, AssertRequest, Assertion, RegisterRequest, Registration};

pub const TIMEOUT: Duration = Duration::from_secs(120);
/// How long a USB ceremony waits for a key to be plugged in.
const PLUG_WAIT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Phone,
    Usb,
}

impl Via {
    fn parse(s: &str) -> Result<Self, Fail> {
        match s {
            "phone" => Ok(Via::Phone),
            "usb" => Ok(Via::Usb),
            _ => Err(Fail::new("error", "Unknown passkey method")),
        }
    }
    fn authenticator(self) -> &'static str {
        match self {
            Via::Phone => "phone",
            Via::Usb => "security-key",
        }
    }
}

/// An error for JS: "code|message" or "code|message|{json}".
#[derive(Debug, Clone, PartialEq)]
pub struct Fail {
    pub code: &'static str,
    pub msg: String,
    pub data: Option<serde_json::Value>,
}

impl Fail {
    pub fn new(code: &'static str, msg: impl Into<String>) -> Self {
        Fail { code, msg: msg.into(), data: None }
    }
    /// "code|message[|{json}]", the error string both front ends parse.
    pub fn to_js(&self) -> String {
        match &self.data {
            Some(d) => crate::err_data(self.code, &self.msg, d.clone()),
            None => crate::err(self.code, &self.msg),
        }
    }
    fn cancelled() -> Self {
        Fail::new("cancelled", "Passkey request cancelled")
    }
    fn timeout() -> Self {
        Fail::new("timeout", "The passkey request timed out")
    }
}

/// Library errors to the codes the UI branches on (see PasskeyNative in native.ts).
pub fn map_error(e: &WebauthnCError) -> Fail {
    use CtapError::*;
    use WebauthnCError as W;
    match e {
        W::Cancelled => Fail::cancelled(),
        W::Ctap(c) => match c {
            Ctap2OperationDenied | Ctap2KeepAliveCancel | Ctap2NotAllowed => Fail::cancelled(),
            Ctap2UserActionTimeout | Ctap2ActionTimeout | Ctap1Timeout => Fail::timeout(),
            Ctap2NoCredentials | Ctap2InvalidCredential => Fail::new("notfound", "No Rare passkey on this phone or key"),
            Ctap2PinInvalid => Fail::new("wrong-pin", "Wrong security key PIN"),
            Ctap2PinBlocked | Ctap2PinAuthBlocked => Fail::new("error", "The security key PIN is blocked. Reset the key or use another."),
            Ctap2PinNotSet | Ctap2PUATRequired => Fail::new("error", "Set a PIN on your security key first"),
            Ctap2UnsupportedAlgorithm => Fail::new("unsupported", "This security key cannot make this kind of passkey"),
            Ctap2KeyStoreFull => Fail::new("error", "The security key is full"),
            Ctap2CredentialExcluded => Fail::new("error", "This passkey already exists on the key"),
            _ => Fail::new("error", "The passkey request failed"),
        },
        W::NoBluetoothAdapter => Fail::new("ble-off", "Turn on Bluetooth"),
        W::BluetoothError(s) if is_powered_off(s) => Fail::new("ble-off", "Turn on Bluetooth"),
        W::BluetoothError(_) => Fail::new("error", "Bluetooth error. Try again."),
        W::PermissionDenied => Fail::new("ble-off", "Rare Launcher cannot use Bluetooth"),
        W::WebsocketError(_) | W::IoError(_) | W::Closed => Fail::new("network", "Could not reach your phone. Check the internet connection on both devices."),
        W::NoSelectedToken | W::NoHidDevices => Fail::new("notfound", "No security key found"),
        W::UserVerificationRequired => Fail::new("error", "Set a PIN on your security key first"),
        W::NotSupported | W::InvalidAlgorithm => Fail::new("unsupported", "This authenticator is not supported"),
        W::PinTooShort | W::PinTooLong | W::PinContainsNull => Fail::new("wrong-pin", "That PIN is not valid for this key"),
        W::HidError(_) => Fail::new("error", "Lost the security key. Plug it in and try again."),
        _ => Fail::new("error", "The passkey request failed"),
    }
}

fn is_powered_off(s: &str) -> bool {
    let s = s.to_ascii_lowercase();
    s.contains("notready") || s.contains("not ready") || s.contains("powered") || s.contains("rfkill")
}

// ------------------------------------------------------------------ the ceremony itself

pub enum Op {
    Register(RegisterRequest),
    Assert(AssertRequest),
}

#[derive(Serialize, Debug)]
#[serde(untagged)]
pub enum Outcome {
    Registered(Registration),
    Asserted(Assertion),
}

/// Runs one WebAuthn operation on a connected authenticator. We build clientDataJSON ourselves
/// and hand the authenticator only its hash, so the bytes we return are exactly what it signed.
/// PRF rides along in the CTAP form the authenticator advertises (see prf.rs).
pub fn perform<T: Token>(auth: &mut CtapAuthenticator<'_, T, Ui>, op: &Op, via: Via) -> Result<Outcome, Fail> {
    let form = {
        let info = auth.get_info();
        prf::form(info.extensions.as_ref(), info.pin_protocols.as_ref())
    };
    match op {
        Op::Register(r) => {
            let cdj = webauthn::client_data_json("webauthn.create", &r.challenge);
            let (cred, unsigned) = auth
                .perform_register_ext(webauthn::sha256(&cdj), webauthn::creation_options(r), prf::registration_extensions(form))
                .map_err(|e| map_error(&e))?;
            let (cred_id, pk, auth_data) = webauthn::parse_attestation(cred.response.attestation_object.as_slice())?;
            let prf = prf::registration_enabled(unsigned.as_ref(), prf::auth_data_extensions(&auth_data).as_ref());
            Ok(Outcome::Registered(Registration { credential_id: b64(&cred_id), public_key: b64(&pk), authenticator: via.authenticator().into(), prf }))
        }
        Op::Assert(a) => {
            let cdj = webauthn::client_data_json("webauthn.get", &a.challenge);
            let salt = a.prf_input.map(|i| prf::hmac_salt(&i));
            let key_agreement = match (salt, form) {
                (Some(_), prf::Form::HmacSecret(p)) => Some(p),
                _ => None,
            };
            let mut session: Option<prf::HmacSecret> = None;
            let (cred, unsigned) = auth
                .perform_auth_ext(webauthn::sha256(&cdj), webauthn::request_options(a), key_agreement, |peer| match (salt, form) {
                    (Some(s), prf::Form::Prf) => Ok(Some(prf::prf_assertion_extensions(&s))),
                    (Some(s), prf::Form::HmacSecret(p)) => {
                        let (x, y) = peer.ok_or(WebauthnCError::MissingRequiredField)?;
                        let hs = prf::HmacSecret::new(p, &x, &y).map_err(|_| WebauthnCError::Internal)?;
                        let input = hs.extension_input(&s).map_err(|_| WebauthnCError::Internal)?;
                        session = Some(hs);
                        Ok(Some(std::collections::BTreeMap::from([("hmac-secret".to_string(), input)])))
                    }
                    _ => Ok(None),
                })
                .map_err(|e| map_error(&e))?;
            let r = cred.response;
            let prf_out = match form {
                prf::Form::Prf => prf::prf_assertion_output(unsigned.as_ref()),
                prf::Form::HmacSecret(_) => match (&session, prf::hmac_secret_output(r.authenticator_data.as_slice())) {
                    (Some(s), Some(enc)) => Some(s.decrypt_output(&enc)?),
                    _ => None,
                },
                prf::Form::None => None,
            };
            Ok(Outcome::Asserted(Assertion {
                credential_id: b64(cred.raw_id.as_slice()),
                authenticator_data: b64(r.authenticator_data.as_slice()),
                client_data_json: b64(&cdj),
                signature: b64(r.signature.as_slice()),
                user_handle: r.user_handle.map(|u| b64(u.as_slice())).filter(|s| !s.is_empty()),
                prf: salt.and(prf_out).map(|o| b64(&o)),
            }))
        }
    }
}

/// Fails early with "ble-off" instead of waiting on a scan that can never see the phone.
async fn ble_ready() -> Result<(), Fail> {
    use btleplug::api::{Central as _, CentralState, Manager as _};
    let off = || Fail::new("ble-off", "Turn on Bluetooth");
    let m = btleplug::platform::Manager::new().await.map_err(|_| off())?;
    let adapter = m.adapters().await.map_err(|_| off())?.into_iter().next().ok_or_else(|| Fail::new("ble-off", "No Bluetooth adapter found"))?;
    match adapter.adapter_state().await {
        Ok(CentralState::PoweredOff) => Err(off()),
        _ => Ok(()),
    }
}

fn udev_fail() -> Fail {
    Fail {
        code: "udev",
        msg: "Rare Launcher cannot open the security key".into(),
        data: Some(serde_json::json!({ "fix": hidraw::udev_fix() })),
    }
}

/// One CTAP2 security key: the only one plugged in, the one plugged in next, or (several) the
/// one the person touches.
async fn usb_authenticator<'a, T, Tr>(transport: &'a Tr, ui: &'a Ui) -> Result<CtapAuthenticator<'a, T, Ui>, Fail>
where
    T: Token + 'a,
    Tr: Transport<'a, Token = T> + Sync,
{
    use futures::StreamExt as _;
    let mut tokens = transport.tokens().await.map_err(|e| map_error(&e))?;
    if tokens.len() == 1 {
        let t = tokens.remove(0);
        return CtapAuthenticator::new(t, ui).await.ok_or_else(|| Fail::new("unsupported", "This security key does not support passkeys (FIDO2)"));
    }
    if tokens.len() > 1 {
        drop(tokens);
        ui.send(status("touch", "Touch the security key you want to use"));
        let stream = transport.watch().await.map_err(|e| map_error(&e))?;
        return select_one_device(stream, ui).await.ok_or_else(|| Fail::new("notfound", "No security key found"));
    }
    if !hidraw::blocked_fido_nodes().is_empty() {
        return Err(udev_fail());
    }
    ui.send(status("waiting", "Plug in your security key"));
    let mut stream = transport.watch().await.map_err(|e| map_error(&e))?;
    let deadline = tokio::time::Instant::now() + PLUG_WAIT;
    loop {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Ok(Some(TokenEvent::Added(t))) => {
                if let Some(a) = CtapAuthenticator::new(t, ui).await {
                    return Ok(a);
                }
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => {
                return Err(if hidraw::blocked_fido_nodes().is_empty() { Fail::new("notfound", "No security key found") } else { udev_fail() });
            }
        }
    }
}

/// Runs `f` until it finishes or the session dies (cancel, timeout). The library waits for a
/// phone's Bluetooth advert or a plugged-in key without any limit, so this is what stops it.
async fn or_dead<T>(dead: &AtomicBool, f: impl std::future::Future<Output = Result<T, Fail>>) -> Result<T, Fail> {
    let watch = async {
        while !dead.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    };
    tokio::select! {
        r = f => r,
        _ = watch => Err(Fail::cancelled()),
    }
}

/// The whole ceremony on the calling (dedicated) thread.
fn ceremony(via: Via, op: Op, ui: Ui) -> Result<Outcome, Fail> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|_| Fail::new("error", "Could not start the passkey request"))?;
    let dead = ui.dead.clone();
    let res = rt.block_on(async {
        // Connecting is async and stops on cancel; `perform` then blocks until the phone or key
        // answers (or gives up on its own).
        match via {
            Via::Phone => {
                let kind = match op {
                    Op::Register(_) => CableRequestType::DiscoverableMakeCredential,
                    Op::Assert(_) => CableRequestType::GetAssertion,
                };
                let mut auth = or_dead(&dead, async {
                    ble_ready().await?;
                    ui.send(status("waiting", "Starting"));
                    webauthn_authenticator_rs::cable::connect_cable_authenticator(kind, &ui).await.map_err(|e| map_error(&e))
                })
                .await?;
                perform(&mut auth, &op, via)
            }
            Via::Usb => {
                let transport = webauthn_authenticator_rs::usb::USBTransport::new().await.map_err(|e| map_error(&e))?;
                let mut auth = or_dead(&dead, usb_authenticator(&transport, &ui)).await?;
                ui.send(status("touch", "Touch your security key"));
                perform(&mut auth, &op, via)
            }
        }
    });
    // Background tasks (tunnel, BLE scan) may still hold the runtime; do not wait for them.
    rt.shutdown_background();
    res
}

// ------------------------------------------------------------------ shared state

struct Session {
    cancel: tokio::sync::Notify,
    pin_tx: Mutex<Option<mpsc::Sender<Option<String>>>>,
    dead: Arc<AtomicBool>,
}

impl Session {
    fn end(&self) {
        self.dead.store(true, Ordering::SeqCst);
        // wakes a PIN prompt that is waiting, so the ceremony thread can finish
        if let Some(tx) = self.pin_tx.lock().unwrap().take() {
            let _ = tx.send(None);
        }
    }
}

#[derive(Default)]
pub struct PasskeyState {
    current: Mutex<Option<Arc<Session>>>,
}

impl PasskeyState {
    async fn run(&self, sink: &Sink, via: &str, op: Op) -> Result<Outcome, String> {
        let via = Via::parse(via).map_err(|f| f.to_js())?;
        let (pin_tx, pin_rx) = mpsc::channel();
        let session = Arc::new(Session { cancel: tokio::sync::Notify::new(), pin_tx: Mutex::new(Some(pin_tx)), dead: Arc::new(AtomicBool::new(false)) });
        {
            let mut cur = self.current.lock().unwrap();
            if cur.is_some() {
                return Err(Fail::new("busy", "Another passkey request is open").to_js());
            }
            *cur = Some(session.clone());
        }
        let emit_sink = sink.clone();
        let emit: ui::Emit = Arc::new(move |e: Event| emit_sink(WalletEvent::Passkey(e)));
        let ui = Ui::new(via, emit, pin_rx, session.dead.clone(), Instant::now() + TIMEOUT);
        let (tx, rx) = tokio::sync::oneshot::channel();
        let spawned = std::thread::Builder::new().name("passkey".into()).spawn(move || {
            let _ = tx.send(ceremony(via, op, ui));
        });
        let res = match spawned {
            Err(_) => Err(Fail::new("error", "Could not start the passkey request")),
            Ok(_) => tokio::select! {
                r = rx => r.unwrap_or_else(|_| Err(Fail::new("error", "The passkey request stopped"))),
                _ = session.cancel.notified() => Err(Fail::cancelled()),
                _ = tokio::time::sleep(TIMEOUT) => Err(Fail::timeout()),
            },
        };
        session.end();
        self.current.lock().unwrap().take();
        if let Err(f) = &res {
            sink(WalletEvent::Passkey(Event::Error { code: f.code.into(), message: f.msg.clone() }));
        }
        res.map_err(|f| f.to_js())
    }

    /// Registers a new passkey for rare.market. Arguments are base64 as JS sends them.
    pub async fn register(&self, sink: &Sink, user_id: &str, user_name: String, display_name: String, challenge: &str, via: &str) -> Result<Registration, String> {
        let req = RegisterRequest {
            user_id: unb64("user id", user_id).map_err(|f| f.to_js())?,
            user_name,
            display_name,
            challenge: unb64("challenge", challenge).map_err(|f| f.to_js())?,
        };
        if req.user_id.is_empty() || req.user_id.len() > 64 {
            return Err(Fail::new("error", "Bad user id").to_js());
        }
        match self.run(sink, via, Op::Register(req)).await? {
            Outcome::Registered(r) => Ok(r),
            Outcome::Asserted(_) => Err(crate::err("error", "Unexpected passkey response")),
        }
    }

    /// One assertion. `credential_ids` empty: a discoverable credential. `prf_salt`: base64
    /// 32-byte WebAuthn PRF input evaluated along with it.
    pub async fn assert(&self, sink: &Sink, challenge: &str, credential_ids: &[String], via: &str, prf_salt: Option<&str>) -> Result<Assertion, String> {
        let req = AssertRequest {
            challenge: unb64("challenge", challenge).map_err(|f| f.to_js())?,
            credential_ids: decode_ids(credential_ids).map_err(|f| f.to_js())?,
            prf_input: prf_salt.map(decode_prf_input).transpose().map_err(|f| f.to_js())?,
        };
        match self.run(sink, via, Op::Assert(req)).await? {
            Outcome::Asserted(a) => Ok(a),
            Outcome::Registered(_) => Err(crate::err("error", "Unexpected passkey response")),
        }
    }

    /// One assertion with a random challenge, only to evaluate PRF for `salt` (the WebAuthn PRF
    /// input, base64 32 bytes). Returns the base64 32-byte output, or "no-prf".
    pub async fn prf(&self, sink: &Sink, credential_ids: &[String], salt: &str, via: &str) -> Result<String, String> {
        let mut challenge = vec![0u8; 32];
        openssl::rand::rand_bytes(&mut challenge).map_err(|_| crate::err("error", "Random failed"))?;
        let req = AssertRequest {
            challenge,
            credential_ids: decode_ids(credential_ids).map_err(|f| f.to_js())?,
            prf_input: Some(decode_prf_input(salt).map_err(|f| f.to_js())?),
        };
        match self.run(sink, via, Op::Assert(req)).await? {
            Outcome::Asserted(Assertion { prf: Some(p), .. }) => Ok(p),
            Outcome::Asserted(_) => Err(prf::no_prf().to_js()),
            Outcome::Registered(_) => Err(crate::err("error", "Unexpected passkey response")),
        }
    }

    /// The PIN a security key asked for (event kind "pin"); None cancels the prompt.
    pub fn pin(&self, pin: Option<String>) {
        let cur = self.current.lock().unwrap().clone();
        if let Some(s) = cur {
            if let Some(tx) = s.pin_tx.lock().unwrap().as_ref() {
                let _ = tx.send(pin);
            }
        }
    }

    pub fn cancel(&self) {
        let cur = self.current.lock().unwrap().clone();
        if let Some(s) = cur {
            s.end();
            s.cancel.notify_one();
        }
    }
}

fn decode_ids(ids: &[String]) -> Result<Vec<Vec<u8>>, Fail> {
    ids.iter().map(|s| unb64("credential id", s)).collect()
}

/// `prfSalt` from JS is the WebAuthn PRF input (32 bytes); the CTAP salt is derived from it.
fn decode_prf_input(s: &str) -> Result<[u8; 32], Fail> {
    unb64("PRF input", s)?.try_into().map_err(|_| Fail::new("error", "The PRF input must be 32 bytes"))
}
