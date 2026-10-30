//! Markup for the Phase 3 screens: the TSX trees of launcher/src, one to one, as plain Rust data.
//!
//! A screen is `view(&props, &state) -> El<Msg>`: the same elements, the same Tailwind class
//! strings (verbatim, so `tw!` or the kit can take them over unchanged), the same text, and the
//! same `data-nav` focusables with the message A sends. Nothing here draws; the renderer maps an
//! `El` onto the engine's scene through the kit (workstream 12). Keeping the tree as data lets the
//! tests pin exactly what each state shows (the wallet's review sheet must show what the TS one
//! shows, word for word) before any pixel exists, and keeps the screens independent of the
//! kit's API while it settles.

use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum Tag {
    Div,
    Span,
    P,
    H1,
    H2,
    H3,
    Section,
    Ul,
    Ol,
    Li,
    Pre,
    Code,
    /// `<button data-nav>`: focusable, A sends `Nav::on`.
    Button,
    /// `<img>` (or `MImg` / `Avatar` art): `src`, never a broken image (empty src shows the fallback).
    Img { src: Option<String>, alt: String },
    /// A lucide icon by name (`check`, `chevron-down`, `x`...), sized by its class.
    Icon(&'static str),
    /// A QR code of `text` (BuySheet's `Qr`).
    Qr(String),
    /// The identicon avatar for an address (`WalletAvatar`).
    WalletAvatar(String),
    /// A controller glyph (`Glyph`): a, b, x, y, lb, rb, start...
    Glyph(&'static str),
    /// Another workstream's view mounted here (the overlay's item detail, the Marketplace's buy
    /// sheet): the renderer asks the host for the element by this name.
    Slot(String),
}

impl Tag {
    fn name(&self) -> String {
        match self {
            Tag::Div => "div".into(),
            Tag::Span => "span".into(),
            Tag::P => "p".into(),
            Tag::H1 => "h1".into(),
            Tag::H2 => "h2".into(),
            Tag::H3 => "h3".into(),
            Tag::Section => "section".into(),
            Tag::Ul => "ul".into(),
            Tag::Ol => "ol".into(),
            Tag::Li => "li".into(),
            Tag::Pre => "pre".into(),
            Tag::Code => "code".into(),
            Tag::Button => "button".into(),
            Tag::Img { src, .. } => format!("img src={}", src.as_deref().unwrap_or("-")),
            Tag::Icon(n) => format!("icon {n}"),
            Tag::Qr(t) => format!("qr {t}"),
            Tag::WalletAvatar(a) => format!("wallet-avatar {a}"),
            Tag::Glyph(g) => format!("glyph {g}"),
            Tag::Slot(n) => format!("slot {n}"),
        }
    }
}

/// A focusable's behaviour (`data-nav`, `data-nav-default`, `data-hint-a`, `disabled`).
#[derive(Clone, Debug, PartialEq)]
pub struct Nav<M> {
    pub on: Option<M>,
    pub default: bool,
    pub disabled: bool,
    pub hint_a: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct El<M> {
    pub tag: Tag,
    pub class: String,
    pub text: Option<String>,
    pub kids: Vec<El<M>>,
    pub key: Option<String>,
    pub nav: Option<Nav<M>>,
    /// role, aria-label, data-testid, style overrides (`max-width`, `width`...).
    pub attrs: Vec<(&'static str, String)>,
    /// `usePadHandler` in this subtree: pad button ("b", "x", "y", "start", "lt", "rt") to message.
    pub pads: Vec<(&'static str, M)>,
}

pub fn el<M>(tag: Tag, class: impl Into<String>) -> El<M> {
    El { tag, class: class.into(), text: None, kids: Vec::new(), key: None, nav: None, attrs: Vec::new(), pads: Vec::new() }
}
pub fn div<M>(class: impl Into<String>) -> El<M> {
    el(Tag::Div, class)
}
pub fn span<M>(class: impl Into<String>) -> El<M> {
    el(Tag::Span, class)
}
pub fn p<M>(class: impl Into<String>) -> El<M> {
    el(Tag::P, class)
}
pub fn h1<M>(class: impl Into<String>) -> El<M> {
    el(Tag::H1, class)
}
pub fn h2<M>(class: impl Into<String>) -> El<M> {
    el(Tag::H2, class)
}
pub fn h3<M>(class: impl Into<String>) -> El<M> {
    el(Tag::H3, class)
}
pub fn section<M>(class: impl Into<String>) -> El<M> {
    el(Tag::Section, class)
}
pub fn ul<M>(class: impl Into<String>) -> El<M> {
    el(Tag::Ul, class)
}
pub fn ol<M>(class: impl Into<String>) -> El<M> {
    el(Tag::Ol, class)
}
pub fn li<M>(class: impl Into<String>) -> El<M> {
    el(Tag::Li, class)
}
pub fn pre<M>(class: impl Into<String>) -> El<M> {
    el(Tag::Pre, class)
}
pub fn code<M>(class: impl Into<String>) -> El<M> {
    el(Tag::Code, class)
}
pub fn icon<M>(name: &'static str, class: impl Into<String>) -> El<M> {
    el(Tag::Icon(name), class)
}
pub fn img<M>(src: Option<&str>, alt: &str, class: impl Into<String>) -> El<M> {
    el(Tag::Img { src: src.filter(|s| !s.is_empty()).map(str::to_string), alt: alt.to_string() }, class)
}
pub fn glyph<M>(b: &'static str) -> El<M> {
    el(Tag::Glyph(b), "")
}
/// `<button data-nav>`: A sends `on`.
pub fn button<M>(class: impl Into<String>, on: Option<M>) -> El<M> {
    let mut e = el(Tag::Button, class);
    e.nav = Some(Nav { on, default: false, disabled: false, hint_a: None });
    e
}
/// A slot for another workstream's view (see `Tag::Slot`).
pub fn slot<M>(name: &str) -> El<M> {
    el(Tag::Slot(name.to_string()), "")
}

/// A plain text node (`{"..."}` between siblings).
pub fn text<M>(t: impl Into<String>) -> El<M> {
    let mut e = el(Tag::Span, "");
    e.text = Some(t.into());
    e
}

/// `cn(...)`: the class names that are on, joined by a space.
pub fn cn(parts: &[&str]) -> String {
    parts.iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" ")
}
/// `cond && "class"`.
pub fn when(cond: bool, class: &'static str) -> &'static str {
    if cond {
        class
    } else {
        ""
    }
}

impl<M> El<M> {
    pub fn text(mut self, t: impl Into<String>) -> Self {
        self.text = Some(t.into());
        self
    }
    pub fn kid(mut self, k: El<M>) -> Self {
        self.kids.push(k);
        self
    }
    pub fn kid_if(mut self, cond: bool, k: impl FnOnce() -> El<M>) -> Self {
        if cond {
            self.kids.push(k());
        }
        self
    }
    pub fn kid_opt(mut self, k: Option<El<M>>) -> Self {
        if let Some(k) = k {
            self.kids.push(k);
        }
        self
    }
    pub fn kids(mut self, ks: impl IntoIterator<Item = El<M>>) -> Self {
        self.kids.extend(ks);
        self
    }
    pub fn key(mut self, k: impl Into<String>) -> Self {
        self.key = Some(k.into());
        self
    }
    pub fn attr(mut self, k: &'static str, v: impl Into<String>) -> Self {
        self.attrs.push((k, v.into()));
        self
    }
    pub fn role(self, r: &str) -> Self {
        self.attr("role", r)
    }
    pub fn label(self, l: &str) -> Self {
        self.attr("aria-label", l)
    }
    pub fn class_add(mut self, c: &str) -> Self {
        if !c.is_empty() {
            if !self.class.is_empty() {
                self.class.push(' ');
            }
            self.class.push_str(c);
        }
        self
    }
    /// `data-nav-default`.
    pub fn def(mut self, on: bool) -> Self {
        if let Some(n) = self.nav.as_mut() {
            n.default = on;
        }
        self
    }
    pub fn disabled(mut self, on: bool) -> Self {
        if let Some(n) = self.nav.as_mut() {
            n.disabled = on;
        }
        self
    }
    pub fn hint_a(mut self, h: &str) -> Self {
        if let Some(n) = self.nav.as_mut() {
            n.hint_a = Some(h.to_string());
        }
        self
    }
    /// A pad button this subtree answers (B closes a sheet, X deletes on a pad).
    pub fn on_pad(mut self, b: &'static str, m: M) -> Self {
        self.pads.push((b, m));
        self
    }

    /// What `b` does here: the innermost handler wins, like nested `usePadHandler`s.
    pub fn pad(&self, b: &str) -> Option<&M> {
        let mut hit = None;
        self.walk_ref(&mut |e| {
            if let Some((_, m)) = e.pads.iter().find(|(x, _)| *x == b) {
                hit = Some(m);
            }
        });
        hit
    }

    pub fn get_attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(a, _)| *a == k).map(|(_, v)| v.as_str())
    }

    /// Maps the messages (a sheet embedded in a page, a panel in a sheet).
    pub fn map<N>(self, f: &impl Fn(M) -> N) -> El<N> {
        El {
            tag: self.tag,
            class: self.class,
            text: self.text,
            kids: self.kids.into_iter().map(|k| k.map(f)).collect(),
            key: self.key,
            nav: self.nav.map(|n| Nav { on: n.on.map(f), default: n.default, disabled: n.disabled, hint_a: n.hint_a }),
            attrs: self.attrs,
            pads: self.pads.into_iter().map(|(b, m)| (b, f(m))).collect(),
        }
    }

    /// Every text in document order, one entry per element that holds text.
    pub fn texts(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.walk(&mut |e| {
            if let Some(t) = &e.text {
                out.push(t.clone());
            }
        });
        out
    }

    /// The visible text of this element and its children, joined like `textContent`.
    pub fn content(&self) -> String {
        let mut s = String::new();
        self.walk(&mut |e| {
            if let Some(t) = &e.text {
                s.push_str(t);
            }
        });
        s
    }

    /// The focusables in document order: (their text, the nav).
    pub fn focusables(&self) -> Vec<(String, &Nav<M>)> {
        let mut out = Vec::new();
        self.walk_ref(&mut |e| {
            if let Some(n) = &e.nav {
                out.push((e.content(), n));
            }
        });
        out
    }

    /// The focusable whose text is `label` (tests: "press Approve").
    pub fn find_nav(&self, label: &str) -> Option<&Nav<M>> {
        self.focusables().into_iter().find(|(t, _)| t == label).map(|(_, n)| n)
    }

    /// Where focus lands first: the `data-nav-default` one, else the first enabled focusable.
    pub fn landing(&self) -> Option<String> {
        let f = self.focusables();
        f.iter().find(|(_, n)| n.default && !n.disabled).or_else(|| f.iter().find(|(_, n)| !n.disabled)).map(|(t, _)| t.clone())
    }

    pub fn find_by_attr(&self, k: &str, v: &str) -> Option<&El<M>> {
        if self.get_attr(k) == Some(v) {
            return Some(self);
        }
        self.kids.iter().find_map(|c| c.find_by_attr(k, v))
    }

    pub fn contains_text(&self, needle: &str) -> bool {
        self.texts().iter().any(|t| t.contains(needle))
    }

    fn walk(&self, f: &mut impl FnMut(&El<M>)) {
        f(self);
        for k in &self.kids {
            k.walk(f);
        }
    }

    fn walk_ref<'a>(&'a self, f: &mut impl FnMut(&'a El<M>)) {
        f(self);
        for k in &self.kids {
            k.walk_ref(f);
        }
    }

    /// An indented outline (tag, class, text, nav), for snapshot files and failing tests.
    pub fn dump(&self) -> String
    where
        M: std::fmt::Debug,
    {
        let mut s = String::new();
        self.dump_into(&mut s, 0);
        s
    }

    fn dump_into(&self, s: &mut String, depth: usize)
    where
        M: std::fmt::Debug,
    {
        let _ = write!(s, "{}<{}", "  ".repeat(depth), self.tag.name());
        if !self.class.is_empty() {
            let _ = write!(s, " class=\"{}\"", self.class);
        }
        for (k, v) in &self.attrs {
            let _ = write!(s, " {k}=\"{v}\"");
        }
        if let Some(n) = &self.nav {
            let _ = write!(s, " nav");
            if n.default {
                let _ = write!(s, " default");
            }
            if n.disabled {
                let _ = write!(s, " disabled");
            }
            if let Some(h) = &n.hint_a {
                let _ = write!(s, " hint-a=\"{h}\"");
            }
            if let Some(m) = &n.on {
                let _ = write!(s, " on={m:?}");
            }
        }
        for (b, m) in &self.pads {
            let _ = write!(s, " pad-{b}={m:?}");
        }
        s.push('>');
        if let Some(t) = &self.text {
            let _ = write!(s, " {t:?}");
        }
        s.push('\n');
        for k in &self.kids {
            k.dump_into(s, depth + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cn_drops_off_parts() {
        assert_eq!(cn(&["a", "", when(true, "b"), when(false, "c")]), "a b");
    }

    #[test]
    fn focus_lands_on_default_then_first() {
        let t: El<u8> = div("").kid(button("", Some(1)).text("One")).kid(button("", Some(2)).text("Two").def(true));
        assert_eq!(t.landing().as_deref(), Some("Two"));
        let t: El<u8> = div("").kid(button("", Some(1)).text("One").disabled(true)).kid(button("", Some(2)).text("Two"));
        assert_eq!(t.landing().as_deref(), Some("Two"));
    }
}
