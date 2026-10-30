//! Port of src/wallet/core/keys/keystore.test.ts and secret.test.ts (keystore half), plus the
//! byte-level cross-checks against testdata/ts-vectors.json (written by the TS wallet itself).

use super::*;
use crate::keys::{hex, personal_message_digest, tx_digest};
use crate::secret::{derive_info, derive_message, ed25519_signature_bytes, finish, prf_input, DERIVE_SALT};
use crate::vault::Kdf;
use serde_json::Value;
use sui_crypto::Verifier as _;
use sui_sdk_types::UserSignature;

const PHRASE: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const PIN: fn() -> Option<PinOptions<'static>> = || Some(PinOptions { secret: "123456", kind: "pin" });
const FAST: Kdf = Kdf { m: 256, t: 1, p: 1 };

struct T {
    _dir: tempfile::TempDir,
    ks: Keystore<FileStore>,
}

fn mk() -> T {
    let dir = tempfile::tempdir().unwrap();
    let vault = Arc::new(Vault::new(dir.path().join("w"), false, FAST));
    let ks = Keystore::new(FileStore::new(dir.path().join("w")), vault);
    T { _dir: dir, ks }
}

fn vectors() -> Value {
    serde_json::from_str(include_str!("../../testdata/ts-vectors.json")).unwrap()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

fn verify_tx(sig_b64: &str, tx: &[u8]) -> Result<(), sui_crypto::SignatureError> {
    let sig = UserSignature::from_base64(sig_b64).unwrap();
    sui_crypto::UserSignatureVerifier::new().verify(&tx_digest(tx), &sig)
}

// ---------------------------------------------------------------- keystore.test.ts

#[test]
fn first_phrase_needs_a_pin_creates_the_vault_and_signs() {
    let t = mk();
    assert_eq!(t.ks.add_phrase(Some(PHRASE), None, None, None).unwrap_err(), KeystoreError::NeedsPassword);
    let short = t.ks.add_phrase(Some(PHRASE), None, Some(PinOptions { secret: "12", kind: "pin" }), None).unwrap_err();
    assert!(short.to_string().contains("6 digits"));
    let acc = t.ks.add_phrase(Some(PHRASE), None, PIN(), None).unwrap();
    assert_eq!(acc.kind, AccountKind::Native);
    assert_eq!(acc.hd.as_ref().unwrap().index, 0);
    assert_eq!(acc.address, Keypair::from_mnemonic(PHRASE, 0).unwrap().address());
    assert!(t.ks.is_unlocked().unwrap());
    assert_eq!(t.ks.get_signer(&acc.address).unwrap().address(), acc.address);
}

#[test]
fn passphrase_rules() {
    let t = mk();
    let e = t.ks.add_phrase(Some(PHRASE), None, Some(PinOptions { secret: "short", kind: "passphrase" }), None).unwrap_err();
    assert!(e.to_string().contains("8 characters"));
    let e = t.ks.add_phrase(Some(PHRASE), None, Some(PinOptions { secret: "12345a", kind: "pin" }), None).unwrap_err();
    assert!(e.to_string().contains("6 digits"));
    t.ks.add_phrase(Some(PHRASE), None, Some(PinOptions { secret: "a long one", kind: "passphrase" }), None).unwrap();
    assert_eq!(t.ks.status().unwrap().kind.as_deref(), Some("passphrase"));
}

#[test]
fn locks_refuses_to_sign_unlocks_with_the_right_pin_only() {
    let t = mk();
    let acc = t.ks.add_phrase(None, None, PIN(), None).unwrap();
    t.ks.lock();
    assert!(!t.ks.is_unlocked().unwrap());
    assert!(t.ks.needs_unlock(&acc.address).unwrap());
    assert_eq!(t.ks.get_signer(&acc.address).unwrap_err(), KeystoreError::Locked);
    assert_eq!(t.ks.unlock("000000").unwrap_err().code(), "wrong");
    t.ks.unlock("123456").unwrap();
    assert_eq!(t.ks.get_signer(&acc.address).unwrap().address(), acc.address);
}

#[test]
fn debug_output_never_shows_secrets() {
    let p = Payload::parse(&serde_json::json!({ "v": 2, "phrases": { "id": PHRASE }, "keys": { "0x1": { "kind": "key", "secretKey": "suiprivkey1secret" } } }).to_string()).unwrap();
    let shown = format!("{p:?} {:?}", p.keys.values().collect::<Vec<_>>());
    assert!(!shown.contains("abandon") && !shown.contains("suiprivkey"), "{shown}");
}

#[test]
fn never_puts_a_phrase_in_public_metadata() {
    let t = mk();
    t.ks.add_phrase(Some(PHRASE), None, PIN(), None).unwrap();
    let raw = std::fs::read_to_string(t._dir.path().join("w").join(FileStore::FILE)).unwrap();
    assert!(!raw.contains("abandon"));
    assert!(!raw.contains("suiprivkey"));
    let blob = std::fs::read_to_string(t._dir.path().join("w").join("vault.json")).unwrap();
    assert!(!blob.contains("abandon"));
}

#[test]
fn a_ui_reload_picks_the_payload_back_up_while_the_vault_stays_unlocked() {
    let t = mk();
    let acc = t.ks.add_phrase(Some(PHRASE), None, PIN(), None).unwrap();
    let revived = Keystore::new(FileStore::new(t._dir.path().join("w")), t.ks.vault().clone());
    assert_eq!(revived.get_signer(&acc.address).unwrap().address(), acc.address);
}

#[test]
fn derives_the_next_index_from_the_same_phrase_and_stores_it_once() {
    let t = mk();
    let a = t.ks.add_phrase(Some(PHRASE), None, PIN(), None).unwrap();
    let b = t.ks.derive_next(None, None).unwrap();
    let c = t.ks.derive_next(None, Some("Games")).unwrap();
    let idx = |x: &RareAccount| x.hd.as_ref().unwrap().index;
    assert_eq!([idx(&a), idx(&b), idx(&c)], [0, 1, 2]);
    assert_eq!(b.address, Keypair::from_mnemonic(PHRASE, 1).unwrap().address());
    assert_eq!(c.label, "Games");
    assert_eq!(b.label, "Account 2");
    assert_eq!(&*t.ks.reveal_mnemonic("123456", &c.address).unwrap(), PHRASE);
    t.ks.remove(&b.address).unwrap();
    assert_eq!(idx(&t.ks.derive_next(None, None).unwrap()), 1);
}

#[test]
fn rejects_duplicates_and_keeps_the_selection_while_locked() {
    let t = mk();
    let a = t.ks.add_phrase(Some(PHRASE), None, PIN(), None).unwrap();
    let b = t.ks.add_phrase(Some(&crate::keys::new_mnemonic(128)), Some("Second"), None, None).unwrap();
    t.ks.select(&b.address).unwrap();
    t.ks.lock();
    assert_eq!(t.ks.current().unwrap().unwrap().address, b.address);
    t.ks.unlock("123456").unwrap();
    assert!(t.ks.add_phrase(Some(PHRASE), None, None, None).unwrap_err().to_string().contains("already exists"));
    let all: Vec<String> = t.ks.list_accounts().unwrap().into_iter().map(|x| x.address).collect();
    assert_eq!(all, vec![a.address, b.address]);
    assert_eq!(t.ks.select("0x1").unwrap_err().to_string(), "Unknown account");
}

#[test]
fn imports_a_suiprivkey_of_every_scheme() {
    let t = mk();
    t.ks.add_phrase(None, None, PIN(), None).unwrap();
    let v = vectors();
    for k in v["keys"].as_array().unwrap().iter().filter(|k| k["scheme"] != "phrase") {
        let acc = t.ks.add_private_key(s(&k["secretKey"]), Some("Imported"), None).unwrap();
        assert_eq!((acc.address.as_str(), acc.kind, acc.label.as_str()), (s(&k["address"]), AccountKind::Native, "Imported"));
        assert_eq!(t.ks.get_signer(&acc.address).unwrap().address(), acc.address);
        assert_eq!(&*t.ks.reveal_private_key("123456", &acc.address).unwrap(), s(&k["secretKey"]));
        assert!(t.ks.reveal_mnemonic("123456", &acc.address).unwrap_err().to_string().contains("No recovery phrase"));
    }
    assert_eq!(t.ks.add_private_key("suiprivkey1nope", None, None).unwrap_err().to_string(), "Invalid private key");
}

#[test]
fn watch_accounts_never_sign_ledger_signs_on_the_device() {
    let t = mk();
    let w = t.ks.add_watch(&format!("0x{}", "ab".repeat(32)), None).unwrap();
    assert_eq!(t.ks.get_signer(&w.address).unwrap_err(), KeystoreError::WatchOnly);
    assert_eq!(t.ks.current_signing_account().unwrap(), None);
    assert_eq!(t.ks.add_watch("0xzz", None).unwrap_err().to_string(), "Invalid Sui address");
    assert_eq!(t.ks.add_watch("abab", None).unwrap_err().to_string(), "Invalid Sui address");
    let pk = Keypair::from_mnemonic(PHRASE, 4).unwrap().public_key_b64();
    let l = t.ks.add_ledger(&pk, "m/44'/784'/0'/0'/0'", None, None).unwrap();
    assert_eq!(l.kind, AccountKind::Ledger);
    let signer = t.ks.get_signer(&l.address).unwrap();
    assert!(matches!(signer, Signer::External { .. }));
    assert!(signer.sign_transaction(b"x").unwrap_err().to_string().contains("not available"));
    assert_eq!(t.ks.add_ledger(&pk, "m/44'/60'/0'/0/0", None, None).unwrap_err().to_string(), "Unsupported Ledger path");
    assert_eq!(t.ks.add_ledger("AAAA", "m/44'/784'/0'/0'/0'", None, None).unwrap_err().to_string(), "Invalid Ledger public key");
    let pk2 = Keypair::from_mnemonic(PHRASE, 5).unwrap().public_key_b64();
    assert_eq!(t.ks.add_keystone(&pk2, "m/44'/784'/0'/0'/0'", "xyz", None, None).unwrap_err().to_string(), "Invalid master fingerprint");
    let k = t.ks.add_keystone(&pk2, "m/44'/784'/0'/0'/0'", "1250B6BC", None, Some("Keystone 3 Pro")).unwrap();
    assert_eq!(k.keystone.as_ref().unwrap().mfp, "1250b6bc");
    assert_eq!(k.label, "Keystone 1");
}

#[test]
fn rename_and_remove() {
    let t = mk();
    let a = t.ks.add_phrase(Some(PHRASE), None, PIN(), None).unwrap();
    assert_eq!(t.ks.rename(&a.address, "   ").unwrap_err().to_string(), "Name cannot be empty");
    assert_eq!(t.ks.rename(&a.address, &"é".repeat(50)).unwrap().label.chars().count(), 40);
    t.ks.remove(&a.address).unwrap();
    assert!(t.ks.list_accounts().unwrap().is_empty());
    assert!(t.ks.current().unwrap().is_none());
    // the phrase went with its last account
    let raw = t.ks.vault().read().unwrap();
    assert!(!raw.contains("abandon"));
    t.ks.wipe().unwrap();
    assert!(!t.ks.is_initialized().unwrap());
}

// ---------------------------------------------------------------- passkey accounts

fn passkey_inputs() -> (String, Keypair, String) {
    let v = vectors();
    let pk = &v["passkey"]["account"]["passkey"];
    (s(&pk["publicKey"]).into(), Keypair::from_secret(s(&v["passkey"]["deviceSecretKey"])).unwrap(), s(&pk["credentialId"]).into())
}

#[test]
fn passkey_signs_with_the_device_key_and_the_multisig_verifies() {
    let t = mk();
    let (pk, device, cred) = passkey_inputs();
    let secret = device.to_suiprivkey();
    let acc = t
        .ks
        .add_passkey(PasskeyAccount { credential_id: &cred, public_key: &pk, device_key: &device.public_key_b64(), device_secret: Some(&secret), label: None, authenticator: None, pin: PIN(), backups: &[] })
        .unwrap();
    assert_eq!(acc.kind, AccountKind::Passkey);
    assert_eq!(acc.label, "Passkey 1");
    assert!(t.ks.has_device_key(&acc.address));
    let signer = t.ks.get_signer(&acc.address).unwrap();
    assert_eq!(signer.address(), acc.address);
    let (bytes, sig) = signer.sign_personal_message(b"rare").unwrap();
    assert_eq!(bytes, B64.encode(b"rare"));
    let user = UserSignature::from_base64(&sig).unwrap();
    sui_crypto::UserSignatureVerifier::new().verify(&personal_message_digest(b"rare"), &user).unwrap();
    let UserSignature::Multisig(m) = user else { panic!("multisig") };
    assert_eq!(m.committee().derive_address().to_string(), acc.address);
}

#[test]
fn passkey_without_the_device_key_signs_on_the_phone() {
    let t = mk();
    let (pk, device, cred) = passkey_inputs();
    let acc = t
        .ks
        .add_passkey(PasskeyAccount { credential_id: &cred, public_key: &pk, device_key: &device.public_key_b64(), device_secret: None, label: None, authenticator: None, pin: None, backups: &[] })
        .unwrap();
    assert!(!t.ks.has_vault().unwrap());
    let signer = t.ks.get_signer(&acc.address).unwrap();
    assert!(matches!(signer, Signer::External { .. }));
    assert!(signer.sign_transaction(b"x").unwrap_err().to_string().contains("not available"));
}

#[test]
fn passkey_refuses_a_device_secret_that_does_not_match() {
    let t = mk();
    let (pk, device, cred) = passkey_inputs();
    let other = Keypair::from_mnemonic(PHRASE, 9).unwrap().to_suiprivkey();
    let e = t
        .ks
        .add_passkey(PasskeyAccount { credential_id: &cred, public_key: &pk, device_key: &device.public_key_b64(), device_secret: Some(&other), label: None, authenticator: None, pin: PIN(), backups: &[] })
        .unwrap_err();
    assert!(e.to_string().contains("does not match"));
}

// ---------------------------------------------------------------- secret.test.ts (keystore half)

#[test]
fn derive_from_key_is_the_same_on_any_device_per_context_and_account() {
    let pinned = "22d6b803437a60cca9d5566218a1c8464c29da5859ea69fb6eac7895976a076c";
    let addr = "0x5e93a736d04fbb25737aa40bee40171ef79f65fae833749e3c089fe7cc2161f1";
    let (a, b) = (mk(), mk());
    for t in [&a, &b] {
        assert_eq!(t.ks.add_phrase(Some(PHRASE), None, PIN(), None).unwrap().address, addr);
    }
    let s1 = a.ks.derive_from_key(addr, DERIVE_SALT, &derive_info("cloud-saves", addr)).unwrap();
    let s2 = b.ks.derive_from_key(addr, DERIVE_SALT, &derive_info("cloud-saves", addr)).unwrap();
    assert_eq!(*s1, *s2);
    assert_eq!(hex(&s1[..]), pinned);
    assert_ne!(*a.ks.derive_from_key(addr, DERIVE_SALT, &derive_info("other", addr)).unwrap(), *s1);
    let next = a.ks.derive_next(None, None).unwrap();
    assert_ne!(*a.ks.derive_from_key(&next.address, DERIVE_SALT, &derive_info("cloud-saves", &next.address)).unwrap(), *s1);
    let w = a.ks.add_watch(&format!("0x{}", "cd".repeat(32)), None).unwrap();
    assert!(a.ks.derive_from_key(&w.address, DERIVE_SALT, &derive_info("x", &w.address)).is_err());
    a.ks.lock();
    assert_eq!(a.ks.derive_from_key(addr, DERIVE_SALT, &derive_info("x", addr)).unwrap_err(), KeystoreError::Locked);
}

// ---------------------------------------------------------------- byte cross-checks with TS

#[test]
fn ts_phrases_derive_the_same_keys_ids_and_secret_strings() {
    let v = vectors();
    for p in v["phrases"].as_array().unwrap() {
        assert_eq!(crate::keys::phrase_id(s(&p["words"])), s(&p["phraseId"]));
        assert_eq!(crate::keys::phrase_id(s(&p["typed"])), s(&p["phraseId"]));
        for a in p["accounts"].as_array().unwrap() {
            let i = a["index"].as_u64().unwrap() as u32;
            let kp = Keypair::from_mnemonic(s(&p["typed"]), i).unwrap();
            assert_eq!(kp.address(), s(&a["address"]), "index {i}");
            assert_eq!(kp.public_key_b64(), s(&a["publicKey"]));
            assert_eq!(&*kp.to_suiprivkey(), s(&a["secretKey"]));
        }
    }
}

#[test]
fn ts_signatures_are_byte_identical_for_every_scheme() {
    let v = vectors();
    let tx = B64.decode(s(&v["tx"])).unwrap();
    let msg = B64.decode(s(&v["message"])).unwrap();
    for k in v["keys"].as_array().unwrap() {
        let kp = Keypair::from_secret(s(&k["secretKey"])).unwrap();
        if let Some(seed) = k["seedHex"].as_str() {
            let by_bytes = Keypair::from_bytes(kp.flag(), &crate::keys::unhex(seed).unwrap()).unwrap();
            assert_eq!(by_bytes.address(), kp.address());
        }
        assert_eq!(kp.address(), s(&k["address"]), "{}", k["scheme"]);
        assert_eq!(kp.public_key_b64(), s(&k["publicKey"]));
        assert_eq!(&*kp.to_suiprivkey(), s(&k["secretKey"]));
        assert_eq!(kp.sign_transaction(&tx), s(&k["txSignature"]), "{} tx", k["scheme"]);
        assert_eq!(kp.sign_personal_message(&msg), s(&k["messageSignature"]), "{} message", k["scheme"]);
        verify_tx(&kp.sign_transaction(&tx), &tx).unwrap();
    }
    let h = &v["hexImport"];
    assert_eq!(Keypair::from_secret(s(&h["hex"])).unwrap().address(), s(&h["address"]));
    assert_eq!(Keypair::from_secret(&s(&h["hex"])[2..]).unwrap().address(), s(&h["address"]));
}

#[test]
fn ts_passkey_multisig_is_byte_identical() {
    let v = vectors();
    let p = &v["passkey"];
    let info = &p["account"]["passkey"];
    let committee = multisig::passkey_multisig_b64(s(&info["publicKey"]), s(&info["deviceKey"])).unwrap();
    assert_eq!(multisig::multisig_address(&committee), s(&p["account"]["address"]));
    assert_eq!(B64.encode(multisig::multisig_public_key_bytes(&committee)), s(&p["multisigPublicKey"]));
    assert_eq!(multisig::passkey_address(&B64.decode(s(&info["publicKey"])).unwrap()).unwrap(), s(&p["passkeyAddress"]));

    let tx = B64.decode(s(&p["tx"])).unwrap();
    let msg = B64.decode(s(&v["message"])).unwrap();
    let dev = MultisigDeviceSigner::new(committee.clone(), Keypair::from_secret(s(&p["deviceSecretKey"])).unwrap()).unwrap();
    assert_eq!(dev.sign_transaction(&tx).unwrap(), s(&p["device"]["txSignature"]));
    assert_eq!(dev.sign_personal_message(&msg).unwrap(), s(&p["device"]["messageSignature"]));
    verify_tx(&dev.sign_transaction(&tx).unwrap(), &tx).unwrap();

    // assertions from the phone: Sui passkey signatures, alone and as the multisig member
    let pk = B64.decode(s(&info["publicKey"])).unwrap();
    let from = |a: &Value, digest: [u8; 32]| {
        let (ad, cdj, der) = (B64.decode(s(&a["authenticatorData"])).unwrap(), B64.decode(s(&a["clientDataJSON"])).unwrap(), B64.decode(s(&a["signature"])).unwrap());
        assert_eq!(B64.decode(s(&a["challenge"])).unwrap(), digest);
        multisig::passkey_signature(&pk, &digest, &multisig::AssertionParts { authenticator_data: &ad, client_data_json: &cdj, signature_der: &der }).unwrap()
    };
    let alone = &p["alone"];
    assert_eq!(from(&alone["txAssertion"], tx_digest(&tx)), s(&alone["txSignature"]));
    // high-S DER from the authenticator is normalized like the TS SDK
    assert_eq!(from(&alone["messageAssertion"], personal_message_digest(&msg)), s(&alone["messageSignature"]));
    let member = &p["member"];
    let tx_member = from(&member["txAssertion"], tx_digest(&tx));
    assert_eq!(multisig::combine_member(&committee, &tx_member).unwrap(), s(&member["txSignature"]));
    let msg_member = from(&member["messageAssertion"], personal_message_digest(&msg));
    assert_eq!(multisig::combine_member(&committee, &msg_member).unwrap(), s(&member["messageSignature"]));
    verify_tx(s(&member["txSignature"]), &tx).unwrap();

    // an assertion over something else is refused
    let a = &alone["txAssertion"];
    let (ad, cdj, der) = (B64.decode(s(&a["authenticatorData"])).unwrap(), B64.decode(s(&a["clientDataJSON"])).unwrap(), B64.decode(s(&a["signature"])).unwrap());
    let parts = multisig::AssertionParts { authenticator_data: &ad, client_data_json: &cdj, signature_der: &der };
    assert!(multisig::passkey_signature(&pk, &personal_message_digest(&msg), &parts).is_err());
    // a member signature from outside the committee is refused
    let stranger = Keypair::from_mnemonic(PHRASE, 3).unwrap().sign_transaction(&tx);
    assert!(multisig::combine_member(&committee, &stranger).is_err());
}

#[test]
fn ts_keystore_payload_and_account_list_open_in_rust() {
    let v = vectors();
    let ks = &v["keystore"];
    // what TS wrote, read by Rust
    let payload = Payload::parse(&ks["payload"].to_string()).unwrap();
    let meta: Meta = serde_json::from_value(ks["meta"].clone()).unwrap();
    // written back by Rust, read the same (shape-identical JSON)
    assert_eq!(serde_json::to_value(&meta).unwrap(), ks["meta"]);
    assert_eq!(serde_json::from_str::<Value>(&payload.to_json()).unwrap(), ks["payload"]);

    // the same wallet opened by the Rust keystore signs and derives like TS
    let dir = tempfile::tempdir().unwrap();
    let vault = Arc::new(Vault::new(dir.path().to_path_buf(), false, FAST));
    vault.create("123456", "pin", &ks["payload"].to_string()).unwrap();
    let store = MemoryStore::default();
    store.save(&meta).unwrap();
    let k = Keystore::new(store, vault);
    assert_eq!(k.current().unwrap().unwrap().label, "Games");
    let accounts = k.list_accounts().unwrap();
    let native: Vec<&RareAccount> = accounts.iter().filter(|a| a.kind == AccountKind::Native).collect();
    assert_eq!(hex(&k.derive_from_key(&native[0].address, DERIVE_SALT, &derive_info("cloud-saves", &native[0].address)).unwrap()[..]), s(&ks["derive"]["phrase"]));
    assert_eq!(hex(&k.derive_from_key(&native[2].address, DERIVE_SALT, &derive_info("cloud-saves", &native[2].address)).unwrap()[..]), s(&ks["derive"]["imported"]));
    for a in &accounts {
        let signer = k.get_signer(&a.address);
        match a.kind {
            AccountKind::Native | AccountKind::Passkey => assert_eq!(signer.unwrap().sign_transaction(b"tx").map(|_| ()), Ok(())),
            AccountKind::Watch => assert_eq!(signer.unwrap_err(), KeystoreError::WatchOnly),
            _ => assert!(matches!(signer.unwrap(), Signer::External { .. })),
        }
        assert_eq!(k.public_key(&a.address).unwrap().is_some(), a.kind != AccountKind::Watch);
    }
    assert!(k.has_device_key(&accounts.iter().find(|a| a.kind == AccountKind::Passkey).unwrap().address));
}

#[test]
fn ts_secret_building_blocks_match() {
    let v = vectors();
    let x = &v["secret"];
    let addr = Keypair::from_mnemonic(PHRASE, 0).unwrap().address();
    assert_eq!(hex(DERIVE_SALT), s(&x["deriveSalt"]));
    assert_eq!(hex(&derive_info("cloud-saves", &addr.to_uppercase().replace("0X", "0x"))), s(&x["info"]));
    assert_eq!(hex(&prf_input("cloud-saves")), s(&x["prfInput"]));
    assert_eq!(hex(&derive_message("cloud-saves", &addr)), s(&x["deriveMessage"]));
    let kp = Keypair::from_mnemonic(PHRASE, 0).unwrap();
    assert_eq!(kp.sign_personal_message(&derive_message("cloud-saves", &addr)), s(&x["ledgerSignature"]));
    assert_eq!(hex(&finish(&ed25519_signature_bytes(s(&x["ledgerSignature"])).unwrap(), "cloud-saves", &addr)[..]), s(&x["ledgerFinish"]));
    use sha2::Digest as _;
    assert_eq!(hex(&finish(&sha2::Sha256::digest(b"prf output"), "cloud-saves", &addr)[..]), s(&x["finishFromPrf"]));
}

#[test]
fn device_vault_accounts_sign_with_no_pin_and_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let w = dir.path().join("w");
    let mk = || Keystore::new(FileStore::new(w.clone()), Arc::new(Vault::new(w.clone(), false, FAST))).with_device(DeviceVault::at(&w, false));
    let ks = mk();
    let a = ks.add_phrase_device(Some(PHRASE), None, None).unwrap();
    assert_eq!(a.vault_id.as_deref(), Some(DEVICE_VAULT));
    assert!(!ks.has_vault().unwrap(), "no PIN vault was made");
    assert!(ks.is_unlocked().unwrap());
    assert!(!ks.needs_unlock(&a.address).unwrap());
    assert_eq!(ks.device_seal(), Some(Seal::File));
    let sig = ks.get_signer(&a.address).unwrap().sign_transaction(b"tx").unwrap();
    // a new process: opens by itself
    let ks2 = mk();
    assert!(ks2.open_device().unwrap());
    assert_eq!(ks2.get_signer(&a.address).unwrap().sign_transaction(b"tx").unwrap(), sig);
    // the next account of the same phrase, no PIN
    let b = ks2.derive_next(None, None).unwrap();
    assert_eq!(b.hd.as_ref().unwrap().index, 1);
    assert!(in_device_vault(&b));
    assert!(ks2.reveal_device_mnemonic(&a.address).unwrap().starts_with("abandon"));
    // the payload on disk is encrypted
    let raw = std::fs::read_to_string(w.join("device/vault.json")).unwrap();
    assert!(!raw.contains("abandon"));
    // removing the last device account removes the device vault and its key
    ks2.remove(&a.address).unwrap();
    assert!(w.join("device/vault.json").exists());
    ks2.remove(&b.address).unwrap();
    assert!(!w.join("device/vault.json").exists());
    assert_eq!(ks2.device_seal(), None);
    assert!(ks2.list_accounts().unwrap().is_empty());
}

#[test]
fn device_vault_beside_a_pin_vault() {
    let dir = tempfile::tempdir().unwrap();
    let w = dir.path().join("w");
    let ks = Keystore::new(FileStore::new(w.clone()), Arc::new(Vault::new(w.clone(), false, FAST))).with_device(DeviceVault::at(&w, false));
    let old = ks.add_phrase(Some(PHRASE), None, PIN(), Some(5)).unwrap();
    ks.lock();
    let key = Keypair::from_mnemonic(&keys::new_mnemonic(128), 0).unwrap().to_suiprivkey().to_string();
    let n = ks.add_private_key_device(&key, Some("Mine")).unwrap();
    assert!(ks.needs_unlock(&old.address).unwrap());
    assert!(!ks.needs_unlock(&n.address).unwrap());
    assert!(ks.get_signer(&n.address).is_ok());
    assert!(matches!(ks.get_signer(&old.address), Err(KeystoreError::Locked)));
    ks.remove(&n.address).unwrap();
    ks.unlock("123456").unwrap();
    assert!(ks.get_signer(&old.address).is_ok());
}

#[test]
fn sealed_secrets_live_in_the_vault_and_need_it_unlocked() {
    let t = mk();
    assert_eq!(t.ks.sealed("agent:claude").unwrap(), None);
    assert_eq!(t.ks.set_sealed("agent:claude", Some("sk-test")).unwrap_err(), KeystoreError::NeedsPassword);
    t.ks.add_phrase(Some(PHRASE), None, PIN(), None).unwrap();
    t.ks.set_sealed("agent:claude", Some("sk-test")).unwrap();
    assert_eq!(t.ks.sealed("agent:claude").unwrap().as_deref().map(|s| s.as_str()), Some("sk-test"));
    // Sealed, not in the clear on disk.
    let raw = std::fs::read_to_string(t._dir.path().join("w").join("vault.json")).unwrap_or_default();
    assert!(!raw.contains("sk-test"));
    t.ks.lock();
    assert_eq!(t.ks.sealed("agent:claude").unwrap_err(), KeystoreError::Locked);
    t.ks.unlock("123456").unwrap();
    assert_eq!(t.ks.sealed("agent:claude").unwrap().as_deref().map(|s| s.as_str()), Some("sk-test"));
    // Accounts saved after keep it.
    t.ks.add_phrase(None, None, None, None).ok();
    assert!(t.ks.sealed("agent:claude").unwrap().is_some());
    t.ks.set_sealed("agent:claude", None).unwrap();
    assert_eq!(t.ks.sealed("agent:claude").unwrap(), None);
}
