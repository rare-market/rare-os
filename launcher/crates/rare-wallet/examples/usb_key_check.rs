//! A PIN account with a USB security key as its backup, through the real CTAP2-over-HID path
//! (`passkey::PasskeyState`, Via::Usb), against whatever FIDO key is plugged in: a real one, or the
//! software key over uhid (tools/wallet/vfido.py, as root) on the build machine. Offline: signatures are checked
//! locally, nothing is sent to a network. Throwaway keys only; prints addresses, never secrets.
//!
//!   cargo run -p rare-wallet --example usb_key_check

use rare_wallet::keys::tx_digest;
use rare_wallet::keystore::{Keystore, MemoryStore};
use rare_wallet::passkey::PasskeyState;
use rare_wallet::passkey_account::{self, Ctap, Via};
use rare_wallet::pin_device::PinDevice;
use rare_wallet::pin_guard::PinGuard;
use rare_wallet::vault::{Kdf, Vault};
use std::sync::{Arc, OnceLock};
use sui_crypto::Verifier as _;

/// The security key's own PIN (vfido.py's), typed when the key asks, like the PIN pad sheet does.
const KEY_PIN: &str = "1234";
static STATE: OnceLock<PasskeyState> = OnceLock::new();

fn verify(sig: &str, digest: &[u8; 32]) -> String {
    let user = sui_sdk_types::UserSignature::from_base64(sig).unwrap();
    sui_crypto::UserSignatureVerifier::new().verify(digest, &user).expect("signature verifies");
    match user {
        sui_sdk_types::UserSignature::Multisig(m) => m.committee().derive_address().to_string(),
        _ => panic!("expected a multisig"),
    }
}

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let pd = PinDevice::new(dir.path().join("pin"), PinGuard::local(dir.path().join("pin"), false));
    let ks = Keystore::new(MemoryStore::default(), Arc::new(Vault::new(dir.path().to_path_buf(), false, Kdf { m: 1024, t: 1, p: 1 }))).with_pin(pd);
    let state = STATE.get_or_init(PasskeyState::default);
    let sink: rare_wallet::Sink = Arc::new(|e: rare_wallet::WalletEvent| {
        if let rare_wallet::WalletEvent::Passkey(p) = e {
            if matches!(p, rare_wallet::passkey::Event::Pin { .. }) {
                println!("the key asks for its PIN");
                STATE.get().unwrap().pin(Some(KEY_PIN.into()));
            } else {
                println!("passkey event: {}", serde_json::to_string(&p).unwrap_or_default());
            }
        }
    });
    let mut ctap = Ctap { state, sink, rt: rt.handle().clone() };

    let (acc, _) = passkey_account::create_pin_account(&ks, &mut ctap, &[Via::Usb], Some("Key test"), "482916", &mut |_| {}).expect("create with the security key");
    println!("PIN account {} (security key backup)", acc.address);
    let tx = (0..120u32).map(|i| (i * 7) as u8).collect::<Vec<_>>();

    ks.arm_pin(&acc.address, "482916").unwrap();
    let sig = ks.get_signer(&acc.address).unwrap().sign_transaction(&tx).unwrap();
    assert_eq!(verify(&sig, &tx_digest(&tx)), acc.address);
    println!("PIN signed");

    let sig = passkey_account::sign_with_passkey(&acc, &mut ctap, Via::Usb, true, &tx).expect("key signs");
    assert_eq!(verify(&sig, &tx_digest(&tx)), acc.address);
    println!("security key signed");

    for i in 1..=5 {
        println!("wrong PIN {i}: {}", ks.arm_pin(&acc.address, "000000").unwrap_err());
    }
    assert_eq!(ks.arm_pin(&acc.address, "482916").unwrap_err().code(), "pin-locked");
    let token = passkey_account::unlock_with_phone(&ks, &acc, &mut ctap, Via::Usb).expect("the key unlocks");
    passkey_account::reset_pin(&ks, &acc.address, &token, "731905").unwrap();
    let sig = ks.get_signer(&acc.address).unwrap().sign_transaction(&tx).unwrap();
    assert_eq!(verify(&sig, &tx_digest(&tx)), acc.address);
    println!("unlocked with the security key, new PIN signed");
    ks.wipe().unwrap();
    println!("OK: security key path passed");
}
