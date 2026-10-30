//! rare-render: the native Home proof of concept.
//!
//! Frame loop: input arrives on the pad thread and is queued; a frame acquires the swapchain image
//! first (FIFO blocks here until the display can take one), then latches every queued input,
//! updates the scene, lays out only if something structural changed, paints and submits. When
//! nothing moves, nothing is drawn and the process sleeps until the next input, image or minute.
//!
//! Env: RR_STATS=<file> frame/input log; RR_PRESENT=fifo|mailbox|immediate; RR_LATENCY=1|2|3;
//! RR_WINDOWED=WxH; RR_LIBRARY=<library.json>; RR_SHOT=<png> (+RR_SCRIPT=right,right,rb and
//! RR_SHOT_EXIT=1) writes a golden image once images are in and nothing moves.

mod data;
mod home;
mod icons;
mod ui;

use rare_render::input::{Pad, PadEvent};
use rare_render::stats::Stats;
use rare_render::theme::{rem_for, tok};
use rare_render::{clock, platform, DrawList, Gpu, ImageLoader, Scene, TextSys};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

static FONTS: [&[u8]; 5] = [
    include_bytes!("../../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../../assets/fonts/FiraCode-Medium.ttf"),
];

enum UserEv {
    Input,
    Wake,
    Live(data::Live),
}

struct Logger;
impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("[{:9.3}] rare-render {}: {}", clock::secs(), r.level(), r.args());
        }
    }
    fn flush(&self) {}
}

struct App {
    proxy: EventLoopProxy<UserEv>,
    inputs: Arc<Mutex<Vec<PadEvent>>>,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    scene: Option<Scene>,
    home: Option<home::Home>,
    text: TextSys,
    images: ImageLoader,
    list: DrawList,
    stats: Stats,
    live: data::Live,
    games: Vec<data::Game>,
    first_done: bool,
    last_frame_ns: u64,
    next_clock: Instant,
    last_mem: f64,
    script: Vec<Pad>,
    shot: Option<String>,
    shot_exit: bool,
    started: Instant,
}

fn key_to_pad(k: KeyCode) -> Option<Pad> {
    Some(match k {
        KeyCode::ArrowUp => Pad::Up,
        KeyCode::ArrowDown => Pad::Down,
        KeyCode::ArrowLeft => Pad::Left,
        KeyCode::ArrowRight => Pad::Right,
        KeyCode::Enter | KeyCode::Space => Pad::A,
        KeyCode::Escape | KeyCode::Backspace => Pad::B,
        KeyCode::KeyX => Pad::X,
        KeyCode::KeyY => Pad::Y,
        KeyCode::KeyQ => Pad::Lb,
        KeyCode::KeyE => Pad::Rb,
        _ => return None,
    })
}

impl App {
    fn build_scene(&mut self) {
        let (Some(window), Some(_gpu)) = (&self.window, &self.gpu) else { return };
        let size = window.inner_size();
        let (w, h) = (size.width as f32, size.height as f32);
        let rem = rem_for(w, h);
        let mut scene = Scene::new(w, h, rem);
        let t0 = Instant::now();
        let mut home = home::Home::build(&mut scene, &mut self.text, &mut self.images, self.games.clone(), &self.live);
        let now = clock::secs();
        home.start(&mut scene, &mut self.text, &mut self.images, now);
        log::info!("scene built in {:.1} ms ({}x{} rem {rem})", t0.elapsed().as_secs_f32() * 1000.0, w, h);
        self.scene = Some(scene);
        self.home = Some(home);
    }

    /// Applies every queued input to the screen. Returns how many there were.
    fn latch_inputs(&mut self) -> usize {
        let evs: Vec<PadEvent> = std::mem::take(&mut *self.inputs.lock().unwrap());
        let (Some(scene), Some(home)) = (self.scene.as_mut(), self.home.as_mut()) else { return 0 };
        let now = clock::secs();
        for e in &evs {
            let t = clock::mono_ns();
            let _ = home.on_pad(scene, &mut self.text, &mut self.images, e.pad, now);
            self.stats.line(format_args!("in {} {} {} {}", e.t_ns, e.pad.name(), e.repeat as u8, t));
        }
        evs.len()
    }

    fn frame(&mut self) {
        let Some(mut gpu) = self.gpu.take() else { return };
        let t_begin = clock::mono_ns();
        let Some(frame) = gpu.acquire() else {
            self.gpu = Some(gpu);
            return;
        };
        let t_acq = clock::mono_ns();
        let n_in = self.latch_inputs();
        let now = clock::secs();
        let (Some(scene), Some(home)) = (self.scene.as_mut(), self.home.as_mut()) else {
            self.gpu = Some(gpu);
            return;
        };
        self.images.poll(&mut gpu);
        scene.sync_images(now, &self.images, 0.12);
        home.tick(scene, &mut self.text, &mut self.images, now);
        let mut laid = scene.layout(&mut self.text);
        home.after_layout(scene, now, self.first_done);
        if scene.layout(&mut self.text) {
            laid = true;
            if std::env::var("RR_DUMP").is_ok() && !self.first_done {
                scene.tree.print_tree(scene.root);
            }
        }
        scene.paint(now, &mut self.text, &self.images, &mut self.list);
        let moving = scene.animating(now);
        self.text.upload(&mut gpu);
        let t_paint = clock::mono_ns();
        let submit_us = gpu.submit(frame, &self.list, tok::BG);
        let t_done = clock::mono_ns();
        self.stats.frames += 1;
        let interval = if self.last_frame_ns > 0 { (t_done - self.last_frame_ns) / 1000 } else { 0 };
        self.last_frame_ns = t_done;
        let fno = self.stats.frames;
        let (np, nb) = (self.list.prims.len(), self.list.batches.len());
        self.stats.line(format_args!(
            "f {} {} {} {} {} {} {} {} {} {} {} {}",
            fno,
            t_begin,
            t_done,
            (t_paint - t_acq) / 1000,
            (t_acq - t_begin) / 1000,
            submit_us,
            np,
            nb,
            laid as u8,
            n_in,
            interval,
            moving as u8
        ));
        if !self.first_done {
            self.first_done = true;
            let ms = (t_done - clock::mark_start()) as f64 / 1e6;
            log::info!("first frame presented {ms:.1} ms after start ({} prims, {} batches)", self.list.prims.len(), self.list.batches.len());
            self.stats.line(format_args!("first {t_done} {ms:.1}"));
            self.stats.flush();
        }
        if now - self.last_mem > 5.0 {
            self.last_mem = now;
            self.stats.line(format_args!("mem {} {} {} {}", t_done, platform::rss_kib(), gpu.texture_bytes, self.text.glyph_count()));
        }
        self.stats.maybe_flush();
        self.gpu = Some(gpu);
        self.maybe_shot();
    }

    fn maybe_shot(&mut self) {
        let Some(path) = self.shot.clone() else { return };
        let now = clock::secs();
        let busy = self.scene.as_ref().is_some_and(|s| s.animating(now)) || self.images.pending() > 0 || self.home.as_ref().and_then(|h| h.next_deadline()).is_some();
        if busy || self.started.elapsed() < Duration::from_millis(1500) {
            if self.started.elapsed().as_secs_f32() > 20.0 {
                log::warn!("shot waiting: animating {} images pending {} deadline {:?}", self.scene.as_ref().is_some_and(|s| s.animating(now)), self.images.pending(), self.home.as_ref().and_then(|h| h.next_deadline()));
            }
            return;
        }
        if let Some(p) = self.script.first().copied() {
            self.script.remove(0);
            self.inputs.lock().unwrap().push(PadEvent { pad: p, repeat: false, t_ns: clock::mono_ns() });
            if let Some(w) = &self.window {
                w.request_redraw();
            }
            return;
        }
        self.shot = None;
        let gpu = self.gpu.as_mut().unwrap();
        let (w, h, px) = gpu.capture(&self.list, tok::BG);
        match image::save_buffer(&path, &px, w, h, image::ExtendedColorType::Rgba8) {
            Ok(()) => log::info!("golden image {path} ({w}x{h})"),
            Err(e) => log::warn!("golden image {path}: {e}"),
        }
        if self.shot_exit {
            std::process::exit(0);
        }
    }
}

impl ApplicationHandler<UserEv> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let mut attrs = Window::default_attributes().with_title("Rare");
        let windowed = std::env::var("RR_WINDOWED").ok().and_then(|v| {
            let (a, b) = v.split_once('x')?;
            Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?))
        });
        if cfg!(target_os = "linux") && windowed.is_none() {
            attrs = attrs.with_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
        } else {
            let (w, h) = windowed.unwrap_or((1280, 800));
            attrs = attrs.with_inner_size(winit::dpi::PhysicalSize::new(w, h)).with_resizable(false);
        }
        let window = Arc::new(el.create_window(attrs).expect("window"));
        log::info!("window: {}", platform::tag_window(&window, platform::RARE_APPID));
        let size = window.inner_size();
        let t = Instant::now();
        let gpu = Gpu::new(window.clone(), size.width, size.height);
        log::info!("gpu ready in {:.1} ms: {}", t.elapsed().as_secs_f32() * 1000.0, gpu.adapter_info);
        self.window = Some(window.clone());
        self.gpu = Some(gpu);
        self.build_scene();
        window.request_redraw();
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, ev: WindowEvent) {
        match ev {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(sz) => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.resize(sz.width, sz.height);
                }
                let changed = self.scene.as_ref().is_some_and(|s| s.w != sz.width as f32 || s.h != sz.height as f32);
                if changed {
                    log::info!("resized to {}x{}: rebuilding", sz.width, sz.height);
                    self.build_scene();
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    if let PhysicalKey::Code(k) = event.physical_key {
                        if let Some(p) = key_to_pad(k) {
                            self.inputs.lock().unwrap().push(PadEvent { pad: p, repeat: event.repeat, t_ns: clock::mono_ns() });
                            if let Some(w) = &self.window {
                                w.request_redraw();
                            }
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => self.frame(),
            _ => {}
        }
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, ev: UserEv) {
        match ev {
            UserEv::Input | UserEv::Wake => {}
            UserEv::Live(l) => {
                self.live = l;
                if let (Some(scene), Some(home)) = (self.scene.as_mut(), self.home.as_mut()) {
                    home.set_live(scene, &mut self.text, &self.live);
                }
            }
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let now = clock::secs();
        if Instant::now() >= self.next_clock {
            self.next_clock = Instant::now() + Duration::from_secs_f32(platform::secs_to_next_minute());
            if let (Some(scene), Some(home)) = (self.scene.as_mut(), self.home.as_mut()) {
                home.tick_clock(scene, &mut self.text);
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
        }
        let moving = self.scene.as_ref().is_some_and(|s| s.animating(now)) || self.shot.is_some();
        if moving {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        } else {
            self.stats.flush();
        }
        let mut wake = self.next_clock;
        if let Some(at) = self.home.as_ref().and_then(|h| h.next_deadline()) {
            if at <= now {
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            let d = Duration::from_secs_f64((at - now).max(0.0));
            wake = wake.min(Instant::now() + d);
        }
        el.set_control_flow(ControlFlow::WaitUntil(wake));
    }
}

fn main() {
    let t_start = clock::mark_start();
    let _ = log::set_logger(&Logger).map(|()| log::set_max_level(log::LevelFilter::Info));
    let mut stats = Stats::from_env();
    stats.line(format_args!("start {t_start}"));

    let event_loop = EventLoop::<UserEv>::with_user_event().build().expect("event loop");
    let proxy = event_loop.create_proxy();

    let inputs: Arc<Mutex<Vec<PadEvent>>> = Arc::default();
    {
        let inputs = inputs.clone();
        let proxy = proxy.clone();
        rare_render::input::spawn(move |e| {
            inputs.lock().unwrap().push(e);
            let _ = proxy.send_event(UserEv::Input);
        });
    }
    let waker = {
        let proxy = proxy.clone();
        Arc::new(move || {
            let _ = proxy.send_event(UserEv::Wake);
        }) as Arc<dyn Fn() + Send + Sync>
    };
    let cache = std::env::var("HOME").ok().map(|h| std::path::PathBuf::from(h).join(".cache/rare-render/img"));
    let images = ImageLoader::new(3, waker, cache);
    let t = Instant::now();
    let text = TextSys::new(&FONTS, std::env::var("RR_NO_SYSTEM_FONTS").is_err(), 2048);
    log::info!("fonts loaded in {:.1} ms", t.elapsed().as_secs_f32() * 1000.0);
    let games = data::load_library();
    log::info!("library: {} games from {}", games.len(), data::library_path().display());

    {
        let proxy = proxy.clone();
        std::thread::Builder::new()
            .name("rr-live".into())
            .spawn(move || loop {
                let l = data::fetch_live();
                if proxy.send_event(UserEv::Live(l)).is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_secs(20));
            })
            .ok();
    }

    let script = std::env::var("RR_SCRIPT")
        .unwrap_or_default()
        .split(',')
        .filter_map(|s| match s.trim() {
            "up" => Some(Pad::Up),
            "down" => Some(Pad::Down),
            "left" => Some(Pad::Left),
            "right" => Some(Pad::Right),
            "lb" => Some(Pad::Lb),
            "rb" => Some(Pad::Rb),
            "a" => Some(Pad::A),
            _ => None,
        })
        .collect();
    let mut app = App {
        proxy,
        inputs,
        window: None,
        gpu: None,
        scene: None,
        home: None,
        text,
        images,
        list: DrawList::default(),
        stats,
        live: data::Live::default(),
        games,
        first_done: false,
        last_frame_ns: 0,
        next_clock: Instant::now(),
        last_mem: 0.0,
        script,
        shot: std::env::var("RR_SHOT").ok(),
        shot_exit: std::env::var("RR_SHOT_EXIT").is_ok(),
        started: Instant::now(),
    };
    let _ = &app.proxy;
    event_loop.run_app(&mut app).expect("run");
}
