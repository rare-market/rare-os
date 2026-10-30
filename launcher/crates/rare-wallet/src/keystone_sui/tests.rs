//! Port of src/wallet/keystone/keystone.test.ts, plus byte cross-checks with the TS UR encoder
//! and the TS mock device's answers (testdata/ts-vectors.json).

use super::fake::{FakeKeystone, TEST_MFP, TEST_PHRASE};
use super::*;
use crate::keys::Keypair;
use serde_json::Value;

fn key(i: u32) -> Keypair {
    Keypair::from_mnemonic(TEST_PHRASE, i).unwrap()
}
fn vectors() -> Value {
    serde_json::from_str(include_str!("../../testdata/ts-vectors.json")).unwrap()
}
fn digest_of(b: &[u8]) -> [u8; 32] {
    use blake2::digest::consts::U32;
    use blake2::Digest;
    blake2::Blake2b::<U32>::digest(b).into()
}

#[test]
fn key_derivation_call_round_trips() {
    let paths = vec![sui_path(0), sui_path(5)];
    let text = key_derivation_ur(&paths);
    assert!(text.starts_with("UR:QR-HARDWARE-CALL/"));
    let ur = decode_ur(&text).unwrap();
    assert_eq!(ur.kind, "qr-hardware-call");
    let m = Cbor::decode(&ur.cbor).unwrap();
    assert_eq!(m.get(1).unwrap().as_u64(), Some(0));
    assert_eq!(m.get(2).unwrap().tag_number(), Some(1301));
    let schemas = m.get(2).unwrap().get(1).unwrap().as_array().unwrap();
    assert_eq!(schemas[0].tag_number(), Some(1302));
    let got: Vec<String> = schemas.iter().map(|s| read_keypath(s.get(1).unwrap()).unwrap().0).collect();
    assert_eq!(got, paths);
    for s in schemas {
        assert_eq!((s.get(2).unwrap().as_u64(), s.get(3).unwrap().as_u64(), s.get(4).unwrap().as_text()), (Some(1), Some(0), Some("SUI")));
    }
    assert_eq!(m.get(3).unwrap().as_text(), Some("Rare Wallet"));
    assert_eq!(m.get(4).unwrap().as_u64(), Some(1));
}

#[test]
fn parses_multi_accounts_master_fingerprint_and_keys_on_the_asked_paths() {
    let mut dev = FakeKeystone::default();
    let answer = dev.handle(2, &key_derivation_ur(&[sui_path(0), sui_path(1)])).unwrap();
    let got = parse_multi_accounts(&decode_ur(&response_payload(&answer).unwrap()).unwrap()).unwrap();
    assert_eq!(got.mfp, TEST_MFP);
    assert_eq!(got.device.as_deref(), Some("Keystone 3 Pro"));
    assert_eq!(got.keys.iter().map(|k| k.0.clone()).collect::<Vec<_>>(), vec![sui_path(0), sui_path(1)]);
    assert_eq!(got.keys[1].1, key(1).public_key());
}

#[test]
fn sign_hash_request_round_trips() {
    let kp = key(0);
    let digest = digest_of(&[1, 2, 3]);
    let (ur, id) = sign_hash_ur(&digest, &sui_path(0), TEST_MFP, &kp.address(), None).unwrap();
    let ok_id = id.len() == 36 && id.as_bytes()[14] == b'4' && b"89ab".contains(&id.as_bytes()[19]);
    assert!(ok_id, "{id}");
    assert!(ur.starts_with("UR:SUI-SIGN-HASH-REQUEST/"));
    let m = Cbor::decode(&decode_ur(&ur).unwrap().cbor).unwrap();
    assert_eq!(m.get(1).unwrap().tag_number(), Some(37));
    assert_eq!(hex(m.get(1).unwrap().as_bytes().unwrap()), id.replace('-', ""));
    assert_eq!(m.get(2).unwrap().as_text().unwrap(), hex(&digest));
    let paths = m.get(3).unwrap().as_array().unwrap();
    assert_eq!(paths[0].tag_number(), Some(304));
    assert_eq!(read_keypath(&paths[0]).unwrap(), (sui_path(0), Some(TEST_MFP.into())));
    assert_eq!(format!("0x{}", hex(m.get(4).unwrap().as_array().unwrap()[0].as_bytes().unwrap())), kp.address());
    assert_eq!(m.get(5).unwrap().as_text(), Some("Rare Wallet"));
}

#[test]
fn sui_signature_parsing() {
    let id = unhex("0123456789abcdef0123456789abcdef").unwrap();
    let pk = key(0).public_key();
    let c = Cbor::Map(vec![(1, Cbor::tag(37, Cbor::Bytes(id))), (2, Cbor::Bytes(vec![9; 64])), (3, Cbor::Bytes(pk.clone()))]);
    let got = parse_sui_signature(&decode_ur(&encode_ur("sui-signature", &c.encode())).unwrap()).unwrap();
    assert_eq!(got.signature, [9; 64]);
    assert_eq!(got.public_key, Some(pk));
    assert_eq!(got.request_id, "01234567-89ab-cdef-0123-456789abcdef");
    // malformed
    let short = Cbor::Map(vec![(1, Cbor::tag(37, Cbor::Bytes(vec![1; 16]))), (2, Cbor::Bytes(vec![0; 63]))]);
    assert_eq!(parse_sui_signature(&decode_ur(&encode_ur("sui-signature", &short.encode())).unwrap()).unwrap_err().code, KeystoneCode::BadResponse);
    assert_eq!(parse_sui_signature(&decode_ur(&key_derivation_ur(&[sui_path(0)])).unwrap()).unwrap_err().code, KeystoneCode::BadResponse);
    assert!(decode_ur("hello").is_none());
}

#[test]
fn answers_as_json_a_json_string_or_a_bare_ur() {
    assert_eq!(response_payload(r#"{"payload":"UR:X/Y"}"#).unwrap(), "UR:X/Y");
    assert_eq!(response_payload(r#""ur:x/y""#).unwrap(), "ur:x/y");
    assert_eq!(response_payload("UR:X/Y").unwrap(), "UR:X/Y");
    assert_eq!(response_payload(r#"{"status":1}"#).unwrap_err().code, KeystoneCode::BadResponse);
}

#[test]
fn signing_digest_is_what_a_sui_key_signs() {
    let kp = key(0);
    let tx = b"some transaction bytes";
    let d = signing_digest(true, tx);
    assert_eq!(d, digest_of(&[&[0u8, 0, 0][..], tx].concat()));
    let sdk = B64.decode(kp.sign_transaction(tx)).unwrap();
    let Keypair::Ed25519(k) = &kp else { unreachable!() };
    use ed25519_dalek::Signer as _;
    assert_eq!(&k.sign(&d).to_bytes()[..], &sdk[1..65]);
    let msg = b"Sign in to Rare";
    assert_eq!(signing_digest(false, msg), digest_of(&[&[3u8, 0, 0][..], &bcs::to_bytes(&msg.to_vec()).unwrap()].concat()));
}

#[test]
fn reads_one_account_and_signs_a_hash_verified_against_the_key() {
    let mut dev = FakeKeystone::default();
    let (mfp, device, accounts) = get_accounts(&mut dev, 2, 1).unwrap();
    assert_eq!((mfp.as_str(), device.as_deref()), (TEST_MFP, Some("Keystone 3 Pro")));
    let a = &accounts[0];
    assert_eq!((a.index, a.path.as_str(), a.address.clone()), (2, sui_path(2).as_str(), key(2).address()));
    let digest = digest_of(&[7]);
    let sig = sign_hash(&mut dev, &a.path, &a.mfp, &a.address, &digest, None).unwrap();
    assert!(verify_ed25519(&key(2).public_key(), &digest, &sig));
    assert!(get_accounts(&mut dev, 0, 25).is_err());
}

#[test]
fn refuses_a_signature_from_another_key_and_a_wrong_fingerprint() {
    let mut dev = FakeKeystone::default();
    let (_, _, accounts) = get_accounts(&mut dev, 0, 1).unwrap();
    let a = &accounts[0];
    let digest = digest_of(&[8]);
    dev.phrase = "legal winner thank year wave sausage worth useful legal winner thank yellow".into();
    let pk = B64.decode(&a.public_key).unwrap();
    assert_eq!(sign_hash(&mut dev, &a.path, &a.mfp, &a.address, &digest, Some(&pk)).unwrap_err().code, KeystoneCode::Mismatch);
    dev.phrase = TEST_PHRASE.into();
    assert_eq!(sign_hash(&mut dev, &a.path, "deadbeef", &a.address, &digest, None).unwrap_err().code, KeystoneCode::Mismatch);
}

#[test]
fn full_flow_accounts_then_transaction_and_message_signatures() {
    use sui_crypto::Verifier as _;
    let mut dev = FakeKeystone::default();
    let (_, _, list) = get_accounts(&mut dev, 0, 3).unwrap();
    assert_eq!(list.iter().map(|a| a.address.clone()).collect::<Vec<_>>(), (0..3).map(|i| key(i).address()).collect::<Vec<_>>());
    assert_eq!(dev.requests, vec!["qr-hardware-call"]);
    let a = &list[1];
    let tx = b"tx bytes for keystone";
    let sig = sign(&mut dev, a, true, tx).unwrap();
    assert_eq!(sig, key(1).sign_transaction(tx));
    assert_eq!(dev.hashes, vec![hex(&tx_digest(tx))]);
    let user = sui_sdk_types::UserSignature::from_base64(&sig).unwrap();
    sui_crypto::UserSignatureVerifier::new().verify(&tx_digest(tx), &user).unwrap();
    assert_eq!(sign(&mut dev, a, false, b"Sign in to Rare").unwrap(), key(1).sign_personal_message(b"Sign in to Rare"));
}

#[test]
fn task_waits_while_locked_or_away_reports_rejection_and_cancels() {
    use std::cell::RefCell;
    let dev = RefCell::new(FakeKeystone { locked: true, ..Default::default() });
    let a = get_accounts(&mut FakeKeystone::default(), 0, 1).unwrap().2.remove(0);
    let mut stages = vec![];
    let r = task(Stage::Confirm, Duration::from_secs(5), Duration::from_millis(1), &mut |s| {
        stages.push(s);
        if s == Stage::Unlock {
            dev.borrow_mut().locked = false;
        }
    }, &|| false, || sign(&mut *dev.borrow_mut(), &a, true, b"x"));
    assert!(r.is_ok());
    assert_eq!(stages, vec![Stage::Confirm, Stage::Unlock, Stage::Confirm, Stage::Done]);
    assert_eq!(Stage::Unlock.text(), "Unlock your Keystone");

    dev.borrow_mut().reject = true;
    let e = task(Stage::Confirm, Duration::from_secs(5), Duration::from_millis(1), &mut |_| {}, &|| false, || sign(&mut *dev.borrow_mut(), &a, true, b"x")).unwrap_err();
    assert_eq!((e.code, e.message.as_str()), (KeystoneCode::Rejected, "Rejected on your Keystone."));

    let e = task(Stage::Confirm, Duration::from_secs(5), Duration::from_millis(1), &mut |_| {}, &|| true, || sign(&mut *dev.borrow_mut(), &a, true, b"x")).unwrap_err();
    assert_eq!(e.code, KeystoneCode::Cancelled);

    // away from the home screen: key export waits for it
    let away = RefCell::new(FakeKeystone { away: true, ..Default::default() });
    let mut seen = vec![];
    let r = task(Stage::Approve, Duration::from_secs(5), Duration::from_millis(1), &mut |s| {
        seen.push(s);
        if s == Stage::HomeScreen {
            away.borrow_mut().away = false;
        }
    }, &|| false, || get_accounts(&mut *away.borrow_mut(), 0, 1));
    assert_eq!(r.unwrap().2.len(), 1);
    assert!(seen.contains(&Stage::HomeScreen));
}

#[test]
fn error_mapping_from_the_transport_and_device_text() {
    let code = |s: &str| KeystoneError::from_native(s).code;
    let fix = "pkexec sh -c \"...\"";
    assert_eq!(KeystoneError::from_native(&crate::err_data("udev", "No access", serde_json::json!({ "fix": fix }))).fix(), Some(fix));
    assert_eq!(code("notfound|x"), KeystoneCode::NoDevice);
    assert_eq!(code("locked|x"), KeystoneCode::Locked);
    assert_eq!(code("rejected|x"), KeystoneCode::Rejected);
    assert_eq!(code("timeout|x"), KeystoneCode::Timeout);
    assert_eq!(code("usb-off|x"), KeystoneCode::UsbOff);
    assert_eq!(code("rejected|Export address is just allowed on specific pages"), KeystoneCode::HomeScreen);
    assert_eq!(code("error|this view type is not supported"), KeystoneCode::Unsupported);
    assert_eq!(code("error|Waiting for user approval"), KeystoneCode::Busy);
    assert_eq!(code("boom"), KeystoneCode::Unknown);
    assert!(KeystoneCode::UsbOff.message().contains("Settings > Connection"));
    assert_eq!(code("usb-off|Open the Keystone home screen and turn on USB in Settings > Connection"), KeystoneCode::UsbOff);
    assert_eq!(code("mismatch|This Keystone holds a different wallet"), KeystoneCode::Mismatch);
    assert_eq!(code("unsupported|Your Keystone does not support this yet. Update its firmware"), KeystoneCode::Unsupported);
    assert_eq!(code("busy|Finish or reject the request on your Keystone first"), KeystoneCode::Busy);
    assert_eq!(code("disconnected|Your Keystone was disconnected"), KeystoneCode::Disconnected);
    assert!(KeystoneError::from_native("rejected|Wrong password on your Keystone").message.contains("Wrong password"));
    for c in KeystoneCode::ALL {
        assert!(c.message().len() > 10 && !c.message().contains('\u{2014}'));
    }
}

#[test]
fn ts_urs_are_byte_identical_and_ts_answers_parse() {
    let v = vectors();
    let k = &v["keystone"];
    let s = |x: &Value| x.as_str().unwrap().to_string();
    let paths: Vec<String> = k["paths"].as_array().unwrap().iter().map(s).collect();
    assert_eq!(key_derivation_ur(&paths), s(&k["derivation"]));
    let accounts = parse_multi_accounts(&decode_ur(&response_payload(&s(&k["accountsAnswer"])).unwrap()).unwrap()).unwrap();
    assert_eq!(accounts.mfp, TEST_MFP);
    assert_eq!(accounts.keys.iter().map(|x| x.0.clone()).collect::<Vec<_>>(), paths);
    assert_eq!(accounts.keys[1].1, key(5).public_key());
    // the Rust fake answers byte-identically to the TS mock
    assert_eq!(FakeKeystone::default().handle(2, &s(&k["derivation"])).unwrap(), s(&k["accountsAnswer"]));

    let bytes = B64.decode(s(&v["sponsor"]["gameBytes"])).unwrap();
    let digest = signing_digest(true, &bytes);
    assert_eq!(hex(&digest), s(&k["digest"]));
    assert_eq!(hex(&signing_digest(false, &B64.decode(s(&v["message"])).unwrap())), s(&k["messageDigest"]));
    let (ur, id) = sign_hash_ur(&digest, &paths[0], TEST_MFP, &key(0).address(), Some(&s(&k["requestId"]))).unwrap();
    assert_eq!((ur.as_str(), id.as_str()), (s(&k["signRequest"]).as_str(), s(&k["requestId"]).as_str()));
    let sig = parse_sui_signature(&decode_ur(&response_payload(&s(&k["signAnswer"])).unwrap()).unwrap()).unwrap();
    assert_eq!(sig.request_id, s(&k["requestId"]));
    assert!(verify_ed25519(&key(0).public_key(), &digest, &sig.signature));
    assert_eq!(FakeKeystone::default().handle(2, &s(&k["signRequest"])).unwrap(), s(&k["signAnswer"]));
    // an answer without a request id is refused, like the TS registry does
    assert_eq!(parse_sui_signature(&decode_ur(&s(&k["bareSignature"])).unwrap()).unwrap_err().code, KeystoneCode::BadResponse);
}
