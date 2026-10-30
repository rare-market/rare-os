//! Remote access (SSH) in Settings. The image's helper `/usr/libexec/rare/os-admin ssh ...` does the
//! root work (start or stop sshd, passwords, keys); this runs it through pkexec like the update
//! actions (polkit org.rare.os.admin: the person at the device only, no password prompt) and reads
//! what it prints. Passwords and keys go in on stdin, never in the argument list. Off by default.
//! See docs/SSH.md.

use std::io::Write;
use std::process::{Command, Stdio};

const ADMIN: &str = "/usr/libexec/rare/os-admin";
/// The Local API on the local network ("Allow network apps"), one fixed port so tools can be set up once.
pub const NETAPI_PORT: u16 = 47100;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SshKey {
    /// `SHA256:...`
    pub fingerprint: String,
    pub kind: String,
    pub comment: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SshStatus {
    /// Switched on by the owner (sshd starts at boot).
    pub enabled: bool,
    /// sshd is running right now.
    pub active: bool,
    pub user: String,
    pub password_auth: bool,
    pub keep_password: bool,
    pub has_password: bool,
    /// Unix seconds when "Turn off after 1 hour" switches it off.
    pub auto_off_at: Option<u64>,
    /// The Local API port is open to the local network.
    pub netapi: bool,
    /// The password is still a default one (GameOS's playtron): password sign-in is off until a
    /// new one is set.
    pub default_password: bool,
    pub keys: Vec<SshKey>,
}

fn yes(v: &str) -> bool {
    v.trim() == "yes"
}

/// The helper's `ssh status` lines (key=value, `key=` lines for each public key).
pub fn parse_status(text: &str) -> SshStatus {
    let mut s = SshStatus::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        match k.trim() {
            "enabled" => s.enabled = yes(v),
            "active" => s.active = yes(v),
            "user" => s.user = v.trim().to_string(),
            "password_auth" => s.password_auth = yes(v),
            "keep_password" => s.keep_password = yes(v),
            "has_password" => s.has_password = yes(v),
            "default_password" => s.default_password = yes(v),
            "auto_off_at" => s.auto_off_at = v.trim().parse().ok(),
            "netapi" => s.netapi = yes(v),
            "key" => {
                let mut p = v.trim().splitn(3, ' ');
                if let (Some(fp), Some(kind)) = (p.next(), p.next()) {
                    let c = p.next().unwrap_or("").trim();
                    s.keys.push(SshKey { fingerprint: fp.to_string(), kind: kind.to_string(), comment: if c == "-" { String::new() } else { c.to_string() } });
                }
            }
            _ => {}
        }
    }
    s
}

fn admin(args: &[&str], stdin: Option<&str>) -> Result<String, String> {
    let mut cmd = Command::new("pkexec");
    cmd.arg("--disable-internal-agent").arg(ADMIN).args(args);
    let mut child = crate::host::env(&mut cmd).stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("pkexec: {e}"))?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let _ = pipe.write_all(text.as_bytes());
        let _ = pipe.write_all(b"\n");
    }
    let out = child.wait_with_output().map_err(|e| format!("pkexec: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let e = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if e.is_empty() { "The helper refused.".into() } else { e })
    }
}

pub fn status() -> Result<SshStatus, String> {
    admin(&["ssh", "status"], None).map(|t| parse_status(&t))
}

/// Whether sshd runs right now (for the top bar icon). Needs no root.
pub fn active() -> bool {
    crate::system::run("systemctl", &["is-active", "sshd.service"]).map(|s| s == "active").unwrap_or(false)
}

pub fn set_enabled(on: bool) -> Result<(), String> {
    admin(&["ssh", if on { "on" } else { "off" }], None).map(|_| ())
}

pub fn set_password(pw: &str) -> Result<(), String> {
    password_problem(pw).map_or(Ok(()), |p| Err(p.to_string()))?;
    admin(&["ssh", "set-password"], Some(pw)).map(|_| ())
}

/// A sentence when the password can't be used (the helper checks again).
pub fn password_problem(pw: &str) -> Option<&'static str> {
    let n = pw.chars().count();
    if n < 8 {
        Some("Use at least 8 characters.")
    } else if n > 128 {
        Some("Use 128 characters or fewer.")
    } else if pw.chars().any(|c| c.is_control()) {
        Some("Use letters, numbers and symbols only.")
    } else {
        None
    }
}

/// Adds public keys (one per line). The answer is the helper's "added=N skipped=N".
pub fn add_keys(lines: &[String]) -> Result<String, String> {
    if lines.is_empty() {
        return Err("There is no key to add.".into());
    }
    admin(&["ssh", "add-key"], Some(&lines.join("\n")))
}

pub fn remove_key(fingerprint: &str) -> Result<(), String> {
    if !fingerprint.starts_with("SHA256:") || fingerprint.chars().any(|c| c.is_whitespace()) {
        return Err("That isn't a key fingerprint.".into());
    }
    admin(&["ssh", "remove-key", fingerprint], None).map(|_| ())
}

pub fn keep_password(on: bool) -> Result<(), String> {
    admin(&["ssh", "keep-password", if on { "on" } else { "off" }], None).map(|_| ())
}

pub fn auto_off(on: bool) -> Result<(), String> {
    admin(&["ssh", "auto-off", if on { "on" } else { "off" }], None).map(|_| ())
}

/// Opens or closes the Local API port on the local network firewall.
pub fn netapi_firewall(on: bool) -> Result<(), String> {
    admin(&["netapi", if on { "on" } else { "off" }, &NETAPI_PORT.to_string()], None).map(|_| ())
}

// ----------------------------------------------------------------------------- keys from text

const KEY_TYPES: [&str; 5] = ["ssh-ed25519", "ssh-rsa", "ecdsa-sha2-", "sk-ssh-ed25519@openssh.com", "sk-ecdsa-sha2-nistp256@openssh.com"];

/// One public key line, or None. Lines with options in front (`command="..."`) are refused, as is
/// anything that doesn't start with a key type and a base64 body.
pub fn clean_key_line(line: &str) -> Option<String> {
    let l = line.trim();
    let mut parts = l.split_whitespace();
    let kind = parts.next()?;
    let body = parts.next()?;
    if !KEY_TYPES.iter().any(|t| if t.ends_with('-') { kind.starts_with(t) } else { kind == *t }) {
        return None;
    }
    if body.len() < 16 || body.len() > 2048 || !body.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=') {
        return None;
    }
    let comment: String = parts.collect::<Vec<_>>().join(" ").chars().filter(|c| !c.is_control()).take(80).collect();
    Some(if comment.is_empty() { format!("{kind} {body}") } else { format!("{kind} {body} {comment}") })
}

/// Every valid key line in a pasted block or a file (comments and blank lines are skipped).
pub fn parse_keys(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in text.lines().take(500) {
        if l.trim_start().starts_with('#') {
            continue;
        }
        if let Some(k) = clean_key_line(l) {
            if !out.contains(&k) {
                out.push(k);
            }
        }
    }
    out
}

/// A GitHub user name: 1 to 39 letters, digits and single hyphens, not starting or ending with one.
pub fn github_user_ok(u: &str) -> bool {
    let u = u.trim();
    !u.is_empty() && u.len() <= 39 && !u.starts_with('-') && !u.ends_with('-') && !u.contains("--") && u.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

pub fn github_keys_url(user: &str) -> String {
    format!("https://github.com/{}.keys", user.trim())
}

/// Key files on USB sticks and cards: `authorized_keys` or any `*.pub` in the top folder (or `.ssh`)
/// of each mounted drive under /run/media. Returns (drive name, file contents) for files with keys.
pub fn usb_key_files() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Ok(users) = std::fs::read_dir("/run/media") else { return out };
    for u in users.flatten() {
        let Ok(drives) = std::fs::read_dir(u.path()) else { continue };
        for d in drives.flatten() {
            let name = d.file_name().to_string_lossy().to_string();
            for dir in [d.path(), d.path().join(".ssh")] {
                let Ok(files) = std::fs::read_dir(&dir) else { continue };
                for f in files.flatten() {
                    let fname = f.file_name().to_string_lossy().to_string();
                    if fname != "authorized_keys" && !fname.ends_with(".pub") {
                        continue;
                    }
                    let Ok(meta) = f.metadata() else { continue };
                    if !meta.is_file() || meta.len() > 64 * 1024 {
                        continue;
                    }
                    if let Ok(text) = std::fs::read_to_string(f.path()) {
                        if !parse_keys(&text).is_empty() {
                            out.push((name.clone(), text));
                        }
                    }
                }
            }
        }
    }
    out
}

/// What is in the clipboard (Wayland, then X11), for pasting a key. None when there is no tool.
pub fn clipboard_text() -> Option<String> {
    for (cmd, args) in [("wl-paste", vec!["-n"]), ("xclip", vec!["-o", "-selection", "clipboard"])] {
        if let Ok(t) = crate::system::run(cmd, &args) {
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

// ----------------------------------------------------------------------------- where to connect

/// The device's network name (what `ssh <name>.local` finds over mDNS).
pub fn hostname() -> String {
    let h = std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    h.or_else(|| crate::system::run("hostname", &[]).ok().filter(|s| !s.is_empty())).unwrap_or_else(|| "rare".into())
}

/// The address other devices on this network reach this one at: the interface the default route
/// uses (a UDP socket "connected" to a public address sends nothing).
pub fn lan_ip() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then(|| ip.to_string())
}

/// `ssh bazzite@suiplay.local`
pub fn connect_command(user: &str, host: &str) -> String {
    let h = host.trim_end_matches(".local");
    format!("ssh {user}@{h}.local")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ED: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";

    #[test]
    fn status_lines() {
        let t = "enabled=yes\nactive=yes\nuser=bazzite\npassword_auth=no\nkeep_password=no\nhas_password=yes\nauto_off_at=1700000000\nnetapi=no\nkey=SHA256:abc ssh-ed25519 me@laptop\nkey=SHA256:def ssh-rsa -\n";
        let s = parse_status(t);
        assert!(s.enabled && s.active && s.has_password && !s.password_auth && !s.netapi);
        assert_eq!(s.user, "bazzite");
        assert_eq!(s.auto_off_at, Some(1700000000));
        assert_eq!(s.keys.len(), 2);
        assert_eq!(s.keys[0].comment, "me@laptop");
        assert_eq!(s.keys[1].comment, "");
        assert_eq!(parse_status("enabled=no\nauto_off_at=none\n").auto_off_at, None);
        assert_eq!(parse_status(""), SshStatus::default());
    }

    #[test]
    fn key_lines() {
        assert_eq!(clean_key_line(&format!("  {ED} me@laptop  ")).unwrap(), format!("{ED} me@laptop"));
        assert_eq!(clean_key_line(ED).unwrap(), ED);
        // Options in front of the key are refused.
        assert!(clean_key_line(&format!("command=\"rm -rf /\" {ED}")).is_none());
        assert!(clean_key_line("ssh-ed25519").is_none());
        assert!(clean_key_line("ssh-ed25519 not base64 !!!").is_none());
        assert!(clean_key_line("hello world this is not a key").is_none());
        assert!(clean_key_line("ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTY me").is_some());
    }

    #[test]
    fn key_blocks_skip_comments_and_duplicates() {
        let t = format!("# my keys\n\n{ED} a\n{ED} a\nnot a key\n");
        assert_eq!(parse_keys(&t).len(), 1);
    }

    #[test]
    fn passwords() {
        assert!(password_problem("short").is_some());
        assert!(password_problem("long enough 1").is_none());
        assert!(password_problem("bad\npassword!").is_some());
        assert!(password_problem(&"a".repeat(129)).is_some());
    }

    #[test]
    fn github_names() {
        assert!(github_user_ok("octocat"));
        assert!(github_user_ok("a-b1"));
        assert!(!github_user_ok("-a"));
        assert!(!github_user_ok("a--b"));
        assert!(!github_user_ok("a/b"));
        assert!(!github_user_ok(""));
        assert_eq!(github_keys_url(" octocat "), "https://github.com/octocat.keys");
    }

    #[test]
    fn connect() {
        assert_eq!(connect_command("bazzite", "suiplay"), "ssh bazzite@suiplay.local");
        assert_eq!(connect_command("bazzite", "suiplay.local"), "ssh bazzite@suiplay.local");
    }

    #[test]
    fn fingerprints_only() {
        assert!(remove_key("bad key").is_err());
        assert!(remove_key("MD5:aa").is_err());
    }
}
