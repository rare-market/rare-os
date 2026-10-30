//! Byte-for-byte parity with the TS NFT send flow: testdata/nft-vectors.json is written by
//! testdata/nft-vectors.test.ts running the TS builders over the same inputs and fake chain states
//! these tests feed the Rust port. A change on either side that breaks parity fails a test.

use std::collections::HashMap;
use std::sync::Mutex;

use base64::Engine;
use rare_sui::grpc::{ChainRead, DynField, FieldKind, Include, ObjectInfo, Owner};
use rare_sui::ids::{canon_addr, canon_type};
use rare_sui::kiosk::dynamic_field_id;
use rare_sui::prices::HttpGet;
use rare_sui::{BoxFut, Error};
use serde_json::Value;

use super::context::{analyze_nfts, policy_ids_from_trade, read_policies, AnalyzeDeps, NftRef, PolicyLoader, PolicySource, Sendability, TxObjectTypes};
use super::limits::{fits, measure, plan_batches, type_tag_bytes, Limits, SUI_LIMITS};
use super::policy::{match_rule, pick_policy, policy_needs, policy_problem, unsupported_reason};
use super::transfer::{build_nft_send, build_send_nfts, plan_nft_sends, NftSendParams};
use super::{KioskRef, NftSendError, PolicyRule, SendItem, TransferPolicyInfo};

pub(super) fn vectors() -> &'static Value {
    static V: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    V.get_or_init(|| serde_json::from_str(include_str!("../../testdata/nft-vectors.json")).unwrap())
}

fn b64(s: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(s).unwrap()
}

fn de<T: serde::de::DeserializeOwned>(v: &Value) -> T {
    serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{e}: {v}"))
}

fn not_found() -> Error {
    Error::Grpc { code: rare_sui::error::GRPC_NOT_FOUND, message: "not found".into() }
}

pub(super) fn id(n: u64) -> String {
    canon_addr(&format!("0x{n:x}"))
}

// ---------- the generated selections (nft-vectors.test.ts `gen`) ----------

pub(super) const SENDER: &str = "0xa1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
pub(super) const TO: &str = "0xb2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";
pub(super) const NFT_T: &str = "0xc3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3::punks::Punk";

pub(super) fn plain_kiosk() -> KioskRef {
    KioskRef { kiosk_id: id(0xa000), owner_cap_id: id(0xa001), personal_cap_id: None, is_personal: false }
}

pub(super) fn royalty_lock() -> TransferPolicyInfo {
    let rule = |m: &str| PolicyRule { type_: m.into(), rule_type: Some(format!("{}::{m}::Rule", super::ids::KIOSK_EXT_PACKAGE)), config: None };
    TransferPolicyInfo { id: id(0xf001), is_shared: Some(true), rules: vec![rule("royalty_rule"), rule("kiosk_lock_rule")] }
}

pub(super) fn wallet(n: u64) -> SendItem {
    SendItem { from: None, item: super::KioskItemRef { nft_id: id(n), nft_type: NFT_T.into(), ..Default::default() }, policies: None }
}

pub(super) fn unlocked(n: u64) -> SendItem {
    SendItem { from: Some(plain_kiosk()), ..wallet(n) }
}

pub(super) fn locked(n: u64) -> SendItem {
    let mut s = unlocked(n);
    s.item.is_locked = true;
    s.policies = Some(vec![royalty_lock()]);
    s
}

fn gen(g: &Value) -> Vec<SendItem> {
    let count = g["count"].as_u64().unwrap();
    let f = match g["kind"].as_str().unwrap() {
        "wallet" => wallet,
        "locked" => locked,
        _ => unlocked,
    };
    (1..=count).map(f).collect()
}

fn items_of(c: &Value) -> Vec<SendItem> {
    if c.get("gen").is_some() {
        gen(&c["gen"])
    } else {
        de(&c["items"])
    }
}

// ---------- the fake chain (nft-vectors.test.ts `reader`) ----------

pub(super) struct JsonChain {
    owned: Vec<(String, ObjectInfo)>,
    fields: HashMap<String, Option<Vec<DynField>>>,
    /// objects by id: the state's objects plus a Field object per dynamic (object) field
    objects: HashMap<String, ObjectInfo>,
    tx_types: HashMap<String, Vec<(String, String)>>,
    store: HashMap<String, bool>,
}

impl JsonChain {
    pub(super) fn new(s: &Value) -> Self {
        let mut owned = Vec::new();
        for (t, list) in s["owned"].as_object().unwrap() {
            for o in list.as_array().unwrap() {
                let oi = ObjectInfo { object_id: canon_addr(o["objectId"].as_str().unwrap()), object_type: t.clone(), contents: Some(b64(o["content"].as_str().unwrap())), ..Default::default() };
                owned.push((canon_type(t), oi));
            }
        }
        let mut fields = HashMap::new();
        for (parent, f) in s["fields"].as_object().unwrap() {
            let v = f.as_array().map(|list| {
                list.iter()
                    .map(|x| DynField { kind: FieldKind::Field, field_id: id(1), child_id: None, name_type: x["nameType"].as_str().unwrap().into(), name_bcs: b64(x["nameBcs"].as_str().unwrap()), value_type: x["valueType"].as_str().unwrap().into() })
                    .collect()
            });
            fields.insert(canon_addr(parent), v);
        }
        let mut objects = HashMap::new();
        let field_obj = |fid: String, name: Vec<u8>, value: Vec<u8>| ObjectInfo { object_id: fid.clone(), contents: Some([vec![0u8; 32], name, value].concat()), ..Default::default() };
        for d in s["dof"].as_array().unwrap() {
            let key = rare_sui::ptb::addr_bytes(d["key"].as_str().unwrap()).unwrap().to_vec();
            let fid = dynamic_field_id(d["parent"].as_str().unwrap(), "0x2::dynamic_object_field::Wrapper<0x2::object::ID>", &key).unwrap();
            let child = rare_sui::ptb::addr_bytes(d["objectId"].as_str().unwrap()).unwrap().to_vec();
            objects.insert(fid.clone(), field_obj(fid, key, child));
        }
        for d in s["df"].as_array().unwrap() {
            let name = b64(d["nameBcs"].as_str().unwrap());
            let fid = dynamic_field_id(d["parent"].as_str().unwrap(), d["nameType"].as_str().unwrap(), &name).unwrap();
            objects.insert(fid.clone(), field_obj(fid, name, b64(d["value"].as_str().unwrap())));
        }
        for (oid, o) in s["objects"].as_object().unwrap() {
            let owner = o.get("owner").and_then(Value::as_str).map(|k| match k {
                "Shared" => Owner::Shared,
                "ConsensusAddressOwner" => Owner::ConsensusAddress { address: id(1) },
                "AddressOwner" => Owner::Address { address: id(1) },
                _ => Owner::Unknown,
            });
            let oi = ObjectInfo {
                object_id: canon_addr(oid),
                owner,
                contents: o.get("content").and_then(Value::as_str).map(b64),
                previous_transaction: o.get("previousTransaction").and_then(Value::as_str).map(str::to_string),
                ..Default::default()
            };
            objects.insert(canon_addr(oid), oi);
        }
        let tx_types = s["txTypes"].as_object().unwrap().iter().map(|(d, l)| (d.clone(), l.as_array().unwrap().iter().map(|p| (p[0].as_str().unwrap().to_string(), p[1].as_str().unwrap().to_string())).collect())).collect();
        let store = s["store"].as_object().unwrap().iter().map(|(t, b)| (canon_type(t), b.as_bool().unwrap())).collect();
        JsonChain { owned, fields, objects, tx_types, store }
    }
}

impl ChainRead for JsonChain {
    fn owned_objects(&self, _owner: &str, t: Option<&str>, _max: usize, _i: Include) -> BoxFut<'_, Vec<ObjectInfo>> {
        let t = canon_type(t.unwrap_or(""));
        let v = self.owned.iter().filter(|(k, _)| *k == t).map(|(_, o)| o.clone()).collect();
        Box::pin(async move { Ok(v) })
    }
    fn dynamic_fields(&self, parent: &str, _max: usize) -> BoxFut<'_, Vec<DynField>> {
        let v = match self.fields.get(&canon_addr(parent)) {
            Some(None) => Err(Error::invalid("unavailable")),
            Some(Some(f)) => Ok(f.clone()),
            None => Ok(vec![]),
        };
        Box::pin(async move { v })
    }
    fn object(&self, oid: &str, _i: Include) -> BoxFut<'_, ObjectInfo> {
        let v = self.objects.get(&canon_addr(oid)).cloned().ok_or_else(not_found);
        Box::pin(async move { v })
    }
    fn objects(&self, ids: &[String], _i: Include) -> BoxFut<'_, Vec<rare_sui::Result<ObjectInfo>>> {
        let v = ids.iter().map(|i| self.objects.get(&canon_addr(i)).cloned().ok_or_else(not_found)).collect();
        Box::pin(async move { Ok(v) })
    }
    fn datatype_abilities(&self, package: &str, module: &str, name: &str) -> BoxFut<'_, Vec<i32>> {
        let v = match self.store.get(&canon_type(&format!("{package}::{module}::{name}"))) {
            Some(true) => vec![3, 4],
            Some(false) => vec![4],
            None => vec![],
        };
        Box::pin(async move { Ok(v) })
    }
}

impl TxObjectTypes for JsonChain {
    fn tx_object_types(&self, digest: &str) -> BoxFut<'_, Vec<(String, String)>> {
        let v = self.tx_types.get(digest).cloned().ok_or_else(|| Error::invalid("pruned"));
        Box::pin(async move { v })
    }
}

/// Policies by type, as the vectors give them ("error" fails the lookup).
struct MapPolicies(HashMap<String, Value>);

impl PolicySource for MapPolicies {
    fn policies<'a>(&'a self, nft_type: &'a str, _nft_id: &'a str) -> BoxFut<'a, Vec<TransferPolicyInfo>> {
        let v = match self.0.get(nft_type) {
            Some(Value::Array(_)) => Ok(de(&self.0[nft_type])),
            _ => Err(Error::invalid("lookup failed")),
        };
        Box::pin(async move { v })
    }
}

/// the Trade API answers as a vector case says, recording the URLs asked for.
pub(super) struct FakeTrade {
    answer: Value,
    pub seen: Mutex<Vec<String>>,
}

impl FakeTrade {
    pub(super) fn new(answer: Value) -> Self {
        FakeTrade { answer, seen: Mutex::new(vec![]) }
    }
}

impl HttpGet for FakeTrade {
    fn get_json(&self, url: &str) -> BoxFut<'_, Value> {
        self.seen.lock().unwrap().push(url.to_string());
        let r = match &self.answer {
            Value::String(_) => Err(Error::http(None, "offline")),
            a if a["status"] == 200 => Ok(a["body"].clone()),
            a => Err(Error::http(a["status"].as_u64().map(|s| s as u16), "error")),
        };
        Box::pin(async move { r })
    }
}

// ---------- the cases ----------

#[test]
fn builders_print_the_same_transactions() {
    let v = vectors();
    let cases = v["build"].as_array().unwrap();
    assert!(cases.len() >= 25);
    for c in cases {
        let name = c["name"].as_str().unwrap();
        let r = build_send_nfts(SENDER, c["to"].as_str().unwrap(), &items_of(c));
        match (r, c.get("error")) {
            (Ok(tx), None) => {
                assert_eq!(tx.to_json(), c["tx"], "{name}: getData()");
                let size = measure(&tx);
                assert_eq!(serde_json::to_value(size).unwrap(), c["size"], "{name}: size");
                assert_eq!(fits(&size, &SUI_LIMITS), c["fits"].as_bool().unwrap(), "{name}");
            }
            (Err(e), Some(want)) => assert_eq!(e.message(), want.as_str().unwrap(), "{name}"),
            (r, want) => panic!("{name}: got {:?}, TS {want:?}", r.map(|t| t.to_json())),
        }
    }
}

#[test]
fn the_screen_entry_point_matches_build_nft_send() {
    for c in vectors()["wrap"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let p = NftSendParams { sender: c["sender"].as_str().unwrap().into(), to: c["to"].as_str().unwrap().into(), items: de(&c["items"]) };
        match (build_nft_send(&p), c.get("error")) {
            (Ok(tx), None) => assert_eq!(tx.to_json(), c["tx"], "{name}"),
            (Err(e), Some(want)) => {
                assert_eq!(e.message(), want.as_str().unwrap(), "{name}");
                let code = match &e {
                    NftSendError::Input(s) => Value::String(
                        match s.code {
                            super::SendErrorCode::BadAddress => "bad-address",
                            super::SendErrorCode::SameAddress => "same-address",
                            super::SendErrorCode::BadAmount => "bad-amount",
                            super::SendErrorCode::NoBalance => "no-balance",
                            super::SendErrorCode::FeeReserve => "fee-reserve",
                        }
                        .into(),
                    ),
                    NftSendError::Build(_) => Value::Null,
                };
                assert_eq!(code, c["code"], "{name}");
            }
            (r, want) => panic!("{name}: got {:?}, TS {want:?}", r.map(|t| t.to_json())),
        }
    }
}

#[test]
fn plans_the_same_batches() {
    let v = vectors();
    for c in v["plan"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let limits: Limits = c.get("limits").map(de).unwrap_or(SUI_LIMITS);
        match (plan_nft_sends(SENDER, TO, &items_of(c), &limits), c.get("error")) {
            (Ok(batches), None) => {
                let want = c["batches"].as_array().unwrap();
                assert_eq!(batches.len(), want.len(), "{name}: batch count");
                for (b, w) in batches.iter().zip(want) {
                    let ids: Vec<&str> = b.iter().map(|s| s.item.nft_id.as_str()).collect();
                    let ids = if b.len() > 20 { vec![ids[0], ids[ids.len() - 1]] } else { ids };
                    assert_eq!(serde_json::to_value(&ids).unwrap(), w["ids"], "{name}");
                    assert_eq!(b.len() as u64, w["count"].as_u64().unwrap(), "{name}");
                    assert_eq!(serde_json::to_value(measure(&build_send_nfts(SENDER, TO, b).unwrap())).unwrap(), w["size"], "{name}");
                }
            }
            (Err(e), Some(want)) => assert_eq!(e.message(), want.as_str().unwrap(), "{name}"),
            (r, want) => panic!("{name}: got {:?} batches, TS {want:?}", r.map(|b| b.len())),
        }
    }
    // planBatches on its own: the 7-object limit
    let items: Vec<u64> = (1..=50).collect();
    let limits = Limits { objects: 7, ..SUI_LIMITS };
    let got: Vec<usize> = plan_batches(&items, |s| build_send_nfts(SENDER, TO, &s.iter().map(|n| wallet(*n)).collect::<Vec<_>>()), &limits).unwrap().iter().map(Vec::len).collect();
    assert_eq!(serde_json::to_value(got).unwrap(), v["planBatches"]);
}

#[test]
fn type_sizes_match() {
    for c in vectors()["typeTagBytes"].as_array().unwrap() {
        assert_eq!(type_tag_bytes(c[0].as_str().unwrap()) as u64, c[1].as_u64().unwrap(), "{}", c[0]);
    }
}

#[test]
fn policy_decisions_match() {
    let v = vectors();
    for c in v["policySets"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let set: Option<Vec<TransferPolicyInfo>> = de(&c["policies"]);
        let set = set.as_deref();
        assert_eq!(serde_json::to_value(pick_policy(set).map(|p| p.id.clone())).unwrap(), c["pick"], "{name}: pick");
        assert_eq!(serde_json::to_value(policy_problem(set)).unwrap(), c["problem"], "{name}: problem");
        for (p, want) in set.unwrap_or_default().iter().zip(c["needs"].as_array().unwrap()) {
            let n = policy_needs(Some(p));
            let got = serde_json::json!({
                "lock": n.lock, "personal": n.personal, "royalty": n.royalty,
                "floor": n.floor.map(|f| f.to_string()),
                "rules": n.rules.iter().map(|r| serde_json::json!({ "id": r.spec.id, "pkg": r.pkg, "callPkg": r.call_pkg })).collect::<Vec<_>>(),
                "unsupported": n.unsupported,
            });
            assert_eq!(&got, want, "{name}: needs of {}", p.id);
        }
    }
    for c in v["rules"].as_array().unwrap() {
        let r: PolicyRule = de(&c["rule"]);
        let m = match_rule(&r).map(|m| serde_json::json!({ "id": m.spec.id, "pkg": m.pkg, "callPkg": m.call_pkg }));
        assert_eq!(m.unwrap_or(Value::Null), c["match"], "{r:?}");
        assert_eq!(unsupported_reason(&r), c["reason"].as_str().unwrap(), "{r:?}");
    }
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().build().unwrap()
}

#[test]
fn sendability_matches() {
    let rt = rt();
    for c in vectors()["analyze"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let chain = JsonChain::new(&c["chain"]);
        let policies = MapPolicies(de(&c["policies"]));
        let nfts: Vec<NftRef> = de(&c["nfts"]);
        let got = rt.block_on(analyze_nfts(&AnalyzeDeps { chain: &chain, policies: &policies, check_store: c["hasStore"].as_bool().unwrap() }, SENDER, &nfts)).unwrap();
        let want: HashMap<String, Sendability> = de(&c["result"]);
        assert_eq!(got.len(), want.len(), "{name}");
        for (k, w) in &want {
            assert_eq!(got.get(k), Some(w), "{name}: {k}");
        }
    }
}

#[test]
fn policies_read_the_same() {
    let rt = rt();
    let r = &vectors()["readPolicies"];
    let chain = JsonChain::new(&r["chain"]);
    let ids: Vec<String> = de(&r["ids"]);
    let got = rt.block_on(read_policies(&chain, &ids)).unwrap();
    // compared as JSON: rule configs are the TS values (floor_price a decimal string)
    assert_eq!(serde_json::to_value(&got).unwrap(), r["result"]);
    assert_eq!(got.len(), 6);
}

#[test]
fn the_policy_loader_falls_back_the_same_way() {
    let rt = rt();
    let r = &vectors()["readPolicies"];
    let chain = JsonChain::new(&r["chain"]);
    for c in vectors()["loader"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let (base, nft_type, nft_id) = (c["base"].as_str().unwrap(), c["nftType"].as_str().unwrap(), c["nftId"].as_str().unwrap());
        // policyIdsFromTrade on its own
        let trade = FakeTrade::new(c["trade"].clone());
        match (rt.block_on(policy_ids_from_trade(base, nft_type, &trade)), c["tradeIds"].get("error")) {
            (Ok(ids), None) => assert_eq!(serde_json::to_value(ids).unwrap(), c["tradeIds"], "{name}"),
            (Err(e), Some(want)) => {
                let want = want.as_str().unwrap();
                if want.starts_with("Policy lookup failed") {
                    assert!(e.to_string().ends_with(want), "{name}: {e}");
                }
            }
            (r, want) => panic!("{name}: got {r:?}, TS {want:?}"),
        }
        // the whole loader, and the URL it asked the Trade API
        let trade = FakeTrade::new(c["trade"].clone());
        let loader = PolicyLoader { chain: &chain, history: &chain, trade_base: base.into(), http: &trade };
        let got = rt.block_on(loader.policies(nft_type, nft_id));
        match (got, c["result"].get("error")) {
            (Ok(p), None) => assert_eq!(serde_json::to_value(p).unwrap(), c["result"], "{name}"),
            (Err(_), Some(_)) => {}
            (r, want) => panic!("{name}: got {r:?}, TS {want:?}"),
        }
        assert_eq!(serde_json::to_value(&*trade.seen.lock().unwrap()).unwrap(), c["urls"], "{name}: urls");
    }
}
