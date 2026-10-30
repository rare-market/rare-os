//! Tailwind v4 class strings into `Tw`, for exactly the utilities launcher/src uses (and their
//! obvious value variations), with the browser's cascade: utilities in Tailwind's own rule order,
//! then app.css's component classes (unlayered, so they win), then `!important` utilities.
//!
//! Anything else is an error, which the tw! macro turns into a compile error: a class the web
//! launcher never used is either a typo or outside DESIGN.md (no blur, no card shadows, no glow).

use crate::appcss::{self, sheet};
use crate::model::*;
use crate::order_list::PROPERTY_ORDER;
use crate::tokens;
use crate::value;
use std::borrow::Cow;
use std::cmp::Ordering;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub class: String,
    pub msg: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "`{}`: {}", self.class, self.msg)
    }
}

/// One class's contribution.
#[derive(Clone, Debug)]
pub struct Rule {
    pub decls: Decls,
    /// Declarations on other targets (children, `divide-*`, `space-*`).
    pub extra: Vec<(When, Decls)>,
    pub marks: Marks,
    /// Tailwind's sort key: property-order indices (ascending) and declaration count.
    pub order: Vec<u16>,
    pub count: u16,
}

impl Rule {
    fn new(props: &[&str], count: usize) -> Rule {
        let mut order: Vec<u16> = props.iter().filter_map(|p| PROPERTY_ORDER.iter().position(|q| q == p).map(|i| i as u16)).collect();
        order.sort_unstable();
        order.dedup();
        Rule { decls: Decls::NONE, extra: Vec::new(), marks: Marks::NONE, order, count: count as u16 }
    }
    fn with(props: &[&str], f: impl FnOnce(&mut Decls)) -> Rule {
        let mut r = Rule::new(props, props.len());
        f(&mut r.decls);
        r
    }
}

/// A class with its variant and flags.
#[derive(Clone, Debug)]
pub struct Parsed {
    pub name: String,
    pub when: Option<When>,
    pub important: bool,
    /// An app.css component class (unlayered: beats every utility).
    pub custom: Option<usize>,
    pub rule: Rule,
}

type R<T> = Result<T, String>;

/// Parses a whole class string.
pub fn parse(src: &str) -> Result<Tw, Vec<Error>> {
    let mut items = Vec::new();
    let mut errs = Vec::new();
    for c in src.split_whitespace() {
        match parse_class(c) {
            Ok(p) => items.push(p),
            Err(msg) => errs.push(Error { class: c.to_string(), msg }),
        }
    }
    if !errs.is_empty() {
        return Err(errs);
    }
    Ok(cascade(src, items))
}

/// Folds parsed classes in cascade order.
pub fn cascade(src: &str, mut items: Vec<Parsed>) -> Tw {
    let mut marks = Marks::NONE;
    for it in &items {
        marks = marks.union(it.rule.marks);
    }
    // Cascade order: layered utilities (Tailwind order), unlayered app.css rules, then !important.
    let tier = |p: &Parsed| -> u8 {
        if p.important {
            2
        } else if p.custom.is_some() {
            1
        } else {
            0
        }
    };
    items.sort_by(|a, b| tier(a).cmp(&tier(b)).then_with(|| match (a.custom, b.custom) {
        (Some(x), Some(y)) => x.cmp(&y),
        _ => tailwind_cmp(a, b),
    }));
    let mut base = Decls::NONE;
    let mut states: Vec<State> = Vec::new();
    let put = |when: Option<&When>, d: &Decls, states: &mut Vec<State>, base: &mut Decls| match when {
        None => base.overlay(d),
        Some(w) => match states.iter_mut().find(|s| &s.when == w) {
            Some(s) => s.decls.overlay(d),
            None => {
                let mut nd = Decls::NONE;
                nd.overlay(d);
                states.push(State { when: w.clone(), decls: nd })
            }
        },
    };
    for it in &items {
        put(it.when.as_ref(), &it.rule.decls, &mut states, &mut base);
        for (w, d) in &it.rule.extra {
            // A child rule under a variant (`first:divide-y`) is not used; children rules stand alone.
            put(Some(w), d, &mut states, &mut base);
        }
    }
    states.retain(|s| !s.decls.is_empty());
    Tw { src: Cow::Owned(src.split_whitespace().collect::<Vec<_>>().join(" ")), base, states: Cow::Owned(states), marks }
}

/// Tailwind's rule order (tailwindcss compileCandidates): variants, then the first differing
/// property-order index, then more declarations first, then the class name (numbers numerically).
pub fn tailwind_cmp(a: &Parsed, b: &Parsed) -> Ordering {
    let va = variant_rank(a.when.as_ref());
    let vb = variant_rank(b.when.as_ref());
    if va != vb {
        return va.cmp(&vb);
    }
    let (oa, ob) = (&a.rule.order, &b.rule.order);
    let mut i = 0;
    while i < oa.len() && i < ob.len() && oa[i] == ob[i] {
        i += 1;
    }
    let ia = oa.get(i).map(|v| *v as u32).unwrap_or(u32::MAX);
    let ib = ob.get(i).map(|v| *v as u32).unwrap_or(u32::MAX);
    ia.cmp(&ib).then(b.rule.count.cmp(&a.rule.count)).then_with(|| natural_cmp(&a.name, &b.name))
}

fn variant_rank(w: Option<&When>) -> u8 {
    match w {
        None => 0,
        Some(When::First) => 1,
        Some(When::Last) => 2,
        Some(When::Placeholder) => 3,
        Some(When::FocusVisible) => 4,
        Some(When::Disabled) => 5,
        Some(_) => 6,
    }
}

/// Tailwind's `compare`: digit runs compare as numbers.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let n = a.len().min(b.len());
    let mut s = 0;
    while s < n {
        let (x, y) = (a[s], b[s]);
        if x.is_ascii_digit() && y.is_ascii_digit() {
            let ea = s + a[s..].iter().take_while(|c| c.is_ascii_digit()).count();
            let eb = s + b[s..].iter().take_while(|c| c.is_ascii_digit()).count();
            let (na, nb) = (&a[s..ea], &b[s..eb]);
            let (fa, fb): (f64, f64) = (std::str::from_utf8(na).unwrap().parse().unwrap(), std::str::from_utf8(nb).unwrap().parse().unwrap());
            if fa != fb {
                return fa.partial_cmp(&fb).unwrap();
            }
            match na.cmp(nb) {
                Ordering::Equal => {}
                o => return o,
            }
            // Tailwind continues from the same index (both runs have equal text here).
            s += 1;
            continue;
        }
        if x != y {
            return x.cmp(&y);
        }
        s += 1;
    }
    a.len().cmp(&b.len())
}

/// Splits `variant:variant:utility` at top-level colons.
fn split_variants(c: &str) -> (Vec<&str>, &str) {
    let mut depth = 0i32;
    let mut cuts = Vec::new();
    for (i, ch) in c.char_indices() {
        match ch {
            '[' | '(' => depth += 1,
            ']' | ')' => depth -= 1,
            ':' if depth == 0 => cuts.push(i),
            _ => {}
        }
    }
    let mut vars = Vec::new();
    let mut start = 0;
    for &i in &cuts {
        vars.push(&c[start..i]);
        start = i + 1;
    }
    (vars, &c[start..])
}

fn variant(v: &str) -> R<When> {
    Ok(match v {
        "focus-visible" => When::FocusVisible,
        "first" => When::First,
        "last" => When::Last,
        "disabled" => When::Disabled,
        "placeholder" => When::Placeholder,
        _ => {
            if let Some(tag) = v.strip_prefix("[&>").and_then(|t| t.strip_suffix(']')) {
                if tag.chars().all(|c| c.is_ascii_alphanumeric()) && !tag.is_empty() {
                    return Ok(When::Child(Cow::Owned(tag.to_string())));
                }
            }
            if let Some(inner) = v.strip_prefix("[[").and_then(|t| t.strip_suffix("]:focus_&]")) {
                if let Some((attr, value)) = inner.split_once('=') {
                    if attr.starts_with("data-") {
                        return Ok(When::AncestorFocus { attr: Cow::Owned(attr.to_string()), value: Cow::Owned(value.trim_matches('"').to_string()) });
                    }
                }
            }
            return Err(format!("variant `{v}:` is not used by the web launcher (supported: focus-visible, first, last, disabled, placeholder, [&>tag], [[data-x=y]:focus_&])"));
        }
    })
}

/// Parses one class token.
pub fn parse_class(c: &str) -> R<Parsed> {
    let (vars, util) = split_variants(c);
    let mut when = None;
    for v in vars {
        if when.is_some() {
            return Err("stacked variants are not used by the web launcher".into());
        }
        when = Some(variant(v)?);
    }
    if when.is_none() {
        if let Some(ix) = custom_index(util) {
            return Ok(Parsed { name: c.to_string(), when: None, important: false, custom: Some(ix), rule: custom_rule(util)? });
        }
    }
    let (important, u) = match (util.strip_prefix('!'), util.strip_suffix('!')) {
        (Some(u), _) => (true, u),
        (_, Some(u)) => (true, u),
        _ => (false, util),
    };
    let rule = if u.starts_with('[') && u.ends_with(']') && u.contains(':') {
        arbitrary_property(&u[1..u.len() - 1])?
    } else {
        utility(u)?
    };
    Ok(Parsed { name: c.to_string(), when, important, custom: None, rule })
}

// ---------------------------------------------------------------------------------------------
// app.css component classes

/// Behaviour-only classes (their CSS is in compound selectors like `.tile[data-nav]:focus`).
const MARK_CLASSES: &[(&str, Marks)] = &[
    ("tile", Marks::TILE),
    ("tile-wide", Marks::TILE_WIDE),
    ("btn", Marks::BTN),
    ("strip-tile", Marks::STRIP_TILE),
    ("strip-label", Marks::STRIP_LABEL),
    ("focus-text", Marks::FOCUS_TEXT),
    ("num", Marks::NUM),
    ("trow", Marks::TROW),
    ("cv-row", Marks::CV_ROW),
    ("cv-cell", Marks::CV_CELL),
    ("backdrop-layer", Marks::BACKDROP_LAYER),
    ("load-bar", Marks::LOAD_BAR),
    // index.html's boot splash (inline <style> there, not app.css).
    ("splash", Marks::SPLASH),
    ("splash-gem", Marks::SPLASH_GEM),
    // Tailwind's group marker.
    ("group", Marks::GROUP),
];

/// Where a component class sits in the cascade (its first rule in app.css), if it is one.
fn custom_index(name: &str) -> Option<usize> {
    let sel = format!(".{name}");
    let sh = sheet();
    if let Some(i) = sh.rules.iter().position(|(pre, _)| pre.split(',').any(|s| s.trim() == sel)) {
        return Some(i);
    }
    MARK_CLASSES.iter().any(|(n, _)| *n == name).then_some(usize::MAX)
}

/// A component class: its marks plus every app.css rule whose selector is exactly `.name`
/// (and `.name > *` for children), interpreted declaration by declaration.
fn custom_rule(name: &str) -> R<Rule> {
    let mut r = Rule::new(&[], 0);
    if let Some((_, m)) = MARK_CLASSES.iter().find(|(n, _)| *n == name) {
        r.marks = *m;
    }
    let sel = format!(".{name}");
    let child = format!(".{name} > *");
    for (pre, body) in &sheet().rules {
        let sels: Vec<&str> = pre.split(',').map(|s| s.trim()).collect();
        if sels.contains(&sel.as_str()) {
            for (k, v) in appcss::decls(body) {
                css_decl(&k, &v, &mut r.decls).map_err(|e| format!("app.css `{sel}`: {e}"))?;
            }
        } else if sels.contains(&child.as_str()) {
            let mut d = Decls::NONE;
            for (k, v) in appcss::decls(body) {
                css_decl(&k, &v, &mut d).map_err(|e| format!("app.css `{child}`: {e}"))?;
            }
            r.extra.push((When::Children, d));
        }
    }
    Ok(r)
}

/// One CSS declaration (app.css rules and `[prop:value]` classes) into the model.
pub fn css_decl(prop: &str, v: &str, d: &mut Decls) -> R<()> {
    let bad = || format!("`{prop}: {v}` is not supported");
    let v = v.trim();
    match prop {
        "display" => {
            d.display = Some(match v {
                "block" => Display::Block,
                "inline" => Display::Inline,
                "inline-block" => Display::InlineBlock,
                "flex" => Display::Flex,
                "inline-flex" => Display::InlineFlex,
                "grid" => Display::Grid,
                "contents" => Display::Contents,
                "none" => Display::None,
                _ => return Err(bad()),
            })
        }
        "opacity" => d.opacity = Some(value::num(v).or_else(|| v.strip_suffix('%').and_then(value::num).map(|p| p / 100.0)).ok_or_else(bad)?),
        "overflow" => {
            let o = overflow(v).ok_or_else(bad)?;
            d.overflow = [Some(o), Some(o)];
        }
        "overflow-x" => d.overflow[0] = Some(overflow(v).ok_or_else(bad)?),
        "overflow-y" => d.overflow[1] = Some(overflow(v).ok_or_else(bad)?),
        "column-gap" => d.gap[1] = Some(value::len(v).ok_or_else(bad)?),
        "row-gap" => d.gap[0] = Some(value::len(v).ok_or_else(bad)?),
        "min-width" => d.min_width = Some(value::len(v).ok_or_else(bad)?),
        "text-overflow" if v == "ellipsis" => d.ellipsis = Some(true),
        "white-space" => d.white_space = Some(white_space(v).ok_or_else(bad)?),
        "font-variant-numeric" if v == "tabular-nums" => d.tabular = Some(true),
        "font-variant-ligatures" => d.ligatures = Some(v != "none"),
        "font-family" => {
            d.family = Some(match v {
                "var(--font-mono)" => Family::Mono,
                "var(--font-sans)" => Family::Sans,
                _ => return Err(bad()),
            })
        }
        "transform-origin" => {
            let pos = match v {
                "0 0" | "0px 0px" => Some([Len::ZERO, Len::ZERO]),
                _ => value::position(v),
            };
            d.transform_origin = Some(pos.ok_or_else(bad)?)
        }
        "transform" => {
            for f in value::words(v) {
                let (name, arg) = f.strip_suffix(')').and_then(|f| f.split_once('(')).ok_or_else(bad)?;
                match name {
                    "translateX" => d.translate[0] = Some(value::len(arg).ok_or_else(bad)?),
                    "translateY" => d.translate[1] = Some(value::len(arg).ok_or_else(bad)?),
                    "scale" => d.scale = Some(value::num(arg).ok_or_else(bad)?),
                    _ => return Err(bad()),
                }
            }
        }
        "transition" => {
            let mut props = Vec::new();
            let (mut dur, mut ease) = (None, None);
            for part in value::split_top(v, ',') {
                let w = value::words(&sheet().resolve_vars(&part));
                props.push(TProp::from_css(w.first().ok_or_else(bad)?).ok_or_else(bad)?);
                for x in &w[1..] {
                    if let Some(ms) = value::time_ms(x) {
                        dur = Some(ms);
                    } else {
                        ease = Some(value::ease(x).ok_or_else(bad)?);
                    }
                }
            }
            d.transition = Some(Cow::Owned(props));
            d.duration = Some(dur.unwrap_or(0.0));
            d.ease = Some(ease.unwrap_or([0.25, 0.1, 0.25, 1.0]));
        }
        "animation" => {
            let w = value::words(&sheet().resolve_vars(v));
            let kind = anim_kind(w.first().ok_or_else(bad)?).ok_or_else(bad)?;
            let mut a = Animation { kind, ms: 0.0, ease: [0.25, 0.1, 0.25, 1.0], delay_ms: 0.0, infinite: false };
            let mut times = 0;
            for x in &w[1..] {
                if let Some(ms) = value::time_ms(x) {
                    if times == 0 {
                        a.ms = ms
                    } else {
                        a.delay_ms = ms
                    }
                    times += 1;
                } else if let Some(e) = value::ease(x) {
                    a.ease = e;
                } else if x == "infinite" {
                    a.infinite = true;
                } else if !matches!(x.as_str(), "1" | "both" | "forwards" | "backwards" | "none") {
                    return Err(bad());
                }
            }
            d.anim = Some(a);
        }
        "animation-name" => {
            let a = d.anim.as_mut().ok_or_else(bad)?;
            a.kind = anim_kind(v).ok_or_else(bad)?;
        }
        "mask-image" => {
            let url = v.strip_prefix("url(").and_then(|u| u.strip_suffix(')')).ok_or_else(bad)?;
            d.mask_image = Some(Cow::Owned(url.trim_matches(|c| c == '"' || c == '\'').to_string()));
        }
        "mask-position" => d.mask_position = Some(value::position(v).ok_or_else(bad)?),
        "mask-repeat" => d.mask_repeat = Some(v != "no-repeat"),
        "mask-size" => {
            let w = value::words(v);
            let l: Vec<Len> = w.iter().map(|x| value::len(x)).collect::<Option<_>>().ok_or_else(bad)?;
            d.mask_size = Some(match l.len() {
                1 => [l[0], Len::Auto],
                2 => [l[0], l[1]],
                _ => return Err(bad()),
            });
        }
        "text-shadow" => d.text_shadow = Some(value::shadow(v).ok_or_else(bad)?),
        // Paint hints with no visual effect of their own here.
        "will-change" | "content-visibility" | "contain-intrinsic-size" => {}
        _ => return Err(bad()),
    }
    Ok(())
}

fn anim_kind(name: &str) -> Option<Anim> {
    Some(match name {
        "pulse" => Anim::Pulse,
        "fade-in" => Anim::FadeIn,
        "slide-in-right" => Anim::SlideInRight,
        "slide-up" => Anim::SlideUp,
        "load-bar" => Anim::LoadBar,
        "num-roll-up" => Anim::NumRollUp,
        "num-roll-down" => Anim::NumRollDown,
        "tint-up" => Anim::TintUp,
        "tint-down" => Anim::TintDown,
        "breathe" => Anim::Breathe,
        "rise" => Anim::Rise,
        "fade" => Anim::Fade,
        "pop" => Anim::Pop,
        "boot-sweep" => Anim::BootSweep,
        _ => return None,
    })
}

fn overflow(v: &str) -> Option<Overflow> {
    Some(match v {
        "visible" => Overflow::Visible,
        "hidden" | "clip" => Overflow::Hidden,
        "auto" | "scroll" => Overflow::Auto,
        _ => return None,
    })
}

fn white_space(v: &str) -> Option<WhiteSpace> {
    Some(match v {
        "normal" => WhiteSpace::Normal,
        "nowrap" => WhiteSpace::NoWrap,
        "pre-wrap" => WhiteSpace::PreWrap,
        _ => return None,
    })
}

/// `[prop:value]`: the arbitrary properties the web launcher uses.
fn arbitrary_property(inner: &str) -> R<Rule> {
    let (prop, v) = inner.split_once(':').ok_or("not a [property:value]")?;
    const OK: &[&str] = &["mask-image", "mask-position", "mask-repeat", "mask-size", "text-shadow", "font-variant-ligatures"];
    if !OK.contains(&prop) {
        return Err(format!("arbitrary property `{prop}` is not used by the web launcher (supported: {})", OK.join(", ")));
    }
    let v = decode_arbitrary(v);
    let mut r = Rule::new(&[prop], 1);
    css_decl(prop, &v, &mut r.decls)?;
    Ok(r)
}

/// Tailwind's arbitrary value decoding: `_` is a space (kept inside url()), `\_` is `_`.
fn decode_arbitrary(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut in_url = 0;
    let b = v.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if v[i..].starts_with("url(") {
            in_url += 1;
        }
        let c = b[i] as char;
        match c {
            '\\' if i + 1 < b.len() && b[i + 1] == b'_' => {
                out.push('_');
                i += 2;
                continue;
            }
            '_' if in_url == 0 => out.push(' '),
            ')' if in_url > 0 => {
                in_url -= 1;
                out.push(c)
            }
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Utilities

/// `[...]` contents, decoded.
fn arb(v: &str) -> Option<String> {
    let inner = v.strip_prefix('[')?.strip_suffix(']')?;
    Some(decode_arbitrary(inner))
}

/// Spacing scale value (`4` = 1rem, `2.5`, `px`, `[1.5rem]`). Negative via `neg`.
fn spacing(v: &str) -> Option<Len> {
    if v == "px" {
        return Some(Len::Px(1.0));
    }
    if let Some(a) = arb(v) {
        return value::len(&a);
    }
    let n = value::num(v)?;
    // Tailwind accepts multiples of 0.25.
    if n < 0.0 || (n * 4.0).fract() != 0.0 || v.starts_with('-') || v.contains('e') {
        return None;
    }
    Some(if n == 0.0 { Len::ZERO } else { Len::Rem(n * tokens::SPACING_REM) })
}

fn fraction(v: &str) -> Option<Len> {
    let (a, b) = v.split_once('/')?;
    let (a, b): (u32, u32) = (a.parse().ok()?, b.parse().ok()?);
    (b != 0).then(|| Len::Pct(a as f32 / b as f32 * 100.0))
}

/// Sizes: spacing, fractions, `full`, `auto`, `px`, `[arbitrary]`.
fn size_value(v: &str, auto: bool, full: bool, frac: bool) -> Option<Len> {
    match v {
        "auto" if auto => Some(Len::Auto),
        "full" if full => Some(Len::Pct(100.0)),
        _ => spacing(v).or_else(|| if frac { fraction(v) } else { None }),
    }
}

/// `name[/alpha]` colour token or `[color]`.
fn color_value(v: &str) -> Option<Rgba> {
    if let Some(a) = arb(v) {
        return value::color(&a);
    }
    let (name, alpha) = match v.rsplit_once('/') {
        Some((n, a)) => (n, Some(a)),
        None => (v, None),
    };
    let c = tokens::color(name)?;
    match alpha {
        None => Some(c),
        Some(a) => {
            let p: f32 = if let Some(x) = arb(a) { value::num(&x)? * if x.ends_with('%') { 0.01 } else { 1.0 } } else { a.parse::<u16>().ok().filter(|p| *p <= 100)? as f32 / 100.0 };
            Some(c.alpha(p))
        }
    }
}

const SIDES: [(&str, &[usize]); 7] = [("", &[0, 1, 2, 3]), ("x", &[1, 3]), ("y", &[0, 2]), ("t", &[0]), ("r", &[1]), ("b", &[2]), ("l", &[3])];

/// The utility families: (root, handler(value, negative)). Bare utilities use value "".
fn utility(u: &str) -> R<Rule> {
    let (neg, name) = match u.strip_prefix('-') {
        Some(n) => (true, n),
        None => (false, u),
    };
    if !neg {
        if let Some(r) = static_utility(name) {
            return Ok(r);
        }
    }
    // Try roots from longest to shortest (Tailwind's functional utility lookup); the value never
    // splits inside brackets.
    let head_end = name.find('[').unwrap_or(name.len());
    let mut cuts: Vec<usize> = name[..head_end].match_indices('-').map(|(i, _)| i).collect();
    cuts.reverse();
    let mut last_err = None;
    let try_root = |root: &str, v: &str| functional(root, v, neg);
    // The whole name as a bare functional utility (`border`, `rounded`, `ring`, `outline`).
    if let Some(res) = try_root(name, "") {
        match res {
            Ok(r) => return Ok(r),
            Err(e) => last_err = Some(e),
        }
    }
    for i in cuts {
        let (root, v) = (&name[..i], &name[i + 1..]);
        if let Some(res) = try_root(root, v) {
            match res {
                Ok(r) => return Ok(r),
                Err(e) => last_err = last_err.or(Some(e)),
            }
        }
    }
    Err(last_err.unwrap_or_else(|| "not a utility the web launcher uses".into()))
}

fn static_utility(n: &str) -> Option<Rule> {
    let display = |d: Display| Rule::with(&["display"], |x| x.display = Some(d));
    let position = |p: Position| Rule::with(&["position"], |x| x.position = Some(p));
    Some(match n {
        "block" => display(Display::Block),
        "inline" => display(Display::Inline),
        "inline-block" => display(Display::InlineBlock),
        "flex" => display(Display::Flex),
        "inline-flex" => display(Display::InlineFlex),
        "grid" => display(Display::Grid),
        "contents" => display(Display::Contents),
        "hidden" => display(Display::None),
        "static" => position(Position::Static),
        "relative" => position(Position::Relative),
        "absolute" => position(Position::Absolute),
        "fixed" => position(Position::Fixed),
        "sticky" => position(Position::Sticky),
        "visible" => Rule::with(&["visibility"], |x| x.visible = Some(true)),
        "invisible" => Rule::with(&["visibility"], |x| x.visible = Some(false)),
        "flex-row" => Rule::with(&["flex-direction"], |x| x.flex_direction = Some(FlexDirection::Row)),
        "flex-col" => Rule::with(&["flex-direction"], |x| x.flex_direction = Some(FlexDirection::Column)),
        "flex-wrap" => Rule::with(&["flex-wrap"], |x| x.flex_wrap = Some(FlexWrap::Wrap)),
        "flex-nowrap" => Rule::with(&["flex-wrap"], |x| x.flex_wrap = Some(FlexWrap::NoWrap)),
        "items-start" => Rule::with(&["align-items"], |x| x.align_items = Some(Align::Start)),
        "items-end" => Rule::with(&["align-items"], |x| x.align_items = Some(Align::End)),
        "items-center" => Rule::with(&["align-items"], |x| x.align_items = Some(Align::Center)),
        "items-stretch" => Rule::with(&["align-items"], |x| x.align_items = Some(Align::Stretch)),
        "items-baseline" => Rule::with(&["align-items"], |x| x.align_items = Some(Align::Baseline)),
        "self-start" => Rule::with(&["align-self"], |x| x.align_self = Some(Align::Start)),
        "self-end" => Rule::with(&["align-self"], |x| x.align_self = Some(Align::End)),
        "self-center" => Rule::with(&["align-self"], |x| x.align_self = Some(Align::Center)),
        "self-stretch" => Rule::with(&["align-self"], |x| x.align_self = Some(Align::Stretch)),
        "justify-start" => Rule::with(&["justify-content"], |x| x.justify_content = Some(Justify::Start)),
        "justify-end" => Rule::with(&["justify-content"], |x| x.justify_content = Some(Justify::End)),
        "justify-center" => Rule::with(&["justify-content"], |x| x.justify_content = Some(Justify::Center)),
        "justify-between" => Rule::with(&["justify-content"], |x| x.justify_content = Some(Justify::Between)),
        "grow" => Rule::with(&["flex-grow"], |x| x.flex_grow = Some(1.0)),
        "shrink" => Rule::with(&["flex-shrink"], |x| x.flex_shrink = Some(1.0)),
        "truncate" => Rule::with(&["overflow", "text-overflow", "white-space"], |x| {
            x.overflow = [Some(Overflow::Hidden); 2];
            x.ellipsis = Some(true);
            x.white_space = Some(WhiteSpace::NoWrap);
        }),
        "whitespace-normal" => Rule::with(&["white-space"], |x| x.white_space = Some(WhiteSpace::Normal)),
        "whitespace-nowrap" => Rule::with(&["white-space"], |x| x.white_space = Some(WhiteSpace::NoWrap)),
        "whitespace-pre-wrap" => Rule::with(&["white-space"], |x| x.white_space = Some(WhiteSpace::PreWrap)),
        "break-all" => Rule::with(&["word-break"], |x| x.break_all = Some(true)),
        "break-words" => Rule::with(&["overflow-wrap"], |x| x.break_words = Some(true)),
        "uppercase" => Rule::with(&["text-transform"], |x| x.uppercase = Some(true)),
        "normal-case" => Rule::with(&["text-transform"], |x| x.uppercase = Some(false)),
        "underline" => Rule::with(&["text-decoration-line"], |x| x.decoration = Some(Decoration::Underline)),
        "line-through" => Rule::with(&["text-decoration-line"], |x| x.decoration = Some(Decoration::LineThrough)),
        "no-underline" => Rule::with(&["text-decoration-line"], |x| x.decoration = Some(Decoration::None)),
        "tabular-nums" => Rule::with(&["--tw-numeric-spacing", "font-variant-numeric"], |x| x.tabular = Some(true)),
        "text-left" => Rule::with(&["text-align"], |x| x.text_align = Some(TextAlign::Left)),
        "text-center" => Rule::with(&["text-align"], |x| x.text_align = Some(TextAlign::Center)),
        "text-right" => Rule::with(&["text-align"], |x| x.text_align = Some(TextAlign::Right)),
        "pointer-events-none" => Rule::with(&["pointer-events"], |x| x.pointer_events = Some(false)),
        "pointer-events-auto" => Rule::with(&["pointer-events"], |x| x.pointer_events = Some(true)),
        "select-none" => Rule::with(&["-webkit-user-select", "user-select"], |x| x.user_select = Some(UserSelect::None)),
        "select-text" => Rule::with(&["-webkit-user-select", "user-select"], |x| x.user_select = Some(UserSelect::Text)),
        "select-all" => Rule::with(&["-webkit-user-select", "user-select"], |x| x.user_select = Some(UserSelect::All)),
        "grayscale" => Rule::with(&["--tw-grayscale", "filter"], |x| x.grayscale = Some(1.0)),
        "mix-blend-lighten" => Rule::with(&["mix-blend-mode"], |x| x.blend = Some(Blend::Lighten)),
        "bg-cover" => Rule::with(&["background-size"], |x| x.bg_cover = Some(true)),
        "bg-center" => Rule::with(&["background-position"], |x| x.bg_position = Some([Len::Pct(50.0); 2])),
        "object-cover" => Rule::with(&["object-fit"], |x| x.object_fit = Some(Fit::Cover)),
        "object-contain" => Rule::with(&["object-fit"], |x| x.object_fit = Some(Fit::Contain)),
        "outline-none" => Rule::with(&["--tw-outline-style", "outline-style"], |x| x.outline_style = Some(OutlineStyle::None)),
        "aspect-square" => Rule::with(&["aspect-ratio"], |x| x.aspect = Some(1.0)),
        "aspect-video" => Rule::with(&["aspect-ratio"], |x| x.aspect = Some(16.0 / 9.0)),
        "overflow-hidden" => Rule::with(&["overflow"], |x| x.overflow = [Some(Overflow::Hidden); 2]),
        "overflow-visible" => Rule::with(&["overflow"], |x| x.overflow = [Some(Overflow::Visible); 2]),
        "overflow-auto" => Rule::with(&["overflow"], |x| x.overflow = [Some(Overflow::Auto); 2]),
        "overflow-x-auto" => Rule::with(&["overflow-x"], |x| x.overflow[0] = Some(Overflow::Auto)),
        "overflow-y-auto" => Rule::with(&["overflow-y"], |x| x.overflow[1] = Some(Overflow::Auto)),
        "overflow-x-hidden" => Rule::with(&["overflow-x"], |x| x.overflow[0] = Some(Overflow::Hidden)),
        "overflow-y-hidden" => Rule::with(&["overflow-y"], |x| x.overflow[1] = Some(Overflow::Hidden)),
        "shadow-xl" => shadow_rule(&[(20.0, 25.0, -5.0), (8.0, 10.0, -6.0)], 0.1),
        "shadow-2xl" => shadow_rule(&[(25.0, 50.0, -12.0)], 0.25),
        "backdrop-blur-sm" => Rule::with(&["--tw-backdrop-blur", "-webkit-backdrop-filter", "backdrop-filter"], |x| x.backdrop_blur = Some(Len::Px(8.0))),
        "animate-pulse" => Rule::with(&["animation"], |x| {
            x.anim = Some(Animation { kind: Anim::Pulse, ms: 2000.0, ease: [0.4, 0.0, 0.6, 1.0], delay_ms: 0.0, infinite: true })
        }),
        "divide-y" => {
            let mut r = Rule::new(&["divide-y-width"], 6);
            let mut d = Decls::NONE;
            d.border[0] = Some(Len::ZERO);
            d.border[2] = Some(Len::Px(1.0));
            r.extra.push((When::ChildrenNotLast, d));
            r
        }
        "transition-colors" => transition_rule(&[
            TProp::Color,
            TProp::BackgroundColor,
            TProp::BorderColor,
            TProp::OutlineColor,
            TProp::TextDecorationColor,
            TProp::Fill,
            TProp::Stroke,
            TProp::GradientFrom,
            TProp::GradientVia,
            TProp::GradientTo,
        ]),
        "transition-opacity" => transition_rule(&[TProp::Opacity]),
        "transition-transform" => transition_rule(&[TProp::Transform, TProp::Translate, TProp::Scale, TProp::Rotate]),
        _ => return None,
    })
}

/// Tailwind's named shadows: (y, blur, spread) layers in px, black at `alpha`.
fn shadow_rule(layers: &[(f32, f32, f32)], alpha: f32) -> Rule {
    let v: Vec<Shadow> =
        layers.iter().map(|&(y, b, s)| Shadow { x: Len::ZERO, y: Len::Px(y), blur: Len::Px(b), spread: Len::Px(s), color: Rgba::rgb(0, 0, 0).alpha(alpha) }).collect();
    Rule::with(&["--tw-shadow", "box-shadow"], |x| x.shadow = Some(Cow::Owned(v)))
}

fn transition_rule(props: &[TProp]) -> Rule {
    let p = props.to_vec();
    Rule::with(&["transition-property", "transition-timing-function", "transition-duration"], |x| x.transition = Some(Cow::Owned(p)))
}

/// Functional utilities. `None` = this root is not a family; `Some(Err)` = family, bad value.
fn functional(root: &str, v: &str, neg: bool) -> Option<R<Rule>> {
    let bad = || Err(format!("`{v}` is not a value `{root}-*` takes here"));
    let negok = |l: Len| if neg { l.neg() } else { l };
    // Families that never take a negative value.
    let no_neg = || -> Option<R<Rule>> { Some(Err(format!("`-{root}-*` (negative) is not supported"))) };

    // Insets.
    let inset_sides: Option<(&[usize], &[&str])> = match root {
        "inset" => Some((&[0, 1, 2, 3], &["inset"])),
        "inset-x" => Some((&[1, 3], &["inset-inline"])),
        "inset-y" => Some((&[0, 2], &["inset-block"])),
        "top" => Some((&[0], &["top"])),
        "right" => Some((&[1], &["right"])),
        "bottom" => Some((&[2], &["bottom"])),
        "left" => Some((&[3], &["left"])),
        _ => None,
    };
    if let Some((sides, props)) = inset_sides {
        let Some(l) = size_value(v, true, true, true) else { return Some(bad()) };
        let l = negok(l);
        return Some(Ok(Rule::with(props, |x| {
            for &s in sides {
                x.inset[s] = Some(l);
            }
        })));
    }

    // Margin and padding.
    for (pre, is_m) in [("m", true), ("p", false)] {
        if let Some(side) = root.strip_prefix(pre) {
            let Some((_, sides)) = SIDES.iter().find(|(s, _)| *s == side) else { continue };
            if !is_m && neg {
                return no_neg();
            }
            let l = if is_m && v == "auto" { Some(Len::Auto) } else { spacing(v) };
            let Some(l) = l else { return Some(bad()) };
            let l = negok(l);
            let prop: &'static str = match (is_m, side) {
                (true, "") => "margin",
                (true, "x") => "margin-inline",
                (true, "y") => "margin-block",
                (true, "t") => "margin-top",
                (true, "r") => "margin-right",
                (true, "b") => "margin-bottom",
                (true, _) => "margin-left",
                (false, "") => "padding",
                (false, "x") => "padding-inline",
                (false, "y") => "padding-block",
                (false, "t") => "padding-top",
                (false, "r") => "padding-right",
                (false, "b") => "padding-bottom",
                (false, _) => "padding-left",
            };
            return Some(Ok(Rule::with(&[prop], |x| {
                for &s in *sides {
                    if is_m {
                        x.margin[s] = Some(l)
                    } else {
                        x.padding[s] = Some(l)
                    }
                }
            })));
        }
    }

    let r = match root {
        "w" | "h" | "size" | "min-w" | "min-h" | "max-w" | "max-h" => {
            if neg {
                return no_neg();
            }
            let l = match (root, v) {
                ("max-w" | "max-h", "none") => None,
                ("max-w", c) if tokens::container(c).is_some() => tokens::container(c),
                ("min-w" | "min-h" | "max-w" | "max-h", _) => match size_value(v, false, true, false) {
                    Some(l) => Some(l),
                    None => return Some(bad()),
                },
                _ => match size_value(v, true, true, true) {
                    Some(l) => Some(l),
                    None => return Some(bad()),
                },
            };
            let none = l.is_none();
            let l = l.unwrap_or(Len::Auto);
            match root {
                "w" => Rule::with(&["width"], |x| x.width = Some(l)),
                "h" => Rule::with(&["height"], |x| x.height = Some(l)),
                // Tailwind adds a `--tw-sort: size` declaration (counted, not ordered).
                "size" => {
                    let mut r = Rule::with(&["width", "height"], |x| {
                        x.width = Some(l);
                        x.height = Some(l)
                    });
                    r.count = 3;
                    r
                }
                "min-w" => Rule::with(&["min-width"], |x| x.min_width = Some(l)),
                "min-h" => Rule::with(&["min-height"], |x| x.min_height = Some(l)),
                // max-*-none: CSS `none` is "no limit", the same as auto here.
                "max-w" => Rule::with(&["max-width"], |x| x.max_width = Some(if none { Len::Auto } else { l })),
                _ => Rule::with(&["max-height"], |x| x.max_height = Some(if none { Len::Auto } else { l })),
            }
        }
        "gap" | "gap-x" | "gap-y" => {
            if neg {
                return no_neg();
            }
            let Some(l) = spacing(v) else { return Some(bad()) };
            match root {
                "gap" => Rule::with(&["gap"], |x| x.gap = [Some(l); 2]),
                "gap-x" => Rule::with(&["column-gap"], |x| x.gap[1] = Some(l)),
                _ => Rule::with(&["row-gap"], |x| x.gap[0] = Some(l)),
            }
        }
        "space-x" | "space-y" => {
            let Some(l) = spacing(v) else { return Some(bad()) };
            let l = negok(l);
            let x = root == "space-x";
            // Sorted by a `--tw-sort` on the gap it stands in for.
            let mut r = Rule::new(&[if x { "row-gap" } else { "column-gap" }], 4);
            let mut d = Decls::NONE;
            if x {
                d.margin[3] = Some(Len::ZERO);
                d.margin[1] = Some(l);
            } else {
                d.margin[0] = Some(Len::ZERO);
                d.margin[2] = Some(l);
            }
            r.extra.push((When::ChildrenNotLast, d));
            r
        }
        "z" => {
            let n = arb(v).as_deref().and_then(|a| a.parse::<i32>().ok()).or_else(|| v.parse::<i32>().ok().filter(|n| *n >= 0));
            let Some(n) = n else { return Some(bad()) };
            Rule::with(&["z-index"], |x| x.z = Some(if neg { -n } else { n }))
        }
        "opacity" => {
            if neg {
                return no_neg();
            }
            let o = arb(v).as_deref().and_then(value::num).or_else(|| v.parse::<u16>().ok().filter(|p| *p <= 100).map(|p| p as f32 / 100.0));
            let Some(o) = o else { return Some(bad()) };
            Rule::with(&["opacity"], |x| x.opacity = Some(o))
        }
        "aspect" => {
            let Some(a) = arb(v) else { return Some(bad()) };
            let r = match a.split_once('/') {
                Some((w, h)) => value::num(w).zip(value::num(h)).map(|(w, h)| w / h),
                None => value::num(&a),
            };
            let Some(r) = r else { return Some(bad()) };
            Rule::with(&["aspect-ratio"], |x| x.aspect = Some(r))
        }
        "flex" => {
            // flex-1, flex-[2]: `flex: N` = grow N, shrink 1, basis 0%.
            let n = arb(v).as_deref().and_then(value::num).or_else(|| v.parse::<u16>().ok().map(|n| n as f32));
            let Some(n) = n else { return Some(bad()) };
            Rule::with(&["flex"], |x| {
                x.flex_grow = Some(n);
                x.flex_shrink = Some(1.0);
                x.flex_basis = Some(Len::Pct(0.0));
            })
        }
        "shrink" | "grow" => {
            let Ok(n) = v.parse::<u16>() else { return Some(bad()) };
            if root == "shrink" {
                Rule::with(&["flex-shrink"], |x| x.flex_shrink = Some(n as f32))
            } else {
                Rule::with(&["flex-grow"], |x| x.flex_grow = Some(n as f32))
            }
        }
        "grid-cols" | "grid-rows" => {
            let tracks = if let Some(a) = arb(v) {
                match tracks(&a) {
                    Some(t) => t,
                    None => return Some(bad()),
                }
            } else {
                let Some(n) = v.parse::<u16>().ok().filter(|n| *n > 0) else { return Some(bad()) };
                vec![Track::Repeat(n, Cow::Owned(vec![Track::MinMax(Sizing::Len(Len::ZERO), Sizing::Fr(1.0))]))]
            };
            if root == "grid-cols" {
                Rule::with(&["grid-template-columns"], |x| x.grid_cols = Some(Cow::Owned(tracks)))
            } else {
                Rule::with(&["grid-template-rows"], |x| x.grid_rows = Some(Cow::Owned(tracks)))
            }
        }
        "col-span" => {
            let Some(n) = v.parse::<u16>().ok().filter(|n| *n > 0) else { return Some(bad()) };
            Rule::with(&["grid-column"], |x| x.grid_column = Some(Line::Span(n)))
        }
        "col-start" | "row-start" => {
            let Ok(n) = v.parse::<i16>() else { return Some(bad()) };
            if root == "col-start" {
                Rule::with(&["grid-column-start"], |x| x.grid_column = Some(Line::At(n)))
            } else {
                Rule::with(&["grid-row-start"], |x| x.grid_row = Some(Line::At(n)))
            }
        }
        "rounded" | "rounded-t" | "rounded-r" | "rounded-b" | "rounded-l" | "rounded-tl" | "rounded-tr" | "rounded-br" | "rounded-bl" => {
            if neg {
                return no_neg();
            }
            let l = match v {
                "" => Some(Len::Rem(0.25)),
                "full" => Some(Len::FULL_RADIUS),
                "none" => Some(Len::ZERO),
                _ => tokens::radius(v).or_else(|| arb(v).as_deref().and_then(value::len)),
            };
            let Some(l) = l else { return Some(bad()) };
            let (corners, props): (&[usize], &[&str]) = match root {
                "rounded" => (&[0, 1, 2, 3], &["border-radius"]),
                "rounded-t" => (&[0, 1], &["border-top-left-radius", "border-top-right-radius"]),
                "rounded-r" => (&[1, 2], &["border-top-right-radius", "border-bottom-right-radius"]),
                "rounded-b" => (&[2, 3], &["border-bottom-right-radius", "border-bottom-left-radius"]),
                "rounded-l" => (&[0, 3], &["border-top-left-radius", "border-bottom-left-radius"]),
                "rounded-tl" => (&[0], &["border-top-left-radius"]),
                "rounded-tr" => (&[1], &["border-top-right-radius"]),
                "rounded-br" => (&[2], &["border-bottom-right-radius"]),
                _ => (&[3], &["border-bottom-left-radius"]),
            };
            Rule::with(props, |x| {
                for &c in corners {
                    x.radius[c] = Some(l);
                }
            })
        }
        "border" | "border-t" | "border-r" | "border-b" | "border-l" | "border-x" | "border-y" => {
            if neg {
                return no_neg();
            }
            let side = root.strip_prefix("border").unwrap().trim_start_matches('-');
            let (_, sides) = SIDES.iter().find(|(s, _)| *s == side).unwrap();
            let width = match v {
                "" => Some(Len::Px(1.0)),
                _ => match v.parse::<u16>() {
                    Ok(n) => Some(Len::Px(n as f32)),
                    Err(_) => arb(v).as_deref().and_then(value::len),
                },
            };
            if let Some(w) = width {
                let (style, wprop): (&str, &str) = match side {
                    "" => ("border-style", "border-width"),
                    "x" => ("border-inline-style", "border-inline-width"),
                    "y" => ("border-block-style", "border-block-width"),
                    "t" => ("border-top-style", "border-top-width"),
                    "r" => ("border-right-style", "border-right-width"),
                    "b" => ("border-bottom-style", "border-bottom-width"),
                    _ => ("border-left-style", "border-left-width"),
                };
                Rule::with(&[style, wprop], |x| {
                    for &s in *sides {
                        x.border[s] = Some(w);
                    }
                })
            } else if side.is_empty() {
                let Some(c) = color_value(v) else { return Some(bad()) };
                Rule::with(&["border-color"], |x| x.border_color = Some(c))
            } else {
                return Some(bad());
            }
        }
        "bg" => {
            if neg {
                return no_neg();
            }
            if let Some(dir) = v.strip_prefix("gradient-to-") {
                let Some(angle) = value::side_angle(dir) else { return Some(bad()) };
                return Some(Ok(Rule::with(&["--tw-gradient-position", "background-image"], |x| x.bg_image = Some(BgImage::Tailwind { angle }))));
            }
            if let Some(a) = arb(v) {
                if a.starts_with("linear-gradient(") {
                    let Some((angle, stops)) = value::linear_gradient(&a) else { return Some(bad()) };
                    return Some(Ok(Rule::with(&["background-image"], |x| x.bg_image = Some(BgImage::Linear { angle, stops: Cow::Owned(stops) }))));
                }
            }
            let Some(c) = color_value(v) else { return Some(bad()) };
            Rule::with(&["background-color"], |x| x.bg = Some(c))
        }
        "from" | "via" | "to" => {
            let Some(c) = color_value(v) else { return Some(bad()) };
            // Tailwind sorts these by a `--tw-sort` on their own stop property.
            let (sort, count, i) = match root {
                "from" => ("--tw-gradient-from", 3, 0),
                "via" => ("--tw-gradient-via", 4, 1),
                _ => ("--tw-gradient-to", 3, 2),
            };
            let mut r = Rule::new(&[sort], count);
            r.decls.grad[i] = Some(c);
            r
        }
        "text" => {
            if neg {
                return no_neg();
            }
            if let Some(t) = tokens::text(v) {
                let mut props = vec!["font-size"];
                if t.line.is_some() {
                    props.push("line-height");
                }
                if t.tracking.is_some() {
                    props.push("letter-spacing");
                }
                Rule::with(&props, |x| {
                    x.font_size = Some(t.size);
                    x.size_line_height = t.line;
                    x.size_tracking = t.tracking;
                })
            } else if let Some(l) = arb(v).as_deref().and_then(value::len) {
                Rule::with(&["font-size"], |x| x.font_size = Some(l))
            } else if let Some(c) = color_value(v) {
                Rule::with(&["color"], |x| x.color = Some(c))
            } else {
                return Some(bad());
            }
        }
        "font" => {
            if let Some(w) = tokens::weight(v) {
                Rule::with(&["--tw-font-weight", "font-weight"], |x| x.weight = Some(w))
            } else if let Some(f) = tokens::family(v) {
                Rule::with(&["font-family"], |x| x.family = Some(f))
            } else {
                return Some(bad());
            }
        }
        "leading" => {
            let lh = match v {
                "none" => Some(LineHeight::Num(1.0)),
                _ => tokens::leading(v).map(LineHeight::Num).or_else(|| {
                    if let Some(a) = arb(v) {
                        value::num(&a).map(LineHeight::Num).or_else(|| value::len(&a).map(LineHeight::Len))
                    } else {
                        spacing(v).map(LineHeight::Len)
                    }
                }),
            };
            let Some(lh) = lh else { return Some(bad()) };
            Rule::with(&["--tw-leading", "line-height"], |x| x.line_height = Some(lh))
        }
        "tracking" => {
            let Some(t) = tokens::tracking(v).or_else(|| arb(v).as_deref().and_then(value::len)) else { return Some(bad()) };
            Rule::with(&["--tw-tracking", "letter-spacing"], |x| x.tracking = Some(negok(t)))
        }
        "line-clamp" => {
            let Some(n) = v.parse::<u16>().ok().filter(|n| *n > 0) else { return Some(bad()) };
            Rule::with(&["overflow", "display", "-webkit-box-orient", "-webkit-line-clamp"], |x| {
                x.overflow = [Some(Overflow::Hidden); 2];
                x.display = Some(Display::LineClamp);
                x.line_clamp = Some(n);
            })
        }
        "shadow" => {
            if let Some(a) = arb(v) {
                if let Some(layers) = value::shadows(&a) {
                    return Some(Ok(Rule::with(&["--tw-shadow", "box-shadow"], |x| x.shadow = Some(Cow::Owned(layers)))));
                }
            }
            // Shadow colour (`shadow-black/30`) recolours the shadow layers.
            let Some(c) = color_value(v) else {
                return Some(Err(format!("`shadow-{v}`: DESIGN.md has no card shadows; the web launcher only uses shadow-xl, shadow-2xl, shadow-[...] and shadow-<colour>")));
            };
            Rule::with(&["--tw-shadow-color"], |x| x.shadow_color = Some(c))
        }
        "ring" => {
            if let Some(w) = if v.is_empty() { Some(1) } else { v.parse::<u16>().ok() } {
                Rule::with(&["--tw-ring-shadow", "box-shadow"], |x| x.ring = Some(Len::Px(w as f32)))
            } else if let Some(c) = color_value(v) {
                Rule::with(&["--tw-ring-color"], |x| x.ring_color = Some(c))
            } else {
                return Some(bad());
            }
        }
        "outline" => {
            if let Some(w) = if v.is_empty() { Some(1) } else { v.parse::<u16>().ok() } {
                Rule::with(&["outline-style", "outline-width"], |x| {
                    x.outline_style = Some(OutlineStyle::Solid);
                    x.outline_width = Some(Len::Px(w as f32));
                })
            } else if let Some(c) = color_value(v) {
                Rule::with(&["outline-color"], |x| x.outline_color = Some(c))
            } else {
                return Some(bad());
            }
        }
        "outline-offset" => {
            let Ok(n) = v.parse::<u16>() else { return Some(bad()) };
            Rule::with(&["outline-offset"], |x| x.outline_offset = Some(negok(Len::Px(n as f32))))
        }
        "drop-shadow" => {
            let Some(s) = arb(v).as_deref().and_then(value::shadow) else { return Some(bad()) };
            Rule::with(&["--tw-drop-shadow-size", "--tw-drop-shadow", "filter"], |x| x.drop_shadow = Some(s))
        }
        "translate-x" | "translate-y" => {
            let Some(l) = size_value(v, false, true, true) else { return Some(bad()) };
            let l = negok(l);
            let i = if root == "translate-x" { 0 } else { 1 };
            Rule::with(&[if i == 0 { "--tw-translate-x" } else { "--tw-translate-y" }, "translate"], |x| x.translate[i] = Some(l))
        }
        "scale" => {
            let Some(s) = arb(v).as_deref().and_then(value::num) else { return Some(bad()) };
            Rule::with(&["scale"], |x| x.scale = Some(if neg { -s } else { s }))
        }
        "rotate" => {
            let Ok(n) = v.parse::<u16>() else { return Some(bad()) };
            Rule::with(&["rotate"], |x| x.rotate = Some(if neg { -(n as f32) } else { n as f32 }))
        }
        "transition" => {
            let Some(a) = arb(v) else { return Some(bad()) };
            let props: Option<Vec<TProp>> = a.split(',').map(|p| TProp::from_css(p.trim())).collect();
            let Some(props) = props else { return Some(bad()) };
            transition_rule(&props)
        }
        "duration" => {
            let ms = arb(v).as_deref().and_then(value::time_ms).or_else(|| v.parse::<u16>().ok().map(|n| n as f32));
            let Some(ms) = ms else { return Some(bad()) };
            Rule::with(&["--tw-duration", "transition-duration"], |x| x.duration = Some(ms))
        }
        "ease" => {
            let e = arb(v).as_deref().and_then(value::ease).or_else(|| tokens::ease(v));
            let Some(e) = e else { return Some(bad()) };
            Rule::with(&["--tw-ease", "transition-timing-function"], |x| x.ease = Some(e))
        }
        "object" => {
            let p = match arb(v) {
                Some(a) => value::position(&a),
                None => value::position(&v.replace('-', " ")),
            };
            let Some(p) = p else { return Some(bad()) };
            Rule::with(&["object-position"], |x| x.object_position = Some(p))
        }
        "divide" => {
            let Some(c) = color_value(v) else { return Some(bad()) };
            let mut r = Rule::new(&["divide-color"], 2);
            let mut d = Decls::NONE;
            d.border_color = Some(c);
            r.extra.push((When::ChildrenNotLast, d));
            r
        }
        _ => return None,
    };
    if neg && !matches!(root, "z" | "space-x" | "space-y" | "translate-x" | "translate-y" | "scale" | "rotate" | "tracking" | "outline-offset") {
        return no_neg();
    }
    Some(Ok(r))
}

/// `minmax(0,1fr)_6rem_auto`, `repeat(auto-fill,minmax(9rem,1fr))` (already decoded: spaces).
fn tracks(s: &str) -> Option<Vec<Track>> {
    value::words(s).iter().map(|w| track(w)).collect()
}

fn track(w: &str) -> Option<Track> {
    if let Some(inner) = w.strip_prefix("minmax(").and_then(|x| x.strip_suffix(')')) {
        let p = value::split_top(inner, ',');
        if p.len() != 2 {
            return None;
        }
        return Some(Track::MinMax(sizing(&p[0])?, sizing(&p[1])?));
    }
    if let Some(inner) = w.strip_prefix("repeat(").and_then(|x| x.strip_suffix(')')) {
        let p = value::split_top(inner, ',');
        if p.len() != 2 {
            return None;
        }
        let t: Vec<Track> = value::words(&p[1]).iter().map(|x| track(x)).collect::<Option<_>>()?;
        return Some(match p[0].as_str() {
            "auto-fill" => Track::AutoFill(Cow::Owned(t)),
            n => Track::Repeat(n.parse().ok()?, Cow::Owned(t)),
        });
    }
    Some(Track::One(sizing(w)?))
}

fn sizing(s: &str) -> Option<Sizing> {
    let s = s.trim();
    if s == "auto" {
        return Some(Sizing::Auto);
    }
    if let Some(f) = s.strip_suffix("fr") {
        return value::num(f).map(Sizing::Fr);
    }
    value::len(s).map(Sizing::Len)
}
