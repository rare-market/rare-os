//! Shaped text to glyph quads. Pens keep their fractional x (the rasteriser bins it to a quarter
//! pixel, like Skia), baselines snap to whole device pixels, and scaled text is rasterised at its
//! drawn size (quantised to a quarter pixel so a scale animation does not fill the atlas), so a
//! focused tile's label is as sharp as the browser's.

use super::{raster, TextSys};
use crate::color::Color;
use crate::gpu::{DrawList, Prim, KIND_GLYPH, KIND_IMAGE};
use cosmic_text::{Buffer, CacheKey, LayoutGlyph};

/// Where and how a buffer is drawn.
#[derive(Clone, Copy, Debug)]
pub struct TextPaint {
    /// Top-left of the text box in device pixels.
    pub x: f32,
    pub y: f32,
    /// Scale about the box's top-left (scene transforms). Glyphs rasterise at this scale.
    pub scale: f32,
    /// Vertical scale when it differs from `scale` (a scaleY transition): quads stretch.
    pub scale_y: f32,
    pub clip: [f32; 4],
    pub color: Color,
    /// Opacity of the node (multiplies everything).
    pub opacity: f32,
}

impl TextPaint {
    pub fn at(x: f32, y: f32, color: Color) -> TextPaint {
        TextPaint { x, y, scale: 1.0, scale_y: 1.0, clip: [-1e6, -1e6, 1e6, 1e6], color, opacity: 1.0 }
    }

    /// The same paint scaled by (sx, sy).
    pub fn scaled(self, sx: f32, sy: f32) -> TextPaint {
        TextPaint { scale: sx, scale_y: sy, ..self }
    }
}

/// A per-glyph offset and fade (number rolls, carets of composition...).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphFx {
    pub dx: f32,
    pub dy: f32,
    pub alpha: f32,
}

impl GlyphFx {
    pub const NONE: GlyphFx = GlyphFx { dx: 0.0, dy: 0.0, alpha: 1.0 };
}

/// How the browser places the baseline inside a line box (measured in headless WebKit and Chrome
/// on the specimen, every size the launcher uses).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Baseline {
    /// WebKit (the launcher's WebKitGTK on the device, Safari): the font's ascent and descent
    /// unrounded, centred in the line height; the baseline rounds to the nearest pixel.
    #[default]
    WebKit,
    /// Blink (headless Chrome, what scripts/shots.mjs captures): ascent and descent rounded to whole
    /// pixels first, the half-leading floored.
    Blink,
}

/// Baseline offset from the line top, before snapping to device pixels (floor of x + 0.5).
pub fn baseline_in_line(line_height: f32, ascent: f32, descent: f32, rule: Baseline) -> f32 {
    match rule {
        Baseline::WebKit => ascent + (line_height - (ascent + descent)) / 2.0,
        Baseline::Blink => {
            let (a, d) = ((ascent + 0.5).floor(), (descent + 0.5).floor());
            a + ((line_height - (a + d)) / 2.0).floor()
        }
    }
}

/// Font size the glyph is rasterised at for a drawn size.
pub(crate) fn raster_size(drawn: f32) -> f32 {
    ((drawn * 4.0).round() / 4.0).max(0.25)
}

impl TextSys {
    /// Ascent and descent of a face in em (cached).
    fn vmetrics(&mut self, id: cosmic_text::fontdb::ID, w: cosmic_text::fontdb::Weight) -> Option<(f32, f32)> {
        if let Some(m) = self.vmetrics.get(&id) {
            return Some(*m);
        }
        let f = self.fs.get_font(id, w)?;
        let m = f.metrics();
        let upm = m.units_per_em as f32;
        let v = (m.ascent / upm, -m.descent / upm);
        self.vmetrics.insert(id, v);
        Some(v)
    }

    pub fn paint(&mut self, buf: &Buffer, tp: &TextPaint, out: &mut DrawList) {
        self.paint_fx(buf, tp, out, |_, _| GlyphFx::NONE)
    }

    /// Like `paint`, with `fx(line index, glyph)` moving or fading single glyphs.
    pub fn paint_fx(&mut self, buf: &Buffer, tp: &TextPaint, out: &mut DrawList, mut fx: impl FnMut(usize, &LayoutGlyph) -> GlyphFx) {
        let bucket = raster::bucket(tp.color);
        let rule = self.baseline;
        let clamp = super::clamp_of(buf);
        let runs = if clamp > 0 { buf.layout_runs().count() } else { 0 };
        let width = buf.size().0.unwrap_or(f32::MAX);
        let mut prev_line = usize::MAX;
        for (ri, run) in buf.layout_runs().enumerate() {
            if clamp > 0 && ri >= clamp {
                break;
            }
            // A wrapped continuation never starts with the space it broke at.
            let continued = run.line_i == prev_line;
            prev_line = run.line_i;
            let space = |g: &LayoutGlyph| run.text.get(g.start..g.end).is_some_and(|t| !t.is_empty() && t.chars().all(char::is_whitespace));
            let lead = if continued { run.glyphs.iter().take_while(|g| space(g)).count() } else { 0 };
            let shift = run.glyphs[..lead].iter().map(|g| g.w).sum::<f32>();
            let mut end = run.glyphs.len();
            let mut ellipsis = None;
            if clamp > 0 && ri + 1 == clamp && runs > clamp {
                // -webkit-line-clamp: the last kept line as wrapped, then characters come off its
                // end until "…" fits, and the space before it goes too.
                if let Some(last) = run.glyphs.last() {
                    if let Some((gid, adv)) = self.ellipsis_glyph(last) {
                        let right = |e: usize| run.glyphs[..e].last().map_or(0.0, |g| g.x + g.w - shift);
                        while end > lead && space(&run.glyphs[end - 1]) {
                            end -= 1;
                        }
                        while end > lead && right(end) + adv > width {
                            end -= 1;
                            while end > lead && space(&run.glyphs[end - 1]) {
                                end -= 1;
                            }
                        }
                        let mut g = last.clone();
                        g.glyph_id = gid;
                        g.x = right(end) + shift;
                        g.w = adv;
                        g.x_offset = 0.0;
                        g.y_offset = 0.0;
                        g.start = run.glyphs.get(end).map_or(last.end, |g| g.start);
                        g.end = g.start;
                        ellipsis = Some(g);
                    }
                }
            }
            let base = match run.glyphs.first().and_then(|g| self.vmetrics(g.font_id, g.font_weight)) {
                Some((asc, desc)) => {
                    let size = run.glyphs[0].font_size;
                    let local = run.line_top + baseline_in_line(run.line_height, asc * size, desc * size, rule);
                    (tp.y + local * tp.scale_y + 0.5).floor()
                }
                None => (tp.y + run.line_y * tp.scale_y).round(),
            };
            for g in run.glyphs[lead..end].iter().chain(ellipsis.as_ref()) {
                let f = fx(run.line_i, g);
                self.draw_glyph(g, base, shift, tp, bucket, f, out);
            }
        }
    }

    /// The ellipsis glyph of `like`'s face and its advance at `like`'s size.
    fn ellipsis_glyph(&mut self, like: &LayoutGlyph) -> Option<(u16, f32)> {
        let key = (like.font_id, like.font_weight.0, like.font_size.to_bits());
        if let Some(e) = self.ellipses.get(&key) {
            return *e;
        }
        let e = (|| {
            use skrifa::MetadataProvider;
            let font = self.fs.get_font(like.font_id, like.font_weight)?;
            let index = self.fs.db().face(like.font_id)?.index;
            let f = skrifa::FontRef::from_index(font.data(), index).ok()?;
            let gid = f.charmap().map('…')?;
            let loc = f.axes().location([("wght", like.font_weight.0 as f32)]);
            let adv = f.glyph_metrics(skrifa::instance::Size::new(like.font_size), &loc).advance_width(gid)?;
            Some((gid.to_u32() as u16, adv))
        })();
        self.ellipses.insert(key, e);
        e
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_glyph(&mut self, g: &LayoutGlyph, base: f32, shift: f32, tp: &TextPaint, bucket: u8, f: GlyphFx, out: &mut DrawList) {
        let f = GlyphFx { alpha: f.alpha * super::span_opacity(g.metadata), ..f };
        if f.alpha <= 0.0 {
            return;
        }
        let a = self.atlas_size as f32;
        let ry = tp.scale_y / tp.scale;
        let drawn = g.font_size * tp.scale;
        let rsize = raster_size(drawn);
        let k = drawn / rsize;
        let pen_x = tp.x + (g.x - shift + g.font_size * g.x_offset) * tp.scale + f.dx;
        let pen_y = ((g.y - g.font_size * g.y_offset) * tp.scale).trunc();
        let (key, ix, iy) = CacheKey::new(g.font_id, g.glyph_id, rsize, (pen_x, pen_y), g.font_weight, g.cache_key_flags);
        let (px, py) = (ix as f32, base + iy as f32 * ry + f.dy);
        let color = match g.color_opt {
            Some(c) => Color { r: c.r() as f32 / 255.0, g: c.g() as f32 / 255.0, b: c.b() as f32 / 255.0, a: c.a() as f32 / 255.0 * tp.color.a },
            None => tp.color,
        };
        if let Some(s) = self.glyph_lum(key, bucket) {
            out.push(
                Prim {
                    rect: [px + s.left as f32 * k, py - s.top as f32 * k * ry, s.w as f32 * k, s.h as f32 * k * ry],
                    clip: tp.clip,
                    c0: color.alpha(f.alpha).arr(),
                    uv: [s.x as f32 / a, s.y as f32 / a, (s.x + s.w) as f32 / a, (s.y + s.h) as f32 / a],
                    p: [0.0, 0.0, KIND_GLYPH, tp.opacity],
                    ..Default::default()
                },
                None,
            );
        } else if let Some(c) = self.color_glyph(&key) {
            let Some(tex) = c.tex else {
                self.want_frame();
                return;
            };
            let s = c.scale * k;
            out.push(
                Prim {
                    rect: [px + c.left as f32 * s, py - c.top as f32 * s * ry, c.w as f32 * s, c.h as f32 * s * ry],
                    clip: tp.clip,
                    c0: [1.0; 4],
                    uv: [0.0, 0.0, 1.0, 1.0],
                    p: [0.0, 0.0, KIND_IMAGE, tp.opacity * f.alpha * tp.color.a],
                    ..Default::default()
                },
                Some(tex),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::TextStyle;

    #[test]
    fn baselines_on_whole_pixels_pens_fractional() {
        let mut t = TextSys::new(&[], false, 1024);
        let buf = t.buffer("Wallet", &TextStyle::new(19.5, 400).line(27.0));
        let mut out = DrawList::default();
        t.paint(&buf, &TextPaint::at(10.3, 20.6, Color::WHITE), &mut out);
        assert_eq!(out.prims.len(), 6);
        for p in &out.prims {
            assert_eq!(p.rect[1], p.rect[1].round());
            assert_eq!(p.rect[0], p.rect[0].round());
        }
    }

    #[test]
    fn baseline_rules() {
        // Inter: ascent 0.96875 em, descent 0.2412 em. Measured in headless Chrome and WebKit.
        let (a, d) = (0.96875, 0.241211);
        for (size, line, webkit, blink) in [(21.0, 30.0, 23.0, 22.0), (19.5, 27.0, 21.0, 20.0), (18.0, 24.0, 19.0, 18.0), (32.0, 36.0, 30.0, 29.0), (22.5, 33.0, 25.0, 25.0), (66.0, 72.0, 60.0, 60.0), (48.0, 54.0, 44.0, 44.0), (25.5, 36.0, 27.0, 27.0), (15.0, 22.0, 16.0, 16.0)] {
            let w = (baseline_in_line(line, a * size, d * size, Baseline::WebKit) + 0.5).floor();
            let b = (baseline_in_line(line, a * size, d * size, Baseline::Blink) + 0.5).floor();
            assert_eq!((w, b), (webkit, blink), "{size}/{line}");
        }
    }

    #[test]
    fn scaled_text_rasterises_at_drawn_size() {
        let mut t = TextSys::new(&[], false, 1024);
        let buf = t.buffer("H", &TextStyle::new(20.0, 600));
        let mut a = DrawList::default();
        let mut b = DrawList::default();
        t.paint(&buf, &TextPaint::at(0.0, 0.0, Color::WHITE), &mut a);
        t.paint(&buf, &TextPaint::at(0.0, 0.0, Color::WHITE).scaled(1.5, 1.5), &mut b);
        let (ha, hb) = (a.prims[0].rect[3], b.prims[0].rect[3]);
        assert!((hb / ha - 1.5).abs() < 0.12, "{ha} {hb}");
        // Different raster, not the same bitmap stretched.
        assert_ne!(a.prims[0].uv, b.prims[0].uv);
    }
}
