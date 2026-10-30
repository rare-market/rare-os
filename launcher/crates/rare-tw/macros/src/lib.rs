//! `tw!("flex items-end gap-[2rem] px-[2.5rem]")`: a web launcher class string, parsed at compile
//! time by rare-tw-core into a `&'static rare_tw::Tw`. A class outside the subset launcher/src uses
//! is a compile error pointing at the string.

use proc_macro::{Delimiter, Literal, Span, TokenStream, TokenTree};
use rare_tw_core::emit::Emit;

#[proc_macro]
pub fn tw(input: TokenStream) -> TokenStream {
    match literal(input) {
        Ok((src, span)) => match rare_tw_core::parse(&src) {
            Ok(tw) => format!("{{ static TW: ::rare_tw::Tw = {}; &TW }}", tw.emit("::rare_tw")).parse().expect("rare-tw: generated code"),
            Err(errs) => {
                let msg = errs.iter().map(|e| format!("rare-tw: unsupported class {e}")).collect::<Vec<_>>().join("\n");
                error(&msg, span)
            }
        },
        Err((msg, span)) => error(&msg, span),
    }
}

fn error(msg: &str, span: Span) -> TokenStream {
    // compile_error!("...") with the literal's span.
    let mut lit = Literal::string(msg);
    lit.set_span(span);
    let mut ts: TokenStream = "::core::compile_error!".parse().unwrap();
    let mut args = proc_macro::Group::new(Delimiter::Parenthesis, TokenStream::from(TokenTree::Literal(lit)));
    args.set_span(span);
    ts.extend([TokenTree::Group(args)]);
    let mut out = TokenStream::new();
    out.extend([TokenTree::Group(proc_macro::Group::new(Delimiter::Brace, ts))]);
    out
}

/// The one string literal argument (possibly wrapped in an invisible group by macro_rules).
fn literal(input: TokenStream) -> Result<(String, Span), (String, Span)> {
    let toks: Vec<TokenTree> = input.into_iter().collect();
    match toks.as_slice() {
        [TokenTree::Group(g)] if g.delimiter() == Delimiter::None => literal(g.stream()),
        [TokenTree::Literal(l)] => {
            let span = l.span();
            unquote(&l.to_string()).map(|s| (s, span)).ok_or_else(|| ("tw! takes one string literal of Tailwind classes".to_string(), span))
        }
        [t, ..] => Err(("tw! takes one string literal of Tailwind classes".into(), t.span())),
        [] => Err(("tw! takes one string literal of Tailwind classes".into(), Span::call_site())),
    }
}

/// The value of a Rust string literal: "..." with escapes, or r#"..."#.
fn unquote(s: &str) -> Option<String> {
    if let Some(r) = s.strip_prefix('r') {
        let hashes = r.chars().take_while(|c| *c == '#').count();
        let body = r[hashes..].strip_prefix('"')?;
        return Some(body[..body.len().checked_sub(1 + hashes)?].to_string());
    }
    let body = s.strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::new();
    let mut it = body.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next()? {
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            '\'' => out.push('\''),
            '0' => out.push('\0'),
            '\n' => {
                // line continuation: skip leading whitespace on the next line
                let rest: String = it.clone().collect();
                let skip = rest.len() - rest.trim_start().len();
                for _ in 0..rest[..skip].chars().count() {
                    it.next();
                }
            }
            _ => return None,
        }
    }
    Some(out)
}
