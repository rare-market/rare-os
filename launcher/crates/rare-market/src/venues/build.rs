//! The buy and sell-into-bid builders for every venue. Ported from
//! launcher/src/marketplace/venues/build.ts (rare.market packages/venues/src/build.ts buildBuy,
//! buildAcceptBid and what they call; ours.ts for rare's own contract, off while its ids are empty).
//!
//! The builder only adds commands to a [`Tx`]; it never signs or executes. Rules it keeps:
//! - Calls target each venue's latest package (versioned stores reject older code).
//! - Royalties are computed on chain with `royalty_rule::fee_amount` on what the request records.
//! - A bought kiosk NFT always ends in the buyer's kiosk (a new one is created and shared when the
//!   buyer has none). Escrow listings deliver to the buyer's wallet.
//! - Selling into a bid always delivers the NFT to the bidder.
//!
//! Commands and inputs are added in exactly the order the TypeScript adds them, so the PTB is the
//! same one (the golden tests compare the two).

use super::ids::{drip_v2, kiosk_ext, tradeport as tp, CLOCK};
use super::model::{ActiveBid, ActiveListing};
use super::policy::{add_resale_prove, rules, Policy};
use super::{BuildError, VenueConfig};
use crate::ptb::{Arg, Tx};
use serde_json::Value;

const KIOSK: &str = "0x2::kiosk::Kiosk";

/// A kiosk the buyer controls. `personal_cap_id` set means the KioskOwnerCap is wrapped in a
/// PersonalKioskCap (borrowed for the PTB and returned).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserKiosk {
    pub kiosk_id: String,
    pub cap_id: String,
    pub personal_cap_id: Option<String>,
    pub personal: bool,
}

pub struct BuyContext<'a> {
    pub sender: &'a str,
    /// The buyer's kiosk, or None to create one in the PTB.
    pub kiosk: Option<&'a UserKiosk>,
    /// Transfer policy of the NFT type (required for kiosk NFTs).
    pub policy: Option<&'a Policy>,
    /// Drip v2: buy the native kiosk listing directly (no fee) and clear Drip's record. Default true.
    pub drip_direct: bool,
    /// Rare Store ResaleLicence: the Game object the licence belongs to (resale::prove needs it).
    pub resale_game: Option<&'a str>,
    pub cfg: &'a VenueConfig,
}

/// Where the seller's NFT is.
pub enum NftAt<'a> {
    Kiosk { kiosk: &'a UserKiosk, nft_id: &'a str, nft_type: &'a str, listed_native: bool },
    Wallet { object_id: &'a str, nft_type: &'a str, kiosk: Option<&'a UserKiosk> },
}

impl NftAt<'_> {
    fn nft_type(&self) -> &str {
        match self {
            NftAt::Kiosk { nft_type, .. } | NftAt::Wallet { nft_type, .. } => nft_type,
        }
    }
}

pub struct SellContext<'a> {
    pub sender: &'a str,
    pub policy: Option<&'a Policy>,
    pub nft: NftAt<'a>,
    pub cfg: &'a VenueConfig,
}

/// What `finish()` does with a kiosk borrowed or created for the PTB.
#[derive(Clone, Copy, Debug)]
enum Finish {
    Nothing,
    /// A kiosk made in the PTB: share it, send its cap to the sender.
    Share { kiosk: Arg, cap: Arg },
    /// A borrowed personal cap: return it.
    Return { pcap: Arg, cap: Arg, borrow: Arg },
}

#[derive(Clone, Copy, Debug)]
struct KioskArgs {
    kiosk: Arg,
    cap: Arg,
    finish: Finish,
}

fn finish(tx: &mut Tx, k: &KioskArgs, sender: &str) {
    match k.finish {
        Finish::Nothing => {}
        Finish::Share { kiosk, cap } => {
            tx.move_call("0x2::transfer::public_share_object", &[KIOSK], &[kiosk]);
            tx.transfer_to(&[cap], sender);
        }
        Finish::Return { pcap, cap, borrow } => {
            tx.move_call(&format!("{}::personal_kiosk::return_val", kiosk_ext::LATEST), &[], &[pcap, cap, borrow]);
        }
    }
}

/// Borrows (or creates) the buyer's kiosk and cap for the PTB.
fn open_kiosk(tx: &mut Tx, k: Option<&UserKiosk>) -> KioskArgs {
    let Some(k) = k else {
        let r = tx.move_call("0x2::kiosk::new", &[], &[]);
        let (kiosk, cap) = (r.nth(0), r.nth(1));
        return KioskArgs { kiosk, cap, finish: Finish::Share { kiosk, cap } };
    };
    let kiosk = tx.object(&k.kiosk_id);
    if let Some(pc) = k.personal_cap_id.as_deref().filter(|s| !s.is_empty()) {
        let pcap = tx.object(pc);
        let r = tx.move_call(&format!("{}::personal_kiosk::borrow_val", kiosk_ext::LATEST), &[], &[pcap]);
        let (cap, borrow) = (r.nth(0), r.nth(1));
        return KioskArgs { kiosk, cap, finish: Finish::Return { pcap, cap, borrow } };
    }
    let cap = tx.object(&k.cap_id);
    KioskArgs { kiosk, cap, finish: Finish::Nothing }
}

fn need_policy<'a>(policy: Option<&'a Policy>, nft_type: &str) -> Result<&'a Policy, BuildError> {
    let p = policy.ok_or_else(|| BuildError::Unsupported(format!("no usable transfer policy for {nft_type}")))?;
    if !p.unknown_rules.is_empty() {
        return Err(BuildError::Unsupported(format!("policy {} has rules we cannot prove: {}", p.policy_id, p.unknown_rules.join(", "))));
    }
    Ok(p)
}

struct Resolve<'a> {
    policy: &'a Policy,
    nft_type: &'a str,
    req: Arg,
    item: Option<Arg>,
    into: Option<(Arg, Arg)>,
    pay_from: Arg,
    /// Rules the venue already proved inside its call.
    proved: &'a [String],
    personal_kiosk: bool,
    resale_game: Option<&'a str>,
}

/// Satisfies a TransferRequest: places or locks the item into `into`, pays royalty from
/// `pay_from`, proves the remaining rules, confirms.
fn resolve(tx: &mut Tx, cfg: &VenueConfig, o: Resolve) -> Result<(), BuildError> {
    let p = o.policy;
    let rl = rules();
    let t = o.nft_type;
    let policy = tx.object(&p.policy_id);
    let has = |r: &str| p.rules.iter().any(|x| x == r) && !o.proved.iter().any(|x| x == r);
    if let Some(item) = o.item {
        let (kiosk, cap) = o.into.ok_or_else(|| BuildError::Other("item without a destination kiosk".into()))?;
        if p.rules.contains(&rl.kiosk_lock) {
            tx.move_call("0x2::kiosk::lock", &[t], &[kiosk, cap, policy, item]);
        } else {
            tx.move_call("0x2::kiosk::place", &[t], &[kiosk, cap, item]);
        }
    }
    if has(&rl.royalty) {
        let paid = tx.move_call("0x2::transfer_policy::paid", &[t], &[o.req]).nth(0);
        let amount = tx.move_call(&format!("{}::royalty_rule::fee_amount", kiosk_ext::LATEST), &[t], &[policy, paid]).nth(0);
        let coin = tx.split_coins(o.pay_from, &[amount]).nth(0);
        tx.move_call(&format!("{}::royalty_rule::pay", kiosk_ext::LATEST), &[t], &[policy, o.req, coin]);
    }
    if has(&rl.kiosk_lock) {
        let (kiosk, _) = o.into.ok_or_else(|| BuildError::Unsupported("kiosk lock rule needs the destination kiosk".into()))?;
        tx.move_call(&format!("{}::kiosk_lock_rule::prove", kiosk_ext::LATEST), &[t], &[o.req, kiosk]);
    }
    if let Some(resale) = cfg.resale.rule() {
        if has(&resale) {
            let (kiosk, cap) = o.into.ok_or_else(|| BuildError::Unsupported("resale rule needs the destination kiosk".into()))?;
            if o.resale_game.is_none() {
                return Err(BuildError::Unsupported("licence resale: the game is unknown, so the resale rule cannot be proved".into()));
            }
            add_resale_prove(tx, cfg, o.resale_game, o.req, kiosk, cap, o.pay_from)?;
        }
    }
    if has(&rl.personal_kiosk) {
        let into = o.into.filter(|_| o.personal_kiosk).ok_or_else(|| BuildError::Unsupported("personal kiosk rule: the item must go to a personal kiosk".into()))?;
        tx.move_call(&format!("{}::personal_kiosk_rule::prove", kiosk_ext::LATEST), &[t], &[into.0, o.req]);
    }
    if has(&rl.floor_price) {
        tx.move_call(&format!("{}::floor_price_rule::prove", kiosk_ext::LATEST), &[t], &[policy, o.req]);
    }
    tx.move_call("0x2::transfer_policy::confirm_request", &[t], &[policy, o.req]);
    Ok(())
}

/// `BigInt(s)` for an amount the builder pays: a decimal u64.
pub(crate) fn amount(s: &str) -> Result<u64, BuildError> {
    let t = s.trim();
    if t.is_empty() {
        return Ok(0);
    }
    t.parse::<u64>().map_err(|_| BuildError::Other(format!("Cannot convert {s} to a BigInt")))
}

/// Lenient amount for prices shown (`BigInt` of a well-formed API string).
fn mist(s: &str) -> i128 {
    s.trim().parse::<i128>().unwrap_or(0)
}

/// `Number(v) === 1` for a JSON value that may be a number or a string.
fn num_is(v: Option<&Value>, want: f64) -> bool {
    match v {
        Some(Value::Number(n)) => n.as_f64() == Some(want),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok() == Some(want) || (s.trim().is_empty() && want == 0.0),
        Some(Value::Bool(b)) => (*b as u8 as f64) == want,
        Some(Value::Null) => want == 0.0,
        _ => false,
    }
}

/// JS truthiness of a JSON value.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// The marketplace fee the buyer pays on top of the price on the path we use: Drip v2 bought
/// directly through the kiosk pays none.
pub fn venue_fee(l: &ActiveListing, drip_direct: bool) -> i128 {
    if l.source == "drip/v2" && drip_direct {
        return 0;
    }
    mist(&l.fee)
}

/// Royalty the buyer pays: the policy's rate on what the seller's kiosk receives (price minus
/// legacy commission; Tradeport E charges it on the price).
pub fn royalty_of(l: &ActiveListing, policy: Option<&Policy>) -> i128 {
    let Some(r) = policy.and_then(|p| p.royalty) else { return 0 };
    // Escrow listings hand over the NFT without a transfer request: no royalty.
    if l.source.ends_with("escrow") || (l.source == "tradeport/simple" && num_is(l.extra.get("listingType"), 0.0)) {
        return 0;
    }
    let base = if l.source == "tradeport/simple" { mist(&l.price) } else { mist(&l.price) - mist(&l.commission) };
    let v = base * r.bps as i128 / 10000;
    let min = r.min as i128;
    if v > min {
        v
    } else {
        min
    }
}

/// All-in price (price + venue fee + royalty, before gas).
pub fn all_in_price(l: &ActiveListing, policy: Option<&Policy>, drip_direct: bool) -> i128 {
    mist(&l.price) + venue_fee(l, drip_direct) + royalty_of(l, policy)
}

// ---------- ours.ts: rare's own contract (venue `market`), off until published ----------

fn build_ours_buy(tx: &mut Tx, l: &ActiveListing, ctx: &BuyContext) -> Result<(), BuildError> {
    let ours = &ctx.cfg.ours;
    if ours.package.is_empty() || ours.config.is_empty() {
        return Err(BuildError::Unsupported("our contract is not deployed yet".into()));
    }
    let t = l.nft_type.as_deref().unwrap_or_default();
    let cfg = tx.object(&ours.config);
    let k = open_kiosk(tx, ctx.kiosk);
    if let Some(kiosk_id) = l.kiosk_id.as_deref() {
        let policy = need_policy(ctx.policy, t)?;
        let pay = tx.split_u64(Arg::Gas, amount(&l.price)?);
        if policy.royalty.is_some() {
            // An upper bound: the change comes back.
            let pol = tx.object(&policy.policy_id);
            let price = tx.pure_u64(amount(&l.price)?);
            let due = tx.move_call(&format!("{}::clearing::royalty_due", ours.package), &[t], &[pol, price]).nth(0);
            let r = tx.split_coins(Arg::Gas, &[due]).nth(0);
            tx.merge_coins(pay, &[r]);
        }
        let args = [tx.object(&l.listing_id), cfg, tx.object(kiosk_id), k.kiosk, k.cap, tx.object(&policy.policy_id), pay];
        let req = tx.move_call(&format!("{}::sale_kiosk::buy", ours.package), &[t], &args).nth(0);
        // Rare Store licences: the hold check and royalty go in before the request is confirmed.
        if let Some(resale) = ctx.cfg.resale.rule() {
            if policy.rules.contains(&resale) {
                add_resale_prove(tx, ctx.cfg, ctx.resale_game, req, k.kiosk, k.cap, Arg::Gas)?;
            }
        }
        let pol = tx.object(&policy.policy_id);
        tx.move_call("0x2::transfer_policy::confirm_request", &[t], &[pol, req]);
        tx.merge_coins(Arg::Gas, &[pay]);
    } else {
        let pay = tx.split_u64(Arg::Gas, amount(&l.price)?);
        let listing = tx.object(&l.listing_id);
        let item = tx.move_call(&format!("{}::sale_escrow::take", ours.package), &[t], &[listing, cfg, pay]).nth(0);
        tx.move_call("0x2::kiosk::place", &[t], &[k.kiosk, k.cap, item]);
        tx.merge_coins(Arg::Gas, &[pay]);
    }
    finish(tx, &k, ctx.sender);
    Ok(())
}

/// The `proof` an offer carries (`extra.proof`, arrays of bytes).
fn proof_of(b: &ActiveBid) -> Vec<Vec<u8>> {
    b.extra
        .get("proof")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().map(|x| x.as_array().map(|bs| bs.iter().filter_map(|n| n.as_u64()).map(|n| n as u8).collect()).unwrap_or_default()).collect())
        .unwrap_or_default()
}

/// Offers (`offer::fill`) and pool members (`offer_pool::fill`) on rare's own contract: the item is
/// bought out of the seller's kiosk into the maker's kiosk with royalty paid from the offer's
/// reserve; the seller receives the proceeds.
fn build_ours_accept_bid(tx: &mut Tx, b: &ActiveBid, ctx: &SellContext) -> Result<(), BuildError> {
    let ours = &ctx.cfg.ours;
    if ours.package.is_empty() || ours.config.is_empty() {
        return Err(BuildError::Unsupported("our contract is not deployed yet".into()));
    }
    let t = ctx.nft.nft_type();
    let cfg = tx.object(&ours.config);
    let proof = tx.pure_vec_bytes(&proof_of(b));
    match &ctx.nft {
        NftAt::Wallet { object_id, .. } => {
            let proceeds = match b.pool_id.as_deref() {
                Some(pool) => {
                    let args = [tx.object(pool), tx.pure_id(&b.bid_id), cfg, tx.object(object_id), proof, tx.object(CLOCK)];
                    tx.move_call(&format!("{}::offer_pool::fill_owned", ours.package), &[t], &args).nth(0)
                }
                None => {
                    let args = [tx.object(&b.bid_id), cfg, tx.object(object_id), proof, tx.object(CLOCK)];
                    tx.move_call(&format!("{}::offer::fill_owned", ours.package), &[t], &args).nth(0)
                }
            };
            tx.transfer_to(&[proceeds], ctx.sender);
            Ok(())
        }
        NftAt::Kiosk { kiosk, nft_id, listed_native, .. } => {
            let policy = need_policy(ctx.policy, t)?;
            let dest = b.extra.get("destKiosk").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).ok_or_else(|| BuildError::Other("offer has no destination kiosk".into()))?;
            let k = open_kiosk(tx, Some(kiosk));
            if *listed_native {
                let id = tx.pure_id(nft_id);
                tx.move_call("0x2::kiosk::delist", &[t], &[k.kiosk, k.cap, id]);
            }
            let args = [cfg, k.kiosk, k.cap, tx.pure_id(nft_id), tx.object(dest), tx.object(&policy.policy_id), proof, tx.object(CLOCK)];
            let r = match b.pool_id.as_deref() {
                Some(pool) => {
                    let mut a = vec![tx.object(pool), tx.pure_id(&b.bid_id)];
                    a.extend(args);
                    tx.move_call(&format!("{}::offer_pool::fill", ours.package), &[t], &a)
                }
                None => {
                    let mut a = vec![tx.object(&b.bid_id)];
                    a.extend(args);
                    tx.move_call(&format!("{}::offer::fill", ours.package), &[t], &a)
                }
            };
            let (proceeds, req) = (r.nth(0), r.nth(1));
            let pol = tx.object(&policy.policy_id);
            tx.move_call("0x2::transfer_policy::confirm_request", &[t], &[pol, req]);
            tx.transfer_to(&[proceeds], ctx.sender);
            finish(tx, &k, ctx.sender);
            Ok(())
        }
    }
}

// ---------- buildBuy ----------

/// Buy one listing. Adds commands to `tx`.
pub fn build_buy(tx: &mut Tx, l: &ActiveListing, ctx: &BuyContext) -> Result<(), BuildError> {
    let t = match l.nft_type.as_deref() {
        Some(t) if !t.is_empty() => t,
        _ => return Err(BuildError::Unsupported("listing has no NFT type yet".into())),
    };
    let sender = ctx.sender;
    let personal = ctx.kiosk.is_some_and(|k| k.personal);
    let kiosk_id = || l.kiosk_id.clone().unwrap_or_default();
    let src = l.source.as_str();
    match src {
        "tradeport/simple" => {
            let ob = tx.object(tp::orderbook::STORE);
            let store = tx.object(tp::simple::STORE);
            if !truthy(l.extra.get("inOrderbook")) {
                // Listings made by pre-orderbook code must join the orderbook before buy_ob_* can remove them.
                let id = tx.pure_id(&l.nft_id);
                tx.move_call(&format!("{}::tradeport_listings::report_ob_listing", tp::simple::LATEST), &[t], &[store, ob, id]);
            }
            let pay = tx.split_u64(Arg::Gas, amount(&l.price)? + amount(&l.fee)?);
            if num_is(l.extra.get("listingType"), 1.0) {
                let policy = need_policy(ctx.policy, t)?;
                if policy.royalty.is_some() {
                    let pol = tx.object(&policy.policy_id);
                    let price = tx.pure_u64(amount(&l.price)?);
                    let roy = tx.move_call(&format!("{}::royalty_rule::fee_amount", kiosk_ext::LATEST), &[t], &[pol, price]).nth(0);
                    let r = tx.split_coins(Arg::Gas, &[roy]).nth(0);
                    tx.merge_coins(pay, &[r]);
                }
                let k = open_kiosk(tx, ctx.kiosk);
                let args = [store, ob, tx.object(&kiosk_id()), k.kiosk, k.cap, tx.pure_address(sender), tx.pure_id(&l.nft_id), pay, tx.object(&policy.policy_id)];
                let req = tx.move_call(&format!("{}::tradeport_listings::buy_ob_listing_with_transfer_policy", tp::simple::LATEST), &[t], &args).nth(0);
                // The call takes its fee, pays royalty, locks or places the item and proves lock, floor and personal rules.
                let rl = rules();
                let mut p = policy.clone();
                p.rules.retain(|r| *r != rl.royalty);
                let proved = [rl.kiosk_lock.clone(), rl.floor_price.clone(), rl.personal_kiosk.clone()];
                resolve(tx, ctx.cfg, Resolve { policy: &p, nft_type: t, req, item: None, into: Some((k.kiosk, k.cap)), pay_from: Arg::Gas, proved: &proved, personal_kiosk: personal, resale_game: ctx.resale_game })?;
                finish(tx, &k, sender);
            } else {
                let id = tx.pure_id(&l.nft_id);
                tx.move_call(&format!("{}::tradeport_listings::buy_ob_listing_without_transfer_policy", tp::simple::LATEST), &[t], &[store, ob, id, pay]);
            }
            tx.merge_coins(Arg::Gas, &[pay]);
            Ok(())
        }
        "tradeport/kiosk" => {
            let policy = need_policy(ctx.policy, t)?;
            let k = open_kiosk(tx, ctx.kiosk);
            let pay = tx.split_u64(Arg::Gas, amount(&l.price)? + amount(&l.fee)?);
            let args = [tx.object(tp::kiosk::LISTINGS_STORE), tx.object(&kiosk_id()), k.kiosk, tx.pure_id(&l.nft_id), pay];
            let r = tx.move_call(&format!("{}::kiosk_listings::buy", tp::kiosk::LATEST), &[t], &args);
            resolve(tx, ctx.cfg, Resolve { policy, nft_type: t, req: r.nth(1), item: Some(r.nth(0)), into: Some((k.kiosk, k.cap)), pay_from: Arg::Gas, proved: &[], personal_kiosk: personal, resale_game: ctx.resale_game })?;
            finish(tx, &k, sender);
            tx.merge_coins(Arg::Gas, &[pay]);
            Ok(())
        }
        "tradeport/kiosk-early" => {
            let policy = need_policy(ctx.policy, t)?;
            let k = open_kiosk(tx, ctx.kiosk);
            // The early module wants exactly price + commission (commission on top).
            let pay = tx.split_u64(Arg::Gas, amount(&l.price)? + amount(&l.fee)? + amount(&l.commission)?);
            let args = [tx.object(tp::kiosk_early::LISTINGS_STORE), tx.object(&kiosk_id()), k.kiosk, k.cap, tx.pure_id(&l.nft_id), pay];
            let r = tx.move_call(&format!("{}::kiosk_listings::buy", tp::kiosk_early::LATEST), &[t], &args);
            resolve(tx, ctx.cfg, Resolve { policy, nft_type: t, req: r.nth(1), item: Some(r.nth(0)), into: Some((k.kiosk, k.cap)), pay_from: Arg::Gas, proved: &[], personal_kiosk: personal, resale_game: ctx.resale_game })?;
            finish(tx, &k, sender);
            tx.merge_coins(Arg::Gas, &[pay]);
            Ok(())
        }
        "tradeport/escrow" => {
            let pay = tx.split_u64(Arg::Gas, amount(&l.price)?);
            let args = [tx.object(tp::escrow::LISTINGS_STORE), tx.pure_id(&l.listing_id), pay];
            tx.move_call(&format!("{}::listings::buy", tp::escrow::LATEST), &[t], &args);
            tx.merge_coins(Arg::Gas, &[pay]);
            Ok(())
        }
        "drip/v2" => {
            let policy = need_policy(ctx.policy, t)?;
            let k = open_kiosk(tx, ctx.kiosk);
            if ctx.drip_direct {
                // Drip v2 listings are plain public kiosk listings: buy at the listed price with
                // kiosk::purchase (no marketplace fee), settle the policy into the buyer's kiosk...
                let pay = tx.split_u64(Arg::Gas, amount(&l.price)?);
                let args = [tx.object(&kiosk_id()), tx.pure_id(&l.nft_id), pay];
                let r = tx.move_call("0x2::kiosk::purchase", &[t], &args);
                resolve(tx, ctx.cfg, Resolve { policy, nft_type: t, req: r.nth(1), item: Some(r.nth(0)), into: Some((k.kiosk, k.cap)), pay_from: Arg::Gas, proved: &[], personal_kiosk: personal, resale_game: ctx.resale_game })?;
                // ...then clear Drip's now stale record so the buyer can list it on Drip later: its
                // delist needs the item listed in the kiosk passed, so list it for a moment and
                // delist through Drip.
                let (id, one) = (tx.pure_id(&l.nft_id), tx.pure_u64(1));
                tx.move_call("0x2::kiosk::list", &[t], &[k.kiosk, k.cap, id, one]);
                let (reg, id) = (tx.object(drip_v2::REGISTRY), tx.pure_id(&l.nft_id));
                tx.move_call(&format!("{}::marketplace::delist", drip_v2::MARKET), &[t], &[reg, k.kiosk, k.cap, id]);
                finish(tx, &k, sender);
                return Ok(());
            }
            let pay = tx.split_u64(Arg::Gas, amount(&l.price)? + amount(&l.fee)?);
            let args = [tx.object(drip_v2::REGISTRY), tx.object(&kiosk_id()), tx.pure_id(&l.nft_id), pay];
            let r = tx.move_call(&format!("{}::marketplace::buy", drip_v2::MARKET), &[t], &args);
            resolve(tx, ctx.cfg, Resolve { policy, nft_type: t, req: r.nth(1), item: Some(r.nth(0)), into: Some((k.kiosk, k.cap)), pay_from: Arg::Gas, proved: &[], personal_kiosk: personal, resale_game: ctx.resale_game })?;
            finish(tx, &k, sender);
            Ok(())
        }
        "kiosk/native" => {
            let policy = need_policy(ctx.policy, t)?;
            let k = open_kiosk(tx, ctx.kiosk);
            let pay = tx.split_u64(Arg::Gas, amount(&l.price)?);
            let args = [tx.object(&kiosk_id()), tx.pure_id(&l.nft_id), pay];
            let r = tx.move_call("0x2::kiosk::purchase", &[t], &args);
            resolve(tx, ctx.cfg, Resolve { policy, nft_type: t, req: r.nth(1), item: Some(r.nth(0)), into: Some((k.kiosk, k.cap)), pay_from: Arg::Gas, proved: &[], personal_kiosk: personal, resale_game: ctx.resale_game })?;
            finish(tx, &k, sender);
            Ok(())
        }
        "market/v1" => build_ours_buy(tx, l, ctx),
        _ => Err(BuildError::Unsupported(format!("buying {src} is not supported"))),
    }
}

// ---------- buildAcceptBid ----------

/// Sell into a collection bid or accept an item offer. The NFT always reaches the bidder.
pub fn build_accept_bid(tx: &mut Tx, b: &ActiveBid, ctx: &SellContext) -> Result<(), BuildError> {
    if !b.fillable {
        return Err(BuildError::Unsupported("bid cannot be filled (pool short or expired)".into()));
    }
    let t = ctx.nft.nft_type();
    if let Some(bt) = b.nft_type.as_deref().filter(|s| !s.is_empty()) {
        if bt != t {
            return Err(BuildError::Unsupported(format!("bid is for {bt}, NFT is {t}")));
        }
    }
    if let (true, Some(bid_nft), NftAt::Kiosk { nft_id, .. }) = (b.kind == "item", b.nft_id.as_deref().filter(|s| !s.is_empty()), &ctx.nft) {
        if *nft_id != bid_nft {
            return Err(BuildError::Unsupported("offer is for another item".into()));
        }
    }
    let sender = ctx.sender;
    let nft = &ctx.nft;
    let prep_kiosk = |tx: &mut Tx| -> (KioskArgs, String) {
        match nft {
            NftAt::Kiosk { kiosk, nft_id, listed_native, .. } => {
                let k = open_kiosk(tx, Some(kiosk));
                if *listed_native {
                    let id = tx.pure_id(nft_id);
                    tx.move_call("0x2::kiosk::delist", &[t], &[k.kiosk, k.cap, id]);
                }
                (k, nft_id.to_string())
            }
            NftAt::Wallet { object_id, kiosk, .. } => {
                let k = open_kiosk(tx, *kiosk);
                let obj = tx.object(object_id);
                tx.move_call("0x2::kiosk::place", &[t], &[k.kiosk, k.cap, obj]);
                (k, object_id.to_string())
            }
        }
    };
    let pooled = |tx: &mut Tx, pkg: &str, module: &str, store: &str| -> Result<(), BuildError> {
        let pool = tx.pure_option_address(b.pool_id.as_deref());
        if let (NftAt::Wallet { object_id, .. }, None) = (nft, ctx.policy) {
            let args = [tx.object(CLOCK), tx.object(store), tx.pure_id(&b.bid_id), pool, tx.object(object_id)];
            tx.move_call(&format!("{pkg}::{module}::accept_bid_without_transfer_policy"), &[t], &args);
            return Ok(());
        }
        let policy = need_policy(ctx.policy, t)?;
        let (k, nft_id) = prep_kiosk(tx);
        // The contract pays seller, royalty and fee from the bid, locks the item in a new kiosk
        // for the bidder and proves lock, personal and floor rules.
        let args = [tx.object(CLOCK), tx.object(store), tx.pure_id(&b.bid_id), pool, k.kiosk, k.cap, tx.pure_id(&nft_id), tx.object(&policy.policy_id)];
        let req = tx.move_call(&format!("{pkg}::{module}::accept_bid_with_transfer_policy"), &[t], &args).nth(0);
        let pol = tx.object(&policy.policy_id);
        tx.move_call("0x2::transfer_policy::confirm_request", &[t], &[pol, req]);
        finish(tx, &k, sender);
        Ok(())
    };
    let wallet_only = |tx: &mut Tx, pkg: &str, store: &str| -> Result<(), BuildError> {
        let NftAt::Wallet { object_id, .. } = nft else {
            return Err(BuildError::Unsupported("this bid takes wallet-held NFTs only".into()));
        };
        let args = [tx.object(store), tx.pure_id(&b.bid_id), tx.object(object_id)];
        tx.move_call(&format!("{pkg}::biddings::accept_bid"), &[t], &args);
        Ok(())
    };
    let rl = rules();
    match b.source.as_str() {
        "tradeport/bids" => pooled(tx, tp::bids::LATEST, "tradeport_biddings", tp::bids::STORE),
        "tradeport/escrow-bids" => wallet_only(tx, tp::escrow::LATEST, tp::escrow::BIDDINGS_STORE),
        "tradeport/kiosk-bids" => {
            let policy = need_policy(ctx.policy, t)?;
            if policy.rules.contains(&rl.personal_kiosk) {
                return Err(BuildError::Unsupported("personal kiosk rule: Tradeport's escrow kiosk cannot satisfy it".into()));
            }
            let (k, nft_id) = prep_kiosk(tx);
            let escrow = tx.object(tp::kiosk::ESCROW_KIOSK);
            // The item moves to Tradeport's escrow kiosk, locked for the bidder to claim; the price
            // reaches the seller.
            let args = [tx.object(tp::kiosk::BIDDINGS_STORE), tx.pure_id(&b.bid_id), escrow, k.kiosk, k.cap, tx.pure_id(&nft_id), tx.object(&policy.policy_id)];
            let r = tx.move_call(&format!("{}::kiosk_biddings::accept_bid", tp::kiosk::LATEST), &[t], &args);
            let (leftover, req) = (r.nth(0), r.nth(1));
            resolve(tx, ctx.cfg, Resolve { policy, nft_type: t, req, item: None, into: Some((escrow, k.cap)), pay_from: Arg::Gas, proved: &[], personal_kiosk: false, resale_game: None })?;
            tx.transfer_to(&[leftover], sender);
            finish(tx, &k, sender);
            Ok(())
        }
        "drip/v2-bids" => {
            let policy = need_policy(ctx.policy, t)?;
            if policy.rules.contains(&rl.personal_kiosk) {
                return Err(BuildError::Unsupported("personal kiosk rule: cannot deliver to the bidder's personal kiosk".into()));
            }
            let (k, nft_id) = prep_kiosk(tx);
            let r = if b.kind == "item" {
                let bid = tx.object(&b.bid_id);
                tx.move_call(&format!("{}::bids::accept_offer", drip_v2::BIDS), &[t], &[bid, k.kiosk, k.cap])
            } else {
                let (bid, id) = (tx.object(&b.bid_id), tx.pure_id(&nft_id));
                tx.move_call(&format!("{}::bids::accept_bid", drip_v2::BIDS), &[t], &[bid, k.kiosk, k.cap, id])
            };
            let (item, req) = (r.nth(0), r.nth(1));
            // Sale proceeds land in the seller kiosk's profits: take them now, pay royalty from them.
            let none = tx.pure_option_u64(None);
            let proceeds = tx.move_call("0x2::kiosk::withdraw", &[], &[k.kiosk, k.cap, none]).nth(0);
            // Deliver to the bidder: a new kiosk, shared, its cap sent to the bidder.
            let nk = tx.move_call("0x2::kiosk::new", &[], &[]);
            let (bk, bcap) = (nk.nth(0), nk.nth(1));
            resolve(tx, ctx.cfg, Resolve { policy, nft_type: t, req, item: Some(item), into: Some((bk, bcap)), pay_from: proceeds, proved: &[], personal_kiosk: false, resale_game: None })?;
            tx.move_call("0x2::transfer::public_share_object", &[KIOSK], &[bk]);
            let bidder = tx.pure_address(&b.bidder);
            tx.transfer_objects(&[bcap], bidder);
            tx.transfer_to(&[proceeds], sender);
            finish(tx, &k, sender);
            Ok(())
        }
        "market/v1" => build_ours_accept_bid(tx, b, ctx),
        other => Err(BuildError::Unsupported(format!("selling into {other} is not supported"))),
    }
}
