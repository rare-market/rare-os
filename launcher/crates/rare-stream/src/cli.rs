//! Moonlight-Qt's command line, built and read here (pure, tested). Rare never links Moonlight:
//! it runs `moonlight pair | list | stream | quit` as a separate program (the Flathub flatpak).
//!
//! Options are the ones in Moonlight-Qt 6.x `app/cli/commandlineparser.cpp`.

use serde::{Deserialize, Serialize};

pub const FLATPAK: &str = "com.moonlight_stream.Moonlight";

/// How Moonlight is installed on this device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Moonlight {
    /// The Flathub app (what Rare installs).
    Flatpak,
    /// A `moonlight` or `moonlight-qt` program on PATH.
    Native(String),
}

impl Moonlight {
    /// Program, arguments and extra environment for one Moonlight action. `headless` runs it on
    /// Qt's offscreen platform, so pair, list and quit never open a window over Rare.
    pub fn command(&self, action: &[String], headless: bool) -> (String, Vec<String>, Vec<(String, String)>) {
        let env: Vec<(String, String)> = if headless {
            vec![("QT_QPA_PLATFORM".into(), "offscreen".into()), ("QT_QUICK_BACKEND".into(), "software".into())]
        } else {
            vec![]
        };
        match self {
            Moonlight::Flatpak => {
                let mut args = vec!["run".to_string()];
                // flatpak run doesn't pass the caller's environment through; --env does.
                args.extend(env.iter().map(|(k, v)| format!("--env={k}={v}")));
                args.push(FLATPAK.into());
                args.extend(action.iter().cloned());
                ("flatpak".into(), args, vec![])
            }
            Moonlight::Native(p) => (p.clone(), action.to_vec(), env),
        }
    }
}

pub fn pair_args(host: &str, pin: &str) -> Vec<String> {
    vec!["pair".into(), host.into(), "--pin".into(), pin.into()]
}

pub fn list_args(host: &str) -> Vec<String> {
    vec!["list".into(), host.into(), "--csv".into()]
}

pub fn quit_args(host: &str) -> Vec<String> {
    vec!["quit".into(), host.into()]
}

// ---------- stream settings ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Codec {
    /// The best both sides support (see `pick_codec`).
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "av1")]
    Av1,
    #[serde(rename = "hevc")]
    Hevc,
    #[serde(rename = "h264")]
    H264,
}

/// The stream settings Rare offers, tuned for the SuiPlay0X1's 1920 x 1200 panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// "1920x1200" (the panel) or "1280x800" (half the pixels: less Wi-Fi, less decode).
    pub resolution: String,
    pub fps: u32,
    pub bitrate_mbps: u32,
    pub codec: Codec,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { resolution: "1920x1200".into(), fps: 60, bitrate_mbps: 20, codec: Codec::Auto }
    }
}

pub const RESOLUTIONS: [&str; 2] = ["1920x1200", "1280x800"];

impl Settings {
    /// Settings from the UI or an old file, held to what Rare offers.
    pub fn sane(mut self) -> Self {
        if !RESOLUTIONS.contains(&self.resolution.as_str()) {
            self.resolution = RESOLUTIONS[0].into();
        }
        if !(30..=120).contains(&self.fps) {
            self.fps = 60;
        }
        self.bitrate_mbps = self.bitrate_mbps.clamp(5, 150);
        self
    }
}

/// What this device's GPU decodes in hardware (VA-API, read from `vainfo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Decode {
    pub av1: bool,
    pub hevc: bool,
    pub h264: bool,
}

/// Reads `vainfo`: a codec counts when its profile has the VLD (decode) entrypoint.
pub fn parse_vainfo(text: &str) -> Option<Decode> {
    let mut d = Decode::default();
    let mut any = false;
    for line in text.lines() {
        let l = line.trim();
        if !l.contains("VAEntrypointVLD") {
            continue;
        }
        any = true;
        if l.starts_with("VAProfileAV1Profile0") {
            d.av1 = true;
        } else if l.starts_with("VAProfileHEVCMain") {
            d.hevc = true;
        } else if l.starts_with("VAProfileH264") {
            d.h264 = true;
        }
    }
    any.then_some(d)
}

// ServerCodecModeSupport bits (moonlight-common-c Limelight.h).
pub const SCM_H264: u32 = 0x0000_0001;
pub const SCM_HEVC: u32 = 0x0000_0100;
pub const SCM_AV1_MAIN8: u32 = 0x0001_0000;
pub const SCM_AV1_MAIN10: u32 = 0x0002_0000;

/// The `--video-codec` value. Auto picks AV1, then HEVC, then H.264, the first both this device
/// decodes and the PC encodes; when either side is unknown, Moonlight decides ("auto").
pub fn pick_codec(pref: Codec, device: Option<Decode>, host: Option<u32>) -> &'static str {
    match pref {
        Codec::Av1 => "AV1",
        Codec::Hevc => "HEVC",
        Codec::H264 => "H.264",
        Codec::Auto => {
            let (Some(d), Some(h)) = (device, host) else { return "auto" };
            if d.av1 && h & (SCM_AV1_MAIN8 | SCM_AV1_MAIN10) != 0 {
                "AV1"
            } else if d.hevc && h & SCM_HEVC != 0 {
                "HEVC"
            } else if h & SCM_H264 != 0 || h == 0 {
                "H.264"
            } else {
                "auto"
            }
        }
    }
}

/// `moonlight stream <host> <app> ...` for the SuiPlay:
/// - borderless: gamescope scales it; no display mode change at 1280 x 800
/// - no quit-after: the game keeps running on the PC if Wi-Fi drops, so Play resumes it
/// - no background gamepad: once Guide brings Rare back, the pad drives Rare, not the PC
/// - no swapped buttons: the SuiPlay's pad is Xbox layout, which SDL already maps right
pub fn stream_args(host: &str, app: &str, s: &Settings, codec: &str) -> Vec<String> {
    let s = s.clone().sane();
    let mut a: Vec<String> = vec!["stream".into(), host.into(), app.into()];
    for (k, v) in [
        ("--resolution", s.resolution.clone()),
        ("--fps", s.fps.to_string()),
        ("--bitrate", (s.bitrate_mbps * 1000).to_string()),
        ("--video-codec", codec.to_string()),
        ("--display-mode", "borderless".into()),
        ("--audio-config", "stereo".into()),
    ] {
        a.push(k.into());
        a.push(v);
    }
    a.extend(["--no-quit-after", "--no-background-gamepad", "--no-swap-gamepad-buttons", "--keep-awake"].map(String::from));
    a
}

// ---------- reading `moonlight list <host> --csv` ----------

/// One line of `moonlight list --csv`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub name: String,
    pub id: u32,
    pub hdr: bool,
    pub hidden: bool,
    /// Moonlight's cached box art (a local file), when it has it.
    pub boxart: Option<String>,
}

/// Moonlight prints `"Name",ID,HDR,Collector,Hidden,Direct,"Boxart URL"` without escaping quotes
/// in names, so each line is read from the right.
pub fn parse_list_csv(text: &str) -> Vec<Listed> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with("Name,") {
            continue;
        }
        let Some(url_at) = line.rfind(",\"") else { continue };
        let url = line[url_at + 2..].trim_end_matches('"');
        let mut rest = line[..url_at].rsplitn(6, ',');
        let (Some(_direct), Some(hidden), Some(_collector), Some(hdr), Some(id), Some(name)) = (rest.next(), rest.next(), rest.next(), rest.next(), rest.next(), rest.next()) else {
            continue;
        };
        let Ok(id) = id.trim().parse::<u32>() else { continue };
        let name = name.strip_prefix('"').and_then(|n| n.strip_suffix('"')).unwrap_or(name).to_string();
        out.push(Listed { name, id, hdr: hdr.trim() == "true", hidden: hidden.trim() == "true", boxart: file_url_path(url) });
    }
    out
}

/// `file:///home/x/a%20b.png` -> `/home/x/a b.png`; anything else (Moonlight's built-in
/// placeholder `qrc:/res/no_app_image.png`) is None.
pub fn file_url_path(url: &str) -> Option<String> {
    let p = url.strip_prefix("file://")?;
    let bytes = p.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(b) = p.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok().filter(|s| s.starts_with('/'))
}

/// A 4 digit pairing PIN.
pub fn pin_from(n: u16) -> String {
    format!("{:04}", n % 10_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn flatpak_and_native_commands() {
        let (p, a, e) = Moonlight::Flatpak.command(&list_args("192.168.0.20"), true);
        assert_eq!(p, "flatpak");
        assert_eq!(a, s(&["run", "--env=QT_QPA_PLATFORM=offscreen", "--env=QT_QUICK_BACKEND=software", FLATPAK, "list", "192.168.0.20", "--csv"]));
        assert!(e.is_empty());
        let (p, a, e) = Moonlight::Flatpak.command(&s(&["stream", "pc", "Desktop"]), false);
        assert_eq!((p.as_str(), a), ("flatpak", s(&["run", FLATPAK, "stream", "pc", "Desktop"])));
        assert!(e.is_empty());
        let (p, a, e) = Moonlight::Native("moonlight".into()).command(&pair_args("pc", "0420"), true);
        assert_eq!(p, "moonlight");
        assert_eq!(a, s(&["pair", "pc", "--pin", "0420"]));
        assert_eq!(e[0], ("QT_QPA_PLATFORM".to_string(), "offscreen".to_string()));
        assert_eq!(quit_args("pc"), s(&["quit", "pc"]));
    }

    #[test]
    fn stream_command_for_the_suiplay() {
        let a = stream_args("192.168.0.20", "Cyberpunk 2077", &Settings::default(), "AV1");
        assert_eq!(
            a,
            s(&[
                "stream",
                "192.168.0.20",
                "Cyberpunk 2077",
                "--resolution",
                "1920x1200",
                "--fps",
                "60",
                "--bitrate",
                "20000",
                "--video-codec",
                "AV1",
                "--display-mode",
                "borderless",
                "--audio-config",
                "stereo",
                "--no-quit-after",
                "--no-background-gamepad",
                "--no-swap-gamepad-buttons",
                "--keep-awake",
            ])
        );
        let light = Settings { resolution: "1280x800".into(), fps: 60, bitrate_mbps: 10, codec: Codec::Hevc };
        let a = stream_args("pc", "Desktop", &light, "HEVC");
        assert!(a.windows(2).any(|w| w == ["--resolution", "1280x800"]));
        assert!(a.windows(2).any(|w| w == ["--bitrate", "10000"]));
        // Anything else is held to what Rare offers.
        let odd = Settings { resolution: "640x480".into(), fps: 1000, bitrate_mbps: 900, codec: Codec::Auto };
        let a = stream_args("pc", "Desktop", &odd, "auto");
        assert!(a.windows(2).any(|w| w == ["--resolution", "1920x1200"]));
        assert!(a.windows(2).any(|w| w == ["--fps", "60"]));
        assert!(a.windows(2).any(|w| w == ["--bitrate", "150000"]));
    }

    #[test]
    fn codec_follows_both_ends() {
        let all = Decode { av1: true, hevc: true, h264: true };
        let rtx40 = SCM_H264 | SCM_HEVC | SCM_AV1_MAIN8 | SCM_AV1_MAIN10;
        let rtx30 = SCM_H264 | SCM_HEVC;
        assert_eq!(pick_codec(Codec::Auto, Some(all), Some(rtx40)), "AV1");
        assert_eq!(pick_codec(Codec::Auto, Some(all), Some(rtx30)), "HEVC");
        assert_eq!(pick_codec(Codec::Auto, Some(Decode { h264: true, ..Default::default() }), Some(rtx40)), "H.264");
        assert_eq!(pick_codec(Codec::Auto, None, Some(rtx40)), "auto");
        assert_eq!(pick_codec(Codec::Auto, Some(all), None), "auto");
        assert_eq!(pick_codec(Codec::Hevc, None, None), "HEVC");
        assert_eq!(pick_codec(Codec::Av1, Some(all), Some(rtx30)), "AV1");
    }

    #[test]
    fn reads_vainfo_on_the_780m() {
        let text = "vainfo: Driver version: Mesa Gallium driver 25.1.4 for AMD Radeon 780M Graphics\n\
            vainfo: Supported profile and entrypoints\n\
            \x20     VAProfileH264ConstrainedBaseline: VAEntrypointVLD\n\
            \x20     VAProfileH264Main               : VAEntrypointEncSlice\n\
            \x20     VAProfileHEVCMain               : VAEntrypointVLD\n\
            \x20     VAProfileHEVCMain10             : VAEntrypointVLD\n\
            \x20     VAProfileAV1Profile0            : VAEntrypointVLD\n\
            \x20     VAProfileAV1Profile0            : VAEntrypointEncSlice\n";
        assert_eq!(parse_vainfo(text), Some(Decode { av1: true, hevc: true, h264: true }));
        let old = "      VAProfileH264Main : VAEntrypointVLD\n      VAProfileHEVCMain : VAEntrypointEncSlice\n";
        assert_eq!(parse_vainfo(old), Some(Decode { av1: false, hevc: false, h264: true }));
        assert_eq!(parse_vainfo("vaInitialize failed with error code -1 (unknown libva error),exit\n"), None);
    }

    #[test]
    fn reads_moonlight_list_csv() {
        let text = "Name, ID, HDR Support, App Collection Game, Hidden, Direct Launch, Boxart URL\n\
            \"Desktop\",881448767,false,false,false,false,\"qrc:/res/no_app_image.png\"\n\
            \"Steam Big Picture\",1093255277,false,false,false,false,\"file:///home/bazzite/.var/app/com.moonlight_stream.Moonlight/cache/Moonlight%20Game%20Streaming%20Project/Moonlight/boxart/ABC/1093255277.png\"\n\
            \"Baldur's Gate 3, \"Deluxe\"\",2093255277,true,false,false,false,\"file:///b/2093255277.png\"\r\n\
            \"Old\",5,false,false,true,false,\"qrc:/res/no_app_image.png\"\n\
            garbage line\n";
        let apps = parse_list_csv(text);
        assert_eq!(apps.len(), 4);
        assert_eq!(apps[0], Listed { name: "Desktop".into(), id: 881448767, hdr: false, hidden: false, boxart: None });
        assert_eq!(apps[1].boxart.as_deref(), Some("/home/bazzite/.var/app/com.moonlight_stream.Moonlight/cache/Moonlight Game Streaming Project/Moonlight/boxart/ABC/1093255277.png"));
        assert_eq!(apps[2].name, "Baldur's Gate 3, \"Deluxe\"");
        assert!(apps[2].hdr);
        assert!(apps[3].hidden);
    }

    #[test]
    fn pins_are_four_digits() {
        assert_eq!(pin_from(7), "0007");
        assert_eq!(pin_from(65535), "5535");
        assert_eq!(file_url_path("qrc:/x.png"), None);
        assert_eq!(file_url_path("file:///a%2"), Some("/a%2".into()));
    }
}
