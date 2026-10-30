//! One coin (launcher/src/wallet/ui/pages/Coin.tsx): the balance card with Send and Receive, and this
//! coin's recent activity.

use crate::screens::wallet::fmt::{fmt_token, fmt_usd, norm_type, to_precision};
use crate::screens::wallet::home::header::activity_row;
use crate::screens::wallet::home::lib::{hint, load_bar, Hint};
use crate::screens::wallet::home::page::{ActivityLoad, PortfolioLoad};
use crate::screens::wallet::home::stack::WalletView;
use crate::screens::wallet::kit::{avatar, btn_icon, delta, empty, page_with, panel, row, Size, Variant};
use crate::screens::wallet::markup::*;

pub struct Props<'a> {
    pub coin_type: &'a str,
    pub can_sign: bool,
    pub pf: &'a PortfolioLoad,
    pub act: &'a ActivityLoad,
    pub tz_min: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Push(WalletView),
}

pub fn view(pr: &Props) -> El<Msg> {
    let ty = norm_type(pr.coin_type);
    let t = pr.pf.data.as_ref().and_then(|d| d.tokens.iter().find(|x| norm_type(&x.coin_type) == ty));
    let symbol = t.map(|t| t.symbol.clone()).unwrap_or_else(|| pr.coin_type.rsplit("::").next().filter(|s| !s.is_empty()).unwrap_or("Coin").to_string());
    let recent: Vec<_> = pr.act.items.iter().filter(|a| a.changes.iter().any(|c| norm_type(&c.coin_type) == ty)).take(8).collect();

    let mut title = span("flex items-center gap-3").kid(avatar(t.and_then(|t| t.icon_url.as_deref()), &symbol, "2.25rem", false)).kid(text(&symbol));
    if let Some(n) = t.map(|t| &t.name).filter(|n| !n.is_empty()) {
        title = title.kid(span("text-base font-normal text-secondary").text(n));
    }

    let value = match t.and_then(|t| t.value_usd) {
        Some(v) => fmt_usd(Some(v)),
        None if pr.pf.data.is_some() => "No price".into(),
        None => "--".into(),
    };
    let mut card = vec![
        div("text-sm text-secondary").text("Your balance"),
        div("mt-1 text-4xl font-bold tabular-nums").kid(text(format!("{} ", fmt_token(Some(t.map(|t| t.amount).unwrap_or(0.0)), 6)))).kid(span("text-xl font-medium text-secondary").text(&symbol)),
        div("mt-1 text-lg tabular-nums text-secondary").text(value),
    ];
    if let Some((tok, price)) = t.and_then(|t| t.price_usd.map(|p| (t, p))) {
        card.push(
            div("mt-5 flex flex-col gap-2 border-t border-border-primary pt-4")
                .kid(row("Price", vec![span("tabular-nums").text(if price >= 1.0 { fmt_usd(Some(price)) } else { format!("${}", to_precision(price, 3)) })], "", true))
                .kid(row("24h", vec![delta(tok.change_24h, 2, "")], "", true)),
        );
    }
    let send = if pr.can_sign {
        btn_icon("arrow-up-right", "Send", Variant::Primary, Size::Lg, "flex-1", Some(Msg::Push(WalletView::Send { coin_type: Some(pr.coin_type.to_string()), to: None }))).def(true)
    } else {
        btn_icon("user-plus", "Add a signing account", Variant::Primary, Size::Lg, "flex-1", Some(Msg::Push(WalletView::Add))).def(true)
    };
    let left = div("flex flex-col gap-4 self-start")
        .kid(panel(None, None, "p-6", card))
        .kid(div("flex gap-2").kid(send).kid(btn_icon("arrow-down-left", "Receive", Variant::Secondary, Size::Lg, "flex-1", Some(Msg::Push(WalletView::Receive)))));
    let mut right = div("min-w-0")
        .kid(h2("mb-2 text-xl font-bold").text("Recent activity"))
        .kid(div("").attr("data-nav-group", "wallet-coin-activity").kids(recent.iter().enumerate().map(|(i, a)| activity_row(a, i, pr.tz_min, Msg::Push(WalletView::Activity { digest: a.digest.clone() })))));
    if pr.act.ready && recent.is_empty() {
        right = right.kid(empty(&format!("No {symbol} activity yet"), None, None, true));
    }
    if pr.act.error && !pr.act.ready {
        right = right.kid(empty("Could not load activity", None, None, true));
    }
    page_with(
        Some(title),
        None,
        None,
        false,
        "",
        vec![load_bar(pr.pf.loading || pr.act.loading), div("mt-4 grid grid-cols-[minmax(0,26rem)_minmax(0,1fr)] gap-8").kid(left).kid(right)],
    )
    .attr("data-ready", pr.pf.data.is_some().to_string())
}

pub fn hints() -> Vec<Hint> {
    vec![hint("a", "Open"), hint("b", "Back")]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::home::fixtures::*;
    use crate::screens::wallet::types::ActivityChange;

    #[test]
    fn coin() {
        let pf = PortfolioLoad { data: Some(portfolio()), ..Default::default() };
        let mut items = activity();
        items[0].changes = vec![ActivityChange { coin_type: "0x2::sui::SUI".into(), amount: "25000000000".into(), symbol: None, decimals: None }];
        let act = ActivityLoad { items, ready: true, ..Default::default() };
        let v = view(&Props { coin_type: "0x2::sui::SUI", can_sign: true, pf: &pf, act: &act, tz_min: 0 });
        let t = v.texts();
        assert_eq!(&t[..8], ["SUI", "Sui", "Your balance", "1,204.5 ", "SUI", "$1,091", "Price", "$0.906"]);
        assert!(t.contains(&"1.56%".to_string()));
        assert!(t.contains(&"Recent activity".to_string()));
        assert!(t.contains(&"Received".to_string()));
        assert_eq!(v.landing().as_deref(), Some("Send"));
        let none = ActivityLoad { ready: true, ..Default::default() };
        let v = view(&Props { coin_type: "0xabc::magma::MAGMA", can_sign: false, pf: &pf, act: &none, tz_min: 0 });
        let t = v.texts();
        assert_eq!(&t[..5], ["MAGMA", "Your balance", "0 ", "MAGMA", "No price"]);
        assert!(t.contains(&"No MAGMA activity yet".to_string()));
        assert_eq!(v.landing().as_deref(), Some("Add a signing account"));
    }
}
