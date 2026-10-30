//! Parity with the web launcher's resolver: every answer in golden/media.json (written by
//! golden/media.test.ts from the TypeScript) must come out the same here.

use super::*;
use serde_json::json;

fn ref_json(r: &Ref) -> Value {
    match r {
        Ref::Http { url } => json!({"kind": "http", "url": url}),
        Ref::Ipfs { path, gateway } => json!({"kind": "ipfs", "path": path, "gateway": gateway}),
        Ref::Arweave { id, url } => json!({"kind": "arweave", "id": id, "url": url}),
        Ref::Walrus { path, original } => json!({"kind": "walrus", "path": path, "original": original}),
        Ref::Data { uri, mime } => json!({"kind": "data", "uri": uri, "mime": mime}),
        Ref::Invalid(reason) => json!({"kind": "invalid", "reason": reason}),
    }
}

#[test]
fn resolver_matches_the_typescript() {
    let g: Value = serde_json::from_str(include_str!("../../golden/media.json")).unwrap();
    let mut bad = Vec::new();
    for case in g["refs"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        let r = parse_ref(input);
        let got = json!({
            "input": input,
            "ref": ref_json(&r),
            "key": ref_key(&r),
            "candidates": candidates(&r),
            "usable": usable_ref(Some(input)),
            "inline": inline_image(input),
            "chain400": image_chain(Some(input), 400.0, Fit::Cover),
            "chainInside": image_chain(Some(input), 1000.6, Fit::Inside),
            "wsrvBlocked": wsrv_blocked(input),
        });
        if got != *case {
            bad.push(format!("{input:?}\n  rust {got}\n  ts   {case}"));
        }
    }
    for case in g["json"].as_array().unwrap() {
        let got = pick_image_from_json(case["input"].as_object().unwrap());
        if json!(got) != case["image"] {
            bad.push(format!("pickImageFromJson {}: rust {got:?}, ts {}", case["input"], case["image"]));
        }
    }
    assert!(bad.is_empty(), "{} differ:\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn uri_decoding_like_js() {
    assert_eq!(decode_uri("a%20b%2Fc").as_deref(), Some("a b%2Fc"));
    assert_eq!(decode_uri_component("a%20b%2Fc").as_deref(), Some("a b/c"));
    assert_eq!(decode_uri_component("%E2%9C%93").as_deref(), Some("✓"));
    assert!(decode_uri_component("%E2%9C").is_none());
    assert!(decode_uri_component("%zz").is_none());
    assert_eq!(size_for(300.0), 400);
    assert_eq!(size_for(9000.0), 2560);
}
