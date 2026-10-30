//! app.css tokens (rare.market's dark theme) as colours. The engine's `theme::tok` holds the core
//! set; this adds the rest the screens use, under the names the web classes use.

use rare_render::Color;

pub use rare_render::theme::tok::{ATTN, BG, BRAND, FG, HAIR, L1, L2, L3, L4, MUTED, NEG, POS, SUI};

/// --border-light (`border-border-light`)
pub const BORDER_LIGHT: Color = Color::rgb(50, 45, 62);
/// --border-interactive
pub const BORDER_INTERACTIVE: Color = Color::rgb(71, 64, 89);
/// --icon-secondary (`text-icon-2`, `bg-icon-2`)
pub const ICON2: Color = Color::rgb(133, 127, 148);
/// `bg-btn-2`: --button-secondary at --button-secondary-a (0.6)
pub const BTN2: Color = Color::rgb(62, 56, 77).alpha(0.6);
/// `bg-button-secondary` (rare.market name): the same 62 56 77 / 0.6
pub const BUTTON_SECONDARY: Color = Color::rgb(62, 56, 77).alpha(0.6);
/// `bg-button-secondary-active`: 71 64 89 / 0.9
pub const BUTTON_SECONDARY_ACTIVE: Color = Color::rgb(71, 64, 89).alpha(0.9);
/// `bg-button-dimmed`: 42 37 55 / 0.4
pub const BUTTON_DIMMED: Color = Color::rgb(42, 37, 55).alpha(0.4);
/// --button-buy
pub const BUY: Color = Color::rgb(42, 223, 114);
/// The buy button's text (#0E0A14).
pub const ON_BUY: Color = Color::rgb(14, 10, 20);
/// Tints for a value that just moved (--tint-up / --tint-down).
pub const TINT_UP: Color = Color::rgb(42, 223, 114).alpha(0.16);
pub const TINT_DOWN: Color = Color::rgb(234, 65, 65).alpha(0.16);

/// `text-fg/85` and friends.
pub fn fg(a: f32) -> Color {
    FG.alpha(a)
}

pub const BLACK: Color = Color::BLACK;
pub const WHITE: Color = Color::WHITE;
