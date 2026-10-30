//! Wi-Fi and Bluetooth for Settings > Network, without the Tauri crate: NetworkManager through
//! `nmcli` (the same calls as src-tauri onboard.rs) and BlueZ through `bluetoothctl` (state, power,
//! looking for devices, pairing). Every call blocks, so callers run them on a worker thread.

use crate::system::run;
use serde::Serialize;

fn nmcli(args: &[&str]) -> Result<String, String> {
    // Joining a network waits for the password check and DHCP; the rest answers at once.
    let limit = if args.contains(&"connect") { 75 } else { 25 };
    crate::system::run_for("nmcli", args, std::time::Duration::from_secs(limit)).map_err(|e| e.strip_prefix("Error: ").unwrap_or(&e).to_string())
}

/// Splits one line of `nmcli -t` output: ':' separates fields, '\' escapes a literal ':' or '\'.
pub fn split_terse(line: &str) -> Vec<String> {
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

pub fn parse_wifi(text: &str) -> Vec<WifiNet> {
    let mut nets: Vec<WifiNet> = Vec::new();
    for line in text.lines() {
        let f = split_terse(line);
        if f.len() < 4 || f[1].is_empty() {
            continue;
        }
        let net = WifiNet { ssid: f[1].clone(), signal: f[2].parse().unwrap_or(0), secure: !f[3].trim().is_empty() && f[3].trim() != "--", active: f[0].trim() == "*" };
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

/// Forgets a saved Wi-Fi network so it is not joined again.
pub fn wifi_forget(ssid: &str) -> Result<(), String> {
    nmcli(&["connection", "delete", "id", ssid]).map(|_| ())
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NetStatus {
    /// NetworkManager reports full internet access.
    pub online: bool,
    /// The connected Wi-Fi network, if any.
    pub wifi: Option<String>,
    /// false when NetworkManager isn't there (only status can be shown).
    pub managed: bool,
}

/// Whether the device has a network right now, from NetworkManager's last state (no probe, so it
/// answers at once). None when NetworkManager isn't there or didn't answer.
pub fn linked() -> Option<bool> {
    let out = crate::system::run_for("nmcli", &["-t", "-f", "STATE,CONNECTIVITY", "general"], std::time::Duration::from_secs(3)).ok()?;
    parse_linked(&out)
}

pub fn parse_linked(out: &str) -> Option<bool> {
    let f = split_terse(out.lines().next()?.trim());
    let state = f.first().map(String::as_str).unwrap_or("");
    let conn = f.get(1).map(String::as_str).unwrap_or("");
    if state.is_empty() {
        return None;
    }
    Some(!(state.starts_with("disconnected") || state == "asleep" || conn == "none"))
}

pub fn net_status() -> NetStatus {
    let managed = nmcli(&["-t", "-f", "RUNNING", "general"]).map(|s| s.trim() == "running").unwrap_or(false);
    let online = managed && nmcli(&["networking", "connectivity", "check"]).map(|s| s.trim() == "full").unwrap_or(false);
    let wifi = if managed { active_wifi() } else { None };
    NetStatus { online, wifi, managed }
}

/// The connected Wi-Fi network's name.
pub fn active_wifi() -> Option<String> {
    nmcli(&["-t", "-f", "ACTIVE,SSID", "dev", "wifi"]).ok().and_then(|out| out.lines().find_map(|l| l.strip_prefix("yes:").map(|s| split_terse(s).join(":")))).filter(|s| !s.is_empty())
}

// ------------------------------------------------------------------------------------ Bluetooth

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct BtState {
    pub present: bool,
    pub powered: bool,
    /// An rfkill soft block is set (airplane mode and the like).
    pub blocked: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BtDevice {
    pub addr: String,
    pub name: String,
    pub paired: bool,
    pub connected: bool,
}

fn bt(args: &[&str]) -> Result<String, String> {
    // Without bluetoothd the tool waits for it forever; pairing and scanning take a while.
    let limit = if matches!(args.first().copied(), Some("pair" | "connect" | "--timeout")) { 60 } else { 8 };
    crate::system::run_for("bluetoothctl", args, std::time::Duration::from_secs(limit))
}

/// `bluetoothctl show` into a state. No controller prints "No default controller available".
pub fn parse_bt_show(out: &str) -> BtState {
    if !out.contains("Controller ") {
        return BtState::default();
    }
    let flag = |k: &str| out.lines().any(|l| l.trim().starts_with(k) && l.trim().ends_with("yes"));
    BtState { present: true, powered: flag("Powered:"), blocked: false }
}

fn bt_soft_blocked() -> bool {
    let Ok(rd) = std::fs::read_dir("/sys/class/rfkill") else { return false };
    rd.flatten().any(|e| {
        let p = e.path();
        std::fs::read_to_string(p.join("type")).map(|t| t.trim() == "bluetooth").unwrap_or(false) && std::fs::read_to_string(p.join("soft")).map(|s| s.trim() == "1").unwrap_or(false)
    })
}

pub fn bt_state() -> BtState {
    let mut s = bt(&["show"]).map(|o| parse_bt_show(&o)).unwrap_or_default();
    if !s.powered {
        s.blocked = bt_soft_blocked();
    }
    s
}

/// Powers the adapter on or off and returns the state afterwards.
pub fn bt_power(on: bool) -> Result<BtState, String> {
    if on && bt_soft_blocked() {
        let _ = run("rfkill", &["unblock", "bluetooth"]);
    }
    bt(&["power", if on { "on" } else { "off" }])?;
    let s = bt_state();
    if s.powered != on {
        return Err(if s.blocked { "Bluetooth is blocked by a switch or airplane mode.".into() } else { "Bluetooth didn't change.".into() });
    }
    Ok(s)
}

/// `Device AA:BB:CC:DD:EE:FF Name` lines.
pub fn parse_bt_devices(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("Device ")?;
            let (addr, name) = rest.split_once(' ').unwrap_or((rest, ""));
            (addr.matches(':').count() == 5).then(|| (addr.to_string(), name.trim().to_string()))
        })
        .collect()
}

/// Every device BlueZ knows (paired, remembered and just found). Devices whose name is only their
/// address are hidden: they are mostly beacons.
pub fn bt_devices() -> Vec<BtDevice> {
    let all = bt(&["devices"]).map(|o| parse_bt_devices(&o)).unwrap_or_default();
    let paired = bt(&["devices", "Paired"]).map(|o| parse_bt_devices(&o)).unwrap_or_default();
    let connected = bt(&["devices", "Connected"]).map(|o| parse_bt_devices(&o)).unwrap_or_default();
    let mut out: Vec<BtDevice> = all
        .into_iter()
        .filter(|(a, n)| !n.is_empty() && n.replace('-', ":") != *a)
        .map(|(addr, name)| BtDevice { paired: paired.iter().any(|(a, _)| *a == addr), connected: connected.iter().any(|(a, _)| *a == addr), addr, name })
        .collect();
    out.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.paired.cmp(&a.paired)).then(a.name.cmp(&b.name)));
    out
}

/// Looks for devices for `secs` seconds, then lists them.
pub fn bt_scan(secs: u32) -> Vec<BtDevice> {
    let t = secs.to_string();
    let _ = bt(&["--timeout", &t, "scan", "on"]);
    bt_devices()
}

/// Pairs, trusts and connects (a controller or headphones, which need no code).
pub fn bt_pair(addr: &str) -> Result<(), String> {
    if !addr.chars().all(|c| c.is_ascii_hexdigit() || c == ':') {
        return Err("Not a Bluetooth address.".into());
    }
    let paired = bt(&["devices", "Paired"]).map(|o| parse_bt_devices(&o).iter().any(|(a, _)| a == addr)).unwrap_or(false);
    if !paired {
        bt(&["pair", addr]).map_err(|e| if e.is_empty() { "Pairing failed.".to_string() } else { e })?;
    }
    let _ = bt(&["trust", addr]);
    bt(&["connect", addr]).map(|_| ()).map_err(|e| if e.is_empty() { "Couldn't connect.".to_string() } else { e })
}

pub fn bt_disconnect(addr: &str) -> Result<(), String> {
    bt(&["disconnect", addr]).map(|_| ())
}

/// Unpairs and forgets a device.
pub fn bt_forget(addr: &str) -> Result<(), String> {
    bt(&["remove", addr]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linked_from_general_state() {
        assert_eq!(parse_linked("connected:full\n"), Some(true));
        assert_eq!(parse_linked("connected (site only):limited"), Some(true));
        assert_eq!(parse_linked("disconnected:none"), Some(false));
        assert_eq!(parse_linked("asleep:none"), Some(false));
        assert_eq!(parse_linked("connecting:none"), Some(false));
        assert_eq!(parse_linked(""), None);
    }

    #[test]
    fn splits_escaped_fields() {
        assert_eq!(split_terse(r"*:Cafe\: Guest:72:WPA2"), vec!["*", "Cafe: Guest", "72", "WPA2"]);
    }

    #[test]
    fn parses_wifi_list() {
        let nets = parse_wifi(" :Home:60:WPA2\n*:Home:80:WPA2\n :Cafe:90:\n :::\n");
        assert_eq!(nets[0], WifiNet { ssid: "Home".into(), signal: 80, secure: true, active: true });
        assert_eq!(nets.len(), 2);
        assert!(!nets[1].secure);
    }

    #[test]
    fn parses_bluetooth() {
        let show = "Controller 00:11:22:33:44:55 (public)\n\tName: rare\n\tPowered: yes\n\tPairable: yes\n";
        assert_eq!(parse_bt_show(show), BtState { present: true, powered: true, blocked: false });
        assert_eq!(parse_bt_show("No default controller available"), BtState::default());
        let d = parse_bt_devices("Device AA:BB:CC:DD:EE:FF Xbox Controller\nDevice 11:22:33:44:55:66 11-22-33-44-55-66\nnoise\n");
        assert_eq!(d.len(), 2);
        assert_eq!(d[0], ("AA:BB:CC:DD:EE:FF".to_string(), "Xbox Controller".to_string()));
    }
}
