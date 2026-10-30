//! Who may call the Local API, from where, and how often. Pure logic (time is passed in), so the
//! rules are unit-tested without a server.
//!
//! - **Tokens.** One random 256-bit token per launch, bound to one running game (its library id).
//!   Native and Proton games get it in `RARE_TOKEN`; a web game gets its own, bound to the origin
//!   its window opened on. A new launch of the same game replaces the old token; a game that has
//!   stopped loses its token.
//! - **Origins.** Browsers send `Origin` on cross-site requests. A native token refuses any request
//!   that carries one (a web page somewhere on the device has no business with a game's token). A
//!   web token takes only its own origin. `Host` must be the loopback address and our port, which
//!   stops DNS rebinding.
//! - **Rate limits.** Per token: reads (`me`, `inventory`) and sheets (`sell`, `buy`, `claim`,
//!   `open`) have separate token buckets, one sheet at a time, two notice streams. Bad tokens are
//!   counted across all callers; past the limit every unauthenticated request waits a minute.

use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Native or Proton game (token in the environment).
    Native,
    /// Web game in Rare's window, at this origin ("https://game.example").
    Web { origin: String },
    /// A tool on the local network (Settings > Developer > Allow network apps): one token for the
    /// device, kept in the owner's config, never sent from a web page. Reads and `open` only.
    Network,
}

#[derive(Clone, Debug)]
pub struct Grant {
    pub token: String,
    /// Library id of the game ("local:...", "rare:<slug>", "web:<slug>").
    pub game_id: String,
    pub kind: Kind,
    pub issued: Instant,
}

#[derive(Default)]
pub struct Grants {
    list: Vec<Grant>,
}

/// Equal-time comparison, so a wrong token's timing says nothing about the right one.
pub fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl Grants {
    /// A new token for a game. Replaces the game's previous token of the same kind.
    pub fn issue(&mut self, token: String, game_id: &str, kind: Kind, now: Instant) -> String {
        let web = matches!(kind, Kind::Web { .. });
        self.list.retain(|g| !(g.game_id == game_id && matches!(g.kind, Kind::Web { .. }) == web));
        self.list.push(Grant { token: token.clone(), game_id: game_id.to_string(), kind, issued: now });
        token
    }

    pub fn find(&self, token: &str) -> Option<&Grant> {
        // Compare against every grant so the time taken doesn't depend on where a match is.
        let mut hit = None;
        for g in &self.list {
            if same(g.token.as_bytes(), token.as_bytes()) {
                hit = Some(g);
            }
        }
        hit
    }

    /// The web token for a game at an origin.
    pub fn web(&self, game_id: &str, origin: &str) -> Option<&Grant> {
        self.list.iter().find(|g| g.game_id == game_id && g.kind == Kind::Web { origin: origin.to_string() })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn revoke_game(&mut self, game_id: &str) {
        self.list.retain(|g| g.game_id != game_id);
    }

    /// Drops tokens of games that are no longer running (after `grace`, so a game just started counts).
    pub fn retain_running(&mut self, running: &[String], now: Instant, grace: Duration) -> Vec<String> {
        let mut gone = Vec::new();
        self.list.retain(|g| {
            let keep = running.contains(&g.game_id) || now.duration_since(g.issued) < grace;
            if !keep {
                gone.push(g.token.clone());
            }
            keep
        });
        gone
    }

    /// Whether some web game has this origin (CORS preflights carry no token).
    pub fn web_origin(&self, origin: &str) -> bool {
        self.list.iter().any(|g| matches!(&g.kind, Kind::Web { origin: o } if o == origin))
    }
}

/// `Authorization: Bearer <token>`.
pub fn bearer(header: Option<&str>) -> Option<&str> {
    let h = header?.trim();
    let (scheme, tok) = h.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let tok = tok.trim();
    (!tok.is_empty() && tok.len() <= 128).then_some(tok)
}

/// `Host` must name the loopback address and our port (no DNS rebinding through another name).
pub fn host_ok(host: Option<&str>, port: u16) -> bool {
    let Some(h) = host else { return false };
    let h = h.trim().to_ascii_lowercase();
    h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}") || h == format!("[::1]:{port}")
}

/// `Host` on the network listener: our port, and either an IP address (no name to rebind) or the
/// device's own network name (`suiplay` or `suiplay.local`).
pub fn host_ok_net(host: Option<&str>, port: u16, name: &str) -> bool {
    let Some(h) = host else { return false };
    let h = h.trim().to_ascii_lowercase();
    let Some(base) = h.strip_suffix(&format!(":{port}")) else { return false };
    if base.parse::<std::net::Ipv4Addr>().is_ok() {
        return true;
    }
    if let Some(v6) = base.strip_prefix('[').and_then(|b| b.strip_suffix(']')) {
        return v6.parse::<std::net::Ipv6Addr>().is_ok();
    }
    let name = name.trim().trim_end_matches(".local").to_ascii_lowercase();
    !name.is_empty() && (base == name || base == format!("{name}.local"))
}

#[derive(Debug, PartialEq, Eq)]
pub enum OriginError {
    /// A native token from a web page.
    NativeFromWeb,
    /// A web token from another origin, or without one.
    WrongOrigin,
}

/// The origin rule for a token (see the module docs).
pub fn origin_ok(grant: &Grant, origin: Option<&str>) -> Result<(), OriginError> {
    let origin = origin.map(str::trim).filter(|o| !o.is_empty());
    match (&grant.kind, origin) {
        (Kind::Native | Kind::Network, None) => Ok(()),
        (Kind::Native | Kind::Network, Some(_)) => Err(OriginError::NativeFromWeb),
        (Kind::Web { origin: want }, Some(o)) if o == want => Ok(()),
        (Kind::Web { .. }, _) => Err(OriginError::WrongOrigin),
    }
}

/// What a call costs against the limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// me, inventory
    Read,
    /// sell, buy, claim, open (each shows the player something)
    Sheet,
    /// notices (a stream)
    Stream,
    /// launch, close (network apps)
    Act,
    /// input events (network apps); costs one per event
    Input,
    /// screenshots (network apps)
    Shot,
}

#[derive(Clone, Debug)]
struct Bucket {
    cap: f64,
    tokens: f64,
    per_sec: f64,
    last: Instant,
}

impl Bucket {
    fn new(cap: f64, per_sec: f64, now: Instant) -> Self {
        Bucket { cap, tokens: cap, per_sec, last: now }
    }
    /// Takes one; Err(seconds until one is back).
    fn take(&mut self, now: Instant) -> Result<(), u64> {
        self.take_n(1.0, now)
    }
    /// Takes `n` (at most the burst); Err(seconds until they are back).
    fn take_n(&mut self, n: f64, now: Instant) -> Result<(), u64> {
        let n = n.min(self.cap);
        let dt = now.saturating_duration_since(self.last).as_secs_f64();
        self.tokens = (self.tokens + dt * self.per_sec).min(self.cap);
        self.last = now;
        if self.tokens >= n {
            self.tokens -= n;
            Ok(())
        } else {
            Err(((n - self.tokens) / self.per_sec).ceil().max(1.0) as u64)
        }
    }
}

/// Reads: 20 at once, then 2 a second.
pub const READ_BURST: f64 = 20.0;
pub const READ_PER_SEC: f64 = 2.0;
/// Sheets: 5 at once, then one every 6 seconds (10 a minute).
pub const SHEET_BURST: f64 = 5.0;
pub const SHEET_PER_SEC: f64 = 1.0 / 6.0;
/// Launch and close: 5 at once, then one every 2 seconds.
pub const ACT_BURST: f64 = 5.0;
pub const ACT_PER_SEC: f64 = 0.5;
/// Input events: 100 at once, then 100 a second (a call of 16 events costs 16).
pub const INPUT_BURST: f64 = 100.0;
pub const INPUT_PER_SEC: f64 = 100.0;
/// Screenshots: 2 at once, then one every 2 seconds.
pub const SHOT_BURST: f64 = 2.0;
pub const SHOT_PER_SEC: f64 = 0.5;
/// Open notice streams per token.
pub const MAX_STREAMS: usize = 2;
/// Bad or missing tokens per minute, across all callers, before everyone unauthenticated waits.
pub const MAX_AUTH_FAILURES: u32 = 30;

#[derive(Debug, PartialEq, Eq)]
pub enum Limited {
    /// Too many; try again after this many seconds.
    Rate(u64),
    /// A sheet from this game is already up.
    SheetOpen,
    /// Too many notice streams.
    Streams,
}

struct PerToken {
    read: Bucket,
    sheet: Bucket,
    act: Bucket,
    input: Bucket,
    shot: Bucket,
    sheet_open: bool,
    streams: usize,
}

pub struct Limits {
    tokens: HashMap<String, PerToken>,
    fail_window: Instant,
    fails: u32,
}

impl Limits {
    pub fn new(now: Instant) -> Self {
        Limits { tokens: HashMap::new(), fail_window: now, fails: 0 }
    }

    fn entry(&mut self, token: &str, now: Instant) -> &mut PerToken {
        self.tokens.entry(token.to_string()).or_insert_with(|| PerToken {
            read: Bucket::new(READ_BURST, READ_PER_SEC, now),
            sheet: Bucket::new(SHEET_BURST, SHEET_PER_SEC, now),
            act: Bucket::new(ACT_BURST, ACT_PER_SEC, now),
            input: Bucket::new(INPUT_BURST, INPUT_PER_SEC, now),
            shot: Bucket::new(SHOT_BURST, SHOT_PER_SEC, now),
            sheet_open: false,
            streams: 0,
        })
    }

    /// Takes a call's cost; a sheet or stream that passes holds its slot until `release`.
    pub fn admit(&mut self, token: &str, class: Class, now: Instant) -> Result<(), Limited> {
        self.admit_cost(token, class, 1, now)
    }

    /// `admit` for a call that costs `cost` (input events).
    pub fn admit_cost(&mut self, token: &str, class: Class, cost: usize, now: Instant) -> Result<(), Limited> {
        let t = self.entry(token, now);
        match class {
            Class::Act => t.act.take(now).map_err(Limited::Rate),
            Class::Shot => t.shot.take(now).map_err(Limited::Rate),
            Class::Input => t.input.take_n(cost.max(1) as f64, now).map_err(Limited::Rate),
            Class::Read => t.read.take(now).map_err(Limited::Rate),
            Class::Sheet => {
                if t.sheet_open {
                    return Err(Limited::SheetOpen);
                }
                t.sheet.take(now).map_err(Limited::Rate)?;
                t.sheet_open = true;
                Ok(())
            }
            Class::Stream => {
                if t.streams >= MAX_STREAMS {
                    return Err(Limited::Streams);
                }
                t.read.take(now).map_err(Limited::Rate)?;
                t.streams += 1;
                Ok(())
            }
        }
    }

    pub fn release(&mut self, token: &str, class: Class) {
        if let Some(t) = self.tokens.get_mut(token) {
            match class {
                Class::Sheet => t.sheet_open = false,
                Class::Stream => t.streams = t.streams.saturating_sub(1),
                Class::Read | Class::Act | Class::Input | Class::Shot => {}
            }
        }
    }

    pub fn forget(&mut self, token: &str) {
        self.tokens.remove(token);
    }

    /// Whether unauthenticated callers are locked out right now (seconds left).
    pub fn auth_locked(&mut self, now: Instant) -> Option<u64> {
        let age = now.saturating_duration_since(self.fail_window);
        if age >= Duration::from_secs(60) {
            self.fail_window = now;
            self.fails = 0;
            return None;
        }
        (self.fails >= MAX_AUTH_FAILURES).then(|| 60 - age.as_secs())
    }

    pub fn auth_failed(&mut self, now: Instant) {
        if now.saturating_duration_since(self.fail_window) >= Duration::from_secs(60) {
            self.fail_window = now;
            self.fails = 0;
        }
        self.fails = self.fails.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grants(now: Instant) -> Grants {
        let mut g = Grants::default();
        g.issue("n".repeat(64), "local:game", Kind::Native, now);
        g.issue("w".repeat(64), "web:act", Kind::Web { origin: "https://act.example".into() }, now);
        g
    }

    #[test]
    fn tokens_map_to_one_game() {
        let now = Instant::now();
        let g = grants(now);
        assert_eq!(g.find(&"n".repeat(64)).unwrap().game_id, "local:game");
        assert_eq!(g.find(&"w".repeat(64)).unwrap().game_id, "web:act");
        assert!(g.find(&"x".repeat(64)).is_none());
        assert!(g.find("").is_none());
        assert!(g.find(&"n".repeat(63)).is_none());
    }

    #[test]
    fn a_new_launch_replaces_the_old_token() {
        let now = Instant::now();
        let mut g = grants(now);
        g.issue("m".repeat(64), "local:game", Kind::Native, now);
        assert!(g.find(&"n".repeat(64)).is_none());
        assert_eq!(g.find(&"m".repeat(64)).unwrap().game_id, "local:game");
        // The web token of another game stays.
        assert!(g.find(&"w".repeat(64)).is_some());
    }

    #[test]
    fn stopped_games_lose_their_tokens() {
        let now = Instant::now();
        let mut g = grants(now);
        let later = now + Duration::from_secs(30);
        let gone = g.retain_running(&["web:act".to_string()], later, Duration::from_secs(10));
        assert_eq!(gone, vec!["n".repeat(64)]);
        assert!(g.find(&"n".repeat(64)).is_none());
        assert!(g.find(&"w".repeat(64)).is_some());
        // Within the grace a just-launched game keeps its token even before it shows as running.
        g.issue("k".repeat(64), "local:new", Kind::Native, later);
        g.retain_running(&[], later + Duration::from_secs(2), Duration::from_secs(10));
        assert!(g.find(&"k".repeat(64)).is_some());
        g.revoke_game("local:new");
        assert!(g.find(&"k".repeat(64)).is_none());
    }

    #[test]
    fn bearer_header() {
        assert_eq!(bearer(Some("Bearer abc")), Some("abc"));
        assert_eq!(bearer(Some("bearer  abc ")), Some("abc"));
        assert_eq!(bearer(Some("Basic abc")), None);
        assert_eq!(bearer(Some("Bearer ")), None);
        assert_eq!(bearer(Some(&format!("Bearer {}", "a".repeat(129)))), None);
        assert_eq!(bearer(None), None);
    }

    #[test]
    fn host_must_be_loopback_and_our_port() {
        assert!(host_ok(Some("127.0.0.1:4100"), 4100));
        assert!(host_ok(Some("localhost:4100"), 4100));
        assert!(host_ok(Some("[::1]:4100"), 4100));
        assert!(!host_ok(Some("127.0.0.1:4101"), 4100));
        assert!(!host_ok(Some("evil.example:4100"), 4100));
        assert!(!host_ok(Some("127.0.0.1"), 4100));
        assert!(!host_ok(None, 4100));
    }

    #[test]
    fn network_host_is_an_ip_or_our_own_name() {
        assert!(host_ok_net(Some("192.168.1.92:47100"), 47100, "suiplay"));
        assert!(host_ok_net(Some("[fe80::1]:47100"), 47100, "suiplay"));
        assert!(host_ok_net(Some("SuiPlay.local:47100"), 47100, "suiplay"));
        assert!(host_ok_net(Some("suiplay:47100"), 47100, "suiplay.local"));
        assert!(!host_ok_net(Some("evil.example:47100"), 47100, "suiplay"));
        assert!(!host_ok_net(Some("192.168.1.92:47101"), 47100, "suiplay"));
        assert!(!host_ok_net(Some("192.168.1.92"), 47100, "suiplay"));
        assert!(!host_ok_net(None, 47100, "suiplay"));
        assert!(!host_ok_net(Some(".local:47100"), 47100, ""));
    }

    #[test]
    fn network_token_takes_no_origin() {
        let now = Instant::now();
        let mut g = Grants::default();
        g.issue("t".repeat(64), "device:network", Kind::Network, now);
        let net = g.find(&"t".repeat(64)).unwrap();
        assert_eq!(origin_ok(net, None), Ok(()));
        assert_eq!(origin_ok(net, Some("https://evil.example")), Err(OriginError::NativeFromWeb));
    }

    #[test]
    fn origin_rules() {
        let now = Instant::now();
        let g = grants(now);
        let native = g.find(&"n".repeat(64)).unwrap();
        let web = g.find(&"w".repeat(64)).unwrap();
        assert_eq!(origin_ok(native, None), Ok(()));
        assert_eq!(origin_ok(native, Some("https://act.example")), Err(OriginError::NativeFromWeb));
        assert_eq!(origin_ok(native, Some("null")), Err(OriginError::NativeFromWeb));
        assert_eq!(origin_ok(web, Some("https://act.example")), Ok(()));
        assert_eq!(origin_ok(web, Some("https://evil.example")), Err(OriginError::WrongOrigin));
        assert_eq!(origin_ok(web, Some("https://act.example.evil")), Err(OriginError::WrongOrigin));
        assert_eq!(origin_ok(web, None), Err(OriginError::WrongOrigin));
        assert!(g.web_origin("https://act.example"));
        assert!(!g.web_origin("https://evil.example"));
        assert!(g.web("web:act", "https://act.example").is_some());
        assert!(g.web("web:act", "https://evil.example").is_none());
    }

    #[test]
    fn reads_are_rate_limited_per_token() {
        let now = Instant::now();
        let mut l = Limits::new(now);
        for _ in 0..READ_BURST as usize {
            assert_eq!(l.admit("a", Class::Read, now), Ok(()));
        }
        assert_eq!(l.admit("a", Class::Read, now), Err(Limited::Rate(1)));
        // Another game's token has its own bucket.
        assert_eq!(l.admit("b", Class::Read, now), Ok(()));
        // Refills at READ_PER_SEC.
        assert_eq!(l.admit("a", Class::Read, now + Duration::from_millis(500)), Ok(()));
        assert!(l.admit("a", Class::Read, now + Duration::from_millis(500)).is_err());
    }

    #[test]
    fn one_sheet_at_a_time_and_ten_a_minute() {
        let now = Instant::now();
        let mut l = Limits::new(now);
        assert_eq!(l.admit("a", Class::Sheet, now), Ok(()));
        assert_eq!(l.admit("a", Class::Sheet, now), Err(Limited::SheetOpen));
        l.release("a", Class::Sheet);
        for _ in 1..SHEET_BURST as usize {
            assert_eq!(l.admit("a", Class::Sheet, now), Ok(()));
            l.release("a", Class::Sheet);
        }
        assert_eq!(l.admit("a", Class::Sheet, now), Err(Limited::Rate(6)));
        assert_eq!(l.admit("a", Class::Sheet, now + Duration::from_secs(6)), Ok(()));
    }

    #[test]
    fn input_is_limited_by_events() {
        let now = Instant::now();
        let mut l = Limits::new(now);
        assert_eq!(l.admit_cost("a", Class::Input, 16, now), Ok(()));
        for _ in 0..5 {
            assert_eq!(l.admit_cost("a", Class::Input, 16, now), Ok(()));
        }
        // 96 spent of 100: 16 more are not there yet, and come back within a second.
        assert_eq!(l.admit_cost("a", Class::Input, 16, now), Err(Limited::Rate(1)));
        assert_eq!(l.admit_cost("a", Class::Input, 16, now + Duration::from_millis(200)), Ok(()));
    }

    #[test]
    fn launch_and_screenshots_have_their_own_limits() {
        let now = Instant::now();
        let mut l = Limits::new(now);
        for _ in 0..ACT_BURST as usize {
            assert_eq!(l.admit("a", Class::Act, now), Ok(()));
        }
        assert_eq!(l.admit("a", Class::Act, now), Err(Limited::Rate(2)));
        assert_eq!(l.admit("a", Class::Shot, now), Ok(()));
        assert_eq!(l.admit("a", Class::Shot, now), Ok(()));
        assert_eq!(l.admit("a", Class::Shot, now), Err(Limited::Rate(2)));
    }

    #[test]
    fn two_notice_streams_per_token() {
        let now = Instant::now();
        let mut l = Limits::new(now);
        assert_eq!(l.admit("a", Class::Stream, now), Ok(()));
        assert_eq!(l.admit("a", Class::Stream, now), Ok(()));
        assert_eq!(l.admit("a", Class::Stream, now), Err(Limited::Streams));
        l.release("a", Class::Stream);
        assert_eq!(l.admit("a", Class::Stream, now), Ok(()));
    }

    #[test]
    fn bad_tokens_lock_everyone_unauthenticated_out_for_a_minute() {
        let now = Instant::now();
        let mut l = Limits::new(now);
        for _ in 0..MAX_AUTH_FAILURES {
            assert_eq!(l.auth_locked(now), None);
            l.auth_failed(now);
        }
        assert_eq!(l.auth_locked(now + Duration::from_secs(10)), Some(50));
        assert_eq!(l.auth_locked(now + Duration::from_secs(61)), None);
    }
}
