//! Every class string in launcher/src parses, and every class means what the web launcher's real
//! stylesheet makes it mean (corpus/computed.tsv, recorded from headless Chromium by
//! tools/oracle.mjs): each class, each class string, and the states (`first:`, `last:`,
//! `focus-visible:`) and child rules (`space-*`, `divide-*`) of the strings that use them. Regenerate the corpus after changing launcher/src or app.css:
//!   node launcher/crates/rare-tw/tools/extract.mjs && node launcher/crates/rare-tw/tools/oracle.mjs

use rare_tw_core::{css, parse, parse_class, Decls, When};
use std::collections::BTreeMap;

const CORPUS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../corpus/");

fn tsv(name: &str) -> Vec<Vec<String>> {
    std::fs::read_to_string(format!("{CORPUS}{name}"))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split('\t').map(str::to_string).collect())
        .collect()
}

#[test]
fn every_class_string_parses() {
    let rows = tsv("strings.tsv");
    assert!(rows.len() > 1500, "corpus too small: {}", rows.len());
    let mut bad = Vec::new();
    for r in &rows {
        if let Err(errs) = parse(&r[2]) {
            for e in errs {
                bad.push(format!("{} ({}): {e}", r[2], r[1]));
            }
        }
    }
    assert!(bad.is_empty(), "{} class strings fail:\n{}", bad.len(), bad.join("\n"));
}

#[test]
fn every_class_sorts_like_tailwind() {
    let mut bad = Vec::new();
    for r in tsv("classes.tsv") {
        let (class, sort) = (&r[1], &r[2]);
        let p = match parse_class(class) {
            Ok(p) => p,
            Err(e) => {
                bad.push(format!("{class}: {e}"));
                continue;
            }
        };
        if sort == "-" || p.custom.is_some() {
            continue;
        }
        let (order, count) = sort.split_once(';').unwrap();
        let order: Vec<u16> = order.split(',').filter(|s| !s.is_empty()).map(|s| s.parse().unwrap()).collect();
        let count: u16 = count.parse().unwrap();
        if p.rule.order != order || p.rule.count != count {
            bad.push(format!("{class}: rust {:?};{} tailwind {:?};{}", p.rule.order, p.rule.count, order, count));
        }
    }
    assert!(bad.is_empty(), "{} classes:\n{}", bad.len(), bad.join("\n"));
}

/// Chrome's value vs ours: same text between numbers, numbers within rounding.
fn same(a: &str, b: &str) -> bool {
    fn toks(s: &str) -> Vec<Result<f64, String>> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut num = String::new();
        let cs: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < cs.len() {
            let c = cs[i];
            let starts_num = c.is_ascii_digit() || ((c == '-' || c == '.') && cs.get(i + 1).is_some_and(|d| d.is_ascii_digit() || *d == '.'));
            if starts_num && (num.is_empty()) && !cur.ends_with(|p: char| p.is_ascii_alphabetic()) {
                if !cur.is_empty() {
                    out.push(Err(std::mem::take(&mut cur)));
                }
                num.push(c);
                i += 1;
                while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.' || ((cs[i] == 'e') && cs.get(i + 1).is_some_and(|d| *d == '+' || *d == '-')) || ((cs[i] == '+' || cs[i] == '-') && cs[i - 1] == 'e')) {
                    num.push(cs[i]);
                    i += 1;
                }
                out.push(Ok(num.parse().unwrap_or(f64::NAN)));
                num.clear();
                continue;
            }
            cur.push(c);
            i += 1;
        }
        if !cur.is_empty() {
            out.push(Err(cur));
        }
        out
    }
    let (ta, tb) = (toks(a), toks(b));
    ta.len() == tb.len()
        && ta.iter().zip(&tb).all(|(x, y)| match (x, y) {
            (Ok(x), Ok(y)) => {
                let tol = if x.abs() <= 1.0 && y.abs() <= 1.0 { 0.006 } else { 0.6f64.max(x.abs() * 1e-4) };
                (x - y).abs() <= tol
            }
            (Err(x), Err(y)) => x.trim() == y.trim(),
            _ => false,
        })
}

fn normalise(prop: &str, v: &str) -> String {
    match prop {
        // Chrome keeps the ratio's two numbers.
        "aspect-ratio" => match v.split_once(" / ") {
            Some((a, b)) => format!("{}", (a.parse::<f64>().unwrap() / b.parse::<f64>().unwrap() * 10000.0).round() / 10000.0),
            None => v.to_string(),
        },
        // Tailwind's box-shadow lists its empty inset/ring/offset layers.
        "box-shadow" => {
            let empty = "rgba(0.00,0.00,0.00,0.0000) 0px 0px 0px 0px";
            let layers: Vec<&str> = v.split(", rgba").collect();
            let layers: Vec<String> = layers.iter().enumerate().map(|(i, l)| if i == 0 { l.to_string() } else { format!("rgba{l}") }).filter(|l| l != empty).collect();
            if layers.is_empty() {
                "none".into()
            } else {
                layers.join(", ")
            }
        }
        "font-family" => v.replace('"', ""),
        _ => v.to_string(),
    }
}

/// Properties our model does not carry (paint hints) or that an animation drives at t = 0.
const UNTRACKED: &[&str] = &["content-visibility"];
const ANIMATED: &[&str] = &["opacity", "translate", "scale", "background-color"];

#[test]
fn every_class_and_string_computes_like_chrome() {
    let rows = tsv("computed.tsv");
    let base: BTreeMap<String, String> = rows[0][2].split('|').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect();
    assert_eq!(rows[0][0], "base");
    let mut bad = Vec::new();
    let mut checked = 0;
    for r in &rows[1..] {
        let (kind, classes) = (&r[0], &r[1]);
        let chrome: BTreeMap<String, String> = r.get(2).map(|s| s.as_str()).unwrap_or("").split('|').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let tw = match parse(classes) {
            Ok(t) => t,
            Err(e) => {
                bad.push(format!("{kind} `{classes}`: parse: {}", e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; ")));
                continue;
            }
        };
        checked += 1;
        let decls: Decls = match kind.as_str() {
            "first" => tw.with_state(&When::First),
            "last" => tw.with_state(&When::Last),
            "focus-visible" => tw.with_state(&When::FocusVisible),
            // A parent's first child against its last: only the not-last rule differs.
            "notlast" => tw.state(&When::ChildrenNotLast).cloned().unwrap_or(Decls::NONE),
            _ => tw.base.clone(),
        };
        let ours: BTreeMap<&str, String> = css::computed(&decls, 16.0).into_iter().collect();
        let animated = ours.contains_key("animation-name");
        let mut errs = Vec::new();
        for (p, v) in &chrome {
            // Chrome's own focus ring (`:focus-visible { outline: auto }`); the launcher turns it off
            // on every focusable ([data-nav] { outline: none }) and draws its ring instead.
            let ua_ring = kind == "focus-visible" && p.starts_with("outline-");
            if ua_ring || UNTRACKED.contains(&p.as_str()) || (animated && ANIMATED.contains(&p.as_str())) {
                continue;
            }
            match ours.get(p.as_str()) {
                Some(o) if same(&normalise(p, o), &normalise(p, v)) => {}
                Some(o) => errs.push(format!("{p}: chrome `{v}` ours `{o}`")),
                None => errs.push(format!("{p}: chrome `{v}` ours (unset)")),
            }
        }
        for (p, o) in &ours {
            let ua_ring = kind == "focus-visible" && p.starts_with("outline-");
            if ua_ring || chrome.contains_key(*p) || (animated && ANIMATED.contains(p)) {
                continue;
            }
            let b = base.get(*p).map(|s| s.as_str()).unwrap_or("?");
            if !same(&normalise(p, o), &normalise(p, b)) {
                errs.push(format!("{p}: chrome keeps `{b}` ours `{o}`"));
            }
        }
        if !errs.is_empty() {
            bad.push(format!("{kind} `{classes}`:\n    {}", errs.join("\n    ")));
        }
    }
    assert!(checked > 2700, "only {checked} rows checked");
    for kind in ["first", "last", "notlast", "focus-visible"] {
        assert!(rows.iter().any(|r| r[0] == kind), "no {kind} rows in computed.tsv");
    }
    assert!(bad.is_empty(), "{} of {} rows differ from Chrome:\n{}", bad.len(), rows.len() - 1, bad.join("\n"));
}
