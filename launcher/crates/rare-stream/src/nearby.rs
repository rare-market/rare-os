//! Rare devices on the same network. Each Rare launcher announces `_rare._tcp` over mDNS and
//! browses for the others. The announcement says as little as it can: a random id made once per
//! install (not the hostname or any account), the device's model name, and whether this device
//! streams (its Sunshine host is on; Moonlight then finds it under `_nvstream._tcp` too).

use serde::Serialize;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const SERVICE: &str = "_rare._tcp.local.";
/// The port in the record: Sunshine's, where a streaming Rare device answers serverinfo.
const PORT: u16 = super::discover::HTTP_PORT;

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Nearby {
    pub id: String,
    pub name: String,
    pub address: String,
    /// Streaming this device is on there.
    pub stream: bool,
}

/// This install's id: 12 hex digits, made once.
pub fn device_id() -> String {
    let path = rare_core::paths::config_dir().join("device-id");
    if let Some(id) = std::fs::read_to_string(&path).ok().map(|s| s.trim().to_string()).filter(|s| valid_id(s)) {
        return id;
    }
    let mut b = [0u8; 6];
    use std::io::Read;
    let _ = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b));
    if b == [0; 6] {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1);
        b.copy_from_slice(&t.to_le_bytes()[..6]);
    }
    let id: String = b.iter().map(|x| format!("{x:02x}")).collect();
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(&path, &id);
    id
}

pub fn valid_id(s: &str) -> bool {
    s.len() == 12 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// TXT properties of our record.
pub fn txt(name: &str, stream: bool) -> Vec<(String, String)> {
    vec![("v".into(), "1".into()), ("name".into(), super::host::clean_name(name)), ("stream".into(), if stream { "1" } else { "0" }.into())]
}

/// One answer, from its instance name ("rare-<id>._rare._tcp.local."), TXT and addresses.
pub fn from_record(fullname: &str, props: &HashMap<String, String>, mut v4: Vec<std::net::Ipv4Addr>) -> Option<Nearby> {
    let inst = fullname.strip_suffix(SERVICE)?.trim_end_matches('.');
    let id = inst.strip_prefix("rare-")?;
    if !valid_id(id) {
        return None;
    }
    v4.sort();
    let address = v4.first()?.to_string();
    let name = props.get("name").map(|n| super::host::clean_name(n)).unwrap_or_else(|| "Rare device".into());
    Some(Nearby { id: id.to_string(), name, address, stream: props.get("stream").map(|s| s == "1").unwrap_or(false) })
}

struct Announcer {
    daemon: mdns_sd::ServiceDaemon,
    fullname: String,
    state: (String, bool),
}

static ANN: Mutex<Option<Announcer>> = Mutex::new(None);

/// Announces this device (again when the name or the streaming state changed).
pub fn announce(name: &str, stream: bool) {
    let mut g = ANN.lock().unwrap();
    let want = (name.to_string(), stream);
    if g.as_ref().is_some_and(|a| a.state == want) {
        return;
    }
    let daemon = match g.take() {
        Some(a) => {
            let _ = a.daemon.unregister(&a.fullname);
            a.daemon
        }
        None => match mdns_sd::ServiceDaemon::new() {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[nearby] mdns: {e}");
                return;
            }
        },
    };
    let id = device_id();
    let instance = format!("rare-{id}");
    let host = format!("rare-{id}.local.");
    let props: HashMap<String, String> = txt(name, stream).into_iter().collect();
    match mdns_sd::ServiceInfo::new(SERVICE, &instance, &host, "", PORT, props).map(|i| i.enable_addr_auto()) {
        Ok(info) => {
            let fullname = info.get_fullname().to_string();
            if let Err(e) = daemon.register(info) {
                eprintln!("[nearby] register: {e}");
            }
            *g = Some(Announcer { daemon, fullname, state: want });
        }
        Err(e) => eprintln!("[nearby] record: {e}"),
    }
}

/// Stops announcing (launcher exit).
pub fn stop() {
    if let Some(a) = ANN.lock().unwrap().take() {
        let _ = a.daemon.unregister(&a.fullname);
        std::thread::sleep(Duration::from_millis(100));
        let _ = a.daemon.shutdown();
    }
}

/// Other Rare devices that answer within `wait` (this one left out).
pub fn browse(wait: Duration) -> Vec<Nearby> {
    use mdns_sd::{ServiceDaemon, ServiceEvent};
    let me = device_id();
    let Ok(mdns) = ServiceDaemon::new() else { return vec![] };
    let Ok(rx) = mdns.browse(SERVICE) else {
        let _ = mdns.shutdown();
        return vec![];
    };
    let end = Instant::now() + wait;
    let mut out: Vec<Nearby> = Vec::new();
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let props: HashMap<String, String> = info.get_properties().iter().map(|p| (p.key().to_string(), p.val_str().to_string())).collect();
                let v4: Vec<_> = info.get_addresses().iter().filter_map(|a| match a.to_ip_addr() {
                    IpAddr::V4(v) => Some(v),
                    _ => None,
                }).collect();
                if let Some(n) = from_record(info.get_fullname(), &props, v4) {
                    if n.id != me && !out.iter().any(|o| o.id == n.id) {
                        out.push(n);
                    }
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let _ = mdns.stop_browse(SERVICE);
    let _ = mdns.shutdown();
    out.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip() {
        let props: HashMap<String, String> = txt("SuiPlay", true).into_iter().collect();
        let n = from_record("rare-0a1b2c3d4e5f._rare._tcp.local.", &props, vec!["192.168.1.92".parse().unwrap(), "10.0.0.4".parse().unwrap()]).unwrap();
        assert_eq!(n, Nearby { id: "0a1b2c3d4e5f".into(), name: "SuiPlay".into(), address: "10.0.0.4".into(), stream: true });
        let off: HashMap<String, String> = txt("Deck", false).into_iter().collect();
        assert!(!from_record("rare-0a1b2c3d4e5f._rare._tcp.local.", &off, vec!["10.0.0.4".parse().unwrap()]).unwrap().stream);
        // Not ours, a bad id, no address.
        assert_eq!(from_record("tv._rare._tcp.local.", &props, vec!["10.0.0.4".parse().unwrap()]), None);
        assert_eq!(from_record("rare-xyz._rare._tcp.local.", &props, vec!["10.0.0.4".parse().unwrap()]), None);
        assert_eq!(from_record("rare-0a1b2c3d4e5f._rare._tcp.local.", &props, vec![]), None);
        assert!(valid_id("0a1b2c3d4e5f"));
        assert!(!valid_id("0a1b2c3d4e5"));
    }
}
