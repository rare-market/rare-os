//! Device check for phrase accounts, run on the SuiPlay under the device lock (never shipped):
//!
//!   wallet_device_check create          a new recovery-phrase account in the device vault, made main
//!   wallet_device_check status <addr>   opens the device vault as a fresh process does, signs a test
//!                                       message, shows the rare.market sign-in kept for the account and
//!                                       asks rare.market whether it is live
//!   wallet_device_check renew <addr>    renews that sign-in with the device key (no wallet)
//!   wallet_device_check remove <addr>   signs it out of rare.market and removes the account (the device
//!                                       vault and its sealed key go with the last one)
//!
//! Prints addresses, the seal and HTTP statuses only: never a phrase or a key.

use rare_friends::storage::FileStorage;
use rare_wallet::keystore::{DeviceVault, FileStore, Keystore};
use rare_wallet::vault::{default_dir, Kdf, Vault};
use std::sync::Arc;

const ORIGIN: &str = "https://rare.market";

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn keystore() -> Keystore<FileStore> {
    let dir = default_dir();
    Keystore::new(FileStore::new(dir.clone()), Arc::new(Vault::new(dir.clone(), true, Kdf::DEFAULT))).with_device(DeviceVault::at(&dir, true))
}

fn friends_store() -> FileStorage {
    FileStorage::new(rare_core::paths::data_dir().join("friends"))
}

/// One request signed by the account's device key: (status, body).
fn signed(method: &str, path: &str, address: &str) -> Result<(u16, String), String> {
    let k = rare_friends::sign::stored_key(&friends_store(), address).ok_or("no device key for this account")?;
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();
    let url = format!("{ORIGIN}{path}");
    let headers = k.headers(method, path, "", now_ms());
    let mut r = match method {
        "GET" => {
            let mut q = agent.get(&url);
            for (n, v) in &headers {
                q = q.header(n, v);
            }
            q.call()
        }
        "DELETE" => {
            let mut q = agent.delete(&url);
            for (n, v) in &headers {
                q = q.header(n, v);
            }
            q.call()
        }
        _ => {
            let mut q = agent.post(&url);
            for (n, v) in &headers {
                q = q.header(n, v);
            }
            q.send_empty()
        }
    }
    .map_err(|e| e.to_string())?;
    let status = r.status().as_u16();
    let body = r.body_mut().read_to_string().unwrap_or_default();
    Ok((status, body))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ks = keystore();
    match args.first().map(String::as_str) {
        Some("create") => {
            let a = ks.add_phrase_device(None, Some("Device check"), None).expect("create");
            ks.select(&a.address).expect("select");
            println!("created {} seal={}", a.address, ks.device_seal().map(|s| s.as_str()).unwrap_or("none"));
        }
        Some("status") => {
            let addr = args.get(1).expect("address");
            let t = std::time::Instant::now();
            let open = ks.open_device();
            println!("device vault open={open:?} in {} ms seal={}", t.elapsed().as_millis(), ks.device_seal().map(|s| s.as_str()).unwrap_or("none"));
            println!("needs unlock: {:?}", ks.needs_unlock(addr));
            let signer = ks.get_signer(addr).expect("signer");
            let (_, sig) = signer.sign_personal_message(b"device check").expect("sign");
            println!("signed a test message: {} bytes of signature", sig.len());
            let now = now_ms();
            match rare_friends::sign::session_of(&friends_store(), addr) {
                Some(s) => println!("rare.market sign-in: since {} ({} h ago), until {} ({} days left)", s.linked_at, (now - s.linked_at) / 3_600_000, s.linked_until, (s.linked_until - now) / 86_400_000),
                None => println!("rare.market sign-in: none"),
            }
            match signed("GET", "/v1/friends", addr) {
                Ok((st, body)) => println!("GET /v1/friends -> {st} {}", body.chars().take(90).collect::<String>()),
                Err(e) => println!("GET /v1/friends failed: {e}"),
            }
            let m = ks.meta().expect("meta");
            println!("main account: {}", m.selected.as_deref() == Some(addr.as_str()));
        }
        Some("renew") => {
            let addr = args.get(1).expect("address");
            match signed("POST", "/v1/friends/link/renew", addr) {
                Ok((st, body)) => println!("POST /v1/friends/link/renew -> {st} {}", body.chars().take(160).collect::<String>()),
                Err(e) => println!("renew failed: {e}"),
            }
        }
        Some("remove") => {
            let addr = args.get(1).expect("address");
            match signed("DELETE", "/v1/friends/link", addr) {
                Ok((st, _)) => println!("DELETE /v1/friends/link -> {st}"),
                Err(e) => println!("sign out: {e}"),
            }
            rare_friends::sign::forget_key(&friends_store(), addr);
            ks.open_device().ok();
            ks.remove(addr).expect("remove");
            println!("removed; device vault left: {}", default_dir().join("device/vault.json").exists());
            println!("accounts now: {}", ks.list_accounts().map(|l| l.len()).unwrap_or(0));
        }
        _ => eprintln!("usage: wallet_device_check create | status <addr> | renew <addr> | remove <addr>"),
    }
}
