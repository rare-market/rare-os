//! The account header (launcher/src/wallet/ui/home/AccountHeader.tsx, after rare.market's profile
//! header): picture, name, address and kind on the left; total value with the 24h move on the
//! right. Coin rows (CoinRows.tsx) and the activity list (ActivityList.tsx) live here too.

use super::lib::{day_label, is_sui_type, kind_label, short_digest};
use crate::screens::wallet::fmt::{fmt_token, fmt_usd, time_hm};
use crate::screens::wallet::kit::{addr, avatar, delta, account_avatar};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::types::{ActivityItem, ActivityKind, Portfolio, PortfolioToken, RareAccount};

/// `CopyAddress`: the short address with a copy icon; "Copied" for 1.5 s after A.
pub fn copy_address<M>(address: &str, done: bool, class: &str, on: M) -> El<M> {
    let mut b = button(cn(&["inline-flex h-8 items-center gap-1.5 rounded px-2 text-sm text-secondary transition-colors focus-visible:text-primary", class]), Some(on))
        .label("Copy address")
        .kid(addr(address, false, ""))
        .kid(icon(if done { "check" } else { "copy" }, "").attr("size", "0.9rem"));
    if done {
        b = b.kid(span("").text("Copied"));
    }
    b
}

/// `AccountHeader`. `pf` is None until the first portfolio answer.
pub fn account_header<M>(account: &RareAccount, pf: Option<&Portfolio>, copied: bool, on_copy: M) -> El<M> {
    let total = pf.and_then(|p| p.total_usd);
    let change = pf.and_then(|p| p.change_24h_usd);
    let before = match (total, change) {
        (Some(t), Some(c)) => Some(t - c),
        _ => None,
    };
    let pct = match (before, change) {
        (Some(b), Some(c)) if b > 0.0 => Some(c / b * 100.0),
        _ => None,
    };
    let sui = pf.and_then(|p| p.tokens.iter().find(|t| is_sui_type(&t.coin_type)));
    let big = match pf {
        None => "--".to_string(),
        Some(_) => match (total, sui) {
            (Some(t), _) => fmt_usd(Some(t)),
            (None, Some(s)) => format!("{} SUI", fmt_token(Some(s.amount), 4)),
            (None, None) => "$0.00".into(),
        },
    };
    let mut line = div("mt-1 flex items-center justify-end gap-3 text-sm tabular-nums");
    if let (Some(c), Some(p_)) = (change, pct) {
        let tone = if c > 0.0 {
            "text-positive"
        } else if c < 0.0 {
            "text-negative"
        } else {
            "text-secondary"
        };
        let sign = if c > 0.0 {
            "+"
        } else if c < 0.0 {
            "-"
        } else {
            ""
        };
        line = line.kid(
            span(cn(&["inline-flex items-center gap-2", tone]))
                .kid(span("font-medium").text(format!("{sign}{}", fmt_usd(Some(c.abs())))))
                .kid(delta(Some(p_), 2, ""))
                .kid(span("text-secondary").text("24h")),
        );
    }
    if let (Some(s), Some(_)) = (sui, total) {
        line = line.kid(span("text-secondary").text(format!("{} SUI", fmt_token(Some(s.amount), 4))));
    }
    if total.is_none() && pf.is_some_and(|p| !p.priced) {
        line = line.kid(span("text-secondary").text("Prices unavailable"));
    }
    div("mb-5 flex items-center gap-5")
        .kid(account_avatar(&account.address, "4.5rem"))
        .kid(
            div("min-w-0")
                // the Rare username when it has one, else the account name, else the short address
                .kid(h1("truncate text-2xl font-bold leading-[1.3]").text(crate::data::account::display_name(crate::data::account::cached_username(&account.address).as_deref(), &account.label, &account.address)))
                .kid(div("-ml-2 mt-0.5 flex items-center gap-3").kid(copy_address(&account.address, copied, "", on_copy)).kid(span("text-sm text-secondary").text(kind_label(account)))),
        )
        .kid(div("ml-auto text-right").kid(div("text-4xl font-bold leading-[1.2] tabular-nums").text(big)).kid(line))
}

pub const COIN_COLS: &str = "grid-cols-[minmax(0,1fr)_12rem_9rem_7rem]";

/// `CoinHead`.
pub fn coin_head<M>() -> El<M> {
    div(cn(&["trow grid h-10 items-center px-3 text-right text-sm text-secondary", COIN_COLS]))
        .kid(span("text-left font-medium").text("Coin"))
        .kid(span("px-3 font-medium").text("Amount"))
        .kid(span("px-3 font-medium").text("Value"))
        .kid(span("px-3 pr-4 font-medium").text("24h"))
}

/// `CoinRow`: a focusable table row.
pub fn coin_row<M>(t: &PortfolioToken, idx: usize, on: M) -> El<M> {
    let mut names = span("min-w-0").kid(span("block truncate text-base font-semibold").text(&t.symbol));
    if !t.name.is_empty() {
        names = names.kid(span("block truncate text-xs text-secondary").text(&t.name));
    }
    let mut value = span("px-3 font-semibold tabular-nums");
    if t.value_usd.is_some() {
        value = value.text(fmt_usd(t.value_usd));
    }
    button(cn(&["trow grid h-16 w-full items-center rounded-lg px-3 text-right text-sm transition-colors focus-visible:bg-layer-03", COIN_COLS]), Some(on))
        .attr("data-idx", idx.to_string())
        .key(&t.coin_type)
        .kid(span("flex min-w-0 items-center gap-3 text-left").kid(avatar(t.icon_url.as_deref(), &t.symbol, "2.5rem", false)).kid(names))
        .kid(span("px-3 tabular-nums").text(fmt_token(Some(t.amount), 6)))
        .kid(value)
        .kid(span("px-3 pr-4 font-medium").kid(delta(t.change_24h, 1, "")))
}

fn activity_title(k: ActivityKind) -> &'static str {
    match k {
        ActivityKind::Send => "Sent",
        ActivityKind::Receive => "Received",
        ActivityKind::Swap => "Swapped",
        ActivityKind::Other => "Transaction",
    }
}

fn activity_icon(k: ActivityKind) -> &'static str {
    match k {
        ActivityKind::Send => "arrow-up-right",
        ActivityKind::Receive => "arrow-down-left",
        ActivityKind::Swap => "arrow-left-right",
        ActivityKind::Other => "repeat",
    }
}

/// `ActivityRow`. `tz_min` is the device's UTC offset in minutes.
pub fn activity_row<M>(a: &ActivityItem, idx: usize, tz_min: i32, on: M) -> El<M> {
    let mut sub = span("block truncate text-sm text-secondary").text(&a.summary);
    if let Some(cp) = a.counterparty.as_deref().filter(|_| a.kind != ActivityKind::Swap) {
        sub.text = None;
        sub = sub.kid(text(&a.summary)).kid(text(format!(" {} ", if a.kind == ActivityKind::Receive { "from" } else { "to" }))).kid(addr(cp, false, ""));
    }
    button("flex h-[4.25rem] w-full items-center gap-4 rounded-lg px-3 text-left transition-colors focus-visible:bg-layer-03", Some(on))
        .attr("data-idx", idx.to_string())
        .key(&a.digest)
        .kid(span("flex size-10 shrink-0 items-center justify-center rounded-full bg-layer-03 text-icon-primary").kid(icon(activity_icon(a.kind), "").attr("size", "1.2rem")))
        .kid(span("min-w-0 flex-1").kid(span("block text-base font-semibold").text(activity_title(a.kind))).kid(sub))
        .kid(
            span("shrink-0 text-right")
                .kid(span(cn(&["block text-sm", if a.failed { "text-negative" } else { "text-secondary" }])).text(if a.failed { "Failed".to_string() } else { time_hm(a.timestamp, tz_min) }))
                .kid(span("block font-mono text-xs text-secondary/70").text(short_digest(&a.digest))),
        )
}

/// `ActivityGroups`: day headings with their rows; rows are numbered across the groups.
pub fn activity_groups<M>(items: &[ActivityItem], now: i64, tz_min: i32, on: impl Fn(&ActivityItem) -> M) -> El<M> {
    let mut groups: Vec<(String, Vec<&ActivityItem>)> = Vec::new();
    for a in items {
        let k = day_label(a.timestamp, now, tz_min);
        match groups.iter_mut().find(|(d, _)| *d == k) {
            Some((_, l)) => l.push(a),
            None => groups.push((k, vec![a])),
        }
    }
    let mut idx = 0;
    div("").kids(groups.into_iter().map(|(day, list)| {
        let mut g = div("mb-3").key(&day).kid(div("px-3 pb-1 pt-2 text-sm font-semibold text-secondary").text(&day));
        for a in list {
            g = g.kid(activity_row(a, idx, tz_min, on(a)));
            idx += 1;
        }
        g
    }))
}
