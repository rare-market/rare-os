//! corpus/COVERAGE.md: what launcher/src uses and what tw! does with it. Regenerated from the corpus
//! and the parser; fails while the committed report is stale (`RARE_TW_BLESS=1` rewrites it).

use rare_tw_core::{parse, parse_class, When};
use std::collections::BTreeMap;
use std::fmt::Write;

const CORPUS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../corpus/");

fn tsv(name: &str) -> Vec<Vec<String>> {
    std::fs::read_to_string(format!("{CORPUS}{name}"))
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split('\t').map(str::to_string).collect())
        .collect()
}

/// Families, longest prefix first.
const ROOTS: &[&str] = &[
    "grid-cols", "grid-rows", "col-span", "col-start", "row-start", "inset-x", "inset-y", "inset", "top", "right", "bottom", "left", "min-w", "min-h", "max-w", "max-h", "size",
    "space-x", "space-y", "gap-x", "gap-y", "gap", "rounded", "border", "divide", "bg-gradient", "bg", "from", "via", "to", "text", "font", "leading", "tracking", "line-clamp", "opacity",
    "shadow", "ring", "outline", "drop-shadow", "backdrop-blur", "translate", "scale", "rotate", "transition", "duration", "ease", "object", "aspect", "flex", "shrink", "grow", "items",
    "justify", "self", "overflow", "whitespace", "break", "select", "pointer-events", "z", "animate", "mix-blend", "w", "h", "m", "p",
];

fn family(class: &str) -> String {
    let p = parse_class(class).unwrap();
    if p.custom.is_some() {
        return "app.css component class".into();
    }
    let mut u = class;
    // strip the variant
    let mut depth = 0;
    let mut cut = None;
    for (i, c) in class.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            ':' if depth == 0 => cut = Some(i),
            _ => {}
        }
    }
    if let Some(i) = cut {
        u = &class[i + 1..];
    }
    let u = u.trim_start_matches(['!', '-']);
    if u.starts_with('[') {
        return "[property:value]".into();
    }
    for r in ROOTS {
        if u == *r || u.starts_with(&format!("{r}-")) {
            return match *r {
                "m" | "p" => "margin / padding".into(),
                "mx" => "margin / padding".into(),
                _ => r.to_string(),
            };
        }
    }
    // mt-, px-, pl-...
    if u.len() > 1 && (u.starts_with('m') || u.starts_with('p')) && u[1..].starts_with(['x', 'y', 't', 'r', 'b', 'l']) && u[2..].starts_with('-') {
        return "margin / padding".into();
    }
    match u {
        "block" | "inline" | "inline-block" | "flex" | "inline-flex" | "grid" | "contents" | "hidden" => "display".into(),
        "static" | "relative" | "absolute" | "fixed" | "sticky" => "position".into(),
        "visible" | "invisible" => "visibility".into(),
        "truncate" | "uppercase" | "underline" | "line-through" | "tabular-nums" => "text style".into(),
        "grayscale" => "filter".into(),
        _ => "other".into(),
    }
}

fn when_name(w: &When) -> String {
    match w {
        When::FocusVisible => "focus-visible:".into(),
        When::First => "first:".into(),
        When::Last => "last:".into(),
        When::Disabled => "disabled:".into(),
        When::Placeholder => "placeholder:".into(),
        When::ChildrenNotLast => "children but the last (space-*, divide-*)".into(),
        When::Children => "every child (.trow > *)".into(),
        When::Child(t) => format!("[&>{t}]:"),
        When::AncestorFocus { attr, value } => format!("[[{attr}={value}]:focus_&]:"),
    }
}

fn report() -> String {
    let meta = std::fs::read_to_string(format!("{CORPUS}extract.json")).unwrap();
    let num = |k: &str| -> String {
        let i = meta.find(&format!("\"{k}\": ")).unwrap() + k.len() + 4;
        meta[i..].split([',', '\n']).next().unwrap().trim().trim_matches('"').to_string()
    };
    let classes = tsv("classes.tsv");
    let strings = tsv("strings.tsv");
    let computed = tsv("computed.tsv");

    let mut fam: BTreeMap<String, (usize, usize, Vec<String>)> = BTreeMap::new();
    let mut states: BTreeMap<String, usize> = BTreeMap::new();
    for r in &classes {
        let uses: usize = r[0].parse().unwrap();
        let f = family(&r[1]);
        let e = fam.entry(f).or_default();
        e.0 += 1;
        e.1 += uses;
        if e.2.len() < 6 {
            e.2.push(r[1].clone());
        }
        let p = parse_class(&r[1]).unwrap();
        if let Some(w) = &p.when {
            *states.entry(when_name(w)).or_default() += uses;
        }
        for (w, _) in &p.rule.extra {
            *states.entry(when_name(w)).or_default() += uses;
        }
    }
    let total_uses: usize = classes.iter().map(|r| r[0].parse::<usize>().unwrap()).sum();
    let string_ok = strings.iter().filter(|r| parse(&r[2]).is_ok()).count();

    let mut s = String::new();
    let w = &mut s;
    writeln!(w, "# tw! coverage of launcher/src").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "Generated by `cargo test -p rare-tw-core --test coverage` from the corpus (tools/extract.mjs, tools/oracle.mjs). Do not edit.").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "| | |").unwrap();
    writeln!(w, "|---|---|").unwrap();
    writeln!(w, "| Source files scanned (launcher/src, .ts/.tsx, tests excluded) | {} |", num("files")).unwrap();
    writeln!(w, "| Class uses | {total_uses} |").unwrap();
    writeln!(w, "| Distinct classes | {} |", classes.len()).unwrap();
    writeln!(w, "| Distinct class strings | {} |", strings.len()).unwrap();
    writeln!(w, "| Class strings tw! parses | {string_ok} of {} |", strings.len()).unwrap();
    writeln!(w, "| Classes, strings, states and child rules whose computed CSS matches Chromium | {} rows (corpus/computed.tsv) |", computed.len() - 1).unwrap();
    writeln!(w, "| Classes glued to a template expression (`text-${{x}}`) | {} |", num("dynamic")).unwrap();
    writeln!(w, "| Tailwind | v{} (the launcher's own, the oracle for validity, rule order and CSS) |", num("tailwind")).unwrap();
    writeln!(w).unwrap();
    writeln!(w, "Every class in this table is supported; anything else is a compile error. Values follow the family (a new `w-[13rem]` works; `shadow-md`, `hover:`, `blur-*` or a palette colour like `bg-red-500` do not).").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "## By family").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "| Family | Classes | Uses | Examples |").unwrap();
    writeln!(w, "|---|---:|---:|---|").unwrap();
    let mut fv: Vec<_> = fam.into_iter().collect();
    fv.sort_by(|a, b| b.1 .1.cmp(&a.1 .1).then(a.0.cmp(&b.0)));
    for (f, (n, u, ex)) in &fv {
        writeln!(w, "| {f} | {n} | {u} | {} |", ex.iter().map(|e| format!("`{}`", e.replace('|', "\\|"))).collect::<Vec<_>>().join(" ")).unwrap();
    }
    writeln!(w).unwrap();
    writeln!(w, "## States and child rules").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "| State | Uses |").unwrap();
    writeln!(w, "|---|---:|").unwrap();
    for (k, v) in &states {
        writeln!(w, "| {k} | {v} |").unwrap();
    }
    writeln!(w).unwrap();
    writeln!(w, "## app.css component classes").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "Their plain declarations are read from app.css (unlayered, so they beat utilities, as in the browser); their focus behaviour is a mark the kit and focus system act on.").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "| Class | Uses | Declarations | Marks |").unwrap();
    writeln!(w, "|---|---:|---|---|").unwrap();
    for r in &classes {
        let p = parse_class(&r[1]).unwrap();
        if p.custom.is_none() {
            continue;
        }
        let mut fields = p.rule.decls.fields();
        for (wh, d) in &p.rule.extra {
            fields.push(Box::leak(format!("{}: {}", when_name(wh), d.fields().join(" ")).into_boxed_str()));
        }
        writeln!(w, "| `{}` | {} | {} | {} |", r[1], r[0], if fields.is_empty() { "-".into() } else { fields.join(", ") }, if p.rule.marks.0 == 0 { "-".into() } else { format!("{:#x}", p.rule.marks.0) }).unwrap();
    }
    writeln!(w).unwrap();
    writeln!(w, "## Outside DESIGN.md, used by the web launcher").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "Supported because the web launcher uses them (parity); each is a candidate for the design review, not a pattern to copy.").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "| Class | Uses | First use |").unwrap();
    writeln!(w, "|---|---:|---|").unwrap();
    let first_use = |c: &str| strings.iter().find(|r| r[2].split(' ').any(|x| x == c)).map(|r| r[1].clone()).unwrap_or_default();
    for r in &classes {
        let c = &r[1];
        let u = if c.starts_with('[') && !c.contains("]:") { c.as_str() } else { c.rsplit(':').next().unwrap_or(c) };
        if u.starts_with("shadow-") && !u.starts_with("shadow-black")
            || u.starts_with("backdrop-blur")
            || u.starts_with("drop-shadow")
            || u.starts_with("[text-shadow")
            || u.starts_with("mix-blend")
            || u == "grayscale"
            || u == "animate-pulse"
            || u.starts_with("[mask-")
        {
            writeln!(w, "| `{c}` | {} | {} |", r[0], first_use(c)).unwrap();
        }
    }
    writeln!(w).unwrap();
    writeln!(w, "## Not classes").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "Words Tailwind's scanner sees in launcher/src that are not used as classes (prose, identifiers): {}.", {
        let i = meta.find("\"scannerUtilitiesNotInCorpus\": [").unwrap();
        let rest = &meta[i..];
        let list = &rest[rest.find('[').unwrap() + 1..rest.find(']').unwrap()];
        list.split(',').map(|x| format!("`{}`", x.trim().trim_matches('"'))).filter(|x| x != "``").collect::<Vec<_>>().join(", ")
    })
    .unwrap();
    writeln!(w).unwrap();
    writeln!(w, "## Top 40 classes").unwrap();
    writeln!(w).unwrap();
    writeln!(w, "| Class | Uses |").unwrap();
    writeln!(w, "|---|---:|").unwrap();
    for r in classes.iter().take(40) {
        writeln!(w, "| `{}` | {} |", r[1], r[0]).unwrap();
    }
    s
}

#[test]
fn coverage_report_is_current() {
    let want = report();
    let path = format!("{CORPUS}COVERAGE.md");
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    if have != want {
        if std::env::var_os("RARE_TW_BLESS").is_some() {
            std::fs::write(&path, &want).unwrap();
            return;
        }
        panic!("{path} is stale. Run\n  RARE_TW_BLESS=1 cargo test --manifest-path launcher/crates/Cargo.toml -p rare-tw-core --test coverage\n");
    }
}
