//! The native kit: launcher/src/ui one to one, as view elements (`rare_render::view::El`) built at
//! the page's rem. Every screen builds from these so a token or a focus look
//! changes in one place.

pub mod art;
pub mod charts;
pub mod controls;
pub mod format;
pub mod glyph;
pub mod icon;
pub mod lucide;
pub mod market;
pub mod num;
pub mod sheet;
pub mod title_fit;
pub mod tok;
pub mod ui;
pub use title_fit::{fit_title, title_cover};

pub use art::{art, ArtFit, Arts};
pub use charts::{mini_curve, spark};
pub use controls::{
    button, card_row, cell, dot, empty, focus_icon, follow_tabs, group, pin_bar, PIN_BAR, head_row, info_card, nav, panel, rail, row, row_look, section_head, segmented, tabs, tile, toggle, Btn, BtnIcon, DotTone, Size, TabItem, Variant,
};
pub use format::{NumKind, NumOptions};
pub use glyph::{glyph, hint, GSize, Pb};
pub use icon::{icon, icon_s, steam, sui_logo};
pub use market::{avatar, delta, stat_line, sui, thumb, verified, StatItem};
pub use num::num;
pub use sheet::sheet;
pub use ui::{s, Track, Ty, Ui, S, T};
