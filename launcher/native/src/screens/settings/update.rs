//! OS updates, launcher side (lib/osupdate.ts pure helpers) and Settings > System's update rows
//! (screens/settings/SystemUpdateRows.tsx).

use super::{config::Config, rows::*, SectionOut, SectionView};
use crate::screens::overlay::common::*;
use rare_i18n::{tr, trf, trn};
use rare_render::Id;

/// What the native side reports (osupdate.rs).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub struct OsUpdate {
    pub booted_version: Option<String>,
    pub staged: bool,
    pub staged_version: Option<String>,
    /// Unix seconds.
    pub checked_at: Option<f64>,
    pub check_ok: Option<bool>,
    pub check_error: Option<String>,
    /// "checking" or "downloading" while the updater is at work (its heartbeat is fresh).
    pub busy: Option<String>,
    /// Download progress in bytes (layers already on the device count as done).
    pub bytes: Option<u64>,
    pub bytes_total: Option<u64>,
    /// The version being downloaded.
    pub offer_version: Option<String>,
    /// Why an automatic download waits: battery, game, metered.
    pub waiting: Option<String>,
    /// The last run's result: current, staged, waiting, failed, skipped, unsupported, stopped.
    pub result: Option<String>,
    /// The last run stopped before it finished.
    pub stopped: bool,
    /// Release notes of the downloaded version.
    pub notes: Option<String>,
    /// The previous Rare OS version, to go back to.
    pub previous_version: Option<String>,
    /// A version that did not start, so the device went back by itself.
    pub fell_back_from: Option<String>,
}

impl OsUpdate {
    pub fn from_sys(u: rare_system::osupdate::OsUpdate) -> OsUpdate {
        OsUpdate {
            booted_version: u.booted_version,
            staged: u.staged,
            staged_version: u.staged_version,
            checked_at: u.checked_at.map(|c| c as f64),
            check_ok: u.check_ok,
            check_error: u.check_error,
            busy: u.busy,
            bytes: u.bytes,
            bytes_total: u.bytes_total,
            offer_version: u.offer_version,
            waiting: u.waiting,
            result: u.result,
            stopped: u.stopped,
            notes: u.notes,
            previous_version: u.previous_version,
            fell_back_from: u.fell_back_from,
        }
    }
}

/// "2.1 GB": a download size in plain units.
pub fn size_text(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        crate::kit::format::in_gb(format!("{:.1}", b / 1e9))
    } else if b >= 1e6 {
        crate::kit::format::in_mb(format!("{:.0}", b / 1e6))
    } else {
        crate::kit::format::in_kb(format!("{:.0}", (b / 1e3).max(1.0)))
    }
}

/// "3 hours ago", "yesterday": when the last check ran, in plain words.
pub fn ago_text(at_sec: Option<f64>, now_ms: f64) -> String {
    let Some(at) = at_sec.filter(|a| *a != 0.0) else { return tr().settings_more.upd_not_yet.into() };
    let s = (now_ms / 1000.0 - at).max(0.0).floor();
    if s < 90.0 {
        return tr().common.just_now.to_lowercase();
    }
    let m = (s / 60.0).round();
    if m < 60.0 {
        return trn!(tr().common.minutes_ago, m as u64);
    }
    let h = (s / 3600.0).round();
    if h < 24.0 {
        return trn!(tr().common.hours_ago, h as u64);
    }
    let d = (s / 86400.0).round();
    if d == 1.0 {
        tr().settings_more.upd_yesterday.into()
    } else {
        trn!(tr().common.days_ago, d as u64)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateLine {
    pub title: String,
    pub body: String,
    /// The updater's own error words behind Details, when the body says it in plain words instead.
    pub detail: Option<String>,
}

/// The generic words for a failed check (English; the screen reads `upd_check_failed`).
pub const CHECK_FAILED: &str = "Rare OS could not check for updates. It tries again by itself.";

/// A failed check in plain words, and the raw error to keep behind Details. os-update's own
/// sentences for free space and battery stay as they are; a bad signature, the clock or a sign-in
/// page, a busy server, a dropped download, another system change and no network each say so,
/// from os-update's words or from bootc's own (an older image's os-update passes those through);
/// anything else gets the generic words.
pub fn failed_words(err: Option<&str>) -> (String, Option<String>) {
    let t = &tr().settings_more;
    let Some(e) = err.map(str::trim).filter(|e| !e.is_empty()) else { return (t.upd_check_failed.into(), None) };
    let l = e.to_lowercase();
    if l.starts_with("there is not enough free space") || l.starts_with("the battery is at") {
        let body = if l.contains("try again") || l.contains("tries again") { e.to_string() } else { trf!(t.upd_retry_suffix, error = e) };
        return (body, None);
    }
    let kinds: [(&[&str], &str); 6] = [
        (&["signature", "source image rejected", "rejected by policy"], t.upd_signature),
        (&["x509", "certificate", "date and time are right"], t.upd_cert),
        (&["busy right now", "too many requests", " 429", " 500 ", " 502 ", " 503 ", " 504 ", "internal server error", "bad gateway", "service unavailable"], t.upd_busy),
        (&["unexpected eof", "connection reset", "broken pipe", "connection to the update server dropped"], t.upd_dropped),
        (&["another system change", "sysroot lock", "acquire lock", "transaction in progress", "another operation"], t.upd_lock),
        (&["offline", "not connected to the internet", "could not be reached", "resolv", "temporary failure", "network is unreachable", "network is down", "no such host", "dial tcp", "no route", "dns", "timed out", "timeout", "connection refused"], t.upd_offline),
    ];
    for (words, body) in kinds {
        if words.iter().any(|w| l.contains(w)) {
            return (body.into(), Some(e.to_string()));
        }
    }
    (t.upd_check_failed.into(), Some(e.to_string()))
}

/// The Updates row's words for a state. `checking` is a check started from this launcher.
pub fn update_line(u: &OsUpdate, checking: bool, now_ms: f64) -> UpdateLine {
    let t = &tr().settings_more;
    if u.staged {
        return UpdateLine {
            title: match &u.staged_version {
                Some(v) => trf!(t.upd_ready_to, version = v),
                None => t.upd_ready.into(),
            },
            body: t.upd_ready_body.into(),
            detail: None,
        };
    }
    if u.busy.as_deref() == Some("downloading") {
        let body = match (u.bytes, u.bytes_total) {
            (Some(b), Some(tot)) if tot > 0 => {
                let pct = ((b as f64 / tot as f64) * 100.0).clamp(0.0, 100.0).floor();
                trf!(t.upd_progress, done = size_text(b.min(tot)), total = size_text(tot), pct = pct)
            }
            _ => t.upd_downloads_bg.into(),
        };
        return UpdateLine { title: match &u.offer_version {
                Some(v) => trf!(t.upd_downloading, version = v),
                None => t.upd_downloading_generic.into(),
            },
            body, detail: None };
    }
    if u.busy.is_some() || checking {
        return UpdateLine { title: t.upd_checking.into(), body: t.upd_checking_body.into(), detail: None };
    }
    if let Some(w) = &u.waiting {
        let body = match w.as_str() {
            "battery" => t.upd_wait_battery,
            "game" => t.upd_wait_game,
            "metered" => t.upd_wait_metered,
            _ => t.upd_wait_soon,
        };
        return UpdateLine { title: t.upd_waiting.into(), body: body.into(), detail: None };
    }
    if u.stopped {
        let body = match (u.bytes, u.bytes_total) {
            (Some(b), Some(tot)) if tot > 0 && b > 0 => trf!(t.upd_stopped_partial, done = size_text(b.min(tot)), total = size_text(tot)),
            _ => t.upd_stopped_body.to_string(),
        };
        return UpdateLine {
            title: t.upd_stopped.into(),
            body,
            detail: None,
        };
    }
    match u.result.as_deref() {
        Some("skipped") => {
            return UpdateLine { title: t.upd_up_to_date.into(), body: u.check_error.clone().unwrap_or_else(|| t.upd_skipped.into()), detail: None };
        }
        Some("unsupported") => {
            return UpdateLine { title: t.upd_unsupported.into(), body: u.check_error.clone().unwrap_or_else(|| t.upd_no_source.into()), detail: None };
        }
        _ => {}
    }
    if u.check_ok == Some(false) {
        let (body, detail) = failed_words(u.check_error.as_deref());
        return UpdateLine { title: t.upd_could_not.into(), body, detail };
    }
    let body = match u.checked_at.filter(|a| *a != 0.0) {
        Some(_) => trf!(t.upd_uptodate_checked, ago = ago_text(u.checked_at, now_ms)),
        None => t.upd_uptodate_unchecked.to_string(),
    };
    UpdateLine { title: t.upd_up_to_date.into(), body, detail: None }
}

/// The staged version to announce once per launch, or None (nothing staged, or already announced).
pub fn to_announce(u: &OsUpdate, announced: Option<&str>) -> Option<String> {
    if !u.staged {
        return None;
    }
    let key = u.staged_version.clone().unwrap_or_else(|| "staged".into());
    if Some(key.as_str()) == announced {
        None
    } else {
        Some(key)
    }
}

/// A check we started is done when the updater wrote a newer time (or reported a failure).
pub fn check_done(u: &OsUpdate, started_sec: f64) -> bool {
    u.checked_at.is_some_and(|c| c >= started_sec)
}

/// What Settings > System shows besides the update state (the device's name and version).
#[derive(Clone, Debug)]
pub struct SystemInfo {
    pub os_name: String,
    pub os_version: String,
    pub update: OsUpdate,
    pub checking: bool,
    pub can_restore: bool,
    pub menu_set: bool,
}

/// A state picked by a `settings-update-<state>` fixture (screenshots of each update state).
static FIXTURE_STATE: std::sync::Mutex<Option<SystemInfo>> = std::sync::Mutex::new(None);

/// Sets the update state fixture_info returns: downloading, checking, waiting-battery,
/// waiting-game, stopped, ready, ready-long, fellback, failed, unsupported, skipped. False for an
/// unknown name.
pub fn set_fixture_state(name: &str) -> bool {
    let base = OsUpdate { booted_version: Some("0.1.41-9ecd9df".into()), checked_at: Some(1_800_000_000.0 - 7200.0), check_ok: Some(true), previous_version: Some("0.1.40-a62daaa".into()), ..Default::default() };
    let long_notes = "Updates download in the background on battery too, and show their progress in Settings.\nIf a new version does not start, the device goes back to the previous one by itself.\nSettings > System can go back to the previous version.\nRemote access starts off after a switch from GameOS, and the default password no longer signs in.\nA very long line about a change that keeps going to check that the row wraps instead of running off the side of the screen on the smaller 1280 by 800 display.";
    let u = match name {
        "downloading" => OsUpdate { busy: Some("downloading".into()), bytes: Some(2_143_000_000), bytes_total: Some(7_912_000_000), offer_version: Some("0.1.42-5f0c1e2".into()), ..base },
        "checking" => OsUpdate { busy: Some("checking".into()), ..base },
        "waiting-battery" => OsUpdate { waiting: Some("battery".into()), result: Some("waiting".into()), ..base },
        "waiting-game" => OsUpdate { waiting: Some("game".into()), result: Some("waiting".into()), ..base },
        "stopped" => OsUpdate { stopped: true, bytes: Some(3_400_000_000), bytes_total: Some(7_912_000_000), ..base },
        "ready" => OsUpdate { staged: true, staged_version: Some("0.1.42-5f0c1e2".into()), notes: Some("Faster start.\nNew Settings page.".into()), ..base },
        "ready-long" => OsUpdate { staged: true, staged_version: Some("0.1.42-5f0c1e2-with-a-long-build-name".into()), notes: Some(long_notes.into()), ..base },
        "fellback" => OsUpdate { fell_back_from: Some("0.1.42-5f0c1e2".into()), result: Some("current".into()), ..base },
        "failed" => OsUpdate { check_ok: Some(false), result: Some("failed".into()), check_error: Some("There is not enough free space for the update. Free some space and try again.".into()), ..base },
        "unsupported" => OsUpdate { check_ok: Some(false), result: Some("unsupported".into()), check_error: Some("This device runs a build installed from a file, so it has no update source yet.".into()), ..base },
        "skipped" => OsUpdate { result: Some("skipped".into()), check_error: Some("Version 0.1.42-5f0c1e2 did not start on this device, so it is skipped. The next version installs by itself.".into()), ..base },
        _ => return false,
    };
    let mut info = default_fixture_info();
    info.update = u;
    if let Ok(mut g) = FIXTURE_STATE.lock() {
        *g = Some(info);
    }
    true
}

pub fn fixture_info() -> SystemInfo {
    if let Some(i) = FIXTURE_STATE.lock().ok().and_then(|g| g.clone()) {
        return i;
    }
    default_fixture_info()
}

fn default_fixture_info() -> SystemInfo {
    SystemInfo {
        os_name: "Rare OS".into(),
        os_version: "43.1".into(),
        update: OsUpdate { booted_version: Some("43.1".into()), checked_at: Some(1_800_000_000.0 - 7200.0), check_ok: Some(true), ..Default::default() },
        checking: false,
        can_restore: true,
        menu_set: false,
    }
}

/// The wall clock the fixtures read (fixed so goldens do not drift).
pub const FIXTURE_NOW_MS: f64 = 1_800_000_000_000.0;

pub struct UpdateRows {
    pub panel: Id,
    check_btn: Id,
    info: SystemInfo,
    /// The update row's body, its plain words, the raw error behind Details and the Details label.
    details: Option<Details>,
}

struct Details {
    body: Id,
    plain: String,
    raw: String,
    label: Id,
    shown: bool,
}

/// The last failed check's raw error already in the log (the rows rebuild often).
static LOGGED_ERROR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn log_check_error(e: &str) {
    if let Ok(mut g) = LOGGED_ERROR.lock() {
        if g.as_deref() != Some(e) {
            log::warn!("[settings] update check failed: {e}");
            *g = Some(e.to_string());
        }
    }
}

impl UpdateRows {
    pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, info: SystemInfo, now_ms: f64) -> UpdateRows {
        let t = &tr().settings_more;
        let panel = panel(cx, parent);
        let title = format!("{} {}", info.os_name, info.update.booted_version.clone().unwrap_or(info.os_version.clone()));
        setting_row(cx, panel, title.trim(), Some(t.upd_running), true);
        if let Some(v) = &info.update.fell_back_from {
            let body = trf!(t.upd_fell_back_body, version = v);
            setting_row(cx, panel, t.upd_fell_back, Some(&body), false);
        }
        let busy = info.checking || info.update.busy.is_some();
        let line = update_line(&info.update, info.checking, now_ms);
        let row_ = setting_row(cx, panel, &line.title, Some(&line.body), false);
        let details = match (&line.detail, row_.body) {
            (Some(raw), Some(body)) => {
                log_check_error(raw);
                let b = button(cx, row_.right, t.upd_details, Variant::Secondary, Sz::Md, false).hint(t.upd_show_details).sid("update-details").tag(6);
                let label = match b.kind {
                    Kind::Btn { text, .. } => text,
                    _ => b.id,
                };
                nav.add(b.group(10));
                Some(Details { body, plain: line.body.clone(), raw: raw.clone(), label, shown: false })
            }
            _ => None,
        };
        let check_btn = if info.update.staged {
            let b = button(cx, row_.right, t.os_restart_now, Variant::Primary, Sz::Md, false).hint(t.upd_restart_hint).sid("updates").tag(1);
            nav.add(b.group(10))
        } else if info.update.result.as_deref() == Some("unsupported") && !busy {
            // No update source: Update now could only fail the same way. The row says why.
            row_.right
        } else {
            let label = match info.update.busy.as_deref() {
                Some("downloading") => tr().common.downloading,
                _ if busy => t.upd_checking_btn,
                _ => t.upd_now,
            };
            let b = button(cx, row_.right, label, if busy { Variant::Dimmed } else { Variant::Secondary }, Sz::Md, false).hint(t.upd_check_hint).sid("updates").tag(2);
            nav.add(b.group(10))
        };
        if let (true, Some(notes)) = (info.update.staged, &info.update.notes) {
            setting_row(cx, panel, t.upd_whats_new, Some(notes), false);
        }
        if let Some(v) = &info.update.previous_version {
            let body = trf!(t.upd_prev_body, version = v);
            let r = setting_row(cx, panel, t.upd_prev, Some(&body), false);
            let b = button(cx, r.right, t.upd_go_back, Variant::Secondary, Sz::Md, false).hint(t.upd_go_back_hint).sid("rollback").tag(5);
            nav.add(b.group(10));
        }
        if info.can_restore {
            let r = setting_row(cx, panel, t.os_restore_title, Some(t.upd_restore_body), false);
            let b = button(cx, r.right, t.upd_restore, Variant::Secondary, Sz::Md, false).hint(t.os_restore_title).sid("restore-system").tag(3);
            nav.add(b.group(10));
        }
        let body = if info.menu_set {
            t.upd_boot_set
        } else {
            t.upd_boot_body
        };
        let r = setting_row(cx, panel, t.upd_boot, Some(body), false);
        let b = button(cx, r.right, if info.menu_set { t.upd_boot_btn_set } else { t.upd_boot_btn }, if info.menu_set { Variant::Dimmed } else { Variant::Secondary }, Sz::Md, false).hint(t.upd_boot_hint).sid("boot-menu").tag(4);
        nav.add(b.group(10));
        UpdateRows { panel, check_btn, info, details }
    }

    /// A on Details shows or hides the updater's own error under the plain words. True when it was Details.
    pub fn press_details(&mut self, cx: &mut Cx, nav: &mut Nav) -> bool {
        if !nav.current().is_some_and(|f| f.tag == 6 && f.sid == "update-details") {
            return false;
        }
        let Some(d) = self.details.as_mut() else { return false };
        d.shown = !d.shown;
        let body = if d.shown { format!("{}\n{}", d.plain, d.raw) } else { d.plain.clone() };
        cx.scene.set_text(cx.t, d.body, &body);
        cx.scene.set_text(cx.t, d.label, if d.shown { tr().settings_more.upd_hide_details } else { tr().settings_more.upd_details });
        if let Some(f) = nav.items.iter_mut().find(|f| f.tag == 6 && f.sid == "update-details") {
            f.hint = if d.shown { tr().settings_more.upd_hide_details.into() } else { tr().settings_more.upd_show_details.into() };
        }
        true
    }

    /// A on one of the update buttons: the host action, or None.
    pub fn press(&self, nav: &Nav) -> Option<&'static str> {
        let f = nav.current()?;
        if f.id == self.check_btn && !self.info.update.staged && (self.info.checking || self.info.update.busy.is_some()) {
            return None;
        }
        match f.tag {
            1 => Some("os:restart-open"),
            2 => Some("os:check"),
            3 => Some("os:restore-open"),
            4 if !self.info.menu_set => Some("os:boot-menu-next"),
            5 => Some("os:rollback-ask"),
            _ => None,
        }
    }
}

pub struct UpdateSection {
    rows: UpdateRows,
}

impl UpdateSection {
    pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, _cfg: &Config, _now: f64) -> UpdateSection {
        let (info, now_ms) = match super::live::get() {
            Some(s) => (s.system, crate::app::wall_ms()),
            None => (fixture_info(), FIXTURE_NOW_MS),
        };
        UpdateSection { rows: UpdateRows::build(cx, nav, parent, info, now_ms) }
    }
}

impl SectionView for UpdateSection {
    fn press(&mut self, cx: &mut Cx, nav: &mut Nav, _cfg: &mut Config, _now: f64) -> SectionOut {
        if self.rows.press_details(cx, nav) {
            return SectionOut::None;
        }
        match self.rows.press(nav) {
            Some(a) => SectionOut::Act(a.into()),
            None => SectionOut::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_800_000_000_000.0;
    const SEC: f64 = NOW / 1000.0;

    #[test]
    fn ago_says_it_in_plain_words() {
        assert_eq!(ago_text(None, NOW), "not yet");
        assert_eq!(ago_text(Some(SEC - 20.0), NOW), "just now");
        assert_eq!(ago_text(Some(SEC - 600.0), NOW), "10 minutes ago");
        assert_eq!(ago_text(Some(SEC - 3600.0), NOW), "1 hour ago");
        assert_eq!(ago_text(Some(SEC - 3.0 * 3600.0), NOW), "3 hours ago");
        assert_eq!(ago_text(Some(SEC - 30.0 * 3600.0), NOW), "yesterday");
        assert_eq!(ago_text(Some(SEC - 5.0 * 86400.0), NOW), "5 days ago");
    }

    #[test]
    fn staged_wins_over_everything() {
        let u = OsUpdate { staged: true, staged_version: Some("43.2".into()), check_ok: Some(false), ..Default::default() };
        assert_eq!(update_line(&u, true, NOW).title, "Ready. Restart to update to 43.2");
    }

    #[test]
    fn checking_failed_up_to_date() {
        assert_eq!(update_line(&OsUpdate::default(), true, NOW).title, "Checking for updates");
        let failed = OsUpdate { check_ok: Some(false), check_error: Some("offline".into()), ..Default::default() };
        assert_eq!(update_line(&failed, false, NOW).body, "No internet connection. It tries again by itself.");
        let ok = update_line(&OsUpdate { check_ok: Some(true), checked_at: Some(SEC - 7200.0), ..Default::default() }, false, NOW);
        assert_eq!(ok.title, "Rare OS is up to date");
        assert!(ok.body.contains("2 hours ago"));
    }

    #[test]
    fn progress_waiting_and_stopped_say_what_is_going_on() {
        let d = OsUpdate { busy: Some("downloading".into()), bytes: Some(2_100_000_000), bytes_total: Some(7_900_000_000), offer_version: Some("0.2".into()), ..Default::default() };
        let l = update_line(&d, false, NOW);
        assert_eq!(l.title, "Downloading Rare OS 0.2");
        assert!(l.body.starts_with("2.1 GB of 7.9 GB (26%)"), "{}", l.body);
        let w = update_line(&OsUpdate { waiting: Some("battery".into()), ..Default::default() }, false, NOW);
        assert!(w.body.contains("30%"));
        let s = update_line(&OsUpdate { stopped: true, bytes: Some(1_000_000_000), bytes_total: Some(4_000_000_000), ..Default::default() }, false, NOW);
        assert_eq!(s.title, "The update stopped before it finished");
        assert!(s.body.contains("1.0 GB of 4.0 GB"));
        assert_eq!(update_line(&OsUpdate { busy: Some("checking".into()), ..Default::default() }, false, NOW).title, "Checking for updates");
    }

    #[test]
    fn uses_no_dashes_in_its_words() {
        for u in [
            OsUpdate::default(),
            OsUpdate { staged: true, ..Default::default() },
            OsUpdate { check_ok: Some(false), ..Default::default() },
            OsUpdate { busy: Some("downloading".into()), bytes: Some(1), bytes_total: Some(2), ..Default::default() },
            OsUpdate { waiting: Some("game".into()), ..Default::default() },
            OsUpdate { waiting: Some("metered".into()), ..Default::default() },
            OsUpdate { stopped: true, ..Default::default() },
        ] {
            let l = update_line(&u, false, NOW);
            assert!(!(l.title + &l.body).contains(['\u{2014}', '\u{2013}']));
        }
    }

    #[test]
    fn a_failed_check_says_it_plainly_and_keeps_the_error_for_details() {
        let raw = "Fetching manifest: failed to invoke method OpenImage: A signature was required, but no signature exists";
        let l = update_line(&OsUpdate { check_ok: Some(false), check_error: Some(raw.into()), ..Default::default() }, false, NOW);
        assert_eq!(l.title, "Could not update");
        assert_eq!(l.body, "The update's signature could not be checked, so it was not installed. It tries again by itself.");
        assert_eq!(l.detail.as_deref(), Some(raw));
        let words = |e: &str| update_line(&OsUpdate { check_ok: Some(false), check_error: Some(e.into()), ..Default::default() }, false, NOW).body;
        assert!(words("tls: failed to verify certificate: x509: certificate has expired or is not yet valid").starts_with("The connection to the update server could not be verified."));
        assert_eq!(words("reading manifest stable: received unexpected HTTP status: 503 Service Unavailable"), "The update server is busy right now. It tries again by itself.");
        assert_eq!(words("Importing: fetching layer: unexpected EOF"), "The connection dropped during the download. It goes on from where it stopped.");
        assert_eq!(words("Another system change is still running. Try again in a few minutes."), "Another system change is still running. It tries again by itself.");
        assert_eq!(words("The device is not connected to the internet. Connect to Wi-Fi and try again."), "No internet connection. It tries again by itself.");
        // bootc's own words when Wi-Fi is up but DNS is not (an older os-update passes them on)
        assert_eq!(words("Creating importer: failed to invoke method OpenImage: pinging container registry ghcr.io: Get \"https://ghcr.io/v2/\": dial tcp: lookup ghcr.io: no such host"), "No internet connection. It tries again by itself.");
        // "clock" holds "lock": not another system change
        assert_eq!(words("Some new error about the clock"), CHECK_FAILED);
        let off = update_line(&OsUpdate { check_ok: Some(false), check_error: Some("The device is offline or the update server could not be reached.".into()), ..Default::default() }, false, NOW);
        assert_eq!(off.body, "No internet connection. It tries again by itself.");
        assert!(off.detail.is_some());
        let space = "There is not enough free space for the update. Free some space and try again.";
        let l = update_line(&OsUpdate { check_ok: Some(false), check_error: Some(space.into()), ..Default::default() }, false, NOW);
        assert_eq!((l.body.as_str(), l.detail), (space, None));
        let l = update_line(&OsUpdate { check_ok: Some(false), ..Default::default() }, false, NOW);
        assert_eq!((l.body.as_str(), l.detail), (CHECK_FAILED, None));
    }

    #[test]
    fn announces_a_staged_version_once_per_launch() {
        let u = OsUpdate { staged: true, staged_version: Some("43.2".into()), ..Default::default() };
        assert_eq!(to_announce(&u, None).as_deref(), Some("43.2"));
        assert_eq!(to_announce(&u, Some("43.2")), None);
        let u3 = OsUpdate { staged_version: Some("43.3".into()), ..u.clone() };
        assert_eq!(to_announce(&u3, Some("43.2")).as_deref(), Some("43.3"));
        assert_eq!(to_announce(&OsUpdate::default(), None), None);
    }

    #[test]
    fn a_check_is_done_with_a_newer_time() {
        assert!(!check_done(&OsUpdate { checked_at: Some(100.0), ..Default::default() }, 200.0));
        assert!(check_done(&OsUpdate { checked_at: Some(200.0), ..Default::default() }, 200.0));
        assert!(!check_done(&OsUpdate::default(), 0.0));
    }
}
