//! The move plan for a Playtron wallet: what to sign, in which order (plan.ts `planMoves`).
//! Kiosks first, then staked SUI, SuiNS names, collectibles, and coins last so there is gas for
//! every earlier step. The unsigned transactions (`buildBatch`) are not ported: nothing in Rare
//! can sign for the old wallet, so the plan is shown for the person to carry out there.

use super::scan::{stuck_reason, SUI_TYPE};
use super::*;
use rare_sui::ids::canon_addr;

/// objects per transaction
const PER_TX: usize = 200;
/// coin objects per transaction
const MAX_COIN_OBJECTS: usize = 1000;
/// SUI kept in mind for fees per transaction (MIST), for the low-gas warning
pub const FEE_PER_TX: u128 = 20_000_000;

/// How many of the largest SUI coins to leave out of explicit transfers so they can pay gas.
fn keep_for_gas(bals: &[u128], n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    if bals.len() != n {
        return 1;
    }
    let mut sum = 0u128;
    for (i, b) in bals.iter().enumerate().take(n.min(50)) {
        sum += b;
        if sum >= FEE_PER_TX * 4 {
            return i + 1;
        }
    }
    n.min(50)
}

fn group3(n: u128) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// plan.ts `formatAmount(raw, decimals, 4)`.
pub fn format_amount(raw: u128, decimals: u32) -> String {
    let max = 4usize;
    let base = 10u128.pow(decimals);
    let whole = raw / base;
    let mut frac: String = format!("{:0>w$}", raw % base, w = decimals as usize).chars().take(max).collect();
    frac = frac.trim_end_matches('0').to_string();
    if frac.is_empty() && whole == 0 && raw > 0 {
        frac = format!("{}1", "0".repeat(max - 1));
    }
    if frac.is_empty() {
        group3(whole)
    } else {
        format!("{}.{frac}", group3(whole))
    }
}

fn sui(mist: u128) -> String {
    format!("{} SUI", format_amount(mist, 9))
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

struct Group {
    title: String,
    lines: Vec<MoveLine>,
}

/// What to move and in which order. `gas` is who pays fees: the wallet itself or a sponsor.
pub fn plan_moves(scan: &RescueScan, to: &str, gas: GasMode) -> MovePlan {
    let dest = canon_addr(to);
    let mut groups: Vec<Group> = Vec::new();
    let mut stuck: Vec<StuckLine> = Vec::new();

    // 1. Kiosks: every normal kiosk moves whole, in one transaction.
    let whole: Vec<&RescueKiosk> = scan.kiosks.iter().filter(|k| !k.personal).collect();
    for (i, part) in whole.chunks(PER_TX).enumerate() {
        let title = if whole.len() > PER_TX {
            format!("Kiosks, part {}", i + 1)
        } else if whole.len() > 1 {
            "Kiosks".to_string()
        } else {
            "Kiosk".to_string()
        };
        let lines = part
            .iter()
            .map(|k| {
                let listed = k.items.iter().filter(|it| it.listed.is_some()).count();
                let mut parts = vec![if k.items.is_empty() { "Empty".to_string() } else { format!("{} item{} inside", k.items.len(), plural(k.items.len())) }];
                if listed > 0 {
                    parts.push(format!("{listed} listed, still listed after the move"));
                }
                if k.profits > 0 {
                    parts.push(format!("{} in sales", sui(k.profits)));
                }
                MoveLine { kind: "kiosk", id: k.kiosk_id.clone(), label: "Kiosk and everything in it".into(), detail: Some(parts.join(", ")) }
            })
            .collect();
        groups.push(Group { title, lines });
    }
    for k in scan.kiosks.iter().filter(|k| k.personal) {
        let movable = |i: &&RescueKioskItem| !i.locked && i.transferable && !i.exclusive;
        for i in &k.items {
            let it = &i.item;
            if i.exclusive {
                stuck.push(StuckLine { id: it.object_id.clone(), label: it.name.clone(), reason: "Listed through a marketplace. Cancel that listing on the marketplace, then scan again.".into() });
            } else if i.locked {
                stuck.push(StuckLine { id: it.object_id.clone(), label: it.name.clone(), reason: "Locked by its collection's rules in a personal kiosk. Sell it, or move it with a marketplace that supports the collection.".into() });
            } else if !i.transferable {
                stuck.push(StuckLine { id: it.object_id.clone(), label: it.name.clone(), reason: stuck_reason(&it.type_).into() });
            }
        }
        let mut lines: Vec<MoveLine> = k
            .items
            .iter()
            .filter(movable)
            .map(|i| MoveLine {
                kind: "kiosk",
                id: i.item.object_id.clone(),
                label: i.item.name.clone(),
                detail: Some(match i.listed {
                    Some(p) => format!("Listed for {}: delisted first", sui(p)),
                    None => "From your personal kiosk".into(),
                }),
            })
            .collect();
        if k.profits > 0 {
            lines.push(MoveLine { kind: "coin", id: k.kiosk_id.clone(), label: "Kiosk sales".into(), detail: Some(sui(k.profits)) });
        }
        for (i, part) in lines.chunks(PER_TX / 2).enumerate() {
            groups.push(Group { title: if i > 0 { "Personal kiosk (continued)".into() } else { "Personal kiosk".into() }, lines: part.to_vec() });
        }
    }

    // 2. Staked SUI
    for part in scan.staked.chunks(PER_TX) {
        groups.push(Group {
            title: "Staked SUI".into(),
            lines: part.iter().map(|s| MoveLine { kind: "stake", id: s.object_id.clone(), label: format!("{} staked", sui(s.principal)), detail: Some("Moves as it is and keeps earning".into()) }).collect(),
        });
    }

    // 3. SuiNS names
    if !scan.names.is_empty() {
        groups.push(Group {
            title: "SuiNS names".into(),
            lines: scan
                .names
                .iter()
                .map(|n| MoveLine {
                    kind: "name",
                    id: n.object_id.clone(),
                    label: n.domain.clone(),
                    detail: (scan.default_name.as_deref() == Some(n.domain.as_str())).then(|| "Points at the old wallet now: point it at your new one after it arrives".to_string()),
                })
                .collect(),
        });
    }

    // 4. Collectibles and other objects
    for (i, part) in scan.nfts.chunks(PER_TX).enumerate() {
        groups.push(Group {
            title: if i > 0 { "Collectibles (continued)".into() } else { "Collectibles".into() },
            lines: part.iter().map(|n| MoveLine { kind: "nft", id: n.object_id.clone(), label: n.name.clone(), detail: Some(n.collection.clone()) }).collect(),
        });
    }

    // 5. Coins, last. Packed so no transaction passes the input limit.
    let sui_coin = scan.coins.iter().find(|c| c.coin_type == SUI_TYPE);
    let mut portions: Vec<(MoveLine, usize)> = Vec::new();
    let mut add_coin = |c: &RescueCoin, ids: &[String], bals: &[u128]| {
        for (i, part) in ids.chunks(MAX_COIN_OBJECTS).enumerate() {
            let from = i * MAX_COIN_OBJECTS;
            let part_bals = bals.get(from..(from + part.len()).min(bals.len()));
            let sum: u128 = match part_bals {
                Some(b) if b.len() == part.len() => b.iter().sum(),
                _ => c.coin_raw,
            };
            let address = if i == 0 { c.address_raw } else { 0 };
            portions.push((
                MoveLine { kind: "coin", id: c.coin_type.clone(), label: c.symbol.clone(), detail: Some(format!("{} {}", format_amount(sum + address, c.decimals), c.symbol)) },
                part.len() + usize::from(address > 0),
            ));
        }
        if ids.is_empty() && c.address_raw > 0 {
            portions.push((MoveLine { kind: "coin", id: c.coin_type.clone(), label: c.symbol.clone(), detail: Some(format!("{} {}", format_amount(c.address_raw, c.decimals), c.symbol)) }, 1));
        }
    };
    for c in scan.coins.iter().filter(|c| c.coin_type != SUI_TYPE) {
        add_coin(c, &c.object_ids, &c.object_balances);
    }
    let mut gas_line: Option<MoveLine> = None;
    if let Some(c) = sui_coin {
        if gas == GasMode::Own {
            // Keep the largest coins for fees; they leave last as the gas coin itself.
            let keep = keep_for_gas(&c.object_balances, c.object_ids.len());
            let ids = &c.object_ids[keep.min(c.object_ids.len())..];
            let bals = if c.object_balances.len() > keep { &c.object_balances[keep..] } else { &[][..] };
            add_coin(c, ids, bals);
            if keep > 0 {
                gas_line = Some(MoveLine { kind: "coin", id: SUI_TYPE.into(), label: "SUI".into(), detail: Some(format!("All SUI that's left after fees (about {} in total)", sui(c.raw))) });
            }
        } else {
            add_coin(c, &c.object_ids, &c.object_balances);
        }
    }
    let mut coin_groups: Vec<(Vec<MoveLine>, usize)> = Vec::new();
    for (line, size) in portions {
        match coin_groups.last_mut() {
            Some(g) if g.1 + size <= MAX_COIN_OBJECTS => {
                g.0.push(line);
                g.1 += size;
            }
            _ => coin_groups.push((vec![line], size)),
        }
    }
    if let Some(gl) = gas_line {
        match coin_groups.last_mut() {
            Some(g) => g.0.push(gl),
            None => coin_groups.push((vec![gl], 0)),
        }
    }
    let n = coin_groups.len();
    for (i, g) in coin_groups.into_iter().enumerate() {
        groups.push(Group { title: if n > 1 { format!("Coins, part {} of {n}", i + 1) } else { "Coins".into() }, lines: g.0 });
    }

    for s in &scan.soulbound {
        stuck.push(StuckLine { id: s.object_id.clone(), label: s.name.clone(), reason: stuck_reason(&s.type_).into() });
    }

    let batches: Vec<MoveBatch> = groups.into_iter().enumerate().map(|(i, g)| MoveBatch { index: i as u32 + 1, title: g.title, lines: g.lines }).collect();
    let mut fee_note = None;
    if gas == GasMode::Own && !batches.is_empty() {
        let need = FEE_PER_TX * batches.len() as u128;
        let have = sui_coin.map_or(0, |c| c.raw);
        if have < need {
            fee_note = Some(format!(
                "Each step needs a little SUI for its fee. This wallet has {}; send it about {} more first, or use a wallet that pays fees for you.",
                sui(have),
                sui(need - have)
            ));
        }
    }
    let empty = batches.is_empty();
    MovePlan { from: scan.address.clone(), to: dest, batches, stuck, empty, fee_note }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(n: u64) -> String {
        format!("0x{n:064x}")
    }
    fn from() -> String {
        a(0xf00)
    }
    fn to() -> String {
        a(0xabc)
    }
    fn usdc() -> String {
        format!("{}::usdc::USDC", a(0xdba3))
    }

    fn coin(coin_type: &str, symbol: &str, n: usize, each: u128, address: u128) -> RescueCoin {
        let off = if symbol == "SUI" { 0 } else { 0x8000 };
        let ids: Vec<String> = (0..n).map(|i| a(0x10000 + i as u64 + off)).collect();
        let coin_raw = each * n as u128;
        RescueCoin {
            coin_type: coin_type.into(),
            symbol: symbol.into(),
            name: symbol.into(),
            decimals: if symbol == "SUI" { 9 } else { 6 },
            raw: coin_raw + address,
            coin_raw,
            address_raw: address,
            object_balances: ids.iter().map(|_| each).collect(),
            object_ids: ids,
            ..Default::default()
        }
    }

    fn scan(f: impl FnOnce(&mut RescueScan)) -> RescueScan {
        let mut s = RescueScan { address: from(), ..Default::default() };
        f(&mut s);
        s
    }

    fn kiosk(id: u64, personal: bool, items: Vec<RescueKioskItem>) -> RescueKiosk {
        RescueKiosk { kiosk_id: a(id), cap_id: a(id + 1), personal, personal_package: personal.then(|| a(0xdfb4)), items, profits: 0 }
    }

    fn nft(n: u64, name: &str) -> RescueItem {
        RescueItem { object_id: a(n), type_: format!("{}::m::N", a(9)), name: name.into(), image: None, collection: "C".into() }
    }

    fn kitem(n: u64, listed: Option<u128>, locked: bool, exclusive: bool) -> RescueKioskItem {
        RescueKioskItem { item: nft(n, &format!("N{n}")), listed, locked, transferable: true, exclusive }
    }

    #[test]
    fn orders_kiosks_stakes_names_collectibles_then_coins() {
        let s = scan(|s| {
            s.coins = vec![coin(SUI_TYPE, "SUI", 3, 1_000_000_000, 0), coin(&usdc(), "USDC", 2, 5_000_000, 0)];
            s.nfts = vec![nft(1, "N")];
            s.kiosks = vec![kiosk(0x100, false, vec![]), kiosk(0x200, false, vec![])];
            s.staked = vec![RescueStake { object_id: a(2), principal: 5_000_000_000, pool_id: a(3), activation_epoch: 1 }];
            s.names = vec![RescueName { object_id: a(4), domain: "me.sui".into(), expires_ms: 0, subname: false }];
        });
        let p = plan_moves(&s, &to(), GasMode::Own);
        assert_eq!(p.batches.iter().map(|b| b.title.as_str()).collect::<Vec<_>>(), ["Kiosks", "Staked SUI", "SuiNS names", "Collectibles", "Coins"]);
        assert_eq!(p.batches[0].lines.len(), 2);
        assert!(p.batches.last().unwrap().lines.last().unwrap().detail.as_ref().unwrap().contains("All SUI that's left"));
        assert!(!p.empty);
        assert_eq!(p.fee_note, None);
        assert_eq!(p.to, to());
    }

    #[test]
    fn empty_when_nothing_can_move_and_soulbound_is_stuck() {
        let s = scan(|s| {
            s.soulbound = vec![RescueItem { object_id: a(5), type_: format!("{}::suiplay_nft::SuiPlay0x1PreorderNFT", a(0xd7b0)), name: "Eternal".into(), image: None, collection: "x".into() }]
        });
        let p = plan_moves(&s, &to(), GasMode::Own);
        assert!(p.empty);
        assert!(p.stuck[0].reason.contains("stay with the wallet"));
    }

    #[test]
    fn personal_kiosk_items_one_by_one() {
        let s = scan(|s| {
            s.kiosks = vec![kiosk(0x300, true, vec![kitem(1, None, false, false), kitem(2, None, true, false), kitem(3, Some(2_000_000_000), false, false), kitem(4, Some(1), false, true)])];
        });
        let p = plan_moves(&s, &to(), GasMode::Own);
        assert_eq!(p.batches.len(), 1);
        assert_eq!(p.batches[0].lines.iter().map(|l| l.label.as_str()).collect::<Vec<_>>(), ["N1", "N3"]);
        assert!(p.batches[0].lines[1].detail.as_ref().unwrap().contains("delisted first"));
        assert_eq!(p.stuck.iter().map(|l| l.label.as_str()).collect::<Vec<_>>(), ["N2", "N4"]);
    }

    #[test]
    fn splits_large_coin_sets_and_keeps_gas() {
        let s = scan(|s| s.coins = vec![coin(SUI_TYPE, "SUI", 900, 10_000_000, 0), coin(&usdc(), "USDC", 1500, 1, 0)]);
        let p = plan_moves(&s, &to(), GasMode::Own);
        assert!(p.batches.len() >= 3);
        assert!(p.batches[0].title.starts_with("Coins, part 1 of"));
        assert!(p.batches.last().unwrap().lines.last().unwrap().detail.as_ref().unwrap().contains("All SUI"));
        // 8 coins of 0.01 SUI stay back to pay fees
        assert_eq!(keep_for_gas(&s.coins[0].object_balances, 900), 8);
    }

    #[test]
    fn sponsored_sends_sui_like_any_coin() {
        let s = scan(|s| s.coins = vec![coin(SUI_TYPE, "SUI", 2, 1_000, 0)]);
        let p = plan_moves(&s, &to(), GasMode::Sponsored);
        assert_eq!(p.batches.len(), 1);
        assert!(!p.batches[0].lines.iter().any(|l| l.detail.as_deref().is_some_and(|d| d.contains("left after fees"))));
        assert_eq!(p.fee_note, None);
    }

    #[test]
    fn warns_when_the_wallet_cannot_pay_its_fees() {
        let s = scan(|s| {
            s.coins = vec![coin(SUI_TYPE, "SUI", 1, 1_000, 0)];
            s.nfts = vec![nft(1, "N")];
        });
        let p = plan_moves(&s, &to(), GasMode::Own);
        assert!(p.fee_note.unwrap().contains("needs a little SUI"));
    }

    #[test]
    fn formats_base_units() {
        assert_eq!(format_amount(1_500_000_000, 9), "1.5");
        assert_eq!(format_amount(12_345_678_900_000, 9), "12,345.6789");
        assert_eq!(format_amount(1, 9), "0.0001");
        assert_eq!(format_amount(0, 9), "0");
    }
}
