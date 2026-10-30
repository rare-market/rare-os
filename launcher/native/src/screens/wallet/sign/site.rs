//! Plain-words checks on the site a request came from (web games), the message decoder and
//! `hostOf`: launcher/src/wallet/ui/sign/site.ts. The URL handling follows what `new URL()` does
//! for these checks: the scheme, a special scheme's host (lower case, IDN labels as `xn--`
//! punycode), the port dropped when it is the scheme's default, and the inputs it refuses.

#[derive(Clone, Debug, PartialEq)]
pub struct Risk {
    pub id: String,
    pub text: String,
    /// red: real danger. Everything else renders muted.
    pub danger: bool,
    /// the person must tick an extra confirmation before signing
    pub requires_confirm: bool,
}

impl Risk {
    pub fn new(id: &str, text: &str, danger: bool) -> Risk {
        Risk { id: id.into(), text: text.into(), danger, requires_confirm: false }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Url {
    pub scheme: String,
    pub hostname: String,
    pub port: Option<u16>,
}

impl Url {
    /// `URL.host`: the hostname plus a non-default port.
    pub fn host(&self) -> String {
        match self.port {
            Some(p) => format!("{}:{p}", self.hostname),
            None => self.hostname.clone(),
        }
    }
}

fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        "ftp" => Some(21),
        _ => None,
    }
}

fn is_special(scheme: &str) -> bool {
    matches!(scheme, "http" | "https" | "ws" | "wss" | "ftp" | "file")
}

/// `new URL(s)` for what these checks read; None where it throws.
pub fn parse_url(input: &str) -> Option<Url> {
    let s = input.trim_matches(|c: char| c <= ' ');
    let colon = s.find(':')?;
    let scheme = &s[..colon];
    let mut chars = scheme.chars();
    if !chars.next()?.is_ascii_alphabetic() || !chars.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) {
        return None;
    }
    let scheme = scheme.to_ascii_lowercase();
    let rest = &s[colon + 1..];
    if !is_special(&scheme) {
        // Opaque or non-special URLs: a host only after "//".
        let Some(after) = rest.strip_prefix("//") else { return Some(Url { scheme, hostname: String::new(), port: None }) };
        let auth = &after[..after.find(['/', '?', '#']).unwrap_or(after.len())];
        let auth = auth.rsplit_once('@').map(|(_, h)| h).unwrap_or(auth);
        let (h, port) = split_port(auth)?;
        return Some(Url { scheme, hostname: h.to_string(), port });
    }
    let after = rest.trim_start_matches(['/', '\\']);
    let auth = &after[..after.find(['/', '\\', '?', '#']).unwrap_or(after.len())];
    let auth = auth.rsplit_once('@').map(|(_, h)| h).unwrap_or(auth);
    let (h, port) = split_port(auth)?;
    if h.is_empty() && scheme != "file" {
        return None;
    }
    let hostname = host_parse(h)?;
    let port = port.filter(|p| Some(*p) != default_port(&scheme));
    Some(Url { scheme, hostname, port })
}

fn split_port(auth: &str) -> Option<(&str, Option<u16>)> {
    if auth.starts_with('[') {
        let end = auth.find(']')?;
        let (h, rest) = auth.split_at(end + 1);
        let port = match rest.strip_prefix(':') {
            Some("") | None => None,
            Some(p) => Some(p.parse::<u16>().ok()?),
        };
        return Some((h, port));
    }
    match auth.rsplit_once(':') {
        Some((h, "")) => Some((h, None)),
        Some((h, p)) => {
            if !p.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            Some((h, Some(p.parse::<u16>().ok()?)))
        }
        None => Some((auth, None)),
    }
}

fn host_parse(h: &str) -> Option<String> {
    if h.starts_with('[') {
        return Some(h.to_ascii_lowercase());
    }
    const FORBIDDEN: &str = " #%/:<>?@[\\]^|\t\n\r";
    let lower = h.to_lowercase();
    let mut labels = Vec::new();
    for label in lower.split('.') {
        if label.chars().any(|c| FORBIDDEN.contains(c)) {
            return None;
        }
        if label.is_ascii() {
            labels.push(label.to_string());
        } else {
            labels.push(format!("xn--{}", punycode(label)?));
        }
    }
    Some(labels.join("."))
}

/// RFC 3492 punycode of one label (the part after `xn--`).
fn punycode(input: &str) -> Option<String> {
    const BASE: u32 = 36;
    const TMIN: u32 = 1;
    const TMAX: u32 = 26;
    const SKEW: u32 = 38;
    const DAMP: u32 = 700;
    fn digit(d: u32) -> char {
        (if d < 26 { b'a' + d as u8 } else { b'0' + (d - 26) as u8 }) as char
    }
    fn adapt(mut delta: u32, num: u32, first: bool) -> u32 {
        delta = if first { delta / DAMP } else { delta / 2 };
        delta += delta / num;
        let mut k = 0;
        while delta > ((BASE - TMIN) * TMAX) / 2 {
            delta /= BASE - TMIN;
            k += BASE;
        }
        k + (((BASE - TMIN + 1) * delta) / (delta + SKEW))
    }
    let cps: Vec<u32> = input.chars().map(|c| c as u32).collect();
    let mut out: String = input.chars().filter(|c| c.is_ascii()).collect();
    let b = out.len() as u32;
    let mut h = b;
    if b > 0 {
        out.push('-');
    }
    let (mut n, mut delta, mut bias) = (128u32, 0u32, 72u32);
    while (h as usize) < cps.len() {
        let m = *cps.iter().filter(|&&c| c >= n).min()?;
        delta = delta.checked_add((m - n).checked_mul(h + 1)?)?;
        n = m;
        for &c in &cps {
            if c < n {
                delta = delta.checked_add(1)?;
            }
            if c == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let t = if k <= bias { TMIN } else if k >= bias + TMAX { TMAX } else { k - bias };
                    if q < t {
                        break;
                    }
                    out.push(digit(t + (q - t) % (BASE - t)));
                    q = (q - t) / (BASE - t);
                    k += BASE;
                }
                out.push(digit(q));
                bias = adapt(delta, h + 1, h == b);
                delta = 0;
                h += 1;
            }
        }
        delta += 1;
        n += 1;
    }
    Some(out)
}

/// `siteRisks`: no URL, no checks.
pub fn site_risks(url: Option<&str>) -> Vec<Risk> {
    let Some(url) = url.filter(|u| !u.is_empty()) else { return Vec::new() };
    let mut out = Vec::new();
    match parse_url(url) {
        None => out.push(Risk::new("site-invalid", "Could not verify this site address.", true)),
        Some(u) => {
            if u.scheme == "http" && !(u.hostname == "localhost" || u.hostname == "127.0.0.1") {
                out.push(Risk::new("site-insecure", "This site is not using HTTPS.", true));
            }
            if u.hostname.split('.').any(|p| p.starts_with("xn--")) {
                out.push(Risk::new("site-lookalike", "This address uses special characters and may imitate another site.", true));
            }
        }
    }
    out
}

/// `hostOf`: the URL's host, or the input when it does not parse.
pub fn host_of(url: Option<&str>) -> String {
    match url {
        Some(u) if !u.is_empty() => parse_url(u).map(|p| p.host()).unwrap_or_else(|| u.to_string()),
        _ => String::new(),
    }
}

/// `decodeMessage`: UTF-8 text when the bytes are printable, else None; plus the hex.
pub struct Decoded {
    pub text: Option<String>,
    pub hex: String,
}

pub fn decode_message(bytes: &[u8]) -> Decoded {
    let hex = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let text = std::str::from_utf8(bytes).ok().filter(|t| !t.chars().any(|c| matches!(c as u32, 0x00..=0x08 | 0x0e..=0x1f))).map(|t| {
        // TextDecoder drops a leading byte order mark.
        t.strip_prefix('\u{feff}').unwrap_or(t).to_string()
    });
    Decoded { text, hex }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(url: &str) -> Vec<String> {
        site_risks(Some(url)).into_iter().map(|r| r.id).collect()
    }

    #[test]
    fn risks_like_new_url() {
        assert!(ids("https://act.game").is_empty());
        assert_eq!(ids("http://act.game"), ["site-insecure"]);
        assert!(ids("http://localhost:5173").is_empty());
        assert!(ids("http://127.0.0.1/x").is_empty());
        assert_eq!(ids("https://аpple.com"), ["site-lookalike"]);
        assert_eq!(ids("https://xn--pple-43d.com"), ["site-lookalike"]);
        assert_eq!(ids("not a url"), ["site-invalid"]);
        assert_eq!(ids("https://"), ["site-invalid"]);
        assert!(site_risks(None).is_empty());
    }

    #[test]
    fn hosts() {
        assert_eq!(host_of(Some("https://WordGame.example.com:443/play?x=1")), "wordgame.example.com");
        assert_eq!(host_of(Some("http://localhost:5173/")), "localhost:5173");
        assert_eq!(host_of(Some("https://аpple.com")), "xn--pple-43d.com");
        assert_eq!(host_of(Some("garbage")), "garbage");
        assert_eq!(host_of(None), "");
        assert_eq!(punycode("münchen").as_deref(), Some("mnchen-3ya"));
    }

    #[test]
    fn messages() {
        let d = decode_message(b"Sign in to ACT\nNonce: 12");
        assert_eq!(d.text.as_deref(), Some("Sign in to ACT\nNonce: 12"));
        let d = decode_message(&[0, 1, 2, 0xff]);
        assert!(d.text.is_none());
        assert_eq!(d.hex, "000102ff");
        assert!(decode_message(&[0xc3, 0x28]).text.is_none());
    }
}
