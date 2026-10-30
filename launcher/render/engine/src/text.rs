//! Text: cosmic-text shapes (HarfRust), our rasteriser draws glyphs into one R8 atlas, and the
//! paint helpers turn a shaped buffer into glyph quads.
//!
//! What makes it look like the web launcher:
//! - the web's own font files: Inter variable (every weight the CSS asks for) and Fira Code 400/500;
//! - horizontal subpixel positioning in quarter pixels, baselines on whole pixels, no hinting
//!   (the fonts carry none, and the browser does not hint them on macOS either);
//! - grayscale anti-aliasing on purpose: the SuiPlay's panel is portrait-native and gamescope
//!   rotates it, so LCD subpixel order is vertical and subpixel AA would fringe;
//! - a coverage curve per text luminance (Skia's mask gamma) so light text on the dark theme has
//!   the browser's weight; glyphs are cached per (glyph, size, subpixel bin, luminance bucket);
//! - colour glyphs (emoji) as their own premultiplied images, not tinted masks.
//!
//! Fallback fonts load lazily: the start only loads the bundled faces; the first glyph no bundled
//! face covers loads a matching system face (Noto CJK, Noto Color Emoji...) found by file name,
//! and anything still missing is looked up by a background scan of the font directories.
//!
//! Submodules: `roll` (rolling digits for Num), `edit` (editable text for the OSK and search).

mod fallback;
pub mod edit;
pub mod fonts;
mod paint;
mod raster;
pub mod roll;

pub use paint::{baseline_in_line, Baseline, GlyphFx, TextPaint};
pub use raster::{Hint, Tune};

use crate::gpu::{Gpu, TexId};
use cosmic_text::{Align, Attrs, Buffer, CacheKey, Ellipsize, EllipsizeHeightLimit, Family, FontFeatures, FontSystem, Metrics, Shaping, Weight, Wrap};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    /// Font size in px (fractional sizes are kept: 0.8125rem at 24 px is 19.5 px in the browser too).
    pub size: f32,
    /// Line height in px.
    pub line: f32,
    pub weight: u16,
    /// Fira Code with ligatures off and tabular figures (the launcher's `.num` class).
    pub mono: bool,
    /// Tabular figures (Num): digits keep their width when they change.
    pub tabular: bool,
    pub wrap: bool,
    /// One line, cut with an ellipsis at the box width (Tailwind truncate).
    pub truncate: bool,
    /// Wrap and cut with an ellipsis after this many lines (Tailwind line-clamp-N); 0 = no clamp.
    pub clamp: u16,
    /// Letter spacing in em (Tailwind tracking).
    pub tracking: f32,
    /// Horizontal alignment inside the box (text-left/center/right).
    pub align: TextAlign,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl TextStyle {
    pub fn new(size: f32, weight: u16) -> TextStyle {
        TextStyle { size, line: (size * 1.25).round(), weight, mono: false, tabular: false, wrap: false, truncate: false, clamp: 0, tracking: 0.0, align: TextAlign::Left }
    }
    pub fn line(mut self, l: f32) -> Self {
        self.line = l;
        self
    }
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self.tabular = true;
        self
    }
    pub fn tabular(mut self) -> Self {
        self.tabular = true;
        self
    }
    pub fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }
    pub fn truncate(mut self) -> Self {
        self.truncate = true;
        self
    }
    /// Multi-line wrap clamped to `lines`, the last one ending in an ellipsis.
    pub fn clamp(mut self, lines: u16) -> Self {
        self.clamp = lines;
        self.wrap = lines > 0;
        self
    }
    pub fn tracking(mut self, em: f32) -> Self {
        self.tracking = em;
        self
    }
    pub fn align(mut self, a: TextAlign) -> Self {
        self.align = a;
        self
    }
    /// The width the text lays out against matters (wrapping, ellipsis, alignment).
    pub fn needs_width(&self) -> bool {
        self.wrap || self.truncate || self.clamp > 0 || self.align != TextAlign::Left
    }
}

/// Which browser's text the engine reproduces. The two differ in hinting and baseline rounding
/// only (shapes, advances, weights and subpixel positions are the same).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextLook {
    /// The launcher as it runs on the device: WebKitGTK through FreeType with fontconfig's
    /// `hintslight` (light autohinting, vertical only) and WebKit's baseline rounding.
    #[default]
    Device,
    /// Headless Chrome on macOS, what launcher/scripts/shots.mjs captures: unhinted outlines and
    /// Blink's baseline rounding. Use it to diff native screens against those shots.
    Chrome,
}

impl TextLook {
    /// `RR_TEXT_LOOK=chrome|device`, else Device.
    pub fn from_env() -> TextLook {
        match std::env::var("RR_TEXT_LOOK").as_deref() {
            Ok("chrome") => TextLook::Chrome,
            _ => TextLook::Device,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GlyphSlot {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub left: i32,
    pub top: i32,
}

/// A colour glyph (emoji): its own premultiplied RGBA texture, made by `TextSys::upload`.
#[derive(Clone, Debug)]
pub struct ColorGlyph {
    pub tex: Option<TexId>,
    pub w: u32,
    pub h: u32,
    pub left: i32,
    pub top: i32,
    /// Drawn size over raster size: bitmap emoji come in fixed strikes (Noto 109 px, Apple 160 px).
    pub scale: f32,
    rgba: Option<Vec<u8>>,
}

pub struct TextSys {
    pub fs: FontSystem,
    raster: raster::Rasterizer,
    alloc: etagere::BucketedAtlasAllocator,
    glyphs: HashMap<(CacheKey, u8), Option<GlyphSlot>>,
    colors: HashMap<CacheKey, Option<ColorGlyph>>,
    /// Glyph bitmaps waiting for the next frame's upload.
    pub pending: Vec<(GlyphSlot, Vec<u8>)>,
    color_pending: Vec<CacheKey>,
    pub atlas_size: u32,
    fallback: fallback::Fallback,
    generation: u64,
    again: bool,
    /// The atlas filled up and was cleared this frame: everything is drawn again next frame.
    pub atlas_resets: u32,
    /// Which browser's baseline rounding to follow.
    pub baseline: Baseline,
    vmetrics: HashMap<cosmic_text::fontdb::ID, (f32, f32)>,
    ellipses: HashMap<(cosmic_text::fontdb::ID, u16, u32), Option<(u16, f32)>>,
}

/// Rounds up to the browser's layout unit (1/64 px), ignoring float noise below it.
pub fn layout_unit_ceil(v: f32) -> f32 {
    ((v * 64.0 - 1e-3).ceil() / 64.0).max(0.0)
}

/// A span's opacity as glyph metadata (0: no span opacity).
fn span_code(opacity: f32) -> usize {
    (opacity.clamp(0.0, 1.0) * 1000.0).round() as usize + 1
}

/// The opacity `set_spans` gave a glyph.
pub(crate) fn span_opacity(metadata: usize) -> f32 {
    match metadata & 0x7FFF {
        0 => 1.0,
        m => (m - 1) as f32 / 1000.0,
    }
}

/// Metadata bit of a `truncate` buffer (the ellipsis is switched on only when the line overflows).
const TRUNCATE_BIT: usize = 1 << 15;

/// Whether a buffer was set as one truncated line.
pub(crate) fn truncates(buf: &Buffer) -> bool {
    buf.lines.first().is_some_and(|l| l.attrs_list().defaults().metadata & TRUNCATE_BIT != 0)
}

/// Lines are laid out against the box width plus this: a box sized to its own measure (rounded up
/// to the 1/64 px layout unit, then through taffy) must never wrap or ellipsize its last letters.
pub const FIT_SLACK: f32 = 1.0 / 32.0;

/// The line clamp a buffer was set with (0: none).
pub(crate) fn clamp_of(buf: &Buffer) -> usize {
    buf.lines.first().map_or(0, |l| l.attrs_list().defaults().metadata >> 16)
}

/// Luminance bucket of text the old `glyph()` call draws: the theme's light foreground.
pub const LIGHT: u8 = raster::BUCKETS - 1;

impl TextSys {
    /// The bundled faces (`fonts::BUNDLED`: Inter variable, Fira Code 400 and 500) are always
    /// loaded; `fonts` adds more. `system` turns on lazy fallback to the system's fonts (CJK, emoji,
    /// symbols): nothing is read from the system until a glyph is missing.
    pub fn new(fonts: &[&'static [u8]], system: bool, atlas_size: u32) -> TextSys {
        let mut db = cosmic_text::fontdb::Database::new();
        for f in fonts::BUNDLED.iter().chain(fonts) {
            db.load_font_data(f.to_vec());
        }
        db.set_sans_serif_family("Inter");
        db.set_monospace_family("Fira Code");
        let fs = FontSystem::new_with_locale_and_db("en-US".into(), db);
        let mut t = TextSys {
            fs,
            raster: raster::Rasterizer::new(Tune::default()),
            alloc: etagere::BucketedAtlasAllocator::new(etagere::size2(atlas_size as i32, atlas_size as i32)),
            glyphs: HashMap::new(),
            colors: HashMap::new(),
            pending: Vec::new(),
            color_pending: Vec::new(),
            atlas_size,
            fallback: fallback::Fallback::new(system),
            generation: 0,
            again: false,
            atlas_resets: 0,
            baseline: Baseline::default(),
            vmetrics: HashMap::new(),
            ellipses: HashMap::new(),
        };
        t.set_look(TextLook::from_env());
        t
    }

    /// Switches hinting and baseline rounding to `look` (TextSys::new reads RR_TEXT_LOOK).
    pub fn set_look(&mut self, look: TextLook) {
        let (hint, baseline) = match look {
            TextLook::Device => (Hint::Light, Baseline::WebKit),
            TextLook::Chrome => (Hint::None, Baseline::Blink),
        };
        self.baseline = baseline;
        self.set_tune(Tune { hint, ..self.raster.tune });
    }

    /// The rasteriser's gamma and contrast (the parity tool fits them against the browser).
    pub fn tune(&self) -> Tune {
        self.raster.tune
    }

    pub fn set_tune(&mut self, t: Tune) {
        if t != self.raster.tune {
            self.raster = raster::Rasterizer::new(t);
            self.clear_atlas();
        }
    }

    pub fn buffer(&mut self, text: &str, st: &TextStyle) -> Buffer {
        let mut buf = Buffer::new(&mut self.fs, Metrics::new(st.size, st.line));
        self.set(&mut buf, text, st);
        buf
    }

    pub fn attrs(st: &TextStyle) -> Attrs<'static> {
        let mut attrs = Attrs::new().family(if st.mono { Family::Name("Fira Code") } else { Family::Name("Inter") }).weight(Weight(st.weight));
        let mut f = FontFeatures::new();
        if st.tabular {
            f.enable(cosmic_text::FeatureTag::new(b"tnum"));
        }
        if st.mono {
            // font-variant-ligatures: none (Fira Code draws "0x" as "0×" with them on).
            for t in [b"liga", b"clig", b"calt", b"dlig"] {
                f.disable(cosmic_text::FeatureTag::new(t));
            }
        }
        attrs = attrs.font_features(f);
        if st.tracking != 0.0 {
            attrs = attrs.letter_spacing(st.tracking);
        }
        attrs
    }

    pub fn set(&mut self, buf: &mut Buffer, text: &str, st: &TextStyle) {
        self.set_spans(buf, &[(text, 1.0)], st);
    }

    /// Text in runs of different opacity, e.g. Num's dimmed cents (`<span className="opacity-50">`).
    /// `paint` multiplies each glyph's alpha by its run's opacity.
    pub fn set_spans(&mut self, buf: &mut Buffer, spans: &[(&str, f32)], st: &TextStyle) {
        buf.set_metrics(Metrics::new(st.size, st.line));
        buf.set_wrap(if st.wrap || st.clamp > 0 { Wrap::WordOrGlyph } else { Wrap::None });
        // line-clamp is drawn by `paint` (the browser's rule: wrap, then fit "…" on the last line);
        // truncate turns cosmic's ellipsis on in `measure`, only when the line overflows its box.
        buf.set_ellipsize(Ellipsize::None);
        let trunc = if st.truncate && st.clamp == 0 { TRUNCATE_BIT } else { 0 };
        let attrs = Self::attrs(st).metadata((st.clamp as usize) << 16 | trunc);
        let align = match st.align {
            TextAlign::Left => None,
            TextAlign::Center => Some(Align::Center),
            TextAlign::Right => Some(Align::Right),
        };
        let apply = |buf: &mut Buffer| {
            if let [(text, o)] = spans {
                if *o >= 1.0 {
                    buf.set_text(text, &attrs, Shaping::Advanced, align);
                    return;
                }
            }
            let rich = spans.iter().map(|(t, o)| (*t, attrs.clone().metadata((st.clamp as usize) << 16 | trunc | span_code(*o))));
            buf.set_rich_text(rich, &attrs, Shaping::Advanced, align);
        };
        apply(buf);
        buf.set_size(None, None);
        buf.shape_until_scroll(&mut self.fs, false);
        let missing = fallback::missing_chars(buf);
        if !missing.is_empty() && self.fallback.resolve(&mut self.fs, &missing) {
            self.generation += 1;
            apply(buf);
            buf.shape_until_scroll(&mut self.fs, false);
        }
    }

    /// Lays the buffer out for `width` (None: one line) and returns its size the way the browser
    /// reports it: rounded up to a 64th of a pixel (LayoutUnit), not to whole pixels, so text in a
    /// flex row or centred in a button lands where the web puts it (measured within 0.08 px of
    /// Chrome and WebKit on every specimen line).
    ///
    /// `width` gets `FIT_SLACK` on top, so text laid out at its own measured width stays one line
    /// and a truncated label that fits keeps every letter (no "Mark…" for "Market").
    pub fn measure(&mut self, buf: &mut Buffer, width: Option<f32>) -> (f32, f32) {
        let (given, width) = (width, width.map(|w| w + FIT_SLACK));
        let trunc = truncates(buf);
        if trunc {
            buf.set_ellipsize(Ellipsize::None);
            buf.set_size(width, None);
            buf.shape_until_scroll(&mut self.fs, false);
            let over = width.is_some_and(|w| buf.layout_runs().any(|r| r.line_w > w));
            if over {
                buf.set_ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(1)));
            }
        }
        buf.set_size(width, None);
        buf.shape_until_scroll(&mut self.fs, false);
        let clamp = clamp_of(buf);
        let mut w: f32 = 0.0;
        let mut h: f32 = 0.0;
        for run in buf.layout_runs().take(if clamp > 0 { clamp } else { usize::MAX }) {
            w = w.max(run.line_w);
            h = h.max(run.line_top + run.line_height);
        }
        if let (true, Some(g)) = (trunc, given) {
            w = w.min(g);
        }
        (layout_unit_ceil(w), layout_unit_ceil(h))
    }

    /// Picks up fonts the background fallback scan found. True when text must be set again
    /// (call `set` on every text, then lay out): glyphs that were missing can now draw.
    pub fn poll_fonts(&mut self) -> bool {
        if self.fallback.poll(&mut self.fs) {
            self.generation += 1;
            return true;
        }
        false
    }

    /// Bumps whenever fonts were added; a text set before a bump may have missing glyphs.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Paths of the system font files loaded so far (fallback).
    pub fn fallback_files(&self) -> &[std::path::PathBuf] {
        &self.fallback.loaded
    }

    /// The atlas slot of a glyph for light text, rasterising it on first use. Colour glyphs return
    /// None here: `paint` draws them.
    pub fn glyph(&mut self, key: CacheKey) -> Option<GlyphSlot> {
        self.glyph_lum(key, LIGHT)
    }

    /// The atlas slot of a glyph for text in luminance `bucket` (`raster::bucket(color)`).
    pub fn glyph_lum(&mut self, key: CacheKey, bucket: u8) -> Option<GlyphSlot> {
        if let Some(s) = self.glyphs.get(&(key, bucket)) {
            return *s;
        }
        if self.colors.contains_key(&key) {
            return None;
        }
        let slot = match self.raster.render(&mut self.fs, key, bucket) {
            Some(raster::Raster { w, h, left, top, ink: raster::Ink::Mask(data) }) => self.place(w, h, left, top, data),
            Some(raster::Raster { w, h, left, top, ink: raster::Ink::Color(rgba, scale) }) => {
                self.colors.insert(key, Some(ColorGlyph { tex: None, w, h, left, top, scale, rgba: Some(rgba) }));
                self.color_pending.push(key);
                return None;
            }
            None => None,
        };
        self.glyphs.insert((key, bucket), slot);
        slot
    }

    fn place(&mut self, w: u32, h: u32, left: i32, top: i32, data: Vec<u8>) -> Option<GlyphSlot> {
        // One pixel of padding so linear sampling never bleeds a neighbour in.
        let size = etagere::size2(w as i32 + 2, h as i32 + 2);
        let a = match self.alloc.allocate(size) {
            Some(a) => a,
            None => {
                // Full: start over. Glyphs already painted this frame may point at reused slots,
                // so the frame is drawn again (`upload` returns true).
                self.clear_atlas();
                self.atlas_resets += 1;
                self.alloc.allocate(size)?
            }
        };
        let s = GlyphSlot { x: a.rectangle.min.x as u32 + 1, y: a.rectangle.min.y as u32 + 1, w, h, left, top };
        self.pending.push((s, data));
        Some(s)
    }

    fn clear_atlas(&mut self) {
        self.glyphs.clear();
        self.pending.clear();
        self.alloc.clear();
        self.again = true;
    }

    /// The colour glyph for `key`, if it is one (after `glyph_lum` saw it).
    pub fn color_glyph(&self, key: &CacheKey) -> Option<&ColorGlyph> {
        self.colors.get(key).and_then(Option::as_ref)
    }

    /// Sends new glyphs to the GPU: atlas rows and one texture per new colour glyph. True when
    /// the frame should be drawn once more (a colour glyph was first seen this frame, or the atlas
    /// was reset).
    pub fn upload(&mut self, gpu: &mut Gpu) -> bool {
        for (slot, data) in self.pending.drain(..) {
            gpu.upload_glyph(slot.x, slot.y, slot.w, slot.h, &data);
        }
        for key in self.color_pending.drain(..) {
            if let Some(Some(c)) = self.colors.get_mut(&key) {
                if let Some(rgba) = c.rgba.take() {
                    c.tex = Some(gpu.create_texture_rgba(c.w, c.h, &rgba));
                    self.again = true;
                }
            }
        }
        std::mem::take(&mut self.again)
    }

    /// Takes the colour glyphs waiting for a texture (headless renderers upload them themselves).
    pub fn take_color_pending(&mut self) -> Vec<(CacheKey, u32, u32, Vec<u8>)> {
        let mut out = Vec::new();
        for key in self.color_pending.drain(..) {
            if let Some(Some(c)) = self.colors.get_mut(&key) {
                if let Some(rgba) = c.rgba.take() {
                    out.push((key, c.w, c.h, rgba));
                }
            }
        }
        out
    }

    /// Records the texture a headless renderer made for a colour glyph.
    pub fn set_color_tex(&mut self, key: CacheKey, tex: TexId) {
        if let Some(Some(c)) = self.colors.get_mut(&key) {
            c.tex = Some(tex);
        }
    }

    pub fn glyph_count(&self) -> usize {
        self.glyphs.len() + self.colors.len()
    }

    pub(crate) fn want_frame(&mut self) {
        self.again = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys() -> TextSys {
        TextSys::new(&[], false, 1024)
    }

    #[test]
    fn bundled_faces_resolve_by_family_and_weight() {
        let mut t = sys();
        for (st, family, weight) in [
            (TextStyle::new(20.0, 400), "Inter", 400),
            (TextStyle::new(20.0, 600), "Inter", 600),
            (TextStyle::new(20.0, 400).mono(), "Fira Code", 400),
            (TextStyle::new(20.0, 500).mono(), "Fira Code", 500),
        ] {
            let buf = t.buffer("Ag0", &st);
            let g = &buf.layout_runs().next().unwrap().glyphs[0];
            let face = t.fs.db().face(g.font_id).unwrap();
            assert!(face.families.iter().any(|(n, _)| n == family), "{family}: {:?}", face.families);
            assert_eq!(g.font_weight.0, weight);
        }
    }

    /// `usd_price` writes tiny prices as `$0.0₅6093`: the subscript digits come from Inter itself.
    #[test]
    fn subscript_digits_are_bundled() {
        let mut t = sys();
        let buf = t.buffer("0.0₀₁₂₃₄₅₆₇₈₉", &TextStyle::new(20.0, 400));
        for g in buf.layout_runs().next().unwrap().glyphs {
            assert_ne!(g.glyph_id, 0, "tofu at {}", g.start);
            let face = t.fs.db().face(g.font_id).unwrap();
            assert!(face.families.iter().any(|(n, _)| n == "Inter"));
        }
    }

    /// The native app adds static Inter files next to the bundled variable one; no weight the
    /// Market and Trade pages use may pick a face without the subscript digits.
    #[test]
    fn subscript_digits_resolve_in_the_apps_font_set_at_every_weight() {
        static EXTRA: [&[u8]; 5] = [
            include_bytes!("../../assets/fonts/Inter-Regular.ttf"),
            include_bytes!("../../assets/fonts/Inter-Medium.ttf"),
            include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
            include_bytes!("../../assets/fonts/Inter-Bold.ttf"),
            include_bytes!("../../assets/fonts/FiraCode-Medium.ttf"),
        ];
        let mut t = TextSys::new(&EXTRA, false, 1024);
        for w in [400u16, 500, 600, 700] {
            for mono in [false, true] {
                let mut st = TextStyle::new(20.0, w);
                st.mono = mono;
                let buf = t.buffer("$0.0\u{2085}6093 \u{2080}\u{2081}\u{2082}\u{2083}\u{2084}\u{2086}\u{2087}\u{2088}\u{2089}", &st);
                for g in buf.layout_runs().next().unwrap().glyphs {
                    assert_ne!(g.glyph_id, 0, "tofu at {} (weight {w}, mono {mono})", g.start);
                }
            }
        }
    }

    #[test]
    fn arrows_and_math_signs_are_not_tofu() {
        let mut t = TextSys::new(&[], false, 1024);
        let buf = t.buffer("\u{2192}\u{2190}\u{2248}\u{2264}\u{2265}\u{2260}\u{2197}\u{221E}\u{2713}", &TextStyle::new(20.0, 400));
        for g in buf.layout_runs().next().unwrap().glyphs {
            assert_ne!(g.glyph_id, 0, "tofu at {}", g.start);
        }
    }

    #[test]
    fn variable_weight_changes_outlines() {
        let mut t = sys();
        let ink = |t: &mut TextSys, w: u16| {
            let buf = t.buffer("H", &TextStyle::new(40.0, w));
            let g = buf.layout_runs().next().unwrap().glyphs[0].physical((0.0, 0.0), 1.0);
            match t.raster.render(&mut t.fs, g.cache_key, LIGHT).unwrap().ink {
                raster::Ink::Mask(m) => m.iter().map(|&v| v as u64).sum::<u64>(),
                _ => 0,
            }
        };
        let (a, b, c) = (ink(&mut t, 400), ink(&mut t, 600), ink(&mut t, 700));
        assert!(a < b && b < c, "{a} {b} {c}");
    }

    #[test]
    fn tabular_figures_share_one_width() {
        let mut t = sys();
        let w = |t: &mut TextSys, s: &str, st: TextStyle| {
            let mut b = t.buffer(s, &st);
            t.measure(&mut b, None).0
        };
        let tab = TextStyle::new(20.0, 400).tabular();
        assert_eq!(w(&mut t, "111111", tab), w(&mut t, "888888", tab));
        let prop = TextStyle::new(20.0, 400);
        assert!(w(&mut t, "111111", prop) < w(&mut t, "888888", prop));
    }

    #[test]
    fn mono_turns_ligatures_off() {
        let mut t = sys();
        let st = TextStyle::new(20.0, 400).mono();
        let buf = t.buffer("0x->", &st);
        assert_eq!(buf.layout_runs().next().unwrap().glyphs.len(), 4);
    }

    #[test]
    fn letter_spacing_widens_by_em_per_glyph() {
        let mut t = sys();
        let mut a = t.buffer("Rare", &TextStyle::new(20.0, 600));
        let mut b = t.buffer("Rare", &TextStyle::new(20.0, 600).tracking(-0.02));
        let (wa, wb) = (t.measure(&mut a, None).0, t.measure(&mut b, None).0);
        // 4 glyphs x -0.4 px
        assert!((wa - wb - 1.6).abs() <= 1.0, "{wa} {wb}");
    }

    #[test]
    fn truncate_ends_in_an_ellipsis() {
        let mut t = sys();
        let mut b = t.buffer("Call of Duty Modern Warfare II Campaign Remastered", &TextStyle::new(20.0, 400).truncate());
        let (w, h) = t.measure(&mut b, Some(160.0));
        assert!(w <= 160.0 && h <= 25.0, "{w} {h}");
        let run = b.layout_runs().next().unwrap();
        assert!(ends_in_ellipsis(&t, run.glyphs));
    }

    /// WS 15 / WS 12: a truncating label laid out at exactly its measured width keeps every letter.
    #[test]
    fn truncate_that_fits_has_no_ellipsis() {
        let mut t = sys();
        for (s, st) in [("Market", TextStyle::new(18.0, 500)), ("ha", TextStyle::new(20.0, 400).mono()), ("Warped Pioneers", TextStyle::new(20.0, 600)), ("mira", TextStyle::new(21.0, 500))] {
            let mut b = t.buffer(s, &st.truncate());
            let (w, _) = t.measure(&mut b, None);
            t.measure(&mut b, Some(w));
            let run = b.layout_runs().next().unwrap();
            assert!(!ends_in_ellipsis(&t, run.glyphs), "{s} at {w}");
            assert_eq!(run.glyphs.len(), s.chars().count(), "{s}");
            // A pixel less: cut.
            t.measure(&mut b, Some(w - 1.0));
            assert!(ends_in_ellipsis(&t, b.layout_runs().next().unwrap().glyphs), "{s} at {}", w - 1.0);
        }
    }

    /// WS 14: wrapped text re-laid at its own measured width stays on one line ("Liv / e").
    #[test]
    fn wrap_at_own_width_stays_one_line() {
        let mut t = sys();
        for s in ["-0.1 SUI", "Live", "Search the store"] {
            let mut b = t.buffer(s, &TextStyle::new(19.5, 500).wrap());
            let (w, _) = t.measure(&mut b, None);
            t.measure(&mut b, Some(w));
            assert_eq!(b.layout_runs().count(), 1, "{s} at {w}");
        }
    }

    fn ends_in_ellipsis(t: &TextSys, glyphs: &[cosmic_text::LayoutGlyph]) -> bool {
        let g = glyphs.last().unwrap();
        let id = t.fs.db().with_face_data(g.font_id, |d, i| swash::FontRef::from_index(d, i as usize).unwrap().charmap().map('…')).unwrap();
        g.glyph_id == id
    }

    #[test]
    fn clamp_wraps_then_fits_the_ellipsis_like_the_browser() {
        let mut t = sys();
        let text = "Hollow Keep is a quiet game about a lighthouse keeper who restores the lamps of a drowned coast, one season at a time.";
        let st = TextStyle::new(14.0, 400).line(20.0).clamp(2);
        let mut b = t.buffer(text, &st);
        let (w, h) = t.measure(&mut b, Some(260.0));
        assert_eq!(h, 40.0);
        assert!(w <= 260.0);
        let mut out = crate::gpu::DrawList::default();
        t.paint(&b, &TextPaint::at(0.0, 0.0, crate::Color::WHITE), &mut out);
        // Chrome 156: "Hollow Keep is a quiet game about a" / "lighthouse keeper who restores the…"
        // (the wrapped line keeps whole words; "…" replaces what does not fit, no space before it).
        let expect = "Hollow Keep is a quiet game about a".chars().filter(|c| *c != ' ').count() + "lighthouse keeper who restores the".chars().filter(|c| *c != ' ').count() + 1;
        assert_eq!(out.prims.len(), expect);
        let last = out.prims.last().unwrap();
        assert!(last.rect[0] + last.rect[2] <= 260.0);
        let mut wrapped = t.buffer(text, &TextStyle::new(14.0, 400).line(20.0).wrap());
        t.measure(&mut wrapped, Some(260.0));
        assert!(wrapped.layout_runs().count() > 2);
    }

    #[test]
    fn atlas_overflow_resets_instead_of_dropping_glyphs() {
        let mut t = TextSys::new(&[], false, 128);
        let mut n = 0;
        for size in [40.0, 44.0, 48.0, 52.0] {
            let buf = t.buffer("ABCDEFGHIJKLMNOPQRSTUVWXYZ", &TextStyle::new(size, 700));
            for g in buf.layout_runs().next().unwrap().glyphs {
                n += t.glyph(g.physical((0.0, 0.0), 1.0).cache_key).is_some() as u32;
            }
        }
        assert_eq!(n, 4 * 26);
        assert!(t.atlas_resets > 0);
    }
}
