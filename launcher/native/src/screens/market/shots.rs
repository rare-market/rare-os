//! Golden images of the Phase 4 pages (Trade, Marketplace, Store, Friends, Share), headless.
//! Ignored by default (needs a GPU adapter):
//!   RR_SHOT_DIR=out cargo test -- screens::market::shots --ignored --nocapture
//! RR_SHOT_SIZE=1920x1200 (default), RR_SHOT_ONLY=trade-perp,market-item, RR_SCRIPT=down,a.
//! Pages are built into the shell's main area (under the 4.25 rem top bar, over the 3 rem hint bar),
//! so a shot lines up with the web launcher's screenshot of the same page.

use super::host::Cx;
use rare_render::input::Pad;
use rare_render::theme::{rem_for, tok};
use rare_render::{clock, DrawList, Gpu, ImageLoader, Scene, TextSys};
use std::sync::Arc;
use std::time::{Duration, Instant};

static FONTS: [&[u8]; 5] = [
    include_bytes!("../../../../render/assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../../../render/assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../../../render/assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../../../render/assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../../../../render/assets/fonts/FiraCode-Medium.ttf"),
];

pub fn pad_of(s: &str) -> Option<Pad> {
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
        "select" => Pad::Select,
        "start" => Pad::Start,
        _ => return None,
    })
}

/// Renders one Phase 4 page fixture, plays `script`, settles and writes `path`.
pub fn shoot(name: &str, w: u32, h: u32, script: &[Pad], path: &str) -> Result<(), String> {
    let mut gpu = Gpu::headless(w, h).ok_or("no headless adapter")?;
    let mut text = TextSys::new(&FONTS, false, 2048);
    let cache = std::env::temp_dir().join("rr-ws15-img");
    let mut images = ImageLoader::new(3, Arc::new(|| {}), Some(cache));
    let mut scene = Scene::new(w as f32, h as f32, rem_for(w as f32, h as f32));
    scene.ring_gap_color = tok::BG;
    scene.ring_color = tok::FG;
    scene.n(scene.root).bg.snap(tok::BG);
    let now = clock::secs();
    let mut page = {
        let mut cx = Cx { scene: &mut scene, t: &mut text, img: &mut images };
        let main = crate::screens::trade::main_area(&mut cx);
        super::any_page(name, &mut cx, main, now).ok_or_else(|| format!("unknown page {name}"))?
    };
    for p in script {
        let mut cx = Cx { scene: &mut scene, t: &mut text, img: &mut images };
        page.pad(&mut cx, *p, clock::secs());
    }
    let mut list = DrawList::default();
    let started = Instant::now();
    loop {
        let now = clock::secs();
        images.poll(&mut gpu);
        scene.sync_images(now, &images, 0.0);
        {
            let mut cx = Cx { scene: &mut scene, t: &mut text, img: &mut images };
            page.tick(&mut cx, now);
        }
        scene.layout(&mut text);
        page.prepare_paint(&scene, &mut text, &mut gpu);
        scene.paint_with(now, &mut text, &images, &mut list, &mut |sp, ts, out| page.paint_slot(sp, ts, out));
        page.paint_extra(&scene, &mut text, &mut gpu, &mut list);
        for (slot, data) in text.pending.drain(..) {
            gpu.upload_glyph(slot.x, slot.y, slot.w, slot.h, &data);
        }
        let busy = scene.animating(now) || images.pending() > 0;
        if (!busy && started.elapsed() > Duration::from_millis(300)) || started.elapsed() > Duration::from_secs(25) {
            let (cw, ch, px) = gpu.capture(&list, tok::BG);
            image::save_buffer(path, &px, cw, ch, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
            eprintln!("{path} ({cw}x{ch}, {} prims, hints {:?})", list.prims.len(), page.hints());
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(16));
    }
}

#[test]
#[ignore]
fn shots() {
    let dir = std::env::var("RR_SHOT_DIR").unwrap_or_else(|_| "shots".into());
    std::fs::create_dir_all(&dir).unwrap();
    let (w, h) = std::env::var("RR_SHOT_SIZE")
        .ok()
        .and_then(|v| {
            let (a, b) = v.split_once('x')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        })
        .unwrap_or((1920, 1200));
    let only: Option<Vec<String>> = std::env::var("RR_SHOT_ONLY").ok().map(|o| o.split(',').map(str::to_string).collect());
    let script: Vec<Pad> = std::env::var("RR_SCRIPT").unwrap_or_default().split(',').filter_map(pad_of).collect();
    let names: Vec<String> = only.unwrap_or_else(|| super::all_shots().iter().map(|s| s.to_string()).collect());
    for name in names {
        shoot(&name, w, h, &script, &format!("{dir}/{name}-{w}x{h}.png")).unwrap();
    }
}
