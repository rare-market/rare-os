//! Rare cloud saves: emulator and web game saves packed on the device, encrypted with a key only
//! the player's wallet can produce, and kept on Walrus. Ports `src/sui/saves/*` and
//! `src-tauri/src/sui/saves.rs`.
//!
//! * [`local`]: find save folders, pack one, restore one safely (no network; src-tauri links it).
//! * [`crypto`]: the save key and AES-256-GCM sealing, byte-compatible with the web launcher.
//! * [`meta`]: the encrypted backup description, latest per set, epochs and days left.
//! * [`walrus`] (`chain` feature): backups listed from chain, downloads, the current epoch.
//! * [`store`] (`walrus-write` feature): writing a backup (encode, register, upload through the
//!   relay, certify), cost estimates and deletes, paid through a [`crate::signer::TxSigner`].
//!
//! [`backup`] and [`restore`] are the Back up and Restore buttons end to end.

pub mod crypto;
pub mod local;
pub mod ludusavi;
pub mod meta;
pub mod sync;
#[cfg(feature = "walrus-write")]
pub mod store;
#[cfg(feature = "chain")]
pub mod walrus;

pub use local::SaveSet;

/// Packs, encrypts and stores one save set (the Back up button; nothing uploads without it).
/// `pack` is [`local::pack`] in the launcher. Two wallet approvals through `signer`.
#[cfg(feature = "walrus-write")]
#[allow(clippy::too_many_arguments)]
pub async fn backup(
    sui: &crate::Sui,
    http: &reqwest::Client,
    signer: &dyn crate::signer::TxSigner,
    key: &crypto::SaveKey,
    set: &SaveSet,
    days: u32,
    on_step: Option<&(dyn Fn(store::Step) + Sync)>,
    pack: impl FnOnce(&str) -> Result<Vec<u8>, String>,
    now_ms: i64,
) -> crate::Result<meta::Backup> {
    let archive = pack(&set.id).map_err(crate::Error::Invalid)?;
    let m = meta::BackupMeta { v: 1, id: set.id.clone(), rel: set.rel.clone(), emulator: set.emulator.clone(), label: set.label.clone(), kind: set.kind.clone(), files: set.files, bytes: archive.len() as u64, created: now_ms, ..Default::default() };
    let blob = crypto::seal(key, &archive);
    let size = blob.len() as u64;
    let testnet = sui.network() == crate::Network::Testnet;
    let attributes = vec![("app".to_string(), meta::APP_TAG.to_string()), ("v".into(), "1".into()), ("key".into(), key.id.clone()), ("meta".into(), meta::seal_meta(key, &m))];
    let opts = store::StoreOpts { epochs: meta::epochs_for(days, testnet), attributes, title: format!("{} {}", set.emulator, set.label.to_lowercase()), tag_blob_id: true, deletable: true, titles: None, on_step };
    let (blob_id, blob_object_id) = store::store_blob(sui, http, signer, blob, opts).await?;
    Ok(meta::Backup { blob_object_id, blob_id, size, end_epoch: None, meta: m })
}

/// Encrypts and stores an already packed copy described by `m` (automatic cloud saves; the writer
/// signs without a review). `m.bytes` is set from the archive.
#[cfg(feature = "walrus-write")]
/// Space left by deleted automatic uploads is used again when it lasts `min_epochs_left` more epochs.
pub async fn upload(sui: &crate::Sui, http: &reqwest::Client, signer: &dyn crate::signer::TxSigner, key: &crypto::SaveKey, mut m: meta::BackupMeta, archive: &[u8], epochs: u32, min_epochs_left: u64) -> crate::Result<meta::Backup> {
    m.bytes = archive.len() as u64;
    let blob = crypto::seal(key, archive);
    let size = blob.len() as u64;
    let attributes = vec![("app".to_string(), meta::APP_TAG.to_string()), ("v".into(), "1".into()), ("key".into(), key.id.clone()), ("meta".into(), meta::seal_meta(key, &m))];
    let title = m.label.clone();
    let opts = store::StoreOpts { epochs, attributes, title, tag_blob_id: true, deletable: true, titles: None, on_step: None };
    let (blob_id, blob_object_id) = store::store_blob_on(sui, http, signer, blob, opts, Some(min_epochs_left)).await?;
    Ok(meta::Backup { blob_object_id, blob_id, size, end_epoch: None, meta: m })
}

/// Deletes an old backup the signer owns (automatic uploads past the kept versions).
#[cfg(feature = "walrus-write")]
pub async fn delete(sui: &crate::Sui, signer: &dyn crate::signer::TxSigner, blob_object_id: &str) -> crate::Result<String> {
    let n = sui.network();
    let sys = store::system_info(sui, n).await?;
    signer.sign_and_execute(store::delete_tx(n, &signer.address(), &sys, blob_object_id), "Delete an old cloud save").await
}

/// Extends a backup the signer owns by `epochs` (keeps the newest copy of a save set alive).
#[cfg(feature = "walrus-write")]
pub async fn extend(sui: &crate::Sui, signer: &dyn crate::signer::TxSigner, b: &meta::Backup, epochs: u32) -> crate::Result<String> {
    let n = sui.network();
    let sys = store::system_info(sui, n).await?;
    signer.sign_and_execute(store::extend_tx(n, &signer.address(), &sys, &b.blob_object_id, b.size, epochs)?, "Keep a cloud save longer").await
}

/// How many epochs to add so a backup ending at `end` lasts until `now + want` (0: no need yet,
/// it has more than `when` left; capped so the end stays within Walrus's reach).
pub fn renewal(end: u64, now: u64, when: u64, want: u64) -> u32 {
    if end <= now || end - now > when {
        return 0;
    }
    let target = now + want.min(u64::from(meta::MAX_EPOCHS) - 1);
    target.saturating_sub(end) as u32
}

/// Downloads, decrypts and unpacks a backup into its save set's folder (the Restore button).
/// Answers where the old folder was moved ("" if nothing was there).
#[cfg(feature = "chain")]
pub async fn restore(http: &reqwest::Client, network: crate::Network, key: &crypto::SaveKey, b: &meta::Backup) -> crate::Result<String> {
    let archive = walrus::fetch_backup(http, network, key, b).await?;
    let (id, rel) = (b.meta.id.clone(), b.meta.rel.clone());
    tokio::task::spawn_blocking(move || local::restore(&id, &rel, &archive)).await.map_err(|e| crate::Error::Invalid(e.to_string()))?.map_err(crate::Error::Invalid)
}

#[cfg(test)]
mod tests {
    #[test]
    fn renewal_only_near_the_end() {
        assert_eq!(super::renewal(100, 50, 4, 26), 0, "plenty left");
        assert_eq!(super::renewal(53, 50, 4, 26), 23, "three left: up to now + 26");
        assert_eq!(super::renewal(50, 50, 4, 26), 0, "already expired");
        assert_eq!(super::renewal(52, 50, 4, 200), 50, "capped at 52 ahead");
    }
}
