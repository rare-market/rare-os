//! The NFT image resolver and the marketplace's image chains. Ported from
//! launcher/src/marketplace/venues-media.ts (copied verbatim from rare.market
//! packages/venues/src/media.ts: which field holds an item's image, how any reference (ipfs,
//! arweave, walrus, gateway URLs, data: URIs, sloppy strings) becomes the URLs to fetch, which hosts
//! are dead) and marketplace/media.ts (the order of sources to try for one picture).
//!
//! In the native launcher each link of a chain goes to `rare-images` as a URL source (fetched once,
//! kept on disk and in memory at the drawn size); the web launcher wrapped them in rimg:// links.

use crate::api::enc;
use regex::Regex;
use serde_json::{Map, Value};
use std::sync::LazyLock;
use url::Url;

// ---------- JS URI functions ----------

fn hex_val(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

/// `decodeURI` (`keep_reserved`) or `decodeURIComponent`; None where JS throws.
fn decode(s: &str, keep_reserved: bool) -> Option<String> {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'%' {
            let c = s[i..].chars().next()?;
            out.push(c);
            i += c.len_utf8();
            continue;
        }
        let start = i;
        let byte = |j: usize| -> Option<u8> {
            if b.get(j) != Some(&b'%') {
                return None;
            }
            Some(hex_val(*b.get(j + 1)?)? << 4 | hex_val(*b.get(j + 2)?)?)
        };
        let first = byte(i)?;
        let n = match first {
            0x00..=0x7f => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => return None,
        };
        let mut bytes = vec![first];
        for k in 1..n {
            bytes.push(byte(i + 3 * k)?);
        }
        let ch = std::str::from_utf8(&bytes).ok()?;
        i += 3 * n;
        if keep_reserved && n == 1 && ";/?:@&=+$,#".contains(ch) {
            out.push_str(&s[start..i]);
        } else {
            out.push_str(ch);
        }
    }
    Some(out)
}

pub fn decode_uri(s: &str) -> Option<String> {
    decode(s, true)
}

pub fn decode_uri_component(s: &str) -> Option<String> {
    decode(s, false)
}

fn safe_decode(s: &str) -> String {
    decode_uri(s).unwrap_or_else(|| s.to_string())
}

fn re(p: &str) -> Regex {
    Regex::new(p).unwrap()
}

// ---------- which field ----------

/// Display / metadata keys that can hold an item's image, best first. image_base_uri last: the
/// SealionSui badges put the whole image there (a raw-leaf CID); when it is a directory it fails
/// like any broken source.
pub const IMAGE_KEYS: [&str; 13] = ["image_url", "image", "img_url", "imageUrl", "image_uri", "imageUri", "media_url", "animation_url", "url", "thumbnail", "thumbnail_url", "image_link", "image_base_uri"];
/// Keys that name any kind of media (or a page): only an image-looking value counts.
const IMAGE_ONLY: [&str; 3] = ["animation_url", "url", "image_link"];
static IMG_EXT: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\.(png|jpe?g|gif|webp|svg|avif|bmp)([?#].*)?$|^data:image/"));
/// Move fields that commonly wrap an NFT's metadata (one level down is searched too).
const NESTED: [&str; 7] = ["metadata", "data", "fields", "inner", "info", "nft", "content"];
static NULLISH: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(null|undefined|none)$"));

fn is_str(v: Option<&Value>) -> Option<&str> {
    match v {
        Some(Value::String(s)) if !s.trim().is_empty() && !NULLISH.is_match(s.trim()) => Some(s),
        _ => None,
    }
}

/// The first usable image reference in a Display-like object, with the key it came from.
pub fn pick_image_field(fields: &Map<String, Value>) -> Option<(&'static str, String)> {
    for k in IMAGE_KEYS {
        let Some(v) = is_str(fields.get(k)) else { continue };
        let s = v.trim();
        if IMAGE_ONLY.contains(&k) && !IMG_EXT.is_match(s) {
            continue;
        }
        if matches!(parse_ref(s), Ref::Invalid(_)) {
            continue;
        }
        return Some((k, s.to_string()));
    }
    None
}

pub fn pick_image(fields: &Map<String, Value>) -> Option<String> {
    pick_image_field(fields).map(|(_, v)| v)
}

/// For objects with no Display image: an image field in the object's own Move JSON (top level, then
/// one wrapper down, e.g. `metadata.image_uri`), or an inscription's data: image in `contents`.
pub fn pick_image_from_json(json: &Map<String, Value>) -> Option<String> {
    if let Some(top) = pick_image(json) {
        return Some(top);
    }
    for k in NESTED {
        if let Some(Value::Object(o)) = json.get(k) {
            if let Some(hit) = pick_image(o) {
                return Some(hit);
            }
        }
    }
    if let Some(Value::Array(c)) = json.get("contents") {
        if let Some(Value::String(s)) = c.first() {
            if s.len() > 64 && s.get(..11).is_some_and(|p| p.eq_ignore_ascii_case("data:image/")) {
                return Some(s.clone());
            }
        }
    }
    None
}

// ---------- references ----------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ref {
    Http { url: String },
    Ipfs { path: String, gateway: Option<String> },
    Arweave { id: String, url: Option<String> },
    Walrus { path: String, original: Option<String> },
    Data { uri: String, mime: String },
    Invalid(String),
}

static CIDV0: LazyLock<Regex> = LazyLock::new(|| re(r"^Qm[1-9A-HJ-NP-Za-km-z]{44}$"));
static CIDV1: LazyLock<Regex> = LazyLock::new(|| re(r"^b[a-z2-7]{50,}$"));
pub fn is_cid(s: &str) -> bool {
    CIDV0.is_match(s) || CIDV1.is_match(s)
}
/// Walrus blob ids are 32 bytes unpadded base64url (43 chars); quilt patch ids are 37 bytes (50 chars).
static WALRUS_BLOB: LazyLock<Regex> = LazyLock::new(|| re(r"^[A-Za-z0-9_-]{43}$"));
static WALRUS_PATCH: LazyLock<Regex> = LazyLock::new(|| re(r"^[A-Za-z0-9_-]{48,52}$"));
static ARWEAVE_ID: LazyLock<Regex> = LazyLock::new(|| re(r"^[A-Za-z0-9_-]{43}$"));

/// Hosts that no longer serve anything (DNS gone or content deleted). Only a rescue override fixes
/// their items.
pub const DEAD_HOSTS: [&str; 2] = ["walrus.tusky.io", "ipfs.bluemove.net"];
/// IPFS gateways that refuse server and <img> fetches (403/429 or a Cloudflare challenge since Sep
/// 2026) or are gone: their URLs are rewritten to the live ones.
static DEAD_GATEWAY: LazyLock<Regex> = LazyLock::new(|| {
    re(r"^(ipfs\.io|gateway\.ipfs\.io|dweb\.link|nftstorage\.link|w3s\.link|cloudflare-ipfs\.com|cf-ipfs\.com|gateway\.pinata\.cloud|ipfs\.infura\.io|infura-ipfs\.io|4everland\.io|ipfs\.4everland\.io|gateway\.lighthouse\.storage|ipfs\.decentralized-content\.com|ipfs\.eth\.aragon\.network|hardbin\.com|ipfs\.runfission\.com|ipfs\.bluemove\.net|tradeport\.mypinata\.cloud|byzantion\.mypinata\.cloud|ipfs\.thirdwebcdn\.com|gateway\.ipfscdn\.io)$|\.ipfs\.(dweb\.link|nftstorage\.link|w3s\.link|cf-ipfs\.com|4everland\.io)$|^ipfs\.tradeport\.xyz$")
});
/// Hosts whose project moved its files to a new domain under the same paths: old host -> new host.
/// The new one is tried first, the old one stays as a fallback.
pub const HOST_MOVES: [(&str, &str); 1] = [
    // The Birds (WORM, 281k items): birds.dog's DNS is gone; thebirds.ai serves the same files.
    ("asset.birds.dog", "assets.thebirds.ai"),
];

/// The URL on its project's new host, or None.
pub fn moved_url(url: &str) -> Option<String> {
    let mut u = Url::parse(url).ok()?;
    let host = u.host_str()?.to_lowercase();
    let to = HOST_MOVES.iter().find(|(h, _)| *h == host)?.1;
    u.set_host(Some(to)).ok()?;
    u.set_scheme("https").ok()?;
    Some(u.to_string())
}

/// Live public IPFS gateways (tested Oct 2026), best first.
pub const IPFS_GATEWAYS: [&str; 4] = ["ipfs.filebase.io", "ipfs.raribleuserdata.com", "gw.ipfs-lens.dev", "dget.top"];
/// Public Walrus mainnet aggregators, best first.
pub const WALRUS_AGGREGATORS: [&str; 7] = [
    "aggregator.walrus-mainnet.walrus.space",
    "aggregator.mainnet.walrus.mirai.cloud",
    "aggregator.suicore.com",
    "walrus.globalstake.io",
    "walrus-mainnet-aggregator.stakecraft.com",
    "aggregator.walrus-mainnet.h2o-nodes.com",
    "walrus-agg.mainnet.obelisk.sh",
];
pub const ARWEAVE_GATEWAYS: [&str; 2] = ["arweave.net", "permagate.io"];

fn host_of(u: &str) -> String {
    Url::parse(u).ok().and_then(|u| u.host_str().map(|h| h.to_lowercase())).unwrap_or_default()
}

pub fn is_dead_host(host: &str) -> bool {
    DEAD_HOSTS.iter().any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

static CTRL: LazyLock<Regex> = LazyLock::new(|| re("[\u{0000}-\u{001f}\u{200b}-\u{200d}\u{feff}]"));
static EDGES: LazyLock<Regex> = LazyLock::new(|| re(r#"^["'`(\[]+|["'`)\],;]+$"#));

/// Trim quotes, brackets, whitespace and trailing junk that creators leave in Display values.
pub fn clean_ref(raw: &str) -> String {
    let s = CTRL.replace_all(raw, "");
    let s = js_trim(&s);
    js_trim(&EDGES.replace_all(s, "")).to_string()
}

/// `String.prototype.trim` (JS whitespace, which `str::trim` matches for the cases that occur).
fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

static MULTI_SLASH: LazyLock<Regex> = LazyLock::new(|| re(r"([^:])/{2,}"));

/// IPFS path ("cid/rest?query") with duplicate slashes collapsed.
fn ipfs_path_of(p: &str) -> String {
    MULTI_SLASH.replace_all(p.trim_start_matches('/'), "$1/").to_string()
}

fn head(s: &str) -> &str {
    s.split(['/', '?', '#']).next().unwrap_or("")
}

static RE_DATA: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^data:"));
static RE_SVG: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^<(svg|\?xml)"));
static RE_IPFS: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:ipfs:/{0,2}(?:ipfs/)?|/ipfs/)(.+)$"));
static RE_IPNS: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^ipns://"));
static RE_WALRUS: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^walrus://"));
static RE_AR: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^ar://"));
static RE_SCHEME: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^[a-z][a-z0-9+.-]*:"));
static RE_BARE_HOST: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^[a-z0-9-]+(\.[a-z0-9-]+)+/"));
static RE_PATH_GW: LazyLock<Regex> = LazyLock::new(|| re(r"^/ipfs/([^/]+)(.*)$"));
static RE_SUB_GW: LazyLock<Regex> = LazyLock::new(|| re(r"^([a-z0-9]+)\.ipfs\."));
static RE_WALRUS_HOST: LazyLock<Regex> = LazyLock::new(|| re(r"walrus|wal\.app$|^wal\.gg$"));
static RE_WALRUS_PATH: LazyLock<Regex> = LazyLock::new(|| re(r"^/v1/(?:blobs/)?((?:by-quilt-patch-id/|by-quilt-id/[^/]+/)?[A-Za-z0-9_-]{20,}.*)$"));
static RE_WALGG: LazyLock<Regex> = LazyLock::new(|| re(r"(^|\.)wal\.gg$|walrus"));
static RE_QUILT: LazyLock<Regex> = LazyLock::new(|| re(r"^by-quilt-(patch-)?id/"));
static RE_NULL_REF: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(null|undefined|none|nan)$"));

fn search_of(u: &Url) -> String {
    match u.query() {
        Some(q) if !q.is_empty() => format!("?{q}"),
        _ => String::new(),
    }
}

/// One reference, classified. Never fails.
pub fn parse_ref(input: &str) -> Ref {
    let mut s = clean_ref(input);
    if s.is_empty() || RE_NULL_REF.is_match(&s) {
        return Ref::Invalid("empty".into());
    }
    if RE_DATA.is_match(&s) {
        let end = s[5..].find([';', ',']).map(|i| i + 5).unwrap_or(s.len());
        let mime = js_trim(&s[5..end]).to_lowercase();
        return Ref::Data { mime: if mime.is_empty() { "text/plain".into() } else { mime }, uri: s };
    }
    if RE_SVG.is_match(&s) {
        return Ref::Data { uri: format!("data:image/svg+xml;charset=utf-8,{}", enc(&s)), mime: "image/svg+xml".into() };
    }
    // ipfs:// ipfs://ipfs/ ipfs:/ ipfs: ipns:// /ipfs/ and bare CIDs
    if let Some(c) = RE_IPFS.captures(&s) {
        let path = ipfs_path_of(&c[1]);
        return if is_cid(head(&path)) { Ref::Ipfs { path, gateway: None } } else { Ref::Invalid("bad ipfs cid".into()) };
    }
    if RE_IPNS.is_match(&s) {
        return Ref::Http { url: format!("https://{}/ipns/{}", IPFS_GATEWAYS[0], &s[7..]) };
    }
    if is_cid(head(&s)) {
        return Ref::Ipfs { path: ipfs_path_of(&s), gateway: None };
    }
    if RE_WALRUS.is_match(&s) {
        return walrus_ref(s[9..].trim_start_matches('/'), None);
    }
    if RE_AR.is_match(&s) {
        let id = s[5..].trim_start_matches('/').to_string();
        return if ARWEAVE_ID.is_match(head(&id)) { Ref::Arweave { id, url: None } } else { Ref::Invalid("bad arweave id".into()) };
    }
    if WALRUS_BLOB.is_match(&s) {
        return walrus_ref(&s, None);
    }
    if s.starts_with("//") {
        s = format!("https:{s}");
    }
    // Bare host/path ("example.com/a.png").
    if !RE_SCHEME.is_match(&s) && RE_BARE_HOST.is_match(&s) {
        s = format!("https://{s}");
    }
    let Ok(mut u) = Url::parse(&s) else { return Ref::Invalid("bad url".into()) };
    if u.scheme() != "http" && u.scheme() != "https" {
        return Ref::Invalid(format!("scheme {}", u.scheme()));
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Ref::Invalid("credentials in url".into());
    }
    u.set_fragment(None);
    let host = u.host_str().unwrap_or("").to_lowercase();
    let path = u.path().to_string();
    let search = search_of(&u);
    let href = u.to_string();
    // IPFS path and subdomain gateways.
    if let Some(c) = RE_PATH_GW.captures(&path) {
        if is_cid(&c[1]) {
            let host_port = match u.port() {
                Some(p) => format!("{}:{p}", u.host_str().unwrap_or("")),
                None => u.host_str().unwrap_or("").to_string(),
            };
            let gateway = if DEAD_GATEWAY.is_match(&host) { None } else { Some(format!("{}://{host_port}", u.scheme())) };
            return Ref::Ipfs { path: ipfs_path_of(&(safe_decode(&format!("{}{}", &c[1], &c[2])) + &search)), gateway };
        }
    }
    if let Some(c) = RE_SUB_GW.captures(&host) {
        if is_cid(&c[1]) {
            let p = if path == "/" { String::new() } else { safe_decode(&path) };
            return Ref::Ipfs { path: ipfs_path_of(&format!("{}{p}{search}", &c[1])), gateway: None };
        }
    }
    // Walrus aggregators: /v1/blobs/<id>, /v1/<id> (legacy), /v1/blobs/by-quilt-patch-id/<id>,
    // /v1/blobs/by-quilt-id/<q>/<name>.
    if RE_WALRUS_HOST.is_match(&host) || path.starts_with("/v1/blobs/") {
        if let Some(c) = RE_WALRUS_PATH.captures(&path) {
            return walrus_ref(&safe_decode(&c[1]), if is_dead_host(&host) { None } else { Some(href) });
        }
    }
    // Blob-id hosts: https://wal.gg/<blob>, https://walrus.tusky.io/<blob>
    let bare = &path[1.min(path.len())..];
    if (WALRUS_BLOB.is_match(bare) || WALRUS_PATCH.is_match(bare)) && RE_WALGG.is_match(&host) {
        return walrus_ref(bare, if is_dead_host(&host) { None } else { Some(href) });
    }
    if host == "arweave.net" || host.ends_with(".arweave.net") || host == "ar-io.net" || host.ends_with(".ar-io.net") {
        let id = bare;
        if ARWEAVE_ID.is_match(id.split('/').next().unwrap_or("")) {
            let url = (host.contains('.') && host.split('.').count() > 2).then(|| href.clone());
            return Ref::Arweave { id: format!("{id}{search}"), url };
        }
        return Ref::Http { url: href };
    }
    Ref::Http { url: href }
}

fn walrus_ref(id: &str, original: Option<String>) -> Ref {
    if RE_QUILT.is_match(id) {
        return Ref::Walrus { path: format!("/v1/blobs/{id}"), original };
    }
    let h = head(id);
    if WALRUS_BLOB.is_match(h) {
        return Ref::Walrus { path: format!("/v1/blobs/{id}"), original };
    }
    if WALRUS_PATCH.is_match(h) {
        return Ref::Walrus { path: format!("/v1/blobs/by-quilt-patch-id/{id}"), original };
    }
    match original {
        Some(url) => Ref::Http { url },
        None => Ref::Invalid("bad walrus id".into()),
    }
}

/// A canonical string per image (one cache key whatever gateway or spelling was used).
pub fn ref_key(r: &Ref) -> String {
    match r {
        Ref::Http { url } => url.clone(),
        Ref::Ipfs { path, .. } => format!("ipfs://{path}"),
        Ref::Arweave { id, .. } => format!("ar://{id}"),
        Ref::Walrus { path, .. } => format!("walrus://{}", path.strip_prefix("/v1/blobs/").unwrap_or(path)),
        Ref::Data { uri, .. } => uri.clone(),
        Ref::Invalid(_) => String::new(),
    }
}

fn uniq(xs: impl IntoIterator<Item = Option<String>>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for x in xs.into_iter().flatten() {
        if !x.is_empty() && !out.contains(&x) {
            out.push(x);
        }
    }
    out
}

/// URLs to fetch for a reference, best first: a creator's own live gateway, then the live public
/// ones. Plain http also gets its https form first. Data URIs return themselves.
pub fn candidates(r: &Ref) -> Vec<String> {
    match r {
        Ref::Http { url } => {
            if is_dead_host(&host_of(url)) {
                return vec![];
            }
            let mut v = vec![moved_url(url)];
            match url.strip_prefix("http://") {
                Some(rest) => {
                    v.push(Some(format!("https://{rest}")));
                    v.push(Some(url.clone()));
                }
                None => v.push(Some(url.clone())),
            }
            uniq(v)
        }
        Ref::Ipfs { path, gateway } => uniq(std::iter::once(gateway.as_ref().map(|g| format!("{g}/ipfs/{path}"))).chain(IPFS_GATEWAYS.iter().map(|g| Some(format!("https://{g}/ipfs/{path}"))))),
        Ref::Arweave { id, url } => uniq(std::iter::once(url.clone()).chain(ARWEAVE_GATEWAYS.iter().map(|g| Some(format!("https://{g}/{id}"))))),
        Ref::Walrus { path, original } => uniq(std::iter::once(original.clone()).chain(WALRUS_AGGREGATORS.iter().map(|a| Some(format!("https://{a}{path}"))))),
        Ref::Data { uri, .. } => vec![uri.clone()],
        Ref::Invalid(_) => vec![],
    }
}

/// Every URL to try for one raw reference.
pub fn media_candidates(raw: &str) -> Vec<String> {
    candidates(&parse_ref(raw))
}

static RE_DATA_URI: LazyLock<Regex> = LazyLock::new(|| re(r"(?s)^data:([^,]*?),(.*)$"));
static RE_B64: LazyLock<Regex> = LazyLock::new(|| re(r"(?i);base64"));

/// The data: URI's payload as UTF-8 text (JSON or SVG), or None for binary or bad URIs.
pub fn data_text(uri: &str) -> Option<String> {
    use base64::Engine;
    let c = RE_DATA_URI.captures(uri)?;
    if RE_B64.is_match(&c[1]) {
        let cleaned: String = c[2].chars().filter(|ch| !ch.is_whitespace()).collect();
        // atob accepts missing padding.
        let bin = base64::engine::general_purpose::STANDARD_NO_PAD.decode(cleaned.trim_end_matches('=')).ok()?;
        return String::from_utf8(bin).ok();
    }
    decode_uri_component(&c[2])
}

static RE_JSONISH: LazyLock<Regex> = LazyLock::new(|| re(r"json|text/plain"));
static RE_SVG_TEXT: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^\s*<svg"));

/// Follows a metadata reference that is inline (a data:application/json URI) to its image. Other
/// refs pass through.
pub fn inline_image(raw: &str) -> String {
    let Ref::Data { uri, mime } = parse_ref(raw) else { return raw.to_string() };
    if !RE_JSONISH.is_match(&mime) {
        return raw.to_string();
    }
    let Some(t) = data_text(&uri) else { return raw.to_string() };
    if RE_SVG_TEXT.is_match(&t) {
        return format!("data:image/svg+xml;charset=utf-8,{}", enc(&t));
    }
    match serde_json::from_str::<Value>(&t) {
        Ok(Value::Object(j)) => pick_image(&j).unwrap_or_else(|| raw.to_string()),
        _ => raw.to_string(),
    }
}

/// Placeholder art and "no image" stand-ins.
static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)placehold|/(null|undefined|none|nan)(\.\w+)?$|no[-_]?image|image[-_]?(not[-_]?found|missing|unavailable)"));
static RE_IMAGE_MIME: LazyLock<Regex> = LazyLock::new(|| re(r"^image/"));

/// A reference that can show an image at all (before anything is fetched).
pub fn usable_ref(raw: Option<&str>) -> bool {
    let Some(raw) = raw.filter(|r| !r.trim().is_empty()) else { return false };
    let r = parse_ref(&inline_image(raw));
    match &r {
        Ref::Invalid(_) => false,
        Ref::Data { uri, mime } => RE_IMAGE_MIME.is_match(mime) && uri.encode_utf16().count() > 64,
        Ref::Http { url } if PLACEHOLDER.is_match(url) || is_dead_host(&host_of(url)) => false,
        _ => !candidates(&r).is_empty(),
    }
}

/// Hosts wsrv.nl refuses ("Domain or TLD blocked by policy", seen Oct 2026).
static WSRV_BLOCKED: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\.(xyz|fun)$|(^|\.)(thebirds\.ai|gamexchange\.app)$"));
pub fn wsrv_blocked(url: &str) -> bool {
    WSRV_BLOCKED.is_match(&host_of(url))
}

// ---------- rare.market web/lib/media.ts: the image chain ----------
//
// One to one with the site (wsrv mode, own thumbnails on), so the launcher and rare.market show the
// same art from the same links: rare.market's own rendition first (market-img thumbnails from
// disk: /img/src/{240|480|1024}.webp?url=<source>, or /img/<item|collection>/<id>/<size>.webp for
// the API's own paths), then the wsrv front (/img/w), wsrv.nl directly, the originals, then the
// passthrough (/img/pass). The site's relative links are on https://rare.market here, and the
// site's last link (the original as a video poster) is left out.

const RARE: &str = "https://rare.market";
const IMG_BASE: &str = "/img";
const IMG_W: &str = "https://rare.market/img/w/i.webp";
const WSRV: &str = "https://wsrv.nl/";

/// "cover" crops to a square (grids), "inside" keeps the aspect (item page).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fit {
    #[default]
    Cover,
    Inside,
}

static OWN_PATH: LazyLock<Regex> = LazyLock::new(|| re(r"^/img/(item|collection)/([^/?#]+)/(thumb|grid|full)$"));
static RE_PASS: LazyLock<Regex> = LazyLock::new(|| re(r"^/img/pass\?url=([^&]+)"));
static RE_HTTP: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^https?://"));
static RE_DATA_BLOB: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(data|blob):"));

fn raw_of(input: Option<&str>) -> String {
    input.map(|s| inline_image(js_trim(s))).unwrap_or_default()
}

/// The site's relative links on rare.market.
fn abs(u: String) -> String {
    if u.starts_with('/') && !u.starts_with("//") {
        format!("{RARE}{u}")
    } else {
        u
    }
}

/// `mediaCandidates`: direct URLs for one reference, best first (dead hosts give none). Own /img
/// paths and data: URIs return themselves (relative, as the site has them).
fn site_candidates(input: Option<&str>) -> Vec<String> {
    let raw = raw_of(input);
    if raw.is_empty() || raw == "null" || raw == "undefined" {
        return vec![];
    }
    if raw.starts_with("blob:") || (raw.starts_with('/') && !raw.starts_with("//") && !raw.starts_with("/ipfs/")) {
        return vec![raw];
    }
    match parse_ref(&raw) {
        Ref::Data { uri, mime } => {
            if mime.starts_with("image/") {
                vec![uri]
            } else {
                vec![]
            }
        }
        r => candidates(&r),
    }
}

/// market-img's rendition for a slot: the square 240 or 480 for cropped slots up to 480 px, 1024
/// (aspect kept) for anything larger or uncropped, none above 1024.
pub fn thumb_size(width: f64, fit: Fit) -> Option<u32> {
    if width > 1024.0 {
        return None;
    }
    if fit == Fit::Inside || width > 480.0 {
        return Some(1024);
    }
    Some(if width <= 240.0 { 240 } else { 480 })
}

fn wsrv_query(url: &str, width: f64, fit: Fit) -> String {
    let w = crate::format::js_round(width).clamp(16.0, 2048.0) as i64;
    let f = if fit == Fit::Inside { "inside" } else { "cover" };
    format!("?url={}&w={w}&h={w}&fit={f}&output=webp&q=80{}", enc(url), if fit == Fit::Inside { "&we" } else { "" })
}

/// wsrv.nl directly (the fallback behind the front).
pub fn wsrv_direct_url(url: &str, width: f64, fit: Fit) -> String {
    format!("{WSRV}{}", wsrv_query(url, width, fit))
}

/// The same rendition through rare.market's cached front (/img/w), which asks wsrv.nl on a miss.
pub fn wsrv_url(url: &str, width: f64, fit: Fit) -> String {
    format!("{IMG_W}{}", wsrv_query(url, width, fit))
}

/// wsrv.nl will fetch this URL (http(s) and not on a host it refuses).
pub fn is_proxyable(url: &str) -> bool {
    RE_HTTP.is_match(url) && !wsrv_blocked(url)
}

fn pass_url(raw: &str) -> String {
    format!("{IMG_BASE}/pass?url={}", enc(raw))
}

/// `proxyUrl`: the first link of the fallback chain, or "" when it must load directly.
fn proxy_url(input: Option<&str>, width: f64, fit: Fit) -> String {
    let Some(url) = site_candidates(input).into_iter().next() else { return String::new() };
    if url.starts_with("data:") || url.starts_with("blob:") {
        return url;
    }
    if let Some(c) = OWN_PATH.captures(&url) {
        return format!("{IMG_BASE}/{}/{}/{}.webp", &c[1], &c[2], thumb_size(width, fit).unwrap_or(1024));
    }
    if url.starts_with('/') {
        return url;
    }
    if is_proxyable(&url) {
        wsrv_url(&url, width, fit)
    } else {
        String::new()
    }
}

/// `ownThumbUrl`: rare.market's own rendition of a source for this slot, or None for inline data,
/// own paths, references that are not fetchable sources and slots wider than 1024. A /img/pass
/// link stands for its source.
pub fn own_thumb_url(input: Option<&str>, width: f64, fit: Fit) -> Option<String> {
    let mut raw = raw_of(input);
    if let Some(c) = RE_PASS.captures(&raw) {
        raw = decode_uri_component(&c[1])?;
    }
    let size = thumb_size(width, fit)?;
    if raw.is_empty() || (raw.starts_with('/') && !raw.starts_with("/ipfs/")) || RE_DATA_BLOB.is_match(&raw) {
        return None;
    }
    if !matches!(parse_ref(&raw), Ref::Http { .. } | Ref::Ipfs { .. } | Ref::Walrus { .. } | Ref::Arweave { .. }) {
        return None;
    }
    // Nothing to fetch (a dead host): no request at all, the tile shows at once.
    if site_candidates(Some(&raw)).is_empty() {
        return None;
    }
    Some(abs(format!("{IMG_BASE}/src/{size}.webp?url={}", enc(&raw))))
}

/// The chain after the own rendition: the proxy, the originals, the passthrough.
fn proxy_chain(input: Option<&str>, width: f64, fit: Fit) -> Vec<String> {
    let raw = raw_of(input);
    // The API sends a passthrough link for collections whose host refuses wsrv: that first, the
    // originals after.
    if let Some(c) = RE_PASS.captures(&raw) {
        let src = decode_uri_component(&c[1]).unwrap_or_default();
        let mut v = vec![Some(format!("{IMG_BASE}/pass?url={}", &c[1]))];
        if !src.is_empty() {
            v.extend(site_candidates(Some(&src)).into_iter().take(2).map(Some));
        }
        return uniq(v);
    }
    let first = proxy_url(input, width, fit);
    let cands = site_candidates(input);
    let Some(c0) = cands.first().cloned() else { return uniq([Some(first)]) };
    if c0.starts_with("data:") || c0.starts_with("blob:") || c0.starts_with('/') {
        return uniq([Some(if first.is_empty() { c0 } else { first })]);
    }
    let direct: Vec<String> = cands.iter().take(2).cloned().collect();
    // Hosts wsrv refuses (.xyz, .fun, ...) often serve AVIF only: the passthrough converts it to
    // WebP, so it goes before the raw original.
    let blocked = direct.iter().any(|u| wsrv_blocked(u));
    let mut proxied: Vec<String> = direct.iter().filter(|u| is_proxyable(u)).map(|u| wsrv_url(u, width, fit)).collect();
    if is_proxyable(&c0) {
        let at = 1.min(proxied.len());
        proxied.insert(at, wsrv_direct_url(&c0, width, fit));
    }
    let pass = pass_url(&raw);
    let tail: Vec<String> = if blocked { std::iter::once(pass).chain(direct).collect() } else { direct.into_iter().chain(std::iter::once(pass)).collect() };
    uniq(std::iter::once(first).chain(proxied).chain(tail).map(Some))
}

/// Ordered sources to try for one image, duplicates removed (the site's `imageChain`): rare's own
/// rendition, then the wsrv front over the first two candidates, wsrv.nl directly, the first two
/// candidates themselves, then the passthrough. `width` is the slot's size in pixels. The last link
/// failing is a quiet tile.
pub fn image_chain(input: Option<&str>, width: f64, fit: Fit) -> Vec<String> {
    let rest: Vec<String> = proxy_chain(input, width, fit).into_iter().map(abs).collect();
    match own_thumb_url(input, width, fit) {
        Some(own) if !rest.is_empty() => uniq(std::iter::once(own).chain(rest).map(Some)),
        _ => rest,
    }
}

/// The image cache's size bucket for a drawn width (lib/state.ts `sizeFor`).
pub fn size_for(width: f64) -> u32 {
    for b in [64, 128, 192, 256, 400, 512, 720, 960, 1280, 1600, 1920, 2560] {
        if b as f64 >= width {
            return b;
        }
    }
    2560
}

/// Browser-like request headers for a fetch on behalf of a viewer.
pub const BROWSER_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";
pub fn browser_headers(url: &str) -> Vec<(String, String)> {
    let mut h = vec![
        ("user-agent".to_string(), BROWSER_UA.to_string()),
        ("accept".to_string(), "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8".to_string()),
        ("accept-language".to_string(), "en-US,en;q=0.9".to_string()),
    ];
    if let Ok(u) = Url::parse(url) {
        let origin = u.origin().ascii_serialization();
        if origin != "null" {
            h.push(("referer".to_string(), format!("{origin}/")));
        }
    }
    h
}

#[cfg(test)]
mod tests;
