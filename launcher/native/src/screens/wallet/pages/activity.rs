//! One past transaction (launcher/src/wallet/ui/pages/ActivityDetail.tsx): what moved, who with,
//! fee, status, time and digest, plus a QR of the explorer link to open it on a phone.

use crate::screens::wallet::fmt::{addr_short, date_time_medium, fmt_token, fmt_usd, units_to_number};
use crate::screens::wallet::home::lib::{copy_value, hint, load_bar, short_digest, Hint};
use crate::screens::wallet::kit::{addr, avatar, btn_full, page, panel, row, Size, Variant};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::types::{ActivityChange, ActivityItem, ActivityKind};
use std::collections::BTreeMap;

/// One coin flow: signed base units.
#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    pub coin_type: String,
    pub amount: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TxObject {
    pub id: String,
    /// "sent" | "created" | "deleted" | "received"
    pub action: String,
    pub type_: Option<String>,
}

/// wallet/data/txdetail.ts `TxDetail` (from rare-sui's transaction detail).
#[derive(Clone, Debug, PartialEq)]
pub struct TxDetail {
    pub flows: Vec<Flow>,
    pub counterparty: Option<String>,
    pub failed: bool,
    pub error: Option<String>,
    pub timestamp_ms: Option<i64>,
    /// net fee in MIST, negative when the storage refund is larger
    pub fee_mist: i128,
    pub fee_paid: bool,
    pub sender: Option<String>,
    /// (module, function)
    pub calls: Vec<(String, String)>,
    pub objects: Vec<TxObject>,
}

/// `useCoinInfo`: symbol, decimals, logo and price per coin type (the Trade API, then chain metadata).
#[derive(Clone, Debug, PartialEq)]
pub struct CoinInfo {
    pub symbol: String,
    pub decimals: u32,
    pub logo: Option<String>,
    pub price: Option<f64>,
    pub known: bool,
}

pub struct Props<'a> {
    pub digest: &'a str,
    /// the row the activity list already had (`knownActivity`)
    pub item: Option<&'a ActivityItem>,
    /// None while loading; Some(None) when the network no longer has the details
    pub detail: Option<Option<&'a TxDetail>>,
    pub loading: bool,
    pub error: bool,
    pub coins: &'a BTreeMap<String, CoinInfo>,
    pub tz_min: i32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub open: bool,
    /// which value shows its copied check
    pub copied: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Details,
    Copy(String),
    OpenWeb(String),
}

fn title(k: ActivityKind) -> &'static str {
    match k {
        ActivityKind::Send => "Sent",
        ActivityKind::Receive => "Received",
        ActivityKind::Swap => "Swapped",
        ActivityKind::Other => "Transaction",
    }
}

fn info(coins: &BTreeMap<String, CoinInfo>, t: &str) -> CoinInfo {
    coins.get(t).cloned().unwrap_or_else(|| CoinInfo { symbol: t.rsplit("::").next().unwrap_or("?").to_string(), decimals: 9, logo: None, price: None, known: false })
}

pub fn suiscan(digest: &str) -> String {
    format!("https://suiscan.xyz/mainnet/tx/{digest}")
}

pub fn view(pr: &Props, st: &State) -> El<Msg> {
    let d = pr.detail.flatten();
    let flows: Vec<Flow> = match d {
        Some(d) => d.flows.clone(),
        None => pr.item.map(|i| i.changes.iter().map(|c: &ActivityChange| Flow { coin_type: c.coin_type.clone(), amount: c.amount.clone() }).collect()).unwrap_or_default(),
    };
    let out: Vec<&Flow> = flows.iter().filter(|f| f.amount.starts_with('-')).collect();
    let inn: Vec<&Flow> = flows.iter().filter(|f| !f.amount.starts_with('-')).collect();
    let kind = pr.item.map(|i| i.kind).unwrap_or(if !out.is_empty() && !inn.is_empty() {
        ActivityKind::Swap
    } else if !out.is_empty() {
        ActivityKind::Send
    } else if !inn.is_empty() {
        ActivityKind::Receive
    } else {
        ActivityKind::Other
    });
    let counterparty = if kind == ActivityKind::Swap { None } else { d.and_then(|d| d.counterparty.clone()).or_else(|| pr.item.and_then(|i| i.counterparty.clone())) };
    let failed = match d {
        Some(d) => d.failed,
        None => pr.item.is_some_and(|i| i.failed),
    };
    let ts = d.and_then(|d| d.timestamp_ms).or(pr.item.map(|i| i.timestamp));
    let fee = d.map(|d| d.fee_mist).unwrap_or(0);
    let sui = info(pr.coins, "0x2::sui::SUI");
    let fee_num = fee.unsigned_abs() as f64 / 1e9;
    let url = suiscan(pr.digest);
    let copied = |v: &str| st.copied.as_deref() == Some(v);

    let list = |t: &str, rows: &[&Flow], sign: &str| -> Option<El<Msg>> {
        if rows.is_empty() {
            return None;
        }
        Some(panel(
            Some(t),
            None,
            "pb-2",
            vec![div("mt-2 px-2").kids(rows.iter().map(|f| {
                let c = info(pr.coins, &f.coin_type);
                let n = units_to_number(&f.amount, c.decimals).abs();
                let mut right = span("text-right tabular-nums").kid(span(if sign == "+" { "block text-base font-semibold text-positive" } else { "block text-base font-semibold" }).text(format!(
                    "{sign}{} {}",
                    if c.known { fmt_token(Some(n), 6) } else { "?".into() },
                    c.symbol
                )));
                if let (Some(p_), true) = (c.price, c.known) {
                    right = right.kid(span("block text-xs text-secondary").text(fmt_usd(Some(n * p_))));
                }
                div("flex h-14 items-center gap-3 px-3")
                    .key(&f.coin_type)
                    .kid(avatar(c.logo.as_deref(), &c.symbol, "2rem", false))
                    .kid(span("min-w-0 flex-1").kid(span("block text-base font-semibold").text(&c.symbol)).kid(span("block text-xs text-secondary").text(match c.price {
                        Some(p_) => fmt_usd(Some(p_)),
                        None => "No price".into(),
                    })))
                    .kid(right)
            }))],
        ))
    };

    let mut col = div("flex min-w-0 flex-col gap-4");
    let mut status = panel(
        None,
        None,
        "flex flex-col gap-2 px-5 py-4",
        vec![row("Status", vec![span(if failed { "text-negative" } else { "" }).text(if failed { "Failed" } else if d.is_some() || pr.item.is_some() { "Success" } else { "--" })], "", true)],
    );
    if let Some(e) = d.and_then(|d| d.error.as_ref()) {
        status = status.kid(p("text-sm text-negative").text(e));
    }
    col = col.kid(status);
    if pr.detail == Some(None) {
        col = col.kid(p("px-1 text-sm text-secondary").text("The network no longer keeps the full details of this transaction. This is what the activity list recorded."));
    }
    col = col.kid_opt(list("You sent", &out, "-")).kid_opt(list("You received", &inn, "+"));
    let mut rows = Vec::new();
    if let Some(cp) = &counterparty {
        let c: Vec<char> = cp.chars().collect();
        let shown = if c.len() >= 14 { format!("{}...{}", c[..8].iter().collect::<String>(), c[c.len() - 6..].iter().collect::<String>()) } else { addr_short(cp) };
        rows.push(row(if kind == ActivityKind::Receive { "From" } else { "To" }, vec![copy_value(&shown, "Copy address", copied(cp), Msg::Copy(cp.clone()))], "", true));
    }
    if let Some(d) = d {
        let v = if !d.fee_paid {
            vec![text("Paid by sponsor")]
        } else {
            let mut s = span(if fee < 0 { "text-positive" } else { "" }).text(format!("{}{} SUI", if fee < 0 { "Refund " } else { "" }, fmt_token(Some(fee_num), 6)));
            if let Some(p_) = sui.price {
                s.text = None;
                s = s.kid(text(format!("{}{} SUI", if fee < 0 { "Refund " } else { "" }, fmt_token(Some(fee_num), 6)))).kid(span("ml-2 text-secondary").text(fmt_usd(Some(fee_num * p_))));
            }
            vec![s]
        };
        rows.push(row("Network fee", v, "", true));
    }
    rows.push(row("Digest", vec![copy_value(&short_digest(pr.digest), "Copy digest", copied(pr.digest), Msg::Copy(pr.digest.to_string()))], "", true));
    if let Some(s) = d.and_then(|d| d.sender.as_ref()) {
        rows.push(row("Sender", vec![addr(s, false, "")], "", true));
    }
    col = col.kid(panel(None, None, "flex flex-col gap-1 px-5 py-4", rows));
    if let Some(d) = d.filter(|d| !d.calls.is_empty() || !d.objects.is_empty()) {
        let mut pnl = vec![button("flex h-14 w-full items-center justify-between rounded-lg px-5 text-base font-semibold", Some(Msg::Details))
            .attr("aria-expanded", st.open.to_string())
            .kid(text("Details"))
            .kid(icon(if st.open { "chevron-up" } else { "chevron-down" }, "").attr("size", "1.1rem"))];
        if st.open {
            let mut lines = div("flex flex-col gap-1 px-5 pb-4 text-sm");
            for (i, (m, f)) in d.calls.iter().enumerate() {
                lines = lines.kid(p("font-mono text-secondary").key(i.to_string()).text(format!("{m}::{f}")));
            }
            for o in &d.objects {
                let act = match o.action.as_str() {
                    "sent" => "Sent",
                    "created" => "Created",
                    "deleted" => "Deleted",
                    _ => "Received",
                };
                lines = lines.kid(
                    p("text-secondary")
                        .key(&o.id)
                        .kid(text(format!("{act} ")))
                        .kid(span("font-mono").text(short_digest(&o.id)))
                        .kid_opt(o.type_.as_ref().map(|t| span("ml-2 font-mono text-xs").text(t.rsplit("::").next().unwrap_or("")))),
                );
            }
            pnl.push(lines);
        }
        col = col.kid(panel(None, None, "", pnl));
    }
    let qr = panel(
        None,
        None,
        "flex flex-col items-center p-5 text-center",
        vec![
            el(Tag::Qr(url.clone()), "size-[12rem]"),
            p("mt-3 text-sm font-semibold").text("Open on your phone"),
            p("mt-1 text-xs text-secondary").text("Scan to see this transaction on Suiscan."),
            btn_full("View on Suiscan", Variant::Secondary, Size::Md, false, "mt-3 w-full", Some(Msg::OpenWeb(url.clone()))),
        ],
    );
    page(
        Some(title(kind)),
        ts.map(|t| text(date_time_medium(t, pr.tz_min))),
        None,
        false,
        "",
        vec![load_bar(pr.loading), div("mt-3 grid grid-cols-[minmax(0,1fr)_16rem] items-start gap-8").kid(col).kid(qr)],
    )
    .attr("data-ready", (d.is_some() || pr.error).to_string())
}

pub fn update(st: &mut State, msg: Msg) -> Option<Msg> {
    match msg {
        Msg::Details => {
            st.open = !st.open;
            None
        }
        Msg::Copy(v) => {
            st.copied = Some(v.clone());
            Some(Msg::Copy(v))
        }
        Msg::OpenWeb(u) => Some(Msg::OpenWeb(u)),
    }
}

pub fn hints() -> Vec<Hint> {
    vec![hint("a", "Select"), hint("b", "Back")]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::home::fixtures::{activity, NOW};
    use crate::screens::wallet::sign::fixtures::OTHER;

    fn coins() -> BTreeMap<String, CoinInfo> {
        let mut m = BTreeMap::new();
        m.insert("0x2::sui::SUI".to_string(), CoinInfo { symbol: "SUI".into(), decimals: 9, logo: None, price: Some(0.906), known: true });
        m
    }

    #[test]
    fn a_receive_with_detail() {
        let items = activity();
        let d = TxDetail {
            flows: vec![Flow { coin_type: "0x2::sui::SUI".into(), amount: "25000000000".into() }],
            counterparty: Some(OTHER.into()),
            failed: false,
            error: None,
            timestamp_ms: Some(NOW - 55 * 60_000),
            fee_mist: 1_997_880,
            fee_paid: true,
            sender: Some(OTHER.into()),
            calls: vec![("pay".into(), "split_and_transfer".into())],
            objects: vec![],
        };
        let c = coins();
        let pr = Props { digest: &items[0].digest, item: Some(&items[0]), detail: Some(Some(&d)), loading: false, error: false, coins: &c, tz_min: 0 };
        let mut st = State::default();
        let v = view(&pr, &st);
        assert_eq!(
            v.texts(),
            [
                "Received", "Oct 27, 2026, 2:12 PM", "Status", "Success", "You received", "SUI", "$0.91", "+25 SUI", "$22.65", "From", "0xababab...ababab", "Network fee", "0.001998 SUI", "$0.00", "Digest", "9xQm3Z...5eAa", "Sender",
                "0xabab...abab", "Details", "Open on your phone", "Scan to see this transaction on Suiscan.", "View on Suiscan",
            ]
        );
        update(&mut st, Msg::Details);
        assert!(view(&pr, &st).contains_text("pay::split_and_transfer"));
    }

    #[test]
    fn pruned_detail_uses_the_list() {
        let items = activity();
        let c = coins();
        let pr = Props { digest: &items[3].digest, item: Some(&items[3]), detail: Some(None), loading: false, error: false, coins: &c, tz_min: 0 };
        let v = view(&pr, &State::default());
        let t = v.texts();
        assert_eq!(&t[..4], ["Transaction", "Oct 26, 2026, 11:07 AM", "Status", "Failed"]);
        assert!(v.contains_text("The network no longer keeps the full details of this transaction. This is what the activity list recorded."));
        let unknown = Props { item: None, detail: None, loading: true, ..pr };
        let t = view(&unknown, &State::default()).texts();
        assert_eq!(&t[..3], ["Transaction", "Status", "--"]);
    }
}
