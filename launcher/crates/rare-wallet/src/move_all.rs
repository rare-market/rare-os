//! "Move everything to this account": one transaction (more only past Sui's limits) that sends
//! everything an old Rare account holds to a new one, signed once by the old account's own method
//! (its PIN, phone passkey, Ledger...). Reads with the rescue scan (`rescue::scan_playtron`).
//!
//! - Kiosks (plain `KioskOwnerCap`): the cap goes, so the kiosk with every item, listing, lock and
//!   sale proceeds moves whole; its owner field is set to the new account first.
//! - Personal kiosks (the cap can never leave): each item comes out through the NFT send builder
//!   (`nft::transfer`), which follows transfer policies (locked items into a kiosk made for the new
//!   account, personal-kiosk rules into a new personal kiosk). Items it can't move stay, with why.
//! - Collectibles, staked SUI and SuiNS names held directly: transferred.
//! - Coins: every other coin in full (`coinWithBalance`, coin objects and address balance).
//! - SUI last: the gas coin itself, paid with every SUI coin (and backed by the address balance
//!   when SUI sits there), so the new account gets all of it less the fee.
//! - Soulbound objects (no `store`) can't move and are listed.

use crate::nft::{self, NftRef, SendItem};
use crate::rescue::{self, RescueScan, SUI_TYPE};
use rare_sui::grpc::{ChainRead, MAX_GAS_COINS};
use rare_sui::ptb::{Arg, Tx};
use rare_sui::tx::BuildOpts;

/// Objects per TransferObjects command, and inputs per transaction (Sui allows 2048).
const PER_COMMAND: usize = 256;
const MAX_INPUTS: usize = 1500;

#[derive(Clone, Debug, PartialEq)]
pub struct Stuck {
    pub label: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MoveAll {
    pub from: String,
    pub to: String,
    /// in signing order (SUI last); usually one
    pub txs: Vec<(Tx, BuildOpts)>,
    pub sui: u128,
    /// (symbol, raw, decimals) of every other coin
    pub coins: Vec<(String, u128, u32)>,
    pub nfts: usize,
    pub kiosks: usize,
    pub kiosk_items: usize,
    pub staked: usize,
    pub names: usize,
    pub stuck: Vec<Stuck>,
    /// more objects than the scan reads: run it again after the move
    pub truncated: bool,
}

impl MoveAll {
    pub fn is_empty(&self) -> bool {
        self.txs.is_empty()
    }
}

/// Builds the move of everything `from` holds to `to`. `nfts` reads personal-kiosk items'
/// sendability (`nft::nft_sendability` on the live node).
pub async fn plan_move_all(chain: &dyn ChainRead, nfts: &(dyn Fn(Vec<NftRef>) -> rare_sui::BoxFut<'static, std::collections::HashMap<String, nft::Sendability>> + Sync), from: &str, to: &str) -> Result<MoveAll, String> {
    let from = rare_sui::ids::canon_addr(from);
    let to = rare_sui::ids::canon_addr(to);
    if from == to {
        return Err("Pick an account other than this one".into());
    }
    let scan = rescue::scan_playtron(chain, &from, &|_| {}).await.map_err(|e| e.to_string())?;
    // personal kiosk items: what the NFT builder can move
    let refs: Vec<NftRef> = scan.kiosks.iter().filter(|k| k.personal).flat_map(|k| k.items.iter().map(|i| NftRef { object_id: i.item.object_id.clone(), type_: i.item.type_.clone(), kiosk: true })).collect();
    let send = if refs.is_empty() { Default::default() } else { nfts(refs).await.map_err(|e| e.to_string())? };
    let sui_coins = chain.list_coins(&from, SUI_TYPE, MAX_GAS_COINS).await.map_err(|e| e.to_string())?;
    let sui_in_address = chain.balance(&from, SUI_TYPE).await.map(|b| b.address_balance.unwrap_or(0)).unwrap_or(0);
    let mut m = build(&scan, &send, &to)?;
    add_sui(&mut m, &from, &to, sui_coins, sui_in_address);
    Ok(m)
}

/// The last transaction pays with every SUI coin and sends the gas coin itself. SUI in the address
/// balance: no coins named, so the node backs the gas coin with the whole address balance (a coin
/// reservation) plus every SUI coin.
fn add_sui(m: &mut MoveAll, from: &str, to: &str, sui_coins: Vec<rare_sui::grpc::CoinRef>, sui_in_address: u64) {
    if sui_coins.is_empty() && sui_in_address == 0 {
        return;
    }
    let payment = if sui_in_address > 0 { Vec::new() } else { sui_coins };
    if let Some(last) = m.txs.last_mut() {
        last.0.transfer_to(&[Arg::Gas], to);
        last.1.gas_payment = payment;
    } else {
        let mut tx = Tx::new();
        tx.set_sender(from);
        tx.transfer_to(&[Arg::Gas], to);
        m.txs.push((tx, BuildOpts { gas_payment: payment, ..Default::default() }));
    }
}

/// The transactions for a scan (pure, no chain): everything but the SUI gas coin.
pub fn build(scan: &RescueScan, send: &std::collections::HashMap<String, nft::Sendability>, to: &str) -> Result<MoveAll, String> {
    let from = scan.address.clone();
    let mut out = MoveAll { from: from.clone(), to: to.to_string(), truncated: scan.truncated, ..Default::default() };
    let mut txs: Vec<Tx> = Vec::new();
    let new_tx = || {
        let mut t = Tx::new();
        t.set_sender(&from);
        t
    };
    let mut tx = new_tx();
    let mut inputs = 0usize;
    // objects sent as they are, grouped into TransferObjects commands
    let mut plain: Vec<String> = Vec::new();

    for k in scan.kiosks.iter().filter(|k| !k.personal) {
        let (kiosk, cap) = (tx.object(&k.kiosk_id), tx.object(&k.cap_id));
        let owner = tx.pure_address(to);
        tx.move_call("0x2::kiosk::set_owner_custom", &[], &[kiosk, cap, owner]);
        tx.transfer_to(&[cap], to);
        inputs += 3;
        out.kiosks += 1;
        out.kiosk_items += k.items.len();
    }
    let mut items: Vec<SendItem> = Vec::new();
    for k in scan.kiosks.iter().filter(|k| k.personal) {
        for i in &k.items {
            match send.get(&i.item.object_id) {
                Some(nft::Sendability::Ok(s)) => items.push(s.clone()),
                Some(nft::Sendability::No(why)) => out.stuck.push(Stuck { label: i.item.name.clone(), reason: why.clone() }),
                None => out.stuck.push(Stuck { label: i.item.name.clone(), reason: "Could not read its kiosk. Try again in a moment".into() }),
            }
        }
        if k.profits > 0 {
            out.stuck.push(Stuck { label: format!("{} SUI of sales", rescue::format_amount(k.profits, 9)), reason: "Sale proceeds in a personal kiosk stay there. Withdraw them on rare.market.".into() });
        }
    }
    if !items.is_empty() {
        out.kiosk_items += items.len();
        inputs += items.len() * 4;
        tx = nft::transfer::build_send_nfts_into(&from, to, &items, tx).map_err(|e| e.message().to_string())?;
    }
    for n in &scan.nfts {
        plain.push(n.object_id.clone());
        out.nfts += 1;
    }
    for s in &scan.staked {
        plain.push(s.object_id.clone());
        out.staked += 1;
    }
    for n in &scan.names {
        plain.push(n.object_id.clone());
        out.names += 1;
    }
    for chunk in plain.chunks(PER_COMMAND) {
        if inputs + chunk.len() > MAX_INPUTS && !tx.commands.is_empty() {
            txs.push(std::mem::replace(&mut tx, new_tx()));
            inputs = 0;
        }
        let args: Vec<Arg> = chunk.iter().map(|id| tx.object(id)).collect();
        tx.transfer_to(&args, to);
        inputs += chunk.len() + 1;
    }
    for c in scan.coins.iter().filter(|c| c.coin_type != SUI_TYPE && c.raw > 0) {
        let Ok(amount) = u64::try_from(c.raw) else {
            out.stuck.push(Stuck { label: c.symbol.clone(), reason: "Too large to move in one go".into() });
            continue;
        };
        let coin = tx.coin_with_balance(&c.coin_type, amount);
        tx.transfer_to(&[coin], to);
        let _ = inputs;
        out.coins.push((c.symbol.clone(), c.raw, c.decimals));
    }
    if let Some(s) = scan.coins.iter().find(|c| c.coin_type == SUI_TYPE) {
        out.sui = s.raw;
    }
    for s in &scan.soulbound {
        out.stuck.push(Stuck { label: if s.name.is_empty() { "Item".into() } else { s.name.clone() }, reason: rescue::stuck_reason(&s.type_).into() });
    }
    if !tx.commands.is_empty() {
        txs.push(tx);
    }
    out.txs = txs.into_iter().map(|t| (t, BuildOpts::default())).collect();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rescue::{RescueCoin, RescueItem, RescueKiosk, RescueKioskItem, RescueStake};

    fn a(c: &str) -> String {
        format!("0x{}", c.repeat(64))
    }

    #[test]
    fn one_transaction_moves_kiosks_items_coins() {
        let scan = RescueScan {
            address: a("1"),
            coins: vec![
                RescueCoin { coin_type: SUI_TYPE.into(), symbol: "SUI".into(), decimals: 9, raw: 5_000_000_000, ..Default::default() },
                RescueCoin { coin_type: "0xabc::usdc::USDC".into(), symbol: "USDC".into(), decimals: 6, raw: 1_500_000, object_ids: vec![a("7")], ..Default::default() },
            ],
            nfts: vec![RescueItem { object_id: a("2"), type_: "0xa::n::N".into(), name: "Ghost".into(), ..Default::default() }],
            kiosks: vec![RescueKiosk {
                kiosk_id: a("3"),
                cap_id: a("4"),
                personal: false,
                items: vec![RescueKioskItem { item: RescueItem { object_id: a("5"), ..Default::default() }, locked: true, transferable: true, ..Default::default() }],
                ..Default::default()
            }],
            staked: vec![RescueStake { object_id: a("6"), principal: 1, ..Default::default() }],
            soulbound: vec![RescueItem { object_id: a("8"), type_: "0xa::badge::Badge".into(), name: "Badge".into(), ..Default::default() }],
            ..Default::default()
        };
        let m = build(&scan, &Default::default(), &a("9")).unwrap();
        assert_eq!(m.txs.len(), 1);
        let j = m.txs[0].0.to_json();
        let kinds: Vec<&str> = j["commands"].as_array().unwrap().iter().map(|c| c["$kind"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["MoveCall", "TransferObjects", "TransferObjects", "$Intent", "TransferObjects"]);
        assert_eq!(m.txs[0].0.targets(), ["0x0000000000000000000000000000000000000000000000000000000000000002::kiosk::set_owner_custom"]);
        assert_eq!((m.kiosks, m.kiosk_items, m.nfts, m.staked, m.sui), (1, 1, 1, 1, 5_000_000_000));
        assert_eq!(m.coins, vec![("USDC".to_string(), 1_500_000, 6)]);
        assert_eq!(m.stuck.len(), 1);
        assert_eq!(build(&RescueScan { address: a("1"), ..Default::default() }, &Default::default(), &a("9")).unwrap().txs.len(), 0);
    }
}
