//! The About and Credits sections (AccountsStorage.tsx AboutSection, Settings.tsx Licences).

use super::{config::Config, rows::*, SectionOut, SectionView};
use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::theme::tok;
use rare_render::{Id, Node};

/// screens/credits.ts
pub const CREDITS: [(&str, &str, &str); 30] = [
    ("Bazzite and Universal Blue", "Apache-2.0", "Base system image, handheld support and updates"),
    ("Fedora Linux", "Various free licences", "Packages underneath the base image"),
    ("Linux kernel", "GPL-2.0", "Kernel"),
    ("gamescope", "BSD-2-Clause", "Game mode compositor"),
    ("gamescope-session-plus", "MIT", "Game mode session"),
    ("InputPlumber", "GPL-3.0", "Controller and button mapping (separate service)"),
    ("PowerStation", "GPL-3.0", "Power limit control over D-Bus (separate service)"),
    ("lsfg-vk", "CC BY-NC-ND 4.0", "Frame generation layer, unmodified (uses Lossless Scaling from your own Steam library)"),
    ("vkBasalt", "Zlib", "Sharpening and colour filters (Vulkan layer)"),
    ("MangoHud", "MIT", "Performance overlay (mangoapp)"),
    ("SuiPlay0X1 hardware quirks", "MIT", "Button and serial-port rules from the SuiPlay0X1's original OS"),
    ("Tauri", "MIT / Apache-2.0", "Launcher app framework"),
    ("WebKitGTK", "LGPL-2.1 / BSD", "Launcher rendering on Linux"),
    ("React", "MIT", "Launcher interface"),
    ("Tailwind CSS", "MIT", "Launcher styles"),
    ("gilrs", "MIT / Apache-2.0", "Controller input"),
    ("Mysten Labs Sui TypeScript SDK", "Apache-2.0", "Rare Wallet"),
    ("noble / scure cryptography", "MIT", "Rare Wallet keys"),
    ("legendary", "GPL-3.0", "Epic Games launching (separate program)"),
    ("heroic-gogdl / Heroic data", "GPL-3.0", "GOG installs (read from disk)"),
    ("umu-launcher", "GPL-3.0", "Running Windows games with Proton (separate program)"),
    ("Moonlight", "GPL-3.0", "Streaming games from your PC (separate program, installed from Flathub)"),
    ("mdns-sd", "MIT / Apache-2.0", "Finding gaming PCs and Rare devices on your network"),
    ("gpu-screen-recorder", "GPL-3.0", "Instant replay (separate program)"),
    ("Sunshine", "GPL-3.0", "Streaming this device to Moonlight (separate program)"),
    ("arRPC", "MIT", "Discord Rich Presence for Discord on the web (separate program)"),
    ("Node.js", "MIT", "Runs arRPC"),
    ("AreWeAntiCheatYet", "MIT", "Anti-cheat status of Steam games on Linux"),
    ("Inter", "SIL OFL 1.1", "Interface font"),
    ("Fira Code", "SIL OFL 1.1", "Number font"),
];

/// The Credits and licences section: a sentence and one panel of focusable credit rows.
/// The licence's text on the web (the first licence of a pair), for A on a Credits row.
pub fn licence_url(licence: &str) -> String {
    let first = licence.split(" / ").next().unwrap_or(licence).trim();
    let spdx = match first {
        "GPL-2.0" => "GPL-2.0-only",
        "GPL-3.0" => "GPL-3.0-only",
        "LGPL-2.1" => "LGPL-2.1-only",
        "CC BY-NC-ND 4.0" => "CC-BY-NC-ND-4.0",
        "SIL OFL 1.1" => "OFL-1.1",
        "Various free licences" => return "https://docs.fedoraproject.org/en-US/legal/allowed-licenses/".into(),
        other => other,
    };
    format!("https://spdx.org/licenses/{spdx}.html")
}

/// A on Credits row `i`: its licence text.
pub struct LicencesView;
impl super::SectionView for LicencesView {
    fn press(&mut self, _cx: &mut Cx, nav: &mut Nav, _cfg: &mut super::config::Config, _now: f64) -> super::SectionOut {
        let Some(i) = nav.current().and_then(|f| f.sid.strip_prefix("licence:")).and_then(|v| v.parse::<usize>().ok()) else { return super::SectionOut::None };
        match CREDITS.get(i) {
            Some((_, licence, _)) => super::SectionOut::Act(format!("open-web:{}", licence_url(licence))),
            None => super::SectionOut::None,
        }
    }
}

pub fn build_licences(cx: &mut Cx, nav: &mut Nav, parent: Id) {
    let r = cx.r();
    let col = cx.scene.add(parent, s().col().gap(0.75 * r).done(), Node::default());
    cx.scene.add_text(cx.t, col, s().done(), "Rare OS is built from our own code and these open-source projects. Thank you to everyone who made them.", ts(r, ty::SMALL.0, ty::SMALL.1, 400).wrap(), tok::MUTED);
    let p = super::rows::panel(cx, col);
    for (i, (name, licence, usage)) in CREDITS.iter().enumerate() {
        let row_ = cx.scene.add(p, s().col().wp(1.0).px(1.4 * r).py(0.85 * r).done(), Node::default());
        if i > 0 {
            edge(cx, row_, 0, tok::HAIR);
        }
        let line = cx.scene.add(row_, s().row().items_end().gap(0.6 * r).done(), Node::default());
        text(cx, line, ty::BODY, 600, tok::FG, name);
        text(cx, line, ty::META, 400, tok::MUTED, licence);
        text(cx, row_, ty::SMALL, 400, tok::MUTED, usage);
        nav.add(Focusable { id: row_, kind: Kind::Row, group: 10, hint: "Read the licence".into(), tag: i as u32, sid: format!("licence:{i}") });
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct DeviceInfo {
    pub name: String,
    pub os_name: String,
    pub os_version: String,
    pub hostname: String,
    pub under_gamescope: bool,
    /// The serial number (DMI), when readable.
    pub serial: Option<String>,
    /// BIOS version and date, "2.19_S20 (02/19/2025)".
    pub bios: Option<String>,
}

pub fn fixture_device() -> DeviceInfo {
    DeviceInfo { name: "SuiPlay0X1".into(), os_name: "Rare OS".into(), os_version: "43.1".into(), hostname: "rare".into(), under_gamescope: true, serial: None, bios: None }
}

/// This launcher's build: the package version and the commit it was built from (RARE_BUILD, set by
/// the build machine; GITHUB_SHA in CI), with the binary's date.
pub fn launcher_build() -> String {
    let commit = option_env!("RARE_BUILD").filter(|c| !c.is_empty()).map(String::from).or_else(|| option_env!("GITHUB_SHA").map(|c| c.chars().take(7).collect())).filter(|c| !c.is_empty());
    let date = std::env::current_exe().ok().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| ymd(d.as_secs() as i64));
    let mut out = env!("CARGO_PKG_VERSION").to_string();
    if let Some(c) = commit {
        out.push_str(&format!(" ({c})"));
    }
    if let Some(d) = date {
        out.push_str(&format!(", {d}"));
    }
    out
}

/// "2026-10-27" for unix seconds (UTC).
fn ymd(secs: i64) -> String {
    let z = secs.div_euclid(86400) + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

static MARK: &[u8] = include_bytes!("../../../../render/assets/brand/mark.png");

pub struct AboutSection {
    credits_btn: Id,
    pass_btn: Option<Id>,
}

/// The Rare Device Pass panel: the pass this account holds, or the one Claim button (the claim
/// itself reviews in Rare Wallet's sheet). Shown once the pass is published on the wallet's network.
fn device_pass(cx: &mut Cx, nav: &mut Nav, parent: Id) -> Option<Id> {
    use crate::features::devicepass::{self as dp, Phase};
    let v = dp::view();
    if !v.shown {
        return None;
    }
    dp::load(false);
    let r = cx.r();
    let p = cx.scene.add(parent, s().col().items_start().gap(0.9 * r).wp(1.0).px(1.6 * r).py(1.4 * r).done(), Node::bg(tok::L1).radius(1.0 * r));
    text(cx, p, ty::TITLE, 600, tok::FG, "Rare Pass");
    if let Some(pass) = &v.pass {
        let number = format!("#{}", pass.number);
        let seen = dp::date(pass.first_seen_ms);
        stat_line_sized(cx, p, &[("Pass", &number), ("Device", &pass.model), ("First seen", &seen)], true);
        let r_ = cx.r();
        cx.scene.add_text(cx.t, p, s().max_w(40.0 * r_).done(), "Kept by this account. It stays with this device: if the device changes hands, its new owner can claim it.", ts(r_, ty::SMALL.0, ty::SMALL.1, 400).wrap(), tok::MUTED);
        return None;
    }
    let body = if v.stale {
        "This device's Rare Pass moved to another account. Claim it again to bring it back here."
    } else {
        "One per device. Rare checks this SuiPlay's security chip, then adds a Rare Pass to your account that shows you own a genuine SuiPlay."
    };
    let r_ = cx.r();
    cx.scene.add_text(cx.t, p, s().max_w(40.0 * r_).done(), body, ts(r_, ty::BODY.0, ty::BODY.0 * 1.625, 400).wrap(), tok::FG.alpha(0.8));
    let busy = matches!(v.phase, Phase::Checking | Phase::Signing);
    let label = match v.phase {
        Phase::Checking => "Checking this device",
        Phase::Signing => "Waiting for your approval",
        _ => "Claim your Rare Pass",
    };
    let b = button(cx, p, label, if busy { Variant::Dimmed } else { Variant::Primary }, Sz::Md, false).sid("device-pass").hint("Claim").tag(2);
    let id = nav.add(b.group(10));
    let note = match &v.phase {
        Phase::Error(e) => Some((e.as_str(), tok::NEG)),
        _ if !v.can_sign => Some((crate::sign::NO_SIGNER, tok::MUTED)),
        _ => None,
    };
    if let Some((t, color)) = note {
        let r_ = cx.r();
        cx.scene.add_text(cx.t, p, s().max_w(40.0 * r_).done(), t, ts(r_, ty::SMALL.0, ty::SMALL.1, 400).wrap(), color);
    }
    Some(id)
}

impl AboutSection {
    pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, cfg: &Config, now: f64) -> AboutSection {
        let d = super::live::get().map(|s| s.device).unwrap_or_else(fixture_device);
        AboutSection::build_with(cx, nav, parent, cfg, &d, now)
    }

    pub fn build_with(cx: &mut Cx, nav: &mut Nav, parent: Id, _cfg: &Config, d: &DeviceInfo, _now: f64) -> AboutSection {
        let r = cx.r();
        let p0 = cx.scene.add(parent, s().col().wp(1.0).px(1.4 * r).py(1.2 * r).done(), Node::bg(tok::L1).radius(1.0 * r));
        let system = format!("{} {}", d.os_name, d.os_version);
        stat_line_sized(cx, p0, &[("Device", &d.name), ("System", system.trim()), ("Hostname", &d.hostname), ("Display", if d.under_gamescope { "Game mode" } else { "Window" })], true);
        // Support details: serial, BIOS and this launcher's build, each when known.
        let build = launcher_build();
        let mut more: Vec<(&str, &str)> = Vec::new();
        if let Some(s) = d.serial.as_deref().filter(|s| !s.is_empty()) {
            more.push(("Serial number", s));
        }
        if let Some(b) = d.bios.as_deref().filter(|b| !b.is_empty()) {
            more.push(("BIOS", b));
        }
        more.push(("Launcher", &build));
        let p_more = cx.scene.add(parent, s().col().wp(1.0).px(1.4 * r).py(1.2 * r).done(), Node::bg(tok::L1).radius(1.0 * r));
        stat_line_sized(cx, p_more, &more, false);
        let pass_btn = device_pass(cx, nav, parent);
        let p1 = cx.scene.add(parent, s().col().items_start().gap(1.0 * r).wp(1.0).px(1.6 * r).py(1.5 * r).done(), Node::bg(tok::L1).radius(1.0 * r));
        let head = cx.scene.add(p1, s().row().items_center().gap(0.9 * r).done(), Node::default());
        let k = cx.img.request(rare_render::ImgReq { src: rare_render::Src::Static("brand/mark.png", MARK), w: (3.0 * r) as u32, h: (3.0 * r) as u32, fit: rare_render::Fit::Contain });
        cx.scene.add(head, s().wh(3.0 * r, 3.0 * r).done(), Node::default().image(k).contain([0.5, 0.5]));
        let col = cx.scene.add(head, s().col().done(), Node::default());
        text(cx, col, ty::TITLE, 600, tok::FG, "Rare OS");
        let v = d.os_version.as_str();
        let r_ = cx.r();
        let mut mono = ts(r_, ty::META.0, ty::META.1, 400);
        mono.mono = true;
        cx.scene.add_text(cx.t, col, s().done(), v, mono, tok::MUTED);
        let b = button(cx, p1, "Credits and licences", Variant::Secondary, Sz::Md, false).sid("licences").tag(1);
        let credits_btn = nav.add(b.group(10));
        AboutSection { credits_btn, pass_btn }
    }
}

impl SectionView for AboutSection {
    fn press(&mut self, _cx: &mut Cx, nav: &mut Nav, _cfg: &mut Config, _now: f64) -> SectionOut {
        if nav.cur == Some(self.credits_btn) {
            SectionOut::Act("settings:goto:licences".into())
        } else if self.pass_btn.is_some() && nav.cur == self.pass_btn {
            crate::features::devicepass::claim();
            SectionOut::None
        } else {
            SectionOut::None
        }
    }
}
