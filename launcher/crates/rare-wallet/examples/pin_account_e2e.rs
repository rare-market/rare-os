//! PIN accounts end to end on Sui testnet, throwaway keys only (nothing here is anyone's wallet):
//!
//!   cargo run -p rare-wallet --example pin_account_e2e
//!
//! 1. An "old" throwaway phrase account, funded by the testnet faucet, makes a kiosk with an item
//!    in it and stakes SUI when the faucet gave enough.
//! 2. "Create account with PIN": this machine's device key behind a PIN, then a phone and a USB
//!    security key as backups (the soft WebAuthn authenticator plays both): a 1-of-3 multisig.
//! 3. Move everything from the old account to the new one in one transaction signed by the old one.
//! 4. The PIN account sends SUI back, signed on the device with the PIN (the review sheet's path:
//!    queue, arm with the PIN, approve).
//! 5. The "phone" signs a transfer directly.
//! 6. Five wrong PINs lock the device key (the guard is restarted to show the lock holds when the
//!    service runs); the phone still signs while locked; the security key unlocks it, a new PIN is
//!    set and the device key signs again.
//! The PIN guard is the root service when /run/rare-pin-guard.sock exists, else in-process.
//! Prints addresses and digests only, never a key or a phrase. Leaves nothing behind but testnet
//! coins on throwaway addresses.

use rare_wallet::keys::Keypair;
use rare_wallet::keystore::{FileStore, Keystore, RareAccount};
use rare_wallet::passkey_account::{self, soft::SoftAuthenticator, Via};
use rare_wallet::pin_device::PinDevice;
use rare_wallet::sign::{Chain, ExternalSigner, SignError, SignOptions, Signing, SuiChain, TxInput};
use rare_wallet::sponsor::{Sponsor, SponsorApi};
use rare_wallet::vault::{Kdf, Vault};
use rare_sui::grpc::ChainRead;
use rare_sui::ptb::{Arg, Tx};
use serde_json::Value;
use std::sync::{Arc, Mutex};

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

/// The phone or the security key, as the review sheet's device step would run them.
struct Soft<'a> {
    auth: &'a Mutex<SoftAuthenticator>,
    via: Via,
}
impl ExternalSigner for Soft<'_> {
    fn sign(&mut self, account: &RareAccount, _: &str, transaction: bool, bytes: &[u8]) -> Result<String, SignError> {
        passkey_account::sign_with_passkey(account, &mut *self.auth.lock().unwrap(), self.via, transaction, bytes).map_err(|e| SignError::Device { code: "passkey".into(), message: e.to_string() })
    }
}

fn faucet(net: &str, address: &str) {
    for _ in 0..if net == "devnet" { 1 } else { 2 } {
        let body = format!(r#"{{"FixedAmountRequest":{{"recipient":"{address}"}}}}"#);
        let o = std::process::Command::new("curl").args(["-s", "-X", "POST", &format!("https://faucet.{net}.sui.io/v2/gas"), "-H", "content-type: application/json", "-d", &body]).output().expect("curl");
        println!("faucet: {}", String::from_utf8_lossy(&o.stdout).chars().take(120).collect::<String>());
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
}

fn wait() {
    std::thread::sleep(std::time::Duration::from_secs(3));
}

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    // testnet by default; NET=devnet when the testnet faucet refuses scripts (it does since 2026)
    let net = std::env::var("NET").unwrap_or_else(|_| "testnet".into());
    let mut cfg = rare_sui::network::NetworkConfig::testnet();
    if net == "devnet" {
        cfg.testnet_rpc = Some("https://fullnode.devnet.sui.io:443".into());
    }
    println!("network: {net}");
    let sui = {
        let _in = rt.enter();
        rare_sui::grpc::Sui::current(&cfg).unwrap()
    };
    let chain = SuiChain { sui: sui.clone(), rt: rt.handle().clone() };
    let dir = tempfile::tempdir().unwrap();
    let vault = Arc::new(Vault::new(dir.path().to_path_buf(), false, Kdf { m: 1024, t: 1, p: 1 }));
    let pd = PinDevice::at(dir.path());
    println!("PIN guard: {}", pd.place().describe());
    let ks = Arc::new(Keystore::new(FileStore::new(dir.path().to_path_buf()), vault).with_pin(pd));
    let signing = Signing::new(ks.clone(), SuiChain { sui: sui.clone(), rt: rt.handle().clone() }, Arc::new(Sponsor::new(NoSponsor)));
    let sui_bal = |a: &str| chain.sui_balance(a).unwrap_or(0);

    // ---- 1. the old throwaway account, a kiosk with an item, maybe a stake
    let old = Keypair::from_mnemonic(&rare_wallet::keys::new_mnemonic(128), 0).unwrap();
    let old_addr = old.address();
    println!("old account {old_addr}");
    faucet(&net, &old_addr);
    for _ in 0..20 {
        if sui_bal(&old_addr) > 0 {
            break;
        }
        wait();
    }
    let funded = sui_bal(&old_addr);
    println!("old has {funded} MIST");
    assert!(funded > 0, "the faucet sent nothing (rate limit?)");
    let run_old = |tx: Tx, opts: rare_sui::tx::BuildOpts| -> String {
        let bytes = chain.build(&tx, &old_addr, &opts).unwrap();
        let sig = old.sign_transaction(&bytes);
        let x = chain.execute(&bytes, &[sig]).unwrap();
        assert!(x.success, "old account tx failed: {}", x.digest);
        x.digest
    };
    let mut tx = Tx::new();
    tx.set_sender(&old_addr);
    let k = tx.move_call("0x2::kiosk::new", &[], &[]);
    let (kiosk, cap) = (k.nth(0), k.nth(1));
    let coin = tx.split_u64(Arg::Gas, 50_000_000);
    tx.move_call("0x2::kiosk::place", &["0x2::coin::Coin<0x2::sui::SUI>"], &[kiosk, cap, coin]);
    tx.move_call("0x2::transfer::public_share_object", &["0x2::kiosk::Kiosk"], &[kiosk]);
    tx.transfer_to(&[cap], &old_addr);
    let validator = if funded > 1_300_000_000 { validator(&net) } else { None };
    println!("validator: {}", validator.as_deref().unwrap_or("none (no stake)"));
    let staked = validator.is_some();
    if let Some(validator) = validator {
        let st = tx.split_u64(Arg::Gas, 1_000_000_000);
        let system = tx.object("0x5");
        let v = tx.pure_address(&validator);
        tx.move_call("0x3::sui_system::request_add_stake", &[], &[system, st, v]);
    }
    println!("old: kiosk with an item{} {}", if staked { " and a stake" } else { "" }, run_old(tx, Default::default()));
    wait();

    // ---- 2. Create account with PIN, phone and security key as backups
    let auth = Mutex::new(SoftAuthenticator::new());
    let (acc, _kp) = passkey_account::create_pin_account(&ks, &mut *auth.lock().unwrap(), &[Via::Phone, Via::Usb], Some("Test"), "482916", &mut |i| println!("backup {} of 2", i + 1)).unwrap();
    let new_addr = acc.address.clone();
    println!("PIN account {new_addr} (members {})", acc.passkey.as_ref().unwrap().committee().unwrap().members().len());

    // ---- 3. move everything from the old account, one transaction signed by the old one
    let http = rare_sui::prices::ReqwestGet::default();
    let nfts = |refs: Vec<rare_wallet::nft::NftRef>| -> rare_sui::BoxFut<'static, std::collections::HashMap<String, rare_wallet::nft::Sendability>> {
        let (sui, owner) = (sui.clone(), old_addr.clone());
        Box::pin(async move { rare_wallet::nft::nft_sendability(&sui, &rare_sui::prices::ReqwestGet::default(), &owner, &refs).await })
    };
    let _ = &http;
    let plan = rt.block_on(rare_wallet::move_all::plan_move_all(&sui, &nfts, &old_addr, &new_addr)).unwrap();
    println!("move plan: {} tx, {} kiosks ({} items), {} staked, {} MIST SUI, stuck {}", plan.txs.len(), plan.kiosks, plan.kiosk_items, plan.staked, plan.sui, plan.stuck.len());
    assert_eq!(plan.txs.len(), 1);
    assert_eq!(plan.kiosks, 1);
    for (tx, opts) in plan.txs.clone() {
        println!("moved: {}", run_old(tx, opts));
    }
    wait();
    let owned = rt.block_on(sui.owned_objects(&new_addr, None, 50, rare_sui::grpc::Include::default())).unwrap();
    let caps = owned.iter().filter(|o| o.object_type.ends_with("::kiosk::KioskOwnerCap")).count();
    let stakes = owned.iter().filter(|o| o.object_type.ends_with("::staking_pool::StakedSui")).count();
    println!("new owns: {} SUI MIST, {caps} kiosk cap, {stakes} stake", sui_bal(&new_addr));
    assert_eq!(caps, 1);
    assert_eq!(stakes, staked as usize);
    assert!(sui_bal(&new_addr) > 0);
    assert_eq!(sui_bal(&old_addr), 0, "the old account keeps nothing");

    let send_back = |amount: u64| -> TxInput {
        let mut tx = Tx::new();
        tx.set_sender(&new_addr);
        let c = tx.split_u64(Arg::Gas, amount);
        tx.transfer_to(&[c], &old_addr);
        TxInput::Ptb(tx)
    };
    let opts = || SignOptions { account: Some(new_addr.clone()), title: Some("Test send".into()), ..Default::default() };
    let approve = |pin: Option<&str>, via: Via| -> Result<String, SignError> {
        let t = signing.sign_and_execute(send_back(1_000_000), opts())?;
        if let Some(p) = pin {
            ks.arm_pin(&new_addr, p)?;
        }
        let r = signing.approve(t.id, None, &mut Soft { auth: &auth, via });
        if r.is_err() {
            signing.reject(t.id, None);
        }
        let r = r?;
        assert_eq!(r.status.as_deref(), Some("success"));
        wait();
        Ok(r.digest.unwrap_or_default())
    };

    // ---- 4. signed on the device with the PIN
    println!("PIN signed: {}", approve(Some("482916"), Via::Phone).unwrap());
    // ---- 5. the phone signs directly
    println!("phone signed: {}", approve(None, Via::Phone).unwrap());

    // ---- 6. five wrong PINs, the lock, the phone while locked, the security key unlocks
    for i in 1..=5 {
        let e = ks.arm_pin(&new_addr, "000000").unwrap_err();
        println!("wrong PIN {i}: {e}");
    }
    assert_eq!(ks.arm_pin(&new_addr, "482916").unwrap_err().code(), "pin-locked");
    if std::path::Path::new(rare_wallet::pin_guard::SOCKET).exists() && std::env::var("RESTART_GUARD").is_ok() {
        let ok = std::process::Command::new("sudo").args(["systemctl", "restart", "rare-pin-guard-test"]).status().is_ok_and(|s| s.success());
        wait();
        println!("guard restarted ({ok}): {}", ks.arm_pin(&new_addr, "482916").unwrap_err());
    }
    println!("phone signed while locked: {}", approve(None, Via::Phone).unwrap());
    let token = passkey_account::unlock_with_phone(&ks, &acc, &mut *auth.lock().unwrap(), Via::Usb).unwrap();
    passkey_account::reset_pin(&ks, &new_addr, &token, "731905").unwrap();
    ks.disarm();
    println!("new PIN signed: {}", approve(Some("731905"), Via::Phone).unwrap());
    println!("security key signed: {}", approve(None, Via::Usb).unwrap());

    // ---- 7. Change PIN, the recovery words, and PIN signing back on "another SuiPlay" from them
    ks.change_pin("731905", "246813").unwrap();
    let words = ks.pin_words("246813").unwrap();
    println!("PIN changed, recovery words read ({} words)", words.split_whitespace().count());
    struct Members(String, Vec<String>);
    impl passkey_account::History for Members {
        fn has_history(&self, _: &str) -> bool {
            false
        }
        fn registered_members(&self, pk: &[u8], _: &[u8]) -> Option<(String, Vec<String>)> {
            use base64::Engine;
            let b = base64::engine::general_purpose::STANDARD.encode(pk);
            self.1.contains(&b).then(|| (self.0.clone(), self.1.clone()))
        }
    }
    let members: Vec<String> = acc.passkey.as_ref().unwrap().passkeys().into_iter().map(|m| m.public_key).collect();
    let dir2 = tempfile::tempdir().unwrap();
    let ks2 = Arc::new(Keystore::new(FileStore::new(dir2.path().to_path_buf()), Arc::new(Vault::new(dir2.path().to_path_buf(), false, Kdf { m: 1024, t: 1, p: 1 }))).with_pin(PinDevice::new(dir2.path().join("pin"), rare_wallet::pin_guard::PinGuard::local(dir2.path().join("pin"), false))));
    let restored = passkey_account::restore_pin_account(&ks2, &mut *auth.lock().unwrap(), &Members(new_addr.clone(), members), Via::Usb, &words, "135792").unwrap();
    assert_eq!(restored.address, new_addr, "same address");
    let signing2 = Signing::new(ks2.clone(), SuiChain { sui: sui.clone(), rt: rt.handle().clone() }, Arc::new(Sponsor::new(NoSponsor)));
    let t = signing2.sign_and_execute(send_back(1_000_000), opts()).unwrap();
    ks2.arm_pin(&new_addr, "135792").unwrap();
    let r = signing2.approve(t.id, None, &mut Soft { auth: &auth, via: Via::Phone }).unwrap();
    assert_eq!(r.status.as_deref(), Some("success"));
    println!("restored on a second keystore from the words, PIN signed: {}", r.digest.unwrap_or_default());
    ks2.wipe().unwrap();

    // clean up: the guard record and the device key
    ks.wipe().unwrap();
    println!("OK: all steps passed");
}

/// A testnet validator's address (GraphQL), or None.
fn validator(net: &str) -> Option<String> {
    let q = r#"{"query":"{ epoch { validatorSet { activeValidators(first: 1) { nodes { contents { json } } } } } }"}"#;
    let o = std::process::Command::new("curl").args(["-s", "-X", "POST", &format!("https://graphql.{net}.sui.io/graphql"), "-H", "content-type: application/json", "-d", q]).output().ok()?;
    let v: Value = serde_json::from_slice(&o.stdout).ok()?;
    let j = &v["data"]["epoch"]["validatorSet"]["activeValidators"]["nodes"][0]["contents"]["json"];
    j["metadata"]["sui_address"].as_str().or_else(|| j["sui_address"].as_str()).map(str::to_string)
}
