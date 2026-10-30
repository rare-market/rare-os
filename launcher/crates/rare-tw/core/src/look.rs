//! The paint side of a `Tw`, resolved to pixels for the renderer, and CSS text inheritance.
//!
//! `Look` is what Rare Render's `Node` needs (fill, gradient, per-corner radius, per-side border,
//! box shadows, opacity, transforms, clip); `Text` is the inherited text context a text node is
//! shaped with (size, line height, weight, family, tracking, colour, wrapping). Both follow the
//! browser: the oracle tests check the same declarations against Chromium.

use crate::model::*;

/// A box-shadow layer in px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxShadow {
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Rgba,
}

/// Paint properties in px at one rem (percentages of the element's own box stay fractions).
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub bg: Rgba,
    pub gradient: Option<Gradient>,
    /// top-left, top-right, bottom-right, bottom-left; f32::INFINITY = fully round (clamp to half the box)
    pub radius: [f32; 4],
    /// Percent radii (`rounded-[22%]`), as fractions of the box, per corner.
    pub radius_pct: [Option<f32>; 4],
    /// top, right, bottom, left
    pub border: [f32; 4],
    pub border_color: Rgba,
    pub opacity: f32,
    /// Outer box shadows, painted below the box, first layer on top (CSS order): the ring first.
    pub shadows: Vec<BoxShadow>,
    /// translate in px, plus a fraction of the element's own size (`-translate-y-1/2`).
    pub translate_px: [f32; 2],
    pub translate_frac: [f32; 2],
    pub scale: f32,
    pub rotate_deg: f32,
    /// transform-origin as fractions of the box (CSS default centre).
    pub origin: [f32; 2],
    /// Children are clipped to the padding box (overflow hidden or scrolling).
    pub clip: bool,
    pub z: i32,
    pub hidden: bool,
    pub blend_lighten: bool,
    pub grayscale: f32,
    pub drop_shadow: Option<BoxShadow>,
    pub backdrop_blur: f32,
    pub outline: Option<(f32, Rgba, f32)>,
    pub object_fit: Option<Fit>,
    /// object-position as fractions (0..1) of the free space.
    pub object_position: [f32; 2],
}

fn frac(l: Len) -> f32 {
    match l {
        Len::Pct(p) => p / 100.0,
        _ => 0.0,
    }
}

impl Decls {
    /// The look at `rem`, with `color` the element's text colour (currentColor for borders, rings).
    pub fn look(&self, rem: f32, color: Rgba) -> Look {
        let em = self.font_size.and_then(|l| l.px(rem, rem, 0.0)).unwrap_or(rem);
        let px = |l: Len| l.px(rem, em, 0.0).unwrap_or(0.0);
        let mut radius = [0.0; 4];
        let mut radius_pct = [None; 4];
        for i in 0..4 {
            match self.radius[i] {
                Some(Len::Pct(p)) => radius_pct[i] = Some(p / 100.0),
                Some(l) => radius[i] = px(l),
                None => {}
            }
        }
        let shadow = |s: &Shadow, c: Rgba| BoxShadow { x: px(s.x), y: px(s.y), blur: px(s.blur), spread: px(s.spread), color: c };
        let mut shadows = Vec::new();
        if let Some(w) = self.ring {
            shadows.push(BoxShadow { x: 0.0, y: 0.0, blur: 0.0, spread: px(w), color: self.ring_color.unwrap_or(color) });
        }
        if let Some(sh) = &self.shadow {
            for s in sh.iter() {
                shadows.push(shadow(s, self.shadow_color.unwrap_or(s.color)));
            }
        }
        let tr = |l: Option<Len>| match l {
            Some(Len::Pct(_)) | None => 0.0,
            Some(l) => px(l),
        };
        let clip = self.overflow.iter().any(|o| matches!(o, Some(Overflow::Hidden | Overflow::Auto)));
        Look {
            bg: self.bg.unwrap_or(Rgba::TRANSPARENT),
            gradient: self.gradient(),
            radius,
            radius_pct,
            border: self.border.map(|b| b.map(px).unwrap_or(0.0)),
            border_color: self.border_color.unwrap_or(color),
            opacity: self.opacity.unwrap_or(1.0),
            shadows,
            translate_px: [tr(self.translate[0]), tr(self.translate[1])],
            translate_frac: [self.translate[0].map(frac).unwrap_or(0.0), self.translate[1].map(frac).unwrap_or(0.0)],
            scale: self.scale.unwrap_or(1.0),
            rotate_deg: self.rotate.unwrap_or(0.0),
            origin: self.transform_origin.map(|[x, y]| [frac(x), frac(y)]).unwrap_or([0.5, 0.5]),
            clip,
            z: self.z.unwrap_or(0),
            hidden: self.visible == Some(false) || self.display == Some(Display::None),
            blend_lighten: self.blend == Some(Blend::Lighten),
            grayscale: self.grayscale.unwrap_or(0.0),
            drop_shadow: self.drop_shadow.as_ref().map(|s| shadow(s, s.color)),
            backdrop_blur: self.backdrop_blur.map(px).unwrap_or(0.0),
            outline: match (self.outline_style, self.outline_width) {
                (Some(OutlineStyle::Solid), Some(w)) => Some((px(w), self.outline_color.unwrap_or(color), self.outline_offset.map(px).unwrap_or(0.0))),
                _ => None,
            },
            object_fit: self.object_fit,
            object_position: self.object_position.map(|[x, y]| [frac(x), frac(y)]).unwrap_or([0.5, 0.5]),
        }
    }
}

/// Inherited text properties, resolved (CSS computed values).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Text {
    pub size_px: f32,
    /// Inherits as a multiple when set as a number, as px when set as a length (CSS).
    pub line: LineHeight,
    pub weight: u16,
    pub family: Family,
    pub color: Rgba,
    pub tracking_px: f32,
    pub align: TextAlign,
    pub uppercase: bool,
    pub decoration: Decoration,
    pub white_space: WhiteSpace,
    pub break_all: bool,
    pub break_words: bool,
    pub tabular: bool,
    pub ligatures: bool,
}

impl Text {
    /// The launcher's root: 1rem Inter, line-height 1.5, fg (app.css html/body + Tailwind preflight).
    pub fn root(rem: f32) -> Text {
        Text {
            size_px: rem,
            line: LineHeight::Num(1.5),
            weight: 400,
            family: Family::Sans,
            color: crate::tokens::color("fg").unwrap_or(Rgba::rgb(242, 242, 243)),
            tracking_px: 0.0,
            align: TextAlign::Left,
            uppercase: false,
            decoration: Decoration::None,
            white_space: WhiteSpace::Normal,
            break_all: false,
            break_words: false,
            tabular: false,
            ligatures: true,
        }
    }

    /// Line height in px at this text's size.
    pub fn line_px(&self) -> f32 {
        match self.line {
            LineHeight::Num(k) => k * self.size_px,
            LineHeight::Len(Len::Px(v)) => v,
            LineHeight::Len(_) => self.size_px * 1.5,
        }
    }

    /// A child's text: inherits this, then applies the child's declarations (rem for its units).
    pub fn child(&self, d: &Decls, rem: f32) -> Text {
        let mut t = *self;
        if let Some(l) = d.font_size {
            t.size_px = l.px(rem, self.size_px, self.size_px).unwrap_or(self.size_px);
        }
        if let Some(lh) = d.effective_line_height() {
            t.line = match lh {
                LineHeight::Num(k) => LineHeight::Num(k),
                LineHeight::Len(l) => LineHeight::Len(Len::Px(l.px(rem, t.size_px, t.size_px).unwrap_or(t.size_px))),
            };
        }
        if let Some(tr) = d.effective_tracking() {
            // letter-spacing computes to an absolute length at the element's own size and inherits so.
            t.tracking_px = tr.px(rem, t.size_px, 0.0).unwrap_or(0.0);
        }
        if let Some(w) = d.weight {
            t.weight = w;
        }
        if let Some(f) = d.family {
            t.family = f;
        }
        if let Some(c) = d.color {
            t.color = c;
        }
        if let Some(a) = d.text_align {
            t.align = a;
        }
        if let Some(u) = d.uppercase {
            t.uppercase = u;
        }
        if let Some(x) = d.decoration {
            // text-decoration does not inherit in CSS, but it propagates to the text inside.
            t.decoration = x;
        }
        if let Some(w) = d.white_space {
            t.white_space = w;
        }
        if let Some(b) = d.break_all {
            t.break_all = b;
        }
        if let Some(b) = d.break_words {
            t.break_words = b;
        }
        if let Some(b) = d.tabular {
            t.tabular = b;
        }
        if let Some(b) = d.ligatures {
            t.ligatures = b;
        }
        t
    }

    /// One line, no wrapping (`whitespace-nowrap`, `truncate`).
    pub fn nowrap(&self) -> bool {
        self.white_space == WhiteSpace::NoWrap
    }
}

impl Tw {
    /// The element's look at `rem`, given its parent's text (for currentColor).
    pub fn look(&self, rem: f32, parent: &Text) -> Look {
        let color = self.base.color.unwrap_or(parent.color);
        self.base.look(rem, color)
    }
    /// The look in a state (focus-visible:, disabled:...).
    pub fn look_in(&self, when: &When, rem: f32, parent: &Text) -> Look {
        let d = self.with_state(when);
        d.look(rem, d.color.unwrap_or(parent.color))
    }
    /// The element's text context.
    pub fn text(&self, parent: &Text, rem: f32) -> Text {
        parent.child(&self.base, rem)
    }
    /// Text is cut with an ellipsis at the box (`truncate`, `.trow` cells).
    pub fn ellipsis(&self) -> bool {
        self.base.ellipsis == Some(true)
    }
    /// `line-clamp-N`
    pub fn line_clamp(&self) -> Option<u16> {
        self.base.line_clamp
    }
}
