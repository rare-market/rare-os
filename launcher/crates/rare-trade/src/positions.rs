//! The computation behind the Positions section, the home lists and the token rows (the web
//! launcher did it inline in PositionsSection.tsx, PerpsSection.tsx and parts.tsx): which coins
//! show, what opening one does, and the meta line under a coin's name.

use crate::amount::{normalize_type, SUI_TYPE};
use crate::api::{Holding, Portfolio, TokenRow};
use crate::format::age;
use crate::nav::{Side, View};
use crate::num::{num_parts, NumKind, NumOptions};

/// The wallet's coins worth showing: a cent or more, and SUI always.
pub fn visible_holdings(p: &Portfolio) -> Vec<&Holding> {
    p.holdings.iter().filter(|h| h.value_usd >= 0.01 || normalize_type(&h.coin_type) == SUI_TYPE).collect()
}

/// A coin row in Positions opens the coin to buy SUI-side, to sell anything else.
pub fn holding_view(h: &Holding) -> View {
    let sui = normalize_type(&h.coin_type) == SUI_TYPE;
    View::Token { coin_type: h.coin_type.clone(), side: Some(if sui { Side::Buy } else { Side::Sell }) }
}

/// The empty or error line under the perps positions.
pub fn positions_note(loaded: bool, failed: bool) -> &'static str {
    if loaded {
        "No open perps positions."
    } else if failed {
        "Could not load perps positions."
    } else {
        ""
    }
}

/// Market cap and age, the meta line the Trade API puts under a coin's name (the name when there is
/// neither).
pub fn token_meta(t: &TokenRow, with_age: bool, now_ms: f64) -> String {
    let mc = num_parts(NumKind::Market, t.mcap, NumOptions::default());
    let mut parts = Vec::new();
    if let Some(mc) = &mc {
        parts.push(format!("MC {}{}", mc.sign, mc.body));
    }
    if with_age {
        if let Some(c) = t.created_ms.filter(|c| *c != 0.0) {
            parts.push(format!("{} old", age(Some(c), now_ms)));
        }
    }
    if mc.is_none() && !with_age {
        parts.push(t.name.clone());
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(t: &str, v: f64) -> Holding {
        Holding { coin_type: t.into(), value_usd: v, ..Holding::default() }
    }

    #[test]
    fn coins_and_rows() {
        let p = Portfolio { holdings: vec![h("0x2::sui::SUI", 0.0), h("0xa::b::C", 0.009), h("0xa::d::E", 3.0)], ..Portfolio::default() };
        let v = visible_holdings(&p);
        assert_eq!(v.iter().map(|x| x.coin_type.as_str()).collect::<Vec<_>>(), ["0x2::sui::SUI", "0xa::d::E"]);
        assert_eq!(holding_view(v[0]), View::Token { coin_type: "0x2::sui::SUI".into(), side: Some(Side::Buy) });
        assert_eq!(holding_view(v[1]), View::Token { coin_type: "0xa::d::E".into(), side: Some(Side::Sell) });
        assert_eq!(positions_note(false, true), "Could not load perps positions.");
    }

    #[test]
    fn meta_line() {
        let now = 1_000_000_000_000.0;
        let t = TokenRow { name: "Meme".into(), mcap: Some(247_300.0), created_ms: Some(now - 3.0 * 86_400_000.0), ..TokenRow::default() };
        assert_eq!(token_meta(&t, true, now), "MC $247K, 3d old");
        assert_eq!(token_meta(&t, false, now), "MC $247K");
        let bare = TokenRow { name: "Meme".into(), ..TokenRow::default() };
        assert_eq!(token_meta(&bare, false, now), "Meme");
        assert_eq!(token_meta(&bare, true, now), "");
    }
}
