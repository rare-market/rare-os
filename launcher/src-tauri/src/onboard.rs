//! What the first-run screens need from the system: Wi-Fi through NetworkManager (nmcli),
//! whether the device is online, and which Steam account is signed in (loginusers.vdf).

use serde::Serialize;
use std::fs;
use std::process::Command;

fn nmcli(args: &[&str]) -> Result<String, String> {
    let out = crate::launch::host_env(Command::new("nmcli").args(args)).output().map_err(|e| format!("nmcli: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(err.strip_prefix("Error: ").unwrap_or(&err).to_string())
    }
}

/// Splits one line of `nmcli -t` output: fields are separated by ':', and a literal ':' or
/// '\' inside a field is escaped with '\'.
fn split_terse(line: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    out.last_mut().unwrap().push(n);
                }
            }
            ':' => out.push(String::new()),
            _ => out.last_mut().unwrap().push(c),
        }
    }
    out
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WifiNet {
    pub ssid: String,
    /// 0 to 100
    pub signal: u8,
    pub secure: bool,
    pub active: bool,
}

fn parse_wifi(text: &str) -> Vec<WifiNet> {
    let mut nets: Vec<WifiNet> = Vec::new();
    for line in text.lines() {
        let f = split_terse(line);
        if f.len() < 4 || f[1].is_empty() {
            continue;
        }
        let net = WifiNet {
            ssid: f[1].clone(),
            signal: f[2].parse().unwrap_or(0),
            secure: !f[3].trim().is_empty() && f[3].trim() != "--",
            active: f[0].trim() == "*",
        };
        // One entry per network name: the active one, else the strongest access point.
        match nets.iter_mut().find(|n| n.ssid == net.ssid) {
            Some(n) => {
                if net.active || (!n.active && net.signal > n.signal) {
                    *n = net;
                }
            }
            None => nets.push(net),
        }
    }
    nets.sort_by(|a, b| b.active.cmp(&a.active).then(b.signal.cmp(&a.signal)));
    nets
}

pub fn wifi_scan() -> Result<Vec<WifiNet>, String> {
    let text = nmcli(&["-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "dev", "wifi", "list", "--rescan", "auto"])?;
    Ok(parse_wifi(&text))
}

pub fn wifi_connect(ssid: &str, password: Option<&str>) -> Result<(), String> {
    let mut args = vec!["dev", "wifi", "connect", ssid];
    if let Some(p) = password.filter(|p| !p.is_empty()) {
        args.push("password");
        args.push(p);
    }
    nmcli(&args).map(|_| ())
}

/// Forgets a saved Wi-Fi network (its NetworkManager connection) so it is not joined again.
pub fn wifi_forget(ssid: &str) -> Result<(), String> {
    nmcli(&["connection", "delete", "id", ssid]).map(|_| ())
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NetStatus {
    /// NetworkManager reports full internet access
    pub online: bool,
    /// the connected Wi-Fi network, if any
    pub wifi: Option<String>,
    /// false when NetworkManager isn't there (the UI then only shows status)
    pub managed: bool,
}

pub fn net_status() -> NetStatus {
    let managed = nmcli(&["-t", "-f", "RUNNING", "general"]).map(|s| s.trim() == "running").unwrap_or(false);
    let online = if managed {
        nmcli(&["networking", "connectivity", "check"]).map(|s| s.trim() == "full").unwrap_or(false)
    } else {
        false
    };
    let wifi = if managed {
        nmcli(&["-t", "-f", "ACTIVE,SSID", "dev", "wifi"])
            .ok()
            .and_then(|out| out.lines().find_map(|l| l.strip_prefix("yes:").map(|s| split_terse(s).join(":"))))
            .filter(|s| !s.is_empty())
    } else {
        None
    };
    NetStatus { online, wifi, managed }
}

/// The Steam account that signed in last on this device, from loginusers.vdf.
pub fn steam_account() -> Option<String> {
    for root in crate::library::steam::find_roots() {
        let Ok(text) = fs::read_to_string(root.path.join("config/loginusers.vdf")) else { continue };
        let v = crate::library::vdf::parse(&text);
        let users = v.get("users").map(|u| u.entries().to_vec()).unwrap_or_default();
        let pick = users
            .iter()
            .find(|(_, u)| u.str("MostRecent") == Some("1"))
            .or_else(|| users.first())
            .and_then(|(_, u)| u.str("PersonaName").or_else(|| u.str("AccountName")).map(String::from));
        if pick.is_some() {
            return pick;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_escaped_fields() {
        assert_eq!(split_terse(r"*:Cafe\: Guest:72:WPA2"), vec!["*", "Cafe: Guest", "72", "WPA2"]);
        assert_eq!(split_terse(r" :back\\slash:5:"), vec![" ", r"back\slash", "5", ""]);
    }

    #[test]
    fn parses_wifi_list() {
        let text = " :Home:60:WPA2\n*:Home:80:WPA2\n :Cafe:90:\n :Neighbour:40:WPA1 WPA2\n :::\n";
        let nets = parse_wifi(text);
        assert_eq!(nets[0], WifiNet { ssid: "Home".into(), signal: 80, secure: true, active: true });
        assert_eq!(nets[1].ssid, "Cafe");
        assert!(!nets[1].secure);
        assert_eq!(nets.len(), 3);
    }
}
