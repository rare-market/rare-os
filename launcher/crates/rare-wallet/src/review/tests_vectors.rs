//! Cross-checks against testdata/review-vectors.json, written by the TS review itself
//! (testdata/review-vectors.test.ts): the same inputs must give the same rows, risks, insights,
//! fee lines and strings. Bigints are decimal strings; null fields of outputs are left out.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use super::*;
use crate::amount::{self, js_number};

fn vectors() -> Value {
    serde_json::from_str(include_str!("../../testdata/review-vectors.json")).expect("review-vectors.json")
}

fn strip_nulls(v: Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(m.into_iter().filter(|(_, v)| !v.is_null()).map(|(k, v)| (k, strip_nulls(v))).collect()),
        Value::Array(a) => Value::Array(a.into_iter().map(strip_nulls).collect()),
        v => v,
    }
}

fn js<T: Serialize>(v: &T) -> Value {
    strip_nulls(serde_json::to_value(v).expect("serialize"))
}

fn from<T: serde::de::DeserializeOwned>(v: &Value) -> T {
    serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{e}: {v}"))
}

fn opt_str(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

fn arr<'a>(v: &'a Value, k: &str) -> &'a Vec<Value> {
    v[k].as_array().unwrap_or_else(|| panic!("{k}"))
}

#[test]
fn cases_match_the_ts_review() {
    let v = vectors();
    let cases = arr(&v, "cases");
    assert!(cases.len() >= 25);
    for c in cases {
        let name = c["name"].as_str().unwrap_or_default();
        let sim: Simulation = from(&c["sim"]);
        let signer = c["signer"].as_str().unwrap_or_default();
        let calls: Vec<MoveCallRef> = if c["calls"].is_null() { vec![] } else { from(&c["calls"]) };
        let kiosks: Vec<String> = if c["kiosks"].is_null() { vec![] } else { from(&c["kiosks"]) };
        let ins = analyze(&sim, signer, &calls, &kiosks);

        let meta: BTreeMap<String, CoinMeta> = if c["meta"].is_null() { BTreeMap::new() } else { from(&c["meta"]) };
        let extras = DescribeExtras {
            display: sim.display.as_ref(),
            originals: sim.packages.as_ref(),
            covered_by_rare: c["extras"]["coveredByRare"].as_bool().unwrap_or(false),
            paid_by: opt_str(&c["extras"]["paidBy"]),
        };
        let rows = describe_review(&ins, CoinMetaLookup::Map(&meta), &extras);

        let r = &c["risk"];
        let decimals: BTreeMap<String, u32> = if r["decimals"].is_null() { BTreeMap::new() } else { from(&r["decimals"]) };
        let symbols: BTreeMap<String, String> = if r["symbols"].is_null() { BTreeMap::new() } else { from(&r["symbols"]) };
        let ctx = RiskContext {
            prices: if r["prices"].is_null() { BTreeMap::new() } else { from(&r["prices"]) },
            decimals: Box::new(|t: &str| decimals.get(t).copied().unwrap_or(9)),
            symbol: Box::new(|t: &str| symbols.get(t).cloned().unwrap_or_else(|| t.rsplit("::").next().unwrap_or_default().to_string())),
            portfolio_usd: r["portfolioUsd"].as_f64(),
            safety: if r["safety"].is_null() { BTreeMap::new() } else { from(&r["safety"]) },
            originals: if r["originals"].is_null() { None } else { Some(from(&r["originals"])) },
            addresses: if r["addresses"].is_null() { None } else { Some(from(&r["addresses"])) },
            self_address: opt_str(&r["self"]),
        };
        let risks = risk_checks(&ins, &ctx);
        let fees = split_fees(&ins.others_receive, &ins.send);

        let out = &c["out"];
        assert_eq!(js(&ins), out["insights"], "{name}: insights");
        assert_eq!(js(&rows), out["rows"], "{name}: rows");
        assert_eq!(js(&risks), out["risks"], "{name}: risks");
        assert_eq!(js(&fees), out["fees"], "{name}: fees");
        // the insights read back from the TS JSON are the same value
        let back: Insights = from(&out["insights"]);
        assert_eq!(js(&back), out["insights"], "{name}: insights round trip");
    }
}

#[test]
fn amounts_match_the_ts_formatting() {
    let v = vectors();
    for a in arr(&v, "amounts") {
        let base: i128 = a["base"].as_str().and_then(|s| s.parse().ok()).expect("base");
        let d = a["decimals"].as_u64().expect("decimals") as u32;
        let ctx = format!("{base} @ {d}");
        assert_eq!(amount::format_amount(base, d), a["formatAmount"], "{ctx}");
        assert_eq!(amount::format_units(base, d, 4), a["formatUnits"], "{ctx}");
        assert_eq!(amount::format_units(base, d, d as usize), a["formatUnitsFull"], "{ctx}");
        assert_eq!(amount::format_units(base, d, 2), a["formatUnits2"], "{ctx}");
        assert_eq!(amount::to_input(base, d), a["toInput"], "{ctx}");
        let want: f64 = a["toNumber"].as_str().and_then(|s| s.parse().ok()).expect("toNumber");
        assert_eq!(amount::to_number(base, d), want, "{ctx}");
    }
    for m in arr(&v, "money") {
        let n = js_number(m["n"].as_str().unwrap_or_default());
        assert_eq!(amount::usd(n), m["usd"], "usd {}", m["n"]);
        assert_eq!(amount::price(n), m["price"], "price {}", m["n"]);
    }
    for p in arr(&v, "parse") {
        let input = p["input"].as_str().unwrap_or_default();
        let d = p["decimals"].as_u64().unwrap_or(0) as u32;
        assert_eq!(amount::parse_units(input, d), opt_str(&p["parseUnits"]), "parseUnits {input:?}");
        assert_eq!(amount::clean_amount(input), opt_str(&p["cleanAmount"]), "cleanAmount {input:?}");
    }
    for u in arr(&v, "usdToBase") {
        let text = u["text"].as_str().unwrap_or_default();
        let price = u["price"].as_str().map(js_number);
        let d = u["decimals"].as_u64().unwrap_or(0) as u32;
        assert_eq!(amount::usd_to_base(text, price, d), opt_str(&u["out"]), "usdToBase {text:?} {price:?} {d}");
    }
    for b in arr(&v, "baseToUsd") {
        let base: i128 = b["base"].as_str().and_then(|s| s.parse().ok()).expect("base");
        let d = b["decimals"].as_u64().unwrap_or(0) as u32;
        let price = b["price"].as_str().map(js_number);
        let want: Option<f64> = b["out"].as_str().map(|s| s.parse().expect("f64"));
        assert_eq!(amount::base_to_usd(base, d, price), want, "baseToUsd {base} {d} {price:?}");
    }
    for c in arr(&v, "chips") {
        let available: i128 = c["available"].as_str().and_then(|s| s.parse().ok()).expect("available");
        let pct = c["pct"].as_u64().unwrap_or(0) as u32;
        let is_sui = c["isSui"].as_bool().unwrap_or(false);
        assert_eq!(amount::chip_amount(available, pct, is_sui).to_string(), c["out"], "chip {available} {pct} {is_sui}");
    }
}

#[test]
fn labels_ids_and_types_match() {
    let v = vectors();
    for l in arr(&v, "labels") {
        let t = l["type"].as_str();
        let originals: Option<BTreeMap<String, String>> = if l["originals"].is_null() { None } else { Some(from(&l["originals"])) };
        assert_eq!(js(&object_label(t, originals.as_ref())), l["label"], "objectLabel {t:?}");
        assert_eq!(js(&parse_type(t)), l["parsed"], "parseType {t:?}");
        assert_eq!(is_coin_object(t), l["isCoin"].as_bool().unwrap_or(false), "isCoinObject {t:?}");
        assert_eq!(t.map(coin_type_of), opt_str(&l["coinTypeOf"]), "coinTypeOf {t:?}");
    }
    for i in arr(&v, "ids") {
        let id = i["id"].as_str().unwrap_or_default();
        assert_eq!(short_id(id), i["shortId"], "shortId {id}");
        assert_eq!(known_package(id, None).map(str::to_string), opt_str(&i["known"]), "knownPackage {id}");
        assert_eq!(known_fee(id).map(|f| f.label.to_string()), opt_str(&i["fee"]), "knownFee {id}");
        assert_eq!(lookalike_short(id), i["lookalike"], "lookalikeShort {id}");
    }
    for t in arr(&v, "normTypes") {
        let s = t["t"].as_str().unwrap_or_default();
        assert_eq!(norm_type(s), t["norm"], "normType {s}");
    }
    for h in arr(&v, "humanize") {
        let s = h["s"].as_str().unwrap_or_default();
        assert_eq!(humanize(s), h["out"], "humanize {s}");
    }
    for c in arr(&v, "callLabels") {
        let call: MoveCallRef = from(&c["call"]);
        let originals: Option<BTreeMap<String, String>> = if c["originals"].is_null() { None } else { Some(from(&c["originals"])) };
        assert_eq!(js(&call_label(&call, originals.as_ref())), c["label"], "callLabel {call:?}");
    }
}

#[test]
fn failures_read_the_same() {
    let v = vectors();
    for f in arr(&v, "failures") {
        let failure: Option<SimFailure> = if f["failure"].is_null() { None } else { Some(from(&f["failure"])) };
        assert_eq!(friendly_failure(failure.as_ref()), f["friendly"], "{failure:?}");
    }
}

#[test]
fn address_checks_match() {
    let v = vectors();
    for a in arr(&v, "addressChecks") {
        let recipients: Vec<String> = from(&a["recipients"]);
        let history: Option<AddressHistory> = if a["history"].is_null() { None } else { Some(from(&a["history"])) };
        let got = address_checks(&recipients, history.as_ref(), a["self"].as_str());
        assert_eq!(js(&got), a["out"], "{recipients:?}");
    }
}
