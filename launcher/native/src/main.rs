//! rare-launcher-native: the Rare launcher on Rare Render.
//!
//! Frame loop (the POC's): input arrives on the pad thread and is queued; a frame acquires the
//! swapchain image first, latches every queued input, updates the app (views rebuild only when
//! their state moved), lays out only if something structural changed, paints and submits. When
//! nothing moves, nothing is drawn and the process sleeps until the next input, data, timer or
//! minute.
//!
//! Env:
//!   RR_FIXTURE=1          the web launcher's browser mocks (library, device, live numbers): the
//!                         same content as launcher/scripts/shots.mjs, for golden images (always on
//!                         off Linux)
//!   RR_TAB=library        start on a tab;  RR_OPEN=game:<id> | search | qam |
//!                         launchfail:<id>|<reason> | options:<id> | uninstall:<id> | osk | handoff | toast:<text> |
//!                         ingame:<run id>|<menu|switcher|quick|power>|<frame png> (Rare over a game)
//!   RR_SEARCH=<q>         the search already typed;  RR_RUNNING=<id,...> games shown as running
//!   RR_CLOCK="8:05 PM"    a fixed clock;  RR_BOOT=hold|off  keep the boot splash up, or skip it
//!   RR_SHOT=<png>         render headless, play RR_SCRIPT (right,right,a,wait:500,text:ha), write a
//!                         golden image once settled and exit;  RR_WINDOWED=WxH  window/shot size
//!   RR_STATS=<file>       frame and input log;  RR_LIBRARY=<library.json>  the device's library
//!   RR_USERLIB=<userlib.json>  collections and hidden games from that file (read only)
//!   RR_MOCK_TITLES=A|B   extra art-less fixture games (text covers)
//!   RR_DEVICE=1           the device backend and live data off Linux too (a device's data under $HOME)
//!   RR_SCRIPT touch steps  tap:X:Y, hold:X:Y (a long press), drag:X1:Y1:X2:Y2[:MS] (a finger moved
//!                         over MS ms, 250 by default, then lifted; short and fast is a flick, from
//!                         the left edge it is Back), in window pixels
//!   RR_TOUCH_ROTATE=0|90|180|270  turn touchscreen points (a compositor that does not rotate them)

use rare_native::app::{App, Start0, TouchPhase};
use rare_native::page::Btn;
use rare_native::state::Tab;
use rare_render::stats::Stats;
use rare_render::theme::{rem_for, tok};
use rare_render::winit;
use rare_render::{clock, platform, DrawList, Gpu, ImageLoader, Scene, TextSys};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

static FONTS: [&[u8]; 5] = [
    include_bytes!("../../render/assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../render/assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../render/assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../render/assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../../render/assets/fonts/FiraCode-Medium.ttf"),
];

struct Logger;
impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("[{:9.3}] rare-launcher {}: {}", clock::secs(), r.level(), r.args());
        }
    }
    fn flush(&self) {}
}

#[derive(Clone, Copy, Debug)]
struct In {
    b: Btn,
    repeat: bool,
    t_ns: u64,
}

enum Step {
    Press(Btn),
    Wait(f64),
    Text(String),
    /// `burst:0+50+100+200` then a button: press it and capture frames at those ms after.
    Burst(Vec<f64>),
    /// Touch contacts: (phase, x, y, ms after the step starts).
    Touch(Vec<(TouchPhase, f32, f32, f64)>),
    /// `held:BTN:MS`: a direction held MS ms, repeating on the pad's cadence, frames in between.
    /// `flick:BTN:N:GAP[:HOLD]`: N presses GAP ms apart, each held HOLD ms (default 150), as a
    /// fast stick flicked again and again.
    Pad { b: Btn, n: usize, gap: f64, hold: f64 },
}

/// `held:` and `flick:` steps.
fn pad_step(t: &str) -> Option<Step> {
    let mut it = t.split(':');
    let kind = it.next()?;
    let b = Btn::parse(it.next()?)?;
    let n: Vec<f64> = it.filter_map(|v| v.parse().ok()).collect();
    match kind {
        "held" => Some(Step::Pad { b, n: 1, gap: 0.0, hold: *n.first()? }),
        "flick" => Some(Step::Pad { b, n: *n.first()? as usize, gap: *n.get(1)?, hold: n.get(2).copied().unwrap_or(150.0) }),
        _ => None,
    }
}

/// `tap:X:Y`, `hold:X:Y`, `drag:X1:Y1:X2:Y2[:MS]`.
fn touch_step(t: &str) -> Option<Step> {
    let (kind, rest) = t.split_once(':')?;
    let n: Vec<f64> = rest.split(':').filter_map(|v| v.parse().ok()).collect();
    let p = |i: usize| n.get(i).copied().map(|v| v as f32);
    Some(Step::Touch(match kind {
        "tap" => vec![(TouchPhase::Down, p(0)?, p(1)?, 0.0), (TouchPhase::Up, p(0)?, p(1)?, 60.0)],
        "hold" => vec![(TouchPhase::Down, p(0)?, p(1)?, 0.0), (TouchPhase::Up, p(0)?, p(1)?, 800.0)],
        "drag" => {
            let (x1, y1, x2, y2) = (p(0)?, p(1)?, p(2)?, p(3)?);
            let ms = n.get(4).copied().unwrap_or(250.0).max(16.0);
            let steps = (ms / 16.0).ceil() as usize;
            let mut v = vec![(TouchPhase::Down, x1, y1, 0.0)];
            for i in 1..=steps {
                let k = i as f32 / steps as f32;
                v.push((TouchPhase::Move, x1 + (x2 - x1) * k, y1 + (y2 - y1) * k, ms * i as f64 / steps as f64));
            }
            v.push((TouchPhase::Up, x2, y2, ms + 1.0));
            v
        }
        _ => return None,
    }))
}

fn parse_script(s: &str) -> Vec<Step> {
    s.split(',')
        .filter_map(|t| {
            let t = t.trim();
            if let Some(ms) = t.strip_prefix("wait:") {
                return ms.parse().ok().map(Step::Wait);
            }
            if let Some(x) = t.strip_prefix("burst:") {
                return Some(Step::Burst(x.split('+').filter_map(|v| v.parse().ok()).collect()));
            }
            if let Some(x) = t.strip_prefix("text:") {
                return Some(Step::Text(x.to_string()));
            }
            if let Some(s) = pad_step(t) {
                return Some(s);
            }
            if let Some(s) = touch_step(t) {
                return Some(s);
            }
            Btn::parse(t).map(Step::Press)
        })
        .collect()
}

fn key_to_btn(k: KeyCode) -> Option<Btn> {
    Some(match k {
        KeyCode::ArrowUp => Btn::Up,
        KeyCode::ArrowDown => Btn::Down,
        KeyCode::ArrowLeft => Btn::Left,
        KeyCode::ArrowRight => Btn::Right,
        KeyCode::Enter => Btn::A,
        KeyCode::Escape | KeyCode::Backspace => Btn::B,
        KeyCode::KeyX => Btn::X,
        KeyCode::KeyY => Btn::Y,
        KeyCode::KeyQ => Btn::Lb,
        KeyCode::KeyE => Btn::Rb,
        KeyCode::KeyZ => Btn::Lt,
        KeyCode::KeyC => Btn::Rt,
        KeyCode::KeyM => Btn::Start,
        KeyCode::KeyV => Btn::Select,
        KeyCode::KeyG | KeyCode::F17 => Btn::Guide,
        KeyCode::Tab | KeyCode::F18 => Btn::Qam,
        _ => return None,
    })
}

fn start0() -> Start0 {
    let boot = std::env::var("RR_BOOT").unwrap_or_default();
    let fixture = std::env::var("RR_FIXTURE").is_ok() || (!cfg!(target_os = "linux") && std::env::var("RR_DEVICE").is_err());
    Start0 {
        tab: std::env::var("RR_TAB").ok().and_then(|t| Tab::parse(&t)),
        open: std::env::var("RR_OPEN").ok().filter(|s| !s.is_empty()),
        boot_hold: boot == "hold",
        no_boot: boot == "off" || (std::env::var("RR_SHOT").is_ok() && boot != "hold"),
        fixture,
        running: std::env::var("RR_RUNNING").ok().map(|s| s.split(',').filter(|x| !x.is_empty()).map(String::from).collect()).unwrap_or_default(),
    }
}

fn backend(fixture: bool) -> Arc<dyn rare_native::data::backend::Backend> {
    if fixture {
        Arc::new(rare_native::data::backend::Mock::new(std::env::var("RR_NOGAMES").is_ok()))
    } else {
        rare_system::host::set(rare_native::screens::settings::live::NativeHost);
        // An audio reading puts the saved "Louder speakers" choice on the speaker chain.
        std::thread::spawn(|| drop(rare_system::audio::read()));
        Arc::new(rare_native::data::backend::DeviceBackend::new())
    }
}

fn size_from_env() -> Option<(u32, u32)> {
    let v = std::env::var("RR_WINDOWED").ok()?;
    let (a, b) = v.split_once('x')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}


/// Golden images: no window, the offscreen GPU, the script, a PNG, exit.
fn headless(shot: String) {
    let (w, h) = size_from_env().unwrap_or((1280, 800));
    let mut gpu = Gpu::headless(w, h).expect("headless gpu");
    let text = TextSys::new(&FONTS, std::env::var("RR_NO_SYSTEM_FONTS").is_err(), 4096);
    let images = rare_native::data::img_fetch::loader(3, Arc::new(|| {}));
    let scene = Scene::new(w as f32, h as f32, rem_for(w as f32, h as f32));
    let s0 = start0();
    let mut app = App::new(scene, text, images, backend(s0.fixture), Arc::new(|| {}), s0);
    app.journey_begin();
    let mut script: std::collections::VecDeque<Step> = parse_script(&std::env::var("RR_SCRIPT").unwrap_or_default()).into();
    // RR_DRIVE=1 (tools/journeys/crawl.py): after the script, take steps from stdin one line at a
    // time instead of exiting; `state` prints the app's state and the frame's change, `shot <png>`
    // writes the frame, `quit` exits. A screen that never settles counts as settled after 4 s.
    let drive = std::env::var("RR_DRIVE").is_ok();
    let settle_ms = std::env::var("RR_SETTLE_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(350u64);
    let mut prev_px: Option<Vec<u8>> = None;
    let mut stdin_lines = std::io::stdin().lines();
    let mut list = DrawList::default();
    let started = Instant::now();
    let mut wait_until = 0.0;
    let mut last_input = Instant::now();
    // RR_STATS here too: one `f` line per pass (the CPU side of a frame: update, layout, paint)
    // and an `in` line per scripted press, in the windowed loop's format.
    let mut stats = Stats::from_env();
    let mut pressed = 0usize;
    loop {
        let now = clock::secs();
        let t_begin = clock::mono_ns();
        let layouts = app.scene.layouts;
        app.update();
        app.frame(&mut gpu, now);
        app.paint(now, &mut gpu, &mut list);
        app.paint_extra(&mut gpu, &mut list);
        if stats.on() {
            let t_done = clock::mono_ns();
            stats.frames += 1;
            let laid = app.scene.layouts != layouts;
            let fno = stats.frames;
            stats.line(format_args!("f {} {} {} {} 0 0 {} {} {} {} 0 {}", fno, t_begin, t_done, (t_done - t_begin) / 1000, list.prims.len(), list.batches.len(), laid as u8, std::mem::take(&mut pressed), app.scene.animating(now) as u8));
            stats.maybe_flush();
        }
        if let Some(r) = app.take_card_shot(now) {
            let (cw, ch, px) = gpu.capture(&list, tok::BG);
            rare_native::features::stats::card_captured(cw, ch, px, r);
        }
        let calm = app.settled(now);
        let settled = (calm || (drive && last_input.elapsed() > Duration::from_secs(4))) && app.st.library.is_some() && started.elapsed() > Duration::from_millis(600) && last_input.elapsed() > Duration::from_millis(settle_ms) && now >= wait_until;
        if settled {
            if drive && script.is_empty() {
                use std::io::Write;
                let mut out = std::io::stdout().lock();
                loop {
                    let Some(Ok(line)) = stdin_lines.next() else { std::process::exit(0) };
                    let line = line.trim().to_string();
                    if line == "quit" {
                        std::process::exit(0);
                    } else if line == "state" {
                        let (_, _, px) = gpu.capture(&list, tok::BG);
                        let diff = prev_px.as_ref().map(|p| if p.len() != px.len() { -1 } else { p.chunks(4).zip(px.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 8)).count() as i64 }).unwrap_or(-1);
                        let _ = writeln!(out, "@@state {{\"diff\":{diff},\"calm\":{calm},\"app\":{}}}", app.crawl_state());
                        prev_px = Some(px);
                    } else if let Some(p) = line.strip_prefix("shot ") {
                        let (cw, ch, px) = gpu.capture(&list, tok::BG);
                        let _ = image::save_buffer(p, &px, cw, ch, image::ExtendedColorType::Rgba8);
                        let _ = writeln!(out, "@@shot");
                    } else if !line.is_empty() {
                        script.extend(parse_script(&line));
                        if !script.is_empty() {
                            break;
                        }
                        let _ = writeln!(out, "@@bad {line}");
                    }
                    let _ = out.flush();
                }
                continue;
            }
            match script.pop_front() {
                Some(Step::Press(b)) => {
                    stats.line(format_args!("in {} {} 0 {}", clock::mono_ns(), b.name(), clock::mono_ns()));
                    pressed += 1;
                    app.input(b, false);
                    last_input = Instant::now();
                    continue;
                }
                Some(Step::Wait(ms)) => {
                    wait_until = now + ms / 1000.0;
                    continue;
                }
                Some(Step::Text(t)) => {
                    app.type_text(&t);
                    last_input = Instant::now();
                    continue;
                }
                Some(Step::Touch(evs)) => {
                    // Played in real time, with frames in between (momentum and long presses run on the clock).
                    let t0 = Instant::now();
                    for (ph, x, y, at) in evs {
                        while t0.elapsed().as_secs_f64() * 1000.0 < at {
                            let now = clock::secs();
                            app.update();
                            app.frame(&mut gpu, now);
                            std::thread::sleep(Duration::from_millis(4));
                        }
                        app.touch(ph, 1, x, y);
                    }
                    last_input = Instant::now();
                    continue;
                }
                Some(Step::Pad { b, n, gap, hold }) => {
                    // Real time, a frame every 16 ms; presses and repeats as rare-input's pad
                    // state machine times them (320 ms, then 120 easing to 40 ms).
                    let t0 = Instant::now();
                    let ms = |t: Instant| t.elapsed().as_secs_f64() * 1000.0;
                    let mut next_log = 0.0;
                    for k in 0..n {
                        let start = k as f64 * gap;
                        while ms(t0) < start {
                            app.update();
                            app.frame(&mut gpu, clock::secs());
                            std::thread::sleep(Duration::from_millis(16));
                        }
                        app.input(b, false);
                        let mut held = rare_input::pad::Held::new(rare_input::Pad::Down, Instant::now());
                        while ms(t0) < start + hold {
                            if Instant::now() >= held.due() {
                                app.input(b, true);
                                held.fire(Instant::now());
                            }
                            let now = clock::secs();
                            app.update();
                            app.frame(&mut gpu, now);
                            app.paint(now, &mut gpu, &mut list);
                            if ms(t0) >= next_log {
                                next_log += 100.0;
                                eprintln!("pad step {:>5.0} ms: {}", ms(t0), app.focus_debug());
                            }
                            std::thread::sleep(Duration::from_millis(16));
                        }
                    }
                    eprintln!("pad step done: {}", app.focus_debug());
                    last_input = Instant::now();
                    continue;
                }
                Some(Step::Burst(offs)) => {
                    let Some(Step::Press(b)) = script.pop_front() else { continue };
                    let stem = shot.trim_end_matches(".png").to_string();
                    app.input(b, false);
                    let t0 = Instant::now();
                    for off in offs {
                        loop {
                            let now = clock::secs();
                            app.update();
                            app.frame(&mut gpu, now);
                            app.paint(now, &mut gpu, &mut list);
                            app.paint_extra(&mut gpu, &mut list);
                            if t0.elapsed().as_secs_f64() * 1000.0 >= off {
                                let (cw, ch, px) = gpu.capture(&list, tok::BG);
                                let p = format!("{stem}-{off:03}ms.png");
                                let _ = image::save_buffer(&p, &px, cw, ch, image::ExtendedColorType::Rgba8);
                                eprintln!("burst {p} at {:.0} ms: backdrop {:?}", t0.elapsed().as_secs_f64() * 1000.0, app.st.backdrop.iter().map(|b| b.0.clone()).collect::<Vec<_>>());
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(1));
                        }
                    }
                    last_input = Instant::now();
                    continue;
                }
                None => {}
            }
            stats.flush();
            let (cw, ch, px) = gpu.capture(&list, tok::BG);
            match image::save_buffer(&shot, &px, cw, ch, image::ExtendedColorType::Rgba8) {
                Ok(()) => eprintln!("golden image {shot} ({cw}x{ch}); {}", app.images.timing_report()),
                Err(e) => eprintln!("golden image {shot}: {e}"),
            }
            rare_native::kit::art::flush();
            return;
        }
        if started.elapsed() > Duration::from_secs(if drive { 1800 } else { 90 }) {
            eprintln!("timed out waiting for the screen to settle (images pending {})", app.images.pending());
            std::process::exit(3);
        }
        std::thread::sleep(Duration::from_millis(8));
    }
}

enum UserEv {
    Wake,
}

struct Main {
    proxy: EventLoopProxy<UserEv>,
    inputs: Arc<Mutex<Vec<In>>>,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    app: Option<App>,
    pending: Option<(TextSys, ImageLoader)>,
    list: DrawList,
    stats: Stats,
    first_done: bool,
    last_frame_ns: u64,
    next_clock: Instant,
    next_wake: Option<f64>,
    /// The tab to come back to after a rebuild (the UI size changed).
    resume_tab: Option<Tab>,
}

/// Settings > Display > UI size, on top of the automatic size (the device only).
fn ui_scale() -> f32 {
    if start0().fixture {
        return 1.0;
    }
    rare_native::screens::settings::display::scale_factor(rare_native::screens::settings::display::scale_of(&rare_native::screens::settings::config::load()))
}

impl Main {
    fn wake(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn build(&mut self) {
        let (Some(window), Some((text, images))) = (&self.window, self.pending.take()) else { return };
        let size = window.inner_size();
        let (w, h) = (size.width as f32, size.height as f32);
        let waker = {
            let proxy = self.proxy.clone();
            Arc::new(move || {
                let _ = proxy.send_event(UserEv::Wake);
            }) as Arc<dyn Fn() + Send + Sync>
        };
        let mut s0 = start0();
        if let Some(t) = self.resume_tab.take() {
            s0.tab = Some(t);
            s0.no_boot = true;
        }
        let be = backend(s0.fixture);
        self.app = Some(App::new(Scene::new(w, h, rem_for(w, h) * ui_scale()), text, images, be, waker, s0));
    }

    fn frame(&mut self) {
        let Some(mut gpu) = self.gpu.take() else { return };
        let t_begin = clock::mono_ns();
        let Some(frame) = gpu.acquire() else {
            self.gpu = Some(gpu);
            return;
        };
        let t_acq = clock::mono_ns();
        let evs: Vec<In> = std::mem::take(&mut *self.inputs.lock().unwrap());
        let Some(app) = self.app.as_mut() else {
            self.gpu = Some(gpu);
            return;
        };
        for e in &evs {
            // A repeat that waited for a frame (the window was hidden or busy) is from a hold that
            // may be over: only fresh repeats move focus.
            if e.repeat && clock::mono_ns().saturating_sub(e.t_ns) > STALE_REPEAT_NS {
                continue;
            }
            app.input(e.b, e.repeat);
            self.stats.line(format_args!("in {} {} {} {}", e.t_ns, e.b.name(), e.repeat as u8, clock::mono_ns()));
        }
        let now = clock::secs();
        let layouts = app.scene.layouts;
        self.next_wake = app.update();
        app.frame(&mut gpu, now);
        app.paint(now, &mut gpu, &mut self.list);
        app.paint_extra(&mut gpu, &mut self.list);
        if let Some(r) = app.take_card_shot(now) {
            let (cw, ch, px) = gpu.capture(&self.list, tok::BG);
            rare_native::features::stats::card_captured(cw, ch, px, r);
        }
        let laid = app.scene.layouts != layouts;
        let moving = app.scene.animating(now);
        let t_paint = clock::mono_ns();
        let submit_us = gpu.submit(frame, &self.list, tok::BG);
        app.presented();
        let t_done = clock::mono_ns();
        self.stats.frames += 1;
        let interval = if self.last_frame_ns > 0 { (t_done - self.last_frame_ns) / 1000 } else { 0 };
        self.last_frame_ns = t_done;
        let (np, nb) = (self.list.prims.len(), self.list.batches.len());
        let fno = self.stats.frames;
        self.stats.line(format_args!("f {} {} {} {} {} {} {} {} {} {} {} {}", fno, t_begin, t_done, (t_paint - t_acq) / 1000, (t_acq - t_begin) / 1000, submit_us, np, nb, laid as u8, evs.len(), interval, moving as u8));
        if !self.first_done {
            self.first_done = true;
            let ms = (t_done - clock::mark_start()) as f64 / 1e6;
            log::info!("first frame presented {ms:.1} ms after start ({np} prims, {nb} batches)");
            self.stats.line(format_args!("first {t_done} {ms:.1}"));
            self.stats.flush();
        }
        self.stats.maybe_flush();
        self.gpu = Some(gpu);
        if self.app.as_mut().is_some_and(|a| std::mem::take(&mut a.rescale)) {
            // The whole app is laid out in rem: rebuild at the new UI size and come back to Settings.
            let old = self.app.take().unwrap();
            self.pending = Some((old.text, old.images));
            self.resume_tab = Some(Tab::Settings);
            self.build();
            self.wake();
        }
    }
}

impl ApplicationHandler<UserEv> for Main {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let mut attrs = Window::default_attributes().with_title("Rare");
        let windowed = size_from_env();
        if cfg!(target_os = "linux") && windowed.is_none() {
            attrs = attrs.with_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
        } else {
            let (w, h) = windowed.unwrap_or((1280, 800));
            attrs = attrs.with_inner_size(winit::dpi::PhysicalSize::new(w, h)).with_resizable(false);
        }
        let window = Arc::new(el.create_window(attrs).expect("window"));
        log::info!("window: {}", platform::tag_window(&window, platform::RARE_APPID));
        rare_native::features::webgame::set_main_window(&window);
        // Rare over a running game (the device; not fixtures, not a bench build next to the live
        // launcher): rare-overlay raises this window over the game and takes the pad.
        if !start0().fixture && std::env::var("RR_NO_IP").is_err() {
            rare_native::features::ingame::install(window.clone());
            rare_native::data::backend::steam_at_start();
            // Power upkeep and per-game power, wake, idle dim and sleep, gyro, Wi-Fi power save,
            // the library watcher and the battery, OS update and dock notices.
            rare_native::features::services::start();
        }
        let size = window.inner_size();
        let gpu = Gpu::new(window.clone(), size.width, size.height);
        log::info!("gpu: {}", gpu.adapter_info);
        self.window = Some(window.clone());
        self.gpu = Some(gpu);
        self.build();
        window.request_redraw();
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, ev: WindowEvent) {
        match ev {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(sz) => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.resize(sz.width, sz.height);
                }
                let changed = self.app.as_ref().is_some_and(|a| a.scene.w != sz.width as f32 || a.scene.h != sz.height as f32);
                if changed {
                    // The whole app is laid out in rem: rebuild at the new size.
                    let old = self.app.take().unwrap();
                    self.pending = Some((old.text, old.images));
                    self.build();
                }
                self.wake();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                // With the keyboard up, typed characters go into it (development on a desktop).
                if self.app.as_ref().is_some_and(|a| a.st.osk.is_some()) {
                    if let Key::Character(c) = &event.logical_key {
                        if let Some(app) = self.app.as_mut() {
                            app.type_text(c);
                        }
                        self.wake();
                        return;
                    }
                }
                if let PhysicalKey::Code(k) = event.physical_key {
                    if let Some(b) = key_to_btn(k) {
                        self.inputs.lock().unwrap().push(In { b, repeat: event.repeat, t_ns: clock::mono_ns() });
                        self.wake();
                    }
                }
            }
            WindowEvent::Touch(t) => {
                if let Some(app) = self.app.as_mut() {
                    use winit::event::TouchPhase as P;
                    let ph = match t.phase {
                        P::Started => TouchPhase::Down,
                        P::Moved => TouchPhase::Move,
                        P::Ended => TouchPhase::Up,
                        P::Cancelled => TouchPhase::Cancel,
                    };
                    app.touch(ph, t.id, t.location.x as f32, t.location.y as f32);
                }
                self.wake();
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(app) = self.app.as_mut() {
                    app.mouse_move(position.x as f32, position.y as f32);
                }
                self.wake();
            }
            WindowEvent::MouseInput { state, button: winit::event::MouseButton::Left, .. } => {
                if let Some(app) = self.app.as_mut() {
                    app.mouse_button(state == ElementState::Pressed);
                }
                self.wake();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(app) = self.app.as_mut() {
                    match delta {
                        winit::event::MouseScrollDelta::LineDelta(x, y) => app.wheel(x, y, false),
                        winit::event::MouseScrollDelta::PixelDelta(p) => app.wheel(p.x as f32, p.y as f32, true),
                    }
                }
                self.wake();
            }
            // Focus moved (a game, the overlay, Steam): what the pad held before is let go.
            WindowEvent::Focused(_) => rare_input::reset::request("window focus"),
            WindowEvent::RedrawRequested => self.frame(),
            _ => {}
        }
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, _ev: UserEv) {
        self.wake();
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let now = clock::secs();
        if Instant::now() >= self.next_clock {
            self.next_clock = Instant::now() + Duration::from_secs_f32(platform::secs_to_next_minute());
            if let Some(app) = self.app.as_mut() {
                app.minute();
            }
            self.wake();
        }
        let moving = self.app.as_ref().is_some_and(|a| a.scene.animating(now) || a.nav.scrolling() || a.touch_busy());
        if moving {
            self.wake();
        } else {
            self.stats.flush();
        }
        let mut wake = self.next_clock;
        if let Some(at) = self.next_wake {
            if at <= now {
                self.wake();
            }
            wake = wake.min(Instant::now() + Duration::from_secs_f64((at - now).max(0.0)));
        }
        el.set_control_flow(ControlFlow::WaitUntil(wake));
    }
}

/// A queued repeat older than this is dropped (input.rs's FRESH, plus a frame).
const STALE_REPEAT_NS: u64 = 150_000_000;

fn main() {
    let t_start = clock::mark_start();
    let _ = log::set_logger(&Logger).map(|()| log::set_max_level(log::LevelFilter::Info));
    // legendary, gogdl and umu-run Rare ships (rare-native/bin, the image's stores folder) on PATH,
    // before any thread starts.
    rare_stores::tools::add_to_path();
    if let Ok(shot) = std::env::var("RR_SHOT") {
        headless(shot);
        return;
    }
    // `--dev-game <url>` (and --dev-slug, --dev-collection): Settings > Developer lists it.
    match rare_system::devmode::parse_args(std::env::args().skip(1)) {
        Ok(Some(g)) => rare_system::devmode::set_active(g),
        Ok(None) => {}
        Err(e) => log::warn!("{e}"),
    }
    let mut stats = Stats::from_env();
    stats.line(format_args!("start {t_start}"));
    let event_loop = EventLoop::<UserEv>::with_user_event().build().expect("event loop");
    let proxy = event_loop.create_proxy();
    let inputs: Arc<Mutex<Vec<In>>> = Arc::default();
    {
        let inputs = inputs.clone();
        let proxy = proxy.clone();
        let push = move |b: Btn, repeat: bool, t_ns: u64| {
            inputs.lock().unwrap().push(In { b, repeat, t_ns });
            let _ = proxy.send_event(UserEv::Wake);
        };
        // Network apps with no virtual pad (no /dev/uinput) press the launcher's own buttons.
        {
            let push = push.clone();
            rare_native::features::localapi::netpad::set_injector(Arc::new(move |p| push(Btn::from_pad(p), false, clock::mono_ns())));
        }
        if std::env::var("RR_NO_IP").is_ok() {
            // A bench build next to the running launcher: pads only, InputPlumber's buttons stay
            // with the launcher that owns them.
            rare_render::input::spawn(move |e| push(Btn::from_pad(e.pad), e.repeat, e.t_ns));
        } else {
            // The pads plus InputPlumber's D-Bus actions: the Rare button (ui_guide: press, and
            // held 0.6 s GuideHold), Quick (ui_quick), and the whole pad while a Rare surface
            // intercepts it over a game (App.tsx's guide / guidehold / qam). On the
            // device rare-overlay sees each press first (features/ingame.rs): the Rare button over
            // a game brings the Rare menu up over it, Quick its Quick settings.
            use rare_native::features::ingame;
            use rare_render::input::Input;
            rare_render::input::spawn_with(
                // A web game in front takes the pad first (features/webgame.rs): what comes back
                // is the launcher's (the Rare button, presses while a sheet is over the game).
                move |i| match {
                    // Any pad event is activity (idle dim and sleep).
                    rare_native::features::services::touch();
                    rare_native::features::webgame::pad_input(i)
                } {
                    None => {}
                    Some(Input::Press(e)) => {
                        if ingame::route_press(e.pad) {
                            push(Btn::from_pad(e.pad), e.repeat, e.t_ns)
                        }
                    }
                    Some(Input::Release(p)) => {
                        // A hold of X on Ask Rare ends: what was said is sent.
                        rare_native::features::agent::released(p);
                        ingame::route_release(p)
                    }
                    Some(Input::Exit) => {
                        if !ingame::exit_chord() {
                            push(Btn::ExitHold, false, clock::mono_ns())
                        }
                    }
                    Some(Input::Side(k, down)) => rare_native::side::press(k, down),
                    Some(Input::Share(s)) => rare_native::side::chord(s),
                    Some(_) => {}
                },
                rare_native::app::surface_up,
            );
        }
    }
    let waker = {
        let proxy = proxy.clone();
        Arc::new(move || {
            let _ = proxy.send_event(UserEv::Wake);
        }) as Arc<dyn Fn() + Send + Sync>
    };
    let images = rare_native::data::img_fetch::loader(3, waker);
    let text = TextSys::new(&FONTS, std::env::var("RR_NO_SYSTEM_FONTS").is_err(), 4096);
    let mut m = Main { proxy, inputs, window: None, gpu: None, app: None, pending: Some((text, images)), list: DrawList::default(), stats, first_done: false, last_frame_ns: 0, next_clock: Instant::now(), next_wake: None, resume_tab: None };
    event_loop.run_app(&mut m).expect("run");
    rare_native::features::ingame::shutdown();
    rare_native::features::capture::shutdown();
    rare_share::shutdown();
}
