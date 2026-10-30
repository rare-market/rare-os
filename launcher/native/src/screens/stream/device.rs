//! The streaming calls on the device: `StreamApi` over rare-stream and the launcher's runner
//! (src-tauri/src/stream.rs). Slow looks at the network (asking every PC who answers) run on a
//! worker and are picked up by the next `refresh`, so the sheet never waits on a PC that is off.

use super::logic::*;
use super::state::StreamApi;
use crate::data::backend::StreamHost;
use rare_render::Src;
use rare_stream::cli;
use rare_stream::store::{self, HostView};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Moonlight's install as the Apps installer reports it ("app-install" events, id MOONLIGHT_ID),
/// waiting for the sheet to pick it up.
static INSTALL: Mutex<Option<(Option<f32>, bool, Option<String>)>> = Mutex::new(None);
const MOONLIGHT_ID: &str = "moonlight";

/// features::apps::on_event hands every install event here; only Moonlight's matters.
pub fn install_event(i: &rare_core::events::InstallEvent) {
    if i.id == MOONLIGHT_ID {
        *INSTALL.lock().unwrap_or_else(|e| e.into_inner()) = Some((i.progress, i.done, i.error.clone()));
    }
}

pub struct DeviceApi {
    runner: rare_launch::launch::Runner,
    ctx: rare_core::Ctx,
    /// The last answer of a background look.
    seen: Arc<Mutex<Option<Vec<HostView>>>>,
    looking: Arc<AtomicBool>,
    last: Vec<Host>,
}

impl DeviceApi {
    pub fn new(runner: rare_launch::launch::Runner, ctx: rare_core::Ctx) -> DeviceApi {
        DeviceApi { runner, ctx, seen: Arc::new(Mutex::new(None)), looking: Arc::new(AtomicBool::new(false)), last: vec![] }
    }

    fn host(&self) -> StreamHost<'_> {
        StreamHost { runner: &self.runner, ctx: &self.ctx }
    }
}

fn host(h: HostView) -> Host {
    Host { uuid: h.uuid, name: h.name, address: h.address, paired: h.paired, online: h.online, current_game: h.current_game, can_wake: h.can_wake, software: h.software, codecs: h.codecs }
}

fn app(a: store::StreamApp) -> StreamApp {
    StreamApp { id: a.id, name: a.name, art: a.art.map(|p| Src::File(p.into())), hdr: a.hdr }
}

fn codec(c: cli::Codec) -> Codec {
    match c {
        cli::Codec::Auto => Codec::Auto,
        cli::Codec::Av1 => Codec::Av1,
        cli::Codec::Hevc => Codec::Hevc,
        cli::Codec::H264 => Codec::H264,
    }
}

fn to_codec(c: Codec) -> cli::Codec {
    match c {
        Codec::Auto => cli::Codec::Auto,
        Codec::Av1 => cli::Codec::Av1,
        Codec::Hevc => cli::Codec::Hevc,
        Codec::H264 => cli::Codec::H264,
    }
}

fn pair_view(p: rare_stream::pair::PairView) -> PairView {
    let phase = match p.phase.as_str() {
        "paired" => PairPhase::Paired,
        "failed" => PairPhase::Failed,
        _ => PairPhase::Waiting,
    };
    PairView { uuid: p.uuid, name: p.name, address: p.address, pin: p.pin, phase, error: p.error, software: p.software }
}

impl StreamApi for DeviceApi {
    fn status(&mut self) -> Result<Status, String> {
        let s = rare_stream::status(&self.host());
        let hosts: Vec<Host> = s.hosts.into_iter().map(host).collect();
        self.last = hosts.clone();
        Ok(Status {
            moonlight: s.moonlight,
            offer: s.offer.map(|o| Offer { flatpak: o.flatpak, name: o.name, command: o.command }),
            decode: s.decode.map(|d| Decode { av1: d.av1, hevc: d.hevc, h264: d.h264 }),
            settings: StreamSettings { resolution: s.settings.resolution, fps: s.settings.fps, bitrate_mbps: s.settings.bitrate_mbps, codec: codec(s.settings.codec) },
            hosts,
            apps: s.apps.into_iter().map(|(u, l)| (u, l.into_iter().map(app).collect())).collect(),
        })
    }

    fn refresh(&mut self) -> Result<Vec<Host>, String> {
        if let Some(v) = self.seen.lock().unwrap_or_else(|e| e.into_inner()).take() {
            self.last = v.into_iter().map(host).collect();
        }
        if !self.looking.swap(true, Ordering::SeqCst) {
            let (seen, looking) = (self.seen.clone(), self.looking.clone());
            std::thread::spawn(move || {
                let v = rare_stream::refresh();
                *seen.lock().unwrap_or_else(|e| e.into_inner()) = Some(v);
                looking.store(false, Ordering::SeqCst);
            });
        }
        Ok(self.last.clone())
    }

    fn discover(&mut self) -> Result<Vec<Host>, String> {
        self.last = rare_stream::scan(3000).into_iter().map(host).collect();
        Ok(self.last.clone())
    }

    fn add(&mut self, address: &str) -> Result<Host, String> {
        rare_stream::add(address).map(host)
    }

    fn pair_start(&mut self, address: &str) -> Result<PairView, String> {
        rare_stream::pair_start(&self.host(), address).map(pair_view)
    }

    fn pair_poll(&mut self) -> Result<Option<PairView>, String> {
        Ok(rare_stream::pair::poll().map(pair_view))
    }

    fn pair_cancel(&mut self) {
        rare_stream::pair::cancel();
    }

    fn apps(&mut self, uuid: &str) -> Result<Vec<StreamApp>, String> {
        rare_stream::apps(&self.host(), uuid).map(|l| l.into_iter().map(app).collect())
    }

    fn launch(&mut self, uuid: &str, app: u32, replace: bool) -> Result<(), String> {
        rare_stream::launch(&self.host(), uuid, app, replace)
    }

    fn resume(&mut self, id: &str) -> Result<(), String> {
        rare_stream::resume(&self.host(), id)
    }

    fn quit(&mut self, uuid: &str) -> Result<(), String> {
        rare_stream::quit(&self.host(), uuid)
    }

    fn wake(&mut self, uuid: &str) -> Result<(), String> {
        rare_stream::wake(uuid)
    }

    fn set_settings(&mut self, s: &StreamSettings) -> Result<(), String> {
        rare_stream::set_settings(cli::Settings { resolution: s.resolution.clone(), fps: s.fps, bitrate_mbps: s.bitrate_mbps, codec: to_codec(s.codec) })
    }

    fn install(&mut self) -> Result<(), String> {
        *INSTALL.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let flathub = rare_launch::apps::env().user_flathub;
        rare_launch::apps::install::start(self.ctx.clone(), MOONLIGHT_ID.into(), cli::FLATPAK.into(), flathub).map_err(|e| e.to_string())
    }

    fn install_poll(&mut self) -> Option<(Option<f32>, bool, Option<String>)> {
        INSTALL.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    fn fork(&self) -> Option<Box<dyn StreamApi + Send>> {
        Some(Box::new(DeviceApi::new(self.runner.clone(), self.ctx.clone())))
    }

    fn running(&mut self) -> Vec<String> {
        self.runner.running()
    }
}
