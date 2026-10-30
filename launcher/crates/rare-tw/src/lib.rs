//! `tw!`: the web launcher's Tailwind class strings, carried over verbatim into native screens.
//!
//! ```
//! use rare_tw::{tw, Len, Tw};
//! let row: &'static Tw = tw!("flex items-end gap-[2rem] px-[2.5rem] text-title text-muted");
//! assert_eq!(row.base.padding[1], Some(Len::Rem(2.5)));
//! let style = row.style(24.0); // taffy::Style at the SuiPlay's 24 px rem
//! assert_eq!(style.gap.width, rare_tw::taffy::style::LengthPercentage::length(48.0));
//! let text = row.text(&rare_tw::look::Text::root(24.0), 24.0);
//! assert_eq!(text.size_px, 33.0); // text-title is 1.375rem
//! ```
//!
//! The string is parsed at compile time (rare-tw-core) into a `static`; nothing is parsed at run
//! time. Classes outside the subset launcher/src uses do not compile:
//!
//! ```compile_fail
//! let card = rare_tw::tw!("rounded-card shadow-md"); // DESIGN.md: no card shadows
//! ```
//!
//! ```compile_fail
//! let x = rare_tw::tw!("hover:bg-l3"); // the launcher has no hover: focus-visible: is its state
//! ```
//!
//! What a `Tw` holds:
//! - `base`: the element's declarations after the browser's cascade (Tailwind's rule order, then
//!   app.css component classes, then `!important`), unresolved CSS units;
//! - `states`: `focus-visible:`, `first:`, `last:`, `disabled:`, `placeholder:`, children of
//!   `space-*` / `divide-*` / `.trow`, `[&>svg]:`...;
//! - `marks`: app.css component classes with behaviour (`tile`, `btn`, `strip-tile`...).
//!
//! Resolve with `style(rem)` (taffy), `look(rem, parent_text)` (paint) and `text(parent, rem)`.
//! Tokens and component classes are read from launcher/src/styles/app.css at build time.

// tw! expands to `::rare_tw::...` paths; this lets the crate use its own macro.
extern crate self as rare_tw;

pub use rare_tw_core::*;
pub use rare_tw_macros::tw;
pub use taffy;

/// Joins class strings at run time, like the web launcher's `cn(...)`, with the browser's cascade
/// (a class's position in the strings does not decide conflicts; Tailwind's rule order does).
/// Every part came from a `tw!`, so the join always parses. Each distinct combination is parsed
/// once per thread and kept, so calling this from a view rebuild costs a hash lookup.
pub fn cn(parts: &[&Tw]) -> &'static Tw {
    use std::cell::RefCell;
    use std::collections::HashMap;
    thread_local! {
        static JOINED: RefCell<HashMap<String, &'static Tw>> = RefCell::new(HashMap::new());
    }
    let key = parts.iter().map(|t| t.src.as_ref()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    JOINED.with(|m| {
        if let Some(t) = m.borrow().get(&key) {
            return *t;
        }
        let t: &'static Tw = Box::leak(Box::new(parse(&key).expect("rare-tw: parts of cn() come from tw! and always parse")));
        m.borrow_mut().insert(key, t);
        t
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macro_matches_runtime_parse() {
        for (m, s) in [
            (tw!("flex items-end gap-[2rem] px-[2.5rem]"), "flex items-end gap-[2rem] px-[2.5rem]"),
            (tw!("btn inline-flex shrink-0 items-center justify-center whitespace-nowrap rounded-chip disabled:opacity-40"), "btn inline-flex shrink-0 items-center justify-center whitespace-nowrap rounded-chip disabled:opacity-40"),
            (tw!("absolute inset-x-0 bottom-0 h-1/2 bg-gradient-to-t from-black/95 via-black/50 to-transparent"), "absolute inset-x-0 bottom-0 h-1/2 bg-gradient-to-t from-black/95 via-black/50 to-transparent"),
            (tw!("trow grid h-[3.25rem] w-full items-center gap-x-[0.75rem] rounded-card px-[0.75rem] text-left text-small"), "trow grid h-[3.25rem] w-full items-center gap-x-[0.75rem] rounded-card px-[0.75rem] text-left text-small"),
            (tw!("grid-cols-[repeat(auto-fill,minmax(9rem,1fr))] shadow-[0_0_0_0.125rem_rgb(var(--background)),0_0_0_0.3125rem_rgb(var(--text-primary))]"), "grid-cols-[repeat(auto-fill,minmax(9rem,1fr))] shadow-[0_0_0_0.125rem_rgb(var(--background)),0_0_0_0.3125rem_rgb(var(--text-primary))]"),
            (tw!("boot-sweep [mask-image:url(/brand/boot-gem.png)] [&>div]:!text-[0.9em] [[data-area=chart]:focus_&]:opacity-100"), "boot-sweep [mask-image:url(/brand/boot-gem.png)] [&>div]:!text-[0.9em] [[data-area=chart]:focus_&]:opacity-100"),
            (tw!(""), ""),
        ] {
            assert_eq!(m, &parse(s).unwrap(), "{s}");
        }
    }

    #[test]
    fn cascade_follows_tailwind_not_string_order() {
        // p-* sorts before px-* in Tailwind's order, so px wins whichever comes first.
        let a = tw!("px-2 p-4");
        assert_eq!(a.base.padding, [Some(Len::Rem(1.0)), Some(Len::Rem(0.5)), Some(Len::Rem(1.0)), Some(Len::Rem(0.5))]);
        // leading-* beats the size's own line height on the same element.
        let b = tw!("leading-none text-title");
        assert_eq!(b.base.effective_line_height(), Some(LineHeight::Num(1.0)));
        // app.css component classes are unlayered and beat utilities.
        let c = tw!("trow gap-x-[2rem]");
        assert_eq!(c.base.gap[1], Some(Len::Rem(0.5)));
        // !important beats them.
        let d = tw!("h-4 !h-[3.75rem]");
        assert_eq!(d.base.height, Some(Len::Rem(3.75)));
    }

    #[test]
    fn layout_resolves_at_rem() {
        use taffy::style::{Dimension as D, Display as Dp, FlexDirection as Fd, JustifyContent as Jc, LengthPercentage as Lp, LengthPercentageAuto as Lpa, Overflow as Ov, Position as Ps};
        let t = tw!("absolute inset-x-0 top-1/2 flex flex-col items-center justify-between gap-y-[0.5rem] w-[20rem] min-w-0 overflow-y-auto");
        let s = t.style(16.0);
        assert_eq!(s.display, Dp::Flex);
        assert_eq!(s.position, Ps::Absolute);
        assert_eq!(s.inset.left, Lpa::length(0.0));
        assert_eq!(s.inset.top, Lpa::percent(0.5));
        assert_eq!(s.size.width, D::length(320.0));
        assert_eq!(s.gap.height, Lp::length(8.0));
        assert_eq!(s.flex_direction, Fd::Column);
        assert_eq!(s.justify_content, Jc::SPACE_BETWEEN);
        assert_eq!(s.overflow.y, Ov::Scroll);
        // A plain div is a block, as in the browser.
        assert_eq!(tw!("p-4").style(16.0).display, Dp::Block);
        let g = tw!("grid grid-cols-3 col-span-2").style(16.0);
        assert_eq!(g.grid_template_columns.len(), 1);
    }

    #[test]
    fn look_and_text() {
        let root = look::Text::root(24.0);
        let t = tw!("rounded-card border border-hair bg-l1/80 text-meta font-semibold tracking-wide text-muted ring-1 ring-black/40 opacity-60");
        let l = t.look(24.0, &root);
        assert_eq!(l.radius, [12.0; 4]);
        assert_eq!(l.border, [1.0; 4]);
        assert_eq!(l.border_color, tokens::color("hair").unwrap());
        assert!((l.bg.a - 0.8).abs() < 1e-6);
        assert_eq!(l.shadows.len(), 1);
        assert_eq!(l.opacity, 0.6);
        let text = t.text(&root, 24.0);
        assert_eq!(text.size_px, 0.8125 * 24.0);
        assert_eq!(text.weight, 600);
        assert!((text.tracking_px - 0.025 * 0.8125 * 24.0).abs() < 1e-4);
        assert_eq!(text.color, tokens::color("muted").unwrap());
        // A child without a size inherits the parent's line height as a length (text-meta's is 1.125rem).
        let child = text.child(&tw!("text-[0.6rem]").base, 24.0);
        assert_eq!(child.line_px(), 1.125 * 24.0);
        let f = tw!("btn focus-visible:bg-button-primary-hover");
        assert!(f.has(Marks::BTN));
        assert_eq!(f.look_in(&When::FocusVisible, 24.0, &root).bg, tokens::color("button-primary-hover").unwrap());
    }

    #[test]
    fn cn_joins_with_the_cascade() {
        let joined = cn(&[tw!("flex px-4"), tw!("p-2")]);
        assert_eq!(joined.base.padding[1], Some(Len::Rem(1.0)));
        assert_eq!(joined.base.padding[0], Some(Len::Rem(0.5)));
        // The same combination is the same static.
        assert!(std::ptr::eq(joined, cn(&[tw!("flex px-4"), tw!("p-2")])));
    }
}
