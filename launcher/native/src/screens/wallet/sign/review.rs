//! The global transaction / message review (launcher/src/wallet/ui/sign/ReviewSheet.tsx), in
//! rare.market's modal. Shows the oldest request in the signing queue: who is asking, what leaves
//! and arrives, the fee and any warning in plain words. Approve asks for the PIN when the account
//! needs it, then signs; hardware wallets and the passkey phone show their live state in place of
//! the footer.
//!
//! The sheet shows exactly what the TS sheet shows for the same request and review: every line,
//! in the same order, with the same enabled and default buttons (tests below pin it). The review
//! itself (simulate, analyze, describe, risks) is rare-wallet's job; this takes its result.

use super::packages::{call_label, split_fees, CoinFlow, MoveCallRef, OtherReceive};
use super::panels::{keystone_panel, ledger_panel, passkey_panel, KeystonePrompt, LedgerPrompt, PanelMsg, Passkey};
use super::site::{decode_message, host_of, site_risks, Risk};
use crate::screens::wallet::fmt::{format_amount, norm_type, short_id, sui_type, to_number, usd};
use crate::screens::wallet::kit::{addr, avatar, btn, btn_full, mimg, row, sheet, toggle_row, SheetSpec, Size, Variant};
use crate::screens::wallet::markup::*;
use std::collections::BTreeMap;

// ------------------------------------------------------------------ inputs

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReqKind {
    Transaction,
    Message,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginKind {
    Wallet,
    Market,
    Game,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Origin {
    /// "Rare Marketplace", a game's title, or "Rare Wallet"
    pub name: String,
    /// web origin for games
    pub url: Option<String>,
    pub icon: Option<String>,
    pub kind: OriginKind,
}

/// A queued sign request (wallet/sign.ts `SignRequest`, the fields the sheet reads).
#[derive(Clone, Debug, PartialEq)]
pub struct SignRequest {
    pub id: u64,
    pub kind: ReqKind,
    /// the account that signs
    pub account: String,
    pub origin: Origin,
    /// submit after signing (signAndExecute)
    pub execute: bool,
    pub dry_run: bool,
    /// "Buy Popkins #18736", "Send 5 SUI"
    pub title: Option<String>,
    /// a line under the network fee ("<game>'s gas budget is used up, so you pay this fee.")
    pub fee_note: Option<String>,
    pub message: Option<Vec<u8>>,
}

/// The wallet's accounts (`RareAccount`).
pub use crate::screens::wallet::types::RareAccount as Account;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Send,
    Receive,
    Fee,
}

/// describe.ts `ReviewRow`.
#[derive(Clone, Debug, PartialEq)]
pub struct ReviewRow {
    pub kind: RowKind,
    /// "You send" / "You receive" / "Network fee"
    pub label: String,
    /// "1.5 SUI", "Popkins #8475", "0.0019 SUI"
    pub text: String,
    pub sub: Option<String>,
    pub coin_type: Option<String>,
    /// absolute base units for coin rows; signed for the fee (negative = refund)
    pub amount: Option<i128>,
    pub icon_url: Option<String>,
    pub object_id: Option<String>,
    pub image: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewCoin {
    pub symbol: String,
    pub decimals: u32,
    pub icon_url: Option<String>,
    pub price_usd: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SharedTouch {
    pub object_id: String,
    pub label: String,
}

/// review/index.ts `Review`, the parts the sheet reads.
#[derive(Clone, Debug, PartialEq)]
pub struct Review {
    pub ok: bool,
    /// insights.reason: plain words for the failure banner
    pub reason: Option<String>,
    pub risks: Vec<Risk>,
    pub rows: Vec<ReviewRow>,
    /// keyed by normalized coin type
    pub coins: BTreeMap<String, ReviewCoin>,
    pub send: Vec<CoinFlow>,
    pub others_receive: Vec<OtherReceive>,
    pub calls: Vec<MoveCallRef>,
    pub shared: Vec<SharedTouch>,
    /// simulation.packages: called package id -> its original id
    pub originals: BTreeMap<String, String>,
    /// the exact simulated bytes (Approve signs these)
    pub bytes: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Load {
    Loading,
    Ready(Box<Review>),
    Unavailable,
}

/// rare.market's view of an object id (name, art, collection) for an object row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemInfo {
    pub name: Option<String>,
    pub image_url: Option<String>,
    pub collection: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SignResult {
    pub digest: Option<String>,
    pub dry_run: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Review,
    Working,
    Done,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    pub phase: Phase,
    pub error: Option<String>,
    /// the request is gone from the queue (the network refused it): only Done is left
    pub final_: bool,
    pub result: Option<SignResult>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hw {
    Ledger,
    Keystone,
    Passkey,
}

fn hw(kind: Option<&str>) -> Option<Hw> {
    match kind? {
        "ledger" => Some(Hw::Ledger),
        "keystone" => Some(Hw::Keystone),
        "passkey" => Some(Hw::Passkey),
        _ => None,
    }
}

/// Everything the sheet reads from the wallet, the queue and the devices.
pub struct Props<'a> {
    pub req: &'a SignRequest,
    pub flow: Option<&'a Flow>,
    pub accounts: &'a [Account],
    pub load: &'a Load,
    pub items: &'a BTreeMap<String, ItemInfo>,
    /// screenshots: pretend the account is this kind (useDevHw)
    pub force: Option<Hw>,
    pub ledger: &'a LedgerPrompt,
    pub keystone: &'a KeystonePrompt,
    pub passkey: &'a Passkey,
    /// How this account approves (a line under the buttons), so the prompt is never a mystery.
    pub method: Option<&'a str>,
    /// The passkey signs on the phone this time (the QR panel shows at once, never a dead button).
    pub phone: bool,
}

/// The sheet's own state (React `useState`s); a new request starts from `Default`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub acked: bool,
    pub anyway: bool,
    /// Details open; None until toggled, then it starts open when the review failed
    pub details: Option<bool>,
    /// "Copied" for 1.5 s after Copy ID
    pub copied: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Approve,
    Cancel,
    Done,
    Ack(bool),
    Anyway,
    Details,
    CopyId(String),
    Panel(PanelMsg),
}

// ------------------------------------------------------------------ body pieces

fn money(n: Option<f64>) -> Option<String> {
    n.map(|n| if n > 0.0 && n < 0.01 { "<$0.01".to_string() } else { usd(n) })
}

fn section_box(title: &str, rows: Vec<El<Msg>>) -> El<Msg> {
    section("mt-6 first:mt-0").label(title).kid(h3("mb-2 text-sm text-secondary").text(title)).kid(div("divide-y divide-border-primary rounded border border-border-primary").kids(rows))
}

fn coin_row(r: &ReviewRow, review: &Review) -> El<Msg> {
    let meta = review.coins.get(&norm_type(r.coin_type.as_deref().unwrap_or(&sui_type())));
    let dec = meta.map(|m| m.decimals).unwrap_or(9);
    let amount = r.amount.unwrap_or(0);
    let value = meta.and_then(|m| m.price_usd).map(|p| to_number(amount, dec) * p);
    let out = r.kind == RowKind::Send;
    let symbol = meta.map(|m| m.symbol.as_str()).unwrap_or("");
    div("flex items-center gap-3 px-4 py-3")
        .kid(avatar(r.icon_url.as_deref().or(meta.and_then(|m| m.icon_url.as_deref())), meta.map(|m| m.symbol.as_str()).unwrap_or("?"), "2rem", false))
        .kid(
            div("min-w-0 flex-1")
                .kid(p("truncate text-base font-medium").text(symbol))
                .kid(p("truncate text-xs text-secondary").text(match meta.and_then(|m| m.price_usd) {
                    Some(pr) => usd(pr),
                    None => "No price".into(),
                })),
        )
        .kid(
            div("text-right")
                .kid(p(cn(&["text-base font-medium tabular-nums", when(!out, "text-positive")])).text(format!("{}{} {}", if out { "-" } else { "+" }, format_amount(amount, dec), symbol)))
                .kid(p("h-4 text-xs text-secondary tabular-nums").text(money(value).unwrap_or_default())),
        )
}

fn object_row(r: &ReviewRow, items: &BTreeMap<String, ItemInfo>) -> El<Msg> {
    let q = r.object_id.as_ref().and_then(|id| items.get(id));
    let name = q.and_then(|q| q.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| r.text.clone());
    let image = r.image.clone().or_else(|| q.and_then(|q| q.image_url.clone()));
    let coll = q.and_then(|q| q.collection.clone());
    let show_sub = r.sub.as_deref().is_some_and(|s| !s.is_empty()) || coll.as_deref().is_some_and(|c| !c.is_empty());
    let sub = r.sub.clone().or(coll);
    div("flex items-center gap-3 px-4 py-3")
        .kid(span("relative size-12 shrink-0 overflow-hidden rounded-md bg-layer-03").kid(mimg(image.as_deref(), &name, 120, "absolute inset-0 size-full")))
        .kid(div("min-w-0 flex-1").kid(p("truncate text-base font-medium").text(&name)).kid_if(show_sub, || p("truncate text-xs text-secondary").text(sub.unwrap_or_default())))
}

fn notes(risks: &[Risk]) -> Option<El<Msg>> {
    if risks.is_empty() {
        return None;
    }
    let mut l = ul("mt-6 flex flex-col gap-1.5 text-sm");
    if risks.iter().any(|r| r.danger) {
        l = l.role("alert");
    }
    Some(l.kids(risks.iter().map(|r| li(if r.danger { "text-negative" } else { "text-secondary" }).key(&r.id).text(&r.text))))
}

fn details(review: &Review, open: bool) -> Option<El<Msg>> {
    if review.calls.is_empty() && review.shared.is_empty() {
        return None;
    }
    let mut s = section("mt-6").label("Details").kid(
        button("flex w-full items-center justify-between rounded px-1 py-2 text-sm text-secondary", Some(Msg::Details))
            .attr("aria-expanded", open.to_string())
            .kid(span("").text("Details"))
            .kid(icon("chevron-down", if open { "rotate-180" } else { "" }).attr("size", "1rem")),
    );
    if open {
        let mut lines = div("mt-1 flex flex-col gap-2 text-sm");
        for (i, c) in review.calls.iter().enumerate() {
            let (title, sub) = call_label(c, &review.originals);
            lines = lines.kid(p("flex justify-between gap-4").key(i.to_string()).kid(code("font-mono text-xs").text(title)).kid(span("text-secondary").text(sub)));
        }
        for sh in &review.shared {
            lines = lines.kid(p("flex justify-between gap-4").key(&sh.object_id).kid(code("font-mono text-xs").text(&sh.label)).kid(span("font-mono text-xs text-secondary").text(short_id(&sh.object_id))));
        }
        s = s.kid(lines);
    }
    Some(s)
}

fn account_line(address: &str, accounts: &[Account]) -> El<Msg> {
    let a = accounts.iter().find(|x| x.address == address);
    span("").kid(text(format!("{} ", a.map(|a| a.label.as_str()).unwrap_or("Account")))).kid(addr(address, false, "ml-1 text-secondary"))
}

fn tx_body(pr: &Props, st: &State, site: &[Risk]) -> Vec<El<Msg>> {
    let review = match pr.load {
        Load::Loading => {
            return vec![
                div("load-bar pointer-events-none absolute inset-x-0 top-0 h-[2px]").role("progressbar").label("Checking"),
                p("py-10 text-center text-sm text-secondary").text("Checking what this does"),
            ]
        }
        Load::Unavailable => {
            let mut v = vec![p("text-sm text-secondary").text("Could not preview this transaction. Approve only if you trust who is asking.")];
            v.extend(notes(site));
            return v;
        }
        Load::Ready(r) => r,
    };
    let failed = !review.ok;
    let sends: Vec<&ReviewRow> = review.rows.iter().filter(|r| r.kind == RowKind::Send).collect();
    let gets: Vec<&ReviewRow> = review.rows.iter().filter(|r| r.kind == RowKind::Receive).collect();
    let fee = review.rows.iter().find(|r| r.kind == RowKind::Fee);
    let sui_price = review.coins.get(&sui_type()).and_then(|c| c.price_usd);
    let fee_usd = match (fee.and_then(|f| f.amount), sui_price) {
        (Some(a), Some(p)) => Some(a.unsigned_abs() as f64 / 1e9 * p),
        _ => None,
    };
    let mut risks: Vec<Risk> = review.risks.iter().filter(|r| !(failed && r.id == "failed")).cloned().chain(site.iter().cloned()).collect();
    risks.sort_by_key(|r| !r.danger); // stable: danger first, order kept
    let needs_ack = review.risks.iter().any(|r| r.requires_confirm);
    let nothing = review.ok && sends.is_empty() && gets.is_empty();
    let (rare_fees, others) = split_fees(&review.others_receive, &review.send);
    let draw = |r: &&ReviewRow| if r.coin_type.is_some() { coin_row(r, review) } else { object_row(r, pr.items) };

    let mut out = Vec::new();
    if failed {
        out.push(
            div("mb-6 rounded border border-negative/60 px-4 py-3")
                .role("alert")
                .kid(p("text-base font-semibold text-negative").text("This would fail"))
                .kid(p("mt-1 text-sm").text(review.reason.clone().unwrap_or_default())),
        );
    }
    if !sends.is_empty() {
        out.push(section_box("You send", sends.iter().map(draw).collect()));
    }
    if !gets.is_empty() {
        out.push(section_box("You receive", gets.iter().map(draw).collect()));
    }
    if nothing {
        out.push(p("text-sm text-secondary").text("No coins or objects move in or out of your wallet."));
    }
    if !others.is_empty() {
        out.push(section_box(
            "Goes to",
            others
                .iter()
                .map(|o| {
                    let m = review.coins.get(&norm_type(&o.coin_type));
                    div("flex items-center justify-between gap-4 px-4 py-3 text-sm")
                        .kid(addr(&o.address, false, ""))
                        .kid(span("tabular-nums").text(format!("{} {}", format_amount(o.amount, m.map(|m| m.decimals).unwrap_or(9)), m.map(|m| m.symbol.as_str()).unwrap_or(""))))
                })
                .collect(),
        ));
    }
    let mut boxed = div("mt-6 flex flex-col gap-1.5 rounded border border-border-primary px-4 py-3").kid(row("Account", vec![account_line(&pr.req.account, pr.accounts)], "", true));
    if let Some(f) = fee {
        let mut v = vec![span(cn(&["tabular-nums", when(f.amount.is_some_and(|a| a < 0), "text-positive")])).text(&f.text)];
        if let Some(u) = money(fee_usd) {
            v.push(span("ml-2 text-secondary tabular-nums").text(u));
        }
        boxed = boxed.kid(row("Network fee", v, "", true));
        if let Some(sub) = f.sub.as_deref().filter(|s| !s.is_empty()) {
            boxed = boxed.kid(p("text-right text-xs text-secondary").text(sub));
        }
    }
    if let Some(n) = pr.req.fee_note.as_deref().filter(|s| !s.is_empty()) {
        boxed = boxed.kid(p("text-right text-xs text-secondary").text(n));
    }
    for f in &rare_fees {
        let m = review.coins.get(&norm_type(&f.coin_type));
        let label = match &f.pct {
            Some(pct) => format!("{} {pct}", f.label),
            None => f.label.clone(),
        };
        boxed = boxed.kid(row(&label, vec![span("tabular-nums").text(format!("{} {}", format_amount(f.amount, m.map(|m| m.decimals).unwrap_or(9)), m.map(|m| m.symbol.as_str()).unwrap_or("")))], "", true));
    }
    out.push(boxed);
    out.extend(notes(&risks));
    if needs_ack {
        out.push(div("mt-4 -mx-4").kid(toggle_row("I checked the full address and it is right", None, st.acked, Msg::Ack(!st.acked))));
    }
    out.extend(details(review, st.details.unwrap_or(failed)));
    out
}

fn message_body(pr: &Props, site: &[Risk]) -> Vec<El<Msg>> {
    let m = decode_message(pr.req.message.as_deref().unwrap_or(&[]));
    let is_text = m.text.is_some();
    let mut v = vec![
        h3("mb-2 text-sm text-secondary").text(if is_text { "Message" } else { "Message (hex)" }),
        pre(cn(&["max-h-[14rem] overflow-y-auto whitespace-pre-wrap break-all rounded border border-border-primary bg-layer-01 px-4 py-3 text-sm", when(!is_text, "font-mono text-xs")]))
            .attr("data-testid", "message-text")
            .text(m.text.unwrap_or(m.hex)),
        p("mt-4 text-sm text-secondary").text("Signing proves you own this account. It cannot move funds by itself, but a site can use it to sign you in or approve actions, so only sign messages you expect."),
        div("mt-6 rounded border border-border-primary px-4 py-3").kid(row("Account", vec![account_line(&pr.req.account, pr.accounts)], "", true)),
    ];
    v.extend(notes(site));
    v
}

fn done_body(pr: &Props, st: &State, result: &SignResult) -> Vec<El<Msg>> {
    let message = pr.req.kind == ReqKind::Message;
    let mut d = div("flex flex-col items-start gap-4").kid(p("text-base").text(if message {
        "Your signature is ready."
    } else if pr.req.execute {
        "Your transaction went through."
    } else {
        "Your signature is ready."
    }));
    if let Load::Ready(review) = pr.load {
        let rows: Vec<&ReviewRow> = review.rows.iter().filter(|r| r.kind != RowKind::Fee).collect();
        if !rows.is_empty() {
            d = d.kid(div("w-full rounded border border-border-primary px-4 py-3 text-sm").kids(rows.iter().enumerate().map(|(i, r)| row(&r.label, vec![text(&r.text)], "", true).key(i.to_string()))));
        }
    }
    if let Some(id) = result.digest.as_deref().filter(|s| !s.is_empty()) {
        d = d.kid(div("w-full").kid(p("mb-1.5 text-sm text-secondary").text("Transaction ID")).kid(p("break-all font-mono text-sm").text(id)));
    }
    if result.dry_run {
        d = d.kid(p("text-sm text-secondary").text("Test run. Nothing was sent to the network."));
    }
    if let Some(id) = result.digest.as_deref().filter(|s| !s.is_empty()) {
        d = d.kid(btn_full(if st.copied { "Copied" } else { "Copy ID" }, Variant::Secondary, Size::Md, false, "", Some(Msg::CopyId(id.to_string()))));
    }
    vec![d]
}

// ------------------------------------------------------------------ the sheet

/// The whole sheet for one request.
pub fn view(pr: &Props, st: &State) -> El<Msg> {
    let req = pr.req;
    let account = pr.accounts.iter().find(|a| a.address == req.account);
    let kind = pr.force.or_else(|| hw(account.map(|a| a.kind.as_str())));
    let phase = pr.flow.map(|f| f.phase).unwrap_or(Phase::Review);
    let is_msg = req.kind == ReqKind::Message;
    let site = site_risks(req.origin.url.as_deref());
    let review = match pr.load {
        Load::Ready(r) => Some(r.as_ref()),
        _ => None,
    };
    let failed = review.is_some_and(|r| !r.ok);
    let needs_ack = review.is_some_and(|r| r.risks.iter().any(|x| x.requires_confirm));
    let loading = !is_msg && *pr.load == Load::Loading;
    let blocked = loading || (failed && !st.anyway) || (needs_ack && !st.acked);

    let title = if phase == Phase::Done {
        if is_msg || !req.execute { "Signed" } else { "Sent" }.to_string()
    } else {
        req.title.clone().unwrap_or_else(|| {
            if is_msg {
                "Sign message"
            } else if req.execute {
                "Approve transaction"
            } else {
                "Sign transaction"
            }
            .to_string()
        })
    };
    let origin_line = if req.origin.kind == OriginKind::Game { format!("{} · {}", req.origin.name, host_of(req.origin.url.as_deref())) } else { req.origin.name.clone() };
    let icon_el = if req.origin.kind == OriginKind::Game {
        avatar(req.origin.icon.as_deref(), &req.origin.name, "3rem", true)
    } else {
        span("relative size-12 shrink-0 overflow-hidden rounded-md bg-layer-03").kid(img(Some("/brand/mark.png"), "", "absolute inset-0 size-full object-contain p-1.5"))
    };

    let flow_final = pr.flow.is_some_and(|f| f.final_);
    let flow_error = pr.flow.and_then(|f| f.error.clone()).unwrap_or_default();
    let footer: Vec<El<Msg>> = if phase == Phase::Done {
        vec![btn("Done", Variant::Primary, "flex-1", Some(Msg::Done)).def(true)]
    } else if phase == Phase::Error && flow_final {
        vec![p("mr-auto self-center text-sm text-negative").text(&flow_error), btn("Done", Variant::Primary, "", Some(Msg::Done)).def(true)]
    } else if let Some(k) = kind.filter(|k| phase == Phase::Working && (*k != Hw::Passkey || pr.force.is_some() || pr.phone || *pr.passkey != Passkey::Idle)) {
        let panel = match k {
            Hw::Ledger => ledger_panel(pr.ledger),
            Hw::Keystone => keystone_panel(pr.keystone),
            Hw::Passkey => passkey_panel(pr.passkey),
        };
        vec![panel.map(&Msg::Panel)]
    } else if phase == Phase::Working {
        vec![btn("Signing", Variant::Dimmed, "flex-1", None), btn("Cancel", Variant::Secondary, "", Some(Msg::Cancel)).def(true)]
    } else {
        let mut col = div("flex w-full flex-col gap-3");
        if phase == Phase::Error {
            col = col.kid(p("text-sm text-negative").text(&flow_error));
        }
        // A transaction that could not even be built (not enough of a coin) has nothing to force.
        let can_force = review.is_some_and(|r| r.bytes.is_some() || !r.calls.is_empty());
        if failed && !st.anyway && can_force {
            col = col.kid(p("text-sm text-secondary").kid(text("Approve is off because this would fail. ")).kid(button("rounded px-1 text-primary underline", Some(Msg::Anyway)).text("Approve anyway")));
        }
        if failed && st.anyway {
            col = col.kid(p("text-sm text-secondary").text("It will likely fail and still cost the network fee."));
        }
        let main = if blocked {
            btn(
                if loading {
                    "Checking"
                } else if is_msg {
                    "Sign"
                } else {
                    "Approve"
                },
                Variant::Dimmed,
                "flex-1",
                None,
            )
        } else {
            btn(
                if phase == Phase::Error {
                    "Retry"
                } else if is_msg {
                    "Sign"
                } else {
                    "Approve"
                },
                Variant::Primary,
                "flex-1",
                Some(Msg::Approve),
            )
            .def(true)
        };
        col = col.kid(div("flex gap-2").kid(main).kid(btn("Cancel", Variant::Secondary, "", Some(Msg::Cancel)).def(blocked)));
        if let Some(m) = pr.method.filter(|m| !m.is_empty()) {
            col = col.kid(p("text-sm text-secondary").text(m));
        }
        vec![col]
    };

    let close = match phase {
        Phase::Working => None,
        Phase::Done => Some(Msg::Done),
        _ if flow_final => Some(Msg::Done),
        _ => Some(Msg::Cancel),
    };
    let body = match (phase, pr.flow.and_then(|f| f.result.as_ref())) {
        (Phase::Done, Some(res)) => done_body(pr, st, res),
        _ if is_msg => message_body(pr, &site),
        _ => tx_body(pr, st, &site),
    };
    sheet(
        SheetSpec {
            title,
            subtitle: (phase != Phase::Done).then(|| format!("{origin_line} · {}", account.map(|a| a.label.as_str()).unwrap_or("Account"))),
            icon: Some(icon_el),
            footer: Some(footer),
            close,
            width: "44rem",
        },
        body,
    )
    .attr("data-ready", format!("{}{}", load_name(pr.load), phase_name(phase)))
}

fn load_name(l: &Load) -> &'static str {
    match l {
        Load::Loading => "loading",
        Load::Ready(_) => "ready",
        Load::Unavailable => "unavailable",
    }
}

fn phase_name(p: Phase) -> &'static str {
    match p {
        Phase::Review => "review",
        Phase::Working => "working",
        Phase::Done => "done",
        Phase::Error => "error",
    }
}

/// Applies a message to the sheet's own state; what the host must do comes back.
pub fn update(st: &mut State, review_failed: bool, msg: &Msg) -> Option<Effect> {
    match msg {
        Msg::Ack(v) => st.acked = *v,
        Msg::Anyway => st.anyway = true,
        Msg::Details => st.details = Some(!st.details.unwrap_or(review_failed)),
        Msg::CopyId(id) => {
            st.copied = true;
            return Some(Effect::Copy(id.clone()));
        }
        Msg::Approve | Msg::Panel(PanelMsg::Retry) => return Some(Effect::Approve),
        Msg::Cancel | Msg::Panel(PanelMsg::Cancel) => return Some(Effect::Cancel),
        Msg::Done => return Some(Effect::Done),
        Msg::Panel(PanelMsg::EnterPin) => return Some(Effect::AskPasskeyPin),
    }
    None
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Sign (the review's bytes when there are any)
    Approve,
    /// Stop the devices and reject the request
    Cancel,
    /// Close the sheet (the next request shows)
    Done,
    /// Copy the transaction id; `copied` goes back off after 1.5 s
    Copy(String),
    /// Ask "Security key PIN" on the keyboard (secret) and hand it to the passkey ceremony
    AskPasskeyPin,
}

// ------------------------------------------------------------------ host

/// What came back from `approveRequest` (the wallet signs, maybe submits).
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// the unlock sheet was cancelled
    NotUnlocked,
    Signed { result: SignResult, failed_on_chain: bool },
    /// signing threw; `still_queued` is false when the sheet's Cancel already removed it
    Threw { message: Option<String>, still_queued: bool },
}

/// `ReviewHost`: the flow around the head of the queue.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Host {
    pub flow: Option<(SignRequest, Flow)>,
}

impl Host {
    /// The request on screen: the one in flight, else the head of the queue.
    pub fn current<'a>(&'a self, head: Option<&'a SignRequest>) -> Option<&'a SignRequest> {
        self.flow.as_ref().map(|(r, _)| r).or(head)
    }

    pub fn flow(&self) -> Option<&Flow> {
        self.flow.as_ref().map(|(_, f)| f)
    }

    /// Approve pressed: the sheet shows its working state until `finish`.
    pub fn start(&mut self, req: &SignRequest) {
        self.flow = Some((req.clone(), Flow { phase: Phase::Working, error: None, final_: false, result: None }));
    }

    pub fn finish(&mut self, out: Outcome) {
        let Some((req, _)) = self.flow.take() else { return };
        self.flow = match out {
            Outcome::NotUnlocked => None,
            Outcome::Signed { failed_on_chain: true, .. } => Some((req, Flow { phase: Phase::Error, error: Some("The network rejected this transaction.".into()), final_: true, result: None })),
            // the Marketplace and games show their own result: no second Done
            Outcome::Signed { .. } if req.origin.kind != OriginKind::Wallet => None,
            Outcome::Signed { result, .. } => Some((req, Flow { phase: Phase::Done, error: None, final_: false, result: Some(result) })),
            Outcome::Threw { still_queued: false, .. } => None,
            Outcome::Threw { message, .. } => Some((req, Flow { phase: Phase::Error, error: Some(message.unwrap_or_else(|| "Could not sign. Try again.".into())), final_: false, result: None })),
        };
    }

    /// Cancel: the devices stop, the request is rejected, the sheet closes.
    pub fn cancel(&mut self) {
        self.flow = None;
    }

    pub fn done(&mut self) {
        self.flow = None;
    }
}

#[cfg(test)]
#[path = "review_tests.rs"]
mod tests;
