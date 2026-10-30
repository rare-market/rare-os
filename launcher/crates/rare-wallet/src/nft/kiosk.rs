//! Kiosk handles for one transaction (`tx/nft/kiosk.ts`): borrow each kiosk's cap once (personal
//! caps through `personal_kiosk::borrow_val`), create kiosks for the recipient, and give every
//! borrowed cap back (and share every new kiosk) exactly once in [`KioskSession::finish`].

use std::collections::HashMap;

use rare_sui::ptb::{Arg, Tx};

use super::ids::{personal_kiosk, KIOSK_TYPE};
use super::{build_err, KioskRef, NftSendError};

/// A kiosk usable in the transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KioskHandle {
    pub kiosk: Arg,
    /// A KioskOwnerCap argument usable as `&KioskOwnerCap`.
    pub cap: Arg,
    /// On-chain id, or None for a kiosk created in this transaction.
    pub kiosk_id: Option<String>,
    pub is_personal: bool,
    pub is_new: bool,
    /// Set when an item was placed or locked into the kiosk during this transaction.
    pub used: bool,
    /// A personal kiosk created for another address: its personal-kiosk proofs wait until
    /// `personal_kiosk::create_for` has made it personal (in `finish`).
    pub(crate) deferred: Option<Vec<Deferred>>,
}

impl KioskHandle {
    /// Whether proofs against this kiosk wait for `finish` (a new personal kiosk).
    pub fn defers_personal(&self) -> bool {
        self.deferred.is_some()
    }
}

/// Commands run once a new personal kiosk is personal: the personal kiosk rule's proof and the
/// request's confirmation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Deferred {
    pub call_pkg: String,
    pub nft_type: String,
    pub policy: Arg,
    pub request: Arg,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Release {
    Nothing,
    /// `personal_kiosk::return_val(personal_cap, borrowed, promise)`.
    Personal { personal_cap_id: String, borrowed: Arg, promise: Arg },
    /// `create_for`, the deferred proofs, then share.
    NewPersonal { owner: String },
    /// `set_owner_custom`, share, cap to the owner.
    NewPlain { owner: String },
}

#[derive(Clone, Debug)]
struct Entry {
    handle: KioskHandle,
    release: Release,
}

/// Every kiosk one transaction touches. Handles are indices into the session.
#[derive(Clone, Debug)]
pub struct KioskSession {
    pub sender: String,
    by_id: HashMap<String, usize>,
    entries: Vec<Entry>,
    finished: bool,
}

impl KioskSession {
    pub fn new(sender: &str) -> Self {
        KioskSession { sender: sender.to_string(), by_id: HashMap::new(), entries: Vec::new(), finished: false }
    }

    pub fn handle(&self, h: usize) -> &KioskHandle {
        &self.entries[h].handle
    }

    pub(crate) fn handle_mut(&mut self, h: usize) -> &mut KioskHandle {
        &mut self.entries[h].handle
    }

    fn assert_open(&self) -> Result<(), NftSendError> {
        if self.finished {
            return Err(build_err("KioskSession already finished"));
        }
        Ok(())
    }

    /// Borrow an existing kiosk's cap (personal caps via `personal_kiosk::borrow_val`); a kiosk
    /// opened twice is the same handle.
    pub fn open(&mut self, tx: &mut Tx, r: &KioskRef) -> Result<usize, NftSendError> {
        self.assert_open()?;
        if let Some(&h) = self.by_id.get(&r.kiosk_id) {
            return Ok(h);
        }
        let (cap, release) = if r.is_personal {
            let Some(pc) = r.personal_cap_id.as_deref().filter(|s| !s.is_empty()) else {
                return Err(build_err(format!("Personal kiosk {} is missing its PersonalKioskCap id", r.kiosk_id)));
            };
            let pcap = tx.object(pc);
            let res = tx.move_call(&personal_kiosk("borrow_val"), &[], &[pcap]);
            let (borrowed, promise) = (res.nth(0), res.nth(1));
            (borrowed, Release::Personal { personal_cap_id: pc.to_string(), borrowed, promise })
        } else {
            if r.owner_cap_id.is_empty() {
                return Err(build_err(format!("Kiosk {} is missing its KioskOwnerCap id", r.kiosk_id)));
            }
            (tx.object(&r.owner_cap_id), Release::Nothing)
        };
        let kiosk = tx.object(&r.kiosk_id);
        let handle = KioskHandle { kiosk, cap, kiosk_id: Some(r.kiosk_id.clone()), is_personal: r.is_personal, is_new: false, used: false, deferred: None };
        self.entries.push(Entry { handle, release });
        let h = self.entries.len() - 1;
        self.by_id.insert(r.kiosk_id.clone(), h);
        Ok(h)
    }

    /// Create a kiosk owned by `owner` inside this transaction; shared (and its cap handed over)
    /// in [`finish`](Self::finish). A personal one is made personal there with `create_for`.
    pub fn create(&mut self, tx: &mut Tx, owner: &str, personal: bool) -> Result<usize, NftSendError> {
        self.assert_open()?;
        if owner == self.sender {
            return Err(build_err("Kiosks are only created here for another address"));
        }
        let res = tx.move_call("0x2::kiosk::new", &[], &[]);
        let (kiosk, raw_cap) = (res.nth(0), res.nth(1));
        let (handle, release) = if personal {
            // items are locked with the raw cap first; create_for then wraps the cap into a
            // PersonalKioskCap owned by `owner` (and sets the kiosk owner)
            (KioskHandle { kiosk, cap: raw_cap, kiosk_id: None, is_personal: true, is_new: true, used: false, deferred: Some(Vec::new()) }, Release::NewPersonal { owner: owner.to_string() })
        } else {
            (KioskHandle { kiosk, cap: raw_cap, kiosk_id: None, is_personal: false, is_new: true, used: false, deferred: None }, Release::NewPlain { owner: owner.to_string() })
        };
        self.entries.push(Entry { handle, release });
        Ok(self.entries.len() - 1)
    }

    /// Return every borrowed cap and share every new kiosk, last opened first. Runs once.
    pub fn finish(&mut self, tx: &mut Tx) {
        if self.finished {
            return;
        }
        self.finished = true;
        for e in self.entries.iter().rev() {
            let h = &e.handle;
            match &e.release {
                Release::Nothing => {}
                Release::Personal { personal_cap_id, borrowed, promise } => {
                    let pcap = tx.object(personal_cap_id);
                    tx.move_call(&personal_kiosk("return_val"), &[], &[pcap, *borrowed, *promise]);
                }
                Release::NewPersonal { owner } => {
                    let o = tx.pure_address(owner);
                    tx.move_call(&personal_kiosk("create_for"), &[], &[h.kiosk, h.cap, o]);
                    for d in h.deferred.iter().flatten() {
                        tx.move_call(&format!("{}::personal_kiosk_rule::prove", d.call_pkg), &[&d.nft_type], &[h.kiosk, d.request]);
                        tx.move_call("0x2::transfer_policy::confirm_request", &[&d.nft_type], &[d.policy, d.request]);
                    }
                    tx.move_call("0x2::transfer::public_share_object", &[KIOSK_TYPE], &[h.kiosk]);
                }
                Release::NewPlain { owner } => {
                    let o = tx.pure_address(owner);
                    tx.move_call("0x2::kiosk::set_owner_custom", &[], &[h.kiosk, h.cap, o]);
                    tx.move_call("0x2::transfer::public_share_object", &[KIOSK_TYPE], &[h.kiosk]);
                    tx.transfer_to(&[h.cap], owner);
                }
            }
        }
    }
}
