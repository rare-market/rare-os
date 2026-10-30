//! Native kiosk listing. Ported from launcher/src/marketplace/venues/list.ts (rare.market
//! web/lib/chain/listing.ts buildList on the "native" path, kiosk.ts KioskScope, purchase.ts
//! listInOwnKiosk and delistFromOwnKiosk).
//!
//! rare.market lists with 0x2::kiosk::list in the seller's own kiosk (zero fee) while its own
//! contract ids are empty, so only that path is here. Wallet items are placed (or locked, when the
//! policy has a lock rule) into a kiosk first, creating one in the same PTB when the seller has none
//! (personal when the policy needs it). Tradeport and Drip listings are not delisted here.

use super::ids::kiosk_ext;
use super::model::{OwnedKioskRef, TransferPolicy};
use super::policy::read_policy;
use super::{venue_label, BuildError, VenueConfig};
use crate::ptb::{Arg, Tx};
use std::collections::HashMap;

const KIOSK_TYPE: &str = "0x2::kiosk::Kiosk";

fn personal_kiosk(f: &str) -> String {
    format!("{}::personal_kiosk::{f}", kiosk_ext::LATEST)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KioskArg {
    pub kiosk: Arg,
    /// Usable as &KioskOwnerCap.
    pub cap: Arg,
    /// On-chain id, or None for a kiosk created in this transaction.
    pub kiosk_id: Option<String>,
    pub is_personal: bool,
    pub is_new: bool,
}

#[derive(Clone, Copy, Debug)]
enum Close {
    Nothing,
    ReturnPersonal { pcap: Arg, borrowed: Arg, promise: Arg },
    NewPersonal { personal_cap: Arg, borrowed: Arg, promise: Arg, kiosk: Arg },
    NewPlain { kiosk: Arg, cap: Arg },
}

/// Every kiosk one transaction touches. `open` and `create` hand out kiosk arguments; `close`
/// returns borrowed caps and shares new kiosks, in reverse order. Call `close` once, after the last
/// command that uses a kiosk.
pub struct KioskScope {
    sender: String,
    opened: HashMap<String, usize>,
    slots: Vec<(KioskArg, Close)>,
    closed: bool,
}

impl KioskScope {
    pub fn new(sender: &str) -> Self {
        KioskScope { sender: sender.to_string(), opened: HashMap::new(), slots: Vec::new(), closed: false }
    }

    fn ensure_open(&self) -> Result<(), BuildError> {
        if self.closed {
            return Err(BuildError::Other("Kiosk scope already closed".into()));
        }
        Ok(())
    }

    pub fn open(&mut self, tx: &mut Tx, r: &OwnedKioskRef) -> Result<KioskArg, BuildError> {
        self.ensure_open()?;
        if let Some(&i) = self.opened.get(&r.kiosk_id) {
            return Ok(self.slots[i].0.clone());
        }
        let (cap, close) = if r.is_personal {
            let pc = r.personal_cap_id.as_deref().filter(|s| !s.is_empty()).ok_or_else(|| BuildError::Other(format!("Personal kiosk {} has no PersonalKioskCap id", r.kiosk_id)))?;
            let pcap = tx.object(pc);
            let res = tx.move_call(&personal_kiosk("borrow_val"), &[], &[pcap]);
            let (borrowed, promise) = (res.nth(0), res.nth(1));
            (borrowed, Close::ReturnPersonal { pcap, borrowed, promise })
        } else {
            if r.owner_cap_id.is_empty() {
                return Err(BuildError::Other(format!("Kiosk {} has no KioskOwnerCap id", r.kiosk_id)));
            }
            (tx.object(&r.owner_cap_id), Close::Nothing)
        };
        let arg = KioskArg { kiosk: tx.object(&r.kiosk_id), cap, kiosk_id: Some(r.kiosk_id.clone()), is_personal: r.is_personal, is_new: false };
        self.opened.insert(r.kiosk_id.clone(), self.slots.len());
        self.slots.push((arg.clone(), close));
        Ok(arg)
    }

    pub fn create(&mut self, tx: &mut Tx, personal: bool) -> Result<KioskArg, BuildError> {
        self.ensure_open()?;
        let r = tx.move_call("0x2::kiosk::new", &[], &[]);
        let (kiosk, raw_cap) = (r.nth(0), r.nth(1));
        let (arg, close) = if personal {
            let personal_cap = tx.move_call(&personal_kiosk("new"), &[], &[kiosk, raw_cap]);
            let b = tx.move_call(&personal_kiosk("borrow_val"), &[], &[personal_cap]);
            let (borrowed, promise) = (b.nth(0), b.nth(1));
            (KioskArg { kiosk, cap: borrowed, kiosk_id: None, is_personal: true, is_new: true }, Close::NewPersonal { personal_cap, borrowed, promise, kiosk })
        } else {
            (KioskArg { kiosk, cap: raw_cap, kiosk_id: None, is_personal: false, is_new: true }, Close::NewPlain { kiosk, cap: raw_cap })
        };
        self.slots.push((arg.clone(), close));
        Ok(arg)
    }

    pub fn close(&mut self, tx: &mut Tx) {
        if self.closed {
            return;
        }
        self.closed = true;
        for (_, c) in self.slots.iter().rev() {
            match *c {
                Close::Nothing => {}
                Close::ReturnPersonal { pcap, borrowed, promise } => {
                    tx.move_call(&personal_kiosk("return_val"), &[], &[pcap, borrowed, promise]);
                }
                Close::NewPersonal { personal_cap, borrowed, promise, kiosk } => {
                    tx.move_call(&personal_kiosk("return_val"), &[], &[personal_cap, borrowed, promise]);
                    tx.move_call(&personal_kiosk("transfer_to_sender"), &[], &[personal_cap]);
                    tx.move_call("0x2::transfer::public_share_object", &[KIOSK_TYPE], &[kiosk]);
                }
                Close::NewPlain { kiosk, cap } => {
                    tx.move_call("0x2::transfer::public_share_object", &[KIOSK_TYPE], &[kiosk]);
                    tx.transfer_to(&[cap], &self.sender);
                }
            }
        }
    }
}

/// List an item in the sender's own kiosk with kiosk::list (no marketplace contract).
pub fn list_in_own_kiosk(tx: &mut Tx, k: &KioskArg, nft_id: &str, nft_type: &str, price_mist: u64) {
    let (id, price) = (tx.pure_id(nft_id), tx.pure_u64(price_mist));
    tx.move_call("0x2::kiosk::list", &[nft_type], &[k.kiosk, k.cap, id, price]);
}

pub fn delist_from_own_kiosk(tx: &mut Tx, k: &KioskArg, nft_id: &str, nft_type: &str) {
    let id = tx.pure_id(nft_id);
    tx.move_call("0x2::kiosk::delist", &[nft_type], &[k.kiosk, k.cap, id]);
}

/// An existing listing of the item.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExistingListing {
    pub venue: String,
    pub order_key: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListingItem {
    pub nft_id: String,
    pub nft_type: String,
    pub price_mist: u64,
    /// The seller's kiosk the item is in, or None when it is in the wallet.
    pub kiosk: Option<OwnedKioskRef>,
    pub policy: Option<TransferPolicy>,
    pub listing: Option<ExistingListing>,
}

#[derive(Default)]
struct Made {
    plain: Option<KioskArg>,
    personal: Option<KioskArg>,
}

/// Pick (or create) the kiosk a wallet item goes into, honouring the collection's policy.
fn home_for(tx: &mut Tx, cfg: &VenueConfig, scope: &mut KioskScope, item: &ListingItem, kiosks: &[OwnedKioskRef], made: &mut Made) -> Result<KioskArg, BuildError> {
    let shape = read_policy(cfg, item.policy.as_ref());
    let personal = kiosks.iter().find(|k| k.is_personal && k.personal_cap_id.as_deref().is_some_and(|s| !s.is_empty()));
    let plain = kiosks.iter().find(|k| !k.is_personal && !k.owner_cap_id.is_empty());
    if shape.personal {
        if let Some(p) = personal {
            return scope.open(tx, p);
        }
        if made.personal.is_none() {
            made.personal = Some(scope.create(tx, true)?);
        }
        return Ok(made.personal.clone().unwrap());
    }
    if let Some(e) = plain.or(personal) {
        return scope.open(tx, e);
    }
    if made.plain.is_none() {
        made.plain = Some(scope.create(tx, false)?);
    }
    Ok(made.plain.clone().unwrap())
}

fn stow(tx: &mut Tx, cfg: &VenueConfig, k: &KioskArg, item: &ListingItem) -> Result<(), BuildError> {
    let shape = read_policy(cfg, item.policy.as_ref());
    if shape.lock {
        let policy = item.policy.as_ref().ok_or_else(|| BuildError::Other("This collection locks items but its policy is unknown.".into()))?;
        let (pol, nft) = (tx.object(&policy.id), tx.object(&item.nft_id));
        tx.move_call("0x2::kiosk::lock", &[&item.nft_type], &[k.kiosk, k.cap, pol, nft]);
    } else {
        let nft = tx.object(&item.nft_id);
        tx.move_call("0x2::kiosk::place", &[&item.nft_type], &[k.kiosk, k.cap, nft]);
    }
    Ok(())
}

/// One PTB listing every item natively in the seller's kiosk. A native relist delists and lists again.
pub fn build_list(cfg: &VenueConfig, sender: &str, items: &[ListingItem], kiosks: &[OwnedKioskRef]) -> Result<Tx, BuildError> {
    if items.is_empty() {
        return Err(BuildError::Other("Nothing to list.".into()));
    }
    let mut tx = Tx::new();
    tx.set_sender(sender);
    let mut scope = KioskScope::new(sender);
    let mut made = Made::default();
    for item in items {
        if item.price_mist == 0 {
            return Err(BuildError::Other("Price must be above zero.".into()));
        }
        if let Some(existing) = &item.listing {
            if existing.venue != "kiosk" {
                return Err(BuildError::Other(format!("Delist it on {} first.", venue_label(&existing.venue))));
            }
            let kref = item.kiosk.as_ref().ok_or_else(|| BuildError::Other("Your wallet doesn't hold the kiosk this item is in.".into()))?;
            let k = scope.open(&mut tx, kref)?;
            delist_from_own_kiosk(&mut tx, &k, &item.nft_id, &item.nft_type);
            list_in_own_kiosk(&mut tx, &k, &item.nft_id, &item.nft_type, item.price_mist);
            continue;
        }
        let k = match &item.kiosk {
            Some(kref) => scope.open(&mut tx, kref)?,
            None => {
                let k = home_for(&mut tx, cfg, &mut scope, item, kiosks, &mut made)?;
                stow(&mut tx, cfg, &k, item)?;
                k
            }
        };
        list_in_own_kiosk(&mut tx, &k, &item.nft_id, &item.nft_type, item.price_mist);
    }
    scope.close(&mut tx);
    Ok(tx)
}
