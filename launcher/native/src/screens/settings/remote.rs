//! Remote access (SSH) and network apps: the rows for Settings > Developer, and the short entry in
//! System. The state is `SshInfo` in the live snapshot (read through `rare_system::ssh`, which asks
//! the image's `os-admin ssh` helper). Off by default. See docs/SSH.md.

use super::rows::*;
use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::{Fit, Id, ImgReq, Node, Src};
use rare_system::ssh::{self, SshKey, SshStatus};

pub const WARNING: &str = "Anyone on your network with the password can control this device.";

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub struct SshInfo {
    /// None until the first reading, or when the helper isn't there (not a Rare OS image).
    pub status: Option<SshStatus>,
    pub host: String,
    pub ip: Option<String>,
    /// A change is under way.
    pub busy: bool,
    /// "Allow network apps" is on.
    pub netapi: bool,
    pub net_token: Option<String>,
    /// The capability switches that are on (`rare_localapi::CAPS` ids).
    pub caps: Vec<String>,
    /// The latest calls from network apps, newest first.
    #[serde(skip)]
    pub activity: Vec<rare_localapi::Activity>,
}

impl SshInfo {
    pub fn on(&self) -> bool {
        self.status.as_ref().is_some_and(|s| s.enabled)
    }
    pub fn user(&self) -> String {
        self.status.as_ref().map(|s| s.user.clone()).filter(|u| !u.is_empty()).unwrap_or_else(|| "bazzite".into())
    }
    pub fn command(&self) -> String {
        ssh::connect_command(&self.user(), &self.host)
    }
}

pub fn fixture_info() -> SshInfo {
    SshInfo { status: Some(SshStatus { user: "bazzite".into(), password_auth: true, ..Default::default() }), host: "suiplay".into(), ip: Some("192.168.1.92".into()), ..Default::default() }
}

/// What a fixture run (no device) shows: SSH off, or on with RR_FIXTURE_SSH=on (screenshots), or
/// on with GameOS's default password still set with RR_FIXTURE_SSH=default-password.
pub fn fixture_default() -> SshInfo {
    match std::env::var("RR_FIXTURE_SSH").as_deref() {
        Ok("on") => return fixture_on(),
        Ok("default-password") => {
            let mut i = fixture_on();
            if let Some(st) = i.status.as_mut() {
                st.user = "playtron".into();
                st.keys.clear();
                st.default_password = true;
                st.password_auth = false;
            }
            return i;
        }
        _ => {}
    }
    fixture_info()
}

/// A fixture with SSH on, a key and network apps allowed (screenshots).
pub fn fixture_on() -> SshInfo {
    let st = SshStatus {
        enabled: true,
        active: true,
        user: "bazzite".into(),
        has_password: true,
        keys: vec![SshKey { fingerprint: "SHA256:9fT2xQ0c1mZ7vYk4bN8aLr3uHw5eJd6sPqXoVtCiAg0".into(), kind: "ed25519".into(), comment: "me@laptop".into() }],
        ..Default::default()
    };
    let now = now_secs() * 1000;
    let act = |ago_s: u64, cap: &str, what: &str, ok: bool| rare_localapi::Activity { at_ms: now.saturating_sub(ago_s * 1000), cap: cap.into(), what: what.into(), ok, from: "192.168.0.50".into() };
    SshInfo {
        status: Some(st),
        netapi: true,
        net_token: Some("4f1d9c7a0b3e58126a9d0c4e7f2b6a83d15e90c7b4a2f6081e3d5c9a7b0f4e21".into()),
        caps: vec!["profile".into(), "library".into(), "input".into()],
        activity: vec![act(12, "input", "input 3 events", true), act(95, "library", "library", true), act(310, "launch", "launch steam:620 (switch is off)", false)],
        ..fixture_info()
    }
}

/// "42 minutes left" for the 1 hour timer.
pub fn left_text(at: u64, now: u64) -> String {
    let m = at.saturating_sub(now).div_ceil(60);
    if m <= 1 {
        "less than a minute left".into()
    } else {
        format!("{m} minutes left")
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub(super) fn small_button(cx: &mut Cx, nav: &mut Nav, parent: Id, label: &str, sid: &str, tag: u32) {
    let b = button(cx, parent, label, Variant::Secondary, Sz::Md, false).sid(sid).tag(tag);
    nav.add(b.group(11));
}

pub(super) fn qr_box(cx: &mut Cx, parent: Id, text: &str, rem: f32) {
    let r = cx.r();
    let px = (rem * r).round() as u32;
    let Some(png) = crate::screens::share::qr::png(text, px) else { return };
    let name: &'static str = Box::leak(format!("ssh-qr-{px}-{}.png", text.len()).into_boxed_str());
    let bytes: &'static [u8] = Box::leak(png.into_boxed_slice());
    let k = cx.img.request(ImgReq { src: Src::Static(name, bytes), w: px, h: px, fit: Fit::Contain });
    let frame = cx.scene.add(parent, s().wh(rem * r, rem * r).done(), Node::bg(rare_render::Color::WHITE).radius(0.4 * r).clip());
    cx.scene.add(frame, s().fill().done(), Node::default().image(k));
}

/// The SSH rows, built into Developer. Returns the keys listed (a Remove button's tag is its index).
pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, rows: &mut RowSet, info: &SshInfo, now_s: u64) -> Vec<SshKey> {
    let on = info.on();
    let st = info.status.clone().unwrap_or_default();
    let p = panel(cx, parent);
    let intro = if info.busy {
        "Working on it."
    } else if on {
        "Computers on your network can sign in to this device."
    } else {
        "Sign in to this device from a computer to install things, edit files and run tools. Off until you turn it on."
    };
    rows.switch(cx, nav, p, "ssh-toggle", "Remote access (SSH)", intro, on, true);
    setting_row(cx, p, "Before you turn it on", Some(WARNING), false);
    if !on {
        return vec![];
    }
    // Where to connect.
    let cmd = info.command();
    let addr = match &info.ip {
        Some(ip) => format!("{}.local or {ip}, user {}", info.host.trim_end_matches(".local"), info.user()),
        None => format!("{}.local, user {}", info.host.trim_end_matches(".local"), info.user()),
    };
    let row_ = setting_row(cx, p, &cmd, Some(&addr), false);
    small_button(cx, nav, row_.right, "Copy", "ssh-copy", 0);
    qr_box(cx, row_.right, &cmd, 9.0);
    if !st.active {
        setting_row(cx, p, "Starting", Some("The SSH server is starting. Check again in a moment."), false);
    }
    // Password.
    let pw_body = if st.default_password {
        "The password is still the default one from before Rare OS, so password sign-in is off. Set a new one to sign in with a password."
    } else if st.has_password {
        if st.password_auth { "A password is set. Password sign-in is on." } else { "A password is set. Password sign-in is off because a key is added." }
    } else {
        "No password yet. Set one, or add a key."
    };
    let row_ = setting_row(cx, p, "Password", Some(pw_body), false);
    small_button(cx, nav, row_.right, if st.default_password { "Set a new password" } else if st.has_password { "Change" } else { "Set a password" }, "ssh-password", 0);
    // Keys.
    let keys_body = if st.keys.is_empty() { "No keys yet. Keys are safer than a password: password sign-in turns off by itself once you add one.".to_string() } else { format!("{} key{}. Anyone with a matching private key can sign in.", st.keys.len(), if st.keys.len() == 1 { "" } else { "s" }) };
    let row_ = setting_row(cx, p, "Add a key", Some(&keys_body), false);
    small_button(cx, nav, row_.right, "Paste or type", "ssh-key-paste", 0);
    small_button(cx, nav, row_.right, "USB stick", "ssh-key-usb", 0);
    small_button(cx, nav, row_.right, "GitHub", "ssh-key-github", 0);
    for (i, k) in st.keys.iter().enumerate() {
        let title = if k.comment.is_empty() { format!("{} key", k.kind) } else { format!("{} ({})", k.comment, k.kind) };
        let row_ = setting_row(cx, p, &title, Some(&k.fingerprint), false);
        small_button(cx, nav, row_.right, "Remove", "ssh-remove", i as u32);
    }
    if !st.keys.is_empty() {
        rows.switch(cx, nav, p, "ssh-keep-password", "Keep password sign-in on", "Off by default once a key is added.", st.keep_password, false);
    }
    let timer = match st.auto_off_at {
        Some(at) => format!("SSH turns off by itself, {}.", left_text(at, now_secs().max(now_s))),
        None => "Switch SSH off again an hour from now.".into(),
    };
    rows.switch(cx, nav, p, "ssh-autooff", "Turn off after 1 hour", &timer, st.auto_off_at.is_some(), false);
    st.keys
}

/// The developer tools panel: where the docs are, and "Allow network apps".
pub fn build_tools(cx: &mut Cx, nav: &mut Nav, parent: Id, rows: &mut RowSet, info: &SshInfo) {
    let p = panel(cx, parent);
    setting_row(cx, p, "Developer tools", Some("Drive this device from your own tools and agents. The Local API and the SDK are at rare.market/developers."), true);
    let host = info.host.trim_end_matches(".local").to_string();
    let body = if info.netapi {
        format!("Tools on your network can call the Local API at http://{host}.local:{}. They need the token below.", ssh::NETAPI_PORT)
    } else {
        "Let tools and agents on a laptop call the Local API. Off by default.".to_string()
    };
    rows.switch(cx, nav, p, "netapi-toggle", "Allow network apps", &body, info.netapi, false);
    if info.netapi {
        if let Some(t) = &info.net_token {
            let row_ = setting_row(cx, p, "Token", Some(t), false);
            small_button(cx, nav, row_.right, "Copy", "netapi-copy", 0);
            small_button(cx, nav, row_.right, "New token", "netapi-newtoken", 0);
        }
        // One switch per capability, all off until the owner turns them on.
        let p = panel(cx, parent);
        setting_row(cx, p, "What network apps can do", Some("Each one is a separate switch. A request to buy or sell always opens a review sheet here, and nothing is signed until you press confirm on this device."), true);
        for (id, title, about) in rare_localapi::CAPS {
            rows.switch(cx, nav, p, &format!("netapi-cap-{id}"), title, about, info.caps.iter().any(|c| c == id), false);
        }
        let p = panel(cx, parent);
        setting_row(cx, p, "Recent network activity", Some("The latest calls from tools on your network, also kept in the log."), true);
        if info.activity.is_empty() {
            setting_row(cx, p, "Nothing yet", Some("Calls from network apps show up here."), false);
        }
        let now = now_secs() * 1000;
        for a in info.activity.iter().take(8) {
            let cap = rare_localapi::CAPS.iter().find(|c| c.0 == a.cap).map(|c| c.1).unwrap_or(a.cap.as_str());
            let body = format!("{cap} - from {} - {}", a.from, if a.ok { "allowed" } else { "refused" });
            setting_row(cx, p, &format!("{} - {}", ago_text(a.at_ms, now), a.what), Some(&body), false);
        }
    }
}

/// "just now", "5 min ago", "2 h ago".
pub fn ago_text(at_ms: u64, now_ms: u64) -> String {
    let s = now_ms.saturating_sub(at_ms) / 1000;
    match s {
        0..=9 => "just now".into(),
        10..=59 => format!("{s} s ago"),
        60..=3599 => format!("{} min ago", s / 60),
        _ => format!("{} h ago", s / 3600),
    }
}

/// The short entry in System.
pub fn build_system(cx: &mut Cx, nav: &mut Nav, parent: Id, rows: &mut RowSet, info: &SshInfo) {
    let p = panel(cx, parent);
    let body = if info.on() { format!("On. {}. Keys and passwords are in Developer.", info.command()) } else { "Off. Turn it on to sign in from a computer. More options in Developer.".to_string() };
    rows.switch(cx, nav, p, "ssh-system", "Remote access (SSH)", &body, info.on(), true);
}

/// A on a focused row of ours: the action for the app, or None when the row is not ours.
pub fn press(rows: &mut RowSet, keys: &[SshKey], cx: &mut Cx, nav: &mut Nav, f: &Focusable, now: f64) -> Option<super::SectionOut> {
    use super::SectionOut::Act;
    let sid = f.sid.as_str();
    let act = |a: &str| Some(Act(a.into()));
    if let Some(cap) = sid.strip_prefix("netapi-cap-") {
        let (_, v) = rows.press(cx, nav, now)?;
        return act(&format!("netapi:cap:{cap}:{}", matches!(v, Val::Bool(true))));
    }
    match sid {
        "ssh-toggle" | "ssh-system" | "ssh-keep-password" | "ssh-autooff" | "netapi-toggle" => {
            let (_, v) = rows.press(cx, nav, now)?;
            let on = matches!(v, Val::Bool(true));
            let a = match sid {
                "ssh-toggle" | "ssh-system" => "ssh:toggle",
                "ssh-keep-password" => "ssh:keep-password",
                "ssh-autooff" => "ssh:auto-off",
                _ => "netapi:toggle",
            };
            act(&format!("{a}:{on}"))
        }
        "ssh-copy" => act("ssh:copy"),
        "ssh-password" => act("ssh:ask-password"),
        "ssh-key-paste" => act("ssh:ask-key"),
        "ssh-key-usb" => act("ssh:usb"),
        "ssh-key-github" => act("ssh:ask-github"),
        "ssh-remove" => keys.get(f.tag as usize).and_then(|k| act(&format!("ssh:remove-ask:{}", k.fingerprint))),
        "netapi-copy" => act("netapi:copy"),
        "netapi-newtoken" => act("netapi:newtoken"),
        _ => None,
    }
}

/// Words for the OSK requests.
pub mod ask {
    pub const PASSWORD_TITLE: &str = "Password for remote sign-in";
    pub const KEY_TITLE: &str = "Public key (starts with ssh-ed25519 or ssh-rsa)";
    pub const GITHUB_TITLE: &str = "GitHub user name";
}

/// The confirm sheet for a pending key list: (title, sub, body).
pub fn github_confirm(user: &str, n: usize) -> (String, String, String) {
    (
        format!("Add {n} key{} from GitHub?", if n == 1 { "" } else { "s" }),
        format!("github.com/{user}"),
        format!("Anyone who controls the GitHub account {user} will be able to sign in to this device. Only continue if that is you."),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_and_state() {
        let i = fixture_on();
        assert!(i.on());
        assert_eq!(i.command(), "ssh bazzite@suiplay.local");
        assert!(!fixture_info().on());
        assert!(!SshInfo::default().on());
    }

    #[test]
    fn ago_words() {
        assert_eq!(ago_text(1_000_000, 1_003_000), "just now");
        assert_eq!(ago_text(1_000_000, 1_030_000), "30 s ago");
        assert_eq!(ago_text(1_000_000, 1_300_000), "5 min ago");
        assert_eq!(ago_text(0, 7_300_000), "2 h ago");
    }

    #[test]
    fn every_capability_has_a_plain_line() {
        assert_eq!(rare_localapi::CAPS.len(), 8);
        for (_, title, about) in rare_localapi::CAPS {
            assert!(!title.is_empty() && about.ends_with('.') && !about.contains('\u{2014}'));
        }
    }

    #[test]
    fn timer_words() {
        assert_eq!(left_text(1000 + 2520, 1000), "42 minutes left");
        assert_eq!(left_text(1000, 1000), "less than a minute left");
    }

    #[test]
    fn github_words() {
        let (t, sub, body) = github_confirm("octocat", 2);
        assert_eq!(t, "Add 2 keys from GitHub?");
        assert_eq!(sub, "github.com/octocat");
        assert!(body.contains("octocat"));
        assert!(github_confirm("a", 1).0.contains("1 key from"));
    }
}

