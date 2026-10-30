//! Port of src/wallet/passkey/passkey.test.ts over the soft authenticator.

use super::soft::{prf_salt_for, SoftAuthenticator};
use super::*;
use crate::keystore::{FileStore, MemoryStore};
use crate::vault::{Kdf, Vault};
use std::cell::RefCell;
use std::sync::Arc;
use sui_crypto::Verifier as _;

const FAST: Kdf = Kdf { m: 256, t: 1, p: 1 };
fn pin() -> Option<PinOptions<'static>> {
    Some(PinOptions { secret: "123456", kind: "pin" })
}

struct Chain {
    funded: Vec<String>,
    asked: RefCell<Vec<String>>,
}
impl History for Chain {
    fn has_history(&self, address: &str) -> bool {
        self.asked.borrow_mut().push(address.into());
        self.funded.iter().any(|f| f == address || f == "*")
    }
}
fn chain(funded: &[&str]) -> Chain {
    Chain { funded: funded.iter().map(|s| s.to_string()).collect(), asked: RefCell::default() }
}

struct Device {
    _dir: tempfile::TempDir,
    ks: Keystore<MemoryStore>,
}
fn device(vault: Option<Arc<Vault>>) -> Device {
    let dir = tempfile::tempdir().unwrap();
    let vault = vault.unwrap_or_else(|| Arc::new(Vault::new(dir.path().to_path_buf(), false, FAST)));
    Device { ks: Keystore::new(MemoryStore::default(), vault), _dir: dir }
}

const MSG: &[u8] = b"rare passkey test";
fn tx_bytes() -> Vec<u8> {
    (0..96u32).map(|i| ((i * 13) & 0xff) as u8).collect()
}

fn verify(sig: &str, digest: &[u8; 32]) -> String {
    let user = sui_sdk_types::UserSignature::from_base64(sig).unwrap();
    sui_crypto::UserSignatureVerifier::new().verify(digest, &user).unwrap();
    match user {
        sui_sdk_types::UserSignature::Multisig(m) => m.committee().derive_address().to_string(),
        _ => panic!("expected a multisig signature"),
    }
}

#[test]
fn create_address_is_the_multisig_and_the_device_key_signs_without_the_passkey() {
    let mut soft = SoftAuthenticator::new();
    let d = device(None);
    let acc = create_account(&d.ks, &mut soft, Via::Phone, Some("Main"), pin()).unwrap();
    assert_eq!(soft.registers, 1);
    assert_eq!((acc.kind, acc.label.as_str()), (AccountKind::Passkey, "Main"));
    let info = acc.passkey.clone().unwrap();
    assert_eq!(info.authenticator.as_deref(), Some("phone"));
    let c = multisig::passkey_multisig_b64(&info.public_key, &info.device_key).unwrap();
    assert_eq!(acc.address, multisig::multisig_address(&c));
    assert!(d.ks.has_device_key(&acc.address));
    assert_eq!(d.ks.current().unwrap().unwrap().address, acc.address);
    let signer = d.ks.get_signer(&acc.address).unwrap();
    let (_, pm) = signer.sign_personal_message(MSG).unwrap();
    assert_eq!(verify(&pm, &personal_message_digest(MSG)), acc.address);
    let tx = signer.sign_transaction(&tx_bytes()).unwrap();
    assert_eq!(verify(&tx, &tx_digest(&tx_bytes())), acc.address);
    assert!(soft.asserts.is_empty());
    // the user handle carries the device key
    let a = soft.assert_raw(&B64.encode([0u8; 32]), &[], None).unwrap();
    assert_eq!(device_key_from_user_handle(a.user_handle.as_deref()).map(|k| B64.encode(k)), Some(info.device_key));
}

#[test]
fn refuses_to_start_a_ceremony_it_could_not_save() {
    let mut soft = SoftAuthenticator::new();
    let d = device(None);
    let e = create_account(&d.ks, &mut soft, Via::Usb, None, None).unwrap_err();
    assert!(e.to_string().contains("PIN"));
    assert_eq!(soft.registers, 0);
    // a locked vault too
    create_account(&d.ks, &mut soft, Via::Usb, None, pin()).unwrap();
    d.ks.lock();
    assert_eq!(create_account(&d.ks, &mut soft, Via::Usb, None, None).unwrap_err(), PasskeyError::Keystore(KeystoreError::Locked));
    assert_eq!(soft.registers, 1);
}

#[test]
fn passkey_member_signature_verifies_for_the_account() {
    let mut soft = SoftAuthenticator::new();
    let d = device(None);
    let acc = create_account(&d.ks, &mut soft, Via::Usb, None, pin()).unwrap();
    let pm = sign_with_passkey(&acc, &mut soft, Via::Usb, false, MSG).unwrap();
    assert_eq!(verify(&pm, &personal_message_digest(MSG)), acc.address);
    let tx = sign_with_passkey(&acc, &mut soft, Via::Usb, true, &tx_bytes()).unwrap();
    assert_eq!(verify(&tx, &tx_digest(&tx_bytes())), acc.address);
    let cred = acc.passkey.as_ref().unwrap().credential_id.clone();
    assert_eq!(soft.asserts, vec![vec![cred.clone()], vec![cred]]);
    // the multisig carries a passkey member signature, not the device key's
    let sui_sdk_types::UserSignature::Multisig(m) = sui_sdk_types::UserSignature::from_base64(&tx).unwrap() else { panic!() };
    assert!(matches!(m.signatures()[0], sui_sdk_types::MultisigMemberSignature::Passkey(_)));
    assert_eq!(m.bitmap(), 1);
}

#[test]
fn sign_in_on_a_new_device_rebuilds_the_same_address() {
    let mut soft = SoftAuthenticator::new();
    let a = device(None);
    let created = create_account(&a.ks, &mut soft, Via::Phone, None, pin()).unwrap();
    let b = device(None);
    soft.asserts.clear();
    let acc = sign_in(&b.ks, &mut soft, &chain(&[]), Via::Phone, None).unwrap();
    assert_eq!(acc.address, created.address);
    let (pi, ci) = (acc.passkey.clone().unwrap(), created.passkey.clone().unwrap());
    assert_eq!((pi.credential_id.clone(), pi.public_key, pi.device_key), (ci.credential_id.clone(), ci.public_key, ci.device_key));
    assert_eq!(soft.asserts, vec![vec![], vec![ci.credential_id]]);
    assert!(!b.ks.has_device_key(&acc.address));
    // no device key here: the account signs with the passkey
    assert!(matches!(b.ks.get_signer(&acc.address).unwrap(), crate::keystore::Signer::External { .. }));
    let pm = sign_with_passkey(&acc, &mut soft, Via::Phone, false, MSG).unwrap();
    assert_eq!(verify(&pm, &personal_message_digest(MSG)), created.address);
}

#[test]
fn chain_history_picks_the_account_and_skips_the_second_assertion() {
    let mut soft = SoftAuthenticator::new();
    let created = create_account(&device(None).ks, &mut soft, Via::Usb, None, pin()).unwrap();
    let b = device(None);
    let c = chain(&[&created.address]);
    let acc = sign_in(&b.ks, &mut soft, &c, Via::Usb, None).unwrap();
    assert_eq!(acc.address, created.address);
    assert_eq!(acc.passkey.unwrap().authenticator.as_deref(), Some("security-key"));
    assert_eq!(soft.asserts, vec![Vec::<String>::new()]);
    assert!(c.asked.borrow().contains(&created.address));
}

#[test]
fn history_on_more_than_one_candidate_falls_back_to_the_second_assertion() {
    let mut soft = SoftAuthenticator::new();
    let created = create_account(&device(None).ks, &mut soft, Via::Phone, None, pin()).unwrap();
    let b = device(None);
    let c = chain(&["*"]);
    let acc = sign_in(&b.ks, &mut soft, &c, Via::Phone, None).unwrap();
    assert_eq!(acc.address, created.address);
    let candidates = c.asked.borrow().iter().collect::<std::collections::HashSet<_>>().len();
    let cred = created.passkey.unwrap().credential_id;
    assert_eq!(soft.asserts, if candidates > 1 { vec![vec![], vec![cred]] } else { vec![vec![]] });
}

#[test]
fn returning_user_on_this_device_no_recovery_no_chain_reads() {
    let mut soft = SoftAuthenticator::new();
    let a = device(None);
    let created = create_account(&a.ks, &mut soft, Via::Phone, None, pin()).unwrap();
    let c = chain(&[]);
    let again = sign_in(&a.ks, &mut soft, &c, Via::Phone, None).unwrap();
    assert_eq!(again.address, created.address);
    assert_eq!(soft.asserts, vec![Vec::<String>::new()]);
    assert!(c.asked.borrow().is_empty());
    assert_eq!(a.ks.list_accounts().unwrap().len(), 1);
}

#[test]
fn account_list_lost_but_vault_kept_the_device_key_names_the_account() {
    let mut soft = SoftAuthenticator::new();
    let a = device(None);
    let created = create_account(&a.ks, &mut soft, Via::Phone, None, pin()).unwrap();
    let b = Keystore::new(MemoryStore::default(), a.ks.vault().clone());
    let c = chain(&[]);
    let acc = sign_in(&b, &mut soft, &c, Via::Phone, None).unwrap();
    assert_eq!(acc.address, created.address);
    assert_eq!(soft.asserts.len(), 1);
    assert!(c.asked.borrow().is_empty());
    assert!(b.has_device_key(&acc.address));
    b.get_signer(&acc.address).unwrap().sign_personal_message(MSG).unwrap();
    assert_eq!(soft.asserts.len(), 1);
}

#[test]
fn rejects_passkeys_rare_did_not_make_and_answers_to_other_requests() {
    assert_eq!(device_key_from_user_handle(None), None);
    let mut h = vec![3u8];
    h.extend([0u8; 32]);
    assert_eq!(device_key_from_user_handle(Some(&B64.encode(&h))), None);
    // 0x02: an account with more than one passkey (its members come from the registry)
    let mut h = vec![2u8];
    h.extend([5u8; 32]);
    assert_eq!(device_key_from_user_handle(Some(&B64.encode(&h))), Some(vec![5u8; 32]));
    assert!(user_handle_is_multi(Some(&B64.encode(&h))));
    let mut h = vec![1u8];
    h.extend([7u8; 32]);
    assert_eq!(device_key_from_user_handle(Some(&B64.encode(&h))), Some(vec![7u8; 32]));

    let mut soft = SoftAuthenticator::new();
    create_account(&device(None).ks, &mut soft, Via::Phone, None, pin()).unwrap();
    let challenge = [1u8; 32];
    let a = soft.assert_raw(&B64.encode(challenge), &[], None).unwrap();
    assert!(recover_passkey_keys(&a, &[2u8; 32]).unwrap_err().to_string().contains("different request"));
    assert!(!recover_passkey_keys(&a, &challenge).unwrap().is_empty());
    // a passkey from another site
    let mut other = a.clone();
    let mut ad = B64.decode(&other.authenticator_data).unwrap();
    ad[0] ^= 1;
    other.authenticator_data = B64.encode(ad);
    assert!(recover_passkey_keys(&other, &challenge).unwrap_err().to_string().contains("not a Rare passkey"));
}

#[test]
fn prf_same_input_same_output_on_any_device() {
    let mut soft = SoftAuthenticator::new();
    let a = device(None);
    let acc = create_account(&a.ks, &mut soft, Via::Phone, None, pin()).unwrap();
    let input: [u8; 32] = Sha256::digest(b"rare/cloud-save").into();
    let one = passkey_prf(&acc, &mut soft, Via::Usb, &input).unwrap();
    assert_eq!(one.len(), 32);
    assert_eq!(soft.prfs, vec![(vec![acc.passkey.clone().unwrap().credential_id], Via::Usb)]);

    let b = device(None);
    let again = sign_in(&b.ks, &mut soft, &chain(&[]), Via::Phone, None).unwrap();
    assert_eq!(again.address, acc.address);
    assert_eq!(*passkey_prf(&again, &mut soft, Via::Phone, &input).unwrap(), *one);
    let other: [u8; 32] = Sha256::digest(b"rare/other").into();
    assert_ne!(*passkey_prf(&again, &mut soft, Via::Phone, &other).unwrap(), *one);
    assert!(passkey_prf(&acc, &mut soft, Via::Usb, &[0u8; 16]).unwrap_err().to_string().contains("32 bytes"));

    // the assertion carries the same PRF output
    let a2 = soft.assert_raw(&B64.encode([0u8; 32]), &[acc.passkey.clone().unwrap().credential_id], Some(&B64.encode(input))).unwrap();
    assert_eq!(B64.decode(a2.prf.unwrap()).unwrap(), *one);
    // the same salt as the CTAP side and python-fido2 (vector in passkey/tests.rs)
    assert_eq!(crate::keys::hex(&prf_salt_for(&input)), "7f8501bdda5b0bbaeb9ca1bcebfdef8e2a5570d4e3c30f4239b442b73d2ac48c");
    assert_eq!(soft.assert_raw(&B64.encode([0u8; 32]), &[], None).unwrap().prf, None);
}

#[test]
fn prf_needs_a_passkey_account() {
    let mut soft = SoftAuthenticator::new();
    let w = RareAccount { address: "0x1".into(), label: "w".into(), kind: AccountKind::Watch, hd: None, passkey: None, vault_id: None, ledger: None, keystone: None };
    assert!(passkey_prf(&w, &mut soft, Via::Usb, &[0u8; 32]).unwrap_err().to_string().contains("Not a passkey account"));
    let _ = FileStore::FILE;
}

#[test]
fn ts_recovery_and_user_handle_match() {
    let v: serde_json::Value = serde_json::from_str(include_str!("../../testdata/ts-vectors.json")).unwrap();
    let p = &v["passkey"];
    let a = &p["alone"]["txAssertion"];
    let s = |x: &serde_json::Value| x.as_str().unwrap().to_string();
    let assertion = Assertion { credential_id: "AQID".into(), authenticator_data: s(&a["authenticatorData"]), client_data_json: s(&a["clientDataJSON"]), signature: s(&a["signature"]), user_handle: None, prf: None };
    let got: Vec<String> = recover_passkey_keys(&assertion, &B64.decode(s(&a["challenge"])).unwrap()).unwrap().iter().map(|k| B64.encode(k)).collect();
    let want: Vec<String> = p["recovered"].as_array().unwrap().iter().map(s).collect();
    assert_eq!(got, want);
    assert!(got.contains(&s(&p["account"]["passkey"]["publicKey"])));
    let device = Keypair::from_secret(&s(&p["deviceSecretKey"])).unwrap();
    assert_eq!(B64.encode(user_handle_for(&device.public_key())), s(&p["userHandle"]));
}

#[test]
fn registry_hit_picks_the_account_in_one_touch() {
    struct Reg(String, RefCell<usize>);
    impl History for Reg {
        fn has_history(&self, _: &str) -> bool {
            panic!("registry decides before chain history")
        }
        fn registered(&self, _: &[u8], _: &[u8]) -> Option<String> {
            *self.1.borrow_mut() += 1;
            Some(self.0.clone())
        }
    }
    let mut soft = SoftAuthenticator::new();
    let created = create_account(&device(None).ks, &mut soft, Via::Phone, None, pin()).unwrap();
    let b = device(None);
    soft.asserts.clear();
    let acc = sign_in(&b.ks, &mut soft, &Reg(created.address.clone(), RefCell::default()), Via::Phone, None).unwrap();
    assert_eq!(acc.address, created.address);
    assert_eq!(soft.asserts, vec![Vec::<String>::new()]);
}

// ---------------------------------------------------------------- PIN accounts

struct PinDev {
    _dir: tempfile::TempDir,
    ks: Keystore<MemoryStore>,
}

fn pin_device() -> PinDev {
    let dir = tempfile::tempdir().unwrap();
    let vault = Arc::new(Vault::new(dir.path().to_path_buf(), false, FAST));
    let pd = crate::pin_device::PinDevice::new(dir.path().join("pin"), crate::pin_guard::PinGuard::local_for_tests(dir.path().join("pin")));
    PinDev { ks: Keystore::new(MemoryStore::default(), vault).with_pin(pd), _dir: dir }
}

struct Registry(Vec<(String, Vec<String>)>);
impl History for Registry {
    fn has_history(&self, _: &str) -> bool {
        false
    }
    fn registered_members(&self, pk: &[u8], _dk: &[u8]) -> Option<(String, Vec<String>)> {
        self.0.iter().find(|(_, m)| m.contains(&B64.encode(pk))).cloned()
    }
}

#[test]
fn pin_account_phone_and_key_sign_lock_unlock() {
    let mut soft = SoftAuthenticator::new();
    let d = pin_device();
    let mut steps = Vec::new();
    let (acc, kp) = create_pin_account(&d.ks, &mut soft, &[Via::Phone, Via::Usb], Some("Main"), "123456", &mut |i| steps.push(i)).unwrap();
    assert_eq!(steps, [0, 1]);
    assert_eq!(soft.registers, 2);
    let info = acc.passkey.clone().unwrap();
    assert!(info.is_pin() && info.backups.len() == 1);
    assert_eq!(info.device_key, kp.public_key_b64());
    // the address is the 1-of-3 multisig: phone, device key, security key
    let c = info.committee().unwrap();
    assert_eq!(c.members().len(), 3);
    assert_eq!(multisig::multisig_address(&c), acc.address);
    assert!(d.ks.is_pin_account(&acc.address) && d.ks.has_device_key(&acc.address) && !d.ks.needs_unlock(&acc.address).unwrap());

    // no PIN yet: the signer is the passkey (the sheet asks for the PIN first)
    assert!(matches!(d.ks.get_signer(&acc.address).unwrap(), crate::keystore::Signer::External { .. }));
    d.ks.arm_pin(&acc.address, "123456").unwrap();
    let sig = d.ks.get_signer(&acc.address).unwrap().sign_transaction(&tx_bytes()).unwrap();
    assert_eq!(verify(&sig, &tx_digest(&tx_bytes())), acc.address);
    // the PIN opened one signature only
    assert!(matches!(d.ks.get_signer(&acc.address).unwrap(), crate::keystore::Signer::External { .. }));

    // either backup signs
    for via in [Via::Phone, Via::Usb] {
        let n = soft.asserts.len();
        let sig = sign_with_passkey(&acc, &mut soft, via, true, &tx_bytes()).unwrap();
        assert_eq!(verify(&sig, &tx_digest(&tx_bytes())), acc.address);
        assert_eq!(soft.asserts[n], vec![credentials_for(&acc, via)[0].credential_id.clone()]);
    }

    // five wrong PINs lock it; the phone unlocks, a new PIN is set, the same key signs
    let msgs: Vec<String> = (0..5).map(|_| d.ks.arm_pin(&acc.address, "000000").unwrap_err().to_string()).collect();
    assert_eq!(msgs[0], "Wrong PIN");
    assert_eq!(msgs[2], "2 tries left. After 5 wrong PINs this device locks and you'll need your phone to unlock it.");
    assert_eq!(msgs[3], "1 try left. After 5 wrong PINs this device locks and you'll need your phone to unlock it.");
    assert_eq!(msgs[4], "Locked. Use your phone to unlock");
    assert_eq!(d.ks.arm_pin(&acc.address, "123456").unwrap_err().code(), "pin-locked");
    // locked: the phone still signs directly
    let sig = sign_with_passkey(&acc, &mut soft, Via::Phone, false, MSG).unwrap();
    assert_eq!(verify(&sig, &personal_message_digest(MSG)), acc.address);
    // the security key can unlock too
    let token = unlock_with_phone(&d.ks, &acc, &mut soft, Via::Usb).unwrap();
    reset_pin(&d.ks, &acc.address, &token, "654321").unwrap();
    let sig = d.ks.get_signer(&acc.address).unwrap().sign_transaction(&tx_bytes()).unwrap();
    assert_eq!(verify(&sig, &tx_digest(&tx_bytes())), acc.address);
    d.ks.arm_pin(&acc.address, "654321").unwrap();
    d.ks.disarm();

    // a second SuiPlay: sign in with the security key, the registry gives the members
    let reg = Registry(vec![(acc.address.clone(), info.passkeys().iter().map(|m| m.public_key.clone()).collect())]);
    let other = device(None);
    let got = sign_in(&other.ks, &mut soft, &reg, Via::Usb, None).unwrap();
    assert_eq!(got.address, acc.address);
    let sig = sign_with_passkey(&got, &mut soft, Via::Usb, true, &tx_bytes()).unwrap();
    assert_eq!(verify(&sig, &tx_digest(&tx_bytes())), acc.address);
    // no registry: a multi-passkey account can't be found from one signature
    let third = device(None);
    assert!(sign_in(&third.ks, &mut soft, &Registry(vec![]), Via::Usb, None).is_err());

    // removing the last PIN account forgets the device key
    d.ks.remove(&acc.address).unwrap();
    assert!(d.ks.pin_device().unwrap().public_key().is_none());
}

#[test]
fn one_backup_keeps_the_original_two_member_address() {
    let mut soft = SoftAuthenticator::new();
    let d = pin_device();
    let (acc, kp) = create_pin_account(&d.ks, &mut soft, &[Via::Phone], None, "123456", &mut |_| {}).unwrap();
    let info = acc.passkey.unwrap();
    let pk = B64.decode(&info.public_key).unwrap();
    assert_eq!(multisig::multisig_address(&multisig::passkey_multisig(&pk, &kp.public_key()).unwrap()), acc.address);
    // the phone's user handle is the v1 layout
    let a = soft.assert_raw(&B64.encode([9u8; 32]), &[], None).unwrap();
    assert_eq!(device_key_from_user_handle(a.user_handle.as_deref()), Some(kp.public_key()));
    assert!(!user_handle_is_multi(a.user_handle.as_deref()));
    // a second account on this SuiPlay reuses the device key and needs its PIN
    assert!(create_pin_account(&d.ks, &mut soft, &[Via::Usb], None, "000000", &mut |_| {}).is_err());
    let (b, _) = create_pin_account(&d.ks, &mut soft, &[Via::Usb], None, "123456", &mut |_| {}).unwrap();
    assert_eq!(b.passkey.unwrap().device_key, info.device_key);
}

#[test]
fn restore_pin_signing_on_a_new_suiplay_same_address() {
    let mut soft = SoftAuthenticator::new();
    let a = pin_device();
    let (acc, _) = create_pin_account(&a.ks, &mut soft, &[Via::Phone, Via::Usb], None, "123456", &mut |_| {}).unwrap();
    assert!(a.ks.pin_words("000000").is_err());
    let words = a.ks.pin_words("123456").unwrap();
    let reg = Registry(vec![(acc.address.clone(), acc.passkey.as_ref().unwrap().passkeys().iter().map(|m| m.public_key.clone()).collect())]);

    // a new SuiPlay: the words and one touch of the security key
    let b = pin_device();
    let got = restore_pin_account(&b.ks, &mut soft, &reg, Via::Usb, &words, "777777").unwrap();
    assert_eq!(got.address, acc.address);
    assert!(b.ks.is_pin_account(&got.address));
    b.ks.arm_pin(&got.address, "777777").unwrap();
    let sig = b.ks.get_signer(&got.address).unwrap().sign_transaction(&tx_bytes()).unwrap();
    assert_eq!(verify(&sig, &tx_digest(&tx_bytes())), acc.address);
    // the backups can unlock it there too
    for _ in 0..5 {
        let _ = b.ks.arm_pin(&got.address, "000000");
    }
    let token = unlock_with_phone(&b.ks, &got, &mut soft, Via::Usb).unwrap();
    reset_pin(&b.ks, &got.address, &token, "888888").unwrap();
    b.ks.disarm();
    b.ks.change_pin("888888", "999999").unwrap();
    assert!(b.ks.arm_pin(&got.address, "999999").is_ok());

    // other words: refused, nothing changes
    let c = pin_device();
    let other = rare_wallet_words();
    assert!(restore_pin_account(&c.ks, &mut soft, &reg, Via::Usb, &other, "777777").is_err());
    assert!(c.ks.pin_accounts().unwrap().is_empty());
}

fn rare_wallet_words() -> String {
    crate::keys::new_mnemonic(128).to_string()
}
