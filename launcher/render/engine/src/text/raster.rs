//! Glyph rasterisation. Outlines are hinted vertically only (the light autohinter: x-height,
//! cap height and baseline land on whole pixels, horizontal shapes and advances stay exact), then
//! drawn with a quarter-pixel horizontal offset. That is what both references do: headless Chrome
//! draws web fonts through Fontations' light autohinter, and the device's WebKitGTK draws through
//! FreeType with fontconfig's default `hintslight`. Colour glyphs come from swash.
//!
//! A coverage curve per text luminance bucket then gives the browser's text weight.
//!
//! The curve is Skia's mask gamma (SkMaskGamma, the code Chrome and WebKit's Skia port draw text
//! with): blending coverage in sRGB-encoded space, as CSS does, makes light text on a dark
//! background look thinner than linear-light blending would; Skia corrects coverage assuming the
//! destination is the text's opposite luminance, with a contrast boost that tapers off toward
//! white. `Tune` holds the two numbers; `text-parity` fits them against the browser.

use cosmic_text::{fontdb, CacheKey, FontSystem};
use skrifa::instance::Size;
use skrifa::outline::{DrawSettings, Engine, GlyphStyles, HintingInstance, HintingOptions, OutlinePen, SmoothMode, Target};
use skrifa::MetadataProvider;
use std::collections::HashMap;
use swash::scale::{image::Content, Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Command, Format, Mask, Origin, Vector};

/// Luminance buckets (Skia uses 3 bits too).
pub const BUCKETS: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tune {
    /// Gamma the correction assumes for the display; 1.0 turns the correction off.
    pub gamma: f32,
    /// Contrast boost of partial coverage (Skia's SK_GAMMA_CONTRAST).
    pub contrast: f32,
    pub hint: Hint,
    /// Light hinting only up to this font size in px (Hint::Light; f32::MAX: every size).
    pub hint_max: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hint {
    /// Unhinted outlines.
    None,
    /// Vertical-only autohinting (FreeType's light mode, fontconfig hintslight).
    Light,
}

impl Default for Tune {
    /// Fitted against headless Chrome and WebKit on the specimen.
    fn default() -> Tune {
        Tune { gamma: 1.0, contrast: 0.0, hint: Hint::Light, hint_max: f32::MAX }
    }
}

pub enum Ink {
    /// 8-bit coverage, row-major w x h.
    Mask(Vec<u8>),
    /// Premultiplied RGBA and the drawn size over the raster size.
    Color(Vec<u8>, f32),
}

pub struct Raster {
    pub w: u32,
    pub h: u32,
    pub left: i32,
    pub top: i32,
    pub ink: Ink,
}

pub struct Rasterizer {
    cx: ScaleContext,
    pub tune: Tune,
    luts: Vec<[u8; 256]>,
    /// Autohinter style analysis per face (computed once), then one instance per size and weight.
    styles: HashMap<fontdb::ID, Option<GlyphStyles>>,
    hinters: HashMap<(fontdb::ID, u16, u32), Option<HintingInstance>>,
    /// Faces with colour tables (emoji) go through swash.
    color_faces: HashMap<fontdb::ID, bool>,
}

/// Collects a skrifa outline as zeno path commands.
#[derive(Default)]
struct Path(Vec<Command>);

impl OutlinePen for Path {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push(Command::MoveTo(Vector::new(x, y)));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push(Command::LineTo(Vector::new(x, y)));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.push(Command::QuadTo(Vector::new(cx0, cy0), Vector::new(x, y)));
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.push(Command::CurveTo(Vector::new(cx0, cy0), Vector::new(cx1, cy1), Vector::new(x, y)));
    }
    fn close(&mut self) {
        self.0.push(Command::Close);
    }
}

/// The luminance bucket of a text colour (sRGB-encoded Rec. 709 weights).
pub fn bucket(c: crate::Color) -> u8 {
    let l = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    (l.clamp(0.0, 1.0) * (BUCKETS - 1) as f32).round() as u8
}

fn to_lin(g: f32, v: f32) -> f32 {
    v.powf(g)
}

fn from_lin(g: f32, v: f32) -> f32 {
    v.max(0.0).powf(1.0 / g)
}

/// Skia's SkTMaskGamma_build_correcting_lut for one source luminance.
pub fn lut(src: f32, t: Tune) -> [u8; 256] {
    let mut out = [0u8; 256];
    let dst = 1.0 - src;
    let lin_src = to_lin(t.gamma, src);
    let lin_dst = to_lin(t.gamma, dst);
    let contrast = t.contrast * lin_dst;
    if (src - dst).abs() < 1.0 / 256.0 {
        for (i, o) in out.iter_mut().enumerate() {
            *o = i as u8;
        }
        return out;
    }
    let inv = 1.0 / (src - dst);
    for (i, o) in out.iter_mut().enumerate() {
        let raw = i as f32 / 255.0;
        let a = raw + (1.0 - raw) * contrast * raw;
        let lin_out = lin_src * a + (1.0 - a) * lin_dst;
        let v = (from_lin(t.gamma, lin_out) - dst) * inv;
        *o = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    out
}

impl Rasterizer {
    pub fn new(tune: Tune) -> Rasterizer {
        let luts = (0..BUCKETS).map(|b| lut(b as f32 / (BUCKETS - 1) as f32, tune)).collect();
        Rasterizer { cx: ScaleContext::new(), tune, luts, styles: HashMap::new(), hinters: HashMap::new(), color_faces: HashMap::new() }
    }

    pub fn render(&mut self, fs: &mut FontSystem, key: CacheKey, bucket: u8) -> Option<Raster> {
        if self.tune.hint == Hint::Light && f32::from_bits(key.font_size_bits) <= self.tune.hint_max {
            let font = fs.get_font(key.font_id, key.font_weight)?;
            let index = fs.db().face(key.font_id).map(|f| f.index).unwrap_or(0);
            let color = *self.color_faces.entry(key.font_id).or_insert_with(|| {
                skrifa::FontRef::from_index(font.data(), index).map_or(true, |f| [b"COLR", b"CBDT", b"sbix", b"SVG "].iter().any(|t| f.table_data(skrifa::Tag::new(t)).is_some()))
            });
            if !color {
                if let Some(r) = self.render_hinted(font.data(), index, key, bucket) {
                    return Some(r);
                }
            }
        }
        self.render_swash(fs, key, bucket)
    }

    /// Light-autohinted outline through skrifa, rasterised by zeno (the rasteriser swash uses).
    fn render_hinted(&mut self, data: &[u8], index: u32, key: CacheKey, bucket: u8) -> Option<Raster> {
        let font = skrifa::FontRef::from_index(data, index).ok()?;
        let outlines = font.outline_glyphs();
        let glyph = outlines.get(skrifa::GlyphId::new(key.glyph_id as u32))?;
        let size = f32::from_bits(key.font_size_bits);
        let styles = self.styles.entry(key.font_id).or_insert_with(|| Some(GlyphStyles::new(&outlines))).clone();
        let hinter = self
            .hinters
            .entry((key.font_id, key.font_weight.0, key.font_size_bits))
            .or_insert_with(|| {
                let loc = font.axes().location([("wght", key.font_weight.0 as f32)]);
                let opts = HintingOptions {
                    engine: Engine::Auto(styles),
                    target: Target::Smooth { mode: SmoothMode::Light, symmetric_rendering: false, preserve_linear_metrics: true },
                };
                HintingInstance::new(&outlines, Size::new(size), &loc, opts).ok()
            })
            .as_ref()?;
        let mut path = Path::default();
        glyph.draw(DrawSettings::hinted(hinter, false), &mut path).ok()?;
        if path.0.is_empty() {
            return None;
        }
        let offset = Vector::new(key.x_bin.as_float(), key.y_bin.as_float());
        let mut mask = Mask::new(&path.0[..]);
        // inspect() sizes the mask first; without it zeno's placement top ignores the height.
        mask.format(Format::Alpha).origin(Origin::BottomLeft).offset(offset).render_offset(offset).inspect(|_, _, _| {});
        let (data, p) = mask.render();
        if p.width == 0 || p.height == 0 {
            return None;
        }
        let lut = &self.luts[bucket.min(BUCKETS - 1) as usize];
        Some(Raster { w: p.width, h: p.height, left: p.left, top: p.top, ink: Ink::Mask(data.iter().map(|&v| lut[v as usize]).collect()) })
    }

    fn render_swash(&mut self, fs: &mut FontSystem, key: CacheKey, bucket: u8) -> Option<Raster> {
        let font = fs.get_font(key.font_id, key.font_weight)?;
        let sf = font.as_swash();
        let size = f32::from_bits(key.font_size_bits);
        let wght = swash::Tag::from_be_bytes(*b"wght");
        let mut b = self.cx.builder(sf).size(size).hint(false);
        if let Some(v) = sf.variations().find_by_tag(wght) {
            b = b.normalized_coords(sf.variations().normalized_coords([(wght, f32::from(key.font_weight.0).clamp(v.min_value(), v.max_value()))]));
        }
        let mut scaler = b.build();
        let img = Render::new(&[Source::ColorOutline(0), Source::ColorBitmap(StrikeWith::BestFit), Source::Outline])
            .format(Format::Alpha)
            .offset(Vector::new(key.x_bin.as_float(), key.y_bin.as_float()))
            .render(&mut scaler, key.glyph_id)?;
        let (w, h) = (img.placement.width, img.placement.height);
        if w == 0 || h == 0 {
            return None;
        }
        let ink = match img.content {
            Content::Mask => {
                let lut = &self.luts[bucket.min(BUCKETS - 1) as usize];
                Ink::Mask(img.data.iter().map(|&v| lut[v as usize]).collect())
            }
            Content::SubpixelMask => Ink::Mask(img.data.chunks(4).map(|p| ((p[0] as u16 + p[1] as u16 + p[2] as u16) / 3) as u8).collect()),
            Content::Color => {
                // swash hands back straight alpha; the pipeline blends premultiplied.
                let mut rgba = img.data;
                for p in rgba.chunks_mut(4) {
                    let a = p[3] as u16;
                    for c in &mut p[..3] {
                        *c = ((*c as u16 * a + 127) / 255) as u8;
                    }
                }
                // Bitmap strikes come at their own size: draw them at the em the text asked for.
                let em = size * 1.17;
                let scale = if h as f32 > em * 1.3 { em / h as f32 } else { 1.0 };
                Ink::Color(rgba, scale)
            }
        };
        Some(Raster { w, h, left: img.placement.left, top: img.placement.top, ink })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_without_correction() {
        let l = lut(1.0, Tune { gamma: 1.0, contrast: 0.0, hint: Hint::None, hint_max: f32::MAX });
        assert!(l.iter().enumerate().all(|(i, &v)| v == i as u8));
    }

    #[test]
    fn light_text_gains_coverage_with_gamma() {
        let l = lut(1.0, Tune { gamma: 1.8, contrast: 0.0, hint: Hint::None, hint_max: f32::MAX });
        assert_eq!(l[0], 0);
        assert_eq!(l[255], 255);
        assert!(l[128] > 160, "{}", l[128]);
    }

    #[test]
    fn mid_grey_text_is_left_alone() {
        let l = lut(0.5, Tune { gamma: 2.2, contrast: 0.5, hint: Hint::None, hint_max: f32::MAX });
        assert!(l.iter().enumerate().all(|(i, &v)| v == i as u8));
    }

    #[test]
    fn buckets() {
        assert_eq!(bucket(crate::Color { r: 242.0 / 255.0, g: 242.0 / 255.0, b: 243.0 / 255.0, a: 1.0 }), 7);
        assert_eq!(bucket(crate::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }), 0);
    }
}

#[cfg(test)]
mod hint_tests {
    use super::*;
    use crate::text::{TextStyle, TextSys};

    /// Light hinting moves edges by at most a pixel vertically and never horizontally.
    #[test]
    fn hinted_placement_close_to_unhinted() {
        let mut t = TextSys::new(&[], false, 512);
        let buf = t.buffer("Hxg", &TextStyle::new(20.0, 400));
        let glyphs: Vec<_> = buf.layout_runs().next().unwrap().glyphs.to_vec();
        let mut r = Rasterizer::new(Tune::default());
        for g in glyphs {
            let key = g.physical((0.0, 0.0), 1.0).cache_key;
            let a = r.render(&mut t.fs, key, 7).unwrap();
            let b = r.render_swash(&mut t.fs, key, 7).unwrap();
            assert!((a.top - b.top).abs() <= 1 && (a.left - b.left).abs() <= 1 && (a.h as i32 - b.h as i32).abs() <= 2);
        }
    }
}
