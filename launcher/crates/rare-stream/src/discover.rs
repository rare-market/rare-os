//! Finding PCs: Sunshine and GeForce Experience announce `_nvstream._tcp` over mDNS, and answer
//! `GET http://<host>:47989/serverinfo` without pairing (name, unique id, MAC, codecs, the app
//! running). The parsing is pure and tested; the network calls are thin.

use serde::Serialize;
use std::net::IpAddr;
use std::time::{Duration, Instant};

pub const HTTP_PORT: u16 = 47989;
const SERVICE: &str = "_nvstream._tcp.local.";

/// A host's `serverinfo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub name: String,
    pub uuid: String,
    pub mac: Option<String>,
    /// The app streaming or running on the PC, 0 or None when it is free.
    pub current_game: Option<u32>,
    /// ServerCodecModeSupport bits (cli::SCM_*), when the host says.
    pub codecs: Option<u32>,
    /// "Sunshine" or "GeForce Experience".
    pub software: String,
}

/// The text of the first `<tag>` in a flat XML document, with entities decoded.
pub fn xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{tag}>"))? + start;
    Some(xml[start..end].replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&"))
}

/// Reads `serverinfo`. None when it isn't one (no unique id).
pub fn parse_serverinfo(xml: &str) -> Option<ServerInfo> {
    let uuid = xml_tag(xml, "uniqueid").map(|u| u.trim().to_string()).filter(|u| !u.is_empty())?;
    let name = xml_tag(xml, "hostname").map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| "PC".into());
    let mac = xml_tag(xml, "mac").map(|m| m.trim().to_ascii_lowercase()).filter(|m| super::wol::parse_mac(m).is_some_and(|b| b != [0; 6]));
    let current_game = xml_tag(xml, "currentgame").and_then(|g| g.trim().parse::<u32>().ok());
    let codecs = xml_tag(xml, "ServerCodecModeSupport").and_then(|c| c.trim().parse::<u32>().ok());
    // Sunshine answers state SUNSHINE_SERVER_FREE / _BUSY; GeForce Experience doesn't.
    let state = xml_tag(xml, "state").unwrap_or_default();
    let software = if state.starts_with("SUNSHINE") { "Sunshine" } else { "GeForce Experience" }.to_string();
    Some(ServerInfo { name, uuid, mac, current_game: current_game.filter(|g| *g != 0), codecs, software })
}

/// `http://<address>:47989/serverinfo`. `address` may carry its own port ("10.0.0.2:48989").
pub fn probe(address: &str, timeout: Duration) -> Result<ServerInfo, String> {
    let base = http_base(address);
    let url = format!("{base}/serverinfo?uniqueid=0123456789ABCDEF&uuid={}", uuid::Uuid::nil());
    let body = ureq::get(&url)
        .timeout(timeout)
        .set("User-Agent", "RareLauncher/0.1")
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;
    parse_serverinfo(&body).ok_or_else(|| "Not a game streaming host".into())
}

/// "10.0.0.2" -> "http://10.0.0.2:47989", "fe80::1" -> "http://[fe80::1]:47989".
pub fn http_base(address: &str) -> String {
    let a = address.trim();
    if let Ok(ip) = a.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => format!("http://{v4}:{HTTP_PORT}"),
            IpAddr::V6(v6) => format!("http://[{v6}]:{HTTP_PORT}"),
        };
    }
    if a.starts_with('[') || a.matches(':').count() == 1 {
        // Already has a port (host:port or [v6]:port).
        return format!("http://{a}");
    }
    format!("http://{a}:{HTTP_PORT}")
}

/// A host address typed by the user: an IPv4 or IPv6 address or a host name, maybe with a port.
pub fn valid_address(a: &str) -> bool {
    let a = a.trim();
    !a.is_empty() && a.len() <= 253 && a.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '[' | ']' | '%'))
}

/// One `_nvstream._tcp` answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    pub address: String,
}

/// "Gaming-PC._nvstream._tcp.local." -> "Gaming-PC". IPv4 first (what Moonlight connects to),
/// lowest first so the same host gives the same answer every time.
pub fn from_service(fullname: &str, port: u16, mut v4: Vec<std::net::Ipv4Addr>, mut v6: Vec<std::net::Ipv6Addr>) -> Option<Found> {
    let name = fullname.strip_suffix(SERVICE).map(|n| n.trim_end_matches('.')).unwrap_or(fullname).to_string();
    v4.sort();
    v6.sort();
    let ip = v4.first().map(|a| a.to_string()).or_else(|| v6.iter().find(|a| !a.is_loopback()).map(|a| a.to_string()))?;
    let address = if port == 0 || port == HTTP_PORT {
        ip
    } else if ip.contains(':') {
        format!("[{ip}]:{port}")
    } else {
        format!("{ip}:{port}")
    };
    Some(Found { name, address })
}

/// Browses `_nvstream._tcp` for `wait`.
pub fn browse(wait: Duration) -> Vec<Found> {
    use mdns_sd::{ServiceDaemon, ServiceEvent};
    let Ok(mdns) = ServiceDaemon::new() else { return vec![] };
    let Ok(rx) = mdns.browse(SERVICE) else {
        let _ = mdns.shutdown();
        return vec![];
    };
    let end = Instant::now() + wait;
    let mut out: Vec<Found> = Vec::new();
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let v4: Vec<_> = info.get_addresses_v4().into_iter().collect();
                let v6: Vec<_> = info
                    .get_addresses()
                    .iter()
                    .filter_map(|a| match a.to_ip_addr() {
                        IpAddr::V6(v) => Some(v),
                        _ => None,
                    })
                    .collect();
                if let Some(f) = from_service(info.get_fullname(), info.get_port(), v4, v6) {
                    if !out.iter().any(|o| o.address == f.address) {
                        out.push(f);
                    }
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let _ = mdns.stop_browse(SERVICE);
    let _ = mdns.shutdown();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUNSHINE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<root status_code="200"><hostname>Gaming-PC</hostname><appversion>7.1.431.-1</appversion><GfeVersion>3.23.0.74</GfeVersion><uniqueid>5F6A2B10-71C3-4E7D-9A55-0B1C2D3E4F50</uniqueid><HttpsPort>47984</HttpsPort><ExternalPort>47989</ExternalPort><MaxLumaPixelsHEVC>1869449984</MaxLumaPixelsHEVC><mac>A8:A1:59:12:34:56</mac><Permission>0</Permission><LocalIP>192.168.0.20</LocalIP><ServerCodecModeSupport>197377</ServerCodecModeSupport><PairStatus>0</PairStatus><currentgame>0</currentgame><state>SUNSHINE_SERVER_FREE</state></root>"#;

    #[test]
    fn reads_sunshine_serverinfo() {
        let s = parse_serverinfo(SUNSHINE).unwrap();
        assert_eq!(s.name, "Gaming-PC");
        assert_eq!(s.uuid, "5F6A2B10-71C3-4E7D-9A55-0B1C2D3E4F50");
        assert_eq!(s.mac.as_deref(), Some("a8:a1:59:12:34:56"));
        assert_eq!(s.current_game, None);
        assert_eq!(s.software, "Sunshine");
        // 197377 = H.264 + HEVC + HEVC Main10 + AV1 Main8 + AV1 Main10
        let c = s.codecs.unwrap();
        assert!(c & super::super::cli::SCM_AV1_MAIN8 != 0 && c & super::super::cli::SCM_HEVC != 0);
    }

    #[test]
    fn reads_gfe_and_busy_hosts() {
        let gfe = "<root status_code=\"200\"><hostname>Tom &amp; Jo</hostname><uniqueid>abc</uniqueid><mac>00:00:00:00:00:00</mac><currentgame>1093255277</currentgame></root>";
        let s = parse_serverinfo(gfe).unwrap();
        assert_eq!(s.name, "Tom & Jo");
        assert_eq!(s.mac, None);
        assert_eq!(s.current_game, Some(1093255277));
        assert_eq!(s.software, "GeForce Experience");
        assert_eq!(s.codecs, None);
        assert_eq!(parse_serverinfo("<html>nope</html>"), None);
    }

    #[test]
    fn reads_mdns_answers() {
        let f = from_service("Gaming-PC._nvstream._tcp.local.", 47989, vec!["192.168.0.20".parse().unwrap(), "10.0.0.5".parse().unwrap()], vec![]).unwrap();
        assert_eq!(f, Found { name: "Gaming-PC".into(), address: "10.0.0.5".into() });
        let f = from_service("Den._nvstream._tcp.local.", 48989, vec!["192.168.0.21".parse().unwrap()], vec![]).unwrap();
        assert_eq!(f.address, "192.168.0.21:48989");
        let f = from_service("V6._nvstream._tcp.local.", 47989, vec![], vec!["::1".parse().unwrap(), "fe80::2".parse().unwrap()]).unwrap();
        assert_eq!(f.address, "fe80::2");
        assert_eq!(from_service("None._nvstream._tcp.local.", 47989, vec![], vec![]), None);
    }

    #[test]
    fn addresses() {
        assert_eq!(http_base("192.168.0.20"), "http://192.168.0.20:47989");
        assert_eq!(http_base("192.168.0.20:48989"), "http://192.168.0.20:48989");
        assert_eq!(http_base("fe80::2"), "http://[fe80::2]:47989");
        assert_eq!(http_base("gaming-pc.local"), "http://gaming-pc.local:47989");
        assert!(valid_address("192.168.0.20"));
        assert!(valid_address("gaming-pc.local:48989"));
        assert!(!valid_address("pc; rm -rf ~"));
        assert!(!valid_address(""));
    }
}
