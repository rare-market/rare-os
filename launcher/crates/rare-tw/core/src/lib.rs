//! rare-tw-core: the Tailwind side of `tw!`.
//!
//! `parse("flex items-end gap-[2rem] px-[2.5rem]")` turns a web launcher class string into a `Tw`:
//! the element's declarations after the browser's cascade, the declarations of its states
//! (`focus-visible:`, `first:`, children of `space-*` / `divide-*` / `.trow`), and the behaviour
//! marks of app.css's component classes (`tile`, `btn`, `strip-tile`...). The tw! macro
//! (rare-tw-macros) runs it at compile time and writes the result as a `static`; the rare-tw crate
//! turns it into a Taffy `Style` and the engine's node look.
//!
//! Tokens come from launcher/src/styles/app.css itself (`appcss`, compiled in), so a token change in
//! the web launcher reaches the native one at the next build. `theme::engine_theme_rs()` renders
//! Rare Render's `engine/src/theme.rs` from the same tokens.

pub mod appcss;
pub mod css;
pub mod emit;
#[cfg(feature = "taffy")]
pub mod layout;
pub mod look;
pub mod model;
pub mod order_list;
pub mod parse;
pub mod theme;
pub mod tokens;
pub mod value;

pub use model::*;
pub use parse::{parse, parse_class, Error};
