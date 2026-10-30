//! Captive portals (hotel and airport Wi-Fi): NetworkManager's connectivity state, and the page to
//! open so the portal shows its sign-in. Every call blocks, so callers run them on a worker thread.

use std::path::{Path, PathBuf};

/// NetworkManager's connectivity state (`nmcli networking connectivity`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connectivity {
    Full,
    /// A captive portal: the network wants a sign-in before it lets traffic out.
    Portal,
    /// Connected, but the internet is not reachable.
    Limited,
    /// Not connected.
    None,
    /// NetworkManager is not there, or its check is off.
    Unknown,
}

impl Connectivity {
    /// The network needs a sign-in, or might once its portal shows up: worth checking often.
    pub fn watch_closely(self) -> bool {
        matches!(self, Connectivity::Portal | Connectivity::Limited)
    }
}

/// One word from `nmcli`: "full", "portal", "limited", "none", "unknown".
pub fn parse_connectivity(out: &str) -> Connectivity {
    match out.trim().to_ascii_lowercase().as_str() {
        "full" => Connectivity::Full,
        "portal" => Connectivity::Portal,
        "limited" => Connectivity::Limited,
        "none" => Connectivity::None,
        _ => Connectivity::Unknown,
    }
}

fn nmcli(args: &[&str]) -> Option<String> {
    crate::system::run_for("nmcli", args, std::time::Duration::from_secs(25)).ok()
}

/// Asks NetworkManager to check now (it fetches its check page), else its last known state.
pub fn connectivity() -> Connectivity {
    if let Some(c) = nmcli(&["-t", "networking", "connectivity", "check"]).map(|o| parse_connectivity(&o)).filter(|c| *c != Connectivity::Unknown) {
        return c;
    }
    cached()
}

/// NetworkManager's last known state, without a new check. Cheap.
pub fn cached() -> Connectivity {
    nmcli(&["-t", "-f", "CONNECTIVITY", "general"]).map(|o| parse_connectivity(&o)).unwrap_or(Connectivity::Unknown)
}

/// The page opened when the network has no check page of its own. Plain http on purpose: a portal
/// can only step in on an unencrypted page.
pub const FALLBACK_URL: &str = "http://neverssl.com/";

/// `uri=` in the `[connectivity]` section of one NetworkManager config file.
pub fn parse_check_uri(text: &str) -> Option<String> {
    let mut in_section = false;
    let mut uri = None;
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with(';') {
            continue;
        }
        if l.starts_with('[') {
            in_section = l.trim_start_matches('[').trim_end_matches(']').trim() == "connectivity";
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            if k.trim() == "uri" {
                let v = v.trim();
                uri = (v.starts_with("http://") || v.starts_with("https://")).then(|| v.to_string());
            }
        }
    }
    uri
}

/// The check page from config files in NetworkManager's order (later files win).
pub fn check_uri_from(files: &[String]) -> Option<String> {
    files.iter().filter_map(|t| parse_check_uri(t)).next_back()
}

/// NetworkManager's read order: NetworkManager.conf, then every conf.d file by name, where a file in
/// /etc hides one of the same name in /run or /usr/lib.
fn config_files() -> Vec<PathBuf> {
    let mut by_name: std::collections::BTreeMap<String, PathBuf> = Default::default();
    for dir in ["/usr/lib/NetworkManager/conf.d", "/run/NetworkManager/conf.d", "/etc/NetworkManager/conf.d"] {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".conf") {
                by_name.insert(name, e.path());
            }
        }
    }
    let mut out = vec![Path::new("/etc/NetworkManager/NetworkManager.conf").to_path_buf()];
    out.extend(by_name.into_values());
    out
}

/// The page to open for a sign-in: NetworkManager's check page when one is set, else neverssl.
pub fn portal_url() -> String {
    let texts: Vec<String> = config_files().iter().filter_map(|p| std::fs::read_to_string(p).ok()).collect();
    check_uri_from(&texts).unwrap_or_else(|| FALLBACK_URL.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_connectivity() {
        assert_eq!(parse_connectivity("portal\n"), Connectivity::Portal);
        assert_eq!(parse_connectivity("full"), Connectivity::Full);
        assert_eq!(parse_connectivity(" LIMITED "), Connectivity::Limited);
        assert_eq!(parse_connectivity("none"), Connectivity::None);
        assert_eq!(parse_connectivity("unknown"), Connectivity::Unknown);
        assert_eq!(parse_connectivity(""), Connectivity::Unknown);
        assert!(Connectivity::Portal.watch_closely() && Connectivity::Limited.watch_closely() && !Connectivity::Full.watch_closely());
    }

    #[test]
    fn reads_the_check_page() {
        let fedora = "[connectivity]\nenabled=true\nuri=http://fedoraproject.org/static/hotspot.txt\nresponse=OK\ninterval=300\n";
        assert_eq!(parse_check_uri(fedora).as_deref(), Some("http://fedoraproject.org/static/hotspot.txt"));
        assert_eq!(parse_check_uri("[main]\nuri=http://x.example/\n[connectivity]\n# uri=http://no/\ninterval=0\n"), None);
        assert_eq!(parse_check_uri("[connectivity]\nuri = file:///etc/passwd\n"), None);
        // Later files win; files without one leave the earlier one.
        let files = vec![fedora.to_string(), "[connectivity]\n uri = https://check.example/ok \n".to_string(), "[main]\nplugins=keyfile\n".to_string()];
        assert_eq!(check_uri_from(&files).as_deref(), Some("https://check.example/ok"));
        assert_eq!(check_uri_from(&[]), None);
    }
}
