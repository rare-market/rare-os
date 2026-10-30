//! Live read smoke check against the public fullnode (no keys, nothing signed or sent):
//! `cargo run -p rare-sui --example smoke [testnet]`.
//!
//! Reads gas, an object, coin metadata, then finds the trophies package's publisher and builds
//! (simulates) a 1 MIST self-transfer for it through `tx::build`, which exercises object
//! resolution and gas selection on the node.

use rare_sui::grpc::{ChainRead, Include};
use rare_sui::ptb::{Arg, Tx};
use rare_sui::tx::{build, BuildOpts};
use rare_sui::{network, NetworkConfig, Sui};

#[tokio::main(flavor = "current_thread")]
async fn main() -> rare_sui::Result<()> {
    let cfg = if std::env::args().nth(1).as_deref() == Some("testnet") { NetworkConfig::testnet() } else { NetworkConfig::mainnet() };
    let sui = Sui::current(&cfg)?;
    println!("network {} at {}", sui.network().as_str(), sui.url());
    println!("gas price {} MIST", sui.reference_gas_price().await?);
    println!("SUI meta {:?}", sui.coin_metadata("0x2::sui::SUI").await?);

    // dynamic field ids derived locally match the node's (Drip's listings bag on mainnet)
    if let Ok(fields) = sui.dynamic_fields(rare_sui::kiosk::DRIP_LISTINGS_BAG, 2).await {
        for f in fields {
            let mine = rare_sui::kiosk::dynamic_field_id(rare_sui::kiosk::DRIP_LISTINGS_BAG, &f.name_type, &f.name_bcs)?;
            println!("field {} derived {} {}", f.field_id, mine, if mine == f.field_id { "ok" } else { "MISMATCH" });
        }
    }

    // Walrus: the blob type and the current epoch, from the system object
    let wt = rare_sui::saves::walrus::blob_type(&sui, sui.network()).await?;
    println!("walrus blob type {wt}, epoch {}", rare_sui::saves::walrus::current_epoch(&sui, sui.network()).await?);

    let Some(t) = network::trophy_deployment(sui.network()) else {
        println!("no trophies deployment on this network");
        return Ok(());
    };
    let reg = sui.object(&t.registry, Include::JSON).await?;
    println!("registry {} type {} owner {:?}", reg.object_id, reg.object_type, reg.owner);
    let pkg = sui.object(&t.package, Include { previous_transaction: true, ..Include::NONE }).await?;
    let digest = pkg.previous_transaction.unwrap_or_default();
    let ptx = sui.transaction(&digest, &["transaction.sender"]).await?;
    let sender = ptx.transaction.and_then(|t| t.sender).unwrap_or_default();
    println!("publisher {sender}");
    match rare_sui::txdetail::activity_detail(&sui, &digest, &sender).await? {
        Some(d) => println!("publish tx {}: {:?}, fee {} MIST, {} call(s), {} object(s)", d.digest, d.status, d.fee_mist, d.calls.len(), d.objects.len()),
        None => println!("publish tx pruned"),
    }
    let bal = sui.balance(&sender, "0x2::sui::SUI").await?;
    println!("publisher SUI {} MIST, name {:?}", bal.balance, sui.default_name(&sender).await?);

    let mut tx = Tx::new();
    tx.set_sender(&sender);
    let c = tx.split_u64(Arg::Gas, 1);
    tx.transfer_to(&[c], &sender);
    match build(&sui, &tx, &BuildOpts::default()).await {
        Ok(b) => println!("built {} ({} bytes), budget {}, price {}, {} gas coin(s)", b.digest, b.bytes.len(), b.data.gas_payment.budget, b.data.gas_payment.price, b.data.gas_payment.objects.len()),
        Err(e) => println!("build: {e}"),
    }
    Ok(())
}
