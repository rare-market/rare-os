//! F-Droid's repository: search, the suggested version of an app, and its APK from the fastest of
//! a few official mirrors (f-droid.org itself is often slow).

use serde::Deserialize;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

/// Official F-Droid mirrors, then the main site.
const MIRRORS: [&str; 5] = [
    "https://plug-mirror.rcac.purdue.edu/fdroid/repo",
    "https://fdroid.tetaneutral.net/fdroid/repo",
    "https://mirror.cyberbits.eu/fdroid/repo",
    "https://ftp.lysator.liu.se/pub/fdroid/repo",
    "https://f-droid.org/repo",
];

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout_read(Duration::from_secs(30)).user_agent("Rare").build()
}

/// One search result.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub package: String,
    pub name: String,
    pub summary: String,
    pub icon: Option<String>,
}

#[derive(Deserialize)]
struct SearchApp {
    name: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    icon: Option<String>,
    url: String,
}

#[derive(Deserialize)]
struct Search {
    apps: Vec<SearchApp>,
}

fn package_from_url(url: &str) -> Option<String> {
    let p = url.trim_end_matches('/').rsplit('/').next()?;
    (p.contains('.') && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')).then(|| p.to_string())
}

fn parse_search(body: &str) -> Vec<Hit> {
    let Ok(s) = serde_json::from_str::<Search>(body) else { return Vec::new() };
    s.apps
        .into_iter()
        .filter_map(|a| Some(Hit { package: package_from_url(&a.url)?, name: a.name.trim().to_string(), summary: a.summary.trim().to_string(), icon: a.icon.filter(|i| i.starts_with("https://")) }))
        .collect()
}

/// F-Droid's search (search.f-droid.org), best matches first.
pub fn search(q: &str) -> Result<Vec<Hit>, String> {
    let body = agent().get("https://search.f-droid.org/api/search_apps").query("q", q).call().map_err(|e| format!("F-Droid search: {e}"))?.into_string().map_err(|e| e.to_string())?;
    Ok(parse_search(&body))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Versions {
    suggested_version_code: u64,
}

/// The version F-Droid suggests for an app.
pub fn suggested(pkg: &str) -> Result<u64, String> {
    let v: Versions = agent()
        .get(&format!("https://f-droid.org/api/v1/packages/{pkg}"))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(404, _) => "F-Droid doesn't have this app".to_string(),
            e => format!("F-Droid: {e}"),
        })?
        .into_json()
        .map_err(|e| format!("F-Droid: {e}"))?;
    Ok(v.suggested_version_code)
}

/// Downloads `<pkg>_<code>.apk` to `dest`, trying the mirrors in turn. `progress(done, total)`.
pub fn download(pkg: &str, code: u64, dest: &Path, mut progress: impl FnMut(u64, u64)) -> Result<(), String> {
    let name = format!("{pkg}_{code}.apk");
    let tmp = dest.with_extension("part");
    let mut last = String::from("no mirror answered");
    for m in MIRRORS {
        let url = format!("{m}/{name}");
        let resp = match agent().get(&url).call() {
            Ok(r) => r,
            Err(e) => {
                last = format!("{m}: {e}");
                continue;
            }
        };
        let total: u64 = resp.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(0);
        let mut r = resp.into_reader();
        let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 256 * 1024];
        let mut done = 0u64;
        let ok = loop {
            match r.read(&mut buf) {
                Ok(0) => break true,
                Ok(n) => {
                    if f.write_all(&buf[..n]).is_err() {
                        break false;
                    }
                    done += n as u64;
                    progress(done, total);
                }
                Err(e) => {
                    last = format!("{m}: {e}");
                    break false;
                }
            }
        };
        drop(f);
        // An APK is a zip: anything else is an error page.
        let head = std::fs::read(&tmp).map(|b| b.starts_with(b"PK")).unwrap_or(false);
        if ok && head && (total == 0 || done == total) {
            std::fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
            return Ok(());
        }
        let _ = std::fs::remove_file(&tmp);
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_results() {
        let body = r#"{"apps": [{"name": "2048 (Privacy Friendly)", "summary": "Reach 2048", "icon": "https://x/icon.png", "url": "https://f-droid.org/en/packages/org.secuso.privacyfriendly2048"}, {"name": "Bad", "summary": "", "url": "https://f-droid.org/en/packages/"}]}"#;
        let h = parse_search(body);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].package, "org.secuso.privacyfriendly2048");
        assert_eq!(h[0].icon.as_deref(), Some("https://x/icon.png"));
        assert!(parse_search("not json").is_empty());
    }
}
