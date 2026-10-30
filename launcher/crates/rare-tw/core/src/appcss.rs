//! launcher/src/styles/app.css, compiled into this crate and read at parse time, so a token change
//! in the web launcher's stylesheet reaches the native screens at the next build (Cargo rebuilds
//! this crate, the tw! macro and every crate that uses it when the file changes).

use std::collections::HashMap;
use std::sync::OnceLock;

/// The web launcher's stylesheet, as committed.
pub const APP_CSS: &str = include_str!("../../../../src/styles/app.css");

/// What we read from app.css.
#[derive(Debug, Default)]
pub struct Sheet {
    /// `:root` custom properties (later blocks win), e.g. `background` -> `16 12 24`.
    pub root: HashMap<String, String>,
    /// `:root` plain declarations (font-size, color-scheme), in order.
    pub root_plain: Vec<(String, String)>,
    /// `@theme` custom properties, e.g. `color-bg` -> `rgb(var(--background))`, in source order.
    pub theme: Vec<(String, String)>,
    /// Top-level style rules (selector, body), outside @media/@keyframes, in source order.
    pub rules: Vec<(String, String)>,
}

pub fn sheet() -> &'static Sheet {
    static S: OnceLock<Sheet> = OnceLock::new();
    S.get_or_init(|| parse_sheet(APP_CSS))
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        match rest[i + 2..].find("*/") {
            Some(j) => rest = &rest[i + 2 + j + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Top-level `prelude { body }` blocks (bodies keep their nested blocks).
pub fn blocks(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let (mut i, mut start) = (0, 0);
    while i < b.len() {
        match b[i] {
            b'{' => {
                let pre = s[start..i].trim();
                // A prelude ends at the last ';' before the brace (skips `@import ...;`).
                let pre = pre.rsplit(';').next().unwrap_or(pre).trim().to_string();
                let mut depth = 1;
                let body_start = i + 1;
                i += 1;
                while i < b.len() && depth > 0 {
                    match b[i] {
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
                out.push((pre, s[body_start..i - 1].to_string()));
                start = i;
            }
            _ => i += 1,
        }
    }
    out
}

/// `prop: value;` pairs of a block body (nested blocks skipped).
pub fn decls(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    for ch in body.chars() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                cur.clear();
            }
            ';' if depth == 0 => {
                if let Some((k, v)) = cur.split_once(':') {
                    out.push((k.trim().to_string(), v.split_whitespace().collect::<Vec<_>>().join(" ")));
                }
                cur.clear();
            }
            _ if depth == 0 => cur.push(ch),
            _ => {}
        }
    }
    if let Some((k, v)) = cur.split_once(':') {
        if !k.trim().is_empty() {
            out.push((k.trim().to_string(), v.split_whitespace().collect::<Vec<_>>().join(" ")));
        }
    }
    out
}

pub fn parse_sheet(css: &str) -> Sheet {
    let css = strip_comments(css);
    let mut sh = Sheet::default();
    for (pre, body) in blocks(&css) {
        if pre == ":root" {
            for (k, v) in decls(&body) {
                match k.strip_prefix("--") {
                    Some(k) => {
                        sh.root.insert(k.to_string(), v);
                    }
                    None => sh.root_plain.push((k, v)),
                }
            }
        } else if pre == "@theme" {
            for (k, v) in decls(&body) {
                if let Some(k) = k.strip_prefix("--") {
                    sh.theme.push((k.to_string(), v));
                }
            }
        } else if !pre.starts_with('@') {
            sh.rules.push((pre.split_whitespace().collect::<Vec<_>>().join(" "), body));
        }
    }
    sh
}

impl Sheet {
    pub fn theme(&self, name: &str) -> Option<&str> {
        self.theme.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// The body of the first rule whose selector list contains `sel` exactly.
    pub fn rule(&self, sel: &str) -> Option<&str> {
        self.rules.iter().find(|(pre, _)| pre.split(',').any(|s| s.trim() == sel)).map(|(_, b)| b.as_str())
    }

    /// One declaration of that rule.
    pub fn decl(&self, sel: &str, prop: &str) -> Option<String> {
        decls(self.rule(sel)?).into_iter().rev().find(|(k, _)| k == prop).map(|(_, v)| v)
    }

    /// `var(--x)` (with fallback) resolved against :root, recursively.
    pub fn resolve_vars(&self, v: &str) -> String {
        let mut s = v.to_string();
        for _ in 0..8 {
            let Some(i) = s.find("var(--") else { break };
            let rest = &s[i + 4..];
            let mut depth = 1;
            let mut end = rest.len();
            for (j, c) in rest.char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = j;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let inner = &rest[..end];
            let (name, fallback) = match inner.split_once(',') {
                Some((n, f)) => (n.trim(), Some(f.trim())),
                None => (inner.trim(), None),
            };
            let val = self.root.get(name.trim_start_matches("--")).map(|x| x.as_str()).or(fallback).unwrap_or("");
            s = format!("{}{}{}", &s[..i], val, &rest[(end + 1).min(rest.len())..]);
        }
        s
    }
}
