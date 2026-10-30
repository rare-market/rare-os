//! Handheld Daemon (hhd) client: TDP, CPU boost, EPP and the fan curve on Bazzite and Rare OS.
//!
//! hhd (github.com/hhd-dev/hhd, v4.1.x) runs as root and serves its REST API twice:
//! - the UNIX socket /run/hhd/api, chmod 0666 and without a token (src/hhd/http/api.py,
//!   `HHDHTTPServer.open`), which a user process can use directly;
//! - 127.0.0.1:5335 with a bearer token that hhd writes world-readable (0644) to /tmp/hhd/token
//!   and /etc/hhd/.token (src/hhd/__main__.py).
//!
//! `GET /api/v1/state` returns the whole state as a nested object; `POST /api/v1/state` takes a
//! partial state with dotted keys (src/hhd/plugins/conf.py `parse_conf`), applies it on hhd's next
//! loop and answers with the new state. `GET /api/v1/settings` returns what the device supports
//! (TDP range, fan, CPU boost, EPP options). Writing /etc/hhd/state.yml would need root, so the
//! API is the only path.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::time::Duration;

pub const SOCKET: &str = "/run/hhd/api";
pub const PORT: u16 = 5335;
const TOKEN_FILES: &[&str] = &["/tmp/hhd/token", "/etc/hhd/.token"];

// State keys (hhd v4.1.x).
/// The TDP plugin's on switch (adjustor/settings.yml `hhd.settings.tdp_enable`).
pub const K_TDP_ENABLE: &str = "hhd.settings.tdp_enable";
/// Set by hhd once TDP control is running (acpi_call works, no conflicting Decky plugin).
pub const K_TDP_READY: &str = "hhd.settings.tdp_ready";
pub const K_TDP_ERROR: &str = "tdp.tdp.tdp_error";
/// Sustained TDP in watts (int), adjustor/drivers/smu/qam.yml.
pub const K_TDP: &str = "tdp.qam.tdp";
/// TDP boost: fast/slow PPT above the sustained limit.
pub const K_TDP_BOOST: &str = "tdp.qam.boost";
pub const K_FAN_MODE: &str = "tdp.qam.fan.mode";
/// "enabled" once hhd's processor settings (amd_energy) are live; "conflict" while TuneD or
/// power-profiles-daemon run, "disabled" before TDP is ready.
pub const K_GPU_STATUS: &str = "hhd.steamos.gpu_status";
/// "auto" (hhd follows the power profile) or "manual" (the keys below apply).
pub const K_ENERGY_MODE: &str = "tdp.amd_energy.mode.mode";
/// "enabled" | "disabled".
pub const K_CPU_BOOST: &str = "tdp.amd_energy.mode.manual.cpu_boost";
/// EPP: "power" | "balance_power" | "balance_performance".
pub const K_CPU_PREF: &str = "tdp.amd_energy.mode.manual.cpu_pref";

/// hhd's edge-temperature fan curve points (adjustor/drivers/smu DEFAULT_EDGE): `st{C}` = fan %.
#[cfg(test)]
pub const FAN_POINTS: [u32; 9] = [40, 45, 50, 55, 60, 65, 70, 80, 90];
pub const FAN_EDGE: &str = "manual_edge";

// ---------- transport ----------

fn token() -> Option<String> {
    let home = dirs::home_dir().map(|h| h.join(".config/hhd/token"));
    TOKEN_FILES
        .iter()
        .map(std::path::PathBuf::from)
        .chain(home)
        .find_map(|p| std::fs::read_to_string(p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
}

fn request_bytes(method: &str, path: &str, token: Option<&str>, body: Option<&str>) -> Vec<u8> {
    let mut req = format!("{method} {path} HTTP/1.0\r\nHost: localhost\r\nAccept: application/json\r\n");
    if let Some(t) = token {
        req.push_str(&format!("Authorization: Bearer {t}\r\n"));
    }
    let body = body.unwrap_or("");
    if method == "POST" {
        req.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    req.push_str(body);
    req.into_bytes()
}

/// Splits an HTTP/1.x response into status and body.
pub fn parse_response(raw: &[u8]) -> Result<(u16, String), String> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text.split_once("\r\n\r\n").ok_or("Handheld Daemon sent an incomplete answer")?;
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .ok_or("Handheld Daemon sent an unreadable answer")?;
    Ok((status, body.to_string()))
}

fn exchange<S: Read + Write>(mut s: S, req: &[u8]) -> Result<(u16, String), String> {
    s.write_all(req).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    // hhd answers HTTP/1.0 and closes the connection, so read to the end.
    s.read_to_end(&mut out).map_err(|e| e.to_string())?;
    parse_response(&out)
}

/// One request: the UNIX socket first (no token), then localhost:5335 with the token.
fn call(method: &str, path: &str, body: Option<&str>, wait: Duration) -> Result<Value, String> {
    let mut errs = Vec::new();
    #[cfg(unix)]
    match std::os::unix::net::UnixStream::connect(SOCKET) {
        Ok(s) => {
            let _ = s.set_read_timeout(Some(wait));
            let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
            match exchange(s, &request_bytes(method, path, None, body)) {
                Ok(r) => return answer(r),
                Err(e) => errs.push(format!("{SOCKET}: {e}")),
            }
        }
        Err(e) => errs.push(format!("{SOCKET}: {e}")),
    }
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], PORT));
    match std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(400)) {
        Ok(s) => {
            let _ = s.set_read_timeout(Some(wait));
            let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
            let tok = token();
            answer(exchange(s, &request_bytes(method, path, tok.as_deref(), body))?)
        }
        Err(e) => {
            errs.push(format!("{addr}: {e}"));
            Err(format!("Handheld Daemon is not reachable ({})", errs.join("; ")))
        }
    }
}

fn answer((status, body): (u16, String)) -> Result<Value, String> {
    if status == 401 {
        return Err("Handheld Daemon refused the API token".into());
    }
    if status != 200 {
        return Err(format!("Handheld Daemon: {}", body.lines().skip(1).collect::<Vec<_>>().join(" ").trim()));
    }
    serde_json::from_str(&body).map_err(|e| format!("Handheld Daemon sent bad JSON: {e}"))
}

pub fn get_state() -> Result<Value, String> {
    call("GET", "/api/v1/state?lang=en", None, Duration::from_secs(3))
}

pub fn get_settings() -> Result<Value, String> {
    call("GET", "/api/v1/settings?lang=en", None, Duration::from_secs(3))
}

/// Applies changes on hhd's next loop (at most ~2 s) and returns the new state.
pub fn set_state(changes: &[(&str, Value)]) -> Result<Value, String> {
    call("POST", "/api/v1/state?lang=en", Some(&encode(changes)), Duration::from_secs(8))
}

// ---------- encode / decode ----------

/// A partial state with dotted keys, which hhd merges into its config.
pub fn encode(changes: &[(&str, Value)]) -> String {
    let mut m = Map::new();
    for (k, v) in changes {
        m.insert((*k).to_string(), v.clone());
    }
    Value::Object(m).to_string()
}

/// A dotted lookup in hhd's nested state.
pub fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    key.split('.').try_fold(v, |v, k| v.get(k))
}

fn num(v: &Value, key: &str) -> Option<f64> {
    get(v, key).and_then(Value::as_f64)
}

fn flag(v: &Value, key: &str) -> Option<bool> {
    get(v, key).and_then(Value::as_bool)
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    get(v, key).and_then(Value::as_str)
}

/// What the device supports, from `GET /api/v1/settings`. Present only while hhd's TDP plugin runs.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Caps {
    pub tdp_min: Option<f64>,
    pub tdp_max: Option<f64>,
    pub fan: bool,
    pub cpu_boost: bool,
    pub epp: Vec<String>,
}

pub fn decode_caps(settings: &Value) -> Caps {
    let qam = get(settings, "tdp.qam.children");
    let tdp = qam.and_then(|q| q.get("tdp"));
    let manual = get(settings, "tdp.amd_energy.children.mode.modes.manual.children");
    Caps {
        tdp_min: tdp.and_then(|t| t.get("min")).and_then(Value::as_f64),
        tdp_max: tdp.and_then(|t| t.get("max")).and_then(Value::as_f64),
        fan: qam.and_then(|q| get(q, "fan.modes.manual_edge")).is_some(),
        cpu_boost: manual.and_then(|m| m.get("cpu_boost")).is_some(),
        epp: manual
            .and_then(|m| get(m, "cpu_pref.options"))
            .and_then(Value::as_object)
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default(),
    }
}

/// The power-related part of hhd's state.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Power {
    pub tdp_enabled: bool,
    pub tdp_ready: bool,
    pub error: Option<String>,
    pub tdp: Option<f64>,
    pub tdp_boost: Option<bool>,
    /// hhd.steamos.tdp_min/max, published once TDP is ready.
    pub tdp_min: Option<f64>,
    pub tdp_max: Option<f64>,
    /// hhd's processor settings are running (not blocked by TuneD/PPD).
    pub energy_live: bool,
    /// TuneD or power-profiles-daemon block hhd's processor settings.
    pub energy_conflict: bool,
    pub energy_manual: bool,
    pub cpu_boost: Option<bool>,
    pub cpu_pref: Option<String>,
    pub fan_mode: Option<String>,
    pub fan_curve: BTreeMap<u32, u32>,
}

pub fn decode(state: &Value) -> Power {
    let fan_mode = text(state, K_FAN_MODE).map(String::from);
    let fan_curve = fan_mode
        .as_deref()
        .and_then(|m| get(state, &format!("tdp.qam.fan.{m}")))
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| Some((k.strip_prefix("st")?.parse().ok()?, v.as_f64()?.round() as u32)))
                .collect()
        })
        .unwrap_or_default();
    Power {
        tdp_enabled: flag(state, K_TDP_ENABLE).unwrap_or(false),
        tdp_ready: flag(state, K_TDP_READY).unwrap_or(false),
        error: text(state, K_TDP_ERROR).map(str::trim).filter(|s| !s.is_empty()).map(String::from),
        tdp: num(state, K_TDP),
        tdp_boost: flag(state, K_TDP_BOOST),
        tdp_min: num(state, "hhd.steamos.tdp_min"),
        tdp_max: num(state, "hhd.steamos.tdp_max"),
        energy_live: text(state, K_GPU_STATUS) == Some("enabled"),
        energy_conflict: text(state, K_GPU_STATUS) == Some("conflict"),
        energy_manual: text(state, K_ENERGY_MODE) == Some("manual"),
        cpu_boost: text(state, K_CPU_BOOST).map(|s| s == "enabled"),
        cpu_pref: text(state, K_CPU_PREF).map(String::from),
        fan_mode,
        fan_curve,
    }
}

/// Changes that turn the TDP plugin on.
pub fn enable_changes() -> Vec<(&'static str, Value)> {
    vec![(K_TDP_ENABLE, json!(true))]
}

/// Changes that pin CPU boost (hhd's processor settings go to manual so it sticks).
pub fn boost_changes(on: bool, epp: Option<&str>) -> Vec<(&'static str, Value)> {
    let mut c = vec![(K_ENERGY_MODE, json!("manual")), (K_CPU_BOOST, json!(if on { "enabled" } else { "disabled" }))];
    if let Some(e) = epp {
        c.push((K_CPU_PREF, json!(e)));
    }
    c
}

/// Changes for a fan curve; `None` hands the fan back to the EC's own curve.
pub fn fan_changes(curve: Option<&[(u32, u32)]>) -> Vec<(String, Value)> {
    match curve {
        None => vec![(K_FAN_MODE.to_string(), json!("disabled"))],
        Some(points) => std::iter::once((K_FAN_MODE.to_string(), json!(FAN_EDGE)))
            .chain(points.iter().map(|(t, pct)| (format!("tdp.qam.fan.{FAN_EDGE}.st{t}"), json!(pct))))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Value {
        json!({
            "hhd": {
                "settings": {"tdp_enable": true, "tdp_ready": true, "enforce_limits": true},
                "steamos": {"tdp_min": 4, "tdp_max": 30, "tdp_default": 15, "gpu_status": "enabled"}
            },
            "tdp": {
                "tdp": {"tdp_error": ""},
                "qam": {
                    "tdp": 15, "boost": false,
                    "fan": {"mode": "manual_edge", "manual_edge": {"info": "", "st40": 30, "st45": 30, "st90": 100, "reset": false}}
                },
                "amd_energy": {"mode": {"mode": "manual", "manual": {"cpu_boost": "disabled", "cpu_pref": "balance_power"}}}
            },
            "info": {}
        })
    }

    #[test]
    fn decodes_state() {
        let p = decode(&state());
        assert!(p.tdp_enabled && p.tdp_ready && p.energy_live && p.energy_manual);
        assert_eq!(p.tdp, Some(15.0));
        assert_eq!(p.tdp_boost, Some(false));
        assert_eq!((p.tdp_min, p.tdp_max), (Some(4.0), Some(30.0)));
        assert_eq!(p.cpu_boost, Some(false));
        assert_eq!(p.cpu_pref.as_deref(), Some("balance_power"));
        assert_eq!(p.fan_mode.as_deref(), Some("manual_edge"));
        assert_eq!(p.fan_curve, BTreeMap::from([(40, 30), (45, 30), (90, 100)]));
        assert_eq!(p.error, None);
    }

    #[test]
    fn decodes_the_owners_state() {
        // /etc/hhd/state.yml on the SuiPlay: TDP plugin off, nothing else set.
        let p = decode(&json!({"hhd": {"settings": {"tdp_enable": false}}, "tdp": {"tdp": {"tdp_enable": false}}}));
        assert!(!p.tdp_enabled && !p.tdp_ready && !p.energy_live);
        assert_eq!((p.tdp, p.cpu_boost, p.fan_mode), (None, None, None));
    }

    #[test]
    fn conflict_is_not_live() {
        let mut s = state();
        s["hhd"]["steamos"]["gpu_status"] = json!("conflict");
        s["tdp"]["tdp"]["tdp_error"] = json!("Can not write to 'acpi_call'.");
        let p = decode(&s);
        assert!(!p.energy_live && p.energy_conflict);
        assert_eq!(p.error.as_deref(), Some("Can not write to 'acpi_call'."));
    }

    #[test]
    fn decodes_caps() {
        let s = json!({
            "tdp": {
                "qam": {"type": "container", "children": {
                    "tdp": {"type": "int", "min": 4, "max": 30, "default": 15},
                    "boost": {"type": "bool"},
                    "fan": {"type": "mode", "modes": {"disabled": {}, "manual_edge": {"children": {"st40": {}}}}}
                }},
                "amd_energy": {"type": "container", "children": {"mode": {"type": "mode", "modes": {"auto": {}, "manual": {"children": {
                    "cpu_boost": {"type": "multiple"},
                    "cpu_pref": {"type": "multiple", "options": {"power": "Low", "balance_power": "Balanced", "balance_performance": "High"}}
                }}}}}}
            }
        });
        let c = decode_caps(&s);
        assert_eq!((c.tdp_min, c.tdp_max), (Some(4.0), Some(30.0)));
        assert!(c.fan && c.cpu_boost);
        assert_eq!(c.epp, vec!["balance_performance", "balance_power", "power"]);
        assert_eq!(decode_caps(&json!({"hhd": {}})), Caps::default());
    }

    #[test]
    fn encodes_dotted_keys() {
        let body = encode(&[(K_TDP, json!(15)), (K_TDP_BOOST, json!(false))]);
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v, json!({"tdp.qam.tdp": 15, "tdp.qam.boost": false}));
        assert_eq!(enable_changes(), vec![("hhd.settings.tdp_enable", json!(true))]);
        assert_eq!(
            boost_changes(false, Some("power")),
            vec![
                ("tdp.amd_energy.mode.mode", json!("manual")),
                ("tdp.amd_energy.mode.manual.cpu_boost", json!("disabled")),
                ("tdp.amd_energy.mode.manual.cpu_pref", json!("power")),
            ]
        );
    }

    #[test]
    fn encodes_fan() {
        assert_eq!(fan_changes(None), vec![("tdp.qam.fan.mode".to_string(), json!("disabled"))]);
        let c = fan_changes(Some(&[(40, 20), (90, 100)]));
        assert_eq!(c[0], ("tdp.qam.fan.mode".to_string(), json!("manual_edge")));
        assert_eq!(c[1], ("tdp.qam.fan.manual_edge.st40".to_string(), json!(20)));
        assert_eq!(c[2], ("tdp.qam.fan.manual_edge.st90".to_string(), json!(100)));
    }

    #[test]
    fn builds_requests() {
        let r = String::from_utf8(request_bytes("POST", "/api/v1/state", Some("abc"), Some("{}"))).unwrap();
        assert!(r.starts_with("POST /api/v1/state HTTP/1.0\r\n"));
        assert!(r.contains("Authorization: Bearer abc\r\n"));
        assert!(r.contains("Content-Length: 2\r\n"));
        assert!(r.ends_with("\r\n\r\n{}"));
        let g = String::from_utf8(request_bytes("GET", "/api/v1/state", None, None)).unwrap();
        assert!(!g.contains("Authorization") && !g.contains("Content-Length") && g.ends_with("\r\n\r\n"));
    }

    #[test]
    fn parses_responses() {
        let ok = b"HTTP/1.0 200 OK\r\nServer: BaseHTTP/0.6\r\nContent-type: application/json\r\n\r\n{\"a\": 1}";
        assert_eq!(parse_response(ok).unwrap(), (200, "{\"a\": 1}".to_string()));
        assert_eq!(answer(parse_response(ok).unwrap()).unwrap(), json!({"a": 1}));
        let denied = b"HTTP/1.0 401 Unauthorized\r\nContent-type: text/plain\r\n\r\nHandheld Daemon Error: Authentication is on";
        assert!(answer(parse_response(denied).unwrap()).unwrap_err().contains("token"));
        let bad = b"HTTP/1.0 400 Bad Request\r\n\r\nHandheld Daemon Error:\nState content should be a dictionary.";
        assert_eq!(answer(parse_response(bad).unwrap()).unwrap_err(), "Handheld Daemon: State content should be a dictionary.");
        assert!(parse_response(b"garbage").is_err());
    }

    #[test]
    fn talks_http_over_a_stream() {
        // A fake hhd on a socket pair: checks the request, answers like BaseHTTPRequestHandler.
        #[cfg(unix)]
        {
            use std::os::unix::net::UnixStream;
            let (a, mut b) = UnixStream::pair().unwrap();
            let t = std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let n = b.read(&mut buf).unwrap();
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                b.write_all(b"HTTP/1.0 200 OK\r\nContent-type: application/json\r\n\r\n{\"tdp\": {\"qam\": {\"tdp\": 8}}}").unwrap();
                req
            });
            let r = exchange(&a, &request_bytes("POST", "/api/v1/state", None, Some(&encode(&[(K_TDP, json!(8))])))).unwrap();
            drop(a);
            let req = t.join().unwrap();
            assert!(req.ends_with("{\"tdp.qam.tdp\":8}"));
            assert_eq!(decode(&answer(r).unwrap()).tdp, Some(8.0));
        }
    }
}
