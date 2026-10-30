//! A Steam game's DLC as Steam keeps it on disk: the DLC whose depots are installed
//! (`InstalledDepots/<depot>/dlcappid` in the game's appmanifest) and the DLC the player turned off
//! (`UserConfig/DisabledDLC` and `MountedConfig/DisabledDLC`, comma separated). Both mean owned:
//! Steam only installs or lets you turn off DLC you own. DLC with no depots of its own (unlocks)
//! is read from the account's licences (`appinfo::licensed`, packageinfo.vdf).
//!
//! Turning DLC on or off edits the appmanifest in place (`vdfedit`, byte preserving). Steam
//! rewrites its manifests while it runs, so the caller closes Steam first (rare-launch `dlc_set`).

use crate::steam::{find_roots, libraries_on_mounts, registered_libraries};
use crate::{vdf, vdfedit};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DlcState {
    /// DLC app ids with installed depots.
    pub installed: BTreeSet<u32>,
    /// DLC app ids the player turned off.
    pub disabled: BTreeSet<u32>,
    /// Every app id the account's licences cover (packageinfo.vdf): unlock-only DLC is only here.
    pub licensed: BTreeSet<u32>,
}

impl DlcState {
    pub fn owned(&self, dlc: u32) -> bool {
        self.installed.contains(&dlc) || self.disabled.contains(&dlc) || self.licensed.contains(&dlc)
    }

    /// Owned with files Steam can turn on or off (installed or turned off).
    pub fn switchable(&self, dlc: u32) -> bool {
        self.installed.contains(&dlc) || self.disabled.contains(&dlc)
    }
}

/// The game's appmanifest, in any Steam library this device knows.
pub fn manifest_path(appid: u32) -> Option<PathBuf> {
    let name = format!("appmanifest_{appid}.acf");
    let mut libs = Vec::new();
    for r in find_roots() {
        libs.extend(registered_libraries(&r.path));
    }
    libs.extend(libraries_on_mounts());
    libs.into_iter().map(|l| l.join("steamapps").join(&name)).find(|p| p.is_file())
}

fn ids(s: &str) -> impl Iterator<Item = u32> + '_ {
    s.split(',').filter_map(|x| x.trim().parse::<u32>().ok())
}

/// What an appmanifest's text says about DLC.
pub fn parse(text: &str) -> DlcState {
    let v = vdf::parse(text);
    let mut out = DlcState::default();
    let Some(st) = v.get("AppState") else { return out };
    if let Some(deps) = st.get("InstalledDepots") {
        for (_, d) in deps.entries() {
            if let Some(id) = d.str("dlcappid").and_then(|s| s.parse::<u32>().ok()) {
                out.installed.insert(id);
            }
        }
    }
    for block in ["UserConfig", "MountedConfig"] {
        if let Some(s) = st.get(block).and_then(|b| b.str("DisabledDLC")) {
            out.disabled.extend(ids(s));
        }
    }
    out
}

/// The DLC state of an installed Steam game (empty when it has no manifest).
pub fn state(appid: u32) -> DlcState {
    let mut s = manifest_path(appid).and_then(|p| fs::read_to_string(p).ok()).map(|t| parse(&t)).unwrap_or_default();
    let roots = find_roots();
    if let Some(r) = roots.iter().find(|r| r.primary).or(roots.first()) {
        s.licensed = crate::appinfo::licensed(&r.path).into_iter().collect();
    }
    s
}

/// The manifest text with `dlc` turned on or off in both config blocks.
pub fn edit(text: &str, dlc: u32, on: bool) -> Result<String, String> {
    let mut out = text.to_string();
    for block in ["UserConfig", "MountedConfig"] {
        let cur = vdfedit::get(&out, &["AppState", block, "DisabledDLC"])?.unwrap_or_default();
        let mut set: Vec<u32> = ids(&cur).collect();
        set.retain(|x| *x != dlc);
        if !on {
            set.push(dlc);
        }
        set.sort_unstable();
        set.dedup();
        let val = set.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");
        if val == cur {
            continue;
        }
        // Only touch MountedConfig when Steam already wrote it.
        if block == "MountedConfig" && !vdfedit::has(&out, &["AppState", block])? {
            continue;
        }
        out = vdfedit::set(&out, &["AppState", block, "DisabledDLC"], &val)?;
    }
    Ok(out)
}

/// Turns one DLC of `appid` on or off. Steam must be closed (it rewrites the file otherwise).
pub fn set_enabled(appid: u32, dlc: u32, on: bool) -> Result<(), String> {
    let path = manifest_path(appid).ok_or("This game is not installed.")?;
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let next = edit(&text, dlc, on)?;
    if next == text {
        return Ok(());
    }
    let tmp = path.with_extension("acf.rare-tmp");
    fs::write(&tmp, next).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACF: &str = "\"AppState\"\n{\n\t\"appid\"\t\t\"1245620\"\n\t\"InstalledDepots\"\n\t{\n\t\t\"1245621\"\n\t\t{\n\t\t\t\"manifest\"\t\t\"1\"\n\t\t}\n\t\t\"2778581\"\n\t\t{\n\t\t\t\"manifest\"\t\t\"2\"\n\t\t\t\"dlcappid\"\t\t\"2778580\"\n\t\t}\n\t}\n\t\"UserConfig\"\n\t{\n\t\t\"language\"\t\t\"english\"\n\t}\n\t\"MountedConfig\"\n\t{\n\t\t\"language\"\t\t\"english\"\n\t}\n}\n";

    #[test]
    fn reads_installed_and_turns_off_and_on() {
        let s = parse(ACF);
        assert!(s.installed.contains(&2778580) && s.disabled.is_empty());
        let off = edit(ACF, 2778580, false).unwrap();
        let s = parse(&off);
        assert!(s.disabled.contains(&2778580) && s.owned(2778580));
        assert!(off.contains("\"language\"\t\t\"english\""));
        let off2 = edit(&off, 3655690, false).unwrap();
        assert_eq!(parse(&off2).disabled.iter().copied().collect::<Vec<_>>(), vec![2778580, 3655690]);
        let on = edit(&off2, 2778580, true).unwrap();
        assert_eq!(parse(&on).disabled.iter().copied().collect::<Vec<_>>(), vec![3655690]);
    }
}
