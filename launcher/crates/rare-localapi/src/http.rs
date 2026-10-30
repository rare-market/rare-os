//! Just enough HTTP/1.1 for the Local API: one request per connection, Content-Length bodies,
//! JSON answers, and a text/event-stream for notices. Plain std, no server crate: the surface is
//! seven routes on the loopback address, and every limit is explicit here.

use std::io::{BufRead, Read, Write};

/// Request line plus headers.
pub const MAX_HEAD: usize = 16 * 1024;
pub const MAX_HEADERS: usize = 64;
/// A body carries an item id or a voucher; nothing real is larger.
pub const MAX_BODY: usize = 32 * 1024;

#[derive(Debug)]
pub struct Request {
    pub method: String,
    /// Path without the query.
    pub path: String,
    #[allow(dead_code)]
    pub query: String,
    /// Names lowercased.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct HttpError {
    pub status: u16,
    pub message: &'static str,
}

const fn err(status: u16, message: &'static str) -> HttpError {
    HttpError { status, message }
}

fn line<R: BufRead>(r: &mut R, budget: &mut usize) -> Result<String, HttpError> {
    let mut buf = Vec::new();
    let n = r.by_ref().take(*budget as u64 + 1).read_until(b'\n', &mut buf).map_err(|_| err(400, "unreadable request"))?;
    if n == 0 {
        return Err(err(400, "empty request"));
    }
    if n > *budget {
        return Err(err(431, "request head too large"));
    }
    *budget -= n;
    if buf.last() != Some(&b'\n') {
        return Err(err(400, "unterminated line"));
    }
    while matches!(buf.last(), Some(b'\n' | b'\r')) {
        buf.pop();
    }
    String::from_utf8(buf).map_err(|_| err(400, "request head is not UTF-8"))
}

pub fn read_request<R: BufRead>(r: &mut R) -> Result<Request, HttpError> {
    let mut budget = MAX_HEAD;
    let first = line(r, &mut budget)?;
    let mut parts = first.split(' ');
    let (Some(method), Some(target), Some(version), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err(err(400, "bad request line"));
    };
    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Err(err(505, "HTTP/1.1 only"));
    }
    if !method.bytes().all(|c| c.is_ascii_uppercase()) || method.is_empty() {
        return Err(err(400, "bad method"));
    }
    if !target.starts_with('/') {
        return Err(err(400, "bad request target"));
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut headers = Vec::new();
    loop {
        let l = line(r, &mut budget)?;
        if l.is_empty() {
            break;
        }
        if headers.len() >= MAX_HEADERS {
            return Err(err(431, "too many headers"));
        }
        let (n, v) = l.split_once(':').ok_or(err(400, "bad header"))?;
        if n.is_empty() || n.bytes().any(|c| c.is_ascii_whitespace()) {
            return Err(err(400, "bad header"));
        }
        headers.push((n.to_ascii_lowercase(), v.trim().to_string()));
    }
    let mut req = Request { method: method.to_string(), path: path.to_string(), query: query.to_string(), headers, body: Vec::new() };
    if req.header("transfer-encoding").is_some() {
        return Err(err(411, "send a Content-Length body"));
    }
    let lens: Vec<&str> = req.headers.iter().filter(|(n, _)| n == "content-length").map(|(_, v)| v.as_str()).collect();
    if lens.len() > 1 {
        return Err(err(400, "more than one Content-Length"));
    }
    let len = match lens.first() {
        None => 0,
        Some(v) => v.parse::<usize>().map_err(|_| err(400, "bad Content-Length"))?,
    };
    if len > MAX_BODY {
        return Err(err(413, "body too large"));
    }
    if len > 0 {
        let mut body = vec![0u8; len];
        r.read_exact(&mut body).map_err(|_| err(400, "body shorter than Content-Length"))?;
        req.body = body;
    }
    Ok(req)
}

pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        421 => "Misdirected Request",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        _ => "Status",
    }
}

/// A complete response (Connection: close). `extra` are additional header lines.
pub fn response(status: u16, extra: &[(String, String)], content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n", reason(status), body.len());
    for (k, v) in extra {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    let mut b = out.into_bytes();
    b.extend_from_slice(body);
    b
}

pub fn json(status: u16, extra: &[(String, String)], v: &serde_json::Value) -> Vec<u8> {
    response(status, extra, "application/json", serde_json::to_string(v).unwrap_or_else(|_| "null".into()).as_bytes())
}

/// The head of an event stream; events follow with `event`.
pub fn sse_head(extra: &[(String, String)]) -> Vec<u8> {
    let mut out = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n".to_string();
    for (k, v) in extra {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    out.into_bytes()
}

/// One server-sent event. `data` is one JSON line (no newlines in serde_json output).
pub fn event(name: &str, data: &str) -> Vec<u8> {
    format!("event: {name}\ndata: {data}\n\n").into_bytes()
}

pub fn write_all<W: Write>(w: &mut W, bytes: &[u8]) -> bool {
    w.write_all(bytes).and_then(|_| w.flush()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn parse(s: &str) -> Result<Request, HttpError> {
        read_request(&mut Cursor::new(s.as_bytes().to_vec()))
    }

    #[test]
    fn reads_a_get() {
        let r = parse("GET /v1/me?x=1 HTTP/1.1\r\nHost: 127.0.0.1:4100\r\nAuthorization: Bearer abc\r\n\r\n").unwrap();
        assert_eq!(r.method, "GET");
        assert_eq!(r.path, "/v1/me");
        assert_eq!(r.query, "x=1");
        assert_eq!(r.header("host"), Some("127.0.0.1:4100"));
        assert_eq!(r.header("authorization"), Some("Bearer abc"));
        assert!(r.body.is_empty());
    }

    #[test]
    fn reads_a_post_body() {
        let body = r#"{"itemId":"0x1"}"#;
        let r = parse(&format!("POST /v1/sell HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}", body.len())).unwrap();
        assert_eq!(r.body, body.as_bytes());
    }

    #[test]
    fn refuses_what_it_does_not_need() {
        assert_eq!(parse("GET /v1/me HTTP/2\r\n\r\n").unwrap_err().status, 505);
        assert_eq!(parse("get /v1/me HTTP/1.1\r\n\r\n").unwrap_err().status, 400);
        assert_eq!(parse("GET http://x/v1/me HTTP/1.1\r\n\r\n").unwrap_err().status, 400);
        assert_eq!(parse("POST /v1/sell HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n").unwrap_err().status, 411);
        assert_eq!(parse(&format!("POST /v1/sell HTTP/1.1\r\nContent-Length: {}\r\n\r\n", MAX_BODY + 1)).unwrap_err().status, 413);
        assert_eq!(parse("POST /v1/sell HTTP/1.1\r\nContent-Length: 5\r\nContent-Length: 5\r\n\r\nhello").unwrap_err().status, 400);
        assert_eq!(parse("POST /v1/sell HTTP/1.1\r\nContent-Length: 10\r\n\r\nshort").unwrap_err().status, 400);
        assert_eq!(parse(&format!("GET /v1/me HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(MAX_HEAD))).unwrap_err().status, 431);
        assert_eq!(parse("GET /v1/me HTTP/1.1\r\nBad Header: 1\r\n\r\n").unwrap_err().status, 400);
        assert_eq!(parse("").unwrap_err().status, 400);
    }

    #[test]
    fn writes_json_and_events() {
        let r = String::from_utf8(json(200, &[], &serde_json::json!({"ok": true}))).unwrap();
        assert!(r.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(r.contains("Content-Length: 11\r\n"));
        assert!(r.ends_with("\r\n\r\n{\"ok\":true}"));
        assert_eq!(event("notice", "{}"), b"event: notice\ndata: {}\n\n");
    }
}
