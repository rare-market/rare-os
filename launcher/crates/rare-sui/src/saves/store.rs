//! Writing a backup to Walrus, paid by the player: the @mysten/walrus `writeBlobFlow` with the
//! upload relay, as `src/sui/saves/walrus.ts` (`payAndStore`, `storeBlob`, `estimate`,
//! `deleteTx`) runs it.
//!
//! Encode (blob id and root hash from MystenLabs' `walrus-core`, the same code the SDK's wasm is
//! built from), register (wallet approval one: pays WAL for storage and the write, the relay tip in
//! SUI, tags the blob), upload the raw blob through the relay (it encodes and distributes the
//! slivers and answers a certificate), certify (wallet approval two).

use std::num::NonZeroU16;
use std::time::Duration;

use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use walrus_core::encoding::{encoded_blob_length_for_n_shards, ReedSolomonEncodingConfig};
use walrus_core::metadata::BlobMetadataApi;
use walrus_core::EncodingType;

use super::walrus::{relay, system_object};
use crate::grpc::{ChainRead, Include, Sui};
use crate::kiosk::dynamic_field_id;
use crate::network::Network;
use crate::ptb::{bcs, Arg, Tx};
use crate::signer::TxSigner;
use crate::{Error, Result};

/// 1 MiB: Walrus prices storage per started unit.
pub const BYTES_PER_UNIT_SIZE: u64 = 1024 * 1024;
/// The relay's tip is capped at 0.5 SUI (walrus.ts `sendTip.max`).
pub const MAX_TIP_MIST: u64 = 500_000_000;
/// Two transactions (register and certify), rough, in MIST.
pub const GAS_ESTIMATE_MIST: u64 = 12_000_000;

/// WAL's coin type.
pub fn wal_type(n: Network) -> &'static str {
    match n {
        Network::Mainnet => "0x356a26eb9e012a68958082340d4c4116e7f55615cf27affcff209cf0ae544f59::wal::WAL",
        Network::Testnet => "0x8270feb7375eee355e64fdb69c50abb6b5f9393a722883c1cf45f8e26048810a::wal::WAL",
    }
}

/// What a write needs from the Walrus system object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemInfo {
    /// The package to call (the system's current `package_id`).
    pub package_id: String,
    pub n_shards: u16,
    /// Committee size (signers bitmap length).
    pub members: usize,
    pub epoch: u64,
    pub storage_price: u64,
    pub write_price: u64,
}

fn num(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

pub async fn system_info(c: &dyn ChainRead, n: Network) -> Result<SystemInfo> {
    let sys = c.object(system_object(n), Include::JSON).await?;
    let j = sys.json.ok_or_else(|| Error::decode("walrus system object"))?;
    let package_id = j.get("package_id").and_then(Value::as_str).ok_or_else(|| Error::decode("walrus package id"))?.to_string();
    let version = j.get("version").and_then(num).ok_or_else(|| Error::decode("walrus system version"))?;
    let inner = c.object(&dynamic_field_id(system_object(n), "u64", &version.to_le_bytes())?, Include::JSON).await?;
    let v = inner.json.as_ref().and_then(|j| j.get("value")).ok_or_else(|| Error::decode("walrus system state"))?;
    let committee = &v["committee"];
    let get = |x: &Value, what: &str| num(x).ok_or_else(|| Error::decode(format!("walrus {what}")));
    Ok(SystemInfo {
        package_id,
        n_shards: get(&committee["n_shards"], "n_shards")? as u16,
        members: committee["members"].as_array().map(Vec::len).unwrap_or(0),
        epoch: get(&committee["epoch"], "epoch")?,
        storage_price: get(&v["storage_price_per_unit_size"], "storage price")?,
        write_price: get(&v["write_price_per_unit_size"], "write price")?,
    })
}

/// A blob's registration data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlobMeta {
    /// 32 bytes; the u256 the contract takes, little endian.
    pub blob_id: [u8; 32],
    pub root_hash: [u8; 32],
    pub size: u64,
    pub encoded_size: u64,
}

impl BlobMeta {
    /// The blob id as Walrus prints it (base64url, no padding).
    pub fn blob_id_text(&self) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(self.blob_id)
    }
}

/// Blob id, root hash and encoded size for `n_shards` (RS2), without encoding the slivers.
pub fn compute_metadata(blob: &[u8], n_shards: u16) -> Result<BlobMeta> {
    let n = NonZeroU16::new(n_shards).ok_or_else(|| Error::invalid("no shards"))?;
    let enc = ReedSolomonEncodingConfig::new(n).get_blob_encoder(blob).map_err(|e| Error::invalid(e.to_string()))?;
    let m = enc.compute_metadata();
    Ok(BlobMeta {
        blob_id: m.blob_id().0,
        root_hash: m.metadata().compute_root_hash().bytes(),
        size: blob.len() as u64,
        encoded_size: encoded_blob_length(blob.len() as u64, n_shards)?,
    })
}

pub fn encoded_blob_length(size: u64, n_shards: u16) -> Result<u64> {
    let n = NonZeroU16::new(n_shards).ok_or_else(|| Error::invalid("no shards"))?;
    encoded_blob_length_for_n_shards(n, size, EncodingType::RS2).ok_or_else(|| Error::invalid("blob too large"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cost {
    pub storage: u64,
    pub write: u64,
    pub total: u64,
}

/// What `epochs` of storage and the write cost, in FROST, for an encoded size.
pub fn storage_cost(encoded_size: u64, epochs: u32, sys: &SystemInfo) -> Cost {
    let units = encoded_size.div_ceil(BYTES_PER_UNIT_SIZE);
    let storage = units * sys.storage_price * u64::from(epochs);
    let write = units * sys.write_price;
    Cost { storage, write, total: storage + write }
}

/// The relay's tip: a constant, or a base plus a price per started KiB of encoded data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TipKind {
    Const(u64),
    Linear { base: u64, per_encoded_kib: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TipConfig {
    pub address: String,
    pub kind: TipKind,
}

impl TipConfig {
    pub fn parse(v: &Value) -> Option<TipConfig> {
        let t = v.get("send_tip")?;
        let address = t.get("address")?.as_str()?.to_string();
        let k = t.get("kind")?;
        let kind = if let Some(c) = k.get("const") {
            TipKind::Const(num(c)?)
        } else {
            let l = k.get("linear")?;
            TipKind::Linear { base: num(l.get("base")?)?, per_encoded_kib: num(l.get("encoded_size_mul_per_kib")?)? }
        };
        Some(TipConfig { address, kind })
    }

    /// The tip for an encoded size; errors above [`MAX_TIP_MIST`].
    pub fn amount(&self, encoded_size: u64) -> Result<u64> {
        let a = match self.kind {
            TipKind::Const(c) => c,
            TipKind::Linear { base, per_encoded_kib } => base + per_encoded_kib * encoded_size.div_ceil(1024),
        };
        if a > MAX_TIP_MIST {
            return Err(Error::invalid(format!("Tip amount ({a}) exceeds the maximum allowed tip ({MAX_TIP_MIST})")));
        }
        Ok(a)
    }
}

/// The relay's tip config, None when it takes no tip.
pub async fn tip_config(http: &reqwest::Client, n: Network) -> Result<Option<TipConfig>> {
    let r = http.get(format!("{}/v1/tip-config", relay(n))).timeout(Duration::from_secs(30)).send().await?;
    if !r.status().is_success() {
        return Err(Error::http(Some(r.status().as_u16()), "tip config"));
    }
    let v: Value = r.json().await?;
    Ok(if v.is_string() { None } else { TipConfig::parse(&v) })
}

/// What storing `size` bytes for `epochs` costs (walrus.ts `estimate`). Spends nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CostEstimate {
    pub epochs: u32,
    pub days: u32,
    /// Storage plus write fee, in FROST (1 WAL = 1e9 FROST).
    pub wal_frost: u64,
    /// Upload relay tip, in MIST.
    pub tip_mist: u64,
    /// The two transactions, rough, in MIST.
    pub gas_mist: u64,
}

pub async fn estimate(c: &dyn ChainRead, http: &reqwest::Client, n: Network, size: u64, days: u32) -> Result<CostEstimate> {
    let testnet = n == Network::Testnet;
    let epochs = super::meta::epochs_for(days, testnet);
    let sys = system_info(c, n).await?;
    let encoded = encoded_blob_length(size, sys.n_shards)?;
    let tip = match tip_config(http, n).await {
        Ok(Some(t)) => t.amount(encoded).unwrap_or(0),
        _ => 0,
    };
    Ok(CostEstimate { epochs, days: epochs * super::meta::epoch_days(testnet), wal_frost: storage_cost(encoded, epochs, &sys).total, tip_mist: tip, gas_mist: GAS_ESTIMATE_MIST })
}

/// The relay's proof that this transaction paid for this blob: sha256(blob) | sha256(nonce) | u64 size.
pub fn auth_payload(blob_digest: &[u8; 32], nonce: &[u8; 32], size: u64) -> Vec<u8> {
    [blob_digest.as_slice(), Sha256::digest(nonce).as_slice(), &size.to_le_bytes()].concat()
}

/// The relay tip's part of a register transaction.
pub struct Tip<'a> {
    pub config: &'a TipConfig,
    pub nonce: [u8; 32],
    pub blob_digest: [u8; 32],
}

/// Register: tip the relay (auth payload input + SUI to its address), reserve storage and register
/// the blob with WAL, write its attributes, hand it to the owner.
#[allow(clippy::too_many_arguments)]
pub fn register_tx(n: Network, owner: &str, sys: &SystemInfo, meta: &BlobMeta, epochs: u32, deletable: bool, attributes: &[(String, String)], tip: Option<Tip<'_>>) -> Result<Tx> {
    register_tx_on(n, owner, sys, meta, epochs, deletable, attributes, tip, None)
}

/// [`register_tx`] on a `Storage` the owner already has (from a deleted backup) instead of buying
/// new space: only the write fee is paid.
#[allow(clippy::too_many_arguments)]
pub fn register_tx_on(n: Network, owner: &str, sys: &SystemInfo, meta: &BlobMeta, epochs: u32, deletable: bool, attributes: &[(String, String)], tip: Option<Tip<'_>>, reuse: Option<&str>) -> Result<Tx> {
    let mut tx = Tx::new();
    tx.set_sender(owner);
    if let Some(t) = tip {
        tx.pure(auth_payload(&t.blob_digest, &t.nonce, meta.size));
        let amount = t.config.amount(meta.encoded_size)?;
        let c = tx.split_u64(Arg::Gas, amount);
        tx.transfer_to(&[c], &t.config.address);
    }
    let cost = storage_cost(meta.encoded_size, epochs, sys);
    let wal = wal_type(n);
    let pkg = &sys.package_id;
    let call = |f: &str| format!("{pkg}::{f}");
    // write payment, then the storage reservation inside it (the SDK's withWal nesting)
    let write_coin = tx.coin_with_balance(wal, cost.write);
    let storage = match reuse {
        Some(id) => tx.object(id),
        None => {
            let storage_coin = tx.coin_with_balance(wal, cost.storage);
            let system = tx.object(system_object(n));
            let amount = tx.pure_u64(meta.encoded_size);
            let ahead = tx.pure(epochs.to_le_bytes().to_vec());
            let storage = tx.move_call(&call("system::reserve_space"), &[], &[system, amount, ahead, storage_coin]);
            tx.move_call("0x2::coin::destroy_zero", &[wal], &[storage_coin]);
            storage
        }
    };
    let system = tx.object(system_object(n));
    let args = [
        system,
        storage,
        tx.pure(meta.blob_id.to_vec()),
        tx.pure(meta.root_hash.to_vec()),
        tx.pure_u64(meta.size),
        tx.pure(bcs::u8(1)),
        tx.pure_bool(deletable),
        write_coin,
    ];
    let blob = tx.move_call(&call("system::register_blob"), &[], &args);
    if !attributes.is_empty() {
        let md = tx.move_call(&call("metadata::new"), &[], &[]);
        tx.move_call(&call("blob::add_metadata"), &[], &[blob, md]);
        for (k, v) in attributes {
            let (k, v) = (tx.pure_string(k), tx.pure_string(v));
            tx.move_call(&call("blob::insert_or_update_metadata_pair"), &[], &[blob, k, v]);
        }
    }
    tx.move_call("0x2::coin::destroy_zero", &[wal], &[write_coin]);
    tx.transfer_to(&[blob], owner);
    Ok(tx)
}

/// The relay's confirmation certificate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Certificate {
    pub signers: Vec<u16>,
    pub message: Vec<u8>,
    pub signature: Vec<u8>,
}

impl Certificate {
    pub fn parse(v: &Value) -> Option<Certificate> {
        let c = v.get("confirmation_certificate")?;
        let signers = c.get("signers")?.as_array()?.iter().map(|s| s.as_u64().map(|x| x as u16)).collect::<Option<Vec<_>>>()?;
        let message = c.get("serialized_message")?.as_array()?.iter().map(|b| b.as_u64().map(|x| x as u8)).collect::<Option<Vec<_>>>()?;
        let sig = c.get("signature")?.as_str()?;
        // The relay answers standard base64 (with + and /); older answers were URL safe.
        let sig = sig.trim_end_matches('=');
        let signature = base64::engine::general_purpose::STANDARD_NO_PAD.decode(sig).or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(sig)).ok()?;
        Some(Certificate { signers, message, signature })
    }
}

/// Signer indices to the contract's bitmap (bit i of byte i/8).
pub fn signers_to_bitmap(signers: &[u16], committee_size: usize) -> Vec<u8> {
    let mut b = vec![0u8; committee_size.div_ceil(8)];
    for s in signers {
        if let Some(byte) = b.get_mut(usize::from(*s) / 8) {
            *byte |= 1 << (s % 8);
        }
    }
    b
}

/// Uploads the raw blob through the relay, which encodes it, stores the slivers and answers the
/// storage certificate.
#[allow(clippy::too_many_arguments)]
pub async fn upload(http: &reqwest::Client, n: Network, meta: &BlobMeta, blob: Vec<u8>, nonce: Option<&[u8; 32]>, tx_digest: &str, blob_object_id: &str, deletable: bool) -> Result<Certificate> {
    let b64 = |b: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b);
    let mut q = vec![("blob_id", meta.blob_id_text())];
    if let Some(nonce) = nonce {
        q.push(("nonce", b64(nonce)));
        q.push(("tx_id", tx_digest.to_string()));
    }
    if deletable {
        q.push(("deletable_blob_object", blob_object_id.to_string()));
    }
    q.push(("encoding_type", "RS2".into()));
    let r = http.post(format!("{}/v1/blob-upload-relay", relay(n))).query(&q).body(blob).timeout(Duration::from_secs(120)).send().await?;
    let status = r.status();
    let text = r.text().await?;
    if !status.is_success() {
        return Err(Error::http(Some(status.as_u16()), text));
    }
    Certificate::parse(&serde_json::from_str(&text)?).ok_or_else(|| Error::decode(format!("relay certificate: {}", text.chars().take(200).collect::<String>())))
}

/// Certify: the certificate moves the blob to certified.
pub fn certify_tx(n: Network, owner: &str, sys: &SystemInfo, blob_object_id: &str, cert: &Certificate) -> Tx {
    let mut tx = Tx::new();
    tx.set_sender(owner);
    let system = tx.object(system_object(n));
    let blob = tx.object(blob_object_id);
    let sig = tx.pure(bcs::bytes(&cert.signature));
    let bitmap = tx.pure(bcs::bytes(&signers_to_bitmap(&cert.signers, sys.members)));
    let msg = tx.pure(bcs::bytes(&cert.message));
    tx.move_call(&format!("{}::system::certify_blob", sys.package_id), &[], &[system, blob, sig, bitmap, msg]);
    tx
}

/// A transaction that deletes an old backup and returns its storage to the owner (player
/// initiated only).
pub fn delete_tx(n: Network, owner: &str, sys: &SystemInfo, blob_object_id: &str) -> Tx {
    let mut tx = Tx::new();
    tx.set_sender(owner);
    let system = tx.object(system_object(n));
    let blob = tx.object(blob_object_id);
    let storage = tx.move_call(&format!("{}::system::delete_blob", sys.package_id), &[], &[system, blob]);
    tx.transfer_to(&[storage], owner);
    tx
}

/// Extends a blob the owner holds by `epochs`, paid in WAL (storage for its encoded size).
pub fn extend_tx(n: Network, owner: &str, sys: &SystemInfo, blob_object_id: &str, size: u64, epochs: u32) -> Result<Tx> {
    let enc = encoded_blob_length(size, sys.n_shards)?;
    let cost = storage_cost(enc, epochs, sys);
    let wal = wal_type(n);
    let mut tx = Tx::new();
    tx.set_sender(owner);
    let coin = tx.coin_with_balance(wal, cost.storage);
    let system = tx.object(system_object(n));
    let blob = tx.object(blob_object_id);
    let e = tx.pure(epochs.to_le_bytes().to_vec());
    tx.move_call(&format!("{}::system::extend_blob", sys.package_id), &[], &[system, blob, e, coin]);
    tx.move_call("0x2::coin::destroy_zero", &[wal], &[coin]);
    Ok(tx)
}

/// Where a write is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Encode,
    Register,
    Upload,
    Certify,
    Done,
}

pub struct StoreOpts<'a> {
    pub epochs: u32,
    pub attributes: Vec<(String, String)>,
    /// What the review sheet calls it ("RetroArch save files").
    pub title: String,
    /// Tag the blob with its id (`blob` attribute), as cloud saves do.
    pub tag_blob_id: bool,
    /// Cloud saves: deletable (old backups can be deleted for a rebate).
    pub deletable: bool,
    /// Review sheet titles (register, certify) when they are not the cloud saves wording.
    pub titles: Option<(String, String)>,
    pub on_step: Option<&'a (dyn Fn(Step) + Sync)>,
}

/// A Walrus quilt of `files` (identifier, bytes), as the SDK's `encodeQuilt` makes it: store the
/// result as one blob with the attribute `_walrusBlobType=quilt`; its blob id is the quilt id.
pub fn encode_quilt(files: &[(String, Vec<u8>)], n_shards: u16) -> Result<Vec<u8>> {
    use walrus_core::encoding::quilt_encoding::{QuiltApi, QuiltConfigApi, QuiltConfigV1, QuiltEncoderApi, QuiltStoreBlob};
    use walrus_core::encoding::EncodingConfigEnum;
    let n = NonZeroU16::new(n_shards).ok_or_else(|| Error::invalid("no shards"))?;
    let blobs = files.iter().map(|(id, b)| QuiltStoreBlob::new(b, id.clone())).collect::<std::result::Result<Vec<_>, _>>().map_err(|e| Error::invalid(e.to_string()))?;
    let enc = QuiltConfigV1::get_encoder(EncodingConfigEnum::ReedSolomon(ReedSolomonEncodingConfig::new(n)), &blobs);
    let q = enc.construct_quilt().map_err(|e| Error::invalid(e.to_string()))?;
    Ok(q.into_data())
}

/// Waits until the node has the transaction, then finds the Blob it created.
async fn created_blob(sui: &Sui, digest: &str, blob_type: &str) -> Result<String> {
    use sui_rpc::proto::sui::rpc::v2::changed_object::IdOperation;
    let mut last = None;
    for _ in 0..30 {
        match sui.transaction(digest, &["effects"]).await {
            Ok(t) => {
                let created = t.effects.iter().flat_map(|e| e.changed_objects.iter()).find(|o| o.id_operation == Some(IdOperation::Created as i32) && o.object_type.as_deref().map(crate::ids::canon_type) == Some(crate::ids::canon_type(blob_type)));
                return created.and_then(|o| o.object_id.clone()).ok_or_else(|| Error::decode(format!("Blob object not found in transaction effects for transaction ({digest})")));
            }
            Err(e) => last = Some(e),
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err(last.unwrap_or_else(|| Error::decode("transaction not found")))
}

/// A `Storage` the owner holds (space left by a deleted blob).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpareStorage {
    pub object_id: String,
    pub size: u64,
    pub end_epoch: u64,
}

/// The owner's spare `Storage` objects.
pub async fn spare_storage(c: &dyn ChainRead, n: Network, owner: &str) -> Result<Vec<SpareStorage>> {
    let blob_t = super::walrus::blob_type(c, n).await?;
    let t = blob_t.replace("::blob::Blob", "::storage_resource::Storage");
    let objs = c.owned_objects(owner, Some(&t), 200, Include::JSON).await?;
    let num = |v: Option<&Value>| v.and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())));
    Ok(objs.iter().filter_map(|o| {
        let j = o.json.as_ref()?;
        Some(SpareStorage { object_id: o.object_id.clone(), size: num(j.get("storage_size"))?, end_epoch: num(j.get("end_epoch"))? })
    }).collect())
}

/// Stores a blob paid by the player: two wallet approvals. Answers (blob id, Blob object id).
pub async fn store_blob(sui: &Sui, http: &reqwest::Client, signer: &dyn TxSigner, blob: Vec<u8>, o: StoreOpts<'_>) -> Result<(String, String)> {
    store_blob_on(sui, http, signer, blob, o, None).await
}

/// [`store_blob`]; with `min_epochs_left`, a spare `Storage` of the signer that fits and lasts at
/// least that many more epochs is used instead of buying space.
pub async fn store_blob_on(sui: &Sui, http: &reqwest::Client, signer: &dyn TxSigner, blob: Vec<u8>, o: StoreOpts<'_>, min_epochs_left: Option<u64>) -> Result<(String, String)> {
    let n = sui.network();
    let step = |s: Step| {
        if let Some(f) = o.on_step {
            f(s)
        }
    };
    let owner = signer.address();
    step(Step::Encode);
    let sys = system_info(sui, n).await?;
    let meta = compute_metadata(&blob, sys.n_shards)?;
    let blob_id = meta.blob_id_text();
    let tip_cfg = tip_config(http, n).await?;
    let nonce: [u8; 32] = {
        use aes_gcm::aead::rand_core::RngCore;
        let mut b = [0u8; 32];
        aes_gcm::aead::OsRng.fill_bytes(&mut b);
        b
    };
    let digest: [u8; 32] = Sha256::digest(&blob).into();
    let tip = tip_cfg.as_ref().map(|config| Tip { config, nonce, blob_digest: digest });
    let mut attrs = o.attributes.clone();
    if o.tag_blob_id {
        attrs.push(("blob".into(), blob_id.clone()));
    }

    step(Step::Register);
    let reuse = match min_epochs_left {
        Some(left) => spare_storage(sui, n, &owner).await.unwrap_or_default().into_iter().filter(|s| s.size >= meta.encoded_size && s.end_epoch >= sys.epoch + left).min_by_key(|s| (s.size, s.end_epoch)).map(|s| s.object_id),
        None => None,
    };
    let reg = register_tx_on(n, &owner, &sys, &meta, o.epochs, o.deletable, &attrs, tip, reuse.as_deref())?;
    let reg_digest = signer.sign_and_execute(reg, &o.titles.as_ref().map(|t| t.0.clone()).unwrap_or_else(|| format!("Back up {}", o.title))).await?;
    let blob_type = super::walrus::blob_type(sui, n).await?;
    let blob_object_id = created_blob(sui, &reg_digest, &blob_type).await?;

    step(Step::Upload);
    let cert = upload(http, n, &meta, blob, tip_cfg.as_ref().map(|_| &nonce), &reg_digest, &blob_object_id, o.deletable).await?;

    step(Step::Certify);
    let cert_tx = certify_tx(n, &owner, &sys, &blob_object_id, &cert);
    signer.sign_and_execute(cert_tx, &o.titles.as_ref().map(|t| t.1.clone()).unwrap_or_else(|| format!("Confirm backup of {}", o.title))).await?;
    step(Step::Done);
    Ok((blob_id, blob_object_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn data() -> Vec<u8> {
        (0..5000u32).map(|i| ((i * 31 + 7) & 255) as u8).collect()
    }

    /// The same bytes through @mysten/walrus-wasm's `BlobEncoder.compute_metadata` (1000 and 10 shards).
    #[test]
    fn blob_ids_match_the_sdk() {
        let m = compute_metadata(&data(), 1000).unwrap();
        assert_eq!(m.blob_id_text(), "LFIiGwYmJRlUkU3Y7VF9FL-x6RQDpL41PC1ccLo_hqQ");
        assert_eq!(hex::encode(m.root_hash), "0c8f7440ca9f1e0ad82f13df61fa00cd6bcee324ebc02f29c5e09ff476ab9510");
        let m = compute_metadata(&data(), 10).unwrap();
        assert_eq!(m.blob_id_text(), "UgBtUrORB9KL9Sbk4AjrQu5MSzukVMsGJlfXrrNjCFo");
        assert_eq!(hex::encode(m.root_hash), "e6a5b2dbfe3069d3ffda11f7766ed22f10dbe283b33392f80750dacc217a53e6");
        assert_eq!(m.size, 5000);
    }

    fn sys() -> SystemInfo {
        SystemInfo { package_id: "0x98da433aa0139512c210597b1c5e3df6cd121d8d77f8652691bb66fadfc8aa1b".into(), n_shards: 1000, members: 101, epoch: 42, storage_price: 63_531, write_price: 115_434 }
    }

    #[test]
    fn costs_and_tips() {
        let enc = encoded_blob_length(5000, 1000).unwrap();
        assert!(enc > 5000);
        let c = storage_cost(enc, 7, &sys());
        let units = enc.div_ceil(BYTES_PER_UNIT_SIZE);
        assert_eq!(c.storage, units * 63_531 * 7);
        assert_eq!(c.total, c.storage + units * 115_434);
        let t = TipConfig::parse(&json!({ "send_tip": { "address": "0xab", "kind": { "linear": { "base": 100, "encoded_size_mul_per_kib": 40 } } } })).unwrap();
        assert_eq!(t.amount(2049).unwrap(), 100 + 40 * 3);
        let t = TipConfig::parse(&json!({ "send_tip": { "address": "0xab", "kind": { "const": 7 } } })).unwrap();
        assert_eq!(t.amount(1).unwrap(), 7);
        assert!(TipConfig { address: "0x1".into(), kind: TipKind::Const(MAX_TIP_MIST + 1) }.amount(1).is_err());
        assert_eq!(signers_to_bitmap(&[0, 9, 100], 101), {
            let mut b = vec![0u8; 13];
            b[0] = 1;
            b[1] = 2;
            b[12] = 16;
            b
        });
    }

    #[test]
    fn register_layout() {
        let meta = compute_metadata(&data(), 1000).unwrap();
        let cfg = TipConfig { address: "0xab".into(), kind: TipKind::Const(1000) };
        let tip = Tip { config: &cfg, nonce: [7; 32], blob_digest: [9; 32] };
        let attrs = vec![("app".to_string(), "rare-saves".to_string()), ("v".into(), "1".into())];
        let tx = register_tx(Network::Mainnet, "0x1", &sys(), &meta, 7, true, &attrs, Some(tip)).unwrap();
        let calls: Vec<String> = tx.move_calls().map(|m| format!("{}::{}", m.module, m.function)).collect();
        assert_eq!(calls, ["system::reserve_space", "coin::destroy_zero", "system::register_blob", "metadata::new", "blob::add_metadata", "blob::insert_or_update_metadata_pair", "blob::insert_or_update_metadata_pair", "coin::destroy_zero"]);
        let j = tx.to_json();
        let kinds: Vec<&str> = j["commands"].as_array().unwrap().iter().map(|c| c["$kind"].as_str().unwrap()).collect();
        assert_eq!(&kinds[..4], ["SplitCoins", "TransferObjects", "$Intent", "$Intent"]);
        assert_eq!(*kinds.last().unwrap(), "TransferObjects");
        // the auth payload is the first input: sha256(blob) | sha256(nonce) | size
        assert_eq!(j["inputs"][0]["Pure"]["bytes"].as_str().unwrap(), base64::engine::general_purpose::STANDARD.encode(auth_payload(&[9; 32], &[7; 32], 5000)));
        assert_eq!(tx.split_amounts(), vec![1000]);
        // the system object is one input however often it is used
        assert_eq!(tx.object_ids(), vec![crate::ids::canon_addr(system_object(Network::Mainnet))]);
    }

    #[test]
    fn certificate_from_the_relay() {
        let c = Certificate::parse(&json!({ "blob_id": "x", "confirmation_certificate": { "signers": [0, 3], "serialized_message": [1, 2, 3], "signature": "AQID" } })).unwrap();
        assert_eq!(c, Certificate { signers: vec![0, 3], message: vec![1, 2, 3], signature: vec![1, 2, 3] });
        // standard alphabet, as the relay answers now
        let c2 = Certificate::parse(&json!({ "confirmation_certificate": { "signers": [1], "serialized_message": [1], "signature": "+/8=" } })).unwrap();
        assert_eq!(c2.signature, vec![0xfb, 0xff]);
        let c3 = Certificate::parse(&json!({ "confirmation_certificate": { "signers": [1], "serialized_message": [1], "signature": "-_8" } })).unwrap();
        assert_eq!(c3.signature, vec![0xfb, 0xff]);
        let tx = certify_tx(Network::Mainnet, "0x1", &sys(), "0x5", &c);
        assert_eq!(tx.targets(), vec![format!("{}::system::certify_blob", sys().package_id)]);
        let d = delete_tx(Network::Mainnet, "0x1", &sys(), "0x5");
        assert_eq!(d.targets(), vec![format!("{}::system::delete_blob", sys().package_id)]);
    }
}
