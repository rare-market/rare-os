//! Phase 3 markup to Rare Render views (WS 14): each `El<M>` of a screen (TSX elements with their
//! Tailwind class strings verbatim) becomes a `rare_render::view::El`. Classes resolve through
//! rare-tw (`parse` at run time, cached per string; WS 3's parser, checked against Chromium):
//! layout to a Taffy `Style`, paint to the node's `Look`, CSS text inheritance to a `TextStyle`.
//! Focus follows app.css: `.btn` fills white with dark text and scales 1.04, `.tile` 1.06,
//! `.tile-wide` 1.03, other `[data-nav]` rows fill layer 04, plus each element's own
//! `focus-visible:` classes. A focusable's tag indexes `msgs`, so A comes back as the screen's
//! message. Icons, art, glyphs and QR codes come from the kit.
//!
//! What CSS does that Taffy does not: inline formatting. A block whose children are inline
//! (spans, text, icons) lays them out as a wrapping row on the baseline, which is what a single
//! line of mixed text looks like in the browser.

use crate::kit::{self, tok, ArtFit, Ui};
use super::markup::{El as M, Nav, Tag};
use rare_render::focus::Focusable;
use rare_render::scene::{GradDir, Gradient, Shadow};
use rare_render::taffy::prelude::*;
use rare_render::taffy::style::{AlignItems, Display, FlexDirection, FlexWrap, Position};
use rare_render::text::TextAlign;
use rare_render::view::{key, El, Look, Transition};
use rare_render::{Color, TextStyle};
use rare_tw::look::Text;
use rare_tw::{Marks, Tw, When};
use std::collections::HashMap;

pub struct Render<'u, 'a, Msg> {
    pub ui: &'u mut Ui<'a>,
    /// tag -> message (a focusable's tag is its index here)
    pub msgs: Vec<Option<Msg>>,
    /// the page's acts: a focusable's tag becomes `Act::Local(index into msgs)`
    acts: Option<&'u mut crate::page::Acts>,
    /// another workstream's element for a `Tag::Slot`
    pub slots: HashMap<String, El>,
    path: Vec<usize>,
    /// text colour while the nearest focusable ancestor has focus (`.btn` -> dark text)
    focus_text: Option<Color>,
}

fn color(c: rare_tw::Rgba) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: c.a }
}

fn is_inline(e: &M<impl Clone>, tw: &Tw) -> bool {
    use rare_tw::model::Display as D;
    match tw.base.display {
        Some(D::Inline) | Some(D::InlineBlock) | Some(D::InlineFlex) => true,
        Some(_) => false,
        None => matches!(e.tag, Tag::Span | Tag::Code | Tag::Img { .. } | Tag::Icon(_) | Tag::WalletAvatar(_) | Tag::Glyph(_)) || (e.tag == Tag::Button && false),
    }
}

/// `style="width: 2rem; height: 2rem"` and the like, in rem (calc(x * k) supported for SuiLogo).
fn style_attr(s: &str, rem: f32, st: &mut Style) {
    for decl in s.split(';') {
        let Some((k, v)) = decl.split_once(':') else { continue };
        let v = v.trim();
        let len = |v: &str| -> Option<f32> {
            if let Some(inner) = v.strip_prefix("calc(").and_then(|x| x.strip_suffix(')')) {
                let (a, b) = inner.split_once('*')?;
                return Some(a.trim().strip_suffix("rem")?.parse::<f32>().ok()? * b.trim().parse::<f32>().ok()? * rem);
            }
            if let Some(r) = v.strip_suffix("rem") {
                return r.parse::<f32>().ok().map(|x| x * rem);
            }
            if let Some(p) = v.strip_suffix("px") {
                return p.parse::<f32>().ok();
            }
            None
        };
        match k.trim() {
            "width" => {
                if v == "auto" {
                    st.size.width = auto();
                } else if let Some(p) = v.strip_suffix('%').and_then(|p| p.parse::<f32>().ok()) {
                    st.size.width = percent(p / 100.0);
                } else if let Some(x) = len(v) {
                    st.size.width = length(x);
                }
            }
            "height" => {
                if let Some(p) = v.strip_suffix('%').and_then(|p| p.parse::<f32>().ok()) {
                    st.size.height = percent(p / 100.0);
                } else if let Some(x) = len(v) {
                    st.size.height = length(x);
                }
            }
            "max-width" => {
                if let Some(x) = len(v) {
                    st.max_size.width = length(x);
                }
            }
            "grid-template-columns" => {
                if let Some(n) = v.strip_prefix("repeat(").and_then(|r| r.split(',').next()).and_then(|n| n.trim().parse::<u16>().ok()) {
                    st.grid_template_columns = vec![repeat(n, vec![minmax(length(0.0), fr(1.0))])];
                }
            }
            _ => {}
        }
    }
}

/// One pixel of slack on a text leaf's right (padding +1, margin -1: the same outer size).
/// `Scene::layout` re-measures wrapped and truncating text at the rounded box width, a fraction
/// under its own measure for a shrink-to-fit box, so "Live" wrapped to "Liv/e" (swarm request to
/// WS 1, line 100). Remove once that pass uses the unrounded width.
fn slack(mut el: El) -> El {
    let pr = el.style.padding.right.into_raw();
    let mr = el.style.margin.right.into_raw();
    if pr.tag() == rare_render::taffy::style::CompactLength::LENGTH_TAG && mr.tag() == rare_render::taffy::style::CompactLength::LENGTH_TAG {
        el.style.padding.right = length(pr.value() + 1.0);
        el.style.margin.right = length(mr.value() - 1.0);
    }
    el
}

/// Hairlines for borders on some sides, as flow children of a wrapper that takes the element's
/// place (its box: size, margins, flex item and position, look); the element fills the rest.
/// Absolute hairlines were sized against the window by taffy (a stale cached pass), not the box.
/// `sides` is top, right, bottom, left.
fn side_borders(el: El, sides: [f32; 4], c: Color) -> El {
    let mut inner = el;
    let outer_style = Style {
        display: Display::Flex,
        position: inner.style.position,
        inset: inner.style.inset,
        size: inner.style.size,
        min_size: inner.style.min_size,
        max_size: inner.style.max_size,
        aspect_ratio: inner.style.aspect_ratio,
        margin: inner.style.margin,
        flex_grow: inner.style.flex_grow,
        flex_shrink: inner.style.flex_shrink,
        flex_basis: inner.style.flex_basis,
        align_self: inner.style.align_self,
        justify_self: inner.style.justify_self,
        grid_row: inner.style.grid_row.clone(),
        grid_column: inner.style.grid_column.clone(),
        ..Default::default()
    };
    let look = std::mem::take(&mut inner.look);
    let z = length(0.0);
    inner.style.position = Position::Relative;
    inner.style.inset = rare_render::taffy::Rect { left: auto(), right: auto(), top: auto(), bottom: auto() };
    inner.style.margin = rare_render::taffy::Rect { left: z, right: z, top: z, bottom: z };
    inner.style.border = rare_render::taffy::Rect { left: length(0.0), right: length(0.0), top: length(0.0), bottom: length(0.0) };
    inner.style.size = Size { width: auto(), height: auto() };
    inner.style.min_size = Size { width: length(0.0), height: length(0.0) };
    inner.style.max_size = Size { width: auto(), height: auto() };
    inner.style.aspect_ratio = None;
    inner.style.flex_grow = 1.0;
    inner.style.flex_shrink = 1.0;
    inner.style.flex_basis = auto();
    inner.style.align_self = None;
    let hair = |w: f32, vertical: bool| {
        let size = if vertical { Size { width: length(w), height: auto() } } else { Size { width: auto(), height: length(w) } };
        El::new(Style { size, flex_shrink: 0.0, ..Default::default() }).bg(c)
    };
    // left and right in a row, then top and bottom in a column around it
    let mut mid = inner;
    if sides[1] > 0.0 || sides[3] > 0.0 {
        let mut row = El::new(Style { display: Display::Flex, flex_direction: FlexDirection::Row, flex_grow: 1.0, min_size: Size { width: length(0.0), height: length(0.0) }, ..Default::default() });
        if sides[3] > 0.0 {
            row = row.child(hair(sides[3], true));
        }
        row = row.child(mid);
        if sides[1] > 0.0 {
            row = row.child(hair(sides[1], true));
        }
        mid = row;
    }
    let mut col = El::new(Style { flex_direction: FlexDirection::Column, ..outer_style });
    if sides[0] > 0.0 {
        col = col.child(hair(sides[0], false));
    }
    col = col.child(mid);
    if sides[2] > 0.0 {
        col = col.child(hair(sides[2], false));
    }
    col.look = look;
    col
}

fn text_style(t: &Text, ellipsis: bool, clamp: Option<u16>) -> TextStyle {
    let mut st = TextStyle::new(t.size_px, t.weight).line(t.line_px().round());
    if t.size_px > 0.0 && t.tracking_px != 0.0 {
        st = st.tracking(t.tracking_px / t.size_px);
    }
    st = st.align(match t.align {
        rare_tw::model::TextAlign::Center => TextAlign::Center,
        rare_tw::model::TextAlign::Right => TextAlign::Right,
        _ => TextAlign::Left,
    });
    st.tabular = t.tabular;
    if t.family == rare_tw::model::Family::Mono {
        st = st.mono();
    }
    if ellipsis {
        st.truncate = true;
    } else if !t.nowrap() {
        st.wrap = true;
    }
    if let Some(n) = clamp {
        st.clamp = n;
    }
    st
}

fn grad(g: &rare_tw::model::Gradient) -> Option<Gradient> {
    let dir = match ((g.angle % 360.0) + 360.0) % 360.0 {
        a if (a - 0.0).abs() < 1.0 => GradDir::ToTop,
        a if (a - 90.0).abs() < 1.0 => GradDir::ToRight,
        a if (a - 180.0).abs() < 1.0 => GradDir::ToBottom,
        a if (a - 270.0).abs() < 1.0 => GradDir::ToLeft,
        _ => GradDir::ToRight,
    };
    let s = &g.stops;
    let (from, via, to, mid) = match s.len() {
        0 => return None,
        1 => (s[0].color, s[0].color, s[0].color, 0.5),
        2 => {
            let mix = rare_tw::Rgba { r: (s[0].color.r + s[1].color.r) / 2.0, g: (s[0].color.g + s[1].color.g) / 2.0, b: (s[0].color.b + s[1].color.b) / 2.0, a: (s[0].color.a + s[1].color.a) / 2.0 };
            (s[0].color, mix, s[1].color, 0.5)
        }
        _ => (s[0].color, s[1].color, s[s.len() - 1].color, s[1].pos.map(|p| p / 100.0).unwrap_or(0.5)),
    };
    Some(Gradient { dir, stops: [color(from), color(via), color(to)], mid })
}

fn look_of(l: &rare_tw::look::Look) -> Look {
    let r = l.radius[0];
    let mut look = Look::default().bg(color(l.bg)).radius(if r.is_infinite() { 9999.0 } else { r }).opacity(l.opacity);
    let bw = l.border.iter().cloned().fold(0.0f32, f32::max);
    if bw > 0.0 && l.border.iter().all(|b| (*b - bw).abs() < 1e-3) {
        look = look.border(bw, color(l.border_color));
    }
    if let Some(g) = l.gradient.as_ref().and_then(grad) {
        look = look.grad(g);
    }
    if !l.shadows.is_empty() {
        let s: Vec<Shadow> = l.shadows.iter().map(|s| Shadow::new(s.x, s.y, s.blur, s.spread, color(s.color))).collect();
        look = look.shadow(&s);
    }
    if l.clip {
        look = look.clip();
    }
    if l.z != 0 {
        look = look.z(l.z.clamp(-100, 100) as i16);
    }
    if (l.scale - 1.0).abs() > 1e-4 {
        look = look.scale(l.scale);
    }
    look = look.origin(l.origin[0], l.origin[1]);
    if l.translate_px != [0.0, 0.0] {
        look = look.translate(l.translate_px[0], l.translate_px[1]);
    }
    look.hidden(l.hidden)
}

fn pb(b: &str) -> kit::Pb {
    use kit::Pb as P;
    match b {
        "a" => P::A,
        "b" => P::B,
        "x" => P::X,
        "y" => P::Y,
        "lb" => P::Lb,
        "rb" => P::Rb,
        "lt" => P::Lt,
        "rt" => P::Rt,
        "start" => P::Start,
        "select" => P::Select,
        "qam" | "quick" => P::Quick,
        "guide" | "rare" => P::Rare,
        _ => P::A,
    }
}

/// `size="1.1rem"` on an icon, else its class's width, else 1rem.
fn icon_size(e: &M<impl Clone>, st: &Style, rem: f32) -> f32 {
    if let Some(v) = e.get_attr("size").and_then(|s| s.strip_suffix("rem")).and_then(|s| s.parse::<f32>().ok()) {
        return v;
    }
    match st.size.width.into_option() {
        Some(px) => px / rem,
        None => 1.0,
    }
}

impl<'u, 'a, Msg: Clone> Render<'u, 'a, Msg> {
    pub fn new(ui: &'u mut Ui<'a>) -> Self {
        Render { ui, msgs: Vec::new(), acts: None, slots: HashMap::new(), path: Vec::new(), focus_text: None }
    }

    /// For an app page: tags come from its `Acts`, A arrives as `Act::Local(i)`, `msgs[i]` is the message.
    pub fn with_acts(ui: &'u mut Ui<'a>, acts: &'u mut crate::page::Acts) -> Self {
        Render { ui, msgs: Vec::new(), acts: Some(acts), slots: HashMap::new(), path: Vec::new(), focus_text: None }
    }

    fn tw(&mut self, class: &str) -> Tw {
        thread_local! {
            static CACHE: std::cell::RefCell<HashMap<String, Tw>> = std::cell::RefCell::new(HashMap::new());
        }
        CACHE.with(|c| {
            if let Some(t) = c.borrow().get(class) {
                return t.clone();
            }
            let t = rare_tw::parse(class).unwrap_or_else(|e| {
                log::warn!("rare-tw: {class:?}: {e:?}");
                rare_tw::parse("").expect("empty parses")
            });
            c.borrow_mut().insert(class.to_string(), t.clone());
            t
        })
    }

    /// The whole tree; the root's text context is the page's (Inter 1rem, fg).
    pub fn render(&mut self, root: &M<Msg>) -> El {
        let t = Text::root(self.ui.rem);
        self.node(root, &t, None, true, true)
    }

    fn node(&mut self, e: &M<Msg>, parent_text: &Text, inherited: Option<&rare_tw::model::Decls>, first: bool, last: bool) -> El {
        let rem = self.ui.rem;
        let mut tw = self.tw(&e.class);
        if let Some(d) = inherited {
            let mut base = d.clone();
            base.overlay(&tw.base);
            tw.base = base;
        }
        for (cond, w) in [(first, When::First), (last, When::Last)] {
            if cond {
                if let Some(d) = tw.state(&w).cloned() {
                    tw.base.overlay(&d);
                }
            }
        }
        let disabled = e.nav.as_ref().is_some_and(|n| n.disabled);
        if disabled {
            if let Some(d) = tw.state(&When::Disabled).cloned() {
                tw.base.overlay(&d);
            }
        }
        let text = tw.text(parent_text, rem);
        let saved_focus_text = self.focus_text;
        if e.nav.is_some() {
            if tw.has(Marks::BTN) {
                self.focus_text = Some(tok::BG);
            } else if let Some(c) = tw.state(&When::FocusVisible).and_then(|d| d.color) {
                self.focus_text = Some(color(c));
            }
        }
        let ft = self.focus_text;
        let txt = move |s: String, ts: TextStyle, c: Color| -> El {
            let t = slack(El::text(s, ts, c));
            match ft {
                Some(f) => t.focus_color(f),
                None => t,
            }
        };
        let mut style = tw.style(rem);
        if let Some(s) = e.get_attr("style") {
            style_attr(s, rem, &mut style);
        }
        let l = tw.look(rem, parent_text);
        let look = look_of(&l);
        let fixed = matches!(tw.base.position, Some(rare_tw::model::Position::Fixed));
        if fixed {
            style.position = Position::Absolute;
        }

        // Leaf content by tag.
        let mut el = match &e.tag {
            Tag::Icon(name) => {
                let size = icon_size(e, &style, rem);
                let c = color(text.color);
                let stroke = e.get_attr("stroke-width").and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.75);
                let mut ic = match *name {
                    "sui" => {
                        let h = style.size.height.into_option().map(|px| px / rem).unwrap_or(size);
                        kit::sui_logo(self.ui, h)
                    }
                    "steam" => kit::steam(self.ui, size, c),
                    "triangle" => kit::icon::svg_tinted(self.ui, "delta-triangle", r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><path d="M4 1 7.5 7h-7z" fill="white"/></svg>"#.into(), size, size, c),
                    "verified" => kit::icon::svg_box(self.ui, "verified", r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path fill="#3D5AFE" d="M8 .8l1.7 1.3 2.1-.2.8 2 2 .8-.2 2.1L15.2 8l-1.3 1.7.2 2.1-2 .8-.8 2-2.1-.2L8 15.2l-1.7-1.3-2.1.2-.8-2-2-.8.2-2.1L.8 8l1.3-1.7-.2-2.1 2-.8.8-2 2.1.2z"/><path d="M5.2 8.2 7.1 10l3.8-4" fill="none" stroke="#fff" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>"##.into(), size, size, kit::s()),
                    "rareling" => kit::icon::svg_box(self.ui, "rareling", r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M8 1.2 13.8 5 11.6 14H4.4L2.2 5z" fill="#3D5AFE"/><path d="M2.2 5h11.6M8 1.2 5.6 5 8 14l2.4-9z" fill="none" stroke="rgba(255,255,255,0.55)" stroke-width="0.9" stroke-linejoin="round"/></svg>"##.into(), size, size, kit::s()),
                    n => match super::icons::body(n) {
                        Some(body) => {
                            let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="{stroke}" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"#);
                            kit::icon::svg_tinted(self.ui, &format!("lucide-x-{n}-{stroke}"), svg, size, size, c)
                        }
                        None => kit::icon(self.ui, n, size, c),
                    },
                };
                // an icon inside a focusable takes the focused text colour too (dark on the white .btn)
                if let (Some(f), false) = (self.focus_text, matches!(*name, "sui" | "verified" | "rareling" | "triangle")) {
                    ic = ic.focus_color(f);
                }
                ic.style.margin = style.margin;
                ic.style.position = style.position;
                ic.style.inset = style.inset;
                ic.look = look.clone();
                // a button's icon takes the dark focus colour with its label
                match (ft, name) {
                    (Some(f), n) if !matches!(*n, "sui" | "verified" | "rareling") => ic.focus_color(f),
                    _ => ic,
                }
            }
            Tag::Glyph(b) => kit::glyph(self.ui, pb(b), if e.get_attr("data-size") == Some("sm") { kit::GSize::Sm } else { kit::GSize::Md }),
            Tag::Img { src, alt } => {
                let w = e.get_attr("data-width").and_then(|w| w.parse::<f32>().ok()).unwrap_or(400.0) / 16.0;
                let ah = e.get_attr("data-height").and_then(|h| h.parse::<f32>().ok()).map(|h| h / 16.0);
                let fit = if l.object_fit == Some(rare_tw::model::Fit::Contain) || e.get_attr("data-contain").is_some() { ArtFit::Contain(0.5, 0.5) } else { ArtFit::Cover(l.object_position[1]) };
                let sources: Vec<String> = src.iter().cloned().chain(e.get_attr("data-sources").map(|s| s.split(' ').map(str::to_string).collect::<Vec<_>>()).unwrap_or_default()).filter(|s| !s.is_empty()).collect();
                let initial = alt.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_default();
                let fb = El::text(initial, TextStyle::new(1.4 * rem, 600).line(1.4 * rem * 1.5), tok::fg(0.35));
                let fb = El::new(Style { display: Display::Flex, align_items: AlignItems::CENTER, justify_content: rare_render::taffy::style::JustifyContent::CENTER, size: Size { width: percent(1.0), height: percent(1.0) }, ..Default::default() }).bg(tok::L3).child(fb);
                let mut im = kit::art(self.ui, kit::s(), &sources, w, ah.unwrap_or(w), fit, Some(fb));
                let mut style = style.clone();
                // width: auto beside a set height (BrandMark): the file's own aspect ratio
                if style.size.width.is_auto() && style.aspect_ratio.is_none() {
                    if let Some(rare_render::Src::File(p)) = sources.first().and_then(|s| crate::data::art::src_of(s)) {
                        if let Ok((iw, ih)) = image::image_dimensions(&p) {
                            style.aspect_ratio = Some(iw as f32 / ih.max(1) as f32);
                        }
                    }
                }
                im.style = style;
                im.look = look.clone().bg(Color::TRANSPARENT);
                im
            }
            Tag::Qr(text) | Tag::WalletAvatar(text) => {
                let px = |d: rare_render::taffy::style::Dimension| if d.tag() == rare_render::taffy::style::CompactLength::LENGTH_TAG { Some(d.value()) } else { None };
                let w = px(style.size.width).or(px(style.size.height)).unwrap_or(rem * 2.0) / rem;
                let h = px(style.size.height).unwrap_or(w * rem) / rem;
                let (name, svg, radius) = if matches!(e.tag, Tag::Qr(_)) {
                    (format!("qr-{text}"), crate::screens::market::buy::qr_svg(text), rem * 0.25)
                } else {
                    (format!("wallet-avatar-{text}"), rare_market::identicon::avatar_svg(text), 9999.0)
                };
                let mut im = kit::icon::svg_box(self.ui, &name, svg, w, h, kit::s());
                im.style = Style { size: Size { width: percent(1.0), height: percent(1.0) }, ..Default::default() };
                El::new(style.clone()).look(look.clone()).radius(radius).clip().child(im)
            }
            Tag::Slot(name) => self.slots.remove(name).unwrap_or_else(|| El::new(style.clone())),
            _ => El::new(style.clone()).look(look.clone()),
        };

        // Text and children.
        let has_text = e.text.as_deref().is_some_and(|t| !t.is_empty());
        if matches!(e.tag, Tag::Div | Tag::Span | Tag::P | Tag::H1 | Tag::H2 | Tag::H3 | Tag::Section | Tag::Ul | Tag::Ol | Tag::Li | Tag::Pre | Tag::Code | Tag::Button) {
            let ts = text_style(&text, tw.ellipsis(), tw.line_clamp());
            let tc = color(text.color);
            let padded = [style.padding.left, style.padding.right, style.padding.top, style.padding.bottom].iter().any(|p| p.into_raw().tag() != rare_render::taffy::style::CompactLength::LENGTH_TAG || p.into_raw().value() != 0.0);
            if has_text && e.kids.is_empty() && style.display == Display::Block && !padded {
                // A block text leaf: the box carries the text itself.
                el = slack(txt(e.text.clone().unwrap_or_default(), ts, tc).with_style(style.clone()).look(look.clone()));
            } else if has_text && e.kids.is_empty() {
                // A flex or grid box (or a padded block): the text is its one item, so alignment applies.
                el = el.child(txt(e.text.clone().unwrap_or_default(), ts, tc));
            } else {
                let mut kids: Vec<El> = Vec::new();
                if has_text {
                    kids.push(txt(e.text.clone().unwrap_or_default(), ts, tc));
                }
                let child_decls = tw.state(&When::Children).cloned();
                let not_last = tw.state(&When::ChildrenNotLast).cloned();
                let n = e.kids.len();
                let inline_kids: Vec<bool> = e.kids.iter().map(|k| {
                    let ktw = self.tw(&k.class);
                    is_inline(k, &ktw)
                }).collect();
                for (i, k) in e.kids.iter().enumerate() {
                    self.path.push(i);
                    let mut inh = child_decls.clone();
                    if i + 1 < n {
                        if let Some(d) = &not_last {
                            match inh.as_mut() {
                                Some(x) => x.overlay(d),
                                None => inh = Some(d.clone()),
                            }
                        }
                    }
                    let mut ke = self.node(k, &text, inh.as_ref(), i == 0, i + 1 == n);
                    self.path.pop();
                    if let Some(kk) = &k.key {
                        ke = ke.key(key(kk));
                    }
                    kids.push(ke);
                }
                // A block of inline children: one wrapping row on the baseline.
                let block = style.display == Display::Block;
                if block && !e.kids.is_empty() && (inline_kids.iter().all(|x| *x) || has_text && inline_kids.iter().all(|x| *x)) {
                    el.style.display = Display::Flex;
                    el.style.flex_direction = FlexDirection::Row;
                    el.style.flex_wrap = FlexWrap::Wrap;
                    el.style.align_items = AlignItems::BASELINE;
                    if text.align == rare_tw::model::TextAlign::Center {
                        el.style.justify_content = rare_render::taffy::style::JustifyContent::CENTER;
                    } else if text.align == rare_tw::model::TextAlign::Right {
                        el.style.justify_content = rare_render::taffy::style::JustifyContent::FLEX_END;
                    }
                }
                el = el.kids(kids);
            }
        }

        // Borders on some sides only (border-b, divide-y): hairlines, the engine has one border.
        let bw = l.border.iter().cloned().fold(0.0f32, f32::max);
        if bw > 0.0 && !l.border.iter().all(|b| (*b - bw).abs() < 1e-3) {
            el = side_borders(el, l.border, color(l.border_color));
        }

        self.focus_text = saved_focus_text;
        // Focus, groups, scroll containers, portals.
        if let Some(nav) = &e.nav {
            el = self.focus(el, &tw, nav, parent_text, &l);
        }
        if let Some(g) = e.get_attr("data-nav-group") {
            let mut grp = kit::group(g, e.get_attr("data-nav-remember").is_some());
            if e.get_attr("data-rail").is_some() {
                grp = grp.rail();
            }
            el = el.group(grp);
        }
        let path_key = key(&(self.path.clone(), e.class.clone()));
        // `[data-scroll]` pages and every other `overflow-y-auto` box (a sheet body, a side panel)
        // scroll and follow focus; `[data-rail]` and `overflow-x-auto` strips scroll sideways.
        let ov = tw.style(rem).overflow;
        let (ov_x, ov_y) = (ov.x, ov.y);
        let scroll_ov = |o: rare_render::taffy::style::Overflow| o == rare_render::taffy::style::Overflow::Scroll;
        if e.get_attr("data-rail").is_some() || (scroll_ov(ov_x) && !scroll_ov(ov_y) && e.get_attr("data-scroll").is_none()) {
            el = el.scrolls_x(path_key);
        } else if e.get_attr("data-scroll").is_some() || scroll_ov(ov_y) {
            el = el.scrolls_y(path_key);
        }
        if e.get_attr("data-nav-scope").is_some() {
            el = el.scope(1);
        }
        if fixed {
            el = el.portal(l.z.clamp(1, 100) as i16);
        }
        el
    }

    fn focus(&mut self, el: El, tw: &Tw, nav: &Nav<Msg>, parent_text: &Text, base: &rare_tw::look::Look) -> El {
        let rem = self.ui.rem;
        let i = self.msgs.len() as u32;
        self.msgs.push(nav.on.clone());
        let tag = match self.acts.as_mut() {
            Some(a) => a.tag(crate::act::Act::Local(i)),
            None => i,
        };
        let mut fl = el.look.clone();
        let mut fcolor = None;
        // the element's own focus-visible: classes first; app.css's .btn / .tile rules win over them
        let mut own_bg = false;
        if let Some(d) = tw.state(&When::FocusVisible) {
            let fv = tw.look_in(&When::FocusVisible, rem, parent_text);
            if d.bg.is_some() {
                fl = fl.bg(color(fv.bg));
                own_bg = true;
            }
            if d.color.is_some() {
                fcolor = d.color.map(color);
            }
            if (fv.scale - base.scale).abs() > 1e-4 {
                fl = fl.scale(fv.scale);
            }
        }
        if tw.has(Marks::BTN) {
            fl = fl.bg(tok::FG).scale(1.04);
            fcolor = Some(tok::BG);
        } else if tw.has(Marks::TILE_WIDE) {
            fl = fl.scale(1.03);
        } else if tw.has(Marks::TILE) {
            fl = fl.scale(1.06);
        } else if !own_bg {
            fl = fl.bg(tok::L4);
        }
        let mut el = el.focus_look(fl).transition(Transition::FOCUS);
        if let Some(c) = fcolor {
            el = el.focus_color(c);
        }
        let hint = nav.hint_a.clone();
        let def = nav.default;
        let disabled = nav.disabled;
        el.nav(|f: Focusable| {
            let mut f = f.tag(tag).hints(hint.as_deref(), None, None).disabled(disabled);
            if def {
                f = f.default();
            }
            f
        })
    }
}

/// One build of a markup screen for an app page: the view, the messages by `Act::Local` code, and
/// the pads the screen answers (innermost wins, like nested `usePadHandler`s).
pub struct Built<Msg> {
    pub msgs: Vec<Option<Msg>>,
    pub pads: Vec<(&'static str, Msg)>,
}

impl<Msg: Clone> Default for Built<Msg> {
    fn default() -> Self {
        Built { msgs: Vec::new(), pads: Vec::new() }
    }
}

impl<Msg: Clone> Built<Msg> {
    pub fn msg(&self, code: u32) -> Option<Msg> {
        self.msgs.get(code as usize).cloned().flatten()
    }
    pub fn pad(&self, b: &str) -> Option<Msg> {
        self.pads.iter().rev().find(|(x, _)| *x == b).map(|(_, m)| m.clone())
    }
}

fn collect_pads<Msg: Clone>(e: &M<Msg>, out: &mut Vec<(&'static str, Msg)>) {
    out.extend(e.pads.iter().cloned());
    for k in &e.kids {
        collect_pads(k, out);
    }
}

/// Renders `m` for a page (`Page::view`): tags through `acts`.
/// Moves every `[data-nav-scope]` focus scope from `from` to `to`: a host that is not the overlay
/// (Rare Wallet's review sheet, scope 20) must route its presses to itself and trap focus over the
/// page and any overlay.
pub fn rescope(el: &mut El, from: u32, to: u32) {
    if el.scope == Some(from) {
        el.scope = Some(to);
    }
    for k in &mut el.kids {
        rescope(k, from, to);
    }
}

pub fn build<Msg: Clone>(ui: &mut Ui, acts: &mut crate::page::Acts, m: &M<Msg>) -> (El, Built<Msg>) {
    let mut pads = Vec::new();
    collect_pads(m, &mut pads);
    let mut r = Render::with_acts(ui, acts);
    let el = r.render(m);
    let msgs = std::mem::take(&mut r.msgs);
    (el, Built { msgs, pads })
}

#[cfg(test)]
mod probe {
    use super::*;
    use crate::screens::wallet::markup::div;

    #[test]
    fn side_border_spans_its_box() {
        static FONTS: [&[u8]; 1] = [include_bytes!("../../../../render/assets/fonts/Inter-Regular.ttf")];
        let mut ts = rare_render::TextSys::new(&FONTS, false, 2048);
        let mut images = rare_render::ImageLoader::new(1, std::sync::Arc::new(|| {}), None);
        let mut arts = crate::kit::Arts::default();
        let mut scene = rare_render::Scene::new(1280.0, 800.0, 16.0);
        let m: M<u8> = div("h-[40rem]").kid(div("flex rounded-lg bg-l1 p-4").kid(div("flex min-w-0 flex-1 flex-col").text("A")).kid(div("flex min-w-0 flex-1 flex-col border-l border-hair pl-4").kid(div("").text("B")).kid(div("").text("C"))));
        let el = {
            let mut ui = Ui::new(16.0, 1280.0, 800.0, &mut images, &mut arts, 0.0);
            Render::new(&mut ui).render(&m)
        };
        let mut view = rare_render::view::View::new(scene.root);
        view.update(&mut scene, &mut ts, el, 0.0);
        scene.layout(&mut ts);
        fn walk(s: &rare_render::Scene, id: rare_render::Id, d: usize, out: &mut Vec<(usize, [f32; 4])>) {
            out.push((d, s.rect(id)));
            for c in s.children(id) {
                walk(s, c, d + 1, out);
            }
        }
        let mut out = Vec::new();
        walk(&scene, scene.root, 0, &mut out);
        let hair = out.iter().find(|(_, r)| r[2] <= 1.01 && r[3] > 0.0).map(|(_, r)| *r).expect("a hairline");
        assert!(hair[3] < 100.0, "hairline {hair:?}");
    }
}

