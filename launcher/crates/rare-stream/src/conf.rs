//! What Moonlight knows, read from its own settings file (Qt QSettings, INI format): the PCs it
//! has seen, which are paired (a saved server certificate), their MAC addresses for Wake-on-LAN
//! and their last addresses. Read only; Moonlight owns this file.

use std::path::PathBuf;

/// One `[hosts]` entry of Moonlight.conf.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Known {
    pub uuid: String,
    pub name: String,
    pub mac: Option<[u8; 6]>,
    /// Local, manual, remote and IPv6 addresses, in that order, without empties.
    pub addresses: Vec<String>,
    pub paired: bool,
    /// (id, name, hidden) from the last app list Moonlight saw.
    pub apps: Vec<(u32, String, bool)>,
}

/// Moonlight.conf for the Flathub app, then for a native install.
pub fn paths() -> Vec<PathBuf> {
    let rel = "Moonlight Game Streaming Project/Moonlight.conf";
    let mut v = Vec::new();
    if let Some(h) = dirs::home_dir() {
        v.push(h.join(".var/app").join(super::cli::FLATPAK).join("config").join(rel));
    }
    if let Some(c) = dirs::config_dir() {
        v.push(c.join(rel));
    }
    v
}

pub fn load() -> Vec<Known> {
    paths().iter().find_map(|p| std::fs::read(p).ok()).map(|b| parse(&String::from_utf8_lossy(&b))).unwrap_or_default()
}

/// Moonlight's box art cache for a host (`<cache>/boxart/<uuid>`).
pub fn boxart_dir(uuid: &str) -> Vec<PathBuf> {
    let rel = format!("Moonlight Game Streaming Project/Moonlight/boxart/{uuid}");
    let mut v = Vec::new();
    if let Some(h) = dirs::home_dir() {
        v.push(h.join(".var/app").join(super::cli::FLATPAK).join("cache").join(&rel));
    }
    if let Some(c) = dirs::cache_dir() {
        v.push(c.join(&rel));
    }
    v
}

/// A QSettings INI value: text, or bytes for `@ByteArray(...)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Bytes(Vec<u8>),
}

impl Value {
    fn text(&self) -> String {
        match self {
            Value::Text(t) => t.clone(),
            Value::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
        }
    }
}

/// Undoes QSettings' INI escaping (`\x` + hex digits, `\0`, C escapes, double quotes).
pub fn unescape(raw: &str) -> Vec<u32> {
    let c: Vec<char> = raw.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '"' => i += 1,
            '\\' if i + 1 < c.len() => {
                let e = c[i + 1];
                i += 2;
                match e {
                    'x' => {
                        let mut v: u32 = 0;
                        while i < c.len() && c[i].is_ascii_hexdigit() {
                            v = v.wrapping_mul(16).wrapping_add(c[i].to_digit(16).unwrap_or(0));
                            i += 1;
                        }
                        out.push(v);
                    }
                    '0'..='7' => {
                        let mut v = e.to_digit(8).unwrap_or(0);
                        let mut n = 1;
                        while n < 3 && i < c.len() && ('0'..='7').contains(&c[i]) {
                            v = v * 8 + c[i].to_digit(8).unwrap_or(0);
                            i += 1;
                            n += 1;
                        }
                        out.push(v);
                    }
                    'a' => out.push(7),
                    'b' => out.push(8),
                    'f' => out.push(12),
                    'n' => out.push(10),
                    'r' => out.push(13),
                    't' => out.push(9),
                    'v' => out.push(11),
                    other => out.push(other as u32),
                }
            }
            ch => {
                out.push(ch as u32);
                i += 1;
            }
        }
    }
    out
}

pub fn value(raw: &str) -> Value {
    let raw = raw.trim();
    let un = |s: &str| unescape(s);
    let inner = raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(raw);
    if let Some(b) = inner.strip_prefix("@ByteArray(").and_then(|r| r.strip_suffix(')')) {
        return Value::Bytes(un(b).into_iter().map(|v| v as u8).collect());
    }
    Value::Text(un(raw).into_iter().filter_map(char::from_u32).collect())
}

/// Reads the `[hosts]` array of Moonlight.conf.
pub fn parse(text: &str) -> Vec<Known> {
    use std::collections::BTreeMap;
    let mut in_hosts = false;
    let mut hosts: BTreeMap<u32, BTreeMap<String, Value>> = BTreeMap::new();
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') && l.ends_with(']') {
            in_hosts = l == "[hosts]";
            continue;
        }
        if !in_hosts {
            continue;
        }
        let Some((k, v)) = l.split_once('=') else { continue };
        let Some((idx, key)) = k.trim().split_once('\\') else { continue };
        let Ok(idx) = idx.parse::<u32>() else { continue };
        hosts.entry(idx).or_default().insert(key.to_string(), value(v));
    }
    hosts
        .into_values()
        .filter_map(|h| {
            let get = |k: &str| h.get(k).map(Value::text).unwrap_or_default();
            let uuid = get("uuid");
            if uuid.is_empty() {
                return None;
            }
            let mac = match h.get("mac") {
                Some(Value::Bytes(b)) if b.len() == 6 && b.iter().any(|x| *x != 0) => Some([b[0], b[1], b[2], b[3], b[4], b[5]]),
                _ => None,
            };
            let paired = h.get("srvcert").map(Value::text).is_some_and(|c| c.contains("BEGIN CERTIFICATE"));
            let mut addresses = Vec::new();
            for (a, p) in [("localaddress", "localport"), ("manualaddress", "manualport"), ("remoteaddress", "remoteport"), ("ipv6address", "ipv6port")] {
                let addr = get(a);
                if addr.is_empty() || addresses.iter().any(|x: &String| x.starts_with(&addr)) {
                    continue;
                }
                let port = get(p).parse::<u16>().unwrap_or(super::discover::HTTP_PORT);
                addresses.push(if port == super::discover::HTTP_PORT {
                    addr
                } else if addr.contains(':') {
                    format!("[{addr}]:{port}")
                } else {
                    format!("{addr}:{port}")
                });
            }
            let mut apps: BTreeMap<u32, (u32, String, bool)> = BTreeMap::new();
            for (k, v) in &h {
                let mut parts = k.split('\\');
                if parts.next() != Some("apps") {
                    continue;
                }
                let (Some(Ok(n)), Some(field)) = (parts.next().map(str::parse::<u32>), parts.next()) else { continue };
                let e = apps.entry(n).or_insert((0, String::new(), false));
                match field {
                    "id" => e.0 = v.text().parse().unwrap_or(0),
                    "name" => e.1 = v.text(),
                    "hidden" => e.2 = v.text() == "true",
                    _ => {}
                }
            }
            Some(Known {
                uuid,
                name: get("hostname"),
                mac,
                addresses,
                paired,
                apps: apps.into_values().filter(|a| a.0 != 0 && !a.1.is_empty()).collect(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // As Moonlight-Qt 6 writes it (QSettings INI: arrays as N\key, byte arrays escaped).
    const CONF: &str = r#"[General]
latestsupportedversion-v1=6.2.0

[hosts]
1\apps\1\directlaunch=false
1\apps\1\hdr=false
1\apps\1\hidden=false
1\apps\1\id=881448767
1\apps\1\name=Desktop
1\apps\2\hidden=true
1\apps\2\id=1093255277
1\apps\2\name=Steam Big Picture
1\apps\size=2
1\customname=false
1\hostname=Gaming-PC
1\ipv6address=
1\ipv6port=0
1\localaddress=192.168.0.20
1\localport=47989
1\mac=@ByteArray(\xa8\xa1Y\x12\x34V)
1\manualaddress=
1\manualport=0
1\nvidiasw=false
1\remoteaddress=203.0.113.9
1\remoteport=47989
1\srvcert="@ByteArray(-----BEGIN CERTIFICATE-----\nMIIC1zCCAb+gAwIBAgIBADANBg\n-----END CERTIFICATE-----\n)"
1\uuid=5F6A2B10-71C3-4E7D-9A55-0B1C2D3E4F50
2\hostname=Living room
2\localaddress=192.168.0.31
2\localport=48989
2\mac=@ByteArray(\0\0\0\0\0\0)
2\srvcert=@ByteArray()
2\uuid=1111-2222
size=2
"#;

    #[test]
    fn reads_paired_hosts() {
        let k = parse(CONF);
        assert_eq!(k.len(), 2);
        let pc = &k[0];
        assert_eq!(pc.name, "Gaming-PC");
        assert_eq!(pc.uuid, "5F6A2B10-71C3-4E7D-9A55-0B1C2D3E4F50");
        assert!(pc.paired);
        // \x12 then \x34: QSettings escapes a hex digit that follows an escape.
        assert_eq!(pc.mac, Some([0xa8, 0xa1, b'Y', 0x12, 0x34, b'V']));
        assert_eq!(pc.addresses, vec!["192.168.0.20".to_string(), "203.0.113.9".to_string()]);
        assert_eq!(pc.apps, vec![(881448767, "Desktop".to_string(), false), (1093255277, "Steam Big Picture".to_string(), true)]);
        let lr = &k[1];
        assert!(!lr.paired);
        assert_eq!(lr.mac, None);
        assert_eq!(lr.addresses, vec!["192.168.0.31:48989".to_string()]);
    }

    #[test]
    fn unescapes_qsettings() {
        assert_eq!(value(r#""a\"b""#), Value::Text("a\"b".into()));
        assert_eq!(value(r"@ByteArray(\0\x1a+<M^)"), Value::Bytes(vec![0, 0x1a, b'+', b'<', b'M', b'^']));
        assert_eq!(value(r"caf\xe9"), Value::Text("café".into()));
        assert_eq!(parse("[hosts]\nsize=0\n"), vec![]);
    }
}
