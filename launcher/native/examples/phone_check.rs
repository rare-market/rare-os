//! Phone control check on a test box (the build machine), never on the owner's device and never shipped:
//!
//!   phone_check import <file>      a recovery-phrase account from <file> into this HOME's device vault, made main
//!   phone_check remove <addr>      signs it out of rare.market and removes it (the vault goes with the last one)
//!
//! Run with HOME set to a throwaway folder. Prints addresses only: never a phrase or a key.

use rare_friends::storage::FileStorage;
use rare_wallet::keystore::{DeviceVault, FileStore, Keystore};
use rare_wallet::vault::{default_dir, Kdf, Vault};
use std::sync::Arc;

fn keystore() -> Keystore<FileStore> {
    let dir = default_dir();
    Keystore::new(FileStore::new(dir.clone()), Arc::new(Vault::new(dir.clone(), true, Kdf::DEFAULT))).with_device(DeviceVault::at(&dir, true))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ks = keystore();
    match (args.first().map(String::as_str), args.get(1)) {
        (Some("import"), Some(file)) => {
            let words = std::fs::read_to_string(file).expect("phrase file");
            let a = ks.add_phrase_device(Some(words.trim()), Some("Phone check"), None).expect("import");
            ks.select(&a.address).expect("select");
            println!("imported {}", a.address);
        }
        (Some("remove"), Some(addr)) => {
            rare_friends::sign::forget_key(&FileStorage::new(rare_core::paths::data_dir().join("friends")), addr);
            ks.open_device().ok();
            ks.remove(addr).expect("remove");
            println!("removed; accounts now: {}", ks.list_accounts().map(|l| l.len()).unwrap_or(0));
        }
        _ => eprintln!("usage: phone_check import <file> | remove <addr>"),
    }
}
