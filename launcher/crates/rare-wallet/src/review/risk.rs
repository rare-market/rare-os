//! Plain risk lines for the review sheet (ports `review/risk.ts`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::analyze::{CoinFlow, Insights, ObjectFlowKind};
use super::fees::known_fee;
use super::packages::{known_package, short_id as sid};

/// Short id that never breaks across lines (word joiners around the ellipsis).
fn short_id(id: &str) -> String {
    sid(id).replacen('\u{2026}', "\u{2060}\u{2026}\u{2060}", 1)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Risk {
    pub id: String,
    pub text: String,
    /// Red: real danger. Everything else renders muted.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub danger: bool,
    /// The user must tick an extra confirmation before signing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub requires_confirm: bool,
}

impl Risk {
    fn muted(id: impl Into<String>, text: impl Into<String>) -> Self {
        Risk { id: id.into(), text: text.into(), danger: false, requires_confirm: false }
    }
    fn danger(id: impl Into<String>, text: impl Into<String>) -> Self {
        Risk { id: id.into(), text: text.into(), danger: true, requires_confirm: false }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contact {
    pub address: String,
    pub name: String,
}

fn yes() -> bool {
    true
}

/// Addresses this wallet already knows, for new-recipient and lookalike checks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressHistory {
    /// Addresses this wallet has sent to.
    pub sent_to: Vec<String>,
    /// Addresses that have sent to this wallet.
    pub received_from: Vec<String>,
    /// Saved contacts.
    pub contacts: Vec<Contact>,
    /// False for flows where fees go to protocol addresses (swaps, lending): skip the first-time line.
    #[serde(default = "yes")]
    pub warn_new: bool,
}

impl Default for AddressHistory {
    fn default() -> Self {
        AddressHistory { sent_to: vec![], received_from: vec![], contacts: vec![], warn_new: true }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenFlags {
    pub spam: bool,
    pub honeypot: bool,
}

/// What the risk checks know beyond the insights.
pub struct RiskContext<'a> {
    /// USD per whole token, by coin type.
    pub prices: BTreeMap<String, Option<f64>>,
    pub decimals: Box<dyn Fn(&str) -> u32 + 'a>,
    pub symbol: Box<dyn Fn(&str) -> String + 'a>,
    /// The signer's portfolio value in USD; None when unknown.
    pub portfolio_usd: Option<f64>,
    /// Spam/honeypot flags per coin type (received coins).
    pub safety: BTreeMap<String, Option<TokenFlags>>,
    /// Called package id -> original id.
    pub originals: Option<BTreeMap<String, String>>,
    /// History for recipient checks; None skips them.
    pub addresses: Option<AddressHistory>,
    /// The signer's own address; never flagged as a recipient.
    pub self_address: Option<String>,
}

impl<'a> RiskContext<'a> {
    /// No prices, 9 decimals, the type's last segment as the symbol, nothing else known.
    pub fn new() -> Self {
        RiskContext {
            prices: BTreeMap::new(),
            decimals: Box::new(|_| 9),
            symbol: Box::new(|t: &str| t.rsplit("::").next().unwrap_or("?").to_string()),
            portfolio_usd: None,
            safety: BTreeMap::new(),
            originals: None,
            addresses: None,
            self_address: None,
        }
    }
}

impl Default for RiskContext<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// `10 ** d` as JS computes it for small integer d.
fn pow10(d: u32) -> f64 {
    format!("1e{d}").parse().unwrap_or(f64::INFINITY)
}

fn to_float(amount: i128, decimals: u32) -> f64 {
    amount as f64 / pow10(decimals)
}

fn price_of(ctx: &RiskContext, t: &str) -> Option<f64> {
    ctx.prices.get(t).copied().flatten()
}

/// USD value of a list of coin flows; None when any price is missing.
pub fn flow_usd(flows: &[CoinFlow], ctx: &RiskContext) -> Option<f64> {
    let mut sum = 0.0;
    for f in flows {
        let p = price_of(ctx, &f.coin_type)?;
        sum += to_float(f.amount, (ctx.decimals)(&f.coin_type)) * p;
    }
    Some(sum)
}

/// USD of the flows that have a price; unpriced coins count as 0.
pub fn priced_usd(flows: &[CoinFlow], ctx: &RiskContext) -> f64 {
    flows.iter().fold(0.0, |s, f| s + price_of(ctx, &f.coin_type).map_or(0.0, |p| to_float(f.amount, (ctx.decimals)(&f.coin_type)) * p))
}

fn norm(a: &str) -> String {
    a.trim().to_lowercase()
}

fn ends(a: &str) -> (String, String) {
    let n = norm(a);
    let body = n.strip_prefix("0x").unwrap_or(&n);
    let head: String = body.chars().take(4).collect();
    let c: Vec<char> = n.chars().collect();
    let tail: String = c[c.len().saturating_sub(4)..].iter().collect();
    (head, tail)
}

/// `0xabcd…1234`: the first and last 4 hex chars, as the user would compare them.
pub fn lookalike_short(a: &str) -> String {
    let (h, t) = ends(a);
    format!("0x{h}\u{2026}{t}")
}

fn push_unique(v: &mut Vec<String>, s: String) {
    if !v.contains(&s) {
        v.push(s);
    }
}

/// Checks on addresses that would receive coins or objects: lookalikes of known addresses,
/// first-time recipients, saved names.
pub fn address_checks(recipients: &[String], h: Option<&AddressHistory>, self_address: Option<&str>) -> Vec<Risk> {
    let Some(h) = h else { return vec![] };
    let mut out = vec![];
    let sent: Vec<String> = h.sent_to.iter().map(|a| norm(a)).collect();
    let mut contacts: Vec<(String, String)> = vec![];
    for c in &h.contacts {
        let k = norm(&c.address);
        match contacts.iter_mut().find(|(a, _)| *a == k) {
            Some(e) => e.1 = c.name.clone(),
            None => contacts.push((k, c.name.clone())),
        }
    }
    let mut known: Vec<String> = vec![];
    for a in sent.iter().cloned().chain(h.received_from.iter().map(|a| norm(a))).chain(contacts.iter().map(|(a, _)| a.clone())) {
        push_unique(&mut known, a);
    }
    let mut raws: Vec<String> = vec![];
    for r in recipients {
        push_unique(&mut raws, norm(r));
    }
    let self_n = self_address.filter(|s| !s.is_empty()).map(norm);
    for raw in raws {
        if self_n.as_deref() == Some(raw.as_str()) {
            continue;
        }
        let e = ends(&raw);
        let twin = known.iter().find(|k| **k != raw && ends(k) == e);
        let name = contacts.iter().find(|(a, _)| *a == raw).map(|(_, n)| n);
        if let Some(twin) = twin {
            out.push(Risk {
                id: format!("lookalike-{raw}"),
                danger: true,
                requires_confirm: true,
                text: format!("Looks like {} you've used before, but it's a different address.", lookalike_short(twin)),
            });
        } else if let Some(name) = name {
            out.push(Risk::muted(format!("contact-{raw}"), format!("Sending to {name}, a saved contact.")));
        } else if h.warn_new && !sent.contains(&raw) {
            out.push(Risk::muted(format!("new-recipient-{raw}"), "First time sending to this address."));
        }
    }
    out
}

fn plural(n: usize, one: &str) -> String {
    format!("{n} {}", if n == 1 { one.to_string() } else { format!("{one}s") })
}

/// `Math.round` (ties toward +infinity).
fn js_round(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 {
        f + 1.0
    } else {
        f
    }
}

/// Plain risk lines for the review screen, most serious first.
pub fn risk_checks(ins: &Insights, ctx: &RiskContext) -> Vec<Risk> {
    let mut out: Vec<Risk> = vec![];
    if !ins.ok {
        out.push(Risk::danger("failed", "Simulation failed. If sent, it fails and still costs a fee."));
    }

    // giving away objects to another address is the classic drainer pattern
    let given: Vec<_> = ins.objects_out.iter().filter(|o| o.kind == ObjectFlowKind::Sent).collect();
    if !given.is_empty() {
        let mut to: Vec<String> = vec![];
        for o in &given {
            push_unique(&mut to, o.to.clone().unwrap_or_default());
        }
        let whom = if to.len() == 1 { short_id(&to[0]) } else { format!("{} other addresses", to.len()) };
        out.push(Risk::danger("gives-objects", format!("Gives {} to {whom}.", plural(given.len(), "object"))));
    }

    // large share of the portfolio leaving
    let out_usd = priced_usd(&ins.send, ctx);
    if let Some(p) = ctx.portfolio_usd.filter(|p| *p != 0.0 && !p.is_nan()) {
        if p > 0.0 && out_usd / p > 0.5 {
            let pct = js_round(out_usd / p * 100.0).min(100.0);
            out.push(Risk::danger("large-share", format!("Sends about {pct}% of your portfolio.")));
        }
    }

    let flags = |t: &str| ctx.safety.get(t).copied().flatten();
    for r in &ins.receive {
        match flags(&r.coin_type) {
            Some(s) if s.honeypot => out.push(Risk::danger(format!("honeypot-{}", r.coin_type), format!("{} may be a honeypot. You may not be able to sell it.", (ctx.symbol)(&r.coin_type)))),
            Some(s) if s.spam => out.push(Risk::muted(format!("spam-{}", r.coin_type), format!("{} is flagged as spam.", (ctx.symbol)(&r.coin_type)))),
            _ => {}
        }
    }
    for r in &ins.send {
        if let Some(s) = flags(&r.coin_type).filter(|s| s.spam || s.honeypot) {
            out.push(Risk::muted(
                format!("spam-out-{}", r.coin_type),
                format!("{} is flagged as {}.", (ctx.symbol)(&r.coin_type), if s.honeypot { "a honeypot" } else { "spam" }),
            ));
        }
    }

    // coins to other addresses (protocol fees, payments): worth seeing, not alarming
    // Rare's own fee wallets show as a fee line, not as "sends coins to someone"
    let mut coin_to: Vec<String> = vec![];
    for o in ins.others_receive.iter().filter(|o| known_fee(&o.address).is_none()) {
        push_unique(&mut coin_to, o.address.clone());
    }
    if !coin_to.is_empty() {
        let whom = if coin_to.len() == 1 { short_id(&coin_to[0]) } else { format!("{} other addresses", coin_to.len()) };
        out.push(Risk::muted("pays-others", format!("Sends coins to {whom}, not you.")));
    }

    // lookalikes first: they are the address-poisoning tell
    let recipients: Vec<String> = ins.recipients.iter().filter(|r| known_fee(r).is_none()).cloned().collect();
    let addr = address_checks(&recipients, ctx.addresses.as_ref(), ctx.self_address.as_deref());
    let (bad, good): (Vec<Risk>, Vec<Risk>) = addr.into_iter().partition(|r| r.danger);
    out.splice(0..0, bad);
    let mut unknown: Vec<String> = vec![];
    for c in &ins.calls {
        push_unique(&mut unknown, c.package.clone());
    }
    unknown.retain(|p| known_package(p, ctx.originals.as_ref()).is_none());
    if !unknown.is_empty() {
        out.push(Risk::muted(
            "unknown-package",
            if unknown.len() == 1 { format!("Uses a contract Rare Wallet doesn't recognize: {}.", short_id(&unknown[0])) } else { format!("Uses {} contracts Rare Wallet doesn't recognize.", unknown.len()) },
        ));
    }

    let moved = ins.objects_out.iter().filter(|o| o.kind != ObjectFlowKind::Sent).count();
    if moved > 0 {
        out.push(Risk::muted("objects-leave", format!("{} leave{} your wallet.", plural(moved, "object"), if moved == 1 { "s" } else { "" })));
    }
    out.extend(good);
    if ins.publishes {
        out.push(Risk::muted("publish", "Publishes or upgrades a Move package."));
    }
    if !ins.fee_paid_by_signer {
        out.push(Risk::muted("sponsored", "Someone else pays the network fee."));
    }
    out
}
