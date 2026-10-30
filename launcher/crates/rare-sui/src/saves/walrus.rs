//! Walrus reads for cloud saves: the player's backups found from chain alone (as a new device
//! finds them), downloads through the public aggregator, and the current epoch. Ports the read
//! half of `src/sui/saves/walrus.ts` and `cloud.ts` (`listTagged`, `readBlob`, `currentEpoch`,
//! `listBackups`, `fetchBackup`).
//!
//! The backup index lives on chain: every backup is a Walrus `Blob` object owned by the player,
//! with attributes `{ app: "rare-saves", v, key, meta, blob }` in its `metadata` dynamic field.

use std::time::Duration;

use serde_json::Value;

use super::crypto::{open, SaveKey};
use super::meta::{open_meta, Backup, APP_TAG};
use crate::grpc::{ChainRead, Include};
use crate::kiosk::{dynamic_field_id, dynamic_field_value};
use crate::network::Network;
use crate::{Error, Result};

pub fn relay(n: Network) -> &'static str {
    match n {
        Network::Mainnet => "https://upload-relay.mainnet.walrus.space",
        Network::Testnet => "https://upload-relay.testnet.walrus.space",
    }
}

pub fn aggregator(n: Network) -> &'static str {
    match n {
        Network::Mainnet => "https://aggregator.walrus-mainnet.walrus.space",
        Network::Testnet => "https://aggregator.walrus-testnet.walrus.space",
    }
}

/// The Walrus system object (@mysten/walrus's package config).
pub fn system_object(n: Network) -> &'static str {
    match n {
        Network::Mainnet => "0x2134d52768ea07e8c43570ef975eb3e4c27a39fa6396bef985b5abc58d03ddd2",
        Network::Testnet => "0x6c2547cbbc38025cf3adac45f63cb0a8d12ecf777cdc75a4971612bf97fdf6af",
    }
}

/// One Rare-tagged Blob object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredBlob {
    pub blob_object_id: String,
    pub blob_id: String,
    pub size: u64,
    pub end_epoch: Option<u64>,
    pub certified: bool,
    pub attributes: Vec<(String, String)>,
}

impl StoredBlob {
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attributes.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str())
    }
}

/// `blobIdFromInt`: a u256 (decimal) as 32 little-endian bytes in base64url without padding.
pub fn blob_id_from_int(dec: &str) -> Option<String> {
    let mut le = [0u8; 32];
    for d in dec.trim().bytes() {
        if !d.is_ascii_digit() {
            return None;
        }
        let mut carry = u32::from(d - b'0');
        for b in le.iter_mut() {
            let v = u32::from(*b) * 10 + carry;
            *b = (v & 0xff) as u8;
            carry = v >> 8;
        }
        if carry != 0 {
            return None;
        }
    }
    use base64::Engine;
    Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(le))
}

fn uleb(b: &[u8], i: &mut usize) -> Option<usize> {
    let (mut v, mut shift) = (0usize, 0u32);
    loop {
        let x = *b.get(*i)?;
        *i += 1;
        v |= usize::from(x & 0x7f) << shift;
        if x & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
        if shift > 35 {
            return None;
        }
    }
}

fn string(b: &[u8], i: &mut usize) -> Option<String> {
    let n = uleb(b, i)?;
    let s = b.get(*i..*i + n)?;
    *i += n;
    String::from_utf8(s.to_vec()).ok()
}

/// `Metadata { metadata: VecMap<String, String> }` BCS to its entries.
pub fn parse_metadata(b: &[u8]) -> Option<Vec<(String, String)>> {
    let mut i = 0;
    let n = uleb(b, &mut i)?;
    (0..n).map(|_| Some((string(b, &mut i)?, string(b, &mut i)?))).collect()
}

/// The Blob object's attributes, None when it has none.
pub async fn read_blob_attributes(c: &dyn ChainRead, blob_object_id: &str) -> Result<Option<Vec<(String, String)>>> {
    let name = [&[8u8][..], b"metadata"].concat();
    match dynamic_field_value(c, blob_object_id, "vector<u8>", &name).await {
        Ok(v) => Ok(parse_metadata(&v)),
        Err(e) if e.is_not_found() => Ok(None),
        Err(e) => Err(e),
    }
}

/// `<walrus package>::blob::Blob`, the package being the system object's type origin.
pub async fn blob_type(c: &dyn ChainRead, n: Network) -> Result<String> {
    let sys = c.object(system_object(n), Include::NONE).await?;
    let pkg = sys.object_type.split("::").next().filter(|p| p.starts_with("0x")).ok_or_else(|| Error::decode("walrus system object type"))?;
    Ok(format!("{pkg}::blob::Blob"))
}

/// The current Walrus epoch (`committee.epoch` of the system state), for "kept until" dates.
pub async fn current_epoch(c: &dyn ChainRead, n: Network) -> Result<u64> {
    let sys = c.object(system_object(n), Include::JSON).await?;
    let version = sys.json.as_ref().and_then(|j| j.get("version")).and_then(|v| v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_u64())).ok_or_else(|| Error::decode("walrus system version"))?;
    let field = dynamic_field_id(system_object(n), "u64", &u64::to_le_bytes(version))?;
    let inner = c.object(&field, Include::JSON).await?;
    let epoch = inner.json.as_ref().and_then(|j| j.pointer("/value/committee/epoch")).ok_or_else(|| Error::decode("walrus committee epoch"))?;
    epoch.as_u64().or_else(|| epoch.as_str().and_then(|s| s.parse().ok())).ok_or_else(|| Error::decode("walrus committee epoch"))
}

fn json_u64(v: Option<&Value>) -> Option<u64> {
    let v = v?;
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// The player's Walrus Blob objects tagged as Rare cloud saves. Read only.
pub async fn list_tagged(c: &dyn ChainRead, n: Network, owner: &str) -> Result<Vec<StoredBlob>> {
    let t = blob_type(c, n).await?;
    let objs = c.owned_objects(owner, Some(&t), 500, Include::JSON).await?;
    let found = futures::future::join_all(objs.iter().map(|o| async move { (o, read_blob_attributes(c, &o.object_id).await.ok().flatten()) })).await;
    Ok(found
        .into_iter()
        .filter_map(|(o, attrs)| {
            let attributes = attrs?;
            let blob = StoredBlob { blob_object_id: o.object_id.clone(), blob_id: String::new(), size: 0, end_epoch: None, certified: false, attributes };
            if blob.attr("app") != Some(APP_TAG) {
                return None;
            }
            let j = o.json.as_ref();
            let certified = j.and_then(|j| j.get("certified_epoch")).is_some_and(|c| !c.is_null() && !c.as_object().is_some_and(|m| m.is_empty()));
            let blob_id = blob.attr("blob").filter(|s| !s.is_empty()).map(str::to_string).or_else(|| j.and_then(|j| j.get("blob_id")).and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_u64().map(|n| n.to_string()))).and_then(|d| blob_id_from_int(&d)))?;
            Some(StoredBlob { blob_id, size: json_u64(j.and_then(|j| j.get("size"))).unwrap_or(0), end_epoch: json_u64(j.and_then(|j| j.pointer("/storage/end_epoch"))), certified, ..blob })
        })
        .collect())
}

/// Downloads a blob through the public aggregator.
pub async fn read_blob(http: &reqwest::Client, n: Network, blob_id: &str) -> Result<Vec<u8>> {
    let url = format!("{}/v1/blobs/{}", aggregator(n), crate::market::encode_component(blob_id));
    let r = http.get(&url).timeout(Duration::from_secs(60)).send().await?;
    if !r.status().is_success() {
        return Err(Error::http(Some(r.status().as_u16()), format!("blob {blob_id}")));
    }
    Ok(r.bytes().await?.to_vec())
}

/// The player's backups that this key opens, newest first. Read only.
pub async fn list_backups(c: &dyn ChainRead, n: Network, owner: &str, key: &SaveKey) -> Result<Vec<Backup>> {
    let mut out: Vec<Backup> = list_tagged(c, n, owner)
        .await?
        .into_iter()
        .filter(|t| t.certified && t.attr("key") == Some(key.id.as_str()))
        .filter_map(|t| {
            let meta = open_meta(key, t.attr("meta")?)?;
            Some(Backup { blob_object_id: t.blob_object_id, blob_id: t.blob_id, size: t.size, end_epoch: t.end_epoch, meta })
        })
        .collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.meta.created));
    Ok(out)
}

/// [`list_backups`] for several owners, reading the description of new blobs only (`known` by
/// Blob object id; a blob's description never changes). End epochs come fresh from the objects.
pub async fn list_backups_known(c: &dyn ChainRead, n: Network, owners: &[String], key: &SaveKey, known: &std::collections::HashMap<String, Backup>) -> Result<Vec<Backup>> {
    let t = blob_type(c, n).await?;
    let mut out = Vec::new();
    for owner in owners {
        let objs = c.owned_objects(owner, Some(&t), 500, Include::JSON).await?;
        let found = futures::future::join_all(objs.iter().map(|o| async move {
            if let Some(k) = known.get(&o.object_id) {
                return (o, Some(Ok(k.clone())));
            }
            (o, read_blob_attributes(c, &o.object_id).await.ok().flatten().map(Err))
        }))
        .await;
        for (o, got) in found {
            let j = o.json.as_ref();
            let end_epoch = json_u64(j.and_then(|j| j.pointer("/storage/end_epoch")));
            match got {
                Some(Ok(mut b)) => {
                    b.end_epoch = end_epoch.or(b.end_epoch);
                    out.push(b);
                }
                Some(Err(attributes)) => {
                    let sb = StoredBlob { blob_object_id: o.object_id.clone(), blob_id: String::new(), size: 0, end_epoch, certified: false, attributes };
                    if sb.attr("app") != Some(APP_TAG) || sb.attr("key") != Some(key.id.as_str()) {
                        continue;
                    }
                    let certified = j.and_then(|j| j.get("certified_epoch")).is_some_and(|c| !c.is_null() && !c.as_object().is_some_and(|m| m.is_empty()));
                    if !certified {
                        continue;
                    }
                    let blob_id = sb.attr("blob").filter(|s| !s.is_empty()).map(str::to_string).or_else(|| j.and_then(|j| j.get("blob_id")).and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_u64().map(|n| n.to_string()))).and_then(|d| blob_id_from_int(&d)));
                    let (Some(blob_id), Some(meta)) = (blob_id, sb.attr("meta").and_then(|m| open_meta(key, m))) else { continue };
                    out.push(Backup { blob_object_id: o.object_id.clone(), blob_id, size: json_u64(j.and_then(|j| j.get("size"))).unwrap_or(0), end_epoch, meta });
                }
                None => {}
            }
        }
    }
    out.sort_by_key(|b| std::cmp::Reverse(b.meta.created));
    Ok(out)
}

/// Downloads and decrypts a backup.
pub async fn fetch_backup(http: &reqwest::Client, n: Network, key: &SaveKey, b: &Backup) -> Result<Vec<u8>> {
    let data = read_blob(http, n, &b.blob_id).await?;
    open(key, &data).map_err(|e| Error::decode(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::ObjectInfo;
    use crate::saves::crypto::derive_save_key;
    use crate::saves::meta::{seal_meta, BackupMeta};
    use crate::BoxFut;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn blob_ids_from_ints() {
        // bcs.u256(1) = 01 00 .. 00
        assert_eq!(blob_id_from_int("1").unwrap(), "AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
        assert_eq!(blob_id_from_int("256").unwrap(), "AAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
        assert_eq!(blob_id_from_int("x"), None);
        assert_eq!(blob_id_from_int(&"9".repeat(80)), None);
    }

    fn uleb_into(mut n: usize, b: &mut Vec<u8>) {
        loop {
            let x = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                b.push(x);
                return;
            }
            b.push(x | 0x80);
        }
    }
    fn meta_bcs(pairs: &[(&str, &str)]) -> Vec<u8> {
        let mut b = vec![];
        uleb_into(pairs.len(), &mut b);
        for (k, v) in pairs {
            for s in [k, v] {
                uleb_into(s.len(), &mut b);
                b.extend(s.as_bytes());
            }
        }
        b
    }

    #[test]
    fn metadata_entries() {
        assert_eq!(parse_metadata(&meta_bcs(&[("app", "rare-saves"), ("v", "1")])).unwrap(), vec![("app".into(), "rare-saves".into()), ("v".into(), "1".into())]);
        assert_eq!(parse_metadata(&[2, 1]), None);
    }

    struct Chain {
        owned: Vec<ObjectInfo>,
        objects: HashMap<String, ObjectInfo>,
    }
    impl ChainRead for Chain {
        fn owned_objects(&self, _o: &str, t: Option<&str>, _m: usize, _i: Include) -> BoxFut<'_, Vec<ObjectInfo>> {
            assert_eq!(t, Some("0xabc::blob::Blob"));
            let v = self.owned.clone();
            Box::pin(async move { Ok(v) })
        }
        fn object(&self, id: &str, _i: Include) -> BoxFut<'_, ObjectInfo> {
            let v = self.objects.get(id).cloned().ok_or_else(|| Error::Grpc { code: crate::error::GRPC_NOT_FOUND, message: "not found".into() });
            Box::pin(async move { v })
        }
    }

    #[tokio::test]
    async fn finds_this_keys_certified_backups_from_chain_alone() {
        let key = derive_save_key(&[9; 32], "0x1").unwrap();
        let other = derive_save_key(&[8; 32], "0x1").unwrap();
        let m = |id: &str, created: i64| BackupMeta { v: 1, id: id.into(), rel: ".config/x".into(), emulator: "X".into(), label: "Saves".into(), kind: "emulator".into(), files: 1, bytes: 3, created, ..Default::default() };
        let name = [&[8u8][..], b"metadata"].concat();
        let mut objects = HashMap::new();
        objects.insert(system_object(Network::Mainnet).to_string(), ObjectInfo { object_type: "0xabc::system::System".into(), json: Some(json!({ "version": "2" })), ..Default::default() });
        let mut owned = vec![];
        let blobs: [(&str, bool, &SaveKey, &str, i64); 4] = [("0xb1", true, &key, "rare-saves", 1), ("0xb2", true, &key, "rare-saves", 5), ("0xb3", false, &key, "rare-saves", 9), ("0xb4", true, &other, "rare-saves", 7)];
        for (id, certified, k, app, created) in blobs {
            owned.push(ObjectInfo { object_id: id.into(), json: Some(json!({ "blob_id": "1", "size": "100", "certified_epoch": if certified { json!(3) } else { Value::Null }, "storage": { "end_epoch": 20 } })), ..Default::default() });
            let meta = seal_meta(k, &m(&format!("set{created}"), created));
            let attrs = meta_bcs(&[("app", app), ("v", "1"), ("key", &k.id), ("meta", &meta)]);
            let field = dynamic_field_id(id, "vector<u8>", &name).unwrap();
            objects.insert(field, ObjectInfo { contents: Some([vec![0u8; 32], name.clone(), attrs].concat()), ..Default::default() });
        }
        // a blob with no attributes at all is skipped
        owned.push(ObjectInfo { object_id: "0xb5".into(), json: Some(json!({ "blob_id": "1" })), ..Default::default() });
        let c = Chain { owned, objects };
        let list = list_backups(&c, Network::Mainnet, "0x1", &key).await.unwrap();
        assert_eq!(list.iter().map(|b| b.blob_object_id.as_str()).collect::<Vec<_>>(), ["0xb2", "0xb1"]);
        assert_eq!(list[0].blob_id, blob_id_from_int("1").unwrap());
        assert_eq!((list[0].size, list[0].end_epoch), (100, Some(20)));
        assert_eq!(list[0].meta.id, "set5");
    }
}
