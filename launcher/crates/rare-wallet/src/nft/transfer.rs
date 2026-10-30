//! Send NFTs (wallet or kiosk) to another address in one PTB (`tx/nft/transfer.ts`, Drip's
//! `buildSendNfts`), plus the screen's entry points (`tx/nftSend.ts`). Wallet NFTs and unlocked
//! kiosk NFTs are transferred directly. Locked kiosk NFTs go into one new kiosk created for the
//! recipient (its cap is sent to them); collections whose policy needs a personal kiosk go into a
//! new personal kiosk made for the recipient with `personal_kiosk::create_for`. Builders never
//! read the chain and never sign.

use rare_sui::ptb::{addr_bytes, norm_addr, Arg, Tx};
use rare_sui::tx::check_address;

use super::ids::{drip_marketplace, tradeport, DRIP_LISTING_REGISTRY, SUI_TYPE, TRADEPORT_ORDERBOOK_STORE, TRADEPORT_STORE};
use super::kiosk::KioskSession;
use super::limits::{chunk, plan_batches, Limits, ARGS_PER_COMMAND, SUI_LIMITS};
use super::policy::{pick_policy, policy_needs, resolve_transfer_request, ResolveRequest};
use super::{build_err, KioskItemRef, Market, NftSendError, SendError, SendErrorCode, SendItem, TransferPolicyInfo};

/// `tx.pure.id(id)`: fails like the TS BCS encoder on something that is not an address.
fn pure_id(tx: &mut Tx, id: &str) -> Result<Arg, NftSendError> {
    if addr_bytes(id).is_none() {
        return Err(build_err(format!("Invalid Sui address {id}")));
    }
    Ok(tx.pure_id(id))
}

fn check_addr(a: &str) -> Result<(), NftSendError> {
    if addr_bytes(a).is_none() {
        return Err(build_err(format!("Invalid Sui address {a}")));
    }
    Ok(())
}

/// Cancel the item's listing so it can leave the kiosk: Drip, Tradeport, or a plain kiosk listing.
fn clear_listing(tx: &mut Tx, kiosk: Arg, cap: Arg, item: &KioskItemRef) -> Result<(), NftSendError> {
    let id = pure_id(tx, &item.nft_id)?;
    let t: &[&str] = &[&item.nft_type];
    match &item.market {
        Some(Market::Drip) => {
            let reg = tx.object(DRIP_LISTING_REGISTRY);
            tx.move_call(&drip_marketplace("delist"), t, &[reg, kiosk, cap, id]);
        }
        Some(Market::Tradeport { orderbook: true }) => {
            let (store, ob) = (tx.object(TRADEPORT_STORE), tx.object(TRADEPORT_ORDERBOOK_STORE));
            tx.move_call(&tradeport("cancel_ob_listing_with_transfer_policy"), t, &[store, ob, kiosk, cap, id]);
        }
        Some(Market::Tradeport { orderbook: false }) => {
            let store = tx.object(TRADEPORT_STORE);
            tx.move_call(&tradeport("cancel_listing_with_transfer_policy"), t, &[store, kiosk, cap, id]);
        }
        None if item.is_listed && item.is_exclusive_listing => return Err(build_err("This NFT is listed elsewhere with a purchase cap; cancel that listing first")),
        None if item.is_listed => {
            tx.move_call("0x2::kiosk::delist", t, &[kiosk, cap, id]);
        }
        None => {}
    }
    Ok(())
}

/// Take an item out of kiosk `from`. Unlocked items are taken with `kiosk::take` and returned for
/// the caller to transfer. Locked items are bought from ourselves with a purchase cap (at the
/// floor price when the policy has one; the payment lands back in `from` and is withdrawn again)
/// and the TransferRequest is resolved against the policy into `dest`.
fn extract_item(
    tx: &mut Tx,
    session: &mut KioskSession,
    from: usize,
    item: &KioskItemRef,
    policies: Option<&[TransferPolicyInfo]>,
    dest: Option<usize>,
    recipient: &str,
) -> Result<Option<Arg>, NftSendError> {
    let src = session.handle(from).clone();
    clear_listing(tx, src.kiosk, src.cap, item)?;
    let t: &[&str] = &[&item.nft_type];
    if !item.is_locked {
        let id = pure_id(tx, &item.nft_id)?;
        return Ok(Some(tx.move_call("0x2::kiosk::take", t, &[src.kiosk, src.cap, id])));
    }
    let Some(policy) = pick_policy(policies) else {
        return Err(build_err(format!("No usable transfer policy for {}; the locked item can't be moved", item.nft_type)));
    };
    let price = policy_needs(Some(policy)).floor.unwrap_or(0);
    let price = u64::try_from(price).map_err(|_| build_err(format!("Invalid u64 value: {price}")))?;
    let id = pure_id(tx, &item.nft_id)?;
    let p = tx.pure_u64(price);
    let purchase_cap = tx.move_call("0x2::kiosk::list_with_purchase_cap", t, &[src.kiosk, src.cap, id, p]);
    let payment = if price > 0 { tx.split_u64(Arg::Gas, price) } else { tx.move_call("0x2::coin::zero", &[SUI_TYPE], &[]) };
    let bought = tx.move_call("0x2::kiosk::purchase_with_cap", t, &[src.kiosk, purchase_cap, payment]);
    let (obj, request) = (bought.nth(0), bought.nth(1));
    resolve_transfer_request(tx, session, ResolveRequest { nft_type: &item.nft_type, policy, request, item: obj, kiosk: dest, recipient, prefer_kiosk: dest.is_some() })?;
    if price > 0 {
        let amount = tx.pure_option_u64(Some(price));
        let refund = tx.move_call("0x2::kiosk::withdraw", &[], &[src.kiosk, src.cap, amount]);
        let sender = session.sender.clone();
        tx.transfer_to(&[refund], &sender);
    }
    Ok(None)
}

/// Send NFTs to another address in one transaction (`buildSendNfts`). The sender is set when
/// `tx` has none. Inputs and commands are laid out exactly as the TS builder lays them out.
pub fn build_send_nfts(sender: &str, recipient: &str, items: &[SendItem]) -> Result<Tx, NftSendError> {
    build_send_nfts_into(sender, recipient, items, Tx::new())
}

/// [`build_send_nfts`] appending to an existing transaction.
pub fn build_send_nfts_into(sender: &str, recipient: &str, items: &[SendItem], mut tx: Tx) -> Result<Tx, NftSendError> {
    let from = norm_addr(sender);
    let to = norm_addr(recipient);
    if from == to {
        return Err(build_err("Pick an address other than this account"));
    }
    check_addr(&from)?;
    check_addr(&to)?;
    if tx.sender.is_none() {
        tx.set_sender(&from);
    }
    let mut session = KioskSession::new(&from);
    let mut direct: Vec<Arg> = Vec::new();
    let mut recipient_kiosk: Option<usize> = None;
    let mut recipient_personal: Option<usize> = None;
    for send in items {
        let Some(kref) = &send.from else {
            direct.push(tx.object(&send.item.nft_id));
            continue;
        };
        let source = session.open(&mut tx, kref)?;
        let policies = send.policies.as_deref();
        let mut dest = None;
        if send.item.is_locked {
            let needs = policy_needs(pick_policy(policies));
            dest = Some(if needs.personal {
                match recipient_personal {
                    Some(h) => h,
                    None => *recipient_personal.insert(session.create(&mut tx, &to, true)?),
                }
            } else {
                match recipient_kiosk {
                    Some(h) => h,
                    None => *recipient_kiosk.insert(session.create(&mut tx, &to, false)?),
                }
            });
        }
        if let Some(taken) = extract_item(&mut tx, &mut session, source, &send.item, policies, dest, &to)? {
            direct.push(taken);
        }
    }
    if !direct.is_empty() {
        let to_arg = tx.pure_address(&to);
        for part in chunk(&direct, ARGS_PER_COMMAND) {
            tx.transfer_objects(&part, to_arg);
        }
    }
    session.finish(&mut tx);
    Ok(tx)
}

/// Kiosk items next to each other, so a batch borrows each kiosk once. Wallet NFTs last.
fn by_kiosk(items: &[SendItem]) -> Vec<SendItem> {
    let mut v: Vec<(usize, &SendItem)> = items.iter().enumerate().collect();
    // JS compares UTF-16 code units; kiosk ids are ASCII hex, where that is byte order
    let key = |s: &SendItem| s.from.as_ref().map(|k| k.kiosk_id.clone()).unwrap_or_else(|| "~".into());
    v.sort_by(|a, b| key(a.1).cmp(&key(b.1)).then(a.0.cmp(&b.0)));
    v.into_iter().map(|(_, s)| s.clone()).collect()
}

/// The fewest transactions that send every item, each inside `limits` (`planNftSends`).
pub fn plan_nft_sends(sender: &str, recipient: &str, items: &[SendItem], limits: &Limits) -> Result<Vec<Vec<SendItem>>, NftSendError> {
    if items.is_empty() {
        return Ok(vec![]);
    }
    plan_batches(&by_kiosk(items), |slice| build_send_nfts(sender, recipient, slice), limits)
}

// ---------- the screen's entry points (nftSend.ts) ----------

/// What the Send NFT screen hands over.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NftSendParams {
    pub sender: String,
    pub to: String,
    /// From [`super::context::nft_sendability`]; items that can't be sent never get here.
    pub items: Vec<SendItem>,
}

/// One transaction that sends every item, sender set. Use [`plan_nft_send`] for selections too
/// big for one.
pub fn build_nft_send(p: &NftSendParams) -> Result<Tx, NftSendError> {
    let sender = check_address(&p.sender)?;
    let to = check_address(&p.to)?;
    if p.items.is_empty() {
        return Err(SendError { code: SendErrorCode::BadAmount, message: "Pick at least one item".into() }.into());
    }
    if sender == to {
        return Err(SendError { code: SendErrorCode::SameAddress, message: "Pick an address other than this account".into() }.into());
    }
    let mut tx = build_send_nfts(&sender, &to, &p.items)?;
    tx.set_sender(&sender);
    Ok(tx)
}

/// Split a large selection into the fewest transactions that fit Sui's limits.
pub fn plan_nft_send(p: &NftSendParams) -> Result<Vec<Vec<SendItem>>, NftSendError> {
    plan_nft_sends(&check_address(&p.sender)?, &check_address(&p.to)?, &p.items, &SUI_LIMITS)
}
