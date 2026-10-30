//! End to end through the real signing pipeline against Sui mainnet, simulate only, never
//! submitted (port of src/wallet/e2e.live.test.ts):
//!
//!   LIVE=1 cargo test -p rare-wallet --test e2e_live -- --ignored --nocapture
//!
//! Throwaway accounts in a fresh keystore queue a no-op transaction as a dry run, approve it the
//! way the review sheet does, and a mainnet fullnode simulates it over gRPC. Gas comes from a
//! funded address as sponsor in the simulation only (a simulation checks no signatures), so
//! nothing can move.
#![cfg(feature = "chain")]

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rare_wallet::keys::{tx_digest, Keypair};
use rare_wallet::keystore::{Keystore, MemoryStore, PasskeyAccount, PinOptions};
use rare_wallet::sign::{NoDevices, SignError, SignOptions, Signing, SuiChain, TxInput};
use rare_wallet::sponsor::{Sponsor, SponsorApi};
use rare_wallet::vault::{Kdf, Vault};
use serde_json::Value;
use std::sync::Arc;
use sui_crypto::Verifier as _;

const SPONSOR: &str = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";

struct NoSponsor;
impl SponsorApi for NoSponsor {
    fn get(&self, _: &str) -> Result<Value, String> {
        Err("off".into())
    }
    fn post(&self, _: &str, _: &Value) -> Result<(u16, Value), String> {
        Err("off".into())
    }
    fn now_ms(&self) -> u64 {
        0
    }
}

fn noop(sender: &str) -> TxInput {
    let mut tx = rare_sui::ptb::Tx::new();
    tx.set_sender(sender);
    tx.move_call("0x1::option::none", &["u64"], &[]);
    TxInput::PtbGas(tx, rare_sui::tx::BuildOpts { sponsor: Some(SPONSOR.into()), gas_budget: Some(10_000_000), ..Default::default() })
}

fn verify(bytes_b64: &str, sig: &str, address: &str) {
    let bytes = B64.decode(bytes_b64).unwrap();
    let user = sui_sdk_types::UserSignature::from_base64(sig).unwrap();
    sui_crypto::UserSignatureVerifier::new().verify(&tx_digest(&bytes), &user).unwrap();
    let who = match &user {
        sui_sdk_types::UserSignature::Simple(sui_sdk_types::SimpleSignature::Ed25519 { public_key, .. }) => public_key.derive_address().to_string(),
        sui_sdk_types::UserSignature::Multisig(m) => m.committee().derive_address().to_string(),
        _ => panic!("unexpected scheme"),
    };
    assert_eq!(who, address);
}

#[test]
#[ignore = "talks to Sui mainnet; LIVE=1 and --ignored"]
fn mainnet_create_import_sign_simulate_never_submitted() {
    if std::env::var("LIVE").is_err() {
        return;
    }
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    // the gRPC channel is made inside the runtime
    let sui = {
        let _in = rt.enter();
        rare_sui::grpc::Sui::current(&rare_sui::network::NetworkConfig::default()).unwrap()
    };
    let dir = tempfile::tempdir().unwrap();
    let ks = Arc::new(Keystore::new(MemoryStore::default(), Arc::new(Vault::new(dir.path().to_path_buf(), false, Kdf { m: 1024, t: 1, p: 1 }))));
    let s = Signing::new(ks.clone(), SuiChain { sui, rt: rt.handle().clone() }, Arc::new(Sponsor::new(NoSponsor)));
    let dry = |account: &str| SignOptions { account: Some(account.into()), dry_run: true, title: Some("Test".into()), ..Default::default() };

    // phrase account
    let acc = ks.add_phrase(None, None, Some(PinOptions { secret: "482916", kind: "pin" }), None).unwrap();
    let t = s.sign_and_execute(noop(&acc.address), dry(&acc.address)).unwrap();
    let r = s.approve(t.id, None, &mut NoDevices).unwrap();
    assert_eq!(t.wait().unwrap(), r);
    assert!(r.dry_run);
    assert_eq!(r.status.as_deref(), Some("success"));
    verify(&r.bytes, &r.signature, &acc.address);
    println!("phrase account {}: simulated {} ({:?}), signature verified", acc.address, r.digest.unwrap(), r.status);

    // imported private key
    let kp = Keypair::from_secret(&format!("0x{}", "11".repeat(32))).unwrap();
    let imp = ks.add_private_key(&kp.to_suiprivkey(), Some("Imported"), None).unwrap();
    let t = s.sign_and_execute(noop(&imp.address), dry(&imp.address)).unwrap();
    let r = s.approve(t.id, None, &mut NoDevices).unwrap();
    assert_eq!(r.status.as_deref(), Some("success"));
    verify(&r.bytes, &r.signature, &imp.address);

    // passkey account: the device member signs, the multisig verifies for the address
    let passkey = Keypair::from_bytes(rare_wallet::keys::FLAG_SECP256R1, &[7u8; 32]).unwrap();
    let device = Keypair::from_bytes(rare_wallet::keys::FLAG_ED25519, &[9u8; 32]).unwrap();
    let secret = device.to_suiprivkey();
    let pk = ks
        .add_passkey(PasskeyAccount { credential_id: "BwcH", public_key: &passkey.public_key_b64(), device_key: &device.public_key_b64(), device_secret: Some(&secret), label: None, authenticator: None, pin: None })
        .unwrap();
    let t = s.sign_and_execute(noop(&pk.address), dry(&pk.address)).unwrap();
    let r = s.approve(t.id, None, &mut NoDevices).unwrap();
    assert_eq!(r.status.as_deref(), Some("success"));
    verify(&r.bytes, &r.signature, &pk.address);
    println!("passkey multisig {}: simulated {}, multisig signature verified", pk.address, r.digest.unwrap());

    // a declined request rejects and leaves nothing queued
    let t = s.sign_and_execute(noop(&acc.address), dry(&acc.address)).unwrap();
    s.reject(t.id, None);
    assert!(matches!(t.wait(), Err(SignError::Rejected(_))));
    assert!(s.requests().is_empty());
}
