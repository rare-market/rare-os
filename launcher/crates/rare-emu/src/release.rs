//! Release feeds: GitHub, Gitea (Citron) and GitLab (ES-DE) JSON into one shape, and the choice of asset.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// SHA-256 hex the forge itself reports for the file (GitHub's `digest`), when it does.
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    /// ISO 8601, as the forge gives it.
    pub published: String,
    pub page: String,
    pub prerelease: bool,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Copy)]
pub enum Forge {
    GitHub(&'static str),
    /// base URL (no trailing slash), "Owner/Repo"
    Gitea(&'static str, &'static str),
    /// URL-encoded project path
    GitLab(&'static str),
}

impl Forge {
    pub fn api_url(&self) -> String {
        match self {
            Forge::GitHub(repo) => format!("https://api.github.com/repos/{repo}/releases?per_page=8"),
            Forge::Gitea(base, repo) => format!("{base}/api/v1/repos/{repo}/releases?limit=8"),
            Forge::GitLab(project) => format!("https://gitlab.com/api/v4/projects/{project}/releases?per_page=8"),
        }
    }
    /// Where a person reads the releases.
    pub fn page(&self) -> String {
        match self {
            Forge::GitHub(repo) => format!("https://github.com/{repo}/releases"),
            Forge::Gitea(base, repo) => format!("{base}/{repo}/releases"),
            Forge::GitLab(project) => format!("https://gitlab.com/{}/-/releases", project.replace("%2F", "/")),
        }
    }
    pub fn parse(&self, body: &str) -> Vec<Release> {
        match self {
            Forge::GitLab(_) => parse_gitlab(body),
            _ => parse_github(body),
        }
    }
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

/// GitHub and Gitea share this shape.
pub fn parse_github(body: &str) -> Vec<Release> {
    let Ok(Value::Array(list)) = serde_json::from_str::<Value>(body) else { return vec![] };
    list.iter()
        .filter(|r| r.get("draft").and_then(Value::as_bool) != Some(true))
        .map(|r| Release {
            tag: s(r, "tag_name"),
            published: if s(r, "published_at").is_empty() { s(r, "created_at") } else { s(r, "published_at") },
            page: s(r, "html_url"),
            prerelease: r.get("prerelease").and_then(Value::as_bool) == Some(true),
            assets: r
                .get("assets")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|x| Asset {
                            name: s(x, "name"),
                            url: s(x, "browser_download_url"),
                            size: x.get("size").and_then(Value::as_u64).unwrap_or(0),
                            sha256: s(x, "digest").strip_prefix("sha256:").map(|h| h.to_ascii_lowercase()).filter(|h| is_hex64(h)),
                        })
                        .filter(|a| !a.name.is_empty() && a.url.starts_with("https://"))
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect()
}

pub fn parse_gitlab(body: &str) -> Vec<Release> {
    let Ok(Value::Array(list)) = serde_json::from_str::<Value>(body) else { return vec![] };
    list.iter()
        .map(|r| Release {
            tag: s(r, "tag_name"),
            published: s(r, "released_at"),
            page: r.pointer("/_links/self").and_then(Value::as_str).unwrap_or("").to_string(),
            prerelease: r.get("upcoming_release").and_then(Value::as_bool) == Some(true),
            assets: r
                .pointer("/assets/links")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|x| {
                            let url = if s(x, "direct_asset_url").is_empty() { s(x, "url") } else { s(x, "direct_asset_url") };
                            Asset { name: s(x, "name"), url, size: 0, sha256: None }
                        })
                        .filter(|a| !a.name.is_empty() && a.url.starts_with("https://"))
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect()
}

/// The newest release that has a usable asset (stable ones first), with the best asset by `score`
/// (0 rejects a name; higher wins).
pub fn pick<'a>(releases: &'a [Release], score: fn(&str) -> u32) -> Option<(&'a Release, &'a Asset)> {
    for want_pre in [false, true] {
        for r in releases.iter().filter(|r| r.prerelease == want_pre) {
            if let Some(a) = r.assets.iter().map(|a| (score(&a.name), a)).filter(|(sc, _)| *sc > 0).max_by_key(|(sc, _)| *sc).map(|(_, a)| a) {
                return Some((r, a));
            }
        }
    }
    None
}

/// The checksum a sidecar asset publishes for `asset` (RPCS3 ships `<name>.sha256`), if the release has one.
pub fn sidecar<'a>(r: &'a Release, asset: &Asset) -> Option<&'a Asset> {
    ["", ".sha256", ".sha256sum", ".sha256.txt"].iter().skip(1).find_map(|suffix| {
        let want = format!("{}{suffix}", asset.name);
        r.assets.iter().find(|a| a.name.eq_ignore_ascii_case(&want))
    })
}

/// First 64-hex token of a checksum file ("<hash>  <name>" or just the hash).
pub fn parse_checksum(text: &str) -> Option<String> {
    let t = text.split_whitespace().next()?.to_ascii_lowercase();
    is_hex64(&t).then_some(t)
}

/// "2026-10-12T08:30:00Z" (or with an offset/fraction) to unix seconds; None when it does not parse.
pub fn iso_to_unix(t: &str) -> Option<i64> {
    let (date, time) = t.split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, dd) = (d.next()??, d.next()??, d.next()??);
    let time = time.trim_end_matches('Z');
    let time = time.split(['+', '.']).next()?;
    let mut hms = time.split(':').map(|x| x.parse::<i64>().ok());
    let (h, mi, se) = (hms.next()??, hms.next()??, hms.next().flatten().unwrap_or(0));
    // Days from civil (Howard Hinnant).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + dd - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + mi * 60 + se)
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GH: &str = r#"[
      {"tag_name":"v0.0.40-nightly","draft":false,"prerelease":true,"published_at":"2026-10-20T10:00:00Z","html_url":"https://github.com/x/y/releases/tag/n","assets":[
        {"name":"x.AppImage","browser_download_url":"https://github.com/x/y/n/x.AppImage","size":10}]},
      {"tag_name":"v0.0.39","draft":false,"prerelease":false,"published_at":"2026-10-01T00:00:00Z","html_url":"https://github.com/x/y/releases/tag/v","assets":[
        {"name":"rpcs3-v0.0.39-1_abc_linux_aarch64.AppImage","browser_download_url":"https://github.com/x/y/a.AppImage","size":5},
        {"name":"rpcs3-v0.0.39-1_abc_linux64.AppImage","browser_download_url":"https://github.com/x/y/b.AppImage","size":7,"digest":"sha256:ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789"},
        {"name":"rpcs3-v0.0.39-1_abc_linux64.AppImage.sha256","browser_download_url":"https://github.com/x/y/b.sha256","size":90}]},
      {"tag_name":"draft","draft":true,"assets":[]}
    ]"#;

    fn score(n: &str) -> u32 {
        let l = n.to_ascii_lowercase();
        if l.ends_with(".appimage") && l.contains("linux64") {
            1
        } else {
            0
        }
    }

    #[test]
    fn parses_github_and_picks_stable_first() {
        let rs = parse_github(GH);
        assert_eq!(rs.len(), 2, "drafts are dropped");
        let (r, a) = pick(&rs, score).unwrap();
        assert_eq!(r.tag, "v0.0.39");
        assert_eq!(a.size, 7);
        assert_eq!(a.sha256.as_deref(), Some("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"));
        assert_eq!(sidecar(r, a).unwrap().name, "rpcs3-v0.0.39-1_abc_linux64.AppImage.sha256");
    }

    #[test]
    fn prerelease_only_when_nothing_stable_matches() {
        let rs = parse_github(GH);
        let (r, _) = pick(&rs, |n| (n == "x.AppImage") as u32).unwrap();
        assert!(r.prerelease);
        assert!(pick(&rs, |_| 0).is_none());
    }

    #[test]
    fn parses_gitlab_links() {
        let j = r#"[{"tag_name":"v3.4.0","released_at":"2026-09-01T00:00:00.000Z","_links":{"self":"https://gitlab.com/es-de/e/-/releases/v3.4.0"},
          "assets":{"links":[{"name":"ES-DE_x64.AppImage","url":"https://x/u","direct_asset_url":"https://x/d"},{"name":"bad","url":"http://x"}]}}]"#;
        let rs = parse_gitlab(j);
        assert_eq!(rs[0].assets.len(), 1);
        assert_eq!(rs[0].assets[0].url, "https://x/d");
    }

    #[test]
    fn checksum_files_and_dates() {
        let h = "a".repeat(64);
        assert_eq!(parse_checksum(&format!("{h}  file.AppImage\n")).as_deref(), Some(h.as_str()));
        assert!(parse_checksum("not a hash").is_none());
        assert_eq!(iso_to_unix("1970-01-02T00:00:00Z"), Some(86400));
        assert_eq!(iso_to_unix("2000-03-01T00:00:00Z"), Some(951868800));
        assert_eq!(iso_to_unix("2026-09-01T00:00:00.000Z"), Some(1788220800));
        assert_eq!(iso_to_unix("garbage"), None);
    }
}

