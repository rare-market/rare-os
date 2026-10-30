//! rare.market's pieces for the native Marketplace (marketplace/parts.tsx, MImg.tsx, media.ts and
//! market/Sections.tsx) as view elements: art through the image chain, Sui, Delta, Avatar,
//! WalletAvatar, VenueMark, RarityBadge, Empty, Btn, Tabs, Segmented, Stat, MoverCard, the top
//! collections table, the item card, the activity and holder rows, SectionTitle. Every interactive
//! piece is a focusable with the launcher's focus look (`.tile` scales, `.btn` fills white, the rest
//! fill layer 04).

use super::host::*;
use crate::kit::icon::{svg_box, svg_tinted};
use crate::kit::{delta, glyph, icon, sui as kit_sui, verified, GSize, Pb};
use rare_market::api::{CollectionRow, Holder, ItemCard, MarketEvent, Rarity};
use rare_market::format::{self as mf, SuiOpts, DASH};
use rare_market::nav::{Section, MARKET_SECTIONS};
use rare_render::view::{key, El, Look, Transition};
use rare_render::Color;

// ---------------------------------------------------------------- type scale and colours

/// Tailwind's default sizes (rem size, rem line height), which rare.market's classes use.
pub const XXS: Ty = Ty::Px(0.625, 0.9375);
pub const XS: Ty = Ty::Px(0.75, 1.0);
pub const SM: Ty = Ty::Px(0.875, 1.25);
pub const BASE: Ty = Ty::Px(1.0, 1.5);
pub const LG: Ty = Ty::Px(1.125, 1.75);
pub const XL: Ty = Ty::Px(1.25, 1.75);
pub const XL2: Ty = Ty::Px(1.5, 2.0);
pub const XL4: Ty = Ty::Px(2.25, 2.5);
/// `text-meta`
pub const META: Ty = Ty::Meta;

/// --border-primary
pub const BORDER: Color = Color::rgb(35, 30, 47);
/// --border-interactive-hover at 0.5 (a focused card's border)
pub const BORDER_HOVER: Color = Color::rgb(111, 104, 125).alpha(0.5);
/// --border-interactive-focus at 0.5 (a focused search field)
pub const BORDER_FOCUS: Color = Color::rgb(133, 127, 148).alpha(0.5);
/// --labels-neutral
pub const LABELS_NEUTRAL: Color = Color::rgb(46, 38, 64);
/// --text-labels-neutral
pub const LABEL_TEXT: Color = Color::rgb(162, 157, 175);
/// --text-royalties
pub const ROYALTIES: Color = Color::rgb(212, 189, 255);

pub fn t(ty: Ty) -> T {
    T::new(ty)
}

/// A CSS border's width in layout (the look paints it; this makes it take its room, as
/// box-sizing: border-box does).
pub fn bw(st: S, px: f32) -> S {
    use rare_render::taffy::prelude::{length, Rect};
    st.with(move |s| s.border = Rect { left: length(px), right: length(px), top: length(px), bottom: length(px) })
}

/// Only the left border's width (border-l), or only the bottom's (border-b).
pub fn bw_side(st: S, left: f32, bottom: f32) -> S {
    use rare_render::taffy::prelude::{length, Rect};
    st.with(move |s| s.border = Rect { left: length(left), right: length(0.0), top: length(0.0), bottom: length(bottom) })
}

/// border-t's width in layout.
pub fn bw_top(st: S, top: f32) -> S {
    use rare_render::taffy::prelude::{length, Rect};
    st.with(move |s| s.border = Rect { left: length(0.0), right: length(0.0), top: length(top), bottom: length(0.0) })
}

/// A hairline over the bottom border of its parent (border-b).
pub fn bottom_line(ui: &Ui) -> El {
    ui.bx(s().abs(None, Some(0.0), Some(0.0), Some(0.0)).h_px(1.0)).style(|st| st.inset.bottom = rare_render::taffy::prelude::length(-1.0)).bg(BORDER)
}

/// A hairline over the top border of its parent (border-t painted in the border box).
pub fn top_line(ui: &Ui) -> El {
    ui.bx(s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h_px(1.0)).style(|st| st.inset.top = rare_render::taffy::prelude::length(-1.0)).bg(BORDER)
}

// ---------------------------------------------------------------- MImg

/// How MImg asks the image chain for art.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Img {
    /// The site's nominal width (MImg's `width`); the native launcher fetches at the drawn size.
    pub width: f64,
    /// `opts.fit: "inside"` (keeps the aspect; the item page) instead of a square crop.
    pub inside: bool,
    /// object-contain instead of object-cover.
    pub contain: bool,
}

impl Img {
    pub fn w(width: f64) -> Img {
        Img { width, inside: false, contain: false }
    }
}

/// MImg: walks rare.market's image chain (its own rendition at the drawn size, the wsrv front,
/// wsrv, originals, passthrough; data/art.rs `resolve`) and ends on a quiet tile with the name's
/// initial (bg-layer-03, 1.6rem semibold, secondary at 60 %). `w` x `h` rem is the drawn size; `st`
/// lays it out (usually `s().fill()`).
/// Like the site, an item's art never falls back to its collection's logo.
pub fn mimg(ui: &mut Ui, st: S, src: Option<&str>, label: &str, w: f32, h: f32, o: Img) -> El {
    let sources: Vec<String> = src.filter(|s| !s.trim().is_empty()).map(String::from).into_iter().collect();
    let fb = ui.bx(st.clone().row().items_center().justify_center()).bg(tok::L3).child(ui.text(initial(label), t(Ty::px(1.6)).w(600).c(tok::MUTED.alpha(0.6))));
    let fit = if o.contain { kit::ArtFit::Contain(0.5, 0.5) } else { kit::ArtFit::Cover(0.5) };
    kit::art(ui, st, &sources, w, h, fit, Some(fb))
}

/// Avatar: round (or rounded-md `square`) art on layer 03; the SUI droplet for SUI.
pub fn avatar(ui: &mut Ui, st: S, src: Option<&str>, label: &str, size: f32, square: bool) -> El {
    let r = if square { ui.r(0.375) } else { ui.r(size / 2.0) };
    let img = mimg(ui, s().fill(), src, label, size, size, Img::w(120.0));
    ui.bx(st.size(size)).bg(tok::L3).radius(r).clip().child(img)
}

/// One of the player's own accounts: its picked picture, else its default picture, round.
pub fn account_avatar(ui: &mut Ui, st: S, address: &str, size: f32) -> El {
    avatar(ui, st, Some(&crate::data::live::picture(address)), "", size, false)
}

/// WalletAvatar: the generated gem pattern for an address, round.
pub fn wallet_avatar(ui: &mut Ui, st: S, address: &str, size: f32) -> El {
    let svg = rare_market::identicon::avatar_svg(address);
    let img = svg_box(ui, &format!("wallet-avatar-{address}"), svg, size, size, s().abs(Some(0.0), None, None, Some(0.0)));
    ui.bx(st.size(size)).radius(ui.r(size / 2.0)).clip().child(img)
}

/// The marketplace icons (public/venues), `size` rem, rounded 3px.
pub fn venue_mark(ui: &mut Ui, venue: &str, size: f32) -> Option<El> {
    let icon = mf::venue_icon(venue)?;
    let srcs = vec![icon.to_string()];
    let img = kit::art(ui, s().fill(), &srcs, size, size, kit::ArtFit::Contain(0.5, 0.5), None);
    Some(ui.bx(s().size(size)).radius(3.0).clip().child(img))
}

/// RarityBadge: the rank on a neutral label, md (h-6 px-1.5 text-xs) or sm.
pub fn rarity_badge(ui: &mut Ui, st: S, r: &Rarity, sm: bool) -> El {
    let (h, px, ty) = if sm { (1.25, 0.25, Ty::Px(0.6875, 1.0)) } else { (1.5, 0.375, XS) };
    ui.bx(st.row().items_center().h(h).px(px)).bg(LABELS_NEUTRAL.alpha(0.9)).radius(ui.r(0.125)).child(ui.text(r.rank.to_string(), t(ty).w(500).c(LABEL_TEXT).tab()))
}

/// The first letter or digit of a name, upper case.
pub fn initial(label: &str) -> String {
    kit::market::initial(label)
}

// ---------------------------------------------------------------- display

/// Sui: the amount, "SUI" after it in the secondary colour; "--" in secondary when unknown.
pub fn sui(ui: &mut Ui, st: S, mist: Option<&str>, dp: usize, compact: bool, tt: T) -> El {
    let v = mist.and_then(|m| m.parse::<u128>().ok());
    if v.is_none() {
        return ui.bx(st.row()).child(ui.text(mf::fmt_sui(mist, SuiOpts { dp, compact, trim: false }), tt.tab().c(tok::MUTED)));
    }
    if compact {
        // kit::sui has no compact form; the same look by hand.
        let txt = mf::fmt_sui(mist, SuiOpts { dp, compact, trim: false });
        return ui.bx(st.row().items_end()).child(ui.text(txt, tt.tab())).child(ui.text_s(s().ml(0.25), "SUI", tt.w(400).c(tok::MUTED)));
    }
    kit_sui(ui, st, v, dp, tt)
}

/// Delta: triangle and percent, green up, red down.
pub fn delta_el(ui: &mut Ui, st: S, v: Option<f64>, dp: usize, tt: T) -> El {
    delta(ui, st, v, dp, tt)
}

/// Lucide `history` (not in the kit's set), tinted.
pub fn history_icon(ui: &mut Ui, size: f32, c: Color) -> El {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l4 2"/></svg>"#;
    svg_tinted(ui, "lucide-history", svg.to_string(), size, size, c)
}

/// Empty (ui/states): centred secondary text, no illustration; `compact` is py-10, else py-20.
pub fn empty(ui: &mut Ui, title: &str, body: Option<&str>, action: Option<El>, compact: bool) -> El {
    let mut b = ui.bx(s().col().items_center().justify_center().gap(0.75).px(1.5).py(if compact { 2.5 } else { 5.0 })).child(ui.text(title, t(SM).c(tok::MUTED)));
    if let Some(body) = body {
        b = b.child(ui.text_s(s().max_w(24.0), body, t(XS).c(tok::MUTED).wrap()));
    }
    b.maybe(action)
}

// ---------------------------------------------------------------- Btn

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V {
    Primary,
    Secondary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sz {
    Sm,
    Md,
    Lg,
}

/// rare.market's Btn (`.btn`): rounded, font-medium; focus fills it white with dark text and scales
/// it to 1.04. `chevron` puts a ChevronDown after the label.
#[allow(clippy::too_many_arguments)]
pub fn btn(ui: &mut Ui, st: S, label: &str, tag: u32, v: V, sz: Sz, def: bool, chevron: bool) -> El {
    let (h, px, ty, gap) = match sz {
        Sz::Sm => (2.0, 0.75, SM, 0.375),
        Sz::Md => (2.5, 1.0, SM, 0.5),
        Sz::Lg => (3.0, 1.5, BASE, 0.5),
    };
    let (fill, w) = match v {
        V::Primary => (tok::BRAND, 600),
        V::Secondary => (tok::BUTTON_SECONDARY, 500),
    };
    let look = Look::default().bg(fill).radius(ui.r(0.25));
    let focus = look.clone().bg(tok::FG).scale(1.04);
    let mut el = ui.bx(st.row().items_center().justify_center().h(h).px(px).gap(gap)).look(look).focus_look(focus).transition(Transition::FOCUS);
    el = el.child(ui.text(label, t(ty).w(w)).focus_color(tok::BG));
    if chevron {
        el = el.child(kit::focus_icon(ui, "chevron-down", 0.875, false, tok::FG, tok::BG));
    }
    kit::nav(el, tag, None, None, def)
}

// ---------------------------------------------------------------- sections, tabs, segmented

/// MarketSections: LT, Collections Coins Perps Traders, RT (display only; LT/RT switch).
pub fn market_sections(ui: &mut Ui, cur: Section) -> El {
    let items: Vec<kit::TabItem> = MARKET_SECTIONS.iter().map(|(_, l)| kit::TabItem::new(*l)).collect();
    let i = MARKET_SECTIONS.iter().position(|(s_, _)| *s_ == cur).unwrap_or(0);
    let lt = glyph(ui, Pb::Lt, GSize::Sm);
    let rt = glyph(ui, Pb::Rt, GSize::Sm);
    let tabs = kit::tabs(ui, s(), &items, i, true);
    ui.bx(s().row().items_stretch().h(2.25).gap(1.0)).child(ui.bx(s().row().items_center()).child(lt)).child(tabs).child(ui.bx(s().row().items_center()).child(rt))
}

/// rare.market's underline Tabs with the LT / RT glyphs: text-base medium, the active one primary
/// with a 2px bar. Each tab is focusable (A picks it).
pub fn tabs(ui: &mut Ui, st: S, items: &[&str], value: usize, tag: u32) -> El {
    let mut row = ui.bx(st.row().items_end().gap(2.0));
    row = row.child(ui.bx(s().pb(0.625)).opacity(0.7).child(glyph(ui, Pb::Lt, GSize::Md)));
    for (i, it) in items.iter().enumerate() {
        let on = i == value;
        let mut b = ui.bx(s().row().items_center().px(0.25).pb(0.625).pt(0.25)).radius(ui.r(0.25)).child(ui.text(*it, t(BASE).w(500).c(if on { tok::FG } else { tok::MUTED })).focus_color(tok::FG));
        if on {
            b = b.child(ui.bx(s().abs(None, Some(0.0), Some(0.0), Some(0.0)).h_px(2.0)).bg(tok::FG).radius(1.0));
        }
        row = row.child(fill_nav(b.key(key(it)), tag + i as u32, None, None, false));
    }
    row.child(ui.bx(s().pb(0.625)).opacity(0.7).child(glyph(ui, Pb::Rt, GSize::Md)))
}

/// rare.market's Segmented: h-9 min-w-11 px-3 text-sm segments 1px apart, the ends rounded.
pub fn segmented(ui: &mut Ui, items: &[&str], value: usize, tag: u32) -> El {
    let mut row = ui.bx(s().row()).style(|st| st.gap = rare_render::taffy::prelude::Size { width: rare_render::taffy::prelude::length(1.0), height: rare_render::taffy::prelude::length(0.0) });
    for (i, it) in items.iter().enumerate() {
        let on = i == value;
        let r = if i == 0 || i + 1 == items.len() { ui.r(0.25) } else { 0.0 };
        let seg = ui
            .bx(s().row().items_center().justify_center().h(2.25).min_w(2.75).px(0.75))
            .bg(if on { tok::BUTTON_SECONDARY_ACTIVE } else { tok::BUTTON_SECONDARY })
            .radius(r)
            .child(ui.text(*it, t(SM).c(if on { tok::FG } else { tok::MUTED })).focus_color(tok::FG));
        row = row.child(fill_nav(seg, tag + i as u32, None, None, false));
    }
    row
}

/// Header stat (StatStrip): label over a semibold base-size line.
pub fn stat(ui: &mut Ui, label: &str, runs: Vec<(T, String)>) -> El {
    let line = baseline_row(ui, s().mt(0.125).gap(0.25), runs);
    ui.bx(s().col()).child(ui.text(label, t(XS).c(tok::MUTED))).child(line)
}

/// SectionTitle: text-xl bold, an optional small secondary note on the right.
pub fn section_title(ui: &mut Ui, st: S, title: &str, right: Option<String>) -> El {
    let mut row = ui.bx(st.row().items_center().gap(0.75).mb(1.0)).child(ui.text(title, t(XL).w(700)));
    if let Some(r) = right {
        row = row.child(ui.bx(s().row().items_center().gap(0.5).ml_auto()).child(ui.text(r, t(SM).c(tok::MUTED))));
    }
    row
}

// ---------------------------------------------------------------- MoverCard

/// A text-sm line inside a block whose font is text-base: the line box keeps the base strut
/// (1.5rem) and the small text sits on its baseline.
fn strut(ui: &Ui, el: El) -> El {
    ui.bx(s().col().h(1.5)).child(ui.bx(s().mt(0.17)).child(el))
}

/// The trending card: art (276:160) over the name and three figures.
pub fn mover_card(ui: &mut Ui, c: &CollectionRow, tag: u32, def: bool) -> El {
    let w = 20.0;
    let ih = w * 160.0 / 276.0;
    let art = mimg(ui, s().fill(), c.r#ref.image_url.as_deref(), &c.r#ref.name, w, ih, Img::w(560.0));
    let pic = ui.bx(s().wp(1.0).aspect(276.0 / 160.0)).bg(tok::L3).child(art);
    let mut name = ui.bx(s().row().items_center().gap(0.375)).child(ui.text(c.r#ref.name.clone(), t(LG).w(700).trunc()));
    if c.r#ref.verified {
        name = name.child(verified(ui, 1.0));
    }
    let label = |ui: &Ui, l: &str| ui.text(l, t(XXS).c(tok::MUTED));
    let semi = t(SM).w(600);
    let fl = sui(ui, s(), c.floor_mist.as_deref(), 2, false, semi);
    let vol = sui(ui, s(), Some(&c.volume_mist), 1, true, semi);
    let ch = delta_el(ui, s(), c.volume_change_pct, 1, semi);
    let cols = [Track::Fr(1.0), Track::Fr(1.0), Track::Fr(1.0)];
    let grid = ui
        .bx(s().grid(&cols).gap(0.5).mt(0.75))
        .child(ui.bx(s().col().min0()).child(label(ui, "FLOOR PRICE")).child(strut(ui, fl)))
        .child(ui.bx(s().col().min0()).child(label(ui, "VOLUME 1D")).child(strut(ui, vol)))
        .child(ui.bx(s().col().min0()).child(label(ui, "VOL CHANGE 1D")).child(strut(ui, ch)));
    let body = ui.bx(s().col().p(1.0)).child(name).child(grid);
    let look = Look::default().bg(tok::L1).radius(ui.r(0.5)).border(1.0, BORDER).clip();
    let focus = look.clone().border(1.0, BORDER_HOVER).scale(1.06);
    let el = ui.bx(bw(s().col().w(w), 1.0)).look(look).focus_look(focus).transition(Transition::FOCUS).child(pic).child(body).key(key(&c.r#ref.slug));
    kit::nav(el, tag, None, None, def)
}

// ---------------------------------------------------------------- the top collections table

/// TABLE_COLS: minmax(8rem,3fr) minmax(4rem,1.4fr) minmax(4rem,1.4fr) minmax(3.5rem,1fr)
/// minmax(4rem,1.4fr) minmax(3rem,0.8fr) minmax(4rem,1.2fr).
pub const TABLE_COLS: [Track; 7] = [Track::MinFr(8.0, 3.0), Track::MinFr(4.0, 1.4), Track::MinFr(4.0, 1.4), Track::MinFr(3.5, 1.0), Track::MinFr(4.0, 1.4), Track::MinFr(3.0, 0.8), Track::MinFr(4.0, 1.2)];

/// A `.trow` cell: shrinks to its column and clips; `end` is text-right.
fn cell(ui: &Ui, st: S, end: bool) -> El {
    let mut st = st.row().items_center().min0().shrink();
    if end {
        st = st.justify_end();
    }
    ui.bx(st).clip()
}

pub fn collection_head(ui: &mut Ui, win: &str) -> El {
    let h = |ui: &Ui, txt: &str, st: S| cell(ui, st, true).child(ui.text(txt, t(SM).w(500).c(tok::MUTED).trunc()));
    let first = cell(ui, s(), false).child(ui.text_s(s().mr(1.25), "#", t(SM).w(500).c(tok::MUTED))).child(ui.text(" Collection", t(SM).w(500).c(tok::MUTED).trunc()));
    let cells = vec![
        first,
        h(ui, "Floor", s().px(0.25)),
        h(ui, "Top offer", s().px(0.25)),
        h(ui, &format!("Floor {win}"), s().px(0.25)),
        h(ui, "Volume", s().px(0.25)),
        h(ui, "Sales", s().px(0.25)),
        h(ui, "Listed", s().pl(0.25).pr(0.5)),
    ];
    ui.bx(s().grid(&TABLE_COLS).items_center().h(3.25).gap_xy(0.5, 0.0).px(0.75)).kids(cells)
}

pub fn collection_row(ui: &mut Ui, r: &CollectionRow, tag: u32) -> El {
    let semi = t(SM).w(600);
    let mut name = ui.bx(s().row().items_center().gap(0.375).min0().shrink()).child(ui.text(r.r#ref.name.clone(), semi.tab().trunc()));
    if r.r#ref.verified {
        name = name.child(verified(ui, 0.95));
    }
    let av = avatar(ui, s(), r.r#ref.image_url.as_deref(), &r.r#ref.name, 2.5, false);
    let first = cell(ui, s().gap(0.75), false).child(ui.text_s(s().w(1.5), r.rank.to_string(), semi.tab())).child(av).child(name);
    let floor = sui(ui, s(), r.floor_mist.as_deref(), 2, false, semi);
    let offer = sui(ui, s(), r.top_offer_mist.as_deref(), 2, false, semi);
    let ch = delta_el(ui, s(), r.floor_change_pct, 1, semi);
    let vol = sui(ui, s(), Some(&r.volume_mist), 2, true, semi);
    let pct = if r.supply > 0 { mf::fmt_pct(Some(r.listed_count as f64 / r.supply as f64 * 100.0), 1) } else { DASH.to_string() };
    let listed = format!("{} / {}", mf::fmt_int(Some(r.listed_count as f64), false), mf::fmt_int(Some(r.supply as f64), true));
    let last = ui.bx(s().col().items_end().min0().shrink().pl(0.25).pr(0.5)).clip().child(ui.text(pct, semi.tab().trunc())).child(ui.text(listed, t(XS).c(tok::MUTED).tab().trunc()));
    let cells = vec![
        first,
        cell(ui, s().px(0.25), true).child(floor),
        cell(ui, s().px(0.25), true).child(offer),
        cell(ui, s().px(0.25), true).child(ch),
        cell(ui, s().px(0.25), true).child(vol),
        cell(ui, s().px(0.25), true).child(ui.text(mf::fmt_int(Some(r.sales as f64), false), semi.tab())),
        last,
    ];
    let el = ui.bx(s().grid(&TABLE_COLS).items_center().h(4.0).wp(1.0).gap_xy(0.5, 0.0).px(0.75)).radius(ui.r(0.5)).kids(cells).key(key(&r.r#ref.slug));
    fill_nav(el, tag, None, None, false)
}

// ---------------------------------------------------------------- ItemCard

/// The item card: square art with the rarity rank, the short name, (the collection), the price
/// and venue or "Not listed", the last sale. `w` is the card's width in rem (the art's size).
pub fn item_card(ui: &mut Ui, st: S, c: &ItemCard, w: f32, tag: u32, def: bool, show_collection: bool) -> El {
    let label = c.collection.as_ref().map(|x| x.name.clone()).unwrap_or_else(|| c.name.clone());
    let art = mimg(ui, s().fill(), c.image_url.as_deref(), &label, w, w, Img::w(400.0));
    let mut pic = ui.bx(s().wp(1.0).aspect(1.0)).bg(tok::L3).child(art);
    if let Some(r) = &c.rarity {
        pic = pic.child(rarity_badge(ui, s().abs(None, Some(0.375), Some(0.375), None), r, false));
    }
    let coll = c.collection.as_ref().map(|x| x.name.as_str());
    let mut body = ui.bx(s().col().wp(1.0).gap(0.5).p(0.75)).child(ui.text(mf::short_name(&c.name, coll).to_string(), t(SM).w(600).trunc()));
    if show_collection {
        body = body.child(ui.text_s(s().mt(-0.25), coll.unwrap_or("").to_string(), t(XS).c(tok::MUTED).trunc()));
    }
    let mut price = ui.bx(s().row().items_center().h(1.25).gap(0.25));
    match &c.listing {
        Some(l) => {
            let m = l.all_in_mist.clone().unwrap_or_else(|| l.price_mist.clone());
            price = price.child(sui(ui, s(), Some(&m), 3, false, t(SM).w(600)));
            if let Some(v) = venue_mark(ui, &l.venue, 0.875) {
                price = price.child(v);
            }
        }
        None => price = price.child(ui.text(if c.staked.is_some() { "Staked" } else { "Not listed" }, t(SM).c(tok::MUTED))),
    }
    let last = c.last_sale_mist.as_deref().map(|m| mf::fmt_sui(Some(m), SuiOpts { dp: 3, compact: false, trim: true })).unwrap_or_else(|| DASH.into());
    let hist = history_icon(ui, 0.875, tok::MUTED);
    let sale = ui.bx(s().row().items_center().h(1.5).gap(0.25)).child(hist).child(ui.text(last, t(SM).c(tok::MUTED).tab()));
    body = body.child(price).child(sale);
    let look = Look::default().bg(tok::L1).radius(ui.r(0.5)).border(1.0, Color::TRANSPARENT).clip();
    let focus = look.clone().bg(tok::L2).border(1.0, BORDER_HOVER).scale(1.06);
    let el = ui.bx(bw(st.col(), 1.0)).look(look).focus_look(focus).transition(Transition::FOCUS).child(pic).child(body).key(key(&c.id));
    kit::nav(el, tag, None, None, def)
}

// ---------------------------------------------------------------- activity rows

/// EVENT_COLS: 5rem minmax(2.25rem,1fr) 6rem 6.25rem 6.25rem 4rem.
pub const EVENT_COLS: [Track; 6] = [Track::Rem(5.0), Track::MinFr(2.25, 1.0), Track::Rem(6.0), Track::Rem(6.25), Track::Rem(6.25), Track::Rem(4.0)];

pub fn event_head(ui: &mut Ui, item: bool) -> El {
    let tm = t(META).w(500).c(tok::MUTED).tab();
    let cells = vec![
        cell(ui, s(), false).child(ui.text("Type", tm.trunc())),
        cell(ui, s(), false).child(ui.text(if item { "Item" } else { "Venue" }, tm.trunc())),
        cell(ui, s(), true).child(ui.text("Price", tm.trunc())),
        cell(ui, s(), false).child(ui.text("From", tm.trunc())),
        cell(ui, s(), false).child(ui.text("To", tm.trunc())),
        cell(ui, s(), true).child(ui.text("Time", tm.trunc())),
    ];
    ui.bx(s().grid(&EVENT_COLS).items_center().h(2.0).gap_xy(0.5, 0.0).px(0.5)).kids(cells)
}

pub fn event_row(ui: &mut Ui, e: &MarketEvent, item: bool, now_ms: f64, tag: u32) -> El {
    let tm = t(META).tab();
    let kind = mf::event_label(&e.kind).map(str::to_string).unwrap_or_else(|| e.kind.clone());
    let second = if item {
        let mut c = cell(ui, s().gap(0.5), false);
        if let Some(it) = &e.item {
            c = c.child(avatar(ui, s(), it.image_url.as_deref(), &it.name, 2.0, true));
        }
        let name = e.item.as_ref().map(|it| mf::short_name(&it.name, e.collection.as_ref().map(|x| x.name.as_str())).to_string()).unwrap_or_else(|| DASH.into());
        c.child(ui.text(name, tm.trunc()))
    } else {
        let mut c = cell(ui, s().gap(0.375), false);
        if let Some(v) = e.venue.as_deref().and_then(|v| venue_mark(ui, v, 0.875)) {
            c = c.child(v);
        }
        let label = e.venue.as_deref().filter(|v| mf::venue_icon(v).is_some()).map(|v| mf::venue_name(Some(v))).unwrap_or_else(|| DASH.into());
        c.child(ui.text(label, tm.trunc()))
    };
    let price = e.price_mist.as_deref().map(|m| mf::fmt_sui(Some(m), SuiOpts { dp: 3, ..Default::default() })).unwrap_or_else(|| DASH.into());
    let from = e.from_name.clone().unwrap_or_else(|| mf::short_addr(e.from.as_deref()));
    let to = e.to_name.clone().unwrap_or_else(|| mf::short_addr(e.to.as_deref()));
    let ago = format!("{} ago", rare_market::format::time_ago(Some(&e.at), now_ms as i64));
    let cells = vec![
        cell(ui, s(), false).child(ui.text(kind, tm.c(tok::MUTED).trunc())),
        second,
        cell(ui, s(), true).child(ui.text(price, tm.tab().trunc())),
        cell(ui, s(), false).child(ui.text(from, tm.tab().trunc())),
        cell(ui, s(), false).child(ui.text(to, tm.tab().trunc())),
        cell(ui, s(), true).child(ui.text(ago, tm.c(tok::MUTED).tab().trunc())),
    ];
    let el = ui.bx(bw_top(s().grid(&EVENT_COLS).items_center().h(3.5).wp(1.0).gap_xy(0.5, 0.0).px(0.5), 1.0)).child(top_line(ui)).kids(cells).key(key(&e.id));
    fill_nav(el, tag, None, None, false)
}

// ---------------------------------------------------------------- holders

/// HOLDER_COLS: 3rem minmax(0,1fr) 6rem 14rem 5rem.
pub const HOLDER_COLS: [Track; 5] = [Track::Rem(3.0), Track::Fr(1.0), Track::Rem(6.0), Track::Rem(14.0), Track::Rem(5.0)];

pub fn holder_head(ui: &mut Ui) -> El {
    let tm = t(XS).c(tok::MUTED);
    let cells = vec![
        cell(ui, s(), false).child(ui.text("#", tm)),
        cell(ui, s(), false).child(ui.text("Holder", tm.trunc())),
        cell(ui, s(), true).child(ui.text("Items", tm.trunc())),
        cell(ui, s(), false).child(ui.text("Share of supply", tm.trunc())),
        cell(ui, s(), true).child(ui.text("Listed", tm.trunc())),
    ];
    ui.bx(s().grid(&HOLDER_COLS).items_center().h(2.5).gap_xy(0.5, 0.0).px(1.0)).kids(cells)
}

pub fn holder_row(ui: &mut Ui, r: &Holder, tag: u32) -> El {
    let mono = t(SM).mono();
    let w = r.pct_of_supply.max(if r.pct_of_supply > 0.0 { 2.0 } else { 0.0 }).min(100.0) as f32 / 100.0;
    let name = r.name.clone().unwrap_or_else(|| mf::short_addr(Some(&r.address)));
    let nt = if r.name.is_some() { t(SM).tab() } else { mono };
    let av = wallet_avatar(ui, s(), &r.address, 1.5);
    let holder = cell(ui, s().gap(0.5), false).child(av).child(ui.text(name, nt.trunc()));
    let bar = ui.bx(s().h(0.25).grow()).bg(tok::L3).radius(ui.r(0.125)).clip().child(ui.bx(s().hp(1.0).wp(w)).bg(tok::FG.alpha(0.45)).radius(ui.r(0.125)));
    let pct = mf::fmt_pct(Some(r.pct_of_supply), if r.pct_of_supply < 1.0 { 2 } else { 1 });
    let share = cell(ui, s().gap(0.5), false).child(bar).child(right_text(ui, s().w(3.5), pct, mono.c(tok::MUTED)));
    let listed = if r.listed > 0 { ui.text(mf::fmt_int(Some(r.listed as f64), false), mono) } else { ui.text("0", mono.c(tok::MUTED)) };
    let cells = vec![
        cell(ui, s(), false).child(ui.text(r.rank.to_string(), mono.c(tok::MUTED))),
        holder,
        cell(ui, s(), true).child(ui.text(mf::fmt_int(Some(r.count as f64), false), mono)),
        share,
        cell(ui, s(), true).child(listed),
    ];
    let el = ui.bx(bw_top(s().grid(&HOLDER_COLS).items_center().h(3.0).wp(1.0).gap_xy(0.5, 0.0).px(1.0), 1.0)).child(top_line(ui)).kids(cells).key(key(&r.address));
    fill_nav(el, tag, None, None, false)
}

/// The search field look (HomePage and SearchPage): a bordered box, transparent, the search icon.
pub fn search_field(ui: &mut Ui, st: S, h: f32, text: &str, placeholder: bool, ty: Ty, icon_size: f32, tag: u32) -> El {
    let ic = icon(ui, "search", icon_size, tok::ICON2);
    let look = Look::default().radius(ui.r(0.25)).border(1.0, tok::BORDER_LIGHT);
    let focus = look.clone().border(1.0, BORDER_FOCUS).bg(tok::L4);
    let el = ui
        .bx(bw(st.row().items_center().gap(0.5).h(h).px(0.75), 1.0))
        .look(look)
        .focus_look(focus)
        .transition(Transition::FOCUS)
        .child(ic)
        .child(ui.text(text, t(ty).c(if placeholder { tok::MUTED } else { tok::FG }).trunc()));
    kit::nav(el, tag, None, None, false)
}
