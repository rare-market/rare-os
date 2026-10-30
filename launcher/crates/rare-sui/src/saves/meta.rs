//! What a backup is: the encrypted description stored next to it, and the bookkeeping the Cloud
//! saves screen needs. Ports the pure half of `src/sui/saves/cloud.ts` and `network.ts`.

use std::collections::HashMap;

use base64::Engine;
use serde::{Deserialize, Serialize};

use super::crypto::{open, seal, SaveKey};

/// The description of a backup (encrypted; public attributes never show what it is).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupMeta {
    pub v: u32,
    pub id: String,
    pub rel: String,
    pub emulator: String,
    pub label: String,
    /// "emulator" | "webgame"
    pub kind: String,
    pub files: u64,
    pub bytes: u64,
    /// unix ms
    pub created: i64,
    /// Automatic cloud saves: a fingerprint of the files (names, sizes, times), to tell an
    /// unchanged copy from a changed one without downloading it. Older backups have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fp: Option<String>,
    /// Newest save file in the copy, unix ms (automatic cloud saves).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<i64>,
    /// Size of the save files themselves (bytes is the packed copy), automatic uploads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<u64>,
    /// "auto" for automatic uploads (older ones are deleted as new ones arrive).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub how: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub blob_object_id: String,
    pub blob_id: String,
    pub size: u64,
    pub end_epoch: Option<u64>,
    pub meta: BackupMeta,
}

/// How long a backup is kept, in days (the picker's choices).
pub const KEEP_OPTIONS: [u32; 4] = [30, 90, 180, 365];
/// The Walrus attribute every Rare backup carries (`app`).
pub const APP_TAG: &str = "rare-saves";
/// The furthest ahead Walrus storage can be bought, in epochs.
pub const MAX_EPOCHS: u32 = 53;

/// Walrus epoch length in days: 14 on mainnet, 1 on testnet.
pub fn epoch_days(testnet: bool) -> u32 {
    if testnet {
        1
    } else {
        14
    }
}

pub fn epochs_for(days: u32, testnet: bool) -> u32 {
    days.div_ceil(epoch_days(testnet)).clamp(1, MAX_EPOCHS)
}

/// Encrypts the description stored next to a backup (standard base64).
pub fn seal_meta(key: &SaveKey, meta: &BackupMeta) -> String {
    base64::engine::general_purpose::STANDARD.encode(seal(key, &serde_json::to_vec(meta).unwrap_or_default()))
}

/// The description, or None when this key doesn't open it or it isn't one.
pub fn open_meta(key: &SaveKey, b64: &str) -> Option<BackupMeta> {
    let raw = base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()?;
    let m: BackupMeta = serde_json::from_slice(&open(key, &raw).ok()?).ok()?;
    (m.v == 1).then_some(m)
}

/// The newest backup of each save set.
pub fn latest_by_set(list: &[Backup]) -> HashMap<String, Backup> {
    let mut out: HashMap<String, Backup> = HashMap::new();
    for b in list {
        if out.get(&b.meta.id).is_none_or(|cur| b.meta.created > cur.meta.created) {
            out.insert(b.meta.id.clone(), b.clone());
        }
    }
    out
}

/// Days of storage left on a backup, from its end epoch.
pub fn days_left(b: &Backup, epoch: Option<u64>, testnet: bool) -> Option<u64> {
    let (end, now) = (b.end_epoch?, epoch?);
    Some(end.saturating_sub(now) * u64::from(epoch_days(testnet)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::saves::crypto::derive_save_key;

    fn meta(id: &str, created: i64) -> BackupMeta {
        BackupMeta { v: 1, id: id.into(), rel: ".config/x".into(), emulator: "X".into(), label: "Saves".into(), kind: "emulator".into(), files: 1, bytes: 10, created, ..Default::default() }
    }
    fn key(seed: u8) -> SaveKey {
        derive_save_key(&[seed; 32], "0x1").unwrap()
    }

    // crypto.test.ts "backup descriptions"
    #[test]
    fn round_trip_encrypted() {
        let k = key(1);
        let m = meta("rpcs3", 5);
        assert_eq!(open_meta(&k, &seal_meta(&k, &m)), Some(m.clone()));
        assert_eq!(open_meta(&key(2), &seal_meta(&k, &m)), None);
    }

    #[test]
    fn latest_backup_per_save_set() {
        let b = |id: &str, created: i64| Backup { blob_object_id: format!("{id}{created}"), blob_id: "x".into(), size: 1, end_epoch: None, meta: meta(id, created) };
        let m = latest_by_set(&[b("a", 1), b("a", 3), b("b", 2), b("a", 2)]);
        assert_eq!(m["a"].meta.created, 3);
        assert_eq!(m["b"].meta.created, 2);
    }

    #[test]
    fn epochs_and_days() {
        assert_eq!(epochs_for(90, false), 7);
        assert_eq!(epochs_for(1, false), 1);
        assert_eq!(epochs_for(3650, false), MAX_EPOCHS);
        assert_eq!(epochs_for(2, true), 2);
        let b = Backup { blob_object_id: String::new(), blob_id: String::new(), size: 0, end_epoch: Some(10), meta: meta("a", 1) };
        assert_eq!(days_left(&b, Some(8), false), Some(28));
        assert_eq!(days_left(&b, Some(12), false), Some(0));
        assert_eq!(days_left(&b, None, false), None);
    }

    #[test]
    fn meta_json_matches_the_ts_shape() {
        let v = serde_json::to_value(meta("webgame:act", 7)).unwrap();
        assert_eq!(v["v"], 1);
        assert_eq!(v["created"], 7);
        assert!(v.get("emulator").is_some());
    }
}
