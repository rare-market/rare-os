//! A numbered grid of recovery words (launcher/src/wallet/ui/onboard/WordGrid.tsx): read-only when
//! showing a new phrase, buttons when importing.

use crate::screens::wallet::markup::*;
use std::collections::BTreeSet;

pub struct Grid<'a> {
    pub words: &'a [String],
    pub cols: Option<usize>,
    /// cells are buttons (import)
    pub pick: bool,
    pub active: Option<usize>,
    pub bad: &'a BTreeSet<usize>,
    pub def_index: Option<usize>,
}

pub fn view<M>(g: &Grid, on: impl Fn(usize) -> M) -> El<M> {
    let c = g.cols.unwrap_or(if g.words.len() > 12 { 6 } else { 4 });
    ol("grid gap-2").attr("style", format!("grid-template-columns: repeat({c}, minmax(0, 1fr))")).kids(g.words.iter().enumerate().map(|(i, w)| {
        let cls = cn(&[
            "flex h-11 items-center gap-2.5 rounded border px-3 text-left",
            if g.bad.contains(&i) {
                "border-negative/60"
            } else if g.active == Some(i) {
                "border-border-light"
            } else {
                "border-border-primary"
            },
            "bg-layer-01",
        ]);
        let num = span("w-6 shrink-0 text-right text-xs tabular-nums text-secondary").text((i + 1).to_string());
        let word = span(cn(&["min-w-0 truncate font-mono text-base", when(w.is_empty(), "text-secondary")])).text(if w.is_empty() { "word" } else { w });
        let cell = if g.pick { button(cn(&[&cls, "w-full"]), Some(on(i))).def(g.def_index == Some(i)) } else { div(cls) };
        li("min-w-0").key(i.to_string()).kid(cell.kid(num).kid(word))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid() {
        let words: Vec<String> = vec!["abandon".into(), "".into()];
        let bad = BTreeSet::from([1]);
        let g: El<usize> = view(&Grid { words: &words, cols: None, pick: true, active: Some(0), bad: &bad, def_index: Some(0) }, |i| i);
        assert_eq!(g.texts(), ["1", "abandon", "2", "word"]);
        assert_eq!(g.landing().as_deref(), Some("1abandon"));
        assert!(g.get_attr("style").unwrap().contains("repeat(4"));
    }
}
