//! Downloads and updates as notices (class System: a toast in the launcher, list only over a game
//! or with Do not disturb). The host reads Steam's downloads and the emulators' releases (native
//! calls) on the schedule below and hands the results here; what comes back is the notices to post.
//!
//! - A Steam download that was running last poll and is gone now, having reached the end, finished
//!   ("Download finished"), not cancelled.
//! - An emulator release newer than what is installed is announced once per release.
//! - A staged Rare OS update is announced once per version (the host decides "once"; the text is here).

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::notifier::LocalNotice;
use crate::storage::{read_value, write_value, Storage};
use crate::types::{NoticeAction, NoticeKind};

/// How often to read Steam's downloads: in use, and idle or away.
pub const DOWNLOAD_POLL_ACTIVE: Duration = Duration::from_secs(5);
pub const DOWNLOAD_POLL_IDLE: Duration = Duration::from_secs(30);
/// The first emulator check after start, then every six hours while the launcher is shown.
pub const EMU_FIRST_CHECK: Duration = Duration::from_secs(60);
pub const EMU_EVERY: Duration = Duration::from_secs(6 * 3600);

const K_EMU: &str = "rare.notify.emuAnnounced";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Download {
    pub id: String,
    pub title: String,
    pub done: f64,
    pub total: f64,
    pub paused: bool,
}

/// 0..1, or none for an unknown size.
pub fn download_fraction(d: &Download) -> Option<f64> {
    (d.total > 0.0).then(|| (d.done / d.total).min(1.0))
}

/// Downloads that were running last poll and are gone now, and had reached the end: they finished
/// (not cancelled).
pub fn finished_downloads(prev: &[Download], next: &[Download]) -> Vec<Download> {
    prev.iter().filter(|d| !next.iter().any(|x| x.id == d.id) && !d.paused && d.total > 0.0 && d.done / d.total >= 0.98).cloned().collect()
}

/// Emulator releases to announce: available and not announced before ("<id>@<tag>" keys).
pub fn emu_to_announce(id: &str, tag: &str, available: bool, announced: &[String]) -> Option<String> {
    let key = format!("{id}@{tag}");
    (available && !announced.contains(&key)).then_some(key)
}

/// The downloads list for the tile bar and the Rare menu, and finished-download notices.
#[derive(Clone, Debug, Default)]
pub struct DownloadWatch {
    pub list: Vec<Download>,
}

impl DownloadWatch {
    /// A new read of Steam's downloads (`None` when the read failed: the list stays); the notices
    /// to post.
    pub fn poll(&mut self, next: Option<Vec<Download>>) -> Vec<LocalNotice> {
        let Some(next) = next else { return Vec::new() };
        let done = finished_downloads(&self.list, &next);
        self.list = next;
        done.into_iter().map(|d| LocalNotice::new(NoticeKind::DownloadDone, format!("{} finished downloading", d.title), NoticeAction::Downloads)).collect()
    }
}

/// One installed emulator's release check.
#[derive(Clone, Debug, PartialEq)]
pub struct EmuRelease {
    pub id: String,
    pub name: String,
    pub tag: String,
    pub available: bool,
}

/// Announces each emulator release once, remembering the last 60 in storage.
pub fn announce_emulators(storage: &dyn Storage, releases: &[EmuRelease]) -> Vec<LocalNotice> {
    let mut announced: Vec<String> = read_value(storage, K_EMU).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    let mut out = Vec::new();
    for e in releases {
        let Some(key) = emu_to_announce(&e.id, &e.tag, e.available, &announced) else { continue };
        announced.push(key);
        let keep = &announced[announced.len().saturating_sub(60)..];
        write_value(storage, K_EMU, Some(keep));
        out.push(
            LocalNotice::new(NoticeKind::EmuUpdate, format!("{} {} is available", e.name, e.tag), NoticeAction::Emulators)
                .body("Update it in Settings > Emulators."),
        );
    }
    out
}

/// The notice for a staged Rare OS update (the web launcher's OsUpdateNotice).
pub fn os_update_notice(staged_version: Option<&str>) -> LocalNotice {
    os_update_notice_with(staged_version, None)
}

/// The same with the release notes' first two lines after "Restart to update."
pub fn os_update_notice_with(staged_version: Option<&str>, notes: Option<&str>) -> LocalNotice {
    let title = match staged_version.filter(|v| !v.is_empty()) {
        Some(v) => format!("Rare OS {v} is ready"),
        None => "A Rare OS update is ready".to_string(),
    };
    let mut body = "Restart to update.".to_string();
    if let Some(n) = notes {
        for line in n.lines().map(str::trim).filter(|l| !l.is_empty()).take(2) {
            body.push(' ');
            body.push_str(line);
        }
    }
    LocalNotice::new(NoticeKind::OsUpdate, title, NoticeAction::OsUpdate).body(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemStorage;

    fn d(id: &str, done: f64, total: f64, paused: bool) -> Download {
        Download { id: id.into(), title: id.into(), done, total, paused }
    }

    #[test]
    fn reads_progress() {
        assert_eq!(download_fraction(&d("a", 50.0, 100.0, false)), Some(0.5));
        assert_eq!(download_fraction(&d("a", 0.0, 0.0, false)), None);
    }

    #[test]
    fn finishes_only_a_download_that_reached_the_end_and_vanished() {
        let f = finished_downloads(&[d("a", 100.0, 100.0, false), d("b", 40.0, 100.0, false), d("c", 99.0, 100.0, true)], &[d("c", 99.0, 100.0, true)]);
        assert_eq!(f.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["a"]);
        assert!(finished_downloads(&[d("a", 100.0, 100.0, false)], &[d("a", 100.0, 100.0, false)]).is_empty());
    }

    #[test]
    fn announces_a_release_once() {
        assert_eq!(emu_to_announce("rpcs3", "v1", true, &[]).as_deref(), Some("rpcs3@v1"));
        assert_eq!(emu_to_announce("rpcs3", "v1", true, &["rpcs3@v1".into()]), None);
        assert_eq!(emu_to_announce("rpcs3", "v1", false, &[]), None);
    }

    #[test]
    fn watchers_make_notices() {
        let mut w = DownloadWatch::default();
        assert!(w.poll(Some(vec![d("Hades", 100.0, 100.0, false)])).is_empty());
        assert!(w.poll(None).is_empty());
        let n = w.poll(Some(vec![]));
        assert_eq!(n[0].title, "Hades finished downloading");
        let s = MemStorage::new();
        let r = [EmuRelease { id: "rpcs3".into(), name: "RPCS3".into(), tag: "v2".into(), available: true }];
        assert_eq!(announce_emulators(&s, &r)[0].title, "RPCS3 v2 is available");
        assert!(announce_emulators(&s, &r).is_empty());
        assert_eq!(os_update_notice(Some("1.4")).title, "Rare OS 1.4 is ready");
        assert_eq!(os_update_notice(None).title, "A Rare OS update is ready");
        let n = os_update_notice_with(Some("1.4"), Some("Faster start.\n\nNew Settings page.\nThird line."));
        assert_eq!(n.body.as_deref(), Some("Restart to update. Faster start. New Settings page."));
    }
}
