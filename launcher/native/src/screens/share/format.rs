//! share/format.ts (the share format rare.market reads), the UI half of share/upload.ts (the quilt's
//! files and their public URLs), share/sponsor.ts's types, share/post.ts's hook, and the figures
//! Share borrows from sui/ui/CloudSaves.tsx (`fmtWal`, `fmtSuiMist`, `fmtBytes`, `ago`). Pure
//! functions, unit tested like format.test.ts.

use rare_share::{Capture, CaptureGame, Kind};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

pub const SHARE_FORMAT: &str = "rare.share/1";
pub const SHARE_APP_TAG: &str = "rare-share";
pub const SHARE_BASE: &str = "https://rare.market/s/";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    #[default]
    Mainnet,
    Testnet,
}

impl Network {
    /// Walrus epoch length in days (sui/saves/network.ts EPOCH_DAYS).
    pub fn epoch_days(self) -> u32 {
        match self {
            Network::Mainnet => 14,
            Network::Testnet => 1,
        }
    }
}

/// The furthest ahead storage can be bought, in epochs.
pub const MAX_EPOCHS: u32 = 53;

/// sui/saves/network.ts epochsFor.
pub fn epochs_for(days: u32, network: Network) -> u32 {
    days.div_ceil(network.epoch_days()).clamp(1, MAX_EPOCHS)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShareGame {
    pub name: String,
    pub slug: Option<String>,
}

/// share.json. Field order is JSON.stringify's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareJson {
    pub format: String,
    pub kind: Kind,
    pub file: String,
    pub mime: String,
    pub width: u32,
    pub height: u32,
    pub duration_ms: Option<u64>,
    /// size of the media file
    pub bytes: u64,
    pub poster: Option<String>,
    pub game: Option<ShareGame>,
    /// unix ms
    pub created: u64,
}

pub fn media_file(kind: Kind) -> &'static str {
    match kind {
        Kind::Clip => "clip.mp4",
        Kind::Screenshot => "shot.webp",
    }
}

pub fn media_mime(kind: Kind) -> &'static str {
    match kind {
        Kind::Clip => "video/mp4",
        Kind::Screenshot => "image/webp",
    }
}

pub fn kind_word(kind: Kind) -> &'static str {
    match kind {
        Kind::Clip => "clip",
        Kind::Screenshot => "screenshot",
    }
}

/// share.json for a capture. Never carries the player's address: the site reads the uploader from chain.
pub fn build_share_json(c: &Capture, bytes: u64, slug: Option<&str>, poster: bool) -> ShareJson {
    ShareJson {
        format: SHARE_FORMAT.into(),
        kind: c.kind,
        file: media_file(c.kind).into(),
        mime: media_mime(c.kind).into(),
        width: c.width,
        height: c.height,
        duration_ms: if c.kind == Kind::Clip { c.duration_ms } else { None },
        bytes,
        poster: (c.kind == Kind::Clip && poster).then(|| "poster.jpg".to_string()),
        game: c.game.as_ref().map(|g| ShareGame { name: g.name.clone(), slug: slug.map(str::to_string) }),
        created: c.created_ms,
    }
}

/// The public link for a share.
pub fn share_link(quilt_id: &str, network: Network) -> String {
    format!("{SHARE_BASE}{quilt_id}{}", if network == Network::Testnet { "?n=testnet" } else { "" })
}

/// Blob attributes, so a later "My shares" list can find them on chain.
pub fn share_attributes(kind: Kind) -> Vec<(&'static str, &'static str)> {
    vec![("app", SHARE_APP_TAG), ("v", "1"), ("kind", kind_word(kind))]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DurationId {
    #[default]
    Month,
    Year,
}

pub struct Duration {
    pub id: DurationId,
    pub label: &'static str,
    pub days: u32,
}

pub const DURATIONS: [Duration; 2] = [Duration { id: DurationId::Month, label: "1 month", days: 30 }, Duration { id: DurationId::Year, label: "1 year", days: 365 }];

pub fn duration(id: DurationId) -> &'static Duration {
    DURATIONS.iter().find(|d| d.id == id).unwrap_or(&DURATIONS[0])
}

/// Walrus epochs and the days they really buy (mainnet epochs are 14 days, so 1 month is 42 days).
pub fn share_epochs(id: DurationId, network: Network) -> (u32, u32) {
    let epochs = epochs_for(duration(id).days, network);
    (epochs, epochs * network.epoch_days())
}

/// Quilt size when the SDK's encoder isn't at hand: the files plus the quilt's column padding and
/// index. Errs high so a price is never low.
pub fn estimate_quilt_bytes(sizes: &[u64]) -> u64 {
    let sum: u64 = sizes.iter().sum();
    (sum as f64 * 1.04).ceil() as u64 + 262_144
}

/// Long edge capped at `max`, aspect kept, whole pixels.
pub fn fit_within(w: u32, h: u32, max: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (max, js_round(max as f64 * 9.0 / 16.0) as u32);
    }
    let s = (max as f64 / w.max(h) as f64).min(1.0);
    ((js_round(w as f64 * s) as u32).max(1), (js_round(h as f64 * s) as u32).max(1))
}

/// Where to grab a clip's poster: about 1 s in, or 40 percent of a shorter clip.
pub fn poster_time(duration_s: f64) -> f64 {
    if !duration_s.is_finite() || duration_s <= 0.0 {
        return 0.0;
    }
    if duration_s >= 2.5 {
        1.0
    } else {
        duration_s * 0.4
    }
}

/// "0:30", "1:05"
pub fn fmt_duration(ms: Option<u64>) -> String {
    let Some(ms) = ms.filter(|m| *m > 0) else { return String::new() };
    let s = js_round(ms as f64 / 1000.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Math.round (halves go up).
pub fn js_round(v: f64) -> f64 {
    (v + 0.5).floor()
}

// ---------------------------------------------------------------- rare.market game slug

/// A rare.market catalog game (lib/rare SuiGame), the fields slugFor reads.
#[derive(Clone, Debug, PartialEq)]
pub struct SuiGame {
    pub slug: String,
    pub name: String,
}

/// A library game (lib/types Game), the fields slugFor reads.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct LibGame {
    pub id: String,
    pub title: String,
    pub source: String,
    pub installed: bool,
    pub url: Option<String>,
}

#[derive(Deserialize, Default)]
struct Profiles {
    games: std::collections::HashMap<String, Profile>,
}

#[derive(Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
struct Profile {
    #[serde(default)]
    hosts: Vec<String>,
    native: Option<NativeInfo>,
}

#[derive(Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
struct NativeInfo {
    #[serde(default)]
    steam_app_ids: Vec<u64>,
    #[serde(default)]
    titles: Vec<String>,
}

/// games/profiles.json, the launcher's per-game profiles (games/catalog.ts).
fn profiles() -> &'static Profiles {
    static P: std::sync::OnceLock<Profiles> = std::sync::OnceLock::new();
    P.get_or_init(|| serde_json::from_str(include_str!("../../../../src/games/profiles.json")).unwrap_or_default())
}

fn profile(slug: &str) -> Option<(&'static str, &'static Profile)> {
    let k = slug.strip_prefix("web:").unwrap_or(slug);
    profiles().games.get_key_value(k).map(|(k, v)| (k.as_str(), v))
}

/// The host of a URL without "www.", lower case.
fn url_host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let auth = rest.split(['/', '?', '#']).next()?;
    let host = auth.rsplit('@').next()?.split(':').next()?.to_ascii_lowercase();
    (!host.is_empty()).then(|| host.trim_start_matches("www.").to_string())
}

/// catalog.ts profileForUrl: the profile's slug for a web game, by id then by its URL's host.
fn profile_for_url(id: &str, url: Option<&str>) -> Option<&'static str> {
    if let Some((k, _)) = profile(id) {
        return Some(k);
    }
    let host = url_host(url?)?;
    profiles().games.iter().find(|(_, p)| p.hosts.iter().any(|h| h.trim_start_matches("www.") == host)).map(|(k, _)| k.as_str())
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).collect()
}

/// catalog.ts installedNative.
fn installed_native<'a>(slug: &str, name: &str, library: &'a [LibGame]) -> Option<&'a LibGame> {
    let n = profile(slug).and_then(|(_, p)| p.native.as_ref());
    let games: Vec<&LibGame> = library.iter().filter(|g| g.installed && g.source != "web").collect();
    let ids: Vec<String> = n.map(|n| n.steam_app_ids.iter().map(|i| format!("steam:{i}")).collect()).unwrap_or_default();
    if let Some(g) = games.iter().find(|g| ids.contains(&g.id)) {
        return Some(g);
    }
    let mut titles = vec![norm(name)];
    titles.extend(n.map(|n| n.titles.iter().map(|t| norm(t)).collect::<Vec<_>>()).unwrap_or_default());
    games.into_iter().find(|g| titles.contains(&norm(&g.title)))
}

/// rare.market's slug for the game in a capture, where the launcher knows it: built-in and added
/// web games by id or URL host, native Sui titles by Steam app id or title. Steam and other games
/// that aren't Sui games get None and send just their name.
pub fn slug_for(game: Option<&CaptureGame>, catalog: &[SuiGame], library: Option<&[LibGame]>) -> Option<String> {
    let game = game?;
    let lib = library.unwrap_or(&[]);
    if let Some(id) = game.id.strip_prefix("web:") {
        if catalog.iter().any(|g| g.slug == id) {
            return Some(id.to_string());
        }
        let url = lib.iter().find(|g| g.id == game.id).and_then(|g| g.url.as_deref());
        if let Some(p) = profile_for_url(id, url) {
            if let Some(hit) = catalog.iter().find(|g| profile_for_url(&g.slug, None) == Some(p)) {
                return Some(hit.slug.clone());
            }
        }
    }
    for g in catalog {
        if installed_native(&g.slug, &g.name, lib).is_some_and(|i| i.id == game.id) {
            return Some(g.slug.clone());
        }
    }
    catalog.iter().find(|g| norm(&g.name) == norm(&game.name)).map(|g| g.slug.clone())
}

// ---------------------------------------------------------------- upload.ts (the UI half)

/// A file in the share's quilt (sponsor.ts SponsorFile).
#[derive(Clone, Debug, PartialEq)]
pub struct QuiltFile {
    pub identifier: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// The quilt's files, identifiers exactly as rare.market reads them.
pub fn share_files(json: &ShareJson, media: Vec<u8>, poster: Option<Vec<u8>>) -> Vec<QuiltFile> {
    let mut files = vec![
        QuiltFile { identifier: "share.json".into(), mime: "application/json".into(), bytes: serde_json::to_vec(json).unwrap_or_default() },
        QuiltFile { identifier: media_file(json.kind).into(), mime: media_mime(json.kind).into(), bytes: media },
    ];
    if let (Some(_), Some(p)) = (&json.poster, poster) {
        files.push(QuiltFile { identifier: "poster.jpg".into(), mime: "image/jpeg".into(), bytes: p });
    }
    files
}

pub fn aggregator(network: Network) -> &'static str {
    match network {
        Network::Mainnet => "https://aggregator.walrus-mainnet.walrus.space",
        Network::Testnet => "https://aggregator.walrus-testnet.walrus.space",
    }
}

fn uri_component(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

/// A file of a share, from the public aggregator.
pub fn quilt_file_url(network: Network, quilt_id: &str, identifier: &str) -> String {
    format!("{}/v1/blobs/by-quilt-id/{}/{}", aggregator(network), uri_component(quilt_id), uri_component(identifier))
}

/// What a Walrus price came to (sui/saves/walrus.ts CostEstimate, the parts Share shows).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct CostEstimate {
    pub wal_frost: u128,
    pub tip_mist: u128,
    pub gas_mist: u128,
}

/// sponsor.ts SponsorQuote: whether "Rare covers it" is offered for this share.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct SponsorQuote {
    /// false: the sponsor won't take this one (too big, daily limit); the player can still pay
    pub covered: bool,
    /// what it costs the sponsor, for display only
    pub wal_frost: Option<u128>,
    pub note: Option<String>,
}

/// The sponsor's `GET <base>/quote` answer, as sponsor.ts reads it (a bad answer is "not covered").
pub fn parse_sponsor_quote(status_ok: bool, body: &str) -> SponsorQuote {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Q {
        covered: Option<bool>,
        wal_frost: Option<String>,
        note: Option<String>,
    }
    if !status_ok {
        return SponsorQuote::default();
    }
    match serde_json::from_str::<Q>(body) {
        Ok(q) => SponsorQuote { covered: q.covered.unwrap_or(false), wal_frost: q.wal_frost.and_then(|w| w.parse().ok()), note: q.note },
        Err(_) => SponsorQuote::default(),
    }
}

/// post.ts: the Apps integration sets a Telegram poster when it lands; Share shows "Post to
/// Telegram" only while one is set.
pub type Poster = fn(&str) -> Result<(), String>;
static TELEGRAM: Mutex<Option<Poster>> = Mutex::new(None);

pub fn set_telegram_poster(p: Option<Poster>) {
    *TELEGRAM.lock().unwrap() = p;
}

pub fn telegram_poster() -> Option<Poster> {
    *TELEGRAM.lock().unwrap()
}

// ---------------------------------------------------------------- CloudSaves figures

/// Number.prototype.toPrecision then back to a number.
pub fn to_precision(v: f64, p: usize) -> f64 {
    if v == 0.0 || !v.is_finite() {
        return v;
    }
    format!("{:.*e}", p.saturating_sub(1), v).parse().unwrap_or(v)
}

/// toLocaleString("en-US", { maximumFractionDigits: max }): grouped, half away from zero, no
/// trailing zeros.
pub fn locale_num(v: f64, max: usize) -> String {
    let neg = v < 0.0;
    let k = 10f64.powi(max as i32);
    let r = (v.abs() * k + 0.5).floor() / k;
    let s = format!("{r:.max$}");
    let (int, frac) = s.split_once('.').unwrap_or((&s, ""));
    let frac = frac.trim_end_matches('0');
    let mut g = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            g.push(',');
        }
        g.push(ch);
    }
    format!("{}{g}{}{frac}", if neg && r != 0.0 { "-" } else { "" }, if frac.is_empty() { "" } else { "." })
}

/// "0.0193 WAL", "<0.0001 WAL", "" for nothing.
pub fn fmt_wal(frost: Option<u128>) -> String {
    let Some(f) = frost else { return String::new() };
    let n = f as f64 / 1e9;
    if n == 0.0 {
        return "0 WAL".into();
    }
    if n < 0.0001 {
        return "<0.0001 WAL".into();
    }
    format!("{} WAL", locale_num(to_precision(n, 3), 4))
}

/// "0.015 SUI"
pub fn fmt_sui_mist(mist: u128) -> String {
    format!("{} SUI", locale_num(to_precision(mist as f64 / 1e9, 2), 4))
}

/// "446 KB", "14.8 MB", "1.2 GB" (decimal units, never below 1 KB).
pub fn fmt_bytes(b: u64) -> String {
    let b = b as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else {
        format!("{} KB", js_round(b / 1e3).max(1.0))
    }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// (month 1..12, day) of a unix day number (UTC).
fn civil(days: i64) -> (u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (m, d)
}

/// "just now", "3 h ago", "yesterday", "12 days ago", then "Oct 3" (UTC).
pub fn ago(t_ms: u64, now_ms: u64) -> String {
    if t_ms == 0 {
        return String::new();
    }
    let s = (now_ms as f64 - t_ms as f64) / 1000.0;
    if s < 3600.0 {
        "just now".into()
    } else if s < 86_400.0 {
        format!("{} h ago", js_round(s / 3600.0))
    } else if s < 86_400.0 * 2.0 {
        "yesterday".into()
    } else if s < 86_400.0 * 30.0 {
        format!("{} days ago", js_round(s / 86_400.0))
    } else {
        let (m, d) = civil((t_ms / 86_400_000) as i64);
        format!("{} {d}", MONTHS[m as usize - 1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot() -> Capture {
        Capture {
            id: "shot-1".into(),
            kind: Kind::Screenshot,
            path: "/home/rare/Pictures/Rare/shot-1.webp".into(),
            mime: "image/webp".into(),
            bytes: 810_598,
            width: 1920,
            height: 1080,
            duration_ms: None,
            created_ms: 1_761_500_000_000,
            game: Some(CaptureGame { id: "web:act".into(), name: "ACT".into() }),
        }
    }
    fn clip() -> Capture {
        Capture { id: "clip-1".into(), kind: Kind::Clip, mime: "video/mp4".into(), bytes: 15_059_947, duration_ms: Some(30_000), game: Some(CaptureGame { id: "steam:1145350".into(), name: "Hades II".into() }), ..shot() }
    }

    #[test]
    fn share_json_describes_a_screenshot() {
        let j = build_share_json(&shot(), 810_598, Some("act"), false);
        let v = serde_json::to_value(&j).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "format": "rare.share/1", "kind": "screenshot", "file": "shot.webp", "mime": "image/webp", "width": 1920, "height": 1080,
                "durationMs": null, "bytes": 810_598, "poster": null, "game": { "name": "ACT", "slug": "act" }, "created": 1_761_500_000_000u64
            })
        );
        // JSON.stringify's key order.
        let s = serde_json::to_string(&j).unwrap();
        assert!(s.starts_with(r#"{"format":"rare.share/1","kind":"screenshot","file":"shot.webp","mime":"image/webp","width":1920"#), "{s}");
    }

    #[test]
    fn share_json_describes_a_clip_with_its_poster_and_a_steam_game_by_name_only() {
        let j = build_share_json(&clip(), 15_059_947, None, true);
        assert_eq!((j.kind, j.file.as_str(), j.mime.as_str(), j.duration_ms, j.poster.as_deref()), (Kind::Clip, "clip.mp4", "video/mp4", Some(30_000), Some("poster.jpg")));
        assert_eq!(j.game, Some(ShareGame { name: "Hades II".into(), slug: None }));
    }

    #[test]
    fn share_json_never_carries_an_address_and_has_no_game_when_none_was_playing() {
        let j = build_share_json(&Capture { game: None, ..shot() }, 1, None, false);
        assert_eq!(j.game, None);
        let s = serde_json::to_string(&j).unwrap();
        assert!(!s.contains("0x"));
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        let mut keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, ["bytes", "created", "durationMs", "file", "format", "game", "height", "kind", "mime", "poster", "width"]);
    }

    #[test]
    fn a_screenshot_never_names_a_poster() {
        assert_eq!(build_share_json(&shot(), 1, None, true).poster, None);
    }

    #[test]
    fn quilt_files_use_the_exact_identifiers_poster_only_for_clips() {
        let s = share_files(&build_share_json(&shot(), 10, None, false), vec![0; 10], None);
        assert_eq!(s.iter().map(|f| (f.identifier.as_str(), f.mime.as_str())).collect::<Vec<_>>(), [("share.json", "application/json"), ("shot.webp", "image/webp")]);
        let c = share_files(&build_share_json(&clip(), 10, None, true), vec![0; 10], Some(vec![0; 3]));
        assert_eq!(c.iter().map(|f| f.identifier.as_str()).collect::<Vec<_>>(), ["share.json", "clip.mp4", "poster.jpg"]);
        let j: serde_json::Value = serde_json::from_slice(&c[0].bytes).unwrap();
        assert_eq!(j["format"], "rare.share/1");
    }

    #[test]
    fn tags_the_blob_for_a_later_list_of_shares() {
        assert_eq!(share_attributes(Kind::Clip), [("app", "rare-share"), ("v", "1"), ("kind", "clip")]);
    }

    #[test]
    fn links_on_mainnet_and_testnet() {
        assert_eq!(share_link("abc_DEF-123", Network::Mainnet), "https://rare.market/s/abc_DEF-123");
        assert_eq!(share_link("abc_DEF-123", Network::Testnet), "https://rare.market/s/abc_DEF-123?n=testnet");
        assert_eq!(quilt_file_url(Network::Mainnet, "q/1", "share.json"), "https://aggregator.walrus-mainnet.walrus.space/v1/blobs/by-quilt-id/q%2F1/share.json");
    }

    #[test]
    fn durations_buy_whole_epochs() {
        assert_eq!(share_epochs(DurationId::Month, Network::Mainnet), (3, 42));
        assert_eq!(share_epochs(DurationId::Year, Network::Mainnet), (27, 378));
        assert_eq!(share_epochs(DurationId::Month, Network::Testnet), (30, 30));
        assert_eq!(share_epochs(DurationId::Year, Network::Testnet), (53, 53));
    }

    #[test]
    fn estimated_quilt_size_errs_high() {
        // format.test.ts checks it against the SDK's encoder; here, the formula itself.
        assert_eq!(estimate_quilt_bytes(&[300, 200_000]), 208_312 + 262_144);
        for sizes in [vec![300, 810_598], vec![300, 15_059_947, 42_309], vec![300, 60_000_000, 150_000]] {
            let sum: u64 = sizes.iter().sum();
            let est = estimate_quilt_bytes(&sizes);
            assert!(est >= sum + 262_144 && est < (sum as f64 * 1.25) as u64 + 300_000);
        }
    }

    #[test]
    fn poster_fits_the_long_edge_and_is_grabbed_about_a_second_in() {
        assert_eq!(fit_within(1920, 1080, 1280), (1280, 720));
        assert_eq!(fit_within(1080, 1920, 1280), (720, 1280));
        assert_eq!(fit_within(800, 600, 1280), (800, 600));
        assert_eq!(fit_within(0, 0, 1280), (1280, 720));
        assert_eq!(poster_time(30.0), 1.0);
        assert!((poster_time(2.0) - 0.8).abs() < 1e-9);
        assert_eq!(poster_time(f64::NAN), 0.0);
    }

    #[test]
    fn formats_durations() {
        assert_eq!(fmt_duration(Some(30_000)), "0:30");
        assert_eq!(fmt_duration(Some(65_400)), "1:05");
        assert_eq!(fmt_duration(None), "");
    }

    fn catalog() -> Vec<SuiGame> {
        [("act", "ACT"), ("wordgame", "Word Game"), ("xociety", "XOCIETY"), ("cosmocadia", "Cosmocadia")].iter().map(|(s, n)| SuiGame { slug: s.to_string(), name: n.to_string() }).collect()
    }
    fn lib(id: &str, source: &str, title: &str, url: Option<&str>) -> LibGame {
        LibGame { id: id.into(), title: title.into(), source: source.into(), installed: true, url: url.map(str::to_string) }
    }
    fn game(id: &str, name: &str) -> CaptureGame {
        CaptureGame { id: id.into(), name: name.into() }
    }

    #[test]
    fn slug_for_built_in_web_games_by_id() {
        assert_eq!(slug_for(Some(&game("web:act", "ACT")), &catalog(), None).as_deref(), Some("act"));
    }

    #[test]
    fn slug_for_an_added_web_game_by_its_url_host() {
        let l = [lib("web:my-cosmo", "web", "My Cosmo", Some("https://www.cosmocadia.io/play"))];
        assert_eq!(slug_for(Some(&game("web:my-cosmo", "My Cosmo")), &catalog(), Some(&l)).as_deref(), Some("cosmocadia"));
    }

    #[test]
    fn slug_for_a_native_sui_game_by_its_library_title() {
        let l = [lib("epic:xo", "epic", "XOCIETY", None)];
        assert_eq!(slug_for(Some(&game("epic:xo", "XOCIETY")), &catalog(), Some(&l)).as_deref(), Some("xociety"));
    }

    #[test]
    fn steam_and_other_games_send_none() {
        let l = [lib("steam:1145350", "steam", "Hades II", None)];
        assert_eq!(slug_for(Some(&game("steam:1145350", "Hades II")), &catalog(), Some(&l)), None);
        assert_eq!(slug_for(None, &catalog(), None), None);
    }

    #[test]
    fn cloud_saves_figures() {
        assert_eq!(fmt_wal(Some(19_345_000)), "0.0193 WAL");
        assert_eq!(fmt_wal(Some(0)), "0 WAL");
        assert_eq!(fmt_wal(Some(50)), "<0.0001 WAL");
        assert_eq!(fmt_wal(Some(1_234_500_000_000)), "1,230 WAL");
        assert_eq!(fmt_wal(None), "");
        assert_eq!(fmt_sui_mist(15_000_000), "0.015 SUI");
        assert_eq!(fmt_sui_mist(2_345_000_000), "2.3 SUI");
        assert_eq!(fmt_bytes(446_000), "446 KB");
        assert_eq!(fmt_bytes(14_800_000), "14.8 MB");
        assert_eq!(fmt_bytes(12), "1 KB");
        assert_eq!(fmt_bytes(2_500_000_000), "2.5 GB");
        let now = 1_793_126_478_575;
        let h = 3_600_000;
        assert_eq!(ago(now - h / 5, now), "just now");
        assert_eq!(ago(now - 3 * h, now), "3 h ago");
        assert_eq!(ago(now - 26 * h, now), "yesterday");
        assert_eq!(ago(now - 75 * h, now), "3 days ago");
        assert_eq!(ago(now - 40 * 24 * h, now), "Sep 17");
        assert_eq!(ago(0, now), "");
    }

    #[test]
    fn sponsor_quotes_read_like_sponsor_ts() {
        assert_eq!(parse_sponsor_quote(true, r#"{"covered":true,"walFrost":"12345","note":"Rare pays"}"#), SponsorQuote { covered: true, wal_frost: Some(12_345), note: Some("Rare pays".into()) });
        assert_eq!(parse_sponsor_quote(false, "{}"), SponsorQuote::default());
        assert_eq!(parse_sponsor_quote(true, "nope"), SponsorQuote::default());
    }
}
