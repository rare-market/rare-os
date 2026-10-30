//! OS updates, launcher side (lib/osupdate.ts pure helpers) and Settings > System's update rows
//! (screens/settings/SystemUpdateRows.tsx).

use super::{config::Config, rows::*, SectionOut, SectionView};
use crate::screens::overlay::common::*;
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
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else {
        format!("{:.0} KB", (b / 1e3).max(1.0))
    }
}

/// "3 hours ago", "yesterday": when the last check ran, in plain words.
pub fn ago_text(at_sec: Option<f64>, now_ms: f64) -> String {
    let Some(at) = at_sec.filter(|a| *a != 0.0) else { return "not yet".into() };
    let s = (now_ms / 1000.0 - at).max(0.0).floor();
    if s < 90.0 {
        return "just now".into();
    }
    let m = (s / 60.0).round();
    if m < 60.0 {
        return format!("{m} minutes ago");
    }
    let h = (s / 3600.0).round();
    if h < 24.0 {
        return if h == 1.0 { "1 hour ago".into() } else { format!("{h} hours ago") };
    }
    let d = (s / 86400.0).round();
    if d == 1.0 {
        "yesterday".into()
    } else {
        format!("{d} days ago")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateLine {
    pub title: String,
    pub body: String,
}

/// The Updates row's words for a state. `checking` is a check started from this launcher.
pub fn update_line(u: &OsUpdate, checking: bool, now_ms: f64) -> UpdateLine {
    if u.staged {
        return UpdateLine {
            title: match &u.staged_version {
                Some(v) => format!("Ready. Restart to update to {v}"),
                None => "Ready. Restart to update".into(),
            },
            body: "It is downloaded and installs while the device restarts. Restart now, or keep playing and do it later.".into(),
        };
    }
    let name = |v: &Option<String>| v.as_ref().map(|v| format!("Rare OS {v}")).unwrap_or_else(|| "the update".into());
    if u.busy.as_deref() == Some("downloading") {
        let body = match (u.bytes, u.bytes_total) {
            (Some(b), Some(t)) if t > 0 => {
                let pct = ((b as f64 / t as f64) * 100.0).clamp(0.0, 100.0).floor();
                format!("{} of {} ({pct}%). It downloads in the background, so you can keep playing.", size_text(b.min(t)), size_text(t))
            }
            _ => "It downloads in the background, so you can keep playing.".into(),
        };
        return UpdateLine { title: format!("Downloading {}", name(&u.offer_version)), body };
    }
    if u.busy.is_some() || checking {
        return UpdateLine { title: "Checking for updates".into(), body: "Looking for a new version. A download goes on in the background.".into() };
    }
    if let Some(w) = &u.waiting {
        let body = match w.as_str() {
            "battery" => "It downloads by itself on the charger, or when the battery is at 30% or more. Update now downloads it at 15% or more.",
            "game" => "It downloads by itself after you finish playing. Update now downloads it during the game.",
            "metered" => "It downloads by itself on a network that is not metered. Update now downloads it on this one.",
            _ => "It downloads by itself soon.",
        };
        return UpdateLine { title: "Update waiting to download".into(), body: body.into() };
    }
    if u.stopped {
        let done = match (u.bytes, u.bytes_total) {
            (Some(b), Some(t)) if t > 0 && b > 0 => format!(" {} of {} is already on the device.", size_text(b.min(t)), size_text(t)),
            _ => String::new(),
        };
        return UpdateLine {
            title: "The update stopped before it finished".into(),
            body: format!("It goes on from where it stopped within the hour, or now with Update now.{done}"),
        };
    }
    match u.result.as_deref() {
        Some("skipped") => {
            return UpdateLine { title: "Rare OS is up to date".into(), body: u.check_error.clone().unwrap_or_else(|| "A version that did not work on this device is skipped.".into()) };
        }
        Some("unsupported") => {
            return UpdateLine { title: "Updates are not set up on this device".into(), body: u.check_error.clone().unwrap_or_else(|| "This build has no update source.".into()) };
        }
        _ => {}
    }
    if u.check_ok == Some(false) {
        return UpdateLine {
            title: "Could not update".into(),
            body: {
                let e = u.check_error.clone().unwrap_or_else(|| "The last check did not finish.".into());
                // os-update's own words may say it already ("It tries again later.")
                if e.contains("tries again") { e } else { format!("{e} It tries again by itself.") }
            },
        };
    }
    let checked = match u.checked_at.filter(|a| *a != 0.0) {
        Some(_) => format!("Last checked {}.", ago_text(u.checked_at, now_ms)),
        None => "Not checked yet.".into(),
    };
    UpdateLine { title: "Rare OS is up to date".into(), body: format!("{checked} Updates download by themselves and install at the next restart.") }
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
}

impl UpdateRows {
    pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, info: SystemInfo, now_ms: f64) -> UpdateRows {
        let panel = panel(cx, parent);
        let title = format!("{} {}", info.os_name, info.update.booted_version.clone().unwrap_or(info.os_version.clone()));
        setting_row(cx, panel, title.trim(), Some("The version running now."), true);
        if let Some(v) = &info.update.fell_back_from {
            let body = format!("Rare OS {v} did not start properly, so the device went back to this version by itself. {v} does not install again; the next version does.");
            setting_row(cx, panel, "Back on the previous version", Some(&body), false);
        }
        let busy = info.checking || info.update.busy.is_some();
        let line = update_line(&info.update, info.checking, now_ms);
        let row_ = setting_row(cx, panel, &line.title, Some(&line.body), false);
        let check_btn = if info.update.staged {
            let b = button(cx, row_.right, "Restart now", Variant::Primary, Sz::Md, false).hint("Restart to update").sid("updates").tag(1);
            nav.add(b.group(10))
        } else if info.update.result.as_deref() == Some("unsupported") && !busy {
            // No update source: Update now could only fail the same way. The row says why.
            row_.right
        } else {
            let label = match info.update.busy.as_deref() {
                Some("downloading") => "Downloading",
                _ if busy => "Checking",
                _ => "Update now",
            };
            let b = button(cx, row_.right, label, if busy { Variant::Dimmed } else { Variant::Secondary }, Sz::Md, false).hint("Check for updates").sid("updates").tag(2);
            nav.add(b.group(10))
        };
        if let (true, Some(notes)) = (info.update.staged, &info.update.notes) {
            setting_row(cx, panel, "What's new", Some(notes), false);
        }
        if let Some(v) = &info.update.previous_version {
            let body = format!("Go back to Rare OS {v} if this version has a problem. Your games and files stay as they are.");
            let r = setting_row(cx, panel, "Previous version", Some(&body), false);
            let b = button(cx, r.right, "Go back", Variant::Secondary, Sz::Md, false).hint("Go back to the previous version").sid("rollback").tag(5);
            nav.add(b.group(10));
        }
        if info.can_restore {
            let r = setting_row(cx, panel, "Restore previous system", Some("Start the system this device ran before Rare OS. Rare OS stays installed and your files and games are shared."), false);
            let b = button(cx, r.right, "Restore", Variant::Secondary, Sz::Md, false).hint("Restore previous system").sid("restore-system").tag(3);
            nav.add(b.group(10));
        }
        let body = if info.menu_set {
            "The boot menu shows on the next start, once. Pick an older version there if an update misbehaves."
        } else {
            "Show the boot menu on the next start, once, to pick an older version or another system."
        };
        let r = setting_row(cx, panel, "Boot menu", Some(body), false);
        let b = button(cx, r.right, if info.menu_set { "Set for next start" } else { "Show next start" }, if info.menu_set { Variant::Dimmed } else { Variant::Secondary }, Sz::Md, false).hint("Boot menu next start").sid("boot-menu").tag(4);
        nav.add(b.group(10));
        UpdateRows { panel, check_btn, info }
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
    fn press(&mut self, _cx: &mut Cx, nav: &mut Nav, _cfg: &mut Config, _now: f64) -> SectionOut {
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
        assert!(update_line(&failed, false, NOW).body.contains("offline"));
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
