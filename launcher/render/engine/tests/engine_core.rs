//! Engine core rendered for real on a headless adapter: box shadows, the image atlas, layers,
//! scroll offsets. Skips (passes) when the machine has no GPU adapter at all.

use rare_render::gpu::Gpu;
use rare_render::scene::{layer, Shadow};
use rare_render::taffy::prelude::*;
use rare_render::{Color, DrawList, ImageLoader, Node, Scene, TextSys};
use std::sync::Arc;

fn gpu(w: u32, h: u32) -> Option<Gpu> {
    let g = Gpu::headless(w, h);
    if g.is_none() {
        eprintln!("no adapter: skipped");
    }
    g
}

fn px(img: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [img[i], img[i + 1], img[i + 2], img[i + 3]]
}

fn abs(x: f32, y: f32, w: f32, h: f32) -> Style {
    Style {
        position: Position::Absolute,
        inset: Rect { left: length(x), top: length(y), right: auto(), bottom: auto() },
        size: Size { width: length(w), height: length(h) },
        ..Default::default()
    }
}

/// RR_TEST_SHOTS=<dir> keeps each test's frame as a PNG for a look.
fn keep(name: &str, w: u32, h: u32, img: &[u8]) {
    if let Ok(d) = std::env::var("RR_TEST_SHOTS") {
        let _ = image::save_buffer(format!("{d}/{name}.png"), img, w, h, image::ExtendedColorType::Rgba8);
    }
}

fn loader() -> ImageLoader {
    ImageLoader::new(1, Arc::new(|| {}), None)
}

const BG: Color = Color::rgb(16, 12, 24);

#[test]
fn toast_shadow_falls_below_and_is_cut_out_under_the_box() {
    let Some(mut g) = gpu(400, 300) else { return };
    let mut ts = TextSys::new(&[], false, 256);
    let images = loader();
    let mut s = Scene::new(400.0, 300.0, 16.0);
    s.n(s.root).bg.snap(BG);
    let toasts = s.layer(layer::TOAST);
    let l3 = Color::rgb(30, 25, 41);
    // Half transparent: CSS never shows the shadow through the box.
    let t = s.add(toasts, abs(100.0, 60.0, 200.0, 80.0), Node::bg(l3).radius(8.0).shadow(&Shadow::xl(0.3)).opacity(0.5));
    s.layout(&mut ts);
    let mut list = DrawList::default();
    s.paint(0.0, &mut ts, &images, &mut list);
    let (w, h, img) = g.capture(&list, BG);
    keep("toast_shadow", w, h, &img);
    let bg = px(&img, w, 5, 5);
    // Inside the box: exactly the box at 50 % over the page.
    let inside = px(&img, w, 200, 100);
    let want = [(16.0 + 30.0) / 2.0, (12.0 + 25.0) / 2.0, (24.0 + 41.0) / 2.0];
    for c in 0..3 {
        assert!((inside[c] as f32 - want[c]).abs() <= 1.5, "inside {inside:?} want {want:?}");
    }
    // Just under the box: darker than the page (the shadow is offset 20 px down).
    let below = px(&img, w, 200, 150);
    assert!(below[0] < bg[0] && below[2] < bg[2], "below {below:?} bg {bg:?}");
    // Above the box the -5 px spread and 20 px offset keep it clear.
    let above = px(&img, w, 200, 52);
    assert!((above[2] as i32 - bg[2] as i32).abs() <= 1, "above {above:?}");
    // Far away: untouched.
    assert_eq!(px(&img, w, 395, 295), bg);
    let _ = t;
}

#[test]
fn small_images_share_an_atlas_page() {
    let Some(mut g) = gpu(64, 64) else { return };
    let red: Vec<u8> = [255, 0, 0, 255].repeat(16 * 16);
    let blue: Vec<u8> = [0, 0, 255, 255].repeat(20 * 10);
    let a = g.upload_image(16, 16, &red);
    let b = g.upload_image(20, 10, &blue);
    assert_eq!(a.tex, b.tex, "one atlas page");
    assert_ne!(a.uv, b.uv);
    let big = g.upload_image(300, 10, &[255u8; 300 * 10 * 4]);
    assert_ne!(big.tex, a.tex, "large images get their own texture");
    assert_eq!(big.uv, [0.0, 0.0, 1.0, 1.0]);
    let (pages, fill) = g.atlas_stats();
    assert_eq!(pages, 1);
    assert!(fill > 0.0);

    // Draw both through the atlas: two prims, one batch, exact colours, no bleed at the edges.
    let mut list = DrawList::default();
    for (img, x) in [(a, 4.0), (b, 32.0)] {
        list.push(
            rare_render::Prim {
                rect: [x, 8.0, img.w as f32, img.h as f32],
                clip: [0.0, 0.0, 64.0, 64.0],
                c0: [1.0; 4],
                uv: img.uv,
                p: [0.0, 0.0, rare_render::gpu::KIND_IMAGE, 1.0],
                ..Default::default()
            },
            Some(img.tex),
        );
    }
    assert_eq!(list.batches.len(), 1);
    let (w, _, out) = g.capture(&list, Color::BLACK);
    assert_eq!(px(&out, w, 4, 8), [255, 0, 0, 255]);
    assert_eq!(px(&out, w, 19, 23), [255, 0, 0, 255]);
    assert_eq!(px(&out, w, 32, 8), [0, 0, 255, 255]);
    assert_eq!(px(&out, w, 51, 17), [0, 0, 255, 255]);
    assert_eq!(px(&out, w, 20, 8), [0, 0, 0, 255]);
    g.free_image(a);
    g.free_image(b);
}

#[test]
fn layers_paint_above_the_page_and_scroll_moves_children() {
    let Some(mut g) = gpu(200, 200) else { return };
    let mut ts = TextSys::new(&[], false, 256);
    let images = loader();
    let mut s = Scene::new(200.0, 200.0, 16.0);
    s.n(s.root).bg.snap(BG);
    let sheet = s.layer(layer::SHEET);
    let page_layer = s.layer(layer::PAGE);
    // Added to the root after the layers, still under them (z 0).
    let page = s.add(s.root, abs(0.0, 0.0, 200.0, 200.0), Node::default());
    let scroller = s.add(page, abs(0.0, 0.0, 100.0, 100.0), Node::default().scrolls());
    let mut col = abs(0.0, 0.0, 100.0, 400.0);
    col.position = Position::Relative;
    col.inset = Rect::auto();
    col.flex_shrink = 0.0;
    let content = s.add(scroller, Style { flex_direction: FlexDirection::Column, ..col }, Node::default());
    let green = Color::rgb(0, 200, 0);
    let red = Color::rgb(200, 0, 0);
    for i in 0..4 {
        let c = if i == 2 { red } else { green };
        let st = Style { size: Size { width: length(100.0), height: length(100.0) }, flex_shrink: 0.0, ..Default::default() };
        s.add(content, st, Node::bg(c));
    }
    s.add(sheet, abs(150.0, 150.0, 50.0, 50.0), Node::bg(Color::WHITE));
    s.add(page_layer, abs(140.0, 140.0, 50.0, 50.0), Node::bg(Color::BLACK));
    s.layout(&mut ts);
    assert_eq!(s.max_scroll(scroller), [0.0, 300.0]);
    assert_eq!(s.layer_of(content), layer::BASE);
    s.n(scroller).scroll_y.snap(200.0);
    let mut list = DrawList::default();
    s.paint(0.0, &mut ts, &images, &mut list);
    let (w, h, img) = g.capture(&list, BG);
    keep("layers_scroll", w, h, &img);
    // Scrolled by two rows: the red third row fills the viewport, and nothing leaks below it.
    assert_eq!(px(&img, w, 50, 50), [200, 0, 0, 255]);
    assert_eq!(px(&img, w, 50, 150), [16, 12, 24, 255]);
    // The sheet layer (z 40) over the page layer (z 30) where they overlap.
    assert_eq!(px(&img, w, 160, 160), [255, 255, 255, 255]);
    assert_eq!(px(&img, w, 145, 145), [0, 0, 0, 255]);
    // settled_rect follows the scroll target.
    let third = s.children(content)[2];
    assert_eq!(s.settled_rect(third)[1], 0.0);
}

#[test]
fn rounded_clip_rounds_children_but_not_the_cut_edge() {
    let Some(mut g) = gpu(200, 100) else { return };
    let mut ts = TextSys::new(&[], false, 256);
    let images = loader();
    let mut s = Scene::new(200.0, 100.0, 16.0);
    s.n(s.root).bg.snap(Color::BLACK);
    // A rail clipping at x = 100; a rounded card (r 20) half out of it, filled by a square child.
    let rail = s.add(s.root, abs(0.0, 0.0, 100.0, 100.0), Node::default().clip());
    let card = s.add(rail, abs(60.0, 10.0, 80.0, 80.0), Node::default().radius(20.0).clip());
    s.add(card, abs(0.0, 0.0, 80.0, 80.0), Node::bg(Color::WHITE));
    s.layout(&mut ts);
    let mut list = DrawList::default();
    s.paint(0.0, &mut ts, &images, &mut list);
    let (w, h, img) = g.capture(&list, Color::BLACK);
    keep("rounded_clip", w, h, &img);
    // The card's own top-left corner is rounded off.
    assert_eq!(px(&img, w, 61, 11), [0, 0, 0, 255]);
    // Inside: white.
    assert_eq!(px(&img, w, 80, 50), [255, 255, 255, 255]);
    // The rail's cut at x = 100 is square right up to the card's top edge.
    assert_eq!(px(&img, w, 99, 11), [255, 255, 255, 255]);
    // Beyond the rail: nothing.
    assert_eq!(px(&img, w, 101, 50), [0, 0, 0, 255]);
}

/// Paint and diff cost of a Home-sized view (about 400 nodes, 150 of them text). Run with
/// `cargo test --release -p rare-render --test engine_core -- --ignored --nocapture bench`.
#[test]
#[ignore]
fn bench_paint_and_diff() {
    use rare_render::view::{key, El, View};
    use rare_render::TextStyle;
    static INTER: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
    let mut ts = TextSys::new(&[INTER], false, 2048);
    let images = loader();
    let mut s = Scene::new(1920.0, 1200.0, 24.0);
    let mut v = View::new(s.root);
    let st = TextStyle::new(17.0, 500);
    let build = |n: u32| {
        El::col().kids((0..6).map(|row| {
            El::row().gap(16.0).key(key(&row)).clip().kids((0..25).map(|i| {
                let id = row * 100 + i;
                El::col()
                    .key(key(&id))
                    .with_style(Style { size: Size { width: length(132.0), height: length(170.0) }, flex_shrink: 0.0, flex_direction: FlexDirection::Column, ..Default::default() })
                    .bg(Color::rgb(30, 25, 41))
                    .radius(12.0)
                    .child(El::text(format!("Game {id} {}", if id == n { "*" } else { "" }), st, Color::WHITE))
            }))
        }))
    };
    let t = std::time::Instant::now();
    v.update(&mut s, &mut ts, build(0), 0.0);
    s.layout(&mut ts);
    let mount = t.elapsed();
    let mut list = DrawList::default();
    s.paint(0.0, &mut ts, &images, &mut list);
    let n = 200;
    let t = std::time::Instant::now();
    for i in 0..n {
        s.paint(i as f64 * 0.016, &mut ts, &images, &mut list);
    }
    let paint = t.elapsed() / n;
    let t = std::time::Instant::now();
    for i in 0..n {
        v.update(&mut s, &mut ts, build(i % 150), 0.0);
    }
    let diff = t.elapsed() / n;
    let t = std::time::Instant::now();
    for _ in 0..n {
        v.update(&mut s, &mut ts, build(9999), 0.0);
    }
    let same = t.elapsed() / n;
    println!("nodes {} prims {} batches {} | mount+layout {:?} | paint {:?} | diff, one text changed {:?} | diff, nothing changed {:?}", v.len(), list.prims.len(), list.batches.len(), mount, paint, diff, same);
}

#[test]
fn trimmed_images_come_back_when_drawn_again() {
    use rare_render::{Fit, ImgReq, ImgState, Src};
    let Some(mut g) = gpu(64, 64) else { return };
    let mut png = Vec::new();
    let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([10, 200, 30, 255]));
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let bytes: &'static [u8] = Box::leak(png.into_boxed_slice());
    let mut l = loader();
    let k = l.request(ImgReq { src: Src::Static("t.png", bytes), w: 8, h: 8, fit: Fit::Contain });
    let wait = |l: &mut ImageLoader, g: &mut Gpu| {
        for _ in 0..500 {
            l.poll(g);
            if let ImgState::Ready { .. } = l.state(k) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        false
    };
    assert!(wait(&mut l, &mut g));
    assert_eq!(l.bytes(), 8 * 8 * 4);
    // Recently drawn: kept.
    assert_eq!(l.trim(&mut g, 0, 60.0), 0);
    // Idle and over budget: freed, then Pending, then back once asked for again.
    assert_eq!(l.trim(&mut g, 0, 0.0), 256);
    assert_eq!(l.bytes(), 0);
    assert!(matches!(l.state(k), ImgState::Pending));
    assert_eq!(l.pending(), 0, "evicted images do not hold up goldens");
    assert!(wait(&mut l, &mut g));
    assert_eq!(l.bytes(), 256);
}

/// WS 3 / WS 12 / WS 15: per-corner radii, one-sided borders and an Oklab gradient at an angle.
#[test]
fn corners_sides_and_oklab_gradients() {
    use rare_render::LinearGradient;
    let Some(mut g) = gpu(400, 300) else { return };
    let mut ts = TextSys::new(&[], false, 256);
    let images = loader();
    let mut s = Scene::new(400.0, 300.0, 16.0);
    s.n(s.root).bg.snap(BG);
    let red = Color::rgb(255, 0, 0);
    let blue = Color::rgb(0, 0, 255);
    s.add(s.root, abs(20.0, 20.0, 100.0, 60.0), Node::bg(Color::WHITE).corners([20.0, 0.0, 0.0, 0.0]));
    s.add(s.root, abs(140.0, 20.0, 100.0, 60.0), Node::default().borders([0.0, 0.0, 4.0, 0.0], red));
    s.add(s.root, abs(20.0, 100.0, 200.0, 40.0), Node::default().lgrad(LinearGradient::new(90.0).stop(red, 0.0).stop(blue, 1.0)));
    s.add(s.root, abs(20.0, 160.0, 200.0, 40.0), Node::default().lgrad(LinearGradient::new(90.0).stop(red, 0.0).stop(blue, 1.0).srgb()));
    s.layout(&mut ts);
    let mut list = DrawList::default();
    s.paint(0.0, &mut ts, &images, &mut list);
    let (w, h, img) = g.capture(&list, BG);
    keep("corners_sides_oklab", w, h, &img);
    let bg = px(&img, w, 5, 5);
    let white = [255, 255, 255, 255];
    assert_eq!(px(&img, w, 21, 21), bg, "top-left is round");
    for (x, y) in [(118, 21), (118, 78), (21, 78)] {
        assert_eq!(px(&img, w, x, y), white, "square corner at {x},{y}");
    }
    assert_eq!(px(&img, w, 190, 40), bg, "no fill");
    assert_eq!(px(&img, w, 142, 40), bg, "no left border");
    assert_eq!(px(&img, w, 190, 78), [255, 0, 0, 255], "bottom border");
    let mid = px(&img, w, 120, 120);
    for (c, want) in [140, 83, 162].iter().enumerate() {
        assert!((mid[c] as i32 - want).abs() <= 3, "oklab mid {mid:?}");
    }
    let mid = px(&img, w, 120, 180);
    assert!(mid[1] < 4 && (mid[0] as i32 - 128).abs() <= 3, "srgb mid {mid:?}");
}

/// WS 15: a paint slot draws in scene order: clipped by its scroll container, under a sheet.
#[test]
fn paint_slots_draw_in_scene_order() {
    use rare_render::gpu::{Prim, KIND_RECT};
    let Some(mut g) = gpu(400, 300) else { return };
    let mut ts = TextSys::new(&[], false, 256);
    let images = loader();
    let mut s = Scene::new(400.0, 300.0, 16.0);
    s.n(s.root).bg.snap(BG);
    let pane = s.add(s.root, abs(50.0, 50.0, 200.0, 100.0), Node::default().clip());
    let chart = s.add(pane, abs(0.0, 50.0, 200.0, 100.0), Node::default().slot(7));
    let sheet = s.layer(layer::SHEET);
    s.add(sheet, abs(150.0, 0.0, 250.0, 300.0), Node::bg(Color::WHITE));
    s.layout(&mut ts);
    let mut list = DrawList::default();
    let mut seen = Vec::new();
    s.paint_with(0.0, &mut ts, &images, &mut list, &mut |sp, _, out| {
        seen.push((sp.slot, sp.id, sp.rect));
        out.push(Prim { rect: sp.rect, clip: [-1e6, -1e6, 1e6, 1e6], c0: [1.0, 0.0, 0.0, 1.0], p: [0.0, 0.0, KIND_RECT, sp.opacity], ..Default::default() }, None);
    });
    assert_eq!(seen, vec![(7, chart, [50.0, 100.0, 200.0, 100.0])]);
    let (w, h, img) = g.capture(&list, BG);
    keep("paint_slots", w, h, &img);
    assert_eq!(px(&img, w, 100, 120), [255, 0, 0, 255], "the chart");
    assert_eq!(px(&img, w, 100, 170), px(&img, w, 5, 5), "clipped by its pane");
    assert_eq!(px(&img, w, 200, 120), [255, 255, 255, 255], "under the sheet");
}

/// WS 15: a streaming texture updates in place (same id) and shows on a node like an image.
#[test]
fn stream_textures_update_in_place() {
    let Some(mut g) = gpu(200, 100) else { return };
    let mut ts = TextSys::new(&[], false, 256);
    let images = loader();
    let mut s = Scene::new(200.0, 100.0, 16.0);
    let tex = g.create_stream(2, 2);
    assert!(g.write_texture(tex, 2, 2, &[255, 0, 0, 255].repeat(4)));
    s.add(s.root, abs(0.0, 0.0, 100.0, 100.0), Node::default().image_tex(tex, 2, 2));
    s.layout(&mut ts);
    let mut list = DrawList::default();
    s.paint(0.0, &mut ts, &images, &mut list);
    let (w, _, img) = g.capture(&list, BG);
    assert_eq!(px(&img, w, 50, 50), [255, 0, 0, 255]);
    // Next frame: new pixels into the same texture, then a new size under the same id.
    assert!(g.write_texture(tex, 2, 2, &[0, 255, 0, 255].repeat(4)));
    let (w, _, img) = g.capture(&list, BG);
    assert_eq!(px(&img, w, 50, 50), [0, 255, 0, 255]);
    assert!(g.write_texture(tex, 4, 2, &[0, 0, 255, 255].repeat(8)));
    assert_eq!(g.texture_size(tex), Some((4, 2)));
    let (w, _, img) = g.capture(&list, BG);
    assert_eq!(px(&img, w, 50, 50), [0, 0, 255, 255]);
    assert!(!g.write_texture(0, 1, 1, &[0; 4]), "the white texture is not writable");
}
