//! Wallet NFTs. Ports `src/wallet/data/nfts.ts`.
//!
//! The grid comes from rare.market (names, images, collections, listings); the detail adds the
//! chain's view over gRPC: type, owner, and whether the type has `store` (without it the object
//! is bound to this account and can't be sent).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::grpc::{has_store, ChainRead, Include, ObjectInfo, Owner};
use crate::ids::canon_addr;
use crate::market::{get_as, CollectionRef, ItemCard, Listing, MarketApi, Page, Rarity};
use crate::{Error, Result};

/// rare.market GET /v1/items/<id>: the card plus detail.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RareItem {
    #[serde(flatten)]
    pub card: ItemCard,
}

impl RareItem {
    fn str(&self, k: &str) -> Option<String> {
        self.card.extra.get(k).and_then(Value::as_str).map(str::to_string)
    }
    pub fn description(&self) -> Option<String> {
        self.str("description")
    }
    pub fn attributes(&self) -> Vec<Value> {
        self.card.extra.get("attributes").and_then(Value::as_array).cloned().unwrap_or_default()
    }
}

/// Who holds it on chain, as the detail screen explains it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChainOwner {
    Address { address: String },
    /// Inside another object: a kiosk (dynamic field) or wrapped.
    Object { id: String },
    Shared,
    Immutable,
    Unknown,
}

fn map_owner(o: Option<&Owner>) -> ChainOwner {
    match o {
        Some(Owner::Address { address }) | Some(Owner::ConsensusAddress { address }) => ChainOwner::Address { address: canon_addr(address) },
        Some(Owner::Object { id }) => ChainOwner::Object { id: id.clone() },
        Some(Owner::Shared) => ChainOwner::Shared,
        Some(Owner::Immutable) => ChainOwner::Immutable,
        _ => ChainOwner::Unknown,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NftDetail {
    pub id: String,
    pub name: String,
    pub image_url: Option<String>,
    pub description: String,
    pub collection: Option<CollectionRef>,
    pub attributes: Vec<Value>,
    pub rarity: Option<Rarity>,
    pub listing: Option<Listing>,
    pub kiosk_id: Option<String>,
    /// Full Move type from the chain (rare.market's when the chain read failed).
    #[serde(rename = "type")]
    pub nft_type: Option<String>,
    pub owner: ChainOwner,
    pub in_kiosk: bool,
    /// None when the node did not say.
    pub has_store: Option<bool>,
    pub can_send: bool,
    /// Plain reason when can_send is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cant_send: Option<String>,
    /// rare.market had no data for it (shown from chain display only).
    pub rare_missing: bool,
}

/// The wallet's NFTs for the grid, from rare.market.
pub async fn wallet_nfts(api: &dyn MarketApi, address: &str, limit: Option<u32>) -> Result<Page<ItemCard>> {
    get_as(api, &format!("/v1/accounts/{}/items?limit={}", canon_addr(address), limit.unwrap_or(60))).await
}

fn is_kiosk_item_field(t: &str) -> bool {
    // `...::kiosk::Item` followed by a non-word character or the end
    let mut rest = t;
    while let Some(i) = rest.find("::kiosk::Item") {
        let after = &rest[i + "::kiosk::Item".len()..];
        if !after.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
            return true;
        }
        rest = after;
    }
    false
}

/// rare.market item data plus the chain's view. `me` is the viewing account: an item it does not
/// hold directly or in a kiosk can't be sent from here. Errors only when both sources fail.
pub async fn nft_detail(api: &dyn MarketApi, chain: &dyn ChainRead, id: &str, me: Option<&str>) -> Result<NftDetail> {
    let object_id = canon_addr(id);
    let path = format!("/v1/items/{object_id}");
    let (rare, chain_obj) = futures::join!(get_as::<RareItem>(api, &path), chain.object(&object_id, Include::DISPLAY));
    let (rare, obj): (Option<RareItem>, Option<ObjectInfo>) = (rare.ok(), chain_obj.ok());
    if rare.is_none() && obj.is_none() {
        return Err(Error::decode("Could not load this item"));
    }
    let nft_type = obj.as_ref().map(|o| o.object_type.clone()).or_else(|| rare.as_ref().map(|r| r.card.item_type.clone())).filter(|t| !t.is_empty());
    let owner = obj.as_ref().map(|o| map_owner(o.owner.as_ref())).unwrap_or(ChainOwner::Unknown);
    let disp = |k: &str| obj.as_ref().and_then(|o| o.display_str(k)).map(str::to_string);
    // a kiosk item is owned by a dynamic field (Field<Wrapper<kiosk::Item>, ID>) owned by the kiosk
    let mut kiosk_id = rare.as_ref().and_then(|r| r.card.kiosk_id.clone());
    let mut in_kiosk = false;
    if let ChainOwner::Object { id: field_id } = &owner {
        let field = chain.object(field_id, Include::DISPLAY).await.ok();
        let parent = field.as_ref().map(|f| map_owner(f.owner.as_ref()));
        match (&field, parent) {
            (Some(f), Some(ChainOwner::Object { id: parent })) if is_kiosk_item_field(&f.object_type) => {
                in_kiosk = true;
                kiosk_id = Some(canon_addr(&parent));
            }
            _ => in_kiosk = field.is_none() && kiosk_id.is_some(),
        }
    }
    let store = match &nft_type {
        Some(t) => has_store(chain, t).await.ok().flatten(),
        None => None,
    };

    let cant_send = if obj.is_none() {
        Some("Could not read it on chain. Try again in a moment")
    } else {
        match &owner {
            ChainOwner::Shared | ChainOwner::Immutable => Some("It is shared on chain and has no owner to send it"),
            ChainOwner::Address { address } if me.is_some_and(|m| *address != canon_addr(m)) => Some("It is not in this wallet"),
            ChainOwner::Object { .. } if !in_kiosk => Some("It is held inside another object"),
            ChainOwner::Address { .. } if store == Some(false) => Some("Can't send this one. It is bound to this account by its creator"),
            _ => None,
        }
    };

    let r = rare.as_ref();
    Ok(NftDetail {
        id: object_id,
        name: r.map(|r| r.card.name.clone()).filter(|n| !n.is_empty()).or_else(|| disp("name")).unwrap_or_else(|| "Untitled".into()),
        image_url: r.and_then(|r| r.card.image_url.clone()).or_else(|| disp("image_url")).or_else(|| disp("img_url")).or_else(|| disp("image")),
        description: r.and_then(RareItem::description).or_else(|| disp("description")).unwrap_or_default(),
        collection: r.and_then(|r| r.card.collection.clone()),
        attributes: r.map(RareItem::attributes).unwrap_or_default(),
        rarity: r.and_then(|r| r.card.rarity.clone()),
        listing: r.and_then(|r| r.card.listing.clone()),
        kiosk_id,
        nft_type,
        owner,
        in_kiosk,
        has_store: store,
        can_send: cant_send.is_none(),
        cant_send: cant_send.map(str::to_string),
        rare_missing: rare.is_none(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BoxFut;
    use serde_json::json;
    use std::sync::Mutex;

    fn me() -> String {
        format!("0x{}", "a1".repeat(32))
    }
    fn bob() -> String {
        format!("0x{}", "b2".repeat(32))
    }
    fn nft() -> String {
        format!("0x{}", "22".repeat(32))
    }
    fn nft_t() -> String {
        format!("0x{}::popkins::Popkins", "e5".repeat(32))
    }

    struct Api(Option<Value>, Mutex<Vec<String>>);
    impl MarketApi for Api {
        fn get(&self, p: &str) -> BoxFut<'_, Value> {
            self.1.lock().unwrap().push(p.to_string());
            let v = self.0.clone();
            Box::pin(async move { v.ok_or_else(|| Error::http(Some(404), "404")) })
        }
    }
    fn api(v: Option<Value>) -> Api {
        Api(v, Mutex::new(vec![]))
    }
    fn popkin() -> Value {
        json!({ "id": nft(), "name": "Popkins #1", "type": nft_t(), "imageUrl": "https://img/1.png", "collection": { "slug": "popkins", "name": "Popkins", "imageUrl": null, "verified": true }, "kioskId": null })
    }

    type ObjFn = Box<dyn Fn(&str) -> ObjectInfo + Send + Sync>;
    struct Chain {
        obj: ObjFn,
        abilities: Vec<i32>,
    }
    impl ChainRead for Chain {
        fn object(&self, id: &str, _i: Include) -> BoxFut<'_, ObjectInfo> {
            let o = (self.obj)(id);
            Box::pin(async move { Ok(o) })
        }
        fn datatype_abilities(&self, _p: &str, _m: &str, _n: &str) -> BoxFut<'_, Vec<i32>> {
            let a = self.abilities.clone();
            Box::pin(async move { Ok(a) })
        }
    }
    fn obj(id: &str, t: &str, owner: Owner) -> ObjectInfo {
        ObjectInfo { object_id: id.into(), object_type: t.into(), owner: Some(owner), ..Default::default() }
    }
    fn owned_by(a: String) -> Chain {
        Chain { obj: Box::new(move |id| obj(id, &nft_t(), Owner::Address { address: a.clone() })), abilities: vec![3, 4] }
    }

    // data.test.ts "nfts"
    #[tokio::test]
    async fn wallet_nfts_reads_the_accounts_items_from_rare_market() {
        let a = api(Some(json!({ "data": [], "nextCursor": null })));
        wallet_nfts(&a, &me(), None).await.unwrap();
        assert_eq!(a.1.lock().unwrap()[0], format!("/v1/accounts/{}/items?limit=60", me()));
    }

    #[tokio::test]
    async fn an_owned_nft_with_store_can_be_sent() {
        let d = nft_detail(&api(Some(popkin())), &owned_by(me()), &nft(), Some(&me())).await.unwrap();
        assert_eq!(d.name, "Popkins #1");
        assert_eq!(d.nft_type.as_deref(), Some(nft_t().as_str()));
        assert_eq!(d.owner, ChainOwner::Address { address: me() });
        assert_eq!(d.has_store, Some(true));
        assert!(d.can_send);
        assert!(!d.in_kiosk);
    }

    #[tokio::test]
    async fn no_store_ability_cant_send_this_one() {
        let mut c = owned_by(me());
        c.abilities = vec![4];
        let d = nft_detail(&api(Some(popkin())), &c, &nft(), Some(&me())).await.unwrap();
        assert!(!d.can_send);
        assert!(d.cant_send.unwrap().starts_with("Can't send this one"));
    }

    #[tokio::test]
    async fn kiosk_items_are_sendable_someone_elses_are_not() {
        let field = format!("0x{}", "77".repeat(32));
        let kiosk = format!("0x{}", "66".repeat(32));
        let (f2, k2) = (field.clone(), kiosk.clone());
        let c = Chain {
            obj: Box::new(move |id| {
                if id == f2 {
                    obj(id, "0x2::dynamic_field::Field<0x2::dynamic_object_field::Wrapper<0x2::kiosk::Item>, 0x2::object::ID>", Owner::Object { id: k2.clone() })
                } else {
                    obj(id, &nft_t(), Owner::Object { id: f2.clone() })
                }
            }),
            abilities: vec![3, 4],
        };
        let a = api(Some(json!({ "id": nft(), "name": "P", "type": nft_t(), "imageUrl": null, "collection": null, "kioskId": null })));
        let d = nft_detail(&a, &c, &nft(), Some(&me())).await.unwrap();
        assert!(d.in_kiosk);
        assert_eq!(d.kiosk_id.as_deref(), Some(kiosk.as_str()));
        assert!(d.can_send);

        let f3 = field.clone();
        let wrapped = Chain { obj: Box::new(move |id| obj(id, if id == f3 { "0x9::vault::Vault" } else { "0xe5::popkins::Popkins" }, Owner::Object { id: f3.clone() })), abilities: vec![3, 4] };
        let d = nft_detail(&api(Some(popkin())), &wrapped, &nft(), Some(&me())).await.unwrap();
        assert!(!d.in_kiosk);
        assert!(!d.can_send);
        assert_eq!(d.cant_send.as_deref(), Some("It is held inside another object"));

        let d = nft_detail(&api(Some(popkin())), &owned_by(me()), &nft(), Some(&bob())).await.unwrap();
        assert!(!d.can_send);
        assert_eq!(d.cant_send.as_deref(), Some("It is not in this wallet"));
    }

    #[tokio::test]
    async fn falls_back_to_chain_display_when_rare_market_has_nothing() {
        let c = Chain {
            obj: Box::new(|id| ObjectInfo { display: Some(json!({ "name": "Chain name", "image_url": "https://c/1.png" })), ..obj(id, &nft_t(), Owner::Address { address: me() }) }),
            abilities: vec![3],
        };
        let d = nft_detail(&api(None), &c, &nft(), Some(&me())).await.unwrap();
        assert_eq!(d.name, "Chain name");
        assert_eq!(d.image_url.as_deref(), Some("https://c/1.png"));
        assert!(d.rare_missing);
        assert!(d.can_send);
    }

    #[test]
    fn kiosk_item_field_types() {
        assert!(is_kiosk_item_field("0x2::dynamic_field::Field<0x2::dynamic_object_field::Wrapper<0x2::kiosk::Item>, 0x2::object::ID>"));
        assert!(!is_kiosk_item_field("0x2::kiosk::ItemX"));
    }
}
