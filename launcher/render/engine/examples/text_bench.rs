//! Text timings on this machine: start, first shape, cold rasterisation, warm paint.
//!   cargo run --release -p rare-render --example text_bench
use rare_render::gpu::DrawList;
use rare_render::text::{TextPaint, TextStyle, TextSys};
use rare_render::Color;
use std::time::Instant;

fn main() {
    let t0 = Instant::now();
    let mut t = TextSys::new(&[], true, 2048);
    let start = t0.elapsed();
    let lines = ["Continue playing", "Home Library Sui Market Wallet", "The quick brown fox jumps over the lazy dog 0123456789", "Played 2 hours ago on Steam, 14.2 GB, controller ready", "1,234,567.89 +12.40% 0.000123 SUI $3.4812"];
    let styles = [TextStyle::new(22.5, 400).line(33.0), TextStyle::new(25.5, 500).line(36.0), TextStyle::new(66.0, 600).line(72.0), TextStyle::new(18.0, 400).line(24.0).tabular(), TextStyle::new(22.5, 400).line(33.0).mono()];
    let t1 = Instant::now();
    let mut bufs = Vec::new();
    for st in &styles {
        for l in &lines {
            let mut b = t.buffer(l, st);
            t.measure(&mut b, None);
            bufs.push(b);
        }
    }
    let shape = t1.elapsed();
    let mut out = DrawList::default();
    let t2 = Instant::now();
    for b in &bufs {
        t.paint(b, &TextPaint::at(0.0, 0.0, Color::WHITE), &mut out);
    }
    let cold = t2.elapsed();
    let glyphs = t.glyph_count();
    out.clear();
    let t3 = Instant::now();
    for b in &bufs {
        t.paint(b, &TextPaint::at(0.0, 0.0, Color::WHITE), &mut out);
    }
    let warm = t3.elapsed();
    println!(
        "start {:.2} ms | shape+measure {} buffers {:.2} ms | cold paint {} glyph rasters {:.2} ms ({:.1} us each) | warm paint {} quads {:.3} ms",
        start.as_secs_f64() * 1e3,
        bufs.len(),
        shape.as_secs_f64() * 1e3,
        glyphs,
        cold.as_secs_f64() * 1e3,
        cold.as_secs_f64() * 1e6 / glyphs as f64,
        out.prims.len(),
        warm.as_secs_f64() * 1e3
    );

    // A fallback face on first use: lazy load, then the first CJK and emoji glyphs.
    for text in ["ゲームを続ける 漢字", "ok 😀🎮"] {
        let t4 = Instant::now();
        let b = t.buffer(text, &TextStyle::new(22.5, 400).line(33.0));
        let load = t4.elapsed();
        let t5 = Instant::now();
        out.clear();
        t.paint(&b, &TextPaint::at(0.0, 0.0, Color::WHITE), &mut out);
        println!("{text}: fallback load + shape {:.1} ms, first paint {:.1} ms, files {:?}", load.as_secs_f64() * 1e3, t5.elapsed().as_secs_f64() * 1e3, t.fallback_files());
    }
}
