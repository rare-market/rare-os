//! Where art comes from: http(s) URLs (Steam's CDN, rare.market), absolute paths (Steam's
//! librarycache, ES-DE media) and the web launcher's own `public/` files ("games/act/cover.jpg",
//! "apps/steam.png", "brand/mark.png"), which the native launcher reads from the same folder.
//!
//! [`resolve`] is the one resolver every image goes through (kit::art and everything built on it):
//! local files as they are, every remote reference (http, ipfs://, ar://, walrus://, data: URIs,
//! rare.market's relative /img paths) through rare.market's image chain at the drawn size, the same
//! links rare.market's site loads (rare_market::media::image_chain).

use rare_render::Src;
use std::path::PathBuf;
use std::sync::OnceLock;

/// launcher/public: $RR_PUBLIC, next to the binary (`public/`), or the source tree's.
pub fn public_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        if let Ok(p) = std::env::var("RR_PUBLIC") {
            return PathBuf::from(p);
        }
        if let Some(d) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("public"))) {
            if d.is_dir() {
                return d;
            }
        }
        // Dev builds run from the source tree; release builds never carry the build machine's path.
        #[cfg(debug_assertions)]
        return PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../public"));
        #[cfg(not(debug_assertions))]
        PathBuf::from("public")
    })
}

/// Links that are already a rendition (rare.market's image service, wsrv): fetched as they are.
fn is_rendition(s: &str) -> bool {
    // profile banners are already rare.market's own sized renditions (default set and uploads)
    s.starts_with("https://rare.market/banners/") || s.starts_with("https://rare.market/v1/banners/") || s.starts_with("https://wsrv.nl/") || s.starts_with("https://rare.market/img/w/") || s.starts_with("https://rare.market/img/src/") || s.starts_with("https://rare.market/img/pass?")
}

/// Game art CDNs that serve sized art fast themselves (Steam's capsules and heroes) and local
/// servers: fetched as they are.
fn is_direct(s: &str) -> bool {
    let host = s.split_once("://").map(|(_, r)| r.split(['/', '?', '#']).next().unwrap_or("")).unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or(host).split(':').next().unwrap_or(host);
    host.ends_with("steamstatic.com") || host.ends_with("steampowered.com") || host.ends_with("akamaihd.net") || host == "localhost" || host == "127.0.0.1"
}

/// Every source to try, best first, for one art reference drawn `px` pixels wide (the larger side),
/// `inside` when the slot keeps the picture's aspect (contain, or a box far from square).
pub fn resolve(raw: &str, px: f32, inside: bool) -> Vec<Src> {
    let raw = raw.trim();
    if raw.is_empty() {
        return vec![];
    }
    if is_rendition(raw) || is_direct(raw) {
        return vec![Src::Url(raw.to_string())];
    }
    // rare.market's own paths, absolute or relative, go to its image service.
    let rel = raw.strip_prefix("https://rare.market").filter(|r| r.starts_with("/img/")).unwrap_or(raw);
    if !rel.starts_with("/img/") && !rel.starts_with("/ipfs/") {
        if let Some(local) = local_src(raw) {
            return vec![local];
        }
        // A file on this device that isn't there (an uninstalled app's icon, a cleared Steam
        // cache): nothing to load. Never join it onto rare.market (it only answers 404).
        if is_device_path(raw) {
            return vec![];
        }
    }
    let fit = if inside { rare_market::media::Fit::Inside } else { rare_market::media::Fit::Cover };
    rare_market::media::image_chain(Some(rel), px.max(1.0).round() as f64, fit).into_iter().map(Src::Url).collect()
}

/// One link for a slot with no fallback (toasts, the in-game menu's rows): the first of the chain
/// that always answers (rare.market's front rather than its own rendition, which may not be made
/// yet).
pub fn single(raw: &str, px: f32) -> Option<Src> {
    let chain = resolve(raw, px, false);
    chain.iter().find(|s| !matches!(s, Src::Url(u) if u.contains("/img/src/"))).or(chain.first()).cloned()
}

/// An absolute path into the device's file system (Steam's librarycache, flatpak icons, ES-DE
/// media), as opposed to a rare.market path.
fn is_device_path(s: &str) -> bool {
    const ROOTS: [&str; 12] = ["/var/", "/home/", "/usr/", "/run/", "/opt/", "/tmp/", "/mnt/", "/media/", "/etc/", "/srv/", "/root/", "/nix/"];
    ROOTS.iter().any(|r| s.starts_with(r))
}

/// A local file: file://, an absolute path that exists, or a public/ file.
fn local_src(s: &str) -> Option<Src> {
    if s.contains("://") || s.starts_with("data:") {
        return s.strip_prefix("file://").map(|p| Src::File(PathBuf::from(p)));
    }
    if s.starts_with('/') && std::path::Path::new(s).exists() {
        return Some(Src::File(PathBuf::from(s)));
    }
    let p = public_dir().join(s.trim_start_matches('/'));
    p.is_file().then_some(Src::File(p))
}

pub fn src_of(s: &str) -> Option<Src> {
    if s.starts_with("http://") || s.starts_with("https://") {
        return Some(Src::Url(s.to_string()));
    }
    if let Some(p) = s.strip_prefix("file://") {
        return Some(Src::File(PathBuf::from(p)));
    }
    if s.starts_with('/') && std::path::Path::new(s).exists() {
        return Some(Src::File(PathBuf::from(s)));
    }
    let rel = s.trim_start_matches('/');
    let p = public_dir().join(rel);
    p.exists().then_some(Src::File(p))
}

/// A bundled file from public/ (brand marks, app icons).
pub fn public(rel: &str) -> Option<Src> {
    let p = public_dir().join(rel);
    p.exists().then_some(Src::File(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_device_files_never_go_to_rare_market() {
        assert!(resolve("/var/home/bazzite/.local/share/icons/emudeck/Nope.png", 200.0, false).is_empty());
        assert!(resolve("/home/nobody/x.jpg", 200.0, false).is_empty());
        assert!(!resolve("/img/abc.webp", 200.0, false).is_empty());
    }

    #[test]
    fn local_files_load_from_disk() {
        let f = std::env::temp_dir().join(format!("rare-art-test-{}.jpg", std::process::id()));
        std::fs::write(&f, b"x").unwrap();
        let p = f.to_string_lossy().to_string();
        assert_eq!(resolve(&p, 300.0, false), vec![Src::File(f.clone())]);
        assert_eq!(resolve(&format!("file://{p}"), 300.0, false), vec![Src::File(f.clone())]);
        // file:// is a local file even when it is gone: never a link.
        assert_eq!(resolve("file:///var/home/bazzite/gone.png", 300.0, false), vec![Src::File(PathBuf::from("/var/home/bazzite/gone.png"))]);
        for s in [p.as_str(), "/var/home/bazzite/.local/share/Steam/appcache/librarycache/1/x/library_hero.jpg", "/var/lib/flatpak/app/com.google.Chrome/icon.svg"] {
            assert!(resolve(s, 300.0, false).iter().all(|x| !matches!(x, Src::Url(u) if u.contains("rare.market"))), "{s}");
        }
        let _ = std::fs::remove_file(&f);
    }
}
