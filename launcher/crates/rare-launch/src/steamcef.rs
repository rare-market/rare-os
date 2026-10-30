//! Steam's own client API (`SteamClient.*` in Steam's UI, the calls its Downloads page and install
//! dialog make) reached through Steam's CEF remote debugging on 127.0.0.1:8080, the way Decky
//! Loader does on the Steam Deck. Steam opens that port when `.cef-enable-remote-debugging` is in
//! its folder at start; `enable` puts it there and Rare's next Steam start picks it up.
//!
//! `eval` runs one expression in Steam's shared JS context over the DevTools WebSocket and returns
//! its value as JSON. Plain std networking: a local, unencrypted socket and one request at a time.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant};

pub const PORT: u16 = 8080;
const FLAG: &str = ".cef-enable-remote-debugging";

/// Puts the flag file in Steam's folder. True when it was missing (Steam needs a restart to open
/// the port).
pub fn enable(root: &Path) -> bool {
    let f = root.join(FLAG);
    if f.exists() {
        return false;
    }
    std::fs::write(&f, b"").is_ok()
}

fn addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], PORT))
}

/// A plain HTTP GET on the DevTools port (the target list).
fn http_get(path: &str, timeout: Duration) -> Result<String, String> {
    let mut s = TcpStream::connect_timeout(&addr(), timeout).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(timeout)).ok();
    s.set_write_timeout(Some(timeout)).ok();
    write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\nConnection: close\r\n\r\n").map_err(|e| e.to_string())?;
    // CEF keeps the connection open whatever the request says: read the head, then exactly the
    // body's Content-Length (or until it closes when there is none).
    let mut buf = Vec::new();
    let mut chunk = [0u8; 16384];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        let n = s.read(&mut chunk).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("no HTTP answer".into());
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
        return Err(head.lines().next().unwrap_or("").to_string());
    }
    let len = head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case("content-length").then(|| v.trim().parse::<usize>().ok()).flatten()
    });
    loop {
        if len.is_some_and(|n| buf.len() - head_end >= n) {
            break;
        }
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if len.is_none() && !buf[head_end..].is_empty() && matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => break,
            Err(e) => return Err(e.to_string()),
        }
    }
    let end = len.map(|n| (head_end + n).min(buf.len())).unwrap_or(buf.len());
    let body = String::from_utf8_lossy(&buf[head_end..end]).into_owned();
    if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        let mut out = String::new();
        let mut rest = body.as_str();
        while let Some((l, tail)) = rest.split_once("\r\n") {
            let n = usize::from_str_radix(l.trim(), 16).unwrap_or(0);
            if n == 0 || tail.len() < n {
                break;
            }
            out.push_str(&tail[..n]);
            rest = tail[n..].trim_start_matches("\r\n");
        }
        return Ok(out);
    }
    Ok(body)
}

/// True when Steam answers on the DevTools port.
pub fn up() -> bool {
    targets(Duration::from_millis(400)).is_ok_and(|t| !t.is_empty())
}

#[derive(Debug, Clone)]
struct Target {
    title: String,
    ws: String,
}

fn targets(timeout: Duration) -> Result<Vec<Target>, String> {
    let body = http_get("/json", timeout)?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    Ok(v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    let ws = t.get("webSocketDebuggerUrl")?.as_str()?.to_string();
                    Some(Target { title: t.get("title").and_then(|x| x.as_str()).unwrap_or("").to_string(), ws })
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Steam's shared JS context (where `SteamClient` lives for the whole client), else its main UI.
fn pick(ts: &[Target]) -> Option<&Target> {
    let rank = |t: &Target| -> u8 {
        match t.title.as_str() {
            "SharedJSContext" => 0,
            x if x.contains("Shared Context") => 1,
            "Steam Big Picture Mode" => 2,
            "Steam" | "SP" => 3,
            _ => 9,
        }
    };
    ts.iter().filter(|t| rank(t) < 9).min_by_key(|t| rank(t))
}

// ---- a minimal WebSocket client (RFC 6455: masked text frames out, plain frames in) ----

struct Ws {
    r: BufReader<TcpStream>,
    w: TcpStream,
}

impl Ws {
    fn open(url: &str, timeout: Duration) -> Result<Ws, String> {
        let path = url.strip_prefix("ws://").and_then(|r| r.find('/').map(|i| &r[i..])).ok_or("bad DevTools URL")?;
        let s = TcpStream::connect_timeout(&addr(), timeout).map_err(|e| e.to_string())?;
        s.set_read_timeout(Some(timeout)).ok();
        s.set_write_timeout(Some(timeout)).ok();
        let mut w = s.try_clone().map_err(|e| e.to_string())?;
        write!(
            w,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: cmFyZS1zdGVhbS1jZWYtMQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
        )
        .map_err(|e| e.to_string())?;
        let mut r = BufReader::new(s);
        let mut status = String::new();
        r.read_line(&mut status).map_err(|e| e.to_string())?;
        if !status.contains(" 101") {
            return Err(format!("Steam refused the connection: {}", status.trim()));
        }
        loop {
            let mut l = String::new();
            if r.read_line(&mut l).map_err(|e| e.to_string())? == 0 || l == "\r\n" {
                break;
            }
        }
        Ok(Ws { r, w })
    }

    fn send(&mut self, op: u8, data: &[u8]) -> Result<(), String> {
        let mut f = vec![0x80 | op];
        let n = data.len();
        if n < 126 {
            f.push(0x80 | n as u8);
        } else if n <= 0xffff {
            f.push(0x80 | 126);
            f.extend((n as u16).to_be_bytes());
        } else {
            f.push(0x80 | 127);
            f.extend((n as u64).to_be_bytes());
        }
        let seed = Instant::now().elapsed().subsec_nanos() ^ std::process::id().rotate_left(13) ^ 0x5a17_c3e1;
        let mask = seed.to_le_bytes();
        f.extend(mask);
        f.extend(data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        self.w.write_all(&f).map_err(|e| e.to_string())
    }

    /// The next whole text message (pings answered, fragments joined).
    fn recv(&mut self) -> Result<String, String> {
        let mut msg = Vec::new();
        loop {
            let mut h = [0u8; 2];
            self.r.read_exact(&mut h).map_err(|e| e.to_string())?;
            let fin = h[0] & 0x80 != 0;
            let op = h[0] & 0x0f;
            let masked = h[1] & 0x80 != 0;
            let mut n = (h[1] & 0x7f) as u64;
            if n == 126 {
                let mut b = [0u8; 2];
                self.r.read_exact(&mut b).map_err(|e| e.to_string())?;
                n = u16::from_be_bytes(b) as u64;
            } else if n == 127 {
                let mut b = [0u8; 8];
                self.r.read_exact(&mut b).map_err(|e| e.to_string())?;
                n = u64::from_be_bytes(b);
            }
            if n > 64 << 20 {
                return Err("message too large".into());
            }
            let mut mask = [0u8; 4];
            if masked {
                self.r.read_exact(&mut mask).map_err(|e| e.to_string())?;
            }
            let mut data = vec![0u8; n as usize];
            self.r.read_exact(&mut data).map_err(|e| e.to_string())?;
            if masked {
                for (i, b) in data.iter_mut().enumerate() {
                    *b ^= mask[i % 4];
                }
            }
            match op {
                0x8 => return Err("Steam closed the connection".into()),
                0x9 => {
                    self.send(0xA, &data)?;
                    continue;
                }
                0xA => continue,
                _ => msg.extend(data),
            }
            if fin {
                return Ok(String::from_utf8_lossy(&msg).into_owned());
            }
        }
    }
}

/// Runs `expr` in Steam's JS context (promises awaited) and returns its value. Errors when the
/// port is closed, no Steam UI context is up, or the expression throws.
pub fn eval(expr: &str, timeout: Duration) -> Result<serde_json::Value, String> {
    let ts = targets(Duration::from_millis(800))?;
    let t = pick(&ts).ok_or("Steam's UI is not up yet")?.clone();
    let mut ws = Ws::open(&t.ws, timeout)?;
    let req = serde_json::json!({
        "id": 1,
        "method": "Runtime.evaluate",
        "params": { "expression": expr, "awaitPromise": true, "returnByValue": true, "userGesture": true }
    });
    ws.send(0x1, req.to_string().as_bytes())?;
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() > deadline {
            return Err("Steam didn't answer".into());
        }
        let m = ws.recv()?;
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&m) else { continue };
        if v.get("id").and_then(|i| i.as_u64()) != Some(1) {
            continue;
        }
        if let Some(e) = v.get("error") {
            return Err(e.get("message").and_then(|x| x.as_str()).unwrap_or("DevTools error").to_string());
        }
        let r = v.get("result").cloned().unwrap_or_default();
        if let Some(ex) = r.get("exceptionDetails") {
            let text = ex.pointer("/exception/description").or_else(|| ex.get("text")).and_then(|x| x.as_str()).unwrap_or("script error");
            return Err(text.lines().next().unwrap_or("").to_string());
        }
        let _ = ws.send(0x8, &[]);
        return Ok(r.pointer("/result/value").cloned().unwrap_or(serde_json::Value::Null));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_shared_context() {
        let t = |title: &str| Target { title: title.into(), ws: format!("ws://127.0.0.1:8080/devtools/page/{title}") };
        let ts = vec![t("Steam"), t("SharedJSContext"), t("notificationtoasts")];
        assert_eq!(pick(&ts).unwrap().title, "SharedJSContext");
        assert_eq!(pick(&[t("notificationtoasts")]).map(|x| x.title.clone()), None);
        assert_eq!(pick(&[t("Steam Big Picture Mode"), t("SP")]).unwrap().title, "Steam Big Picture Mode");
    }

    #[test]
    fn enable_writes_the_flag_once() {
        let d = tempfile::tempdir().unwrap();
        assert!(enable(d.path()));
        assert!(d.path().join(FLAG).exists());
        assert!(!enable(d.path()));
    }
}
