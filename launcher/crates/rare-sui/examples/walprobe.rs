//! Live Walrus write check without spending: builds (simulates on the node) the register
//! transaction a backup would send, for an address that holds WAL (whoever last touched the
//! Walrus system object), and asks the relay for its tip config. Nothing is signed or uploaded.
//! `cargo run -p rare-sui --example walprobe [testnet]`

use rare_sui::grpc::{ChainRead, Include};
use rare_sui::saves::{store, walrus};
use rare_sui::tx::{build, BuildOpts};
use rare_sui::{NetworkConfig, Sui};

#[tokio::main(flavor = "current_thread")]
async fn main() -> rare_sui::Result<()> {
    let cfg = if std::env::args().nth(1).as_deref() == Some("testnet") { NetworkConfig::testnet() } else { NetworkConfig::mainnet() };
    let sui = Sui::current(&cfg)?;
    let n = sui.network();
    let http = reqwest::Client::new();
    let sys = store::system_info(&sui, n).await?;
    println!("walrus {:?}", sys);
    let wal = sui.coin_metadata(store::wal_type(n)).await?;
    println!("WAL meta {:?}", wal.map(|m| m.symbol));
    let tip = store::tip_config(&http, n).await?;
    println!("relay tip {:?}", tip);

    let so = sui.object(walrus::system_object(n), Include { previous_transaction: true, ..Include::NONE }).await?;
    let t = sui.transaction(&so.previous_transaction.unwrap_or_default(), &["transaction.sender"]).await?;
    let who = t.transaction.and_then(|t| t.sender).unwrap_or_default();
    println!("simulating for {who}: WAL {} FROST", sui.balance(&who, store::wal_type(n)).await?.balance);

    let blob: Vec<u8> = (0..48_000u32).map(|i| (i * 7) as u8).collect();
    let meta = store::compute_metadata(&blob, sys.n_shards)?;
    let est = store::estimate(&sui, &http, n, blob.len() as u64, 30).await?;
    println!("blob {} encoded {} bytes, estimate {:?}", meta.blob_id_text(), meta.encoded_size, est);
    let tipped = tip.as_ref().map(|config| store::Tip { config, nonce: [1; 32], blob_digest: [2; 32] });
    let attrs = vec![("app".to_string(), "rare-saves".to_string()), ("v".into(), "1".into())];
    let tx = store::register_tx(n, &who, &sys, &meta, est.epochs, true, &attrs, tipped)?;
    match build(&sui, &tx, &BuildOpts::default()).await {
        Ok(b) => println!("register builds: {} ({} bytes), budget {}", b.digest, b.bytes.len(), b.data.gas_payment.budget),
        Err(e) => println!("register build: {e}"),
    }
    Ok(())
}
