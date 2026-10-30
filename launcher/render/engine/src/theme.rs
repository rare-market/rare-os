//! GENERATED from launcher/src/styles/app.css by rare-tw (launcher/crates/rare-tw/core/src/theme.rs).
//! Do not edit: change app.css, then run
//!   RARE_TW_BLESS=1 cargo test --manifest-path launcher/crates/Cargo.toml -p rare-tw-core --test theme
//! (that test fails while this copy is stale, so a token change reaches both front ends).
//!
//! Layout is in rem against a 1280 x 800 design: rem = min(width / 80, height / 50), so the
//! SuiPlay's 1920 x 1200 is 24 px.

use crate::color::Color;

pub fn rem_for(w: f32, h: f32) -> f32 {
    (w / 80.0).min(h / 50.0)
}

/// The core palette (DESIGN.md names).
pub mod tok {
    use super::Color;
    pub const BG: Color = Color::rgb(16, 12, 24);
    pub const L1: Color = Color::rgb(21, 17, 29);
    pub const L2: Color = Color::rgb(25, 21, 35);
    pub const L3: Color = Color::rgb(30, 25, 41);
    pub const L4: Color = Color::rgb(35, 30, 47);
    pub const BRAND: Color = Color::rgb(61, 90, 254);
    pub const FG: Color = Color::rgb(242, 242, 243);
    pub const MUTED: Color = Color::rgb(133, 127, 148);
    pub const POS: Color = Color::rgb(42, 223, 114);
    pub const NEG: Color = Color::rgb(234, 65, 65);
    pub const ATTN: Color = Color::rgb(236, 182, 19);
    pub const HAIR: Color = Color::rgb(35, 30, 47);
    /// The SUI mark's blue (launcher/src/ui/sui.tsx).
    pub const SUI: Color = Color::rgb(77, 162, 255);
}

/// Every `--color-*` in app.css's @theme (`bg-l3` is `color::L3`).
pub mod color {
    use super::Color;
    pub const BG: Color = Color::rgb(16, 12, 24);
    pub const L1: Color = Color::rgb(21, 17, 29);
    pub const L2: Color = Color::rgb(25, 21, 35);
    pub const L3: Color = Color::rgb(30, 25, 41);
    pub const L4: Color = Color::rgb(35, 30, 47);
    pub const BRAND: Color = Color::rgb(61, 90, 254);
    pub const BRAND_2: Color = Color::rgb(92, 118, 255);
    pub const FG: Color = Color::rgb(242, 242, 243);
    pub const MUTED: Color = Color::rgb(133, 127, 148);
    pub const POS: Color = Color::rgb(42, 223, 114);
    pub const NEG: Color = Color::rgb(234, 65, 65);
    pub const WARN: Color = Color::rgb(236, 182, 19);
    pub const LINE: Color = Color::rgb(35, 30, 47);
    pub const LINE_2: Color = Color::rgb(71, 64, 89);
    pub const BTN: Color = Color::rgb(62, 56, 77);
    pub const BTN_2: Color = Color::rgb(62, 56, 77).alpha(0.6);
    pub const BTN_ACTIVE: Color = Color::rgb(71, 64, 89);
    pub const ICON: Color = Color::rgb(242, 242, 243);
    pub const ICON_2: Color = Color::rgb(133, 127, 148);
    pub const BACKGROUND: Color = Color::rgb(16, 12, 24);
    pub const LAYER_01: Color = Color::rgb(21, 17, 29);
    pub const LAYER_02: Color = Color::rgb(25, 21, 35);
    pub const LAYER_03: Color = Color::rgb(30, 25, 41);
    pub const LAYER_04: Color = Color::rgb(35, 30, 47);
    pub const LABELS_NEUTRAL: Color = Color::rgb(46, 38, 64);
    pub const LABELS_BRAND: Color = Color::rgb(92, 118, 255);
    pub const PRIMARY: Color = Color::rgb(242, 242, 243);
    pub const SECONDARY: Color = Color::rgb(133, 127, 148);
    pub const POSITIVE: Color = Color::rgb(42, 223, 114);
    pub const ATTENTION: Color = Color::rgb(236, 182, 19);
    pub const NEGATIVE: Color = Color::rgb(234, 65, 65);
    pub const ON_BRAND: Color = Color::rgb(242, 242, 243);
    pub const ROYALTIES: Color = Color::rgb(212, 189, 255);
    pub const LABEL_NEUTRAL_TEXT: Color = Color::rgb(162, 157, 175);
    pub const ICON_PRIMARY: Color = Color::rgb(242, 242, 243);
    pub const ICON_SECONDARY: Color = Color::rgb(133, 127, 148);
    pub const BORDER_PRIMARY: Color = Color::rgb(35, 30, 47);
    pub const BORDER_LIGHT: Color = Color::rgb(50, 45, 62);
    pub const BORDER_DARK: Color = Color::rgb(30, 25, 41);
    pub const BORDER_INTERACTIVE: Color = Color::rgb(71, 64, 89).alpha(0.6);
    pub const BORDER_INTERACTIVE_HOVER: Color = Color::rgb(111, 104, 125).alpha(0.5);
    pub const BORDER_INTERACTIVE_FOCUS: Color = Color::rgb(133, 127, 148).alpha(0.5);
    pub const BUTTON_PRIMARY: Color = Color::rgb(61, 90, 254);
    pub const BUTTON_PRIMARY_HOVER: Color = Color::rgb(47, 72, 217);
    pub const BUTTON_SECONDARY: Color = Color::rgb(62, 56, 77).alpha(0.6);
    pub const BUTTON_SECONDARY_HOVER: Color = Color::rgb(62, 56, 77).alpha(0.4);
    pub const BUTTON_SECONDARY_ACTIVE: Color = Color::rgb(71, 64, 89).alpha(0.9);
    pub const BUTTON_DIMMED: Color = Color::rgb(42, 37, 55).alpha(0.4);
    pub const BUTTON_DIMMED_HOVER: Color = Color::rgb(62, 56, 77).alpha(0.4);
    pub const BUTTON_DIMMED_ACTIVE: Color = Color::rgb(71, 64, 89).alpha(0.6);
    pub const BUTTON_ON_IMAGE: Color = Color::rgb(136, 136, 136).alpha(0.3);
    pub const HAIR: Color = Color::rgb(35, 30, 47);
    pub const TINT_UP: Color = Color::rgb(42, 223, 114).alpha(0.12);
    pub const TINT_DOWN: Color = Color::rgb(234, 65, 65).alpha(0.12);
}

/// Type scale in rem (DESIGN.md "Type").
pub mod size {
    pub const XXS: f32 = 0.625;
    pub const LABEL: f32 = 0.75;
    pub const META: f32 = 0.8125;
    pub const SMALL: f32 = 0.875;
    pub const BODY: f32 = 0.9375;
    pub const ROW: f32 = 1.0625;
    pub const TITLE: f32 = 1.375;
    pub const DISPLAY: f32 = 2.0;
    pub const HERO: f32 = 2.75;
}

/// Line height per type step, in rem.
pub mod line {
    pub const XXS: f32 = 0.9375;
    pub const LABEL: f32 = 1.0;
    pub const META: f32 = 1.125;
    pub const SMALL: f32 = 1.25;
    pub const BODY: f32 = 1.375;
    pub const ROW: f32 = 1.5;
    pub const TITLE: f32 = 1.75;
    pub const DISPLAY: f32 = 2.25;
    pub const HERO: f32 = 3.0;
}

/// Letter spacing per type step, in em (0 where the step has none).
pub mod tracking {
    pub const XXS: f32 = 0.0;
    pub const LABEL: f32 = 0.0;
    pub const META: f32 = 0.0;
    pub const SMALL: f32 = 0.0;
    pub const BODY: f32 = 0.0;
    pub const ROW: f32 = 0.0;
    pub const TITLE: f32 = -0.01;
    pub const DISPLAY: f32 = -0.02;
    pub const HERO: f32 = -0.03;
}

/// Radii in rem.
pub mod radius {
    pub const MODAL: f32 = 1.25;
    pub const CHIP: f32 = 0.25;
    pub const CARD: f32 = 0.5;
    pub const PANEL: f32 = 1.0;
    pub const SHEET: f32 = 1.25;
}

/// Motion tokens in seconds; one curve (`Ease::PRO`, app.css --ease).
pub mod motion {
    /// Focus ring and focus scale ([data-nav] transition).
    pub const FOCUS: f32 = 0.11;
    pub const FAST: f32 = 0.12;
    /// Home's strip tile growth (.strip-tile).
    pub const STRIP: f32 = 0.16;
    /// The strip tile's label fade (.strip-label).
    pub const LABEL: f32 = 0.12;
    pub const BASE: f32 = 0.2;
    pub const SLOW: f32 = 0.32;
    pub const TINT: f32 = 0.6;
    /// cubic-bezier control points of the one curve.
    pub const EASE: [f32; 4] = [0.2, 0.8, 0.2, 1.0];
}

/// The focus ring: a 0.125 rem gap in the page colour, then white out to 0.3125 rem.
pub mod ring {
    pub const GAP: f32 = 0.125;
    pub const OUTER: f32 = 0.3125;
}

/// What focus does to tiles and buttons (app.css [data-nav] rules).
pub mod focus {
    /// `.tile`
    pub const TILE_SCALE: f32 = 1.06;
    /// `.tile.tile-wide`
    pub const TILE_WIDE_SCALE: f32 = 1.03;
    /// `.btn` (also fills fg with bg text)
    pub const BTN_SCALE: f32 = 1.04;
    /// `.strip-tile` grows from its top-left corner
    pub const STRIP_SCALE: f32 = 1.45;
    /// and every later strip tile slides right by this, in rem
    pub const STRIP_SHIFT: f32 = 2.475;
}
