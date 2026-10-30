//! Kiosk reads over gRPC: the kiosks a wallet owns (plain, personal, OriginByte), what sits in
//! each one (locked, listed, exclusive), which marketplace holds a listing (Drip, Tradeport), and
//! dynamic fields by name. Ports the read half of `src/wallet/tx/nft/context.ts`
//! (`ownedKiosks`, `loadKioskContext`, `listingMarket`); building the sends is the wallet's.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use futures::future::join_all;
use serde::{Deserialize, Serialize};
use sui_sdk_types::TypeTag;

use crate::grpc::{ChainRead, Include, ObjectInfo};
use crate::ids::{address, canon_addr, KIOSK_OWNER_CAP_TYPE};
use crate::{Error, Result};

// ---------- on-chain ids (src/wallet/tx/nft/ids.ts) ----------

/// Drip: a plain kiosk listing plus a ListingRecord in the registry's `listings` bag.
pub const DRIP_MARKETPLACE_PACKAGE: &str = "0xe50387176587c707c428c537b7b8fb530c3eaeadd16a45b1d975369f372dbc0a";
pub const DRIP_LISTING_REGISTRY: &str = "0x6c2f62f0ca9b2d2ae09b6fc53defff6e35a66b8c5412d00b8d3ff435a5ab7693";
/// ListingRegistry.listings (a Bag): dynamic fields ID -> ListingRecord.
pub const DRIP_LISTINGS_BAG: &str = "0x6b3e13b4fc2fe1cdea8dfb6458f7b38f66323f4568dc54c7648b4f5e32cf6408";
/// Tradeport: an exclusive kiosk listing whose PurchaseCap sits in a SimpleListing (a dynamic
/// object field of the Store keyed by the NFT id); orderbook listings also have an entry in the
/// orderbook Store keyed by the SimpleListing id.
pub const TRADEPORT_LISTINGS_PACKAGE: &str = "0x81e16ff388eacc2908f4e8248f54d140eb6d7834ff01dc7a86042a3b701b68f6";
pub const TRADEPORT_STORE: &str = "0xf96f9363ac5a64c058bf7140723226804d74c0dab2dd27516fb441a180cd763b";
pub const TRADEPORT_ORDERBOOK_STORE: &str = "0x3af0a94360253c80fbabe73f6832be0a49ddfc0b8772ccd5335f568cbc356da1";
/// Mysten kiosk extensions; every call goes to v4.
pub const KIOSK_RULES_ORIGINAL_PACKAGE: &str = "0x434b5bd8f6a7b05fede0ff46c6e511d71ea326ed38056e3bcd681d2d7c2a7879";
pub const KIOSK_EXT_PACKAGE: &str = "0xdfb4f1d4e43e0c3ad834dcd369f0d39005c872e118c9dc1c5da9765bb93ee5f3";
pub const KIOSK_EXT_VERSIONS: [&str; 4] = [
    KIOSK_RULES_ORIGINAL_PACKAGE,
    "0x0cb4bcc0560340eb1a1b929cabe56b33fc6449820ec8c1980d69bb98b649b802",
    "0x34cc6762780f4f6f153c924c0680cfe2a1fb4601e7d33cc28a92297b62de1e0e",
    KIOSK_EXT_PACKAGE,
];
/// PersonalKioskCap as owned-object queries report it (type origin is the v2 package).
pub const PERSONAL_KIOSK_CAP_TYPE: &str = "0x0cb4bcc0560340eb1a1b929cabe56b33fc6449820ec8c1980d69bb98b649b802::personal_kiosk::PersonalKioskCap";
/// OriginByte kiosks: the wallet holds an OwnerToken that names the kiosk.
pub const OB_OWNER_TOKEN_TYPE: &str = "0x95a441d389b07437d00dd07e0b6f05f513d7659b13fd7c5d3923c7d9d847199b::ob_kiosk::OwnerToken";
pub const KIOSK_TYPE: &str = "0x2::kiosk::Kiosk";

/// A kiosk the wallet owns.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KioskRef {
    pub kiosk_id: String,
    /// KioskOwnerCap id. For personal kiosks the cap is wrapped inside the PersonalKioskCap.
    pub owner_cap_id: String,
    /// PersonalKioskCap id when the kiosk is personal.
    pub personal_cap_id: Option<String>,
    pub is_personal: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KioskSlot {
    pub kiosk: KioskRef,
    pub is_locked: bool,
    pub is_listed: bool,
    pub is_exclusive_listing: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KioskContext {
    /// Every item in a kiosk this wallet can open, by object id.
    pub items: HashMap<String, KioskSlot>,
    /// Items in OriginByte kiosks.
    pub ob_items: HashSet<String>,
    /// Some kiosk could not be listed.
    pub partial: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OwnedKiosks {
    pub kiosks: Vec<KioskRef>,
    /// OriginByte kiosk ids.
    pub ob: Vec<String>,
}

/// Drip: kiosk listing + registry record. Tradeport: exclusive listing, cap held by Tradeport.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Market {
    Drip,
    Tradeport { orderbook: bool },
}

fn hex_at(b: &[u8], at: usize) -> Option<String> {
    b.get(at..at + 32).map(|s| canon_addr(&hex::encode(s)))
}

async fn owned_all(c: &dyn ChainRead, owner: &str, t: &str) -> Result<Vec<ObjectInfo>> {
    c.owned_objects(owner, Some(t), 1000, Include::CONTENTS).await
}

/// KioskOwnerCap { id: UID, for: ID } -> a plain kiosk.
pub fn kiosk_from_cap(cap_id: &str, contents: &[u8]) -> Option<KioskRef> {
    Some(KioskRef { kiosk_id: hex_at(contents, 32)?, owner_cap_id: canon_addr(cap_id), personal_cap_id: None, is_personal: false })
}

/// PersonalKioskCap { id: UID, cap: Option<KioskOwnerCap> } -> a personal kiosk (when the cap is in).
pub fn kiosk_from_personal(personal_id: &str, b: &[u8]) -> Option<KioskRef> {
    if b.len() < 32 + 1 + 64 || b[32] != 1 {
        return None;
    }
    Some(KioskRef { kiosk_id: hex_at(b, 33 + 32)?, owner_cap_id: hex_at(b, 33)?, personal_cap_id: Some(canon_addr(personal_id)), is_personal: true })
}

/// The kiosks a wallet owns: plain ones (KioskOwnerCap), personal ones (PersonalKioskCap), and
/// OriginByte kiosk ids (ob_kiosk::OwnerToken { id, kiosk: ID, owner }).
pub async fn owned_kiosks(c: &dyn ChainRead, owner: &str) -> Result<OwnedKiosks> {
    let (caps, personal, tokens) = futures::join!(owned_all(c, owner, KIOSK_OWNER_CAP_TYPE), owned_all(c, owner, PERSONAL_KIOSK_CAP_TYPE), owned_all(c, owner, OB_OWNER_TOKEN_TYPE));
    let mut kiosks: Vec<KioskRef> = caps?.iter().filter_map(|o| kiosk_from_cap(&o.object_id, o.contents.as_deref()?)).collect();
    kiosks.extend(personal.unwrap_or_default().iter().filter_map(|o| kiosk_from_personal(&o.object_id, o.contents.as_deref()?)));
    let ob = tokens.unwrap_or_default().iter().filter_map(|o| hex_at(o.contents.as_deref()?, 32)).collect();
    Ok(OwnedKiosks { kiosks, ob })
}

/// `0x2::kiosk::Item` / `Listing` / `Lock`, or None for any other field name type.
pub fn kiosk_key(t: &str) -> Option<&'static str> {
    let t = t.trim();
    let (addr, rest) = t.split_once("::kiosk::")?;
    if !addr.starts_with("0x") || canon_addr(addr) != canon_addr("0x2") {
        return None;
    }
    match rest {
        "Item" => Some("Item"),
        "Listing" => Some("Listing"),
        "Lock" => Some("Lock"),
        _ => None,
    }
}

/// Every item in the wallet's kiosks with its lock and listing state.
pub async fn load_kiosk_context(c: &dyn ChainRead, owner: &str) -> Result<KioskContext> {
    let OwnedKiosks { kiosks, ob } = owned_kiosks(c, owner).await?;
    let mut ctx = KioskContext::default();
    let plain = join_all(kiosks.iter().map(|k| async move { (k, c.dynamic_fields(&k.kiosk_id, 2000).await) })).await;
    for (kiosk, fields) in plain {
        let Ok(fields) = fields else {
            ctx.partial = true;
            continue;
        };
        let mut locked = HashSet::new();
        let mut listed: HashMap<String, bool> = HashMap::new();
        let mut ids = vec![];
        for f in &fields {
            let Some(key) = kiosk_key(&f.name_type) else { continue };
            let Some(id) = hex_at(&f.name_bcs, 0) else { continue };
            match key {
                "Item" => ids.push(id),
                "Lock" => {
                    locked.insert(id);
                }
                // Listing { id: ID, is_exclusive: bool }
                _ => {
                    listed.insert(id, f.name_bcs.get(32) == Some(&1));
                }
            }
        }
        for id in ids {
            let slot = KioskSlot { kiosk: kiosk.clone(), is_locked: locked.contains(&id), is_listed: listed.contains_key(&id), is_exclusive_listing: listed.get(&id) == Some(&true) };
            ctx.items.insert(id, slot);
        }
    }
    let obs = join_all(ob.iter().map(|k| c.dynamic_fields(k, 2000))).await;
    for fields in obs {
        match fields {
            Ok(fields) => ctx.ob_items.extend(fields.iter().filter(|f| kiosk_key(&f.name_type) == Some("Item")).filter_map(|f| hex_at(&f.name_bcs, 0))),
            Err(_) => ctx.partial = true,
        }
    }
    Ok(ctx)
}

/// The id of the dynamic field `name` (of Move type `name_type`, BCS `name_bcs`) under `parent`.
pub fn dynamic_field_id(parent: &str, name_type: &str, name_bcs: &[u8]) -> Result<String> {
    let t = TypeTag::from_str(name_type).map_err(|_| Error::invalid(format!("not a Move type: {name_type}")))?;
    Ok(address(parent)?.derive_dynamic_child_id(&t, name_bcs).to_string())
}

/// A dynamic field's value BCS (`getDynamicField`): the Field object's contents after its UID and
/// the name.
pub async fn dynamic_field_value(c: &dyn ChainRead, parent: &str, name_type: &str, name_bcs: &[u8]) -> Result<Vec<u8>> {
    let id = dynamic_field_id(parent, name_type, name_bcs)?;
    let o = c.object(&id, Include::CONTENTS).await?;
    let b = o.contents.ok_or_else(|| Error::decode("no field contents"))?;
    b.get(32 + name_bcs.len()..).map(<[u8]>::to_vec).ok_or_else(|| Error::decode("short field contents"))
}

/// The object a dynamic object field points at (`getDynamicObjectField`).
pub async fn dynamic_object_field(c: &dyn ChainRead, parent: &str, name_type: &str, name_bcs: &[u8]) -> Result<String> {
    let wrapped = format!("0x2::dynamic_object_field::Wrapper<{name_type}>");
    let v = dynamic_field_value(c, parent, &wrapped, name_bcs).await?;
    hex_at(&v, 0).ok_or_else(|| Error::decode("short object field"))
}

fn id_key(id: &str) -> Result<Vec<u8>> {
    Ok(address(id)?.into_inner().to_vec())
}

/// Which marketplace holds a listed kiosk item. Exclusive listings: Tradeport when its Store has a
/// SimpleListing for the NFT (orderbook when the orderbook Store also has that listing), else
/// [`ListingMarket::Unknown`]. Plain listings: Drip when its registry has a record, else a bare
/// kiosk listing.
pub async fn listing_market(c: &dyn ChainRead, nft_id: &str, exclusive: bool) -> Result<ListingMarket> {
    let key = id_key(nft_id)?;
    if exclusive {
        let Ok(listing) = dynamic_object_field(c, TRADEPORT_STORE, "0x2::object::ID", &key).await else { return Ok(ListingMarket::Unknown) };
        let ob = dynamic_field_value(c, TRADEPORT_ORDERBOOK_STORE, "0x2::object::ID", &id_key(&listing)?).await.is_ok();
        return Ok(ListingMarket::At(Market::Tradeport { orderbook: ob }));
    }
    Ok(match dynamic_field_value(c, DRIP_LISTINGS_BAG, "0x2::object::ID", &key).await {
        Ok(_) => ListingMarket::At(Market::Drip),
        Err(_) => ListingMarket::PlainKiosk,
    })
}

/// Where a kiosk listing lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListingMarket {
    /// A marketplace that has to cancel it.
    At(Market),
    /// A bare `kiosk::list`.
    PlainKiosk,
    /// An exclusive listing no known marketplace holds: it can't be delisted from here.
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::{DynField, FieldKind};
    use crate::BoxFut;

    fn id(n: u32) -> String {
        canon_addr(&format!("0x{n:x}"))
    }
    fn raw(n: u32) -> Vec<u8> {
        address(&id(n)).unwrap().into_inner().to_vec()
    }
    fn field(name_type: &str, name_bcs: Vec<u8>) -> DynField {
        DynField { kind: FieldKind::Field, field_id: id(1), child_id: None, name_type: name_type.into(), name_bcs, value_type: String::new() }
    }

    struct Chain {
        owned: HashMap<String, Vec<ObjectInfo>>,
        fields: HashMap<String, Vec<DynField>>,
        objects: HashMap<String, Vec<u8>>,
    }
    impl ChainRead for Chain {
        fn owned_objects(&self, _o: &str, t: Option<&str>, _m: usize, _i: Include) -> BoxFut<'_, Vec<ObjectInfo>> {
            let v = self.owned.get(t.unwrap_or("")).cloned().unwrap_or_default();
            Box::pin(async move { Ok(v) })
        }
        fn dynamic_fields(&self, parent: &str, _m: usize) -> BoxFut<'_, Vec<DynField>> {
            let v = self.fields.get(parent).cloned().ok_or_else(|| Error::invalid("no kiosk"));
            Box::pin(async move { v })
        }
        fn object(&self, id: &str, _i: Include) -> BoxFut<'_, ObjectInfo> {
            let v = self.objects.get(id).cloned().map(|c| ObjectInfo { object_id: id.into(), contents: Some(c), ..Default::default() }).ok_or_else(|| Error::Grpc { code: crate::error::GRPC_NOT_FOUND, message: "not found".into() });
            Box::pin(async move { v })
        }
    }

    fn obj(oid: u32, contents: Vec<u8>) -> ObjectInfo {
        ObjectInfo { object_id: id(oid), contents: Some(contents), ..Default::default() }
    }

    #[tokio::test]
    async fn kiosks_items_locks_and_listings() {
        // KioskOwnerCap { id, for } ; PersonalKioskCap { id, cap: Some(KioskOwnerCap { id, for }) } ; OwnerToken { id, kiosk, owner }
        let cap = [raw(0xa001), raw(0xa000)].concat();
        let personal = [raw(0xb002), vec![1], raw(0xb001), raw(0xb000)].concat();
        let token = [raw(0xc001), raw(0xc000), raw(0x1)].concat();
        let mut owned = HashMap::new();
        owned.insert(KIOSK_OWNER_CAP_TYPE.to_string(), vec![obj(0xa001, cap)]);
        owned.insert(PERSONAL_KIOSK_CAP_TYPE.to_string(), vec![obj(0xb002, personal)]);
        owned.insert(OB_OWNER_TOKEN_TYPE.to_string(), vec![obj(0xc001, token)]);
        let mut fields = HashMap::new();
        fields.insert(
            id(0xa000),
            vec![
                field("0x2::kiosk::Item", raw(1)),
                field("0x2::kiosk::Item", raw(2)),
                field("0x2::kiosk::Lock", raw(2)),
                field("0x2::kiosk::Listing", [raw(1), vec![1]].concat()),
                field("0x2::kiosk::Item", raw(3)),
                field("0x2::kiosk::Listing", [raw(3), vec![0]].concat()),
                field("0x9::kiosk::Item", raw(4)),
            ],
        );
        fields.insert(id(0xc000), vec![field("0x2::kiosk::Item", raw(9))]);
        // the personal kiosk can't be listed: partial
        let c = Chain { owned, fields, objects: HashMap::new() };

        let k = owned_kiosks(&c, "0x1").await.unwrap();
        assert_eq!(k.kiosks[0], KioskRef { kiosk_id: id(0xa000), owner_cap_id: id(0xa001), personal_cap_id: None, is_personal: false });
        assert_eq!(k.kiosks[1], KioskRef { kiosk_id: id(0xb000), owner_cap_id: id(0xb001), personal_cap_id: Some(id(0xb002)), is_personal: true });
        assert_eq!(k.ob, vec![id(0xc000)]);

        let ctx = load_kiosk_context(&c, "0x1").await.unwrap();
        assert!(ctx.partial);
        assert_eq!(ctx.items.len(), 3);
        let s1 = &ctx.items[&id(1)];
        assert!(s1.is_listed && s1.is_exclusive_listing && !s1.is_locked);
        let s2 = &ctx.items[&id(2)];
        assert!(s2.is_locked && !s2.is_listed);
        let s3 = &ctx.items[&id(3)];
        assert!(s3.is_listed && !s3.is_exclusive_listing);
        assert!(!ctx.items.contains_key(&id(4)));
        assert!(ctx.ob_items.contains(&id(9)));
    }

    #[test]
    fn personal_cap_without_its_cap_is_skipped() {
        assert_eq!(kiosk_from_personal(&id(1), &[raw(1), vec![0]].concat()), None);
        assert_eq!(kiosk_key("0x0000000000000000000000000000000000000000000000000000000000000002::kiosk::Lock"), Some("Lock"));
        assert_eq!(kiosk_key("0x2::kiosk::Kiosk"), None);
    }

    #[tokio::test]
    async fn listing_markets() {
        let nft = id(0x55);
        let listing = id(0x66);
        let tp = dynamic_field_id(TRADEPORT_STORE, "0x2::dynamic_object_field::Wrapper<0x2::object::ID>", &raw(0x55)).unwrap();
        let ob = dynamic_field_id(TRADEPORT_ORDERBOOK_STORE, "0x2::object::ID", &raw(0x66)).unwrap();
        let drip = dynamic_field_id(DRIP_LISTINGS_BAG, "0x2::object::ID", &raw(0x55)).unwrap();
        // Field<Wrapper<ID>, ID> = UID + name + value
        let mut objects = HashMap::new();
        objects.insert(tp, [raw(0x77), raw(0x55), raw(0x66)].concat());
        let c = Chain { owned: HashMap::new(), fields: HashMap::new(), objects: objects.clone() };
        assert_eq!(listing_market(&c, &nft, true).await.unwrap(), ListingMarket::At(Market::Tradeport { orderbook: false }));
        assert_eq!(dynamic_object_field(&c, TRADEPORT_STORE, "0x2::object::ID", &raw(0x55)).await.unwrap(), listing);
        objects.insert(ob, [raw(0x78), raw(0x66), vec![0; 8]].concat());
        objects.insert(drip, [raw(0x79), raw(0x55), vec![1, 2]].concat());
        let c = Chain { owned: HashMap::new(), fields: HashMap::new(), objects };
        assert_eq!(listing_market(&c, &nft, true).await.unwrap(), ListingMarket::At(Market::Tradeport { orderbook: true }));
        assert_eq!(listing_market(&c, &nft, false).await.unwrap(), ListingMarket::At(Market::Drip));
        assert_eq!(listing_market(&c, &id(0x56), false).await.unwrap(), ListingMarket::PlainKiosk);
        assert_eq!(listing_market(&c, &id(0x56), true).await.unwrap(), ListingMarket::Unknown);
        assert_eq!(dynamic_field_value(&c, DRIP_LISTINGS_BAG, "0x2::object::ID", &raw(0x55)).await.unwrap(), vec![1, 2]);
    }
}
