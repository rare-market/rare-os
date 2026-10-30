//! A mocked gaming PC (stream/mock.ts): one paired PC running Sunshine with an RTX 40 (AV1), and
//! one unpaired PC on the network. `Mode` stands for the web's URL switches. The box art is the
//! two covers under render/assets (the web uses Steam's library covers).

use super::logic::*;
use super::state::StreamApi;
use rare_render::Src;

pub const PC: &str = "5F6A2B10-71C3-4E7D-9A55-0B1C2D3E4F50";
pub const DEN: &str = "B7E01C44-2D9A-4F10-8C3B-6A5D4E3F2A10";
pub const COMMAND: &str = "flatpak install --user -y flathub com.moonlight_stream.Moonlight";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// The paired PC answers.
    Normal,
    /// ?stream=none: no PCs yet.
    NoPcs,
    /// ?stream=nomoonlight
    NoMoonlight,
    /// ?stream=asleep: Wake-on-LAN works.
    Asleep,
    /// ?streambusy: the PC runs another game (ELDEN RING).
    Busy,
    /// ?streampair: the PIN stays up.
    Pairing,
    /// The PIN is wrong.
    PairFail,
    /// The unpaired PC runs GeForce Experience.
    Gfe,
    /// Off, and no MAC address to wake it with.
    Offline,
    /// Asleep, and it never comes up.
    NeverWakes,
    /// Online with nothing listed in Sunshine.
    NoGames,
    /// Every call fails.
    Broken,
}

fn cover(i: usize) -> Src {
    static ACT: &[u8] = include_bytes!("../../../../render/assets/games/act/cover.jpg");
    static SUI: &[u8] = include_bytes!("../../../../render/assets/games/act/cover.jpg");
    if i % 2 == 0 {
        Src::Static("stream-act-cover.jpg", ACT)
    } else {
        Src::Static("stream-act-cover.jpg", SUI)
    }
}

pub fn apps_list() -> Vec<StreamApp> {
    let a = |i: usize, id: u32, name: &str, art: bool, hdr: bool| StreamApp { id, name: name.into(), art: art.then(|| cover(i)), hdr };
    vec![
        a(0, 1091500, "Cyberpunk 2077", true, true),
        a(1, 1245620, "ELDEN RING", true, false),
        a(2, 1086940, "Baldur's Gate 3", true, false),
        a(3, 1551360, "Forza Horizon 5", true, true),
        a(4, 1174180, "Red Dead Redemption 2", true, false),
        a(5, 2358720, "Black Myth: Wukong", true, true),
        a(0, 881448767, "Desktop", false, false),
        a(0, 1093255277, "Steam Big Picture", false, false),
    ]
}

pub fn found(gfe: bool) -> Host {
    Host {
        uuid: DEN.into(),
        name: "Living-room".into(),
        address: Some("192.168.0.31".into()),
        paired: false,
        online: true,
        current_game: None,
        can_wake: true,
        software: Some(if gfe { "GeForce Experience" } else { "Sunshine" }.into()),
        codecs: Some(0x1 | 0x100),
    }
}

pub struct MockApi {
    pub mode: Mode,
    hosts: Vec<Host>,
    settings: StreamSettings,
    installed: bool,
    pairing: Option<PairView>,
    polls: u32,
    wake_pending: bool,
    running: Vec<String>,
}

impl MockApi {
    pub fn new(mode: Mode) -> MockApi {
        let hosts = if mode == Mode::NoPcs {
            vec![]
        } else {
            vec![Host {
                uuid: PC.into(),
                name: "Gaming-PC".into(),
                address: Some("192.168.0.20".into()),
                paired: true,
                online: !matches!(mode, Mode::Asleep | Mode::Offline | Mode::NeverWakes),
                current_game: (mode == Mode::Busy).then_some(1245620),
                can_wake: mode != Mode::Offline,
                software: Some("Sunshine".into()),
                codecs: Some(0x1 | 0x100 | 0x200 | 0x10000 | 0x20000),
            }]
        };
        MockApi { mode, hosts, settings: default_settings(), installed: mode != Mode::NoMoonlight, pairing: None, polls: 0, wake_pending: false, running: vec![] }
    }

    /// Streams already going (the launcher's running list).
    pub fn with_running(mut self, running: Vec<String>) -> MockApi {
        self.running = running;
        self
    }

    fn broken(&self) -> Result<(), String> {
        if self.mode == Mode::Broken {
            Err("connection refused".into())
        } else {
            Ok(())
        }
    }

    fn apps_for(&self, uuid: &str) -> Vec<StreamApp> {
        if self.mode == Mode::NoGames {
            return vec![];
        }
        if self.hosts.iter().any(|h| h.uuid == uuid && h.paired) {
            apps_list()
        } else {
            vec![]
        }
    }
}

impl StreamApi for MockApi {
    fn status(&mut self) -> Result<Status, String> {
        self.broken()?;
        Ok(Status {
            moonlight: self.installed.then(|| "flatpak".to_string()),
            offer: (!self.installed).then(|| Offer { flatpak: "com.moonlight_stream.Moonlight".into(), name: "Moonlight".into(), command: COMMAND.into() }),
            decode: Some(Decode { av1: true, hevc: true, h264: true }),
            settings: self.settings.clone(),
            hosts: self.hosts.clone(),
            apps: self.hosts.iter().filter(|h| h.paired).map(|h| (h.uuid.clone(), self.apps_for(&h.uuid))).collect(),
        })
    }
    fn refresh(&mut self) -> Result<Vec<Host>, String> {
        self.broken()?;
        if self.wake_pending && self.mode != Mode::NeverWakes {
            self.wake_pending = false;
            for h in &mut self.hosts {
                h.online = true;
            }
        }
        Ok(self.hosts.clone())
    }
    fn discover(&mut self) -> Result<Vec<Host>, String> {
        self.broken()?;
        if !self.hosts.iter().any(|h| h.uuid == DEN) {
            self.hosts.push(found(self.mode == Mode::Gfe));
        }
        Ok(self.hosts.clone())
    }
    fn add(&mut self, address: &str) -> Result<Host, String> {
        self.broken()?;
        let known = self.hosts.iter().any(|h| h.address.as_deref() == Some(address));
        if address != "192.168.0.31" && address != "192.168.0.20" && !known {
            return Err(format!("No PC answered at {address}. Check Sunshine is running on it."));
        }
        if let Some(h) = self.hosts.iter().find(|h| h.address.as_deref() == Some(address)) {
            return Ok(h.clone());
        }
        let f = found(self.mode == Mode::Gfe);
        self.hosts.push(f.clone());
        Ok(f)
    }
    fn pair_start(&mut self, address: &str) -> Result<PairView, String> {
        self.broken()?;
        let h = self.hosts.iter().find(|h| h.address.as_deref() == Some(address)).cloned().unwrap_or_else(|| found(self.mode == Mode::Gfe));
        self.polls = 0;
        let p = PairView { uuid: h.uuid, name: h.name, address: address.into(), pin: "4821".into(), phase: PairPhase::Waiting, error: None, software: h.software.unwrap_or_else(|| "Sunshine".into()) };
        self.pairing = Some(p.clone());
        Ok(p)
    }
    fn pair_poll(&mut self) -> Result<Option<PairView>, String> {
        self.polls += 1;
        let (mode, polls) = (self.mode, self.polls);
        let Some(p) = self.pairing.as_mut() else { return Ok(None) };
        match mode {
            Mode::Pairing => {}
            Mode::PairFail if polls >= 2 => {
                p.phase = PairPhase::Failed;
                p.error = Some("The PIN didn't match. Try again.".into());
            }
            Mode::PairFail => {}
            _ if polls >= 8 => {
                p.phase = PairPhase::Paired;
                let uuid = p.uuid.clone();
                if let Some(h) = self.hosts.iter_mut().find(|h| h.uuid == uuid) {
                    h.paired = true;
                }
            }
            _ => {}
        }
        Ok(self.pairing.clone())
    }
    fn pair_cancel(&mut self) {
        self.pairing = None;
    }
    fn apps(&mut self, uuid: &str) -> Result<Vec<StreamApp>, String> {
        self.broken()?;
        Ok(self.apps_for(uuid))
    }
    fn launch(&mut self, uuid: &str, app: u32, _replace: bool) -> Result<(), String> {
        self.broken()?;
        self.running.push(run_id(uuid, app));
        Ok(())
    }
    fn resume(&mut self, _id: &str) -> Result<(), String> {
        self.broken()
    }
    fn quit(&mut self, uuid: &str) -> Result<(), String> {
        self.broken()?;
        let pre = format!("stream:{uuid}:");
        self.running.retain(|r| !r.starts_with(&pre));
        for h in self.hosts.iter_mut().filter(|h| h.uuid == uuid) {
            h.current_game = None;
        }
        Ok(())
    }
    fn wake(&mut self, _uuid: &str) -> Result<(), String> {
        self.broken()?;
        self.wake_pending = true;
        Ok(())
    }
    fn set_settings(&mut self, s: &StreamSettings) -> Result<(), String> {
        self.broken()?;
        self.settings = s.clone();
        Ok(())
    }
    fn install(&mut self) -> Result<(), String> {
        self.broken()?;
        self.installed = true;
        Ok(())
    }
    fn running(&mut self) -> Vec<String> {
        self.running.clone()
    }
}
