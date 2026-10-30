//! Address balances probe (no keys, nothing signed or sent): finds accounts that hold funds only
//! in their address balance and dry-runs the launcher's builders for them.
//!
//! `cargo run -p rare-sui --example abprobe [testnet] scan [checkpoints]` lists recent deposit
//! recipients with their SUI and USDC split (coins vs address balance).
//! `cargo run -p rare-sui --example abprobe [testnet] try <address> [usdc type]` builds (simulates) a
//! SUI send, a max SUI send, a USDC send and a gas-coin payment for that address.

use rare_sui::grpc::ChainRead;
use rare_sui::ptb::{Arg, Tx};
use rare_sui::tx::{build, build_coin_send, BuildOpts, CoinSend};
use rare_sui::{NetworkConfig, Sui};
use sui_rpc::field::{FieldMask, FieldMaskUtil};
use sui_rpc::proto::sui::rpc::v2 as pb;

const USDC_MAIN: &str = "0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC";
const USDC_TEST: &str = "0xa1ec7fc00a6f40db9693ad1415d0c193ad3906494428cf252621037bd7117e29::usdc::USDC";

#[tokio::main(flavor = "current_thread")]
async fn main() -> rare_sui::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let testnet = args.first().map(String::as_str) == Some("testnet");
    if testnet {
        args.remove(0);
    }
    let cfg = if testnet { NetworkConfig::testnet() } else { NetworkConfig::mainnet() };
    let sui = Sui::current(&cfg)?;
    let usdc = if testnet { USDC_TEST } else { USDC_MAIN };
    match args.first().map(String::as_str) {
        Some("scan") => scan(&sui, usdc, args.get(1).and_then(|s| s.parse().ok()).unwrap_or(200)).await,
        Some("try") => try_builds(&sui, args.get(1).expect("address"), args.get(2).map(String::as_str).unwrap_or(usdc)).await,
        _ => {
            eprintln!("usage: abprobe [testnet] scan [n] | try <address> [coin type]");
            Ok(())
        }
    }
}

async fn split(sui: &Sui, owner: &str, t: &str) -> rare_sui::Result<(u64, u64, u64)> {
    let b = sui.balance(owner, t).await?;
    Ok((b.balance, b.coin_balance.unwrap_or(0), b.address_balance.unwrap_or(0)))
}

async fn scan(sui: &Sui, usdc: &str, n: u64) -> rare_sui::Result<()> {
    let mut ledger = sui.rpc();
    let latest = ledger
        .ledger_client()
        .get_checkpoint(pb::GetCheckpointRequest::default().with_read_mask(FieldMask::from_paths(["sequence_number"])))
        .await
        .map_err(|e| rare_sui::Error::invalid(e.to_string()))?
        .into_inner()
        .checkpoint
        .and_then(|c| c.sequence_number)
        .unwrap_or(0);
    let mut seen = std::collections::BTreeSet::new();
    for seq in (latest.saturating_sub(n)..=latest).rev() {
        let req = pb::GetCheckpointRequest::default().with_sequence_number(seq).with_read_mask(FieldMask::from_paths(["transactions.effects.changed_objects"]));
        let Ok(r) = ledger.ledger_client().get_checkpoint(req).await else { continue };
        for t in r.into_inner().checkpoint.map(|c| c.transactions).unwrap_or_default() {
            for o in t.effects.map(|e| e.changed_objects).unwrap_or_default() {
                if let Some(w) = o.accumulator_write {
                    let ty = w.accumulator_type.unwrap_or_default();
                    if ty.contains("balance::Balance<") {
                        if let Some(a) = w.address {
                            seen.insert(a);
                        }
                    }
                }
            }
        }
    }
    println!("{} addresses with balance accumulator writes in the last {n} checkpoints", seen.len());
    for a in seen {
        let (s, sc, sa) = split(sui, &a, "0x2::sui::SUI").await.unwrap_or_default();
        let (u, uc, ua) = split(sui, &a, usdc).await.unwrap_or_default();
        if sa > 0 || ua > 0 {
            let tag = if sa > 0 && sc == 0 && ua > 0 && uc == 0 { "  <-- SUI+USDC only in address balance" } else if sa > 0 && sc == 0 { "  <-- SUI only in address balance" } else { "" };
            println!("{a} SUI {s} (coins {sc}, ab {sa}) USDC {u} (coins {uc}, ab {ua}){tag}");
        }
    }
    Ok(())
}

async fn show(label: &str, sui: &Sui, tx: &Tx, opts: &BuildOpts) {
    match build(sui, tx, opts).await {
        Ok(b) => {
            let g = &b.data.gas_payment;
            let res = g.objects.iter().filter(|o| o.digest().inner()[12..32] == [0xac; 20]).count();
            println!("{label}: OK budget {} price {} payment {} object(s) ({res} reservation) expiration {:?}", g.budget, g.price, g.objects.len(), b.data.expiration);
            match sui.simulate(&b.data).await {
                Ok(s) => {
                    let st = s.transaction.as_ref().and_then(|t| t.effects.as_ref()).and_then(|e| e.status.as_ref());
                    println!("   dry run: success {:?} {:?}", st.and_then(|s| s.success), st.and_then(|s| s.error.as_ref()).map(|e| e.description.clone()));
                    for c in s.transaction.as_ref().map(|t| t.balance_changes.clone()).unwrap_or_default() {
                        println!("   change {} {} {}", c.address.unwrap_or_default(), c.coin_type.unwrap_or_default(), c.amount.unwrap_or_default());
                    }
                }
                Err(e) => println!("   dry run error: {e}"),
            }
        }
        Err(e) => println!("{label}: FAILED {e}"),
    }
}

async fn try_builds(sui: &Sui, me: &str, usdc: &str) -> rare_sui::Result<()> {
    let to = "0x00000000000000000000000000000000000000000000000000000000000000aa";
    println!("SUI {:?}", split(sui, me, "0x2::sui::SUI").await?);
    println!("{usdc} {:?}", split(sui, me, usdc).await?);
    // what the wallet's coin list shows (list_balances, the totals)
    for b in rare_sui::balances::balances(sui, me).await?.iter().take(6) {
        println!("wallet row: {} {} ({} raw)", b.amount, b.symbol, b.raw);
    }

    // what the launcher's send sheet builds
    for (label, coin, amount, max) in [("send 0.001 SUI", "0x2::sui::SUI", 1_000_000u64, false), ("send max SUI", "0x2::sui::SUI", 0, true), ("send 0.01 USDC", usdc, 10_000, false), ("send max USDC", usdc, 0, true)] {
        match build_coin_send(sui, &CoinSend { sender: me.into(), to: to.into(), coin_type: coin.into(), amount, max, balance: None }).await {
            Ok((tx, opts)) => show(label, sui, &tx, &opts).await,
            Err(e) => println!("{label}: refused: {e}"),
        }
    }

    // max SUI with a fixed budget (the node's estimate for sending a reservation-backed gas coin)
    for budget in [2_000_000u64, 5_000_000] {
        let mut tx = Tx::new();
        tx.set_sender(me);
        tx.transfer_to(&[Arg::Gas], to);
        show(&format!("transfer the gas coin, budget {budget}"), sui, &tx, &BuildOpts { gas_budget: Some(budget), ..Default::default() }).await;
    }

    // a market-style payment split from the gas coin
    let mut tx = Tx::new();
    tx.set_sender(me);
    let pay = tx.split_u64(Arg::Gas, 1_000_000);
    tx.transfer_to(&[pay], to);
    show("split 0.001 SUI from the gas coin", sui, &tx, &BuildOpts::default()).await;

    // SUI and USDC paid by the sender while a sponsor pays gas (coin intents never touch the
    // sponsor's gas coin)
    if let Ok(sponsor) = std::env::var("SPONSOR") {
        let mut tx = Tx::new();
        tx.set_sender(me);
        let s = tx.coin_with_balance("0x2::sui::SUI", 1_000_000);
        let u = tx.coin_with_balance(usdc, 10_000);
        tx.transfer_to(&[s, u], to);
        show("sponsored: 0.001 SUI + 0.01 USDC from the sender", sui, &tx, &BuildOpts { sponsor: Some(sponsor), ..Default::default() }).await;
    }

    // a withdrawal redeemed by hand
    let mut tx = Tx::new();
    tx.set_sender(me);
    let c = tx.withdraw_coin(usdc, 10_000);
    tx.transfer_to(&[c], to);
    show("withdraw_coin 0.01 USDC", sui, &tx, &BuildOpts::default()).await;
    Ok(())
}
