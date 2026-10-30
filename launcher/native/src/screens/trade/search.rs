//! Coin search (trade/CoinSearch.tsx, search.ts): results as the Trade API's coin rows, recent searches when
//! the box is empty. The launcher's Search (WS 12) places `coin_results` in its page; the swap
//! picker uses the same rows. Tags come from the caller (`tag(i)`), so it fits both the app's
//! `Acts` and a self-hosted page's constants.

use super::cards::token_list_row;
use super::fmt::normalize_type;
use super::model::TokenRow;
use crate::kit::section_head;
use crate::screens::market::host::*;
use rare_render::view::El;

/// A coin searched before (search.ts Recent), kept on the device, newest first, at most 8.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recent {
    pub coin_type: String,
    pub symbol: String,
    pub name: String,
    pub logo: Option<String>,
}

/// What the results show for the box's text.
#[derive(Clone, Debug, Default)]
pub struct Results {
    pub q: String,
    /// None while the search is running.
    pub rows: Option<Vec<TokenRow>>,
    pub error: bool,
    pub recents: Vec<Recent>,
}

/// search.ts `searchPath`.
pub fn search_path(q: &str) -> String {
    let t: String = q.trim().chars().take(160).collect();
    format!("/search?q={}", urlencode(&t))
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Pasted text that names a coin: a full type (quotes stripped), normalised.
pub fn pasted_type(q: &str) -> Option<String> {
    let t = q.trim().trim_matches(|c| c == '"' || c == '\'' || c == '`');
    let mut parts = t.split("::");
    let (a, m, n) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let hex = a.strip_prefix("0x")?;
    let word = |w: &str| !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    (!hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()) && word(m) && word(n)).then(|| normalize_type(t))
}

/// Puts a coin at the front of the recents (search.ts rememberCoin).
pub fn remember(recents: &mut Vec<Recent>, t: &TokenRow) {
    let ty = normalize_type(&t.coin_type);
    recents.retain(|r| normalize_type(&r.coin_type) != ty);
    recents.insert(0, Recent { coin_type: t.coin_type.clone(), symbol: t.symbol.clone(), name: t.name.clone(), logo: t.logo.clone() });
    recents.truncate(8);
}

/// CoinResults: "Recent" rows for an empty box, else "Coins" with a count, rows, or a calm line.
/// `first` gives the first row `data-nav-default`; `group` names the rows' nav group.
pub fn coin_results(ui: &mut Ui, r: &Results, group: &str, limit: usize, first: bool, tag: &mut dyn FnMut(usize) -> u32) -> Option<El> {
    if r.q.trim().is_empty() {
        if r.recents.is_empty() {
            return None;
        }
        let mut rows = ui.bx(s().col()).group(crate::kit::group(group, false));
        for (i, rc) in r.recents.iter().enumerate() {
            let t = TokenRow { coin_type: rc.coin_type.clone(), symbol: rc.symbol.clone(), name: rc.name.clone(), decimals: 9, logo: rc.logo.clone(), ..Default::default() };
            rows = rows.child(token_list_row(ui, &t, Some(rc.name.clone()), tag(i), first && i == 0).key(rare_render::view::key(&rc.coin_type)));
        }
        return Some(ui.bx(s().col()).section().child(section_head(ui, s().mb(0.5), "Recent", None, None)).child(rows));
    }
    let shown: Vec<&TokenRow> = r.rows.as_ref().map(|v| v.iter().take(limit).collect()).unwrap_or_default();
    let note = (!shown.is_empty()).then(|| shown.len().to_string());
    let mut sec = ui.bx(s().col()).section().child(section_head(ui, s().mb(0.5), "Coins", note.as_deref(), None));
    if shown.is_empty() {
        let msg = match (&r.rows, r.error) {
            (None, true) => "Could not search coins right now.",
            (None, false) => "Searching",
            (Some(_), _) => "No coin matches. Paste a full coin type to open one that is not listed.",
        };
        sec = sec.child(ui.text_s(s().py(1.0), msg, T::new(Ty::Small).c(tok::MUTED)));
    } else {
        let mut rows = ui.bx(s().col()).group(crate::kit::group(group, false));
        for (i, t) in shown.iter().enumerate() {
            rows = rows.child(token_list_row(ui, t, None, tag(i), first && i == 0).key(rare_render::view::key(&t.coin_type)));
        }
        sec = sec.child(rows);
    }
    Some(sec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasted_types_and_paths() {
        assert_eq!(pasted_type("0x2::sui::SUI").as_deref(), Some("0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI"));
        assert_eq!(pasted_type(" \"0xdeeb::deep::DEEP\" ").as_deref(), Some("0x000000000000000000000000000000000000000000000000000000000000deeb::deep::DEEP"));
        assert_eq!(pasted_type("deep"), None);
        assert_eq!(pasted_type("0x2::sui"), None);
        assert_eq!(search_path("  sui token "), "/search?q=sui%20token");
        let mut rec = Vec::new();
        for i in 0..10 {
            remember(&mut rec, &TokenRow { coin_type: format!("0x{i}::a::A"), symbol: format!("A{i}"), ..Default::default() });
        }
        remember(&mut rec, &TokenRow { coin_type: "0x5::a::A".into(), symbol: "A5".into(), ..Default::default() });
        assert_eq!(rec.len(), 8);
        assert_eq!(rec[0].symbol, "A5");
    }
}
