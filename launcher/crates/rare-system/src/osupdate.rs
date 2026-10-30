//! OS updates in the launcher. The image downloads the next version in the background
//! (rare-update.service, `/usr/libexec/rare/os-update`) and stages it; it takes effect at the next
//! restart. This reads that state (`rpm-ostree status --json`, the updater's
//! `/var/lib/rare/update-state.json`) for Settings and the "Restart to update" notice, and runs the
//! few root actions through `pkexec /usr/libexec/rare/os-admin` (polkit org.rare.os.admin, the
//! person at the device only).

use serde::{Deserialize, Serialize};
use serde_json::Value;

const ADMIN: &str = "/usr/libexec/rare/os-admin";
const STATE_FILE: &str = "/var/lib/rare/update-state.json";
/// A version the updater skips: {digest, version, checksum, reason: failed | rolled-back, at}.
const SKIP_FILE: &str = "/var/lib/rare/update-skip.json";
/// The updater writes a heartbeat every 20 s while it works; older than this, it stopped.
const STALE_SECS: u64 = 120;
/// How long Settings mentions an automatic fallback.
const FELL_BACK_SECS: u64 = 14 * 86400;

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct OsUpdate {
    /// The version running now.
    pub booted_version: Option<String>,
    /// A newer version is downloaded and waits for a restart.
    pub staged: bool,
    pub staged_version: Option<String>,
    /// Unix seconds of the last update check, and whether it worked.
    pub checked_at: Option<u64>,
    pub check_ok: Option<bool>,
    pub check_error: Option<String>,
    /// The updater is at work now ("checking" or "downloading", with a fresh heartbeat).
    pub busy: Option<String>,
    /// Download progress in bytes, layers already on the device included.
    pub bytes: Option<u64>,
    pub bytes_total: Option<u64>,
    /// The version being downloaded.
    pub offer_version: Option<String>,
    /// Why an automatic download waits: "battery", "game" or "metered".
    pub waiting: Option<String>,
    /// The last run's result: current, staged, waiting, failed, skipped, unsupported, stopped.
    pub result: Option<String>,
    /// The last run stopped without finishing (it died, or the device turned off).
    pub stopped: bool,
    /// Release notes of the downloaded version.
    pub notes: Option<String>,
    /// The previous Rare OS version on the device, to go back to.
    pub previous_version: Option<String>,
    /// A version that did not start and was rolled back by itself (shown for two weeks).
    pub fell_back_from: Option<String>,
}

fn dep_version(d: &Value) -> Option<String> {
    d.get("version").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from)
}

/// Booted and staged versions from `rpm-ostree status --json`. A staged deployment has
/// `"staged": true`; the booted one `"booted": true`. A deployment without a version string
/// (a plain image tag) shows its short checksum instead.
pub fn parse_status(json: &str) -> OsUpdate {
    let mut out = OsUpdate::default();
    let Ok(v) = serde_json::from_str::<Value>(json) else { return out };
    let Some(deps) = v.get("deployments").and_then(|d| d.as_array()) else { return out };
    let name = |d: &Value| dep_version(d).or_else(|| d.get("checksum").and_then(|c| c.as_str()).map(|c| c.chars().take(8).collect()));
    let flag = |d: &Value, k: &str| d.get(k).and_then(|b| b.as_bool()) == Some(true);
    for d in deps {
        if flag(d, "booted") && out.booted_version.is_none() {
            out.booted_version = name(d);
        }
        if flag(d, "staged") && !out.staged {
            out.staged = true;
            out.staged_version = name(d);
        }
        // The previous Rare OS version: the first other Rare OS deployment (os-admin rollback's pick).
        if !flag(d, "booted") && !flag(d, "staged") && out.previous_version.is_none() && is_rare(d) {
            out.previous_version = name(d);
        }
    }
    // The same version again (a fresh install's first start changed only kernel arguments): there
    // is nothing older to go back to, and "Go back to <the version running now>" read as a fault.
    if out.previous_version.is_some() && out.previous_version == out.booted_version {
        out.previous_version = None;
    }
    out
}

fn is_rare(d: &Value) -> bool {
    d.get("container-image-reference").or_else(|| d.get("origin")).and_then(|r| r.as_str()).is_some_and(|r| r.contains("rare-os"))
}

fn booted_checksum(json: &str) -> Option<String> {
    let v = serde_json::from_str::<Value>(json).ok()?;
    v.get("deployments")?.as_array()?.iter().find(|d| d.get("booted").and_then(|b| b.as_bool()) == Some(true))?.get("checksum")?.as_str().map(String::from)
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct StateFile {
    checked_at: Option<u64>,
    ok: Option<bool>,
    error: Option<String>,
    phase: Option<String>,
    result: Option<String>,
    updated_at: Option<u64>,
    bytes: Option<u64>,
    bytes_total: Option<u64>,
    offer_version: Option<String>,
    wait_reason: Option<String>,
    notes: Option<String>,
}

/// Fills the last-check fields from the updater's state file contents (as read at `now`).
pub fn with_state_at(mut u: OsUpdate, state: Option<&str>, now: u64) -> OsUpdate {
    let Some(s) = state.and_then(|s| serde_json::from_str::<StateFile>(s).ok()) else { return u };
    u.checked_at = s.checked_at;
    u.check_ok = s.ok;
    u.check_error = s.error.filter(|e| !e.is_empty());
    u.result = s.result.filter(|r| !r.is_empty());
    let fresh = s.updated_at.is_some_and(|t| now.saturating_sub(t) <= STALE_SECS);
    match s.phase.as_deref() {
        Some(p @ ("checking" | "downloading")) if fresh => {
            u.busy = Some(p.to_string());
            u.bytes = s.bytes;
            u.bytes_total = s.bytes_total.filter(|t| *t > 0);
            u.offer_version = s.offer_version.filter(|v| !v.is_empty());
        }
        Some("checking" | "downloading") => {
            u.stopped = true;
            u.bytes = s.bytes;
            u.bytes_total = s.bytes_total.filter(|t| *t > 0);
        }
        Some("waiting") => u.waiting = s.wait_reason.filter(|w| !w.is_empty()),
        _ => {}
    }
    if u.result.as_deref() == Some("stopped") {
        u.stopped = true;
    }
    if u.staged {
        u.notes = s.notes.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    }
    u
}

pub fn with_state(u: OsUpdate, state: Option<&str>) -> OsUpdate {
    with_state_at(u, state, now_secs())
}

/// The version greenboot rolled back from (reason "failed"), while it is recent and not running.
pub fn fell_back(skip: Option<&str>, booted_checksum: Option<&str>, now: u64) -> Option<String> {
    let v = serde_json::from_str::<Value>(skip?).ok()?;
    if v.get("reason").and_then(|r| r.as_str()) != Some("failed") {
        return None;
    }
    let at = v.get("at").and_then(|a| a.as_u64())?;
    if now.saturating_sub(at) > FELL_BACK_SECS {
        return None;
    }
    let sum = v.get("checksum").and_then(|c| c.as_str());
    if sum.is_some() && sum == booted_checksum {
        return None;
    }
    Some(v.get("version").and_then(|x| x.as_str()).filter(|x| !x.is_empty()).unwrap_or("The new version").to_string())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn status() -> OsUpdate {
    let json = crate::system::run("rpm-ostree", &["status", "--json"]).unwrap_or_default();
    let now = now_secs();
    let mut u = with_state_at(parse_status(&json), std::fs::read_to_string(STATE_FILE).ok().as_deref(), now);
    u.fell_back_from = fell_back(std::fs::read_to_string(SKIP_FILE).ok().as_deref(), booted_checksum(&json).as_deref(), now);
    // An image from before `os-admin rollback` existed cannot go back from Settings.
    if !std::fs::read_to_string(ADMIN).is_ok_and(|t| t.contains("rollback_cmd")) {
        u.previous_version = None;
    }
    u
}

/// Ask the updater to download now (also on battery, down to 15%). Returns at once; the download
/// goes on in the background, its progress and the staged version show up in `status`.
pub fn check_now() -> Result<(), String> {
    admin(&["update-now"])
}

/// Make the previous Rare OS version the next start (and restart with `reboot`).
pub fn rollback(reboot: bool) -> Result<String, String> {
    if reboot {
        admin(&["rollback", "--reboot"]).map(|_| "Restarting into the previous version.".into())
    } else {
        admin(&["rollback"]).map(|_| "The next start is the previous version.".into())
    }
}

/// A system from before Rare OS exists to go back to (`rare-undo --check`, no root needed).
pub fn restore_available() -> bool {
    crate::system::run("/usr/bin/rare-undo", &["--check"]).is_ok()
}

/// Make the previous system the next start. With `reboot` it restarts at once.
pub fn restore(reboot: bool) -> Result<String, String> {
    if !restore_available() {
        return Err("There is no previous system on this device.".into());
    }
    if reboot {
        admin(&["undo", "--reboot"]).map(|_| "Restarting into the previous system.".into())
    } else {
        admin(&["undo"]).map(|_| "The next start is the previous system.".into())
    }
}

pub fn boot_menu_next() -> Result<(), String> {
    admin(&["boot-menu-once"])
}

fn admin(args: &[&str]) -> Result<(), String> {
    let mut a = vec!["--disable-internal-agent", ADMIN];
    a.extend_from_slice(args);
    crate::system::run("pkexec", &a).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_and_booted() {
        let j = r#"{"deployments":[
          {"booted":false,"staged":true,"version":"43.20261030.0","checksum":"aaaaaaaaaaaa"},
          {"booted":true,"version":"43.20261020.0","checksum":"bbbbbbbbbbbb"},
          {"booted":false,"version":"bazzite","checksum":"cccccccccccc"}]}"#;
        let u = parse_status(j);
        assert!(u.staged);
        assert_eq!(u.staged_version.as_deref(), Some("43.20261030.0"));
        assert_eq!(u.booted_version.as_deref(), Some("43.20261020.0"));
    }

    #[test]
    fn nothing_staged_and_no_version() {
        let u = parse_status(r#"{"deployments":[{"booted":true,"checksum":"0123456789abcdef"}]}"#);
        assert!(!u.staged);
        assert_eq!(u.booted_version.as_deref(), Some("01234567"));
    }

    #[test]
    fn bad_json_is_empty() {
        assert_eq!(parse_status("not json"), OsUpdate::default());
        assert_eq!(parse_status("{}"), OsUpdate::default());
    }

    #[test]
    fn previous_rare_version_skips_the_pinned_system() {
        let j = r#"{"deployments":[
          {"booted":true,"version":"0.1.0-b","checksum":"b","container-image-reference":"ostree-image-signed:docker://ghcr.io/example/rare-os:stable"},
          {"booted":false,"pinned":true,"version":"42.1","checksum":"c","container-image-reference":"ostree-image-signed:docker://ghcr.io/ublue-os/bazzite-deck:stable"},
          {"booted":false,"version":"0.1.0-a","checksum":"a","container-image-reference":"ostree-unverified-image:oci-archive:/var/tmp/rare-os-a.ociarchive"}]}"#;
        assert_eq!(parse_status(j).previous_version.as_deref(), Some("0.1.0-a"));
        let only = r#"{"deployments":[{"booted":true,"version":"1","container-image-reference":"rare-os"},{"version":"42","container-image-reference":"bazzite"}]}"#;
        assert_eq!(parse_status(only).previous_version, None);
        // a fresh install after its first start's kernel-argument change: the same version twice
        let same = r#"{"deployments":[{"booted":true,"version":"0.1.0-a","checksum":"b","container-image-reference":"rare-os"},{"version":"0.1.0-a","checksum":"a","container-image-reference":"rare-os"}]}"#;
        assert_eq!(parse_status(same).previous_version, None);
    }

    #[test]
    fn progress_waiting_and_a_stopped_run() {
        let now = 1_000_000;
        let s = r#"{"checkedAt":1,"ok":true,"phase":"downloading","updatedAt":999990,"bytes":2000,"bytesTotal":8000,"offerVersion":"0.2"}"#;
        let u = with_state_at(OsUpdate::default(), Some(s), now);
        assert_eq!((u.busy.as_deref(), u.bytes, u.bytes_total, u.offer_version.as_deref(), u.stopped), (Some("downloading"), Some(2000), Some(8000), Some("0.2"), false));
        let u = with_state_at(OsUpdate::default(), Some(s), now + 600);
        assert!(u.busy.is_none() && u.stopped);
        let w = with_state_at(OsUpdate::default(), Some(r#"{"phase":"waiting","waitReason":"battery","result":"waiting"}"#), now);
        assert_eq!(w.waiting.as_deref(), Some("battery"));
        let staged = OsUpdate { staged: true, ..Default::default() };
        let r = with_state_at(staged, Some(r#"{"phase":"ready","result":"staged","notes":"  Faster start.\n"}"#), now);
        assert_eq!(r.notes.as_deref(), Some("Faster start."));
    }

    #[test]
    fn fell_back_only_for_a_recent_failed_version_not_running() {
        let f = r#"{"reason":"failed","version":"0.2","checksum":"new","at":1000}"#;
        assert_eq!(fell_back(Some(f), Some("old"), 2000).as_deref(), Some("0.2"));
        assert_eq!(fell_back(Some(f), Some("new"), 2000), None);
        assert_eq!(fell_back(Some(f), Some("old"), 1000 + 15 * 86400), None);
        assert_eq!(fell_back(Some(r#"{"reason":"rolled-back","version":"0.2","at":1000}"#), Some("old"), 2000), None);
    }

    #[test]
    fn state_file() {
        let u = with_state(OsUpdate::default(), Some(r#"{"checkedAt":1700000000,"ok":false,"error":"offline"}"#));
        assert_eq!(u.checked_at, Some(1700000000));
        assert_eq!(u.check_ok, Some(false));
        assert_eq!(u.check_error.as_deref(), Some("offline"));
        let u = with_state(OsUpdate::default(), Some(r#"{"checkedAt":1,"ok":true,"error":null}"#));
        assert_eq!(u.check_error, None);
        assert_eq!(with_state(OsUpdate::default(), None), OsUpdate::default());
    }
}
