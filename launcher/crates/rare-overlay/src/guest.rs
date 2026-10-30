//! Guest mode: Rare running as a shortcut inside someone else's Steam Game Mode (the launcher-only
//! AppImage added to Steam, or Bazzite's Steam session), not as Rare OS's own session client. There
//! Steam owns Guide and the quick access button and Rare can't take them, so the Rare menu is a View
//! hold and the hints say so.
//!
//! The launcher is a guest when either holds:
//! - a `SteamLaunch` process (Steam's reaper, `reaper SteamLaunch AppId=<id> -- ...`) is above it:
//!   Steam started it as a game or shortcut;
//! - it runs under gamescope but is not the session's own client (`RARE_SESSION_CLIENT=1`, set by
//!   Rare OS's session on the launcher only).
//!
//! Outside gamescope with no Steam above it (desktop mode, development) it is not a guest: the Rare
//! button works as on Rare OS.

use std::sync::OnceLock;

/// The decision, from the command lines of the launcher's ancestors (nearest first).
pub fn is_guest(ancestors: &[String], under_gamescope: bool, session_client: bool) -> bool {
    let steam_parent = ancestors.iter().any(|c| c.split(' ').any(|t| t == "SteamLaunch"));
    steam_parent || (under_gamescope && !session_client)
}

/// "pid (comm) S ppid ..." to (ppid, comm); comm may hold spaces and parentheses.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn parse_stat(stat: &str) -> Option<(u32, String)> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let comm = stat.get(open + 1..close)?.to_string();
    let ppid = stat.get(close + 1..)?.split_whitespace().nth(1)?.parse().ok()?;
    Some((ppid, comm))
}

/// Command lines of every process above this one, nearest first (Linux; empty elsewhere).
#[cfg(target_os = "linux")]
fn ancestors() -> Vec<String> {
    let mut out = Vec::new();
    let mut pid = std::process::id();
    for _ in 0..64 {
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { break };
        let Some((ppid, _)) = parse_stat(&stat) else { break };
        if ppid <= 1 {
            break;
        }
        let cmd = std::fs::read(format!("/proc/{ppid}/cmdline")).map(|b| String::from_utf8_lossy(&b).replace('\0', " ")).unwrap_or_default();
        out.push(cmd);
        pid = ppid;
    }
    out
}

#[cfg(not(target_os = "linux"))]
fn ancestors() -> Vec<String> {
    Vec::new()
}

/// Guest mode, worked out once (the parent chain and the session don't change while it runs).
pub fn guest() -> bool {
    static G: OnceLock<bool> = OnceLock::new();
    *G.get_or_init(|| {
        let g = is_guest(&ancestors(), crate::gamescope::active(), crate::gamescope::session_client());
        crate::host::log(&format!("[overlay] guest mode: {}", if g { "yes (Steam owns Guide; hold View for the Rare menu)" } else { "no" }));
        g
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_parent_from_proc_stat() {
        assert_eq!(parse_stat("12 (a) b) S 7 12 12"), Some((7, "a) b".to_string())));
        assert_eq!(parse_stat("garbage"), None);
    }

    #[test]
    fn who_is_a_guest() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        // Rare OS: the session's client, under gamescope, no Steam above.
        assert!(!is_guest(&s(&["gamescope --steam -- rare-launcher", "/usr/lib/systemd/systemd --user"]), true, true));
        // A shortcut in Steam's Game Mode: Steam's reaper is above it.
        assert!(is_guest(&s(&["/home/u/.steam/ubuntu12_32/reaper SteamLaunch AppId=2876543210 -- /home/u/Rare.AppImage", "steam -gamepadui"]), true, false));
        // Even outside gamescope (desktop Steam started it).
        assert!(is_guest(&s(&["reaper SteamLaunch AppId=1 -- rare"]), false, false));
        // Under someone else's gamescope session without the mark.
        assert!(is_guest(&s(&["bash"]), true, false));
        // Desktop or development.
        assert!(!is_guest(&s(&["bash", "Terminal"]), false, false));
        // A word that merely contains it is not Steam's reaper.
        assert!(!is_guest(&s(&["grep NotSteamLaunchy"]), false, false));
    }
}
