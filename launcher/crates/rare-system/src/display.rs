//! Display state for docked (TV or monitor) use: which DRM connectors are plugged in, which one
//! is lit, and what mode the panel or TV prefers. Read only, from /sys/class/drm. gamescope does
//! the switching itself (it prefers an external connector, applies --force-orientation only to
//! the internal panel, and picks the best connected output again on every hotplug); this only
//! reports what it did, so the launcher can say "Docked".

use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    /// Connector name as gamescope prints it: eDP-1, DP-1, HDMI-A-1.
    pub name: String,
    pub connected: bool,
    /// The kernel reports the connector as driven by a CRTC right now.
    pub lit: bool,
    pub internal: bool,
    /// Monitor or TV name from the EDID, if it has one.
    pub label: Option<String>,
    /// The preferred (first listed) mode, as "3840x2160".
    pub mode: Option<String>,
    /// sysfs has no refresh rate, so this stays None; the launcher shows the window size instead.
    pub refresh: Option<f64>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DisplayState {
    pub outputs: Vec<Output>,
    /// The connector gamescope is showing on, if it can be told.
    pub active: Option<String>,
    /// An external display is connected and carries the picture.
    pub docked: bool,
    pub dock_name: Option<String>,
    pub dock_mode: Option<String>,
}

/// card0-DP-1 -> DP-1, and whether it is a built-in panel.
pub fn connector_name(dir: &str) -> Option<(String, bool)> {
    let rest = dir.strip_prefix("card")?;
    let (_, name) = rest.split_once('-')?;
    if name.is_empty() {
        return None;
    }
    let internal = name.starts_with("eDP") || name.starts_with("LVDS") || name.starts_with("DSI");
    Some((name.to_string(), internal))
}

/// The monitor name descriptor (tag 0xFC) of an EDID base block.
pub fn edid_name(edid: &[u8]) -> Option<String> {
    if edid.len() < 128 || edid[..8] != [0, 255, 255, 255, 255, 255, 255, 0] {
        return None;
    }
    for off in [54usize, 72, 90, 108] {
        let d = &edid[off..off + 18];
        if d[0] == 0 && d[1] == 0 && d[2] == 0 && d[3] == 0xFC {
            let s: String = d[5..18].iter().take_while(|&&b| b != 0x0A && b != 0).map(|&b| b as char).collect();
            let s = s.trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

/// First line of a connector's `modes` file ("3840x2160"); sysfs lists the preferred mode first.
pub fn preferred_mode(modes: &str) -> Option<String> {
    let l = modes.lines().next()?.trim();
    let (w, h) = l.split_once('x')?;
    let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    // Interlaced modes read 1920x1080i.
    let h = h.trim_end_matches('i');
    (ok(w) && ok(h)).then(|| format!("{w}x{h}"))
}

/// Pure: from the connectors, decide the docked state. The picture is on the external display
/// when one is connected and lit, or connected while the internal panel is dark.
pub fn summarise(mut outputs: Vec<Output>) -> DisplayState {
    outputs.sort_by(|a, b| b.internal.cmp(&a.internal).then(a.name.cmp(&b.name)));
    let lit: Vec<&Output> = outputs.iter().filter(|o| o.connected && o.lit).collect();
    let ext = outputs.iter().find(|o| o.connected && !o.internal && (o.lit || lit.is_empty() || lit.iter().all(|l| !l.internal)));
    let docked_out = ext.filter(|e| e.lit || !outputs.iter().any(|o| o.internal && o.lit));
    let active = lit.first().map(|o| o.name.clone()).or_else(|| docked_out.map(|o| o.name.clone()));
    DisplayState {
        active,
        docked: docked_out.is_some(),
        dock_name: docked_out.map(|o| o.label.clone().unwrap_or_else(|| o.name.clone())),
        dock_mode: docked_out.and_then(|o| o.mode.clone()),
        outputs,
    }
}

pub fn read_dir(root: &Path) -> DisplayState {
    let mut outs = Vec::new();
    if let Ok(rd) = fs::read_dir(root) {
        for e in rd.flatten() {
            let fname = e.file_name().to_string_lossy().to_string();
            let Some((name, internal)) = connector_name(&fname) else { continue };
            let p = e.path();
            let rd = |f: &str| fs::read_to_string(p.join(f)).unwrap_or_default();
            let connected = rd("status").trim() == "connected";
            outs.push(Output {
                name,
                connected,
                lit: rd("enabled").trim() == "enabled",
                internal,
                label: if connected { fs::read(p.join("edid")).ok().and_then(|b| edid_name(&b)) } else { None },
                mode: if connected { preferred_mode(&rd("modes")) } else { None },
                refresh: None,
            });
        }
    }
    summarise(outs)
}

pub fn display_state() -> DisplayState {
    read_dir(Path::new("/sys/class/drm"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(name: &str, connected: bool, lit: bool, mode: Option<&str>) -> Output {
        let (_, internal) = connector_name(&format!("card1-{name}")).unwrap();
        Output { name: name.into(), connected, lit, internal, label: None, mode: mode.map(String::from), refresh: None }
    }

    #[test]
    fn names() {
        assert_eq!(connector_name("card1-DP-1"), Some(("DP-1".into(), false)));
        assert_eq!(connector_name("card1-eDP-1"), Some(("eDP-1".into(), true)));
        assert_eq!(connector_name("card1-HDMI-A-1"), Some(("HDMI-A-1".into(), false)));
        assert_eq!(connector_name("card1"), None);
        assert_eq!(connector_name("version"), None);
    }

    #[test]
    fn modes() {
        assert_eq!(preferred_mode("3840x2160\n1920x1080\n"), Some("3840x2160".into()));
        assert_eq!(preferred_mode("1920x1080i\n"), Some("1920x1080".into()));
        assert_eq!(preferred_mode(""), None);
        assert_eq!(preferred_mode("garbage\n"), None);
    }

    #[test]
    fn edid() {
        let mut b = vec![0u8; 128];
        b[..8].copy_from_slice(&[0, 255, 255, 255, 255, 255, 255, 0]);
        b[72 + 3] = 0xFC;
        b[77..77 + 8].copy_from_slice(b"LG TV\n  ");
        assert_eq!(edid_name(&b), Some("LG TV".into()));
        assert_eq!(edid_name(&[0u8; 128]), None);
        assert_eq!(edid_name(&[1, 2]), None);
    }

    #[test]
    fn undocked_internal_only() {
        let s = summarise(vec![out("eDP-1", true, true, Some("1200x1920")), out("DP-1", false, false, None)]);
        assert!(!s.docked);
        assert_eq!(s.active.as_deref(), Some("eDP-1"));
    }

    #[test]
    fn docked_external_lit_internal_dark() {
        let mut tv = out("DP-1", true, true, Some("3840x2160"));
        tv.label = Some("LG TV".into());
        let s = summarise(vec![out("eDP-1", true, false, Some("1200x1920")), tv]);
        assert!(s.docked);
        assert_eq!(s.active.as_deref(), Some("DP-1"));
        assert_eq!(s.dock_name.as_deref(), Some("LG TV"));
        assert_eq!(s.dock_mode.as_deref(), Some("3840x2160"));
    }

    #[test]
    fn plugged_but_internal_still_lit_is_not_docked() {
        // Cable in, gamescope has not switched yet (or the sink is mirrored away): not docked.
        let s = summarise(vec![out("eDP-1", true, true, Some("1200x1920")), out("DP-1", true, false, Some("3840x2160"))]);
        assert!(!s.docked);
        assert_eq!(s.active.as_deref(), Some("eDP-1"));
    }

    #[test]
    fn nothing_lit_external_connected_counts_as_docked() {
        // gamescope not running (desktop mode reports through the same files): external only.
        let s = summarise(vec![out("eDP-1", true, false, None), out("HDMI-A-1", true, false, Some("1920x1080"))]);
        assert!(s.docked);
        assert_eq!(s.dock_name.as_deref(), Some("HDMI-A-1"));
    }
}
