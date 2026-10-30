//! The wallet sheets' small pure helpers: the amount pad's key rules (ui/sign/padKeys.ts), the
//! unlock sheet's words (ui/sign/unlockCopy.ts), plain error words (ui/onboard/flow.ts plainError),
//! message decoding (ui/sign/site.ts decodeMessage) and the auto-lock timer (ui/home/autoLock.ts).

/// Applies one key ("0"-"9", ".", "back") to the amount text. `decimals` caps the fraction digits.
pub fn pad_press(value: &str, key: &str, decimals: u32) -> String {
    if key == "back" {
        let mut v = value.to_string();
        v.pop();
        return v;
    }
    if key == "." {
        if decimals == 0 || value.contains('.') {
            return value.into();
        }
        return if value.is_empty() { "0.".into() } else { format!("{value}.") };
    }
    if key.len() != 1 || !key.as_bytes()[0].is_ascii_digit() {
        return value.into();
    }
    if let Some((_, frac)) = value.split_once('.') {
        return if frac.len() as u32 >= decimals { value.into() } else { format!("{value}{key}") };
    }
    if value.len() >= 15 {
        return value.into();
    }
    // no leading zeros: "0" then "5" is "5", and "0" then "0" stays "0"
    if value == "0" {
        return key.into();
    }
    format!("{value}{key}")
}

/// Text typed on a keyboard or pasted, cleaned to the same rules (the first "," reads as ".").
pub fn clean_typed(text: &str, decimals: u32) -> String {
    let text = text.replacen(',', ".", 1);
    text.chars().fold(String::new(), |v, ch| pad_press(&v, &ch.to_string(), decimals))
}

const MAX_TRIES: u32 = 5;

/// "Wrong PIN. 4 tries before a wait."
pub fn wrong_message(failures: u32, what: &str) -> String {
    let left = MAX_TRIES.saturating_sub(failures).max(1);
    format!("Wrong {what}. {left} {} before a wait.", if left == 1 { "try" } else { "tries" })
}

/// "30 s", "2 min".
pub fn wait_text(ms: u64) -> String {
    let s = ms.div_ceil(1000).max(1);
    if s >= 60 {
        format!("{} min", s.div_ceil(60))
    } else {
        format!("{s} s")
    }
}

/// Plain words for keystore and device errors (code and message). Never includes secrets.
pub fn plain_error(code: Option<&str>, message: &str, fallback: &str) -> String {
    let known = match code {
        Some("cancelled") => Some("Cancelled."),
        Some("timeout") => Some("That took too long. Try again."),
        Some("ble-off") => Some("Bluetooth is off. Turn it on and try again."),
        Some("notfound") => Some("No device found. Check that it is connected and try again."),
        Some("busy") => Some("The device is busy. Finish what it is showing, then try again."),
        Some("network") => Some("Couldn't reach the network. Check your connection and try again."),
        Some("wrong-pin") => Some("That PIN is wrong. Try again."),
        Some("unsupported") => Some("This is not supported on this device."),
        _ => None,
    };
    if let Some(k) = known {
        return k.into();
    }
    if !message.is_empty() && message.encode_utf16().count() < 140 {
        message.into()
    } else {
        fallback.into()
    }
}

pub const PLAIN_FALLBACK: &str = "That didn't go through. Try again.";

/// A message as UTF-8 text when it is printable, else None; plus its hex.
pub fn decode_message(bytes: &[u8]) -> (Option<String>, String) {
    let hex = crate::keys::hex(bytes);
    match std::str::from_utf8(bytes) {
        Ok(t) if !t.chars().any(|c| matches!(c, '\u{0}'..='\u{8}' | '\u{e}'..='\u{1f}')) => (Some(t.to_string()), hex),
        _ => (None, hex),
    }
}

/// Plain-words checks on the site a web game's request came from (ui/sign/site.ts siteRisks).
pub fn site_risks(url: Option<&str>) -> Vec<crate::review::Risk> {
    let Some(url) = url.filter(|u| !u.is_empty()) else { return vec![] };
    let risk = |id: &str, text: &str| crate::review::Risk { id: id.into(), text: text.into(), danger: true, requires_confirm: false };
    let parsed = url.trim().split_once("://").and_then(|(scheme, rest)| {
        let ok = !scheme.is_empty() && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
        let auth = rest.split(['/', '?', '#']).next().unwrap_or("");
        let host = auth.rsplit('@').next().unwrap_or("");
        let host = if host.starts_with('[') { host.split(']').next().map(|h| format!("{h}]")).unwrap_or_default() } else { host.split(':').next().unwrap_or("").to_string() };
        (ok && !host.is_empty()).then(|| (scheme.to_ascii_lowercase(), host.to_ascii_lowercase()))
    });
    let Some((scheme, host)) = parsed else { return vec![risk("site-invalid", "Could not verify this site address.")] };
    let mut out = vec![];
    if scheme == "http" && host != "localhost" && host != "127.0.0.1" {
        out.push(risk("site-insecure", "This site is not using HTTPS."));
    }
    if host.split('.').any(|p| p.starts_with("xn--")) {
        out.push(risk("site-lookalike", "This address uses special characters and may imitate another site."));
    }
    out
}

/// Auto-lock: after N idle minutes the vault locks. Any key, pointer or pad press is activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoLock {
    /// 0 = never
    pub minutes: u32,
    last_activity_ms: u64,
}

impl AutoLock {
    pub const OPTIONS: [u32; 4] = [0, 1, 5, 15];
    pub const DEFAULT: u32 = 15;

    /// A stored choice; anything else is the default.
    pub fn new(stored: Option<u32>, now_ms: u64) -> Self {
        let minutes = stored.filter(|m| Self::OPTIONS.contains(m)).unwrap_or(Self::DEFAULT);
        AutoLock { minutes, last_activity_ms: now_ms }
    }
    pub fn activity(&mut self, now_ms: u64) {
        self.last_activity_ms = now_ms;
    }
    /// When the vault should lock, if ever.
    pub fn deadline(&self) -> Option<u64> {
        (self.minutes > 0).then(|| self.last_activity_ms + self.minutes as u64 * 60_000)
    }
    pub fn due(&self, now_ms: u64) -> bool {
        self.deadline().is_some_and(|d| now_ms >= d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(keys: &str, d: u32) -> String {
        keys.chars().fold(String::new(), |v, k| {
            let key = if k == '<' { "back".to_string() } else { k.to_string() };
            pad_press(&v, &key, d)
        })
    }

    // padKeys.test.ts
    #[test]
    fn amount_pad() {
        assert_eq!(typed("12.5", 9), "12.5");
        assert_eq!(typed("1.2.3", 9), "1.23");
        assert_eq!(typed(".5", 9), "0.5");
        assert_eq!(typed("007", 9), "7");
        assert_eq!(typed("0.05", 9), "0.05");
        assert_eq!(typed("0.1234567", 4), "0.1234");
        assert_eq!(typed("1.5", 0), "15");
        assert_eq!(typed("1.5<<", 9), "1");
        assert_eq!(typed("<", 9), "");
        assert_eq!(clean_typed("1,5abc", 9), "1.5");
        assert_eq!(typed("1234567890123456", 9), "123456789012345");
    }

    // webgame.test.ts: unlock copy and message checks
    #[test]
    fn unlock_copy() {
        assert_eq!(wrong_message(1, "PIN"), "Wrong PIN. 4 tries before a wait.");
        assert_eq!(wrong_message(4, "PIN"), "Wrong PIN. 1 try before a wait.");
        assert_eq!(wrong_message(9, "passphrase"), "Wrong passphrase. 1 try before a wait.");
        assert_eq!(wait_text(30_000), "30 s");
        assert_eq!(wait_text(120_000), "2 min");
        assert_eq!(wait_text(0), "1 s");
    }

    #[test]
    fn messages_decode_to_text_or_hex() {
        assert_eq!(decode_message(b"Sign in to Game").0.as_deref(), Some("Sign in to Game"));
        let (t, h) = decode_message(&[0xff, 0xfe, 0x01]);
        assert_eq!((t, h.as_str()), (None, "fffe01"));
        assert_eq!(decode_message(b"a\x01b").0, None);
        assert_eq!(decode_message(b"line\nnext").0.as_deref(), Some("line\nnext"));
    }

    #[test]
    fn site_checks() {
        assert!(site_risks(Some("https://game.example")).is_empty());
        assert!(site_risks(None).is_empty());
        assert!(site_risks(Some("http://game.example"))[0].danger);
        assert_eq!(site_risks(Some("http://game.example"))[0].id, "site-insecure");
        assert!(site_risks(Some("http://localhost:3000")).is_empty());
        assert_eq!(site_risks(Some("https://xn--gme-6na.example"))[0].id, "site-lookalike");
        assert_eq!(site_risks(Some("not a url"))[0].id, "site-invalid");
    }

    #[test]
    fn plain_errors() {
        assert_eq!(plain_error(Some("cancelled"), "x", PLAIN_FALLBACK), "Cancelled.");
        assert_eq!(plain_error(Some("error"), "Use at least 6 digits", PLAIN_FALLBACK), "Use at least 6 digits");
        assert_eq!(plain_error(None, &"x".repeat(200), PLAIN_FALLBACK), PLAIN_FALLBACK);
        assert_eq!(plain_error(None, "", PLAIN_FALLBACK), PLAIN_FALLBACK);
    }

    #[test]
    fn auto_lock() {
        let mut a = AutoLock::new(Some(5), 0);
        assert_eq!(a.deadline(), Some(300_000));
        assert!(!a.due(299_999) && a.due(300_000));
        a.activity(100_000);
        assert!(!a.due(300_000));
        assert_eq!(AutoLock::new(Some(7), 0).minutes, 15);
        assert_eq!(AutoLock::new(None, 0).minutes, 15);
        assert_eq!(AutoLock::new(Some(0), 0).deadline(), None);
    }
}
