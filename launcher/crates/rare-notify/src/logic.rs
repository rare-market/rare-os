//! The pure parts of notifications: notice ids (decimal strings that only grow), merging and
//! dedupe, which kinds are on, and what one arriving notice does (toast, queue while a game runs,
//! centre only, or nothing). No state, no I/O.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::{Notice, NoticeKind, NOTICE_KINDS};

/// Compares two decimal id strings without losing precision (ids can pass 2^53).
pub fn cmp_id(a: &str, b: &str) -> Ordering {
    fn strip(s: &str) -> &str {
        let t = s.trim_start_matches('0');
        // Keep one digit of an all-zero id, as the TS regex `^0+(?=\d)` does.
        if t.is_empty() && !s.is_empty() {
            &s[s.len() - 1..]
        } else {
            t
        }
    }
    let (x, y) = (strip(a), strip(b));
    x.len().cmp(&y.len()).then_with(|| x.cmp(y))
}

/// A notice id: 1 to 40 decimal digits.
pub fn is_id(v: &str) -> bool {
    !v.is_empty() && v.len() <= 40 && v.bytes().all(|c| c.is_ascii_digit())
}

/// The larger of two ids; none and junk lose.
pub fn max_id(a: Option<&str>, b: Option<&str>) -> Option<String> {
    let x = a.filter(|v| is_id(v));
    let y = b.filter(|v| is_id(v));
    match (x, y) {
        (None, y) => y.map(str::to_string),
        (x, None) => x.map(str::to_string),
        (Some(x), Some(y)) => Some(if cmp_id(x, y) != Ordering::Less { x } else { y }.to_string()),
    }
}

/// When a notice happened, for ordering, read and clear marks: server and local notices share one
/// timeline.
pub fn notice_time(n: &Notice) -> i64 {
    if n.created_at != 0 {
        n.created_at
    } else {
        n.cp_ts.filter(|t| *t != 0).unwrap_or(0)
    }
}

/// Newest first, one copy of each id, at most `cap`. Later copies of an id never replace the first.
pub fn merge_notices(existing: &[Notice], incoming: &[Notice], cap: usize) -> Vec<Notice> {
    let mut seen = HashSet::new();
    let mut out: Vec<Notice> = Vec::new();
    for n in existing.iter().chain(incoming) {
        if is_id(&n.id) && seen.insert(n.id.clone()) {
            out.push(n.clone());
        }
    }
    out.sort_by(|a, b| notice_time(b).cmp(&notice_time(a)).then_with(|| cmp_id(&b.id, &a.id)));
    out.truncate(cap);
    out
}

/// The default cap of the merged list.
pub const MERGE_CAP: usize = 100;

/// The ids in `incoming` not in `existing` (what is new to this launcher), in arrival order.
pub fn new_ids(existing: &[Notice], incoming: &[Notice]) -> Vec<String> {
    let have: HashSet<&str> = existing.iter().map(|n| n.id.as_str()).collect();
    let mut out: Vec<String> = Vec::new();
    for n in incoming {
        if is_id(&n.id) && !have.contains(n.id.as_str()) && !out.contains(&n.id) {
            out.push(n.id.clone());
        }
    }
    out
}

/// The query string for the stream: every account address (unique, lowercase, at most 20), the
/// device, and `after` so a launcher restart picks up where it left off. Encoded as
/// URLSearchParams does it.
pub fn stream_query(addrs: &[String], device: &str, after: Option<&str>) -> String {
    let mut q: Vec<(&str, String)> = Vec::new();
    let list = norm_addrs(addrs);
    if !list.is_empty() {
        q.push(("addr", list.join(",")));
    }
    q.push(("device", device.to_string()));
    if let Some(a) = after.filter(|a| is_id(a)) {
        q.push(("after", a.to_string()));
    }
    form_query(&q)
}

/// Addresses trimmed, lowercased, valid, unique and sorted, at most 20.
pub fn norm_addrs<S: AsRef<str>>(addrs: &[S]) -> Vec<String> {
    let mut v: Vec<String> = addrs
        .iter()
        .map(|a| a.as_ref().trim().to_lowercase())
        .filter(|a| a.len() >= 3 && a.len() <= 66 && a.starts_with("0x") && a[2..].bytes().all(|c| c.is_ascii_hexdigit()))
        .collect();
    v.sort();
    v.dedup();
    v.truncate(20);
    v
}

/// `application/x-www-form-urlencoded`, the encoding URLSearchParams uses.
pub fn form_query(pairs: &[(&str, String)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", form_enc(k), form_enc(v))).collect::<Vec<_>>().join("&")
}

fn form_enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// JavaScript's encodeURIComponent.
pub fn uri_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Notice classes. Each notice belongs to one class, and the
/// class decides what it does in the launcher, over a game and with Do not disturb.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NoticeClass {
    Money,
    Invites,
    /// Direct and party messages: they pop up over a game too.
    Messages,
    Trophies,
    Social,
    System,
    Battery,
}

pub const CLASSES: [NoticeClass; 7] =
    [NoticeClass::Money, NoticeClass::Invites, NoticeClass::Messages, NoticeClass::Trophies, NoticeClass::Social, NoticeClass::System, NoticeClass::Battery];

impl NoticeClass {
    pub fn label(&self) -> &'static str {
        match self {
            NoticeClass::Money => "Money",
            NoticeClass::Invites => "Invites",
            NoticeClass::Messages => "Messages",
            NoticeClass::Trophies => "Trophies",
            NoticeClass::Social => "Social",
            NoticeClass::System => "System",
            NoticeClass::Battery => "Battery",
        }
    }

    /// The model's table for this class.
    pub fn rule(&self) -> ClassRule {
        use Delivery::*;
        let r = |launcher, over_game, dnd| ClassRule { launcher, over_game, dnd };
        match self {
            NoticeClass::Money => r(Toast, Toast, List),
            NoticeClass::Invites => r(Toast, Toast, List),
            NoticeClass::Messages => r(Toast, Toast, List),
            NoticeClass::Trophies => r(Toast, Small, List),
            NoticeClass::Social => r(Toast, List, List),
            NoticeClass::System => r(Toast, List, List),
            NoticeClass::Battery => r(Toast, Toast, Toast),
        }
    }
}

/// The class of a kind. A kind from a newer server is Social: it toasts in the launcher and waits
/// otherwise.
pub fn class_of(kind: &NoticeKind) -> NoticeClass {
    use NoticeKind::*;
    match kind {
        Sold | Bought | OfferReceived | CoinReceived | NftReceived | Airdrop | PerpLiquidation | PriceAlert => NoticeClass::Money,
        PartyInvite => NoticeClass::Invites,
        PartyMessage | DirectMessage => NoticeClass::Messages,
        FollowedTrade | FriendRequest | FriendOnline => NoticeClass::Social,
        Trophy => NoticeClass::Trophies,
        OsUpdate | DownloadDone | EmuUpdate => NoticeClass::System,
        Battery => NoticeClass::Battery,
        Other(_) => NoticeClass::Social,
    }
}

/// [`class_of`] by wire name.
pub fn class_of_str(kind: &str) -> NoticeClass {
    class_of(&NoticeKind::parse(kind))
}

/// How a class shows up in one situation: a toast, a small toast (trophies over a game), or only
/// in the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    Toast,
    Small,
    List,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassRule {
    pub launcher: Delivery,
    /// Over a game a toast goes through the HUD.
    pub over_game: Delivery,
    pub dnd: Delivery,
}

/// The person's notification settings, stored as the web launcher stores them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    /// One switch per known kind, by wire name.
    pub kinds: BTreeMap<String, bool>,
    /// Toasts over a game, for the classes the model lets through (Money, Invites, Trophies,
    /// Battery). Off: they wait in the list.
    pub game_toasts: bool,
    /// A desktop notification when the launcher is not the focused window.
    pub desktop: bool,
    /// Do not disturb (Quick Settings): only Battery still toasts; everything else waits in the list.
    pub dnd: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { kinds: NOTICE_KINDS.iter().map(|k| (k.as_str().to_string(), true)).collect(), game_toasts: true, desktop: true, dnd: false }
    }
}

impl Prefs {
    /// Unknown kinds (a newer server) are shown: only a switched-off kind is hidden.
    pub fn kind_on(&self, kind: &NoticeKind) -> bool {
        self.kinds.get(kind.as_str()) != Some(&false)
    }
    pub fn set_kind(&mut self, kind: &NoticeKind, on: bool) {
        self.kinds.insert(kind.as_str().to_string(), on);
    }
}

/// Reads stored prefs, keeping defaults for anything missing or of the wrong type.
pub fn read_prefs(raw: &Value) -> Prefs {
    let mut p = Prefs::default();
    let Some(o) = raw.as_object() else { return p };
    if let Some(k) = o.get("kinds").and_then(Value::as_object) {
        for (name, on) in p.kinds.iter_mut() {
            if let Some(b) = k.get(name).and_then(Value::as_bool) {
                *on = b;
            }
        }
    }
    if let Some(b) = o.get("gameToasts").and_then(Value::as_bool) {
        p.game_toasts = b;
    }
    if let Some(b) = o.get("desktop").and_then(Value::as_bool) {
        p.desktop = b;
    }
    if let Some(b) = o.get("dnd").and_then(Value::as_bool) {
        p.dnd = b;
    }
    p
}

/// A notice is fresh enough to pop up when it was recorded in the last two minutes. Replays after a
/// restart are not.
pub const FRESH_MS: i64 = 120_000;

pub fn is_fresh(n: &Notice, now: i64) -> bool {
    now - notice_time(n) < FRESH_MS
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// Its kind is switched off.
    Drop,
    /// An old notice (from `recent` or a replay), or Do not disturb: into the list quietly.
    Centre,
    /// A game is running and its class does not toast over a game: into the list, counted for the
    /// bottom bar.
    Queue,
    /// Pop up with the notify sound.
    Toast,
}

/// What a class does right now: Do not disturb first, then a game in front, then the launcher.
pub fn delivery_for(cls: NoticeClass, dnd: bool, playing: bool, game_toasts: bool) -> Delivery {
    let r = cls.rule();
    if dnd {
        r.dnd
    } else if playing {
        if game_toasts || cls == NoticeClass::Battery {
            r.over_game
        } else {
            Delivery::List
        }
    } else {
        r.launcher
    }
}

/// The situation one arriving notice is routed in.
#[derive(Clone, Copy, Debug)]
pub struct RouteCtx<'a> {
    pub prefs: &'a Prefs,
    /// From the live stream (or made in the launcher now), not a backfill.
    pub live: bool,
    pub now: i64,
    pub playing: bool,
    pub focused: bool,
}

/// What an arriving notice does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Routing {
    pub route: Route,
    /// Also a desktop notification (the launcher is in the background).
    pub desktop: bool,
    /// Over a game: the HUD draws the toast.
    pub hud: bool,
    /// The small trophy toast.
    pub small: bool,
    pub cls: NoticeClass,
}

pub fn route(n: &Notice, ctx: &RouteCtx) -> Routing {
    let cls = class_of(&n.kind);
    let none = |route| Routing { route, desktop: false, hud: false, small: false, cls };
    if !ctx.prefs.kind_on(&n.kind) {
        return none(Route::Drop);
    }
    if !ctx.live || !is_fresh(n, ctx.now) {
        return none(Route::Centre);
    }
    let d = delivery_for(cls, ctx.prefs.dnd, ctx.playing, ctx.prefs.game_toasts);
    if d == Delivery::List {
        return none(if ctx.prefs.dnd { Route::Centre } else { Route::Queue });
    }
    Routing { route: Route::Toast, desktop: ctx.prefs.desktop && !ctx.focused && !ctx.playing, hud: ctx.playing, small: d == Delivery::Small, cls }
}

/// Counts what arrived while the device slept, and says what to show on wake.
#[derive(Clone, Debug, Default)]
pub struct WakeSummary {
    ids: HashSet<String>,
}

impl WakeSummary {
    pub fn add(&mut self, id: &str) {
        self.ids.insert(id.to_string());
    }
    pub fn count(&self) -> usize {
        self.ids.len()
    }
    pub fn flush(&mut self) -> Option<String> {
        let n = self.ids.len();
        self.ids.clear();
        (n > 0).then(|| format!("{n} new while you were away"))
    }
}

/// The notices the centre shows: kinds that are on, newer than the last Clear (a time, ms).
pub fn visible<'a>(notices: &'a [Notice], prefs: &Prefs, cleared_at: Option<i64>) -> Vec<&'a Notice> {
    notices.iter().filter(|n| prefs.kind_on(&n.kind) && cleared_at.map_or(true, |c| c == 0 || notice_time(n) > c)).collect()
}

pub fn unread_count(notices: &[Notice], prefs: &Prefs, cleared_at: Option<i64>, read: &HashSet<String>, read_at: Option<i64>) -> usize {
    visible(notices, prefs, cleared_at).into_iter().filter(|n| !is_read(n, read, read_at)).count()
}

/// Read: opened one by one, or at or before the last "Mark all read" (a time, ms).
pub fn is_read(n: &Notice, read: &HashSet<String>, read_at: Option<i64>) -> bool {
    read.contains(&n.id) || read_at.is_some_and(|t| t != 0 && notice_time(n) <= t)
}

/// Holds what arrived while a game ran, and says what to tell the person when it ends.
#[derive(Clone, Debug, Default)]
pub struct PlayQueue {
    ids: Vec<String>,
}

impl PlayQueue {
    pub fn add(&mut self, id: &str) {
        if !self.ids.iter().any(|x| x == id) {
            self.ids.push(id.to_string());
        }
    }
    pub fn count(&self) -> usize {
        self.ids.len()
    }
    /// The game ended: the summary line (or none when nothing came in), and the queue empties.
    pub fn flush(&mut self) -> Option<String> {
        let n = self.ids.len();
        self.ids.clear();
        match n {
            0 => None,
            1 => Some("1 new notification while you played".to_string()),
            n => Some(format!("{n} new notifications while you played")),
        }
    }
}

/// Latency samples: the last `cap`, oldest first.
pub fn push_sample<T>(list: &mut Vec<T>, s: T, cap: usize) {
    list.push(s);
    if list.len() > cap {
        list.drain(..list.len() - cap);
    }
}

pub fn percentile(values: &[f64], p: f64) -> Option<f64> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let i = ((p / 100.0) * v.len() as f64).ceil() as i64 - 1;
    Some(v[i.clamp(0, v.len() as i64 - 1) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::NoticeAction;
    use serde_json::json;

    const NOW: i64 = 1_800_000_000_000;

    fn n(id: &str, kind: NoticeKind, age: i64) -> Notice {
        Notice {
            id: id.into(),
            kind,
            addr: Some("0x1".into()),
            title: format!("Notice {id}"),
            body: None,
            image_url: None,
            amount: None,
            item: None,
            action: NoticeAction::Tx { digest: "abc".into() },
            digest: Some("abc".into()),
            cp_ts: Some(NOW - age - 300),
            created_at: NOW - age,
            sent_at: NOW - age,
        }
    }
    fn sold(id: &str) -> Notice {
        n(id, NoticeKind::Sold, 0)
    }
    fn ids(v: &[Notice]) -> Vec<&str> {
        v.iter().map(|x| x.id.as_str()).collect()
    }
    /// Decodes a form query the way URLSearchParams.get does.
    fn param(q: &str, key: &str) -> Option<String> {
        q.split('&').find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            (k == key).then(|| {
                let b = v.replace('+', " ");
                let mut out = Vec::new();
                let bytes = b.as_bytes();
                let mut i = 0;
                while i < bytes.len() {
                    if bytes[i] == b'%' {
                        out.push(u8::from_str_radix(&b[i + 1..i + 3], 16).unwrap());
                        i += 3;
                    } else {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
                String::from_utf8(out).unwrap()
            })
        })
    }
    fn ctx(prefs: &Prefs) -> RouteCtx<'_> {
        RouteCtx { prefs, live: true, now: NOW, playing: false, focused: true }
    }
    fn out(r: Routing) -> (Route, bool, bool, bool) {
        (r.route, r.desktop, r.hud, r.small)
    }

    // ids

    #[test]
    fn compares_decimal_strings_past_2_53() {
        assert_eq!(cmp_id("9", "10"), Ordering::Less);
        assert_eq!(cmp_id("9007199254740993", "9007199254740992"), Ordering::Greater);
        assert_eq!(cmp_id("0012", "12"), Ordering::Equal);
    }

    #[test]
    fn keeps_the_larger_id_for_resume_and_ignores_junk() {
        assert_eq!(max_id(None, Some("5")).as_deref(), Some("5"));
        assert_eq!(max_id(Some("12"), Some("9")).as_deref(), Some("12"));
        assert_eq!(max_id(Some("12"), Some("abc")).as_deref(), Some("12"));
        assert_eq!(max_id(None, None), None);
    }

    // dedupe

    #[test]
    fn merges_newest_first_with_one_copy_per_id() {
        let a = merge_notices(&[sold("3"), sold("1")], &[sold("2"), sold("3"), sold("4")], MERGE_CAP);
        assert_eq!(ids(&a), ["4", "3", "2", "1"]);
    }

    #[test]
    fn keeps_the_first_copy_of_an_id() {
        let first = Notice { title: "first".into(), ..sold("7") };
        let second = Notice { title: "second".into(), ..sold("7") };
        assert_eq!(merge_notices(&[first], &[second], MERGE_CAP)[0].title, "first");
    }

    #[test]
    fn caps_the_list() {
        let many: Vec<Notice> = (1..=30).map(|i| sold(&i.to_string())).collect();
        assert_eq!(merge_notices(&[], &many, 10)[0].id, "30");
        assert_eq!(merge_notices(&[], &many, 10).len(), 10);
    }

    #[test]
    fn orders_server_and_local_notices_by_time_not_id() {
        // A local id is a long decimal; it must not jump ahead of a newer server notice.
        let local = n("1793000000000001", NoticeKind::FollowedTrade, 5000);
        let m = merge_notices(&[local], &[n("42", NoticeKind::Sold, 0), n("41", NoticeKind::Sold, 9000)], MERGE_CAP);
        assert_eq!(ids(&m), ["42", "1793000000000001", "41"]);
    }

    #[test]
    fn reports_only_ids_it_has_not_seen() {
        assert_eq!(new_ids(&[sold("1"), sold("2")], &[sold("2"), sold("3")]), ["3"]);
    }

    // stream query

    #[test]
    fn dedupes_and_lowercases_addresses_adds_device_and_after() {
        let addrs: Vec<String> = ["0xAB", "0xab", "nope", "0x01"].iter().map(|s| s.to_string()).collect();
        let q = stream_query(&addrs, "dev_1234567890abcdef", Some("42"));
        assert_eq!(param(&q, "addr").as_deref(), Some("0x01,0xab"));
        assert_eq!(param(&q, "device").as_deref(), Some("dev_1234567890abcdef"));
        assert_eq!(param(&q, "after").as_deref(), Some("42"));
    }

    #[test]
    fn leaves_out_after_when_there_is_no_id_yet() {
        assert_eq!(param(&stream_query(&["0x1".to_string()], "d", None), "after"), None);
    }

    // kind toggles

    fn offers_off() -> Prefs {
        let mut p = Prefs::default();
        p.set_kind(&NoticeKind::OfferReceived, false);
        p
    }

    #[test]
    fn kind_toggles_drop_a_switched_off_kind() {
        let prefs = offers_off();
        assert_eq!(route(&n("1", NoticeKind::OfferReceived, 0), &ctx(&prefs)).route, Route::Drop);
        assert_eq!(route(&sold("1"), &ctx(&prefs)).route, Route::Toast);
    }

    #[test]
    fn hides_switched_off_kinds_and_cleared_notices_from_the_centre() {
        let prefs = offers_off();
        let list = [n("5", NoticeKind::Sold, 0), n("4", NoticeKind::OfferReceived, 1000), n("3", NoticeKind::Bought, 2000), n("2", NoticeKind::Sold, 3000)];
        let v: Vec<&str> = visible(&list, &prefs, Some(NOW - 3000)).iter().map(|x| x.id.as_str()).collect();
        assert_eq!(v, ["5", "3"]);
    }

    #[test]
    fn counts_unread_from_the_read_set_and_the_read_mark() {
        let list = [n("5", NoticeKind::Sold, 0), n("4", NoticeKind::Sold, 1000), n("3", NoticeKind::Sold, 2000), n("2", NoticeKind::Sold, 3000)];
        let read: HashSet<String> = ["5".to_string()].into();
        assert_eq!(unread_count(&list, &Prefs::default(), None, &read, Some(NOW - 2000)), 1);
    }

    #[test]
    fn reads_stored_prefs_with_defaults_for_missing_or_bad_values() {
        let p = read_prefs(&json!({ "kinds": { "sold": false, "bought": "yes" }, "desktop": false }));
        assert!(!p.kind_on(&NoticeKind::Sold));
        assert!(p.kind_on(&NoticeKind::Bought));
        assert!(!p.desktop);
        assert!(p.game_toasts);
        assert_eq!(read_prefs(&Value::Null), Prefs::default());
    }

    // routing and the play queue

    #[test]
    fn toasts_a_fresh_live_notice_desktop_only_when_unfocused() {
        let d = Prefs::default();
        assert_eq!(out(route(&sold("1"), &ctx(&d))), (Route::Toast, false, false, false));
        assert!(route(&sold("1"), &RouteCtx { focused: false, ..ctx(&d) }).desktop);
        let off = Prefs { desktop: false, ..Prefs::default() };
        assert!(!route(&sold("1"), &RouteCtx { focused: false, ..ctx(&off) }).desktop);
    }

    #[test]
    fn sends_old_and_replayed_notices_to_the_centre_only() {
        let d = Prefs::default();
        assert!(!is_fresh(&n("1", NoticeKind::Sold, 10 * 60_000), NOW));
        assert_eq!(route(&n("1", NoticeKind::Sold, 10 * 60_000), &ctx(&d)).route, Route::Centre);
        assert_eq!(route(&sold("1"), &RouteCtx { live: false, ..ctx(&d) }).route, Route::Centre);
    }

    #[test]
    fn routing_drops_a_switched_off_kind() {
        let mut p = Prefs::default();
        p.set_kind(&NoticeKind::Sold, false);
        assert_eq!(route(&sold("1"), &ctx(&p)).route, Route::Drop);
    }

    #[test]
    fn puts_every_kind_in_a_class() {
        assert_eq!(class_of_str("sold"), NoticeClass::Money);
        assert_eq!(class_of_str("offer_received"), NoticeClass::Money);
        assert_eq!(class_of_str("party_invite"), NoticeClass::Invites);
        assert_eq!(class_of_str("friend_online"), NoticeClass::Social);
        assert_eq!(class_of_str("os_update"), NoticeClass::System);
        assert_eq!(class_of_str("download_done"), NoticeClass::System);
        assert_eq!(class_of_str("emu_update"), NoticeClass::System);
        assert_eq!(class_of_str("trophy"), NoticeClass::Trophies);
        assert_eq!(class_of_str("battery"), NoticeClass::Battery);
        assert_eq!(class_of_str("something_new"), NoticeClass::Social);
    }

    #[test]
    fn follows_the_models_table_over_a_game() {
        let d = Prefs::default();
        let g = RouteCtx { playing: true, focused: false, ..ctx(&d) };
        assert_eq!(out(route(&sold("1"), &g)), (Route::Toast, false, true, false));
        assert_eq!(route(&n("1", NoticeKind::PartyInvite, 0), &g).route, Route::Toast);
        assert_eq!(out(route(&n("1", NoticeKind::Trophy, 0), &g)), (Route::Toast, false, true, true));
        assert_eq!(route(&n("1", NoticeKind::FriendOnline, 0), &g).route, Route::Queue);
        assert_eq!(route(&n("1", NoticeKind::DownloadDone, 0), &g).route, Route::Queue);
        assert_eq!(route(&n("1", NoticeKind::Battery, 0), &g).route, Route::Toast);
    }

    #[test]
    fn queues_over_a_game_when_toasts_over_a_game_are_off_except_battery() {
        let p = Prefs { game_toasts: false, ..Prefs::default() };
        let g = RouteCtx { playing: true, ..ctx(&p) };
        assert_eq!(route(&sold("1"), &g).route, Route::Queue);
        assert_eq!(route(&n("1", NoticeKind::Battery, 0), &g).route, Route::Toast);
    }

    #[test]
    fn do_not_disturb_keeps_everything_in_the_list_but_battery() {
        let p = Prefs { dnd: true, ..Prefs::default() };
        let d = ctx(&p);
        use NoticeKind::*;
        for k in [Sold, PartyInvite, Trophy, FriendRequest, OsUpdate, DownloadDone] {
            assert_eq!(route(&n("1", k, 0), &d).route, Route::Centre);
        }
        assert_eq!(route(&n("1", Battery, 0), &d).route, Route::Toast);
        assert_eq!(route(&n("1", Battery, 0), &RouteCtx { playing: true, ..d }).route, Route::Toast);
        assert_eq!(delivery_for(NoticeClass::System, true, false, true), NoticeClass::System.rule().dnd);
    }

    #[test]
    fn summarises_what_arrived_while_asleep() {
        let mut w = WakeSummary::default();
        assert_eq!(w.flush(), None);
        w.add("1");
        w.add("1");
        w.add("2");
        assert_eq!(w.flush().as_deref(), Some("2 new while you were away"));
        assert_eq!(w.count(), 0);
    }

    #[test]
    fn counts_queued_notices_once_each_and_summarises_on_exit() {
        let mut q = PlayQueue::default();
        assert_eq!(q.flush(), None);
        q.add("1");
        q.add("1");
        assert_eq!(q.count(), 1);
        assert_eq!(q.flush().as_deref(), Some("1 new notification while you played"));
        q.add("2");
        q.add("3");
        assert_eq!(q.flush().as_deref(), Some("2 new notifications while you played"));
        assert_eq!(q.count(), 0);
    }

    // latency samples

    #[test]
    fn keeps_the_last_50() {
        let mut l: Vec<i32> = Vec::new();
        for i in 0..60 {
            push_sample(&mut l, i, 50);
        }
        assert_eq!(l.len(), 50);
        assert_eq!(l[0], 10);
    }

    #[test]
    fn finds_percentiles() {
        assert_eq!(percentile(&[300.0, 100.0, 200.0, 400.0], 50.0), Some(200.0));
        assert_eq!(percentile(&[300.0, 100.0, 200.0, 400.0], 95.0), Some(400.0));
        assert_eq!(percentile(&[], 50.0), None);
    }

    #[test]
    fn encodes_like_the_browser() {
        assert_eq!(form_query(&[("addr", "0x1,0x2".into()), ("q", "a b".into())]), "addr=0x1%2C0x2&q=a+b");
        assert_eq!(uri_component("a+b/c=d ~"), "a%2Bb%2Fc%3Dd%20~");
    }
}
