//! Quick Settings' radio tiles: Wi-Fi, Bluetooth and Airplane mode.
//!
//! Wi-Fi goes through NetworkManager (`nmcli radio wifi`), Bluetooth through BlueZ's Powered
//! property (wallet::bluetooth) and Airplane mode through rfkill: one event to /dev/rfkill that
//! blocks or unblocks every radio, the same switch the kernel's own airplane key uses. The session
//! user may write /dev/rfkill (uaccess, as for the Bluetooth unblock).

use serde::Serialize;
use std::process::Command;

#[derive(Serialize, Clone, Copy, Default, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Radios {
    /// Wi-Fi radio is on; None when the device has none or NetworkManager is missing.
    pub wifi: Option<bool>,
    /// Bluetooth adapter is powered; None when there is no adapter.
    pub bluetooth: Option<bool>,
    /// Every radio is soft-blocked.
    pub airplane: bool,
}

/// rfkill_event (packed): u32 idx, u8 type, u8 op, u8 soft, u8 hard. Type 0 is all radios,
/// op 3 is RFKILL_OP_CHANGE_ALL.
pub fn rfkill_all_event(block: bool) -> [u8; 8] {
    [0, 0, 0, 0, 0, 3, block as u8, 0]
}

/// Airplane mode is on when at least one wlan or bluetooth rfkill exists and all of them are soft blocked.
pub fn airplane_from(radios: &[(String, bool)]) -> bool {
    let r: Vec<_> = radios.iter().filter(|(t, _)| t == "wlan" || t == "bluetooth").collect();
    !r.is_empty() && r.iter().all(|(_, soft)| *soft)
}

fn wifi_enabled() -> Option<bool> {
    let out = crate::host::env(Command::new("nmcli").args(["radio", "wifi"])).output().ok()?;
    if !out.status.success() {
        return None;
    }
    match String::from_utf8_lossy(&out.stdout).trim() {
        "enabled" => Some(true),
        "disabled" => Some(false),
        _ => None,
    }
}

fn rfkills() -> Vec<(String, bool)> {
    let Ok(rd) = std::fs::read_dir("/sys/class/rfkill") else { return vec![] };
    rd.flatten()
        .filter_map(|e| {
            let p = e.path();
            let t = std::fs::read_to_string(p.join("type")).ok()?.trim().to_string();
            let soft = std::fs::read_to_string(p.join("soft")).ok()?.trim() == "1";
            Some((t, soft))
        })
        .collect()
}

pub fn state() -> Radios {
    let bt = crate::host::get().bluetooth_state();
    Radios { wifi: wifi_enabled(), bluetooth: bt, airplane: airplane_from(&rfkills()) }
}

#[cfg(target_os = "linux")]
fn rfkill_write(block: bool) -> Result<(), String> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().write(true).open("/dev/rfkill").map_err(|_| "Airplane mode isn't available on this device".to_string())?;
    f.write_all(&rfkill_all_event(block)).map_err(|_| "Airplane mode didn't change".to_string())
}

#[cfg(not(target_os = "linux"))]
fn rfkill_write(_block: bool) -> Result<(), String> {
    Ok(())
}

pub fn set(kind: &str, on: bool) -> Result<Radios, String> {
    match kind {
        "wifi" => {
            let out = crate::host::env(Command::new("nmcli").args(["radio", "wifi", if on { "on" } else { "off" }])).output().map_err(|_| "Wi-Fi isn't available".to_string())?;
            if !out.status.success() {
                return Err("Wi-Fi didn't change".into());
            }
        }
        "bluetooth" => {
            crate::host::get().bluetooth_set(on).map_err(|_| "Bluetooth didn't change".to_string())?;
        }
        "airplane" => {
            rfkill_write(on)?;
            if !on {
                // Unblocking brings the radios back; make sure the Wi-Fi radio follows.
                let _ = Command::new("nmcli").args(["radio", "wifi", "on"]).output();
            }
        }
        _ => return Err("Unknown radio".into()),
    }
    Ok(state())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfkill_event_blocks_every_radio() {
        assert_eq!(rfkill_all_event(true), [0, 0, 0, 0, 0, 3, 1, 0]);
        assert_eq!(rfkill_all_event(false)[6], 0);
    }

    #[test]
    fn airplane_needs_every_radio_blocked() {
        let s = |t: &str, b| (t.to_string(), b);
        assert!(airplane_from(&[s("wlan", true), s("bluetooth", true)]));
        assert!(!airplane_from(&[s("wlan", true), s("bluetooth", false)]));
        assert!(!airplane_from(&[s("wlan", false)]));
        assert!(!airplane_from(&[]));
        assert!(airplane_from(&[s("wlan", true), s("bluetooth", true), s("other", false)]));
    }
}
