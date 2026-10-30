//! Rare Store licence check, launcher side: does an owner hold a licence for a game? Ports
//! `src/sui/licence.ts`.
//!
//! The contract (rare-store sources/licence.move) has two types: `licence::Licence` (soulbound,
//! key only, owned by the buyer) and `licence::ResaleLicence` (key + store, after
//! `make_tradable`; usually in a kiosk, listed or not, and it still counts until sold). Both carry
//! `game: ID` and `edition`. Until the package is published every check answers
//! [`LicenceResult::Unavailable`], so nothing can mistake "not deployed" for "not owned".

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::grpc::{ChainRead, Include};
use crate::ids::{canon_addr, canon_outer};
use crate::json::{id_text, num};
use crate::{BoxFut, Result};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenceFields {
    pub id: String,
    /// The store's `Game` object id.
    pub game: String,
    pub slug: String,
    pub edition: f64,
    pub purchased_at_ms: f64,
    /// A tradable ResaleLicence (else the soulbound Licence).
    pub resale: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Where {
    Wallet,
    Kiosk,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum LicenceResult {
    Unavailable,
    None,
    Owned {
        licence: LicenceFields,
        #[serde(rename = "where")]
        at: Where,
        #[serde(rename = "kioskId", skip_serializing_if = "Option::is_none")]
        kiosk_id: Option<String>,
    },
}

/// An owned object's id and JSON.
#[derive(Clone, Debug, PartialEq)]
pub struct Owned {
    pub object_id: String,
    pub json: Option<Value>,
}

/// One kiosk item: its object id and type.
#[derive(Clone, Debug, PartialEq)]
pub struct KioskField {
    pub object_id: String,
    pub object_type: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Obj {
    pub object_id: String,
    pub object_type: String,
    pub json: Option<Value>,
}

/// The reads the check needs; fakes in tests, [`ChainLicenceReader`] in the app.
pub trait LicenceReader: Send + Sync {
    fn owned(&self, owner: &str, object_type: &str) -> BoxFut<'_, Vec<Owned>>;
    fn kiosk_fields(&self, kiosk_id: &str) -> BoxFut<'_, Vec<KioskField>>;
    fn objects(&self, ids: Vec<String>) -> BoxFut<'_, Vec<Obj>>;
}

pub struct Types {
    pub licence: String,
    pub resale: String,
}

pub fn licence_types(pkg: &str) -> Types {
    let p = canon_addr(pkg);
    Types { licence: format!("{p}::licence::Licence"), resale: format!("{p}::licence::ResaleLicence") }
}

fn to_licence(id: &str, j: Option<&Value>, resale: bool) -> Option<LicenceFields> {
    let j = j?;
    let game = id_text(j.get("game"))?;
    Some(LicenceFields {
        id: id.to_string(),
        game: canon_addr(&game),
        slug: j.get("slug").and_then(Value::as_str).unwrap_or("").to_string(),
        edition: num(j.get("edition")),
        purchased_at_ms: num(j.get("purchased_at_ms")),
        resale,
    })
}

/// Does `owner` hold a licence for `game_id`? `pkg` None means the store is not published.
pub async fn check_licence(owner: &str, game_id: &str, pkg: Option<&str>, read: &dyn LicenceReader) -> Result<LicenceResult> {
    let Some(pkg) = pkg else { return Ok(LicenceResult::Unavailable) };
    let want = canon_addr(game_id);
    let t = licence_types(pkg);
    // 1. Held in the wallet: the soulbound Licence, or a ResaleLicence taken out of a kiosk.
    for (ty, resale) in [(&t.licence, false), (&t.resale, true)] {
        for o in read.owned(owner, ty).await? {
            if let Some(l) = to_licence(&o.object_id, o.json.as_ref(), resale) {
                if l.game == want {
                    return Ok(LicenceResult::Owned { licence: l, at: Where::Wallet, kiosk_id: None });
                }
            }
        }
    }
    // 2. A ResaleLicence inside a kiosk the owner controls (KioskOwnerCap.for -> the kiosk's items).
    for cap in read.owned(owner, "0x2::kiosk::KioskOwnerCap").await? {
        let Some(kiosk_id) = id_text(cap.json.as_ref().and_then(|j| j.get("for"))) else { continue };
        let fields: Vec<String> = read.kiosk_fields(&kiosk_id).await?.into_iter().filter(|f| f.object_type == t.resale).map(|f| f.object_id).collect();
        if fields.is_empty() {
            continue;
        }
        for o in read.objects(fields).await? {
            let l = if o.object_type == t.resale { to_licence(&o.object_id, o.json.as_ref(), true) } else { None };
            if let Some(l) = l {
                if l.game == want {
                    return Ok(LicenceResult::Owned { licence: l, at: Where::Kiosk, kiosk_id: Some(kiosk_id) });
                }
            }
        }
    }
    Ok(LicenceResult::None)
}

/// [`LicenceReader`] over the gRPC reads (`grpcReader` in licence.ts).
pub struct ChainLicenceReader<'a>(pub &'a dyn ChainRead);

impl LicenceReader for ChainLicenceReader<'_> {
    fn owned(&self, owner: &str, object_type: &str) -> BoxFut<'_, Vec<Owned>> {
        let (owner, ty) = (owner.to_string(), object_type.to_string());
        Box::pin(async move {
            let v = self.0.owned_objects(&owner, Some(&ty), 50, Include::JSON).await?;
            Ok(v.into_iter().map(|o| Owned { object_id: o.object_id, json: o.json }).collect())
        })
    }

    fn kiosk_fields(&self, kiosk_id: &str) -> BoxFut<'_, Vec<KioskField>> {
        let kiosk_id = kiosk_id.to_string();
        Box::pin(async move {
            let v = self.0.dynamic_fields(&kiosk_id, 100).await?;
            // Kiosk items are dynamic object fields keyed by kiosk::Item { id }; the value is the object itself.
            Ok(v.into_iter()
                .filter(|f| f.name_type.contains("::kiosk::Item"))
                .map(|f| KioskField { object_id: f.child_id.unwrap_or(f.field_id), object_type: canon_outer(&f.value_type) })
                .collect())
        })
    }

    fn objects(&self, ids: Vec<String>) -> BoxFut<'_, Vec<Obj>> {
        Box::pin(async move {
            let v = self.0.objects(&ids, Include::JSON).await?;
            Ok(v.into_iter().flatten().map(|o| Obj { object_type: canon_outer(&o.object_type), object_id: o.object_id, json: o.json }).collect())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PKG: &str = "0xabc";
    const GAME: &str = "0x0000000000000000000000000000000000000000000000000000000000000077";

    type OwnedFn = Box<dyn Fn(&str) -> Vec<Owned> + Send + Sync>;
    struct Fake {
        owned: OwnedFn,
        fields: Vec<KioskField>,
        objects: Vec<Obj>,
    }
    fn fake(owned: impl Fn(&str) -> Vec<Owned> + Send + Sync + 'static) -> Fake {
        Fake { owned: Box::new(owned), fields: vec![], objects: vec![] }
    }
    impl LicenceReader for Fake {
        fn owned(&self, _o: &str, t: &str) -> BoxFut<'_, Vec<Owned>> {
            let v = (self.owned)(t);
            Box::pin(async move { Ok(v) })
        }
        fn kiosk_fields(&self, _k: &str) -> BoxFut<'_, Vec<KioskField>> {
            let v = self.fields.clone();
            Box::pin(async move { Ok(v) })
        }
        fn objects(&self, _ids: Vec<String>) -> BoxFut<'_, Vec<Obj>> {
            let v = self.objects.clone();
            Box::pin(async move { Ok(v) })
        }
    }
    fn owned(id: &str, j: Value) -> Vec<Owned> {
        vec![Owned { object_id: id.into(), json: Some(j) }]
    }

    #[test]
    fn reads_the_real_types_not_the_old_license_license() {
        let t = licence_types(PKG);
        assert_eq!(t.licence, "0x0000000000000000000000000000000000000000000000000000000000000abc::licence::Licence");
        assert_eq!(t.resale, "0x0000000000000000000000000000000000000000000000000000000000000abc::licence::ResaleLicence");
    }

    #[tokio::test]
    async fn says_unavailable_while_the_package_isnt_published() {
        assert_eq!(check_licence("0x1", GAME, None, &fake(|_| vec![])).await.unwrap(), LicenceResult::Unavailable);
        // and the shipped config has no package yet
        assert_eq!(check_licence("0x1", GAME, crate::network::RARE_STORE_PACKAGE, &fake(|_| vec![])).await.unwrap(), LicenceResult::Unavailable);
    }

    #[tokio::test]
    async fn finds_the_soulbound_licence_in_the_wallet() {
        let l = licence_types(PKG).licence;
        let r = check_licence("0x1", "0x77", Some(PKG), &fake(move |t| if t == l { owned("0xl1", json!({ "game": GAME, "slug": "act", "edition": 1, "purchased_at_ms": "5" })) } else { vec![] })).await.unwrap();
        assert_eq!(
            r,
            LicenceResult::Owned { at: Where::Wallet, kiosk_id: None, licence: LicenceFields { id: "0xl1".into(), game: GAME.into(), slug: "act".into(), edition: 1.0, purchased_at_ms: 5.0, resale: false } }
        );
    }

    #[tokio::test]
    async fn finds_a_resale_licence_held_in_the_wallet() {
        let rt = licence_types(PKG).resale;
        let r = check_licence("0x1", GAME, Some(PKG), &fake(move |t| if t == rt { owned("0xr1", json!({ "game": GAME, "edition": 2 })) } else { vec![] })).await.unwrap();
        match r {
            LicenceResult::Owned { at: Where::Wallet, licence, .. } => {
                assert_eq!(licence.id, "0xr1");
                assert_eq!(licence.edition, 2.0);
                assert!(licence.resale);
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn finds_a_resale_licence_inside_the_owners_kiosk() {
        let rt = licence_types(PKG).resale;
        let mut f = fake(|t| if t.contains("KioskOwnerCap") { owned("0xcap", json!({ "for": "0xk" })) } else { vec![] });
        f.fields = vec![KioskField { object_id: "0xl2".into(), object_type: rt.clone() }];
        f.objects = vec![Obj { object_id: "0xl2".into(), object_type: rt, json: Some(json!({ "game": GAME })) }];
        match check_licence("0x1", GAME, Some(PKG), &f).await.unwrap() {
            LicenceResult::Owned { at: Where::Kiosk, kiosk_id, licence } => {
                assert_eq!(kiosk_id.as_deref(), Some("0xk"));
                assert!(licence.resale);
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_licence_for_another_game_doesnt_count() {
        let l = licence_types(PKG).licence;
        let r = check_licence("0x1", GAME, Some(PKG), &fake(move |t| if t == l { owned("0xl1", json!({ "game": "0x78" })) } else { vec![] })).await.unwrap();
        assert_eq!(r, LicenceResult::None);
    }

    #[tokio::test]
    async fn the_old_field_name_game_id_is_not_read() {
        let l = licence_types(PKG).licence;
        let r = check_licence("0x1", GAME, Some(PKG), &fake(move |t| if t == l { owned("0xl1", json!({ "game_id": GAME })) } else { vec![] })).await.unwrap();
        assert_eq!(r, LicenceResult::None);
    }

    #[test]
    fn result_serializes_like_the_ts() {
        let r = LicenceResult::Owned { at: Where::Kiosk, kiosk_id: Some("0xk".into()), licence: LicenceFields { id: "0x1".into(), game: "0x2".into(), slug: String::new(), edition: 0.0, purchased_at_ms: 0.0, resale: true } };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["status"], "owned");
        assert_eq!(v["where"], "kiosk");
        assert_eq!(v["kioskId"], "0xk");
        assert_eq!(serde_json::to_value(LicenceResult::Unavailable).unwrap(), json!({ "status": "unavailable" }));
    }
}
