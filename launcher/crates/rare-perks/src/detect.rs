//! Rareling ownership over Sui gRPC (launcher/src/perks/detect.ts): held in the wallet, or inside a
//! kiosk the owner controls (a KioskOwnerCap, or one wrapped in a PersonalKioskCap). Rarelings are
//! kiosk-locked by their TransferPolicy, so the kiosk read is the usual case.

use rare_market::types::canon_type;
use serde_json::Value;
use std::sync::Arc;

/// Mysten's kiosk extensions package that defines PersonalKioskCap.
pub const PERSONAL_KIOSK_CAP: &str = "0x0cb4bcc0560340eb1a1b929cabe56b33fc6449820ec8c1980d69bb98b649b802::personal_kiosk::PersonalKioskCap";
pub const KIOSK_OWNER_CAP: &str = "0x2::kiosk::KioskOwnerCap";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Where {
    Wallet,
    Kiosk,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RarelingResult {
    /// No Rareling type is set (not deployed, or Rarelings are off).
    Unavailable,
    None,
    Owned { count: usize, at: Where, ids: Vec<String> },
}

impl RarelingResult {
    pub fn owned(&self) -> bool {
        matches!(self, RarelingResult::Owned { .. })
    }
}

/// An owned object: its id and the node's JSON of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Owned {
    pub object_id: String,
    pub json: Option<Value>,
}

/// The reads the check needs; fakes in tests.
pub trait RarelingReader: Send + Sync {
    fn owned(&self, owner: &str, object_type: &str) -> Result<Vec<Owned>, String>;
    /// A kiosk's items: object id and type.
    fn kiosk_items(&self, kiosk_id: &str) -> Result<Vec<(String, String)>, String>;
}

/// The kiosk a cap opens: KioskOwnerCap.for, or PersonalKioskCap.cap (an Option) .for.
pub fn kiosk_of(json: Option<&Value>) -> Option<String> {
    let j = json?;
    if let Some(f) = j.get("for").and_then(|v| v.as_str()) {
        return Some(f.to_string());
    }
    let cap = j.get("cap")?;
    cap.get("for").or_else(|| cap.get("vec").and_then(|v| v.get(0)).and_then(|v| v.get("for"))).and_then(|v| v.as_str()).map(String::from)
}

pub fn check_rareling(owner: &str, rareling_type: Option<&str>, read: &dyn RarelingReader) -> Result<RarelingResult, String> {
    let Some(t) = rareling_type else { return Ok(RarelingResult::Unavailable) };
    let want = canon_type(t);
    // 1. In the wallet.
    let held = read.owned(owner, &want)?;
    if !held.is_empty() {
        return Ok(RarelingResult::Owned { count: held.len(), at: Where::Wallet, ids: held.into_iter().map(|o| o.object_id).collect() });
    }
    // 2. In the owner's kiosks.
    let mut caps = read.owned(owner, KIOSK_OWNER_CAP)?;
    caps.extend(read.owned(owner, PERSONAL_KIOSK_CAP).unwrap_or_default());
    let mut kiosks: Vec<String> = Vec::new();
    for k in caps.iter().filter_map(|c| kiosk_of(c.json.as_ref())) {
        if !kiosks.contains(&k) {
            kiosks.push(k);
        }
    }
    // Every kiosk (a few at most for a person), so the badge can say how many.
    let mut ids = Vec::new();
    for k in kiosks.iter().take(20) {
        for (id, ty) in read.kiosk_items(k).unwrap_or_default() {
            if canon_type(&ty) == want {
                ids.push(id);
            }
        }
    }
    Ok(if ids.is_empty() { RarelingResult::None } else { RarelingResult::Owned { count: ids.len(), at: Where::Kiosk, ids } })
}

/// The reads over `rare-sui`'s gRPC client, waiting on its runtime (never call from inside it).
pub struct ChainReader {
    pub chain: Arc<dyn rare_sui::grpc::ChainRead>,
    pub rt: tokio::runtime::Handle,
}

impl RarelingReader for ChainReader {
    fn owned(&self, owner: &str, object_type: &str) -> Result<Vec<Owned>, String> {
        let include = rare_sui::grpc::Include { json: true, ..Default::default() };
        let objs = self.rt.block_on(self.chain.owned_objects(owner, Some(object_type), 50, include)).map_err(|e| e.to_string())?;
        Ok(objs.into_iter().map(|o| Owned { object_id: o.object_id, json: o.json }).collect())
    }
    fn kiosk_items(&self, kiosk_id: &str) -> Result<Vec<(String, String)>, String> {
        let fields = self.rt.block_on(self.chain.dynamic_fields(kiosk_id, 200)).map_err(|e| e.to_string())?;
        // Items are dynamic object fields keyed by kiosk::Item { id }; the value type is the item's.
        Ok(fields.into_iter().filter(|f| f.name_type.contains("::kiosk::Item")).map(|f| (f.child_id.unwrap_or(f.field_id), f.value_type)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    const PKG: &str = "0x7777777777777777777777777777777777777777777777777777777777777777";
    const OWNER: &str = "0x1111111111111111111111111111111111111111111111111111111111111111";

    fn ty() -> String {
        format!("{PKG}::rareling::Rareling")
    }

    #[derive(Default)]
    struct Fake {
        wallet: Vec<String>,
        caps: Vec<Value>,
        personal: Vec<Value>,
        kiosks: HashMap<String, Vec<(String, String)>>,
    }
    impl RarelingReader for Fake {
        fn owned(&self, _: &str, t: &str) -> Result<Vec<Owned>, String> {
            Ok(if t == canon_type(&ty()) {
                self.wallet.iter().map(|id| Owned { object_id: id.clone(), json: None }).collect()
            } else if t == KIOSK_OWNER_CAP {
                self.caps.iter().enumerate().map(|(i, j)| Owned { object_id: format!("cap{i}"), json: Some(j.clone()) }).collect()
            } else if t == PERSONAL_KIOSK_CAP {
                self.personal.iter().enumerate().map(|(i, j)| Owned { object_id: format!("pcap{i}"), json: Some(j.clone()) }).collect()
            } else {
                vec![]
            })
        }
        fn kiosk_items(&self, k: &str) -> Result<Vec<(String, String)>, String> {
            Ok(self.kiosks.get(k).cloned().unwrap_or_default())
        }
    }

    #[test]
    fn unavailable_until_the_type_is_set() {
        assert_eq!(check_rareling(OWNER, None, &Fake::default()).unwrap(), RarelingResult::Unavailable);
    }

    #[test]
    fn finds_one_in_the_wallet() {
        let f = Fake { wallet: vec!["0xr1".into(), "0xr2".into()], ..Default::default() };
        assert_eq!(check_rareling(OWNER, Some(&ty()), &f).unwrap(), RarelingResult::Owned { count: 2, at: Where::Wallet, ids: vec!["0xr1".into(), "0xr2".into()] });
    }

    #[test]
    fn finds_one_locked_in_a_kiosk() {
        let mut kiosks = HashMap::new();
        kiosks.insert("0xk1".to_string(), vec![("0xother".to_string(), "0x9::x::Y".to_string())]);
        kiosks.insert("0xk2".to_string(), vec![("0xr9".to_string(), ty())]);
        let f = Fake { caps: vec![json!({"for": "0xk1"}), json!({"for": "0xk2"})], kiosks, ..Default::default() };
        assert_eq!(check_rareling(OWNER, Some(&ty()), &f).unwrap(), RarelingResult::Owned { count: 1, at: Where::Kiosk, ids: vec!["0xr9".into()] });
    }

    #[test]
    fn reads_a_personal_kiosks_wrapped_cap() {
        let mut kiosks = HashMap::new();
        kiosks.insert("0xpk".to_string(), vec![("0xr3".to_string(), ty())]);
        let f = Fake { personal: vec![json!({"cap": {"vec": [{"for": "0xpk"}]}})], kiosks, ..Default::default() };
        assert!(check_rareling(OWNER, Some(&ty()), &f).unwrap().owned());
    }

    #[test]
    fn a_look_alike_from_another_package_does_not_count() {
        let mut kiosks = HashMap::new();
        kiosks.insert("0xk".to_string(), vec![("0xfake".to_string(), format!("0x{}::rareling::Rareling", "8".repeat(64)))]);
        let f = Fake { caps: vec![json!({"for": "0xk"})], kiosks, ..Default::default() };
        assert_eq!(check_rareling(OWNER, Some(&ty()), &f).unwrap(), RarelingResult::None);
    }

    #[test]
    fn kiosk_of_handles_both_cap_shapes() {
        assert_eq!(kiosk_of(Some(&json!({"for": "0xa"}))).as_deref(), Some("0xa"));
        assert_eq!(kiosk_of(Some(&json!({"cap": {"for": "0xb"}}))).as_deref(), Some("0xb"));
        assert_eq!(kiosk_of(Some(&json!({"cap": null}))), None);
        assert_eq!(kiosk_of(None), None);
    }
}
