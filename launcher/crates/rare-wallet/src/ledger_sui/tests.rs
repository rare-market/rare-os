//! Port of src/wallet/ledger/ledger.test.ts (and the session parts of api.test.ts), plus the
//! byte-level cross-check: the same calls send exactly the APDUs @mysten/ledgerjs-hw-app-sui
//! sends (testdata/ts-vectors.json, recorded from the TS fake device).

use super::fake::{device_key, FakeLedger, TEST_PHRASE};
use super::*;
use crate::keys::{sui_path, Keypair};
use serde_json::Value;
use std::cell::RefCell;
use std::rc::Rc;
use sui_sdk_types::{Address, Digest as ObjDigest, GasPayment, ObjectReference, ProgrammableTransaction, TransactionExpiration, TransferObjects};

fn obj(n: u64) -> String {
    format!("0x{n:064x}")
}

fn tx_bytes(sender: &str, gas_owner: Option<&str>) -> Vec<u8> {
    let r = |n: u64, v: u64| ObjectReference::new(obj(n).parse::<Address>().unwrap(), v, ObjDigest::new([1; 32]));
    let p = ProgrammableTransaction {
        inputs: vec![Input::ImmutableOrOwned(r(2, 3)), Input::Pure(bcs::to_bytes(&obj(0xbeef).parse::<Address>().unwrap()).unwrap())],
        commands: vec![sui_sdk_types::Command::TransferObjects(TransferObjects { objects: vec![sui_sdk_types::Argument::Input(0)], address: sui_sdk_types::Argument::Input(1) })],
    };
    let tx = Transaction {
        kind: TransactionKind::ProgrammableTransaction(p),
        sender: sender.parse().unwrap(),
        gas_payment: GasPayment { objects: vec![r(1, 7)], owner: gas_owner.unwrap_or(sender).parse().unwrap(), price: 1000, budget: 5_000_000 },
        expiration: TransactionExpiration::None,
    };
    bcs::to_bytes(&tx).unwrap()
}

fn verify_tx(sig: &str, bytes: &[u8]) -> bool {
    use sui_crypto::Verifier as _;
    let s = sui_sdk_types::UserSignature::from_base64(sig).unwrap();
    sui_crypto::UserSignatureVerifier::new().verify(&tx_digest(bytes), &s).is_ok()
}

// ---------------------------------------------------------------- paths and addresses

#[test]
fn derivation_paths() {
    assert_eq!(sui_path(0), "m/44'/784'/0'/0'/0'");
    assert_eq!(crate::keys::path_index(&sui_path(12)), Some(12));
    assert_eq!(crate::keys::path_index("m/44'/60'/0'/0/0"), None);
    assert_eq!(crate::keys::path_index("m/44'/784'/0'/0'/0'/1"), None);
    assert_eq!(bip32_payload("m/44'/784'/0'/0'/0'"), crate::keys::unhex("052c00008010030080000000800000008000000080").unwrap());
    assert_eq!(fake::parse_bip32(&bip32_payload("m/44'/784'/7'/0'/0'")), "m/44'/784'/7'/0'/0'");
}

#[test]
fn address_from_public_key() {
    let kp = device_key(TEST_PHRASE, &sui_path(3));
    assert_eq!(address_of(FLAG_ED25519, &kp.public_key()).unwrap(), kp.address());
    assert_eq!(crate::keys::ed25519_address(&kp.public_key_b64()).unwrap(), kp.address());
    assert!(address_of(FLAG_ED25519, &[0u8; 33]).is_err());
}

// ---------------------------------------------------------------- the Sui app over a fake device

#[test]
fn lists_the_first_3_addresses_without_a_prompt() {
    let mut t = FakeLedger::default();
    for i in 0..3 {
        let (_, addr) = read_account(&mut t, &sui_path(i), false).unwrap();
        assert_eq!(addr, device_key(TEST_PHRASE, &sui_path(i)).address());
    }
    assert!(t.shown.is_empty());
}

#[test]
fn shows_an_address_only_when_asked() {
    let mut t = FakeLedger::default();
    read_account(&mut t, &sui_path(1), true).unwrap();
    assert_eq!(t.shown, vec![sui_path(1)]);
    t.reject = true;
    assert_eq!(read_account(&mut t, &sui_path(1), true).unwrap_err().code, LedgerCode::Rejected);
}

#[test]
fn signs_a_transaction_with_intent_path_objects_and_serialized_signature() {
    let mut t = FakeLedger::default();
    let path = sui_path(0);
    let kp = device_key(TEST_PHRASE, &path);
    let bytes = tx_bytes(&kp.address(), None);
    let ids = clear_signing_object_ids(&bytes);
    assert_eq!(ids, vec![obj(1), obj(2)]);
    let objects: Vec<Vec<u8>> = ids.iter().enumerate().map(|(i, _)| vec![0xaa, i as u8]).collect();
    let sig = sign_transaction(&mut t, &path, &kp.public_key(), &bytes, &objects).unwrap();
    let call = &t.signs[0];
    assert_eq!(call.path, path);
    assert_eq!(&call.payload[..3], &[0, 0, 0]);
    assert_eq!(&call.payload[3..], &bytes[..]);
    assert_eq!(call.objects, vec![vec![0xaa, 0], vec![0xaa, 1]]);
    let raw = B64.decode(&sig).unwrap();
    assert_eq!((raw.len(), raw[0]), (97, 0));
    assert_eq!(&raw[65..], &kp.public_key()[..]);
    assert!(verify_tx(&sig, &bytes));
}

#[test]
fn still_signs_without_clear_signing_objects() {
    let mut t = FakeLedger::default();
    let kp = device_key(TEST_PHRASE, &sui_path(0));
    let bytes = tx_bytes(&kp.address(), None);
    let sig = sign_transaction(&mut t, &sui_path(0), &kp.public_key(), &bytes, &[]).unwrap();
    assert!(t.signs[0].objects.is_empty());
    assert!(verify_tx(&sig, &bytes));
}

#[test]
fn signs_a_sponsored_transaction_as_the_sender() {
    let mut t = FakeLedger::default();
    let kp = device_key(TEST_PHRASE, &sui_path(0));
    let sponsor = device_key(TEST_PHRASE, &sui_path(9));
    let bytes = tx_bytes(&kp.address(), Some(&sponsor.address()));
    let sender_sig = sign_transaction(&mut t, &sui_path(0), &kp.public_key(), &bytes, &[]).unwrap();
    assert!(verify_tx(&sender_sig, &bytes));
    assert!(verify_tx(&sponsor.sign_transaction(&bytes), &bytes));
    let parsed: Transaction = bcs::from_bytes(&bytes).unwrap();
    assert_eq!(parsed.gas_payment.owner.to_string(), sponsor.address());
}

#[test]
fn signs_a_personal_message_with_the_intent_over_bcs_bytes() {
    let mut t = FakeLedger::default();
    let kp = device_key(TEST_PHRASE, &sui_path(2));
    let msg = b"Sign in to Rare";
    let sig = sign_personal_message(&mut t, &sui_path(2), &kp.public_key(), msg).unwrap();
    let p = &t.signs[0].payload;
    assert_eq!(&p[..3], &[3, 0, 0]);
    assert_eq!(&p[3..], &bcs::to_bytes(&msg.to_vec()).unwrap()[..]);
    assert!(t.signs[0].objects.is_empty());
    assert_eq!(sig, kp.sign_personal_message(msg));
}

#[test]
fn refuses_a_device_with_a_different_key() {
    let mut t = FakeLedger::new().phrase("legal winner thank year wave sausage worth useful legal winner thank yellow");
    let kp = device_key(TEST_PHRASE, &sui_path(0));
    let e = sign_transaction(&mut t, &sui_path(0), &kp.public_key(), &tx_bytes(&kp.address(), None), &[]).unwrap_err();
    assert_eq!(e.code, LedgerCode::Mismatch);
    assert!(t.signs.is_empty());
}

#[test]
fn maps_device_answers_rejected_blind_signing_locked() {
    let kp = device_key(TEST_PHRASE, &sui_path(0));
    let bytes = tx_bytes(&kp.address(), None);
    let run = |t: &mut FakeLedger| sign_transaction(t, &sui_path(0), &kp.public_key(), &bytes, &[]).unwrap_err().code;
    assert_eq!(run(&mut FakeLedger::new().rejecting()), LedgerCode::Rejected);
    assert_eq!(run(&mut FakeLedger::new().blind_off()), LedgerCode::BlindSigning);
    assert_eq!(run(&mut FakeLedger::new().locked()), LedgerCode::Locked);
}

#[test]
fn checks_the_open_app() {
    assert_eq!(check_sui_app(&mut FakeLedger::default()).unwrap(), Some(("Sui".into(), "1.4.0".into())));
    assert_eq!(check_sui_app(&mut FakeLedger::new().app("BOLOS")).unwrap_err().code, LedgerCode::AppClosed);
    let wrong = check_sui_app(&mut FakeLedger::new().app("Bitcoin")).unwrap_err();
    assert_eq!(wrong.code, LedgerCode::WrongApp);
    assert!(wrong.message.contains("Bitcoin app open"));
    assert_eq!(check_sui_app(&mut FakeLedger::new().locked()).unwrap_err().code, LedgerCode::Locked);
}

// ---------------------------------------------------------------- formatting

#[test]
fn serializes_ed25519_device_signatures_and_refuses_malformed_ones() {
    let s = B64.decode(serialize_signature(&[1u8; 64], &[7u8; 32]).unwrap()).unwrap();
    assert_eq!((s[0], s.len()), (0, 97));
    assert!(serialize_signature(&[0u8; 63], &[7u8; 32]).is_err());
}

#[test]
fn parses_app_and_version_and_judges_the_app() {
    let mut r = vec![1, 3];
    r.extend_from_slice(b"Sui");
    r.push(5);
    r.extend_from_slice(b"1.4.0");
    r.extend_from_slice(&[1, 0]);
    assert_eq!(parse_app_and_version(&r), Some(("Sui".into(), "1.4.0".into())));
    assert!(assert_sui_app("Sui").is_ok());
    assert!(assert_sui_app("BOLOS").unwrap_err().message.contains("Open the Sui app"));
    assert_eq!(parse_app_and_version(&[2, 0, 0]), None);
}

// ---------------------------------------------------------------- error mapping

#[test]
fn maps_status_words() {
    let c = |sw| LedgerError::from_status(sw).code;
    assert_eq!(c(0x5515), LedgerCode::Locked);
    assert_eq!(c(0x6b0c), LedgerCode::Locked);
    assert_eq!(c(0x6985), LedgerCode::Rejected);
    assert_eq!(c(0x6e04), LedgerCode::Rejected);
    assert_eq!(c(0x6808), LedgerCode::BlindSigning);
    assert_eq!(c(0x6e00), LedgerCode::AppClosed);
    assert_eq!(c(0x6d00), LedgerCode::AppClosed);
    assert_eq!(c(0x6511), LedgerCode::AppClosed);
    let odd = LedgerError::from_status(0x6f42);
    assert_eq!(odd.code, LedgerCode::Unknown);
    assert!(odd.message.contains("0x6f42"));
}

#[test]
fn maps_rust_transport_errors_and_keeps_the_udev_fix() {
    let fix = "echo 'SUBSYSTEMS==\"usb\", ATTRS{idVendor}==\"2c97\", MODE=\"0660\", TAG+=\"uaccess\"' | pkexec tee /etc/udev/rules.d/20-ledger.rules";
    let udev = LedgerError::from_native(&crate::err_data("udev", "No access", serde_json::json!({ "fix": fix })));
    assert_eq!((udev.code, udev.fix()), (LedgerCode::Udev, Some(fix)));
    assert!(udev.message.contains("permission"));
    for (native, code) in [
        ("ble-off", LedgerCode::BleOff),
        ("ble-pairing", LedgerCode::BlePairing),
        ("busy", LedgerCode::Busy),
        ("notfound", LedgerCode::NoDevice),
        ("disconnected", LedgerCode::Disconnected),
        ("timeout", LedgerCode::Timeout),
        ("cancelled", LedgerCode::Cancelled),
        ("weird", LedgerCode::Unknown),
    ] {
        assert_eq!(LedgerError::from_native(&crate::err(native, "x")).code, code);
    }
}

#[test]
fn plain_messages_without_em_dashes() {
    for c in LedgerCode::ALL {
        let m = LedgerError::new(c).message;
        assert!(m.len() > 10);
        assert!(!m.contains('\u{2014}'));
    }
    assert!(LedgerCode::BlindSigning.message().contains("Settings and turn on Blind signing"));
}

// ---------------------------------------------------------------- sessions (api.test.ts)

struct Port(Rc<RefCell<FakeLedger>>, Option<LedgerError>);
struct Conn(Rc<RefCell<FakeLedger>>);
impl Apdu for Conn {
    fn exchange(&mut self, apdu: &[u8]) -> R<Vec<u8>> {
        self.0.borrow_mut().exchange(apdu)
    }
}
impl LedgerPort for Port {
    type Conn = Conn;
    fn open(&mut self, _id: &str) -> R<Conn> {
        match &self.1 {
            Some(e) => Err(e.clone()),
            None => Ok(Conn(self.0.clone())),
        }
    }
}

#[test]
fn session_waits_on_open_the_sui_app_and_carries_on() {
    let dev = Rc::new(RefCell::new(FakeLedger::new().app("BOLOS")));
    let mut port = Port(dev.clone(), None);
    let mut stages = vec![];
    let d2 = dev.clone();
    let kp = device_key(TEST_PHRASE, &sui_path(0));
    let r = session(
        &mut port,
        "usb:mock",
        Duration::from_secs(5),
        Duration::from_millis(1),
        &mut |s| {
            stages.push(s);
            d2.borrow_mut().app = "Sui".into(); // the person opens the app
        },
        &|| false,
        |c: &mut Conn, version: Option<String>| {
            assert_eq!(version.as_deref(), Some("1.4.0"));
            sign_personal_message(c, &sui_path(0), &kp.public_key(), b"hi")
        },
    );
    assert!(r.is_ok());
    assert_eq!(stages, vec![Stage::OpenApp, Stage::Connecting]);
    assert_eq!(Stage::OpenApp.text(), "Open the Sui app on your Ledger");
}

#[test]
fn session_gives_up_reports_other_errors_and_cancels() {
    let dev = Rc::new(RefCell::new(FakeLedger::new().locked()));
    let mut port = Port(dev.clone(), None);
    let mut n = 0;
    let e = session(&mut port, "x", Duration::from_millis(0), Duration::from_millis(1), &mut |_| n += 1, &|| false, |_: &mut Conn, _: Option<String>| Ok(())).unwrap_err();
    assert_eq!(e.code, LedgerCode::Locked);
    let fix = serde_json::json!({ "fix": "pkexec sh -c \"...\"" });
    let mut port = Port(dev.clone(), Some(LedgerError::from_native(&crate::err_data("udev", "no access", fix))));
    let e = session(&mut port, "x", Duration::from_secs(5), Duration::from_millis(1), &mut |_| {}, &|| false, |_: &mut Conn, _: Option<String>| Ok(())).unwrap_err();
    assert_eq!((e.code, e.fix()), (LedgerCode::Udev, Some("pkexec sh -c \"...\"")));
    let mut port = Port(dev, None);
    let e = session(&mut port, "x", Duration::from_secs(5), Duration::from_millis(1), &mut |_| {}, &|| true, |_: &mut Conn, _: Option<String>| Ok(())).unwrap_err();
    assert_eq!(e.code, LedgerCode::Cancelled);
}

// ---------------------------------------------------------------- byte cross-check with TS

#[test]
fn ts_apdus_and_signatures_are_byte_identical() {
    let v: Value = serde_json::from_str(include_str!("../../testdata/ts-vectors.json")).unwrap();
    let l = &v["ledger"];
    let s = |x: &Value| x.as_str().unwrap().to_string();
    let bytes = B64.decode(s(&v["sponsor"]["gameBytes"])).unwrap();
    let message = B64.decode(s(&v["message"])).unwrap();
    let path = s(&l["path"]);
    let mut t = FakeLedger::default();
    let info = check_sui_app(&mut t).unwrap().unwrap();
    assert_eq!(serde_json::json!({ "name": info.0, "version": info.1 }), l["appInfo"]);
    let (pk, address) = read_account(&mut t, &path, true).unwrap();
    assert_eq!((B64.encode(&pk), address), (s(&l["publicKey"]), s(&l["address"])));
    assert_eq!(serde_json::json!(clear_signing_object_ids(&bytes)), l["objectIds"]);
    let objects: Vec<Vec<u8>> = l["objects"].as_array().unwrap().iter().map(|o| B64.decode(o.as_str().unwrap()).unwrap()).collect();
    assert_eq!(sign_transaction(&mut t, &path, &pk, &bytes, &objects).unwrap(), s(&l["txSignature"]));
    assert_eq!(sign_personal_message(&mut t, &path, &pk, &message).unwrap(), s(&l["messageSignature"]));
    let want: Vec<String> = l["apdus"].as_array().unwrap().iter().map(s).collect();
    assert_eq!(t.apdus, want);
    // the phrase key signs the same message the same way
    assert_eq!(Keypair::from_mnemonic(TEST_PHRASE, 0).unwrap().sign_personal_message(&message), s(&l["messageSignature"]));
}
