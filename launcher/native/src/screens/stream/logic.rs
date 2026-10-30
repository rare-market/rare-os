//! PC streaming: the pure parts (stream/logic.ts, stream/types.ts). Which PC shows on Home, what a
//! tile says, what Play does, and the quality presets for the SuiPlay. Tests mirror logic.test.ts.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Codec {
    Auto,
    Av1,
    Hevc,
    H264,
}

#[derive(Clone, PartialEq, Debug)]
pub struct StreamSettings {
    /// "1920x1200" (the SuiPlay's panel) or "1280x800".
    pub resolution: String,
    pub fps: u32,
    pub bitrate_mbps: u32,
    pub codec: Codec,
}

/// Hardware video decode on this device (VA-API).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Decode {
    pub av1: bool,
    pub hevc: bool,
    pub h264: bool,
}

/// A gaming PC (native `HostView`).
#[derive(Clone, PartialEq, Debug)]
pub struct Host {
    pub uuid: String,
    pub name: String,
    pub address: Option<String>,
    /// Moonlight holds this PC's certificate.
    pub paired: bool,
    /// Answered just now.
    pub online: bool,
    /// The app running on the PC.
    pub current_game: Option<u32>,
    /// Wake-on-LAN possible (a MAC address is known).
    pub can_wake: bool,
    /// "Sunshine" or "GeForce Experience".
    pub software: Option<String>,
    /// ServerCodecModeSupport bits.
    pub codecs: Option<u32>,
}

/// A game or app on a PC. `art` is Rare's cached box art.
#[derive(Clone, PartialEq, Debug)]
pub struct StreamApp {
    pub id: u32,
    pub name: String,
    pub art: Option<rare_render::Src>,
    pub hdr: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Offer {
    pub flatpak: String,
    pub name: String,
    pub command: String,
}

#[derive(Clone, Debug)]
pub struct Status {
    /// "flatpak" or "native"; None when Moonlight isn't installed.
    pub moonlight: Option<String>,
    pub offer: Option<Offer>,
    pub decode: Option<Decode>,
    pub settings: StreamSettings,
    pub hosts: Vec<Host>,
    /// Host uuid -> its apps.
    pub apps: Vec<(String, Vec<StreamApp>)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PairPhase {
    Waiting,
    Paired,
    Failed,
}

#[derive(Clone, PartialEq, Debug)]
pub struct PairView {
    pub uuid: String,
    pub name: String,
    pub address: String,
    pub pin: String,
    pub phase: PairPhase,
    pub error: Option<String>,
    pub software: String,
}

/// Streams run as "stream:<host uuid>:<app id>" in the launcher's running list.
pub fn run_id(uuid: &str, app: u32) -> String {
    format!("stream:{uuid}:{app}")
}

pub fn parse_run_id(rid: &str) -> Option<(String, u32)> {
    let rest = rid.strip_prefix("stream:")?;
    let i = rest.rfind(':')?;
    let app: u32 = rest[i + 1..].parse().ok()?;
    if i == 0 {
        return None;
    }
    Some((rest[..i].to_string(), app))
}

/// The PC Home's "On your PC" row shows: the first paired one that answers.
pub fn home_host(hosts: &[Host]) -> Option<&Host> {
    hosts.iter().find(|h| h.paired && h.online)
}

/// Every game on every paired PC, for Library > Streaming.
pub fn pc_games<'a>(hosts: &'a [Host], apps: &'a [(String, Vec<StreamApp>)]) -> Vec<(&'a Host, &'a StreamApp)> {
    hosts
        .iter()
        .filter(|h| h.paired)
        .flat_map(|h| apps.iter().filter(move |(u, _)| *u == h.uuid).flat_map(move |(_, l)| l.iter().map(move |a| (h, a))))
        .collect()
}

/// Games with box art first (Sunshine lists Desktop and Steam Big Picture first), order kept otherwise.
pub fn ordered(apps: &[StreamApp]) -> Vec<StreamApp> {
    apps.iter().filter(|a| a.art.is_some()).chain(apps.iter().filter(|a| a.art.is_none())).cloned().collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlayState {
    /// Streaming to this SuiPlay now.
    Playing,
    /// Running on the PC with no stream: Play picks it back up.
    OnPc,
    /// The PC runs a different game: Play quits it first (asked).
    Busy,
    Ready,
    /// Not answering, but Wake-on-LAN can start it.
    Asleep,
    Offline,
}

pub fn play_state(host: &Host, app: &StreamApp, running: &[String]) -> PlayState {
    if running.contains(&run_id(&host.uuid, app.id)) {
        return PlayState::Playing;
    }
    if !host.online {
        return if host.can_wake { PlayState::Asleep } else { PlayState::Offline };
    }
    if host.current_game == Some(app.id) {
        return PlayState::OnPc;
    }
    if host.current_game.is_some() {
        return PlayState::Busy;
    }
    PlayState::Ready
}

/// The second line under a cover.
pub fn tile_line(host: &Host, state: PlayState) -> String {
    match state {
        PlayState::Playing => "Playing".into(),
        PlayState::OnPc => "Running on your PC".into(),
        PlayState::Asleep => format!("{} is asleep", host.name),
        PlayState::Offline => format!("{} is off", host.name),
        _ => host.name.clone(),
    }
}

// ServerCodecModeSupport bits (moonlight-common-c).
const SCM_H264: u32 = 0x1;
const SCM_HEVC: u32 = 0x100;
const SCM_AV1: u32 = 0x10000 | 0x20000;

/// The codec Auto lands on (same rule as the native `pick_codec`); None: Moonlight decides.
pub fn auto_codec(decode: Option<Decode>, host_codecs: Option<u32>) -> Option<Codec> {
    let (decode, host) = (decode?, host_codecs?);
    if decode.av1 && host & SCM_AV1 != 0 {
        return Some(Codec::Av1);
    }
    if decode.hevc && host & SCM_HEVC != 0 {
        return Some(Codec::Hevc);
    }
    if host & SCM_H264 != 0 || host == 0 {
        return Some(Codec::H264);
    }
    None
}

pub fn codec_label(c: Codec) -> &'static str {
    match c {
        Codec::Auto => "Auto",
        Codec::Av1 => "AV1",
        Codec::Hevc => "HEVC",
        Codec::H264 => "H.264",
    }
}

pub const RESOLUTIONS: [(&str, &str); 2] = [("1920x1200", "1920 x 1200"), ("1280x800", "1280 x 800")];
pub const BITRATES: [u32; 4] = [10, 20, 30, 50];
pub const CODECS: [Codec; 4] = [Codec::Auto, Codec::Av1, Codec::Hevc, Codec::H264];

pub fn default_settings() -> StreamSettings {
    StreamSettings { resolution: "1920x1200".into(), fps: 60, bitrate_mbps: 20, codec: Codec::Auto }
}

/// A on a setting row: the next value, wrapping.
pub fn cycle<T: PartialEq + Clone>(values: &[T], cur: &T) -> T {
    let i = values.iter().position(|v| v == cur);
    // indexOf -1 in the web wraps to values[0].
    values[(i.map(|i| i as i64).unwrap_or(-1) + 1) as usize % values.len()].clone()
}

/// One line for the quality the next stream uses: "1920 x 1200, 60 fps, 20 Mbps, AV1".
pub fn quality_line(s: &StreamSettings, decode: Option<Decode>, host_codecs: Option<u32>) -> String {
    let res = RESOLUTIONS.iter().find(|r| r.0 == s.resolution).map(|r| r.1.to_string()).unwrap_or_else(|| s.resolution.clone());
    let codec = if s.codec == Codec::Auto { auto_codec(decode, host_codecs) } else { Some(s.codec) };
    let mut parts = vec![res, format!("{} fps", s.fps), format!("{} Mbps", s.bitrate_mbps)];
    if let Some(c) = codec {
        parts.push(codec_label(c).to_string());
    }
    parts.join(", ")
}

/// "192.168.0.20" style check for Add by address (an IPv4, IPv6 or a .local name, maybe a port).
pub fn valid_address(a: &str) -> bool {
    let s = a.trim();
    !s.is_empty() && s.chars().count() <= 253 && s.chars().all(|c| c.is_ascii_alphanumeric() || ".:-[]%".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(f: impl FnOnce(&mut Host)) -> Host {
        let mut h = Host {
            uuid: "A".into(),
            name: "Gaming-PC".into(),
            address: Some("192.168.0.20".into()),
            paired: true,
            online: true,
            current_game: None,
            can_wake: true,
            software: Some("Sunshine".into()),
            codecs: Some(0x1 | 0x100 | 0x10000),
        };
        f(&mut h);
        h
    }
    fn app(id: u32, name: &str, art: bool) -> StreamApp {
        StreamApp { id, name: name.into(), art: art.then(|| rare_render::Src::Static("x", &[])), hdr: false }
    }
    fn elden() -> StreamApp {
        app(1245620, "ELDEN RING", true)
    }
    fn desktop() -> StreamApp {
        app(881448767, "Desktop", false)
    }
    const ALL: Decode = Decode { av1: true, hevc: true, h264: true };

    #[test]
    fn round_trips_running_ids() {
        let id = run_id("5F6A-71C3", 1093255277);
        assert_eq!(id, "stream:5F6A-71C3:1093255277");
        assert_eq!(parse_run_id(&id), Some(("5F6A-71C3".into(), 1093255277)));
        assert_eq!(parse_run_id("app:netflix"), None);
        assert_eq!(parse_run_id("stream:nope"), None);
    }

    #[test]
    fn puts_the_first_paired_pc_that_answers_on_home() {
        let hosts = [host(|h| (h.uuid = "U".into(), h.paired = false).1), host(|h| (h.uuid = "S".into(), h.online = false).1), host(|h| h.uuid = "P".into())];
        assert_eq!(home_host(&hosts).map(|h| h.uuid.as_str()), Some("P"));
        assert!(home_host(&[host(|h| h.online = false)]).is_none());
        assert!(home_host(&[]).is_none());
    }

    #[test]
    fn lists_paired_pcs_games_for_the_library_games_with_art_first() {
        let hosts = [host(|_| {}), host(|h| (h.uuid = "B".into(), h.paired = false).1)];
        let apps = vec![("A".to_string(), vec![desktop(), elden()]), ("B".to_string(), vec![elden()])];
        let g = pc_games(&hosts, &apps);
        let ids: Vec<String> = g.iter().map(|(h, a)| format!("{}:{}", h.uuid, a.id)).collect();
        assert_eq!(ids, ["A:881448767", "A:1245620"]);
        let o: Vec<String> = ordered(&[desktop(), elden()]).into_iter().map(|a| a.name).collect();
        assert_eq!(o, ["ELDEN RING", "Desktop"]);
    }

    #[test]
    fn knows_what_play_does() {
        let e = elden();
        assert_eq!(play_state(&host(|_| {}), &e, &[run_id("A", e.id)]), PlayState::Playing);
        assert_eq!(play_state(&host(|h| h.current_game = Some(e.id)), &e, &[]), PlayState::OnPc);
        assert_eq!(play_state(&host(|h| h.current_game = Some(42)), &e, &[]), PlayState::Busy);
        assert_eq!(play_state(&host(|_| {}), &e, &[]), PlayState::Ready);
        assert_eq!(play_state(&host(|h| h.online = false), &e, &[]), PlayState::Asleep);
        assert_eq!(play_state(&host(|h| (h.online = false, h.can_wake = false).1), &e, &[]), PlayState::Offline);
        // A stream from another PC doesn't count.
        assert_eq!(play_state(&host(|_| {}), &e, &[run_id("B", e.id)]), PlayState::Ready);
        assert_eq!(tile_line(&host(|_| {}), PlayState::Asleep), "Gaming-PC is asleep");
        assert_eq!(tile_line(&host(|_| {}), PlayState::Ready), "Gaming-PC");
        assert_eq!(tile_line(&host(|_| {}), PlayState::OnPc), "Running on your PC");
    }

    #[test]
    fn picks_the_codec_both_ends_support() {
        assert_eq!(auto_codec(Some(ALL), Some(0x1 | 0x100 | 0x10000)), Some(Codec::Av1));
        assert_eq!(auto_codec(Some(ALL), Some(0x1 | 0x100)), Some(Codec::Hevc));
        assert_eq!(auto_codec(Some(Decode { av1: false, hevc: false, h264: true }), Some(0x1 | 0x100)), Some(Codec::H264));
        assert_eq!(auto_codec(None, Some(0x1)), None);
        assert_eq!(auto_codec(Some(ALL), None), None);
        assert_eq!(quality_line(&default_settings(), Some(ALL), Some(0x10101)), "1920 x 1200, 60 fps, 20 Mbps, AV1");
        let s = StreamSettings { resolution: "1280x800".into(), bitrate_mbps: 10, codec: Codec::Hevc, ..default_settings() };
        assert_eq!(quality_line(&s, None, None), "1280 x 800, 60 fps, 10 Mbps, HEVC");
    }

    #[test]
    fn cycles_settings_and_checks_addresses() {
        assert_eq!(cycle(&["10", "20", "30"], &"30"), "10");
        assert_eq!(cycle(&["a", "b"], &"a"), "b");
        assert!(valid_address("192.168.0.20"));
        assert!(valid_address("gaming-pc.local:48989"));
        assert!(valid_address("fe80::2"));
        assert!(!valid_address("pc; rm -rf ~"));
        assert!(!valid_address(" "));
    }
}
