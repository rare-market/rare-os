//! Send coins (launcher/src/wallet/ui/pages/Send.tsx): pick the coin, type the amount on the pad,
//! pick or type the recipient, Review. Review builds the transfer and hands it to the signing
//! queue; the global review sheet takes over (simulation, PIN, device or phone), and a sent coin
//! lands back on home.

use crate::screens::wallet::fmt::{format_units, norm_type, sui_type, usd};
use crate::screens::wallet::home::lib::{hint, is_sui_type, Hint};
use crate::screens::wallet::home::page::PortfolioLoad;
use crate::screens::wallet::home::stack::WalletView;
use crate::screens::wallet::kit::{avatar, btn, btn_full, field, list_button, page, panel, sheet, FieldSpec, SheetSpec, Size, Variant};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::sign::amount::{amount_pad, base_to_usd, chip_amount, pad_press, parse_units, to_input, usd_to_base, SUI_GAS_RESERVE};
use crate::screens::wallet::sign::recipient::{recipient_line, recipient_picks, Recent, RecipientState};
use crate::screens::wallet::types::{PortfolioToken, RareAccount};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Token,
    Usd,
}

pub struct Props<'a> {
    pub account: Option<&'a RareAccount>,
    pub accounts: &'a [RareAccount],
    pub can_sign: bool,
    pub pf: &'a PortfolioLoad,
    /// `useRecipient(to)`: the resolved state of what is typed in To
    pub rs: &'a RecipientState,
    pub recents: &'a [Recent],
    /// a sign request or the PIN sheet is open: the pad stops listening
    pub sheet_open: bool,
    pub active: bool,
    pub now: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub coin_type: String,
    pub picker: bool,
    pub mode: Mode,
    pub text: String,
    pub is_max: bool,
    pub exact: Option<i128>,
    pub to: String,
    pub busy: bool,
    pub error: Option<String>,
}

impl State {
    pub fn new(coin_type: Option<&str>, to: Option<&str>) -> State {
        State {
            coin_type: norm_type(coin_type.unwrap_or("0x2::sui::SUI")),
            picker: false,
            mode: Mode::Token,
            text: String::new(),
            is_max: false,
            exact: None,
            to: to.unwrap_or("").to_string(),
            busy: false,
            error: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    OpenPicker,
    ClosePicker,
    Pick(String),
    Key(&'static str),
    Chip(i128),
    ToggleMode,
    /// A on To: keyboard "Send to"
    TypeTo,
    To(String),
    Review,
    Push(WalletView),
}

/// The transfer to build and sign (`buildCoinSend` then `signAndExecute` with this title).
#[derive(Clone, Debug, PartialEq)]
pub struct SendCoin {
    pub to: String,
    pub suins: Option<String>,
    pub coin_type: String,
    pub amount: i128,
    pub max: bool,
    pub balance: i128,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    AskTo(String),
    /// the To text changed: resolve it (address or name) into `Props::rs`
    Resolve(String),
    Send(SendCoin),
    Push(WalletView),
}

/// Everything the page derives from its state and data (the TS body's consts).
pub struct Derived<'a> {
    pub token: Option<&'a PortfolioToken>,
    pub held: Vec<&'a PortfolioToken>,
    pub is_sui: bool,
    pub decimals: u32,
    pub symbol: String,
    pub avail: i128,
    pub price: Option<f64>,
    pub amt: i128,
    pub has_amount: bool,
    pub over: bool,
    pub no_gas: bool,
    pub usd_value: Option<f64>,
    pub ready: Option<(String, Option<String>)>,
    pub self_: bool,
    pub cta: String,
    pub can_review: bool,
    pub live: bool,
}

pub fn derive<'a>(pr: &Props<'a>, st: &State) -> Derived<'a> {
    let tokens: &'a [PortfolioToken] = pr.pf.data.as_ref().map(|d| d.tokens.as_slice()).unwrap_or(&[]);
    let mut held: Vec<&PortfolioToken> = tokens.iter().filter(|t| t.raw > 0).collect();
    held.sort_by(|a, b| b.value_usd.unwrap_or(0.0).partial_cmp(&a.value_usd.unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal));
    let token = tokens.iter().find(|t| norm_type(&t.coin_type) == st.coin_type);
    let is_sui = is_sui_type(&st.coin_type) || st.coin_type == sui_type();
    let decimals = token.map(|t| t.decimals).unwrap_or(9);
    let symbol = token.map(|t| t.symbol.clone()).unwrap_or_else(|| if is_sui { "SUI".into() } else { st.coin_type.rsplit("::").next().unwrap_or("?").to_string() });
    let avail = token.map(|t| t.raw).unwrap_or(0);
    // SUI the account does not hold still has a price (the shell's), so the USD line and $ entry work.
    let price = token.and_then(|t| t.price_usd).or_else(|| if is_sui && !cfg!(test) && std::env::var_os("RR_FIXTURE").is_none() { crate::data::live::sui_usd() } else { None });
    let base = if st.is_max {
        Some(avail)
    } else if let Some(e) = st.exact {
        Some(e)
    } else if st.mode == Mode::Token {
        parse_units(&st.text, decimals)
    } else {
        usd_to_base(&st.text, price, decimals)
    };
    let amt = base.unwrap_or(0);
    let has_amount = amt > 0;
    let over = amt > avail;
    let no_gas = !over && is_sui && !st.is_max && amt > avail - SUI_GAS_RESERVE;
    let usd_value = if has_amount { base_to_usd(amt, decimals, price) } else { None };
    let ready = match pr.rs {
        RecipientState::Ready { address, suins } => Some((address.clone(), suins.clone())),
        _ => None,
    };
    let self_ = match (&ready, pr.account) {
        (Some((a, _)), Some(acct)) => *a == acct.address.to_lowercase(),
        _ => false,
    };
    let cta = if !has_amount {
        "Enter an amount".to_string()
    } else if over {
        format!("Not enough {symbol}")
    } else if no_gas {
        "Leave some SUI for gas".into()
    } else if ready.is_none() {
        match pr.rs {
            RecipientState::Empty => "Enter a recipient",
            RecipientState::Loading { .. } => "Looking up name",
            _ => "Check the recipient",
        }
        .into()
    } else if self_ {
        "Pick another address".into()
    } else {
        "Review".into()
    };
    let can_review = pr.can_sign && has_amount && !over && !no_gas && ready.is_some() && !self_ && !st.busy;
    let live = pr.active && !st.picker && !st.busy && !pr.sheet_open;
    Derived { token, held, is_sui, decimals, symbol, avail, price, amt, has_amount, over, no_gas, usd_value, ready, self_, cta, can_review, live }
}

pub fn view(pr: &Props, st: &State) -> Option<El<Msg>> {
    let account = pr.account?;
    let d = derive(pr, st);
    let shown = if st.mode == Mode::Usd { format!("${}", if st.text.is_empty() { "0" } else { &st.text }) } else if st.text.is_empty() { "0".into() } else { st.text.clone() };
    let converse = if st.mode == Mode::Usd {
        format!("≈ {} {}", format_units(d.amt, d.decimals, 6), d.symbol)
    } else if d.price.is_some() {
        format!("≈ {}", usd(d.usd_value.unwrap_or(0.0)))
    } else {
        "No price".into()
    };
    let mut body = Vec::new();
    if !pr.can_sign {
        body.push(panel(
            None,
            None,
            "mx-auto mb-6 max-w-[64rem] px-5 py-4",
            vec![
                p("text-base font-semibold").text("This account is watch only"),
                p("mt-1 text-sm text-secondary").text("Add an account that can sign to send from this wallet."),
                btn("Add an account", Variant::Primary, "mt-3", Some(Msg::Push(WalletView::Add))),
            ],
        ));
    }
    let picker_btn = button("flex h-16 w-full items-center gap-3 rounded border border-border-light px-4 text-left", Some(Msg::OpenPicker))
        .kid(avatar(d.token.and_then(|t| t.icon_url.as_deref()), &d.symbol, "2.25rem", false))
        .kid(span("min-w-0 flex-1").kid(span("block truncate text-base font-semibold").text(&d.symbol)).kid(span("block truncate text-xs text-secondary tabular-nums").text(format!("Balance {}", format_units(d.avail, d.decimals, 4)))))
        .kid(icon("chevron-down", "text-icon-secondary").attr("size", "1.25rem"));
    let amount_line = p(cn(&[
        "max-w-full truncate font-bold tabular-nums",
        if shown.chars().count() > 12 { "text-3xl" } else { "text-5xl" },
        when(st.text.is_empty() && !st.is_max, "text-secondary"),
        when(d.over, "text-negative"),
    ]))
    .attr("data-testid", "amount")
    .text(&shown);
    let conv = if d.price.is_some() {
        button("mt-1 rounded px-2 py-1 text-base text-secondary tabular-nums", Some(Msg::ToggleMode))
            .label(if st.mode == Mode::Usd { "Type in coins" } else { "Type in USD" })
            .kid(text(format!("{converse} ")))
            .kid(span("ml-1 text-xs").text("Switch"))
    } else {
        p("mt-1 py-1 text-base text-secondary").text(&converse)
    };
    let left = div("flex flex-col gap-4")
        .kid(picker_btn)
        .kid(div("flex min-h-[7rem] flex-col items-center justify-center text-center").kid(amount_line).kid(conv))
        .kid(
            div("grid grid-cols-3 gap-2")
                .kid(btn_full("25%", Variant::Secondary, Size::Md, false, "", Some(Msg::Chip(25))))
                .kid(btn_full("50%", Variant::Secondary, Size::Md, false, "", Some(Msg::Chip(50))))
                .kid(btn_full("Max", Variant::Secondary, Size::Md, false, "", Some(Msg::Chip(100)))),
        )
        .kid(amount_pad(d.live, Msg::Key, d.can_review.then_some(Msg::Review)));
    let mut right = div("flex min-w-0 flex-col gap-4").kid(
        div("")
            .kid(field(FieldSpec { label: Some("To"), value: &st.to, placeholder: Some("0x address or name.sui"), ..Default::default() }, Msg::TypeTo))
            .kid_opt(recipient_line(pr.rs, d.self_)),
    );
    if d.ready.is_none() {
        right = right.kid_opt(recipient_picks(pr.accounts, Some(&account.address), pr.recents, &st.to, pr.now, |a| Msg::To(a.to_string())));
    }
    let mut foot = div("mt-auto");
    if st.is_max && d.is_sui {
        foot = foot.kid(p("mb-2 text-sm text-secondary").text("Max sends everything. The network fee comes out of it."));
    }
    if let Some(e) = &st.error {
        foot = foot.kid(p("mb-2 text-sm text-negative").role("alert").text(e));
    }
    foot = foot.kid(btn(if st.busy { "Opening review" } else { &d.cta }, if d.can_review { Variant::Primary } else { Variant::Dimmed }, "w-full", d.can_review.then_some(Msg::Review)));
    right = right.kid(foot);
    body.push(div("mx-auto grid max-w-[64rem] grid-cols-[minmax(0,22rem)_minmax(0,1fr)] gap-10").kid(left).kid(right));
    if st.picker {
        let has_current = d.held.iter().any(|t| norm_type(&t.coin_type) == st.coin_type);
        let mut list = div("-mx-4 flex flex-col");
        if d.held.is_empty() {
            list = list.kid(p("px-4 py-6 text-center text-sm text-secondary").text("No coins in this account yet."));
        }
        for (i, t) in d.held.iter().enumerate() {
            let body_text = format!("{}{}", format_units(t.raw, t.decimals, 4), t.value_usd.map(|v| format!(" · {}", usd(v))).unwrap_or_default());
            list = list.kid(
                list_button(Some(avatar(t.icon_url.as_deref(), &t.symbol, "2.25rem", false)), &t.symbol, Some(text(body_text)), None, Msg::Pick(t.coin_type.clone()), norm_type(&t.coin_type) == st.coin_type || (i == 0 && !has_current), "")
                    .key(&t.coin_type),
            );
        }
        body.push(sheet(SheetSpec { title: "Send which coin".into(), subtitle: None, icon: None, footer: None, close: Some(Msg::ClosePicker), width: "36rem" }, vec![list]));
    }
    Some(page(Some("Send"), Some(text(format!("From {}", account.label))), None, false, "", body).attr("data-ready", account.address.clone()))
}

pub fn hints(pr: &Props, st: &State) -> Vec<Hint> {
    let mut h = vec![hint("a", "Select"), hint("x", "Delete")];
    if derive(pr, st).can_review {
        h.push(hint("start", "Review"));
    }
    h.push(hint("b", "Back"));
    h
}

pub fn update(pr: &Props, st: &mut State, msg: Msg) -> Option<Effect> {
    let d = derive(pr, st);
    match msg {
        Msg::OpenPicker => st.picker = true,
        Msg::ClosePicker => st.picker = false,
        Msg::Pick(t) => {
            st.coin_type = norm_type(&t);
            st.picker = false;
            st.text.clear();
            st.exact = None;
            st.is_max = false;
            st.mode = Mode::Token;
        }
        Msg::Key(k) => {
            if !d.live {
                return None;
            }
            let next = pad_press(&st.text, k, if st.mode == Mode::Usd { 2 } else { d.decimals });
            if next != st.text {
                st.is_max = false;
                st.exact = None;
                st.text = next;
                st.error = None;
            }
        }
        Msg::Chip(pct) => {
            let b = chip_amount(d.avail, pct, d.is_sui);
            st.error = None;
            st.is_max = pct == 100;
            st.exact = Some(b);
            st.text = if st.mode == Mode::Token { to_input(b, d.decimals) } else { crate::screens::wallet::fmt::to_fixed(base_to_usd(b, d.decimals, d.price).unwrap_or(0.0), 2) };
        }
        Msg::ToggleMode => {
            if st.mode == Mode::Token {
                st.text = if d.has_amount && d.price.is_some() { {
                    let t = crate::screens::wallet::fmt::to_fixed(d.usd_value.unwrap_or(0.0), 2);
                    t.strip_suffix(".00").map(str::to_string).unwrap_or(t)
                } } else { String::new() };
                st.mode = Mode::Usd;
            } else {
                st.text = if d.has_amount { to_input(d.amt, d.decimals) } else { String::new() };
                st.mode = Mode::Token;
            }
            st.exact = None;
        }
        Msg::TypeTo => return Some(Effect::AskTo(st.to.clone())),
        Msg::To(v) => {
            st.to = v.split_whitespace().collect();
            return Some(Effect::Resolve(st.to.clone()));
        }
        Msg::Review => {
            let (to, suins) = d.ready.clone()?;
            if !d.can_review {
                return None;
            }
            st.busy = true;
            st.error = None;
            return Some(Effect::Send(SendCoin {
                to,
                suins,
                coin_type: st.coin_type.clone(),
                amount: d.amt,
                max: st.is_max,
                balance: d.avail,
                title: format!("Send {} {}", format_units(d.amt, d.decimals, 4), d.symbol),
            }));
        }
        Msg::Push(v) => return Some(Effect::Push(v)),
    }
    None
}

/// After `Effect::Send`: Ok resets the wallet to home (the host does), a rejection says nothing,
/// any other error shows.
pub fn sent(st: &mut State, res: Result<(), Option<String>>) {
    st.busy = false;
    if let Err(Some(m)) = res {
        st.error = Some(m);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::wallet::home::fixtures::{account, portfolio, NOW};
    use crate::screens::wallet::sign::fixtures::OTHER;

    fn props<'a>(acct: &'a RareAccount, pf: &'a PortfolioLoad, rs: &'a RecipientState) -> Props<'a> {
        Props { account: Some(acct), accounts: std::slice::from_ref(acct), can_sign: true, pf, rs, recents: &[], sheet_open: false, active: true, now: NOW }
    }

    #[test]
    fn empty_send() {
        let acct = account();
        let pf = PortfolioLoad { data: Some(portfolio()), ..Default::default() };
        let rs = RecipientState::Empty;
        let pr = props(&acct, &pf, &rs);
        let st = State::new(None, None);
        let v = view(&pr, &st).unwrap();
        let t = v.texts();
        assert_eq!(&t[..9], ["Send", "From Main", "SUI", "Balance 1,204.5", "0", "≈ $0.00 ", "Switch", "25%", "50%"]);
        assert!(t.contains(&"0x address or name.sui".to_string()));
        assert_eq!(t.last().unwrap(), "Enter an amount");
        assert!(v.find_nav("Enter an amount").unwrap().on.is_none());
        assert_eq!(v.landing().as_deref(), Some("5"));
        assert_eq!(hints(&pr, &st).iter().map(|h| h.label.as_str()).collect::<Vec<_>>(), ["Select", "Delete", "Back"]);
    }

    #[test]
    fn type_amount_and_review() {
        let acct = account();
        let pf = PortfolioLoad { data: Some(portfolio()), ..Default::default() };
        let rs = RecipientState::Ready { address: OTHER.into(), suins: Some("bob.sui".into()) };
        let pr = props(&acct, &pf, &rs);
        let mut st = State::new(None, Some("bob.sui"));
        for k in ["1", "2", ".", "5"] {
            update(&pr, &mut st, Msg::Key(k));
        }
        let v = view(&pr, &st).unwrap();
        assert_eq!(v.find_by_attr("data-testid", "amount").unwrap().text.as_deref(), Some("12.5"));
        assert!(v.contains_text("≈ $11.33 "));
        assert_eq!(v.find_nav("Review").unwrap().on, Some(Msg::Review));
        assert_eq!(v.pad("start"), Some(&Msg::Review));
        let e = update(&pr, &mut st, Msg::Review).unwrap();
        assert_eq!(
            e,
            Effect::Send(SendCoin { to: OTHER.into(), suins: Some("bob.sui".into()), coin_type: norm_type("0x2::sui::SUI"), amount: 12_500_000_000, max: false, balance: 1_204_500_000_000, title: "Send 12.5 SUI".into() })
        );
        assert!(view(&pr, &st).unwrap().contains_text("Opening review"));
        sent(&mut st, Err(Some("Could not send".into())));
        assert!(view(&pr, &st).unwrap().contains_text("Could not send"));
    }

    #[test]
    fn limits_chips_and_usd() {
        let acct = account();
        let pf = PortfolioLoad { data: Some(portfolio()), ..Default::default() };
        let rs = RecipientState::Ready { address: OTHER.into(), suins: None };
        let pr = props(&acct, &pf, &rs);
        let mut st = State::new(None, None);
        for k in ["9", "9", "9", "9"] {
            update(&pr, &mut st, Msg::Key(k));
        }
        assert!(view(&pr, &st).unwrap().contains_text("Not enough SUI"));
        update(&pr, &mut st, Msg::Chip(100));
        let v = view(&pr, &st).unwrap();
        assert_eq!(st.text, "1204.49");
        assert!(v.contains_text("Max sends everything. The network fee comes out of it."));
        update(&pr, &mut st, Msg::Key("back"));
        assert!(!st.is_max);
        st = State::new(None, None);
        update(&pr, &mut st, Msg::Chip(50));
        update(&pr, &mut st, Msg::ToggleMode);
        assert_eq!(st.mode, Mode::Usd);
        assert_eq!(st.text, "545.64");
        let v = view(&pr, &st).unwrap();
        assert_eq!(v.find_by_attr("data-testid", "amount").unwrap().text.as_deref(), Some("$545.64"));
        assert!(v.contains_text("≈ 602.251655 SUI "));
        // self
        let me = RecipientState::Ready { address: acct.address.clone(), suins: None };
        let pr = props(&acct, &pf, &me);
        assert!(view(&pr, &st).unwrap().contains_text("Pick another address"));
    }

    #[test]
    fn coin_picker() {
        let acct = account();
        let pf = PortfolioLoad { data: Some(portfolio()), ..Default::default() };
        let rs = RecipientState::Empty;
        let pr = props(&acct, &pf, &rs);
        let mut st = State::new(None, None);
        update(&pr, &mut st, Msg::OpenPicker);
        let v = view(&pr, &st).unwrap();
        assert!(v.contains_text("Send which coin"));
        assert!(v.contains_text("1,204.5 · $1,091.28"));
        let usdc = portfolio().tokens[1].coin_type.clone();
        update(&pr, &mut st, Msg::Pick(usdc));
        assert!(!st.picker);
        assert!(view(&pr, &st).unwrap().contains_text("USDC"));
        // the pad does not listen while the picker is open
        update(&pr, &mut st, Msg::OpenPicker);
        update(&pr, &mut st, Msg::Key("1"));
        assert_eq!(st.text, "");
    }
}
