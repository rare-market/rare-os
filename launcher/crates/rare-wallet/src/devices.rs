//! The real devices behind `sign::ExternalSigner`: Ledger (USB or Bluetooth), Keystone (USB) and
//! the passkey on a phone or security key, with the prompt stages the review sheet shows. Port of
//! the device halves of src/wallet/{ledger,keystone}/index.ts and the passkey signer provider.
//!
//! Blocking: call from the wallet worker thread; `rt` runs the async transports.

use crate::keystone::{KeystoneState, KeystoneUsb};
use crate::keystone_sui::{self, KeystoneAccount};
use crate::keystore::{AccountKind, RareAccount};
use crate::ledger::{LedgerDevice, LedgerState};
use crate::ledger_sui::{self, LedgerCode, LedgerError, LedgerPort};
use crate::passkey::PasskeyState;
use crate::passkey_account::{self, Ctap, Via};
use crate::sign::{ExternalSigner, SignError};
use crate::Sink;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// What the review sheet shows while a device works.
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceStage {
    Ledger { stage: ledger_sui::Stage, device: Option<String> },
    /// `digest` is the lowercase hex hash the Keystone shows while signing
    Keystone { stage: keystone_sui::Stage, digest: Option<String> },
    /// passkey ceremonies report through the wallet's event sink ("passkey-event")
    Passkey,
}

/// Reads object BCS for Ledger clear signing (by object id); empty when they can't be read.
pub type ObjectReader<'a> = Box<dyn Fn(&[String]) -> Vec<Vec<u8>> + 'a>;

/// How long the person has to confirm on a Ledger.
pub const CONFIRM: Duration = Duration::from_secs(5 * 60);

/// The Ledger, Keystone and passkey transports with the UI hooks.
pub struct Devices<'a> {
    pub ledger: &'a LedgerState,
    pub keystone: &'a KeystoneState,
    pub passkey: &'a PasskeyState,
    pub rt: tokio::runtime::Handle,
    pub sink: Sink,
    /// phone or USB key for passkey signatures (the remembered choice)
    pub via: Via,
    pub on_stage: Box<dyn FnMut(DeviceStage) + 'a>,
    /// the sheet's Cancel: stops waiting (the device may still show the request)
    pub cancelled: Arc<AtomicBool>,
    /// BCS of objects for Ledger clear signing (gas coins and owned inputs), best effort
    pub objects: Option<ObjectReader<'a>>,
    /// the Ledger the last conversation used
    pub current: Option<LedgerDevice>,
}

fn device_err(code: &str, message: &str) -> SignError {
    SignError::Device { code: code.into(), message: message.into() }
}

fn ledger_err(e: LedgerError) -> SignError {
    device_err(e.code.as_str(), &e.message)
}

struct Port<'a> {
    state: &'a LedgerState,
    rt: tokio::runtime::Handle,
    sink: Sink,
}

impl<'a> LedgerPort for Port<'a> {
    type Conn = &'a LedgerState;
    fn open(&mut self, id: &str) -> Result<&'a LedgerState, LedgerError> {
        self.rt.block_on(self.state.open(&self.sink, id)).map_err(|e| LedgerError::from_native(&e))?;
        Ok(self.state)
    }
}

impl Devices<'_> {
    /// The Ledger to sign with: the remembered one, else the first USB Ledger, else a Bluetooth
    /// one (the one the account was added with when its name matches, else a paired one).
    fn pick_ledger(&mut self, name: Option<&str>) -> Result<LedgerDevice, LedgerError> {
        if let Some(d) = &self.current {
            return Ok(d.clone());
        }
        let usb = self.rt.block_on(LedgerState::list(false, 0)).map_err(|e| LedgerError::from_native(&e))?;
        if let Some(d) = usb.iter().find(|d| Some(d.name.as_str()) == name).or(usb.first()) {
            self.current = Some(d.clone());
            return Ok(d.clone());
        }
        let all = self.rt.block_on(LedgerState::list(true, 3000)).map_err(|e| LedgerError::from_native(&e))?;
        let ble: Vec<&LedgerDevice> = all.iter().filter(|d| d.transport == "ble").collect();
        let d = ble
            .iter()
            .find(|d| Some(d.name.as_str()) == name)
            .or_else(|| ble.iter().find(|d| d.paired == Some(true)))
            .or(ble.first())
            .map(|d| (*d).clone())
            .ok_or_else(|| LedgerError::new(LedgerCode::NoDevice))?;
        self.current = Some(d.clone());
        Ok(d)
    }

    fn ledger_sign(&mut self, account: &RareAccount, public_key: &str, transaction: bool, bytes: &[u8]) -> Result<String, LedgerError> {
        let info = account.ledger.as_ref().ok_or_else(|| LedgerError::with(LedgerCode::Unknown, "This account is not a Ledger account."))?;
        let pk = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, public_key)
            .ok()
            .filter(|p| p.len() == 32)
            .ok_or_else(|| LedgerError::with(LedgerCode::Unknown, "This Ledger account has no valid public key. Remove it and add it again."))?;
        let objects = if transaction { self.objects.as_ref().map(|f| f(&ledger_sui::clear_signing_object_ids(bytes))).unwrap_or_default() } else { vec![] };
        let attempt = |me: &mut Self, d: &LedgerDevice| -> Result<String, LedgerError> {
            (me.on_stage)(DeviceStage::Ledger { stage: ledger_sui::Stage::Connecting, device: Some(d.name.clone()) });
            let mut port = Port { state: me.ledger, rt: me.rt.clone(), sink: me.sink.clone() };
            let cancelled = me.cancelled.clone();
            // both the session and the signing step report stages; they never run at once
            let on_stage = std::cell::RefCell::new(&mut me.on_stage);
            let name = d.name.clone();
            let r = ledger_sui::session(
                &mut port,
                &d.id,
                ledger_sui::APP_WAIT,
                ledger_sui::POLL,
                &mut |s| (on_stage.borrow_mut())(DeviceStage::Ledger { stage: s, device: Some(name.clone()) }),
                &|| cancelled.load(Ordering::SeqCst),
                |c: &mut &LedgerState, _| {
                    (on_stage.borrow_mut())(DeviceStage::Ledger { stage: ledger_sui::Stage::Confirm, device: Some(name.clone()) });
                    if transaction {
                        ledger_sui::sign_transaction(c, &info.path, &pk, bytes, &objects)
                    } else {
                        ledger_sui::sign_personal_message(c, &info.path, &pk, bytes)
                    }
                },
            );
            me.rt.block_on(me.ledger.close());
            r
        };
        let d = self.pick_ledger(info.device.as_deref())?;
        match attempt(self, &d) {
            // a stale remembered device falls back to discovery once
            Err(e) if matches!(e.code, LedgerCode::NoDevice | LedgerCode::Disconnected) => {
                self.current = None;
                let d = self.pick_ledger(info.device.as_deref())?;
                attempt(self, &d)
            }
            r => r,
        }
        .inspect(|_| (self.on_stage)(DeviceStage::Ledger { stage: ledger_sui::Stage::Done, device: None }))
    }

    fn keystone_sign(&mut self, account: &RareAccount, public_key: &str, transaction: bool, bytes: &[u8]) -> Result<String, keystone_sui::KeystoneError> {
        let info = account.keystone.as_ref().ok_or_else(|| keystone_sui::KeystoneError {
            message: "This account is not a Keystone account.".into(),
            ..keystone_sui::KeystoneError::new(keystone_sui::KeystoneCode::Unknown)
        })?;
        let acc = KeystoneAccount {
            index: crate::keys::path_index(&info.path).unwrap_or(0),
            path: info.path.clone(),
            public_key: public_key.to_string(),
            address: account.address.clone(),
            mfp: info.mfp.clone(),
        };
        let digest = crate::keys::hex(&keystone_sui::signing_digest(transaction, bytes));
        let mut link = KeystoneUsb { state: self.keystone, rt: self.rt.clone() };
        let cancelled = self.cancelled.clone();
        let on_stage = &mut self.on_stage;
        keystone_sui::task(
            keystone_sui::Stage::Confirm,
            keystone_sui::WAIT,
            keystone_sui::POLL,
            &mut |s| on_stage(DeviceStage::Keystone { stage: s, digest: Some(digest.clone()) }),
            &|| cancelled.load(Ordering::SeqCst),
            || keystone_sui::sign(&mut link, &acc, transaction, bytes),
        )
    }
}

impl ExternalSigner for Devices<'_> {
    fn sign(&mut self, account: &RareAccount, public_key: &str, transaction: bool, bytes: &[u8]) -> Result<String, SignError> {
        self.cancelled.store(false, Ordering::SeqCst);
        match account.kind {
            AccountKind::Ledger => self.ledger_sign(account, public_key, transaction, bytes).map_err(ledger_err),
            AccountKind::Keystone => self.keystone_sign(account, public_key, transaction, bytes).map_err(|e| device_err(e.code.as_str(), &e.message)),
            AccountKind::Passkey => {
                (self.on_stage)(DeviceStage::Passkey);
                let mut ctap = Ctap { state: self.passkey, sink: self.sink.clone(), rt: self.rt.clone() };
                passkey_account::sign_with_passkey(account, &mut ctap, self.via, transaction, bytes).map_err(|e| match e {
                    passkey_account::PasskeyError::Native(n) => device_err(&n.code, &n.message),
                    other => device_err("error", &other.to_string()),
                })
            }
            kind => Err(crate::keystore::KeystoreError::Unsupported(kind).into()),
        }
    }
}
