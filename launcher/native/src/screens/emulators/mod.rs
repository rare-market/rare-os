//! Settings > Emulators (screens/Emulators.tsx): what is installed, the newest official release,
//! and a one-press update. Nothing downloads until the user presses Update.

use crate::screens::overlay::common::*;
use crate::screens::settings::{config::Config, rows::*, SectionOut, SectionView};
use rare_render::Id;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct EmuInfo {
    pub id: String,
    pub name: String,
    /// None: not installed.
    pub path: Option<String>,
    /// Unix seconds.
    pub modified: Option<i64>,
    pub tag: Option<String>,
    /// "quiet", "balanced", "performance" or "custom".
    pub power: String,
}

#[derive(Clone, Debug, Default)]
pub struct EmuUpdateInfo {
    pub available: bool,
    pub tag: String,
    /// "checksum" or something else.
    pub verify: String,
}

fn power_label(p: &str) -> &'static str {
    match p {
        "quiet" => "Quiet",
        "performance" => "Performance",
        "custom" => "Custom",
        _ => "Balanced",
    }
}

/// toLocaleDateString(undefined, {year, month short, day}) in en-US: "Oct 27, 2026".
pub fn date_text(secs: Option<i64>) -> String {
    let Some(s) = secs.filter(|s| *s != 0) else { return String::new() };
    let days = s.div_euclid(86400);
    // Civil from days (Howard Hinnant).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {}, {}", MONTHS[(m - 1) as usize], d, y)
}

/// The row's line for an emulator (checked or not).
pub fn emu_body(e: &EmuInfo, u: Option<&EmuUpdateInfo>) -> String {
    match u {
        Some(u) if u.available => format!("{} is out{}. Installed {}.", u.tag, if u.verify == "checksum" { ", checksum published" } else { ", no checksum published" }, date_text(e.modified)),
        _ => format!("Installed {}{}. Starts on {} power.", date_text(e.modified), e.tag.as_ref().map(|t| format!(", {t}")).unwrap_or_default(), power_label(&e.power)),
    }
}

pub struct EmulatorsSection {
    list: Vec<EmuInfo>,
    found: Vec<Option<EmuUpdateInfo>>,
    busy: Option<String>,
}

impl EmulatorsSection {
    /// `list`: None while loading; `busy`: the emulator being checked or updated.
    pub fn build_with(cx: &mut Cx, nav: &mut Nav, parent: Id, list: Option<Vec<EmuInfo>>, found: &[(String, EmuUpdateInfo)], busy: Option<String>) -> EmulatorsSection {
        let p = panel(cx, parent);
        let installed: Vec<EmuInfo> = list.unwrap_or_default().into_iter().filter(|e| e.path.is_some()).collect();
        if installed.is_empty() {
            setting_row(cx, p, "No emulators found", Some("Put emulator AppImages in the Applications folder in your home folder and they show up here."), true);
        }
        let mut f_out = Vec::new();
        for (i, e) in installed.iter().enumerate() {
            let u = found.iter().find(|(id, _)| *id == e.id).map(|(_, u)| u.clone());
            let row_ = setting_row(cx, p, &e.name, Some(&emu_body(e, u.as_ref())), i == 0);
            let disabled = busy.is_some();
            let is_busy = busy.as_deref() == Some(e.id.as_str());
            let b = if u.as_ref().is_some_and(|u| u.available) {
                button(cx, row_.right, if is_busy { "Updating" } else { "Update" }, if disabled { Variant::Dimmed } else { Variant::Primary }, Sz::Md, false)
            } else {
                button(cx, row_.right, if is_busy { "Checking" } else { "Check for update" }, if disabled { Variant::Dimmed } else { Variant::Secondary }, Sz::Md, false)
            };
            nav.add(b.sid("emulators").tag(i as u32).group(10));
            f_out.push(u);
        }
        EmulatorsSection { list: installed, found: f_out, busy }
    }

    /// The emulators rare-emu finds in ~/Applications on the device; the fixture list otherwise.
    pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, _cfg: &Config, _now: f64) -> EmulatorsSection {
        match crate::screens::settings::live::get() {
            Some(s) => EmulatorsSection::build_with(cx, nav, parent, s.emu, &s.emu_found, s.emu_busy),
            None => EmulatorsSection::build_with(cx, nav, parent, Some(fixture_list()), &[], None),
        }
    }
}

impl SectionView for EmulatorsSection {
    fn press(&mut self, _cx: &mut Cx, nav: &mut Nav, _cfg: &mut Config, _now: f64) -> SectionOut {
        if self.busy.is_some() {
            return SectionOut::None;
        }
        let Some(i) = nav.current().map(|f| f.tag as usize) else { return SectionOut::None };
        let Some(e) = self.list.get(i) else { return SectionOut::None };
        if self.found.get(i).and_then(|u| u.as_ref()).is_some_and(|u| u.available) {
            SectionOut::Act(format!("emu:update:{}", e.id))
        } else {
            SectionOut::Act(format!("emu:check:{}", e.id))
        }
    }
}

pub fn fixture_list() -> Vec<EmuInfo> {
    let e = |id: &str, name: &str, path: Option<&str>, modified: i64, tag: Option<&str>, power: &str| EmuInfo { id: id.into(), name: name.into(), path: path.map(String::from), modified: Some(modified), tag: tag.map(String::from), power: power.into() };
    vec![
        e("rpcs3", "RPCS3", Some("/home/rare/Applications/rpcs3.AppImage"), 1_795_000_000, Some("v0.0.34"), "performance"),
        e("dolphin", "Dolphin", Some("/home/rare/Applications/dolphin.AppImage"), 1_790_000_000, None, "balanced"),
        e("cemu", "Cemu", None, 0, None, "balanced"),
    ]
}

/// Fixtures: `emulators-list`, `emulators-update` (an update found), `emulators-empty`.
pub fn fixture(name: &str, cx: &mut Cx, now: f64) -> Option<Box<dyn Screen>> {
    use crate::screens::settings::Settings;
    let _ = (&cx, now);
    let _ = Settings::fixture_with_emulators;
    match name {
        "emulators-list" | "emulators-update" | "emulators-empty" => Some(Box::new(Settings::fixture_with_emulators(cx, name, now))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_read_like_en_us() {
        assert_eq!(date_text(None), "");
        assert_eq!(date_text(Some(0)), "");
        assert_eq!(date_text(Some(1_700_000_000)), "Nov 14, 2023");
        assert_eq!(date_text(Some(1_709_251_200)), "Mar 1, 2024");
    }

    #[test]
    fn the_row_line() {
        let e = &fixture_list()[0];
        assert!(emu_body(e, None).ends_with("Starts on Performance power."));
        assert!(emu_body(e, None).contains(", v0.0.34."));
        let u = EmuUpdateInfo { available: true, tag: "v1".into(), verify: "checksum".into() };
        assert!(emu_body(e, Some(&u)).starts_with("v1 is out, checksum published. Installed "));
        let u = EmuUpdateInfo { verify: "none".into(), ..u };
        assert!(emu_body(e, Some(&u)).contains("no checksum published"));
    }
}
