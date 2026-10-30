//! Rare's own streaming file (`~/.config/rare/stream.json`): the stream settings, PCs added by
//! address or paired from Rare, and each PC's last app list with Rare's cached box art. Plus
//! `merge`, which turns Moonlight's hosts, Rare's and the ones answering right now into one list
//! (pure, tested).

use super::cli::Settings;
use super::conf::Known;
use super::discover::ServerInfo;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Saved {
    pub uuid: String,
    pub name: String,
    pub address: String,
    pub mac: Option<String>,
}

/// A game or app on a PC, as the Library and Home show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct StreamApp {
    pub id: u32,
    pub name: String,
    /// Box art cached by Rare (a local file), from the PC through Moonlight.
    pub art: Option<String>,
    pub hdr: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct File {
    pub settings: Settings,
    pub hosts: Vec<Saved>,
    /// Host uuid -> its apps, as last listed.
    pub apps: BTreeMap<String, Vec<StreamApp>>,
}

fn path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("rare/stream.json")
}

pub fn load() -> File {
    std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save(f: &File) -> Result<(), String> {
    let p = path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(tmp, p).map_err(|e| e.to_string())
}

/// Adds or updates a PC Rare has talked to.
pub fn remember(f: &mut File, s: Saved) {
    match f.hosts.iter_mut().find(|h| h.uuid == s.uuid) {
        Some(h) => {
            h.name = s.name;
            h.address = s.address;
            if s.mac.is_some() {
                h.mac = s.mac;
            }
        }
        None => f.hosts.push(s),
    }
}

/// A PC as the UI sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostView {
    pub uuid: String,
    pub name: String,
    /// The address Moonlight is given: the one that answered last.
    pub address: Option<String>,
    /// Moonlight holds this PC's certificate.
    pub paired: bool,
    /// Answered just now.
    pub online: bool,
    /// The app running on the PC.
    pub current_game: Option<u32>,
    /// A MAC address is known, so Wake-on-LAN can be sent.
    pub can_wake: bool,
    pub software: Option<String>,
    pub codecs: Option<u32>,
}

/// One answer from a PC: the address that answered and its serverinfo.
pub type Online = HashMap<String, (String, ServerInfo)>;

/// Moonlight's hosts, Rare's and the ones answering right now, by uuid. Paired first, then
/// online, then by name.
pub fn merge(known: &[Known], saved: &[Saved], online: &Online) -> Vec<HostView> {
    let mut ids: Vec<&str> = Vec::new();
    for id in known.iter().map(|k| k.uuid.as_str()).chain(saved.iter().map(|s| s.uuid.as_str())).chain(online.keys().map(String::as_str)) {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let mut out: Vec<HostView> = ids
        .into_iter()
        .map(|id| {
            let k = known.iter().find(|k| k.uuid == id);
            let s = saved.iter().find(|s| s.uuid == id);
            let o = online.get(id);
            let name = o.map(|(_, i)| i.name.clone()).or_else(|| s.map(|s| s.name.clone())).or_else(|| k.map(|k| k.name.clone())).filter(|n| !n.is_empty()).unwrap_or_else(|| "PC".into());
            let address = o.map(|(a, _)| a.clone()).or_else(|| s.map(|s| s.address.clone()).filter(|a| !a.is_empty())).or_else(|| k.and_then(|k| k.addresses.first().cloned()));
            let can_wake = o.is_some_and(|(_, i)| i.mac.is_some()) || s.is_some_and(|s| s.mac.is_some()) || k.is_some_and(|k| k.mac.is_some());
            HostView {
                uuid: id.to_string(),
                name,
                address,
                paired: k.is_some_and(|k| k.paired),
                online: o.is_some(),
                current_game: o.and_then(|(_, i)| i.current_game),
                can_wake,
                software: o.map(|(_, i)| i.software.clone()),
                codecs: o.and_then(|(_, i)| i.codecs),
            }
        })
        .collect();
    out.sort_by(|a, b| b.paired.cmp(&a.paired).then(b.online.cmp(&a.online)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

/// Every address worth trying for a host, best first.
pub fn addresses(uuid: &str, known: &[Known], saved: &[Saved], online: &Online) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut add = |a: &str| {
        if !a.is_empty() && !v.iter().any(|x| x == a) {
            v.push(a.to_string());
        }
    };
    if let Some((a, _)) = online.get(uuid) {
        add(a);
    }
    if let Some(s) = saved.iter().find(|s| s.uuid == uuid) {
        add(&s.address);
    }
    if let Some(k) = known.iter().find(|k| k.uuid == uuid) {
        for a in &k.addresses {
            add(a);
        }
    }
    v
}

/// The MAC to wake a host with.
pub fn mac_of(uuid: &str, known: &[Known], saved: &[Saved], online: &Online) -> Option<[u8; 6]> {
    let parse = super::wol::parse_mac;
    online
        .get(uuid)
        .and_then(|(_, i)| i.mac.as_deref().and_then(parse))
        .or_else(|| saved.iter().find(|s| s.uuid == uuid).and_then(|s| s.mac.as_deref().and_then(parse)))
        .or_else(|| known.iter().find(|k| k.uuid == uuid).and_then(|k| k.mac))
}

/// The apps of a host: Rare's last list, else what Moonlight saved. Hidden ones are left out.
pub fn apps_of(uuid: &str, file: &File, known: &[Known]) -> Vec<StreamApp> {
    if let Some(a) = file.apps.get(uuid).filter(|a| !a.is_empty()) {
        return a.clone();
    }
    known
        .iter()
        .find(|k| k.uuid == uuid)
        .map(|k| k.apps.iter().filter(|(_, _, hidden)| !hidden).map(|(id, name, _)| StreamApp { id: *id, name: name.clone(), art: None, hdr: false }).collect())
        .unwrap_or_default()
}

/// The running id of a stream: "stream:<host uuid>:<app id>".
pub fn run_id(uuid: &str, app: u32) -> String {
    format!("stream:{uuid}:{app}")
}

/// (host uuid, app id) from a running id.
pub fn parse_run_id(id: &str) -> Option<(String, u32)> {
    let rest = id.strip_prefix("stream:")?;
    let (uuid, app) = rest.rsplit_once(':')?;
    Some((uuid.to_string(), app.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(name: &str, uuid: &str, mac: Option<&str>, game: Option<u32>) -> ServerInfo {
        ServerInfo { name: name.into(), uuid: uuid.into(), mac: mac.map(String::from), current_game: game, codecs: Some(0x10101), software: "Sunshine".into() }
    }

    #[test]
    fn merges_moonlight_rare_and_the_network() {
        let known = vec![
            Known { uuid: "A".into(), name: "Gaming-PC".into(), mac: Some([1, 2, 3, 4, 5, 6]), addresses: vec!["192.168.0.20".into(), "203.0.113.9".into()], paired: true, apps: vec![] },
            Known { uuid: "C".into(), name: "Old laptop".into(), mac: None, addresses: vec![], paired: false, apps: vec![] },
        ];
        let saved = vec![Saved { uuid: "B".into(), name: "Den".into(), address: "192.168.0.31".into(), mac: None }];
        let mut online: Online = HashMap::new();
        online.insert("A".into(), ("192.168.0.25".into(), info("Gaming-PC", "A", None, Some(42))));
        online.insert("D".into(), ("192.168.0.40".into(), info("Living-room", "D", Some("aa:bb:cc:dd:ee:ff"), None)));
        let v = merge(&known, &saved, &online);
        let ids: Vec<&str> = v.iter().map(|h| h.uuid.as_str()).collect();
        // Paired first, then the online one, then the rest by name.
        assert_eq!(ids, vec!["A", "D", "B", "C"]);
        let a = &v[0];
        assert!(a.paired && a.online && a.can_wake);
        assert_eq!(a.address.as_deref(), Some("192.168.0.25"));
        assert_eq!(a.current_game, Some(42));
        assert_eq!(a.software.as_deref(), Some("Sunshine"));
        let b = v.iter().find(|h| h.uuid == "B").unwrap();
        assert!(!b.paired && !b.online && !b.can_wake);
        assert_eq!(b.address.as_deref(), Some("192.168.0.31"));
        assert!(v.iter().find(|h| h.uuid == "D").unwrap().can_wake);
        assert_eq!(addresses("A", &known, &saved, &online), vec!["192.168.0.25", "192.168.0.20", "203.0.113.9"]);
        assert_eq!(mac_of("A", &known, &saved, &online), Some([1, 2, 3, 4, 5, 6]));
        assert_eq!(mac_of("D", &known, &saved, &online), Some([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]));
    }

    #[test]
    fn apps_and_run_ids() {
        let known = vec![Known { uuid: "A".into(), apps: vec![(1, "Desktop".into(), false), (2, "Hidden".into(), true)], ..Default::default() }];
        let mut f = File::default();
        assert_eq!(apps_of("A", &f, &known).iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), vec!["Desktop"]);
        f.apps.insert("A".into(), vec![StreamApp { id: 9, name: "Elden Ring".into(), art: Some("/c/9.png".into()), hdr: true }]);
        assert_eq!(apps_of("A", &f, &known)[0].id, 9);
        let id = run_id("5F6A-71C3", 1093255277);
        assert_eq!(id, "stream:5F6A-71C3:1093255277");
        assert_eq!(parse_run_id(&id), Some(("5F6A-71C3".into(), 1093255277)));
        assert_eq!(parse_run_id("app:netflix"), None);
        let mut f = File::default();
        remember(&mut f, Saved { uuid: "A".into(), name: "PC".into(), address: "1.2.3.4".into(), mac: Some("aa:bb:cc:dd:ee:ff".into()) });
        remember(&mut f, Saved { uuid: "A".into(), name: "Gaming-PC".into(), address: "1.2.3.5".into(), mac: None });
        assert_eq!(f.hosts.len(), 1);
        assert_eq!(f.hosts[0].name, "Gaming-PC");
        assert_eq!(f.hosts[0].mac.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
    }
}
