//! Discord's local RPC over a Unix socket ($XDG_RUNTIME_DIR/discord-ipc-N): little-endian
//! (op, length) headers and JSON bodies. Rare is a client here, the same as a game with Discord's
//! SDK: a handshake with an application id, then SET_ACTIVITY. Closing the connection clears the
//! activity, so a launcher that stops leaves nothing behind.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const OP_HANDSHAKE: u32 = 0;
pub const OP_FRAME: u32 = 1;
pub const OP_CLOSE: u32 = 2;
pub const OP_PING: u32 = 3;
pub const OP_PONG: u32 = 4;

pub fn encode(op: u32, body: &Value) -> Vec<u8> {
    let data = body.to_string().into_bytes();
    let mut out = Vec::with_capacity(8 + data.len());
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

/// One frame from the start of `buf`: (op, body, bytes used), or None when it is incomplete.
pub fn decode(buf: &[u8]) -> Option<Result<(u32, Value, usize), String>> {
    if buf.len() < 8 {
        return None;
    }
    let op = u32::from_le_bytes(buf[0..4].try_into().ok()?);
    let len = u32::from_le_bytes(buf[4..8].try_into().ok()?) as usize;
    if len > 1 << 20 {
        return Some(Err("frame too large".into()));
    }
    if buf.len() < 8 + len {
        return None;
    }
    Some(serde_json::from_slice(&buf[8..8 + len]).map(|v| (op, v, 8 + len)).map_err(|e| e.to_string()))
}

/// The sockets a Discord (or arRPC) may listen on in `dir`: discord-ipc-0 .. 9.
pub fn socket_paths(dir: &Path) -> Vec<PathBuf> {
    (0..10).map(|i| dir.join(format!("discord-ipc-{i}"))).collect()
}

pub fn runtime_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|p| p.is_dir())
}

/// A connected client.
pub struct Client {
    stream: UnixStream,
    buf: Vec<u8>,
    pub client_id: String,
    pub path: PathBuf,
    nonce: u64,
}

impl Client {
    /// Connects and handshakes; Ok once the server said READY.
    pub fn connect(path: &Path, client_id: &str) -> Result<Client, String> {
        let stream = UnixStream::connect(path).map_err(|e| format!("{}: {e}", path.display()))?;
        stream.set_read_timeout(Some(Duration::from_secs(4))).map_err(|e| e.to_string())?;
        stream.set_write_timeout(Some(Duration::from_secs(4))).map_err(|e| e.to_string())?;
        let mut c = Client { stream, buf: Vec::new(), client_id: client_id.to_string(), path: path.to_path_buf(), nonce: 0 };
        c.send(OP_HANDSHAKE, &json!({ "v": 1, "client_id": client_id }))?;
        loop {
            let (op, v) = c.read()?;
            match op {
                OP_CLOSE => return Err(format!("refused: {}", v["message"].as_str().unwrap_or("closed"))),
                OP_FRAME if v["evt"] == "READY" => return Ok(c),
                OP_FRAME if v["evt"] == "ERROR" => return Err(v["data"]["message"].as_str().unwrap_or("error").to_string()),
                _ => {}
            }
        }
    }

    fn send(&mut self, op: u32, body: &Value) -> Result<(), String> {
        self.stream.write_all(&encode(op, body)).map_err(|e| e.to_string())
    }

    fn read(&mut self) -> Result<(u32, Value), String> {
        loop {
            if let Some(r) = decode(&self.buf) {
                let (op, v, used) = r?;
                self.buf.drain(..used);
                if op == OP_PING {
                    self.send(OP_PONG, &v)?;
                    continue;
                }
                return Ok((op, v));
            }
            let mut chunk = [0u8; 4096];
            let n = self.stream.read(&mut chunk).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("closed".into());
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }

    /// Sets (or with None clears) this client's activity. Waits for the answer to its nonce.
    pub fn set_activity(&mut self, pid: u32, activity: Option<&Value>) -> Result<(), String> {
        self.nonce += 1;
        let nonce = format!("rare-{}", self.nonce);
        self.send(OP_FRAME, &json!({ "cmd": "SET_ACTIVITY", "args": { "pid": pid, "activity": activity }, "nonce": nonce }))?;
        loop {
            let (op, v) = self.read()?;
            if op == OP_CLOSE {
                return Err("closed".into());
            }
            if v["nonce"] == nonce.as_str() {
                if v["evt"] == "ERROR" {
                    return Err(v["data"]["message"].as_str().unwrap_or("refused").to_string());
                }
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn frames_round_trip() {
        let b = encode(OP_FRAME, &json!({"cmd": "SET_ACTIVITY"}));
        assert_eq!(&b[0..4], &1u32.to_le_bytes());
        assert_eq!(b.len(), 8 + r#"{"cmd":"SET_ACTIVITY"}"#.len());
        let (op, v, used) = decode(&b).unwrap().unwrap();
        assert_eq!((op, v["cmd"].as_str(), used), (1, Some("SET_ACTIVITY"), b.len()));
        assert!(decode(&b[..5]).is_none());
        assert!(decode(&b[..b.len() - 1]).is_none());
        let mut big = vec![1, 0, 0, 0];
        big.extend_from_slice(&(2u32 << 20).to_le_bytes());
        assert!(decode(&big).unwrap().is_err());
    }

    /// A fake server that behaves like arRPC: READY after the handshake, an echo per request.
    #[test]
    fn client_talks_to_a_server() {
        let dir = std::env::temp_dir().join(format!("rare-ipc-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("discord-ipc-0");
        let _ = std::fs::remove_file(&path);
        let l = UnixListener::bind(&path).unwrap();
        let t = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = Vec::new();
            let mut seen = Vec::new();
            loop {
                let mut chunk = [0u8; 4096];
                let n = s.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                while let Some(Ok((op, v, used))) = decode(&buf) {
                    buf.drain(..used);
                    if op == OP_HANDSHAKE {
                        assert_eq!(v["client_id"], "123");
                        s.write_all(&encode(OP_FRAME, &json!({"cmd": "DISPATCH", "evt": "READY", "data": {"v": 1}}))).unwrap();
                    } else {
                        seen.push(v.clone());
                        s.write_all(&encode(OP_FRAME, &json!({"cmd": v["cmd"], "data": v["args"]["activity"], "evt": null, "nonce": v["nonce"]}))).unwrap();
                    }
                }
            }
            seen
        });
        let mut c = Client::connect(&path, "123").unwrap();
        c.set_activity(42, Some(&json!({"name": "Celeste", "timestamps": {"start": 1}}))).unwrap();
        c.set_activity(42, None).unwrap();
        drop(c);
        let seen = t.join().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0]["args"]["activity"]["name"], "Celeste");
        assert_eq!(seen[0]["args"]["pid"], 42);
        assert!(seen[1]["args"]["activity"].is_null());
        let _ = std::fs::remove_dir_all(dir);
    }
}
