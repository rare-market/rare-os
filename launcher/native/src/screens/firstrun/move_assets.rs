//! Moving assets out of the old SuiPlay wallet (launcher/src/onboarding/move/MoveAssets.tsx): your
//! new address (QR and text), the old address, what's in it (a read-only scan), the move plan in
//! signing order, and check again until only items that can't move are left. Used by the
//! wallet's "Move from your old SuiPlay wallet" page; Rare only reads the old wallet.
//!
//! The scan and the plan (onboarding/rescue: scan.ts, plan.ts) come in as data; planning and
//! sponsored sends belong to the wallet side.

use super::kit::{done_line, step, working};
use crate::screens::wallet::fmt::format_amount;
use crate::screens::wallet::markup::*;
use crate::screens::wallet::types::RareAccount;
use crate::screens::wallet::ui::{button_el, empty, BSize, ButtonSpec, BVariant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    To,
    From,
    Scan,
    Plan,
}

const ORDER: [Page; 4] = [Page::To, Page::From, Page::Scan, Page::Plan];
const MAX_ROWS: usize = 5;
const BATCH_ROWS: usize = 8;

/// `short`.
pub fn short(a: &str) -> String {
    crate::screens::wallet::fmt::addr_short(a)
}

/// `grouped`: 0x and then groups of four, easier to compare by eye.
pub fn grouped(a: &str) -> String {
    let hex: Vec<char> = a.chars().skip(2).collect();
    format!("0x {}", hex.chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join(" "))
}

fn sui(mist: i128) -> String {
    format!("{} SUI", format_amount(mist, 9))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Coin {
    pub coin_type: String,
    pub symbol: String,
    pub decimals: u32,
    pub raw: i128,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub object_id: String,
    pub name: String,
    pub collection: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KioskItem {
    pub object_id: String,
    pub name: String,
    pub listed: Option<i128>,
    pub locked: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Kiosk {
    pub kiosk_id: String,
    pub personal: bool,
    pub items: Vec<KioskItem>,
    pub profits: i128,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Name {
    pub object_id: String,
    pub domain: String,
    pub expires_ms: i64,
}

/// rescue/types.ts `RescueScan` (the parts the screen shows).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scan {
    pub default_name: Option<String>,
    pub coins: Vec<Coin>,
    pub nfts: Vec<Item>,
    pub kiosks: Vec<Kiosk>,
    /// principal of each stake, MIST
    pub staked: Vec<i128>,
    pub names: Vec<Name>,
    pub soulbound: Vec<Item>,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub kind: String,
    pub id: String,
    pub label: String,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    pub index: u32,
    pub title: String,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stuck {
    pub id: String,
    pub label: String,
    pub reason: String,
}

/// rescue/types.ts `MovePlan`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub to: String,
    pub batches: Vec<Batch>,
    pub stuck: Vec<Stuck>,
    pub empty: bool,
    pub fee_note: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub page: Page,
    /// the old wallet's address (remembered on this device)
    pub from: Option<String>,
    /// the scan's progress line while it runs
    pub stage: Option<String>,
    pub error: Option<String>,
    pub checks: u32,
    /// per batch: "sending", "sent" or the error (sponsored moves)
    pub sent: Vec<(u32, String)>,
}

impl State {
    pub fn new(saved_from: Option<String>) -> State {
        State { page: Page::To, from: saved_from, stage: None, error: None, checks: 0, sent: Vec::new() }
    }
}

pub struct Props<'a> {
    /// the current account; None while loading (Some(None) none at all)
    pub account: Option<Option<&'a RareAccount>>,
    pub scan: Option<&'a Scan>,
    pub plan: Option<&'a Plan>,
    /// Rare covers the fees for these moves (sponsored, sent from here)
    pub covered: bool,
    pub exit_label: &'a str,
    pub has_back: bool,
    pub tz_min: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Go(Page),
    Exit,
    NeedWallet,
    EnterFrom,
    ScanFrom,
    ScanAgain,
    CheckAgain,
    SendCovered(u32),
    Back,
}

fn to_address<'a>(pr: &Props<'a>) -> Option<&'a str> {
    pr.account.flatten().filter(|a| a.kind != "watch").map(|a| a.address.as_str())
}

fn btn_l(label: &str, v: BVariant, def: bool, m: Msg) -> El<Msg> {
    button_el(ButtonSpec::new(label).v(v).size(BSize::Lg).def(def), Some(m))
}

pub fn view(pr: &Props, st: &State) -> El<Msg> {
    let to = to_address(pr);
    let body = match st.page {
        Page::To => {
            let actions = match (pr.account, to) {
                (None, _) => vec![],
                (Some(_), Some(_)) => vec![btn_l("Continue", BVariant::Primary, true, Msg::Go(Page::From)), btn_l("I don't have an old wallet", BVariant::Ghost, false, Msg::Exit)],
                (Some(_), None) => vec![
                    btn_l("Set up your wallet", BVariant::Primary, true, Msg::NeedWallet),
                    btn_l("Just look at my old wallet", BVariant::Secondary, false, Msg::Go(Page::From)),
                    btn_l("I don't have one", BVariant::Ghost, false, Msg::Exit),
                ],
            };
            let content = match (pr.account, to) {
                (Some(Some(a)), Some(t)) => vec![div("flex items-start gap-[2.5rem]").kid(el(Tag::Qr(t.to_string()), "h-[13rem] w-[13rem] shrink-0")).kid(
                    div("min-w-0 flex-1")
                        .kid(div("text-[0.8rem] font-medium uppercase tracking-[0.1em] text-muted").text("Your new address"))
                        .kid(div("mt-[0.3rem] text-[1.05rem] font-semibold").text(&a.label))
                        .kid(div("mt-[0.6rem] max-w-[30rem] break-words font-mono [font-variant-ligatures:none] text-[1.02rem] leading-[1.7] text-fg/90").text(grouped(t)))
                        .kid(p("mt-[1rem] max-w-[34rem] text-[0.9rem] leading-relaxed text-muted").text("Everything from your old SuiPlay wallet goes here. Next, Rare reads what's in the old wallet and lists exactly what to send, in order.")),
                )],
                (Some(acct), None) => vec![empty(
                    "First, a wallet you control",
                    Some(text(match acct {
                        Some(a) => format!("{} is watch-only, so it can't receive with a key you hold. Create a passkey wallet to move your assets into.", a.label),
                        None => "Your assets need somewhere to go: a Rare Wallet signed in with your passkey. You can still look at what's in your old wallet first.".into(),
                    })),
                    vec![],
                    "",
                )],
                _ => vec![],
            };
            step(
                text("Move your assets to a wallet you control"),
                Some(text("The wallet your SuiPlay came with signs through its maker's sign-in. If that service stops, what's in it can't move anymore. Moving it now keeps it yours.")),
                content,
                actions,
                true,
            )
        }
        Page::From => {
            let mut actions = vec![btn_l(if st.from.is_some() { "Change address" } else { "Enter address" }, BVariant::Primary, true, Msg::EnterFrom)];
            if let Some(f) = &st.from {
                actions.push(btn_l(&format!("Scan {}", short(f)), BVariant::Secondary, false, Msg::ScanFrom));
            }
            let mut content = vec![match &st.from {
                Some(f) => div("max-w-[34rem] break-words font-mono [font-variant-ligatures:none] text-[1.02rem] leading-[1.7] text-fg/90").text(grouped(f)),
                None => div("text-[0.95rem] text-muted").text("A 0x address, or a name ending in .sui if you set one."),
            }];
            if let Some(e) = &st.error {
                content.push(p("mt-[1rem] text-small text-neg").text(e));
            }
            step(
                text("Which wallet came with your SuiPlay?"),
                Some(text("Enter its address. On the SuiPlay's original system it's under Wallet, at the top. Rare only reads it: nothing moves until you approve it in that wallet.")),
                content,
                actions,
                true,
            )
        }
        Page::Scan => {
            let actions = if st.stage.is_some() {
                vec![]
            } else if pr.scan.is_some() {
                let first = match to {
                    Some(_) => button_el(ButtonSpec::new("See the move plan").v(BVariant::Primary).size(BSize::Lg).def(true).disabled(pr.plan.is_none_or(|p| p.empty)), Some(Msg::Go(Page::Plan))),
                    None => btn_l("Set up your wallet to move these", BVariant::Primary, true, Msg::NeedWallet),
                };
                vec![first, btn_l("Scan again", BVariant::Secondary, false, Msg::ScanFrom), btn_l("Different wallet", BVariant::Ghost, false, Msg::Go(Page::From))]
            } else {
                vec![btn_l("Try again", BVariant::Primary, true, Msg::ScanFrom), btn_l("Different wallet", BVariant::Ghost, false, Msg::Go(Page::From))]
            };
            let body = st.from.as_ref().map(|f| {
                let mut b = span("").kid(text("Wallet ")).kid(span("num text-fg/80").text(short(f)));
                if let Some(n) = pr.scan.and_then(|s| s.default_name.as_ref()) {
                    b = b.kid(text(format!(" ({n})")));
                }
                b.kid(text(". Read from the Sui network; nothing has moved."))
            });
            let mut content = Vec::new();
            if let Some(s) = &st.stage {
                content.push(working(true, Some(s)));
            }
            if let Some(e) = &st.error {
                content.push(p("text-small text-neg").text(e));
            }
            if let (Some(s), None) = (pr.scan, &st.stage) {
                content.push(scan_results(s, pr.plan, pr.tz_min));
            }
            step(text(if st.stage.is_some() { "Reading your old wallet" } else { "What's in your old wallet" }), body, content, actions, true)
        }
        Page::Plan => {
            let (Some(plan), Some(_)) = (pr.plan, pr.scan) else { return div("h-full") };
            let title = if plan.empty {
                if st.checks > 0 {
                    "Everything that can move has moved"
                } else {
                    "Nothing to move"
                }
            } else {
                "Send these, in this order"
            };
            let body = if plan.empty {
                if plan.stuck.is_empty() {
                    "The old wallet is empty. Rare keeps an eye on it in case something new arrives."
                } else {
                    "What's left can't be sent by anyone but its own app. Rare keeps an eye on the old wallet in case something new arrives."
                }
            } else {
                "Sign these in your old SuiPlay wallet while it still works. Each one sends to your new address. Come back here and check again when you're done."
            };
            let mut actions = vec![if plan.empty {
                btn_l(pr.exit_label, BVariant::Primary, true, Msg::Exit)
            } else {
                button_el(ButtonSpec::new("Check again").v(BVariant::Primary).size(BSize::Lg).def(true).disabled(st.stage.is_some()), Some(Msg::CheckAgain))
            }];
            if !plan.empty {
                actions.push(btn_l("Finish later", BVariant::Ghost, false, Msg::Exit));
            }
            let mut c = div("flex flex-col gap-[1rem]");
            if let Some(s) = &st.stage {
                c = c.kid(working(true, Some(s)));
            }
            if plan.empty && st.checks > 0 {
                c = c.kid(done_line(vec![text(format!("Moved to {}", short(&plan.to)))]));
            }
            if let Some(n) = plan.fee_note.as_ref().filter(|_| !plan.empty) {
                c = c.kid(p("text-small text-warn").text(n));
            }
            if pr.covered && !plan.empty {
                c = c.kid(p("text-small text-muted").text("Rare covers the fees for these moves."));
            }
            if !plan.empty {
                c = c.kid(
                    div("grid grid-cols-[minmax(0,1fr)_15rem] items-start gap-[1.5rem]")
                        .kid(ol("flex flex-col gap-[0.6rem]").attr("data-nav-group", "move-batches").kids(plan.batches.iter().map(|b| batch(b, pr.covered, st))))
                        .kid(
                            div("flex flex-col items-center rounded-card border border-hair bg-l1 p-[1rem] text-center")
                                .kid(el(Tag::Qr(plan.to.clone()), "h-[11rem] w-[11rem]"))
                                .kid(div("mt-[0.7rem] text-[0.85rem] font-semibold").text("Send to"))
                                .kid(div("mt-[0.2rem] break-all font-mono [font-variant-ligatures:none] text-[0.72rem] leading-relaxed text-muted").text(&plan.to)),
                        ),
                );
                c = c.kid(p("max-w-[44rem] text-[0.86rem] leading-relaxed text-muted").text(
                    "Already on Rare OS? The original system stays on this SuiPlay in the boot menu: hold Shift while it powers on (with a USB-C keyboard), pick it, send, then restart into Rare OS.",
                ));
            }
            if !plan.stuck.is_empty() {
                let mut s = div("flex flex-col gap-[0.4rem]").kid(h2("text-[0.92rem] font-semibold").text("Stays behind"));
                for x in plan.stuck.iter().take(BATCH_ROWS) {
                    s = s.kid(div("text-[0.86rem]").key(&x.id).kid(span("text-fg/90").text(&x.label)).kid(span("text-muted").text(format!(": {}", x.reason))));
                }
                if plan.stuck.len() > BATCH_ROWS {
                    s = s.kid(div("text-[0.82rem] text-muted").text(format!("and {} more", plan.stuck.len() - BATCH_ROWS)));
                }
                c = c.kid(s);
            }
            step(text(title), Some(text(body)), vec![c], actions, true)
        }
    };
    let mut root = div("h-full").kid(body);
    if st.page != Page::To || pr.has_back {
        root = root.on_pad("b", Msg::Back);
    }
    root
}

fn batch(b: &Batch, covered: bool, st: &State) -> El<Msg> {
    let sent = st.sent.iter().find(|(i, _)| *i == b.index).map(|(_, s)| s.as_str());
    let mut lines = span("mt-[0.3rem] flex flex-col gap-[0.15rem]");
    for l in b.lines.iter().take(BATCH_ROWS) {
        lines = lines.kid(span("flex gap-[0.6rem] text-[0.86rem]").key(format!("{}{}", l.kind, l.id)).kid(span("min-w-0 truncate text-fg/90").text(&l.label)).kid_opt(l.detail.as_ref().map(|d| span("min-w-0 truncate text-muted").text(d))));
    }
    if b.lines.len() > BATCH_ROWS {
        lines = lines.kid(span("text-[0.82rem] text-muted").text(format!("and {} more", b.lines.len() - BATCH_ROWS)));
    }
    let mut mid = span("min-w-0 flex-1").kid(span("block text-[0.98rem] font-semibold").text(&b.title)).kid(lines);
    if let Some(err) = sent.filter(|s| covered && *s != "sending" && *s != "sent") {
        mid = mid.kid(span("mt-[0.3rem] block text-[0.82rem] text-neg").text(err));
    }
    let mut card = div("flex gap-[1rem] rounded-card border border-hair bg-l1 px-[1.2rem] py-[1rem]")
        .kid(span("num flex h-[2rem] w-[2rem] shrink-0 items-center justify-center rounded-full bg-l4 text-[0.9rem] font-semibold").text(b.index.to_string()))
        .kid(mid);
    if covered {
        card = card.kid(span("shrink-0 self-center").kid(if sent == Some("sent") {
            span("text-[0.86rem] text-muted").text("Sent")
        } else {
            button_el(ButtonSpec::new(if sent == Some("sending") { "Sending" } else { "Send" }).size(BSize::Sm).disabled(sent == Some("sending")), Some(Msg::SendCovered(b.index)))
        }));
    }
    // the card itself is focusable (data-nav tabIndex 0): A does nothing on it
    card.nav = Some(Nav { on: None, default: false, disabled: false, hint_a: None });
    li("").key(b.index.to_string()).kid(card)
}

fn section_box(title: &str, right: Option<String>, rows: Vec<El<Msg>>) -> El<Msg> {
    let mut s = el(Tag::Section, "flex flex-col rounded-card border border-hair bg-l1 px-[1.2rem] py-[1rem]").kid(div("mb-[0.5rem] flex items-baseline gap-[0.6rem]").kid(h2("text-[0.98rem] font-semibold").text(title)).kid_opt(right.map(|r| span("text-[0.82rem] text-muted").text(r)))).kid(div("flex flex-col gap-[0.25rem] text-[0.88rem]").kids(rows));
    s.nav = Some(Nav { on: None, default: false, disabled: false, hint_a: None });
    s
}

fn row_el(left: El<Msg>, right: Option<String>) -> El<Msg> {
    div("flex items-baseline gap-[0.8rem]").kid(span("min-w-0 flex-1 truncate text-fg/90").kid(left)).kid_opt(right.map(|r| span("num shrink-0 text-muted").text(r)))
}

fn more(n: usize) -> Option<El<Msg>> {
    (n > MAX_ROWS).then(|| div("text-[0.82rem] text-muted").text(format!("and {} more", n - MAX_ROWS)))
}

fn scan_results(scan: &Scan, plan: Option<&Plan>, tz_min: i32) -> El<Msg> {
    let kiosk_items: usize = scan.kiosks.iter().map(|k| k.items.len()).sum();
    let staked: i128 = scan.staked.iter().sum();
    let nothing = scan.coins.is_empty() && scan.nfts.is_empty() && scan.kiosks.is_empty() && scan.staked.is_empty() && scan.names.is_empty();
    let mut d = div("flex flex-col gap-[1rem]");
    if nothing {
        d = d.kid(empty("Nothing to move", Some(text(if scan.soulbound.is_empty() { "This wallet is empty." } else { "Only items that can't be sent are in this wallet." })), vec![], ""));
    }
    let mut g = div("grid grid-cols-2 items-start gap-[0.9rem]").attr("data-nav-group", "scan-sections");
    if !scan.coins.is_empty() {
        let mut rows: Vec<El<Msg>> = scan.coins.iter().take(MAX_ROWS).map(|c| row_el(text(&c.symbol), Some(format_amount(c.raw, c.decimals)))).collect();
        rows.extend(more(scan.coins.len()));
        g = g.kid(section_box("Coins", Some(scan.coins.len().to_string()), rows));
    }
    if !scan.nfts.is_empty() {
        let mut rows: Vec<El<Msg>> = scan.nfts.iter().take(MAX_ROWS).map(|n| row_el(text(&n.name), Some(n.collection.clone()))).collect();
        rows.extend(more(scan.nfts.len()));
        g = g.kid(section_box("Collectibles", Some(scan.nfts.len().to_string()), rows));
    }
    if !scan.kiosks.is_empty() {
        let rows = scan
            .kiosks
            .iter()
            .map(|k| {
                let mut b = div("flex flex-col gap-[0.2rem]")
                    .key(&k.kiosk_id)
                    .kid(row_el(text(format!("{} {}", if k.personal { "Personal kiosk" } else { "Kiosk" }, short(&k.kiosk_id))), (k.profits > 0).then(|| format!("{} in sales", sui(k.profits)))));
                for it in k.items.iter().take(MAX_ROWS) {
                    let parts: Vec<String> = [it.listed.map(|l| format!("Listed {}", sui(l))), it.locked.then(|| "Locked".to_string())].into_iter().flatten().collect();
                    b = b.kid(row_el(span("pl-[0.9rem] text-fg/80").text(&it.name), (!parts.is_empty()).then(|| parts.join(", "))));
                }
                b.kid_opt(more(k.items.len()))
            })
            .collect();
        g = g.kid(section_box("Kiosks", Some(format!("{}, {kiosk_items} items", scan.kiosks.len())), rows));
    }
    if !scan.staked.is_empty() {
        g = g.kid(section_box("Staked SUI", Some(scan.staked.len().to_string()), vec![row_el(text("Total staked"), Some(sui(staked))), div("text-[0.82rem] text-muted").text("Moves as it is and keeps earning.")]));
    }
    if !scan.names.is_empty() {
        let mut rows: Vec<El<Msg>> = scan
            .names
            .iter()
            .take(MAX_ROWS)
            .map(|n| {
                let l = crate::screens::wallet::fmt::local(n.expires_ms, tz_min);
                row_el(text(&n.domain), Some(format!("Until {} {}", crate::screens::wallet::fmt::MONTHS[(l.month - 1) as usize], l.year)))
            })
            .collect();
        rows.extend(more(scan.names.len()));
        g = g.kid(section_box("SuiNS names", Some(scan.names.len().to_string()), rows));
    }
    if !scan.soulbound.is_empty() {
        let stuck: Vec<Stuck> = match plan {
            Some(p) => p.stuck.clone(),
            None => scan.soulbound.iter().map(|s| Stuck { id: s.object_id.clone(), label: s.name.clone(), reason: String::new() }).collect(),
        };
        let mut rows: Vec<El<Msg>> = stuck
            .iter()
            .take(MAX_ROWS)
            .map(|s| div("").key(&s.id).kid(div("truncate text-fg/90").text(&s.label)).kid_if(!s.reason.is_empty(), || div("text-[0.8rem] leading-relaxed text-muted").text(&s.reason)))
            .collect();
        rows.extend(more(scan.soulbound.len()));
        g = g.kid(section_box("Can't move", Some(scan.soulbound.len().to_string()), rows));
    }
    d = d.kid(g);
    if scan.truncated {
        d = d.kid(p("text-[0.86rem] text-muted").text("This wallet holds more objects than Rare reads at once. Move these first, then check again for the rest."));
    }
    d
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Exit,
    NeedWallet,
    /// keyboard "Your old SuiPlay wallet address" ("0x... or name.sui"), resolve, then `from_entered`
    AskFrom(Option<String>),
    /// read the old wallet (again = a check after moving)
    Scan { address: String, again: bool },
    /// stop a running scan
    Abort,
    SendCovered(u32),
    /// B on the first page with somewhere to go
    Back,
}

pub fn update(pr: &Props, st: &mut State, msg: Msg) -> Option<Effect> {
    match msg {
        Msg::Go(p) => {
            st.page = p;
            None
        }
        Msg::Exit => Some(Effect::Exit),
        Msg::NeedWallet => Some(Effect::NeedWallet),
        Msg::EnterFrom => Some(Effect::AskFrom(st.from.clone())),
        Msg::ScanFrom => {
            let f = st.from.clone()?;
            st.page = Page::Scan;
            run(st, f, false)
        }
        Msg::ScanAgain => {
            let f = st.from.clone()?;
            run(st, f, false)
        }
        Msg::CheckAgain => {
            let f = st.from.clone()?;
            run(st, f, true)
        }
        Msg::SendCovered(i) => {
            st.sent.retain(|(x, _)| *x != i);
            st.sent.push((i, "sending".into()));
            Some(Effect::SendCovered(i))
        }
        Msg::Back => {
            let i = ORDER.iter().position(|p| *p == st.page).unwrap_or(0);
            if i == 0 {
                return pr.has_back.then_some(Effect::Back);
            }
            st.stage = None;
            st.page = ORDER[i - 1];
            Some(Effect::Abort)
        }
    }
}

fn run(st: &mut State, address: String, again: bool) -> Option<Effect> {
    st.error = None;
    st.stage = Some("Starting".into());
    Some(Effect::Scan { address, again })
}

/// The resolved address from the keyboard (or the error).
pub fn from_entered(pr: &Props, st: &mut State, res: Result<String, ()>) -> Option<Effect> {
    match res {
        Ok(a) => {
            if to_address(pr) == Some(a.as_str()) {
                st.error = Some("That's your new wallet. Enter the address of the wallet your SuiPlay came with.".into());
                return None;
            }
            st.from = Some(a.clone());
            st.page = Page::Scan;
            run(st, a, false)
        }
        Err(()) => {
            st.error = Some(crate::screens::wallet::fmt::failed("Couldn't open that wallet."));
            None
        }
    }
}

/// The scan finished (the host holds the scan and plans the moves); `again` counts a check.
pub fn scanned(st: &mut State, ok: bool, again: bool) {
    st.stage = None;
    if ok {
        if again {
            st.checks += 1;
        }
    } else {
        st.error = Some("Couldn't read that wallet. Check the address and try again.".into());
    }
}

/// A sponsored move's answer: sent, or the plain error.
pub fn covered_sent(st: &mut State, index: u32, res: Result<(), String>) {
    st.sent.retain(|(x, _)| *x != index);
    st.sent.push((index, res.err().unwrap_or_else(|| "sent".into())));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::home::fixtures::{account, ME};

    const OLD: &str = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";

    fn scan() -> Scan {
        Scan {
            default_name: Some("old.sui".into()),
            coins: vec![Coin { coin_type: "0x2::sui::SUI".into(), symbol: "SUI".into(), decimals: 9, raw: 12_500_000_000 }],
            nfts: vec![Item { object_id: "0x1".into(), name: "SuiFren #1".into(), collection: "SuiFrens".into() }],
            kiosks: vec![Kiosk { kiosk_id: "0x00000000000000000000000000000000000000000000000000000000000000k1".into(), personal: true, items: vec![KioskItem { object_id: "0x2".into(), name: "Popkin".into(), listed: Some(3_000_000_000), locked: true }], profits: 0 }],
            staked: vec![1_000_000_000],
            names: vec![],
            soulbound: vec![Item { object_id: "0x9".into(), name: "Badge".into(), collection: "Quests".into() }],
            truncated: false,
        }
    }

    #[test]
    fn walk_the_pages() {
        let acct = account();
        let s = scan();
        let plan = Plan {
            to: ME.into(),
            batches: vec![Batch { index: 1, title: "Coins and collectibles".into(), lines: vec![Line { kind: "coin".into(), id: "sui".into(), label: "12.5 SUI".into(), detail: None }] }],
            stuck: vec![Stuck { id: "0x9".into(), label: "Badge".into(), reason: "Only Quests can move it.".into() }],
            empty: false,
            fee_note: None,
        };
        let pr = Props { account: Some(Some(&acct)), scan: Some(&s), plan: Some(&plan), covered: false, exit_label: "Done", has_back: false, tz_min: 0 };
        let mut st = State::new(Some(OLD.into()));
        let v = view(&pr, &st);
        assert_eq!(v.texts()[0], "Move your assets to a wallet you control");
        assert!(v.contains_text("Your new address"));
        assert!(v.contains_text(&grouped(ME)));
        assert_eq!(v.landing().as_deref(), Some("Continue"));
        assert_eq!(v.pad("b"), None);
        update(&pr, &mut st, Msg::Go(Page::From));
        let v = view(&pr, &st);
        assert!(v.find_nav("Scan 0xa049...887e").is_some());
        assert_eq!(update(&pr, &mut st, Msg::ScanFrom), Some(Effect::Scan { address: OLD.into(), again: false }));
        assert!(view(&pr, &st).contains_text("Reading your old wallet"));
        scanned(&mut st, true, false);
        let v = view(&pr, &st);
        let t = v.texts();
        assert!(t.contains(&"What's in your old wallet".to_string()));
        assert!(t.contains(&" (old.sui)".to_string()));
        assert!(t.contains(&"Personal kiosk 0x0000...00k1".to_string()));
        assert!(t.contains(&"Listed 3 SUI, Locked".to_string()));
        assert!(t.contains(&"Only Quests can move it.".to_string()));
        update(&pr, &mut st, Msg::Go(Page::Plan));
        let v = view(&pr, &st);
        assert!(v.contains_text("Send these, in this order"));
        assert!(v.contains_text("Stays behind"));
        assert_eq!(update(&pr, &mut st, Msg::CheckAgain), Some(Effect::Scan { address: OLD.into(), again: true }));
        scanned(&mut st, true, true);
        assert_eq!(st.checks, 1);
        assert_eq!(update(&pr, &mut st, Msg::Back), Some(Effect::Abort));
        assert_eq!(st.page, Page::Scan);
    }

    #[test]
    fn no_wallet_and_errors() {
        let w = RareAccount::new("0x1", "Demo wallet", "watch");
        let pr = Props { account: Some(Some(&w)), scan: None, plan: None, covered: false, exit_label: "Done", has_back: true, tz_min: 0 };
        let mut st = State::new(None);
        let v = view(&pr, &st);
        assert!(v.contains_text("Demo wallet is watch-only, so it can't receive with a key you hold. Create a passkey wallet to move your assets into."));
        assert_eq!(v.landing().as_deref(), Some("Set up your wallet"));
        assert_eq!(update(&pr, &mut st, Msg::Back), Some(Effect::Back));
        let acct = account();
        let pr2 = Props { account: Some(Some(&acct)), ..pr };
        assert_eq!(from_entered(&pr2, &mut st, Ok(acct.address.clone())), None);
        st.page = Page::From;
        assert!(view(&pr2, &st).contains_text("That's your new wallet. Enter the address of the wallet your SuiPlay came with."));
        assert_eq!(grouped("0xabcdef12"), "0x abcd ef12");
    }
}
