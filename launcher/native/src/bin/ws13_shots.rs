//! Dev harness for the Phase 2 screens: builds one fixture screen, plays an input script and writes
//! a golden image, all headless (no window). RR_SCREEN=<name> RR_SCRIPT=down,a RR_SHOT=out.png
//! RR_WINDOWED=1920x1200 RR_NO_SYSTEM_FONTS=1. RR_SHOT_EXIT is accepted and ignored. Temporary:
//! the test-infra harness replaces it.

use rare_native::screens::{self, overlay::{Cx, Screen}};
use rare_render::input::Pad;
use rare_render::theme::{rem_for, tok};
use rare_render::{clock, DrawList, Gpu, ImageLoader, Scene, TextSys};
use std::sync::Arc;
use std::time::{Duration, Instant};

static FONTS: [&[u8]; 5] = [
    include_bytes!("../../../render/assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../../render/assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../../render/assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../../render/assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../../../render/assets/fonts/FiraCode-Medium.ttf"),
];

fn pad_of(s: &str) -> Option<Pad> {
    Some(match s.trim() {
        "up" => Pad::Up,
        "down" => Pad::Down,
        "left" => Pad::Left,
        "right" => Pad::Right,
        "a" => Pad::A,
        "b" => Pad::B,
        "x" => Pad::X,
        "y" => Pad::Y,
        "lb" => Pad::Lb,
        "rb" => Pad::Rb,
        "lt" => Pad::Lt,
        "rt" => Pad::Rt,
        _ => return None,
    })
}

fn main() {
    let name = std::env::var("RR_SCREEN").unwrap_or_else(|_| "power".into());
    let (w, h) = std::env::var("RR_WINDOWED")
        .ok()
        .and_then(|v| {
            let (a, b) = v.split_once('x')?;
            Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?))
        })
        .unwrap_or((1920, 1200));
    let path = std::env::var("RR_SHOT").unwrap_or_else(|_| "shot.png".into());
    let script: Vec<Pad> = std::env::var("RR_SCRIPT").unwrap_or_default().split(',').filter_map(pad_of).collect();
    let mut gpu = Gpu::headless(w, h).expect("headless gpu");
    let mut text = TextSys::new(&FONTS, std::env::var("RR_NO_SYSTEM_FONTS").is_err(), 2048);
    let cache = std::env::var("RR_IMG_CACHE").ok().map(std::path::PathBuf::from);
    let mut images = ImageLoader::new(3, Arc::new(|| {}), cache);
    let mut scene = Scene::new(w as f32, h as f32, rem_for(w as f32, h as f32));
    scene.ring_gap_color = tok::BG;
    scene.ring_color = tok::FG;
    scene.n(scene.root).bg.snap(tok::BG);
    let mut screen: Box<dyn Screen> = {
        let mut cx = Cx { scene: &mut scene, t: &mut text, img: &mut images };
        match screens::fixture(&name, &mut cx, clock::secs()) {
            Some(s) => s,
            None => {
                eprintln!("unknown screen {name}");
                std::process::exit(2);
            }
        }
    };
    let mut list = DrawList::default();
    let mut script = script.into_iter();
    let started = Instant::now();
    let mut last_input = Instant::now();
    loop {
        let now = clock::secs();
        images.poll(&mut gpu);
        scene.sync_images(now, &images, 0.12);
        {
            let mut cx = Cx { scene: &mut scene, t: &mut text, img: &mut images };
            screen.tick(&mut cx, now);
        }
        scene.layout(&mut text);
        scene.paint(now, &mut text, &images, &mut list);
        for (slot, data) in text.pending.drain(..) {
            gpu.upload_glyph(slot.x, slot.y, slot.w, slot.h, &data);
        }
        let busy = scene.animating(now) || images.pending() > 0 || screen.next_deadline().is_some_and(|d| d <= now);
        let settled = !busy && started.elapsed() > Duration::from_millis(400) && last_input.elapsed() > Duration::from_millis(450);
        if settled {
            if let Some(p) = script.next() {
                let mut cx = Cx { scene: &mut scene, t: &mut text, img: &mut images };
                screen.pad(&mut cx, p, clock::secs());
                last_input = Instant::now();
                continue;
            }
            let (cw, ch, px) = gpu.capture(&list, tok::BG);
            match image::save_buffer(&path, &px, cw, ch, image::ExtendedColorType::Rgba8) {
                Ok(()) => eprintln!("golden image {path} ({cw}x{ch})"),
                Err(e) => eprintln!("golden image {path}: {e}"),
            }
            return;
        }
        if started.elapsed() > Duration::from_secs(60) {
            eprintln!("timed out waiting for the scene to settle");
            std::process::exit(3);
        }
        std::thread::sleep(Duration::from_millis(16));
    }
}
