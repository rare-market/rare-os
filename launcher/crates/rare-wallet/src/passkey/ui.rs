//! Turns the authenticator library's UI callbacks into "passkey-event"s for the launcher UI.
//! The PIN prompt blocks the ceremony thread until passkey_pin answers (or the ceremony ends).

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use webauthn_authenticator_rs::types::{CableRequestType, CableState, EnrollSampleStatus};
use webauthn_authenticator_rs::ui::UiCallback;

use super::Via;

/// `PasskeyEvent` in native.ts.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Event {
    Qr { url: String },
    Status { stage: &'static str, message: String },
    Pin {
        #[serde(rename = "attemptsLeft")]
        attempts_left: Option<u32>,
    },
    Error { code: String, message: String },
}

pub fn status(stage: &'static str, message: &str) -> Event {
    Event::Status { stage, message: message.to_string() }
}

pub type Emit = Arc<dyn Fn(Event) + Send + Sync>;

pub struct Ui {
    pub via: Via,
    emit: Emit,
    pin_rx: Mutex<mpsc::Receiver<Option<String>>>,
    /// set when the ceremony was cancelled or timed out: stay quiet, refuse PIN prompts
    pub dead: Arc<AtomicBool>,
    deadline: Instant,
}

impl fmt::Debug for Ui {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ui").field("via", &self.via).finish()
    }
}

impl Ui {
    pub fn new(via: Via, emit: Emit, pin_rx: mpsc::Receiver<Option<String>>, dead: Arc<AtomicBool>, deadline: Instant) -> Self {
        Ui { via, emit, pin_rx: Mutex::new(pin_rx), dead, deadline }
    }

    pub fn send(&self, e: Event) {
        if !self.dead.load(Ordering::SeqCst) {
            (self.emit)(e)
        }
    }
}

impl UiCallback for Ui {
    fn request_pin(&self) -> Option<String> {
        if self.dead.load(Ordering::SeqCst) {
            return None;
        }
        // The library asks for retries first but does not hand them over.
        self.send(Event::Pin { attempts_left: None });
        let rx = self.pin_rx.lock().ok()?;
        let left = self.deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(Some(pin)) if !self.dead.load(Ordering::SeqCst) => {
                self.send(status("processing", "Checking PIN"));
                Some(pin)
            }
            _ => None,
        }
    }

    fn request_touch(&self) {
        self.send(match self.via {
            Via::Usb => status("touch", "Touch your security key"),
            Via::Phone => status("touch", "Confirm on your phone"),
        });
    }

    fn processing(&self) {
        self.send(status("processing", "Working"));
    }

    fn fingerprint_enrollment_feedback(&self, _remaining: u32, _feedback: Option<EnrollSampleStatus>) {}

    fn cable_qr_code(&self, _t: CableRequestType, url: String) {
        self.send(Event::Qr { url });
    }

    fn dismiss_qr_code(&self) {
        self.send(status("connecting", "Phone found. Connecting"));
    }

    fn cable_status_update(&self, state: CableState) {
        match state {
            CableState::ConnectingToTunnelServer | CableState::Handshaking => self.send(status("connecting", "Connecting to your phone")),
            CableState::WaitingForAuthenticatorResponse => self.send(status("touch", "Confirm on your phone")),
            CableState::Processing => self.send(status("processing", "Working")),
            _ => {}
        }
    }
}
