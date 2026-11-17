//! Sending games from a phone or a computer over Wi-Fi: while the Add games page has it open,
//! the device serves a small page on the local network, and files dropped on it go straight into
//! the ROM folders (add.rs decides where).
//!
//! - Only while it is open, on a port of its own, and only under a random path the device shows
//!   (as a QR code and an address): nothing is reachable without it.
//! - Each file is one `POST /<token>/file?name=...&system=...` whose body is the file, streamed to a
//!   `.part` file and renamed when it is all there. A file whose type fits many systems (`.zip`,
//!   `.iso`) without a system named gets a 409 with the choices; the page asks and sends again.
//! - Plain std, one thread per connection, at most a few at once.

use crate::add;
use crate::systems::{by_id, SYSTEMS};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The first port tried; the next ones when it is taken.
pub const PORT: u16 = 47130;
const MAX_CONNS: usize = 4;
const MAX_HEAD: usize = 16 * 1024;
/// The biggest file taken (a Wii U or PS3 game can be tens of gigabytes).
const MAX_FILE: u64 = 128 << 30;

/// A file being received or received.
#[derive(Clone, Debug, PartialEq)]
pub struct Got {
    pub name: String,
    pub system: String,
    pub done: u64,
    pub total: u64,
    pub finished: bool,
    pub error: Option<String>,
}

/// What the page shows: the files of this session, newest last.
static GOT: Mutex<Vec<Got>> = Mutex::new(Vec::new());
static VER: AtomicU64 = AtomicU64::new(0);

pub fn received() -> Vec<Got> {
    GOT.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Moves whenever a file starts, moves on or ends.
pub fn version() -> u64 {
    VER.load(Ordering::Relaxed)
}

/// An open session: dropping it (or `stop`) closes the port.
pub struct Session {
    pub url: String,
    stop: Arc<AtomicBool>,
}

impl Session {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A random path: 128 bits from the kernel, as 26 lowercase letters and digits.
fn token() -> std::io::Result<String> {
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    const A: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut bits = u128::from_le_bytes(b);
    Ok((0..26).map(|_| {
        let c = A[(bits & 31) as usize] as char;
        bits >>= 5;
        c
    }).collect())
}

/// Opens the port on every interface and serves until stopped. `ip` is the address the page is
/// shown under; `roms` the ROM folder; `added` runs after each file (a library rescan).
pub fn start(ip: &str, roms: PathBuf, added: impl Fn() + Send + Sync + 'static) -> Result<Session, String> {
    let (listener, port) = (PORT..PORT + 20).find_map(|p| TcpListener::bind(("0.0.0.0", p)).ok().map(|l| (l, p))).ok_or("No free port to listen on")?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let tok = token().map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    GOT.lock().unwrap_or_else(|e| e.into_inner()).clear();
    VER.fetch_add(1, Ordering::Relaxed);
    let (st, t) = (stop.clone(), tok.clone());
    let added: Arc<dyn Fn() + Send + Sync> = Arc::new(added);
    std::thread::Builder::new()
        .name("rare-emu-upload".into())
        .spawn(move || {
            let live = Arc::new(AtomicUsize::new(0));
            while !st.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((s, _)) => {
                        if live.load(Ordering::Relaxed) >= MAX_CONNS {
                            continue;
                        }
                        let (live, t, roms, added, st) = (live.clone(), t.clone(), roms.clone(), added.clone(), st.clone());
                        live.fetch_add(1, Ordering::Relaxed);
                        std::thread::spawn(move || {
                            let _ = s.set_nonblocking(false);
                            let _ = s.set_read_timeout(Some(Duration::from_secs(60)));
                            serve(s, &t, &roms, &*added, &st);
                            live.fetch_sub(1, Ordering::Relaxed);
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(150)),
                    Err(_) => std::thread::sleep(Duration::from_millis(500)),
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Session { url: format!("http://{ip}:{port}/{tok}/"), stop })
}

struct Req {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    length: Option<u64>,
}

fn read_head(r: &mut BufReader<TcpStream>) -> Option<Req> {
    let mut line = String::new();
    let mut seen = r.read_line(&mut line).ok()?;
    let mut it = line.split_whitespace();
    let (method, target) = (it.next()?.to_string(), it.next()?.to_string());
    let mut length = None;
    loop {
        let mut h = String::new();
        let n = r.read_line(&mut h).ok()?;
        seen += n;
        if n == 0 || seen > MAX_HEAD {
            return None;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                length = v.trim().parse().ok();
            }
        }
    }
    let (path, q) = target.split_once('?').unwrap_or((&target, ""));
    let query = q.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (decode(k), decode(v))).collect();
    Some(Req { method, path: path.to_string(), query, length })
}

/// %XX and + decoding of a query value; a % not followed by two hex digits stays as it is.
fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |k: usize| b.get(k).and_then(|c| (*c as char).to_digit(16));
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if hex(i + 1).is_some() && hex(i + 2).is_some() => {
                out.push((hex(i + 1).unwrap() * 16 + hex(i + 2).unwrap()) as u8);
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn reply(s: &mut TcpStream, code: u16, kind: &str, body: &[u8]) {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Payload Too Large",
        _ => "Error",
    };
    let head = format!("HTTP/1.1 {code} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len());
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
}

fn json(s: &mut TcpStream, code: u16, v: serde_json::Value) {
    reply(s, code, "application/json", v.to_string().as_bytes());
}

fn serve(s: TcpStream, tok: &str, roms: &Path, added: &dyn Fn(), stop: &AtomicBool) {
    let Ok(w) = s.try_clone() else { return };
    let mut out = w;
    let mut r = BufReader::with_capacity(256 * 1024, s);
    let Some(req) = read_head(&mut r) else { return };
    let base = format!("/{tok}");
    let Some(rest) = req.path.strip_prefix(&base) else {
        return reply(&mut out, 404, "text/plain", b"Not found");
    };
    match (req.method.as_str(), rest) {
        // The page's links are relative to `/<token>/`.
        ("GET", "") => {
            let _ = write!(out, "HTTP/1.1 301 Moved Permanently\r\nLocation: {base}/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
        ("GET", "/") => reply(&mut out, 200, "text/html; charset=utf-8", PAGE.as_bytes()),
        ("GET", "/systems") => {
            let list: Vec<serde_json::Value> = SYSTEMS.iter().map(|s| serde_json::json!({ "id": s.id, "name": s.name, "exts": s.exts })).collect();
            json(&mut out, 200, serde_json::Value::Array(list));
        }
        ("POST", "/file") => receive(&mut r, &mut out, &req, roms, added, stop),
        _ => reply(&mut out, 404, "text/plain", b"Not found"),
    }
}

fn receive(r: &mut BufReader<TcpStream>, out: &mut TcpStream, req: &Req, roms: &Path, added: &dyn Fn(), stop: &AtomicBool) {
    let q = |k: &str| req.query.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone()).unwrap_or_default();
    let Some(name) = add::safe_name(&q("name")) else {
        return json(out, 400, serde_json::json!({ "error": "no name" }));
    };
    let sys = match by_id(&q("system")) {
        Some(s) if add::systems_for(&name).iter().any(|f| f.id == s.id) => s,
        Some(_) => return json(out, 400, serde_json::json!({ "error": "wrong system" })),
        None => match add::system_of(&name, None) {
            Some(s) => s,
            None => {
                let fits: Vec<serde_json::Value> = add::systems_for(&name).iter().map(|s| serde_json::json!({ "id": s.id, "name": s.name })).collect();
                // Not a game file at all: no choices to offer.
                let code = if fits.is_empty() { 400 } else { 409 };
                return json(out, code, serde_json::json!({ "error": "which system", "choices": fits }));
            }
        },
    };
    let Some(total) = req.length else {
        return json(out, 411, serde_json::json!({ "error": "length" }));
    };
    if total > MAX_FILE {
        return json(out, 413, serde_json::json!({ "error": "too big" }));
    }
    let dir = add::folder_in(roms, sys);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return json(out, 500, serde_json::json!({ "error": e.to_string() }));
    }
    let dst = add::unique(&dir, &name);
    let part = dst.with_file_name(format!("{}.part", dst.file_name().unwrap().to_string_lossy()));
    let idx = {
        let mut g = GOT.lock().unwrap_or_else(|e| e.into_inner());
        g.push(Got { name: dst.file_name().unwrap().to_string_lossy().into_owned(), system: sys.short.into(), done: 0, total, finished: false, error: None });
        g.len() - 1
    };
    VER.fetch_add(1, Ordering::Relaxed);
    let set = |f: &dyn Fn(&mut Got)| {
        if let Some(g) = GOT.lock().unwrap_or_else(|e| e.into_inner()).get_mut(idx) {
            f(g);
        }
        VER.fetch_add(1, Ordering::Relaxed);
    };
    let res = (|| -> Result<(), String> {
        let mut f = std::fs::File::create(&part).map_err(|e| e.to_string())?;
        let mut left = total;
        let mut buf = vec![0u8; 1 << 20];
        let mut since = 0u64;
        while left > 0 {
            if stop.load(Ordering::Relaxed) {
                return Err("closed on the device".into());
            }
            let n = r.read(&mut buf[..(left.min(1 << 20)) as usize]).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("the upload stopped".into());
            }
            f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            left -= n as u64;
            since += n as u64;
            if since >= 4 << 20 {
                since = 0;
                let d = total - left;
                set(&|g| g.done = d);
            }
        }
        f.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&part, &dst).map_err(|e| e.to_string())
    })();
    match res {
        Ok(()) => {
            set(&|g| {
                g.done = total;
                g.finished = true;
            });
            added();
            json(out, 200, serde_json::json!({ "ok": true, "system": sys.name, "file": dst.file_name().unwrap().to_string_lossy() }));
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            set(&|g| {
                g.finished = true;
                g.error = Some(e.clone());
            });
            json(out, 500, serde_json::json!({ "error": e }));
        }
    }
}

/// The page the phone or computer opens. Sends one file at a time, with its progress; asks for
/// the system when the device can't tell.
const PAGE: &str = include_str!("upload.html");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_decoded() {
        assert_eq!(decode("Super%20Mario+World.sfc"), "Super Mario World.sfc");
        assert_eq!(decode("%D0%98%D0%B3%D1%80%D0%B0.gba"), "Игра.gba");
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("%zz"), "%zz");
    }

    #[test]
    fn tokens_are_long_and_differ() {
        let (a, b) = (token().unwrap(), token().unwrap());
        assert_eq!(a.len(), 26);
        assert_ne!(a, b);
    }

    #[test]
    fn a_file_goes_into_its_system_folder() {
        let t = tempfile::tempdir().unwrap();
        let roms = t.path().to_path_buf();
        let s = start("127.0.0.1", roms.clone(), || {}).unwrap();
        let url = s.url.clone();
        let port: u16 = url.split(':').nth(2).unwrap().split('/').next().unwrap().parse().unwrap();
        let tok = url.trim_end_matches('/').rsplit('/').next().unwrap().to_string();
        let send = |path: &str, body: &[u8]| -> String {
            let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(c, "POST /{tok}{path} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n", body.len()).unwrap();
            c.write_all(body).unwrap();
            let mut s = String::new();
            c.read_to_string(&mut s).unwrap();
            s
        };
        let ok = send("/file?name=..%2FZelda.gba", b"rom");
        assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
        assert_eq!(std::fs::read(roms.join("gba/Zelda.gba")).unwrap(), b"rom");
        let ask = send("/file?name=Game.zip", b"zip");
        assert!(ask.starts_with("HTTP/1.1 409") && ask.contains("\"choices\""), "{ask}");
        let told = send("/file?name=Game.zip&system=snes", b"zip");
        assert!(told.starts_with("HTTP/1.1 200"), "{told}");
        assert!(roms.join("snes/Game.zip").exists());
        assert!(send("/file?name=notes.txt", b"x").starts_with("HTTP/1.1 400"));
        // Without the token: nothing.
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(c, "GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut r = String::new();
        c.read_to_string(&mut r).unwrap();
        assert!(r.starts_with("HTTP/1.1 404"));
        assert_eq!(received().iter().filter(|g| g.finished && g.error.is_none()).count(), 2);
        s.stop();
    }
}
