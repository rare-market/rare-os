//! The purchase sheet (marketplace/BuySheet.tsx) in rare.market's modal style: the total, the fee
//! split (rare_market::buy::quote_of), where it is listed, and either "Buy for X SUI" (an account
//! Rare Wallet can sign with: the app builds the purchase and hands it to the wallet's review) or
//! a QR of the rare.market item page to finish on a phone. Watch accounts and no account get the QR
//! straight away, with AddPasskey under it.

use super::host::*;
use super::parts::*;
use crate::kit::{button, Btn};
use rare_market::api::{ItemCard, Listing};
use rare_market::buy::quote_of;
use rare_market::format::{self as mf, SuiOpts};
use rare_render::scene::{layer, Shadow};
use rare_render::view::{El, Motion};

pub const T_CLOSE: u32 = 30_000;
pub const T_BUY: u32 = 30_001;
pub const T_SECOND: u32 = 30_002;
pub const T_PASSKEY: u32 = 30_003;
pub const T_DONE: u32 = 30_004;
pub const SCOPE: u32 = 7;

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    Building,
    Signing,
    Error(String),
    Done(String),
}

/// What the sheet knows besides the item: who buys and whether the QR was asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct BuyState {
    pub phase: Phase,
    pub qr_asked: bool,
    /// The account can sign in Rare Wallet (canBuyInApp).
    pub in_app: bool,
    /// A Rare Wallet account is selected at all (AddPasskey says "Add a passkey" then).
    pub signed_in: bool,
}

impl BuyState {
    pub fn qr(&self) -> bool {
        self.qr_asked || !self.in_app
    }
}

/// "0x1234567890...abcdef12" for a long digest.
pub fn short_digest(d: &str) -> String {
    let c: Vec<char> = d.chars().collect();
    if c.len() > 20 {
        format!("{}...{}", c[..10].iter().collect::<String>(), c[c.len() - 8..].iter().collect::<String>())
    } else {
        d.to_string()
    }
}

/// The QR as an SVG (margin 1, level M, #100C18 on white), the way `qrcode` draws it on the web.
pub fn qr_svg(text: &str) -> String {
    use qrcode::{Color as QC, EcLevel, QrCode};
    let Ok(code) = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M) else { return String::new() };
    let w = code.width();
    let n = w + 2;
    let mut path = String::new();
    for (i, c) in code.to_colors().iter().enumerate() {
        if *c == QC::Dark {
            let (x, y) = (i % w + 1, i / w + 1);
            path.push_str(&format!("M{x} {y}h1v1h-1z"));
        }
    }
    format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {n} {n}" shape-rendering="crispEdges"><rect width="{n}" height="{n}" fill="#ffffff"/><path d="{path}" fill="#100C18"/></svg>"##)
}

fn mono_line(ui: &mut Ui, label: &str, mist: i128, royalty: bool) -> El {
    let c = if royalty { ROYALTIES } else { tok::MUTED };
    let v = format!("{} SUI", mf::fmt_sui_num(rare_market::format::big_to_sui(mist), SuiOpts { dp: 4, ..Default::default() }));
    ui.bx(s().row().justify_between()).child(ui.text(label, t(SM).c(c))).child(ui.text(v, t(SM).mono().c(if royalty { ROYALTIES } else { tok::FG })))
}

/// The sheet over the item page.
pub fn buy_sheet(ui: &mut Ui, item: &ItemCard, listing: &Listing, st: &BuyState, sui_usd: Option<f64>) -> El {
    let q = quote_of(listing);
    let url = mf::item_url(&item.id);
    let coll = item.collection.as_ref().map(|c| c.name.as_str());
    let name = mf::short_name(&item.name, coll).to_string();
    let total_n = mf::big_to_sui(q.total_mist);
    let total = format!("{} SUI", mf::fmt_sui_num(total_n, SuiOpts { dp: 4, compact: false, trim: true }));
    let done = matches!(st.phase, Phase::Done(_));

    // ---- head
    let art = mimg(ui, s().fill(), item.image_url.as_deref(), &item.name, 3.0, 3.0, Img::w(120.0));
    let thumb = ui.bx(s().size(3.0)).bg(tok::L3).radius(ui.r(0.375)).clip().child(art);
    let words = ui
        .bx(s().col().min0().shrink())
        .child(ui.text(if done { "Bought".to_string() } else { format!("Buy {name}") }, t(XL).w(700).lead(1.9375)))
        .child(ui.text(coll.unwrap_or("").to_string(), t(SM).c(tok::MUTED).trunc()));
    let x = kit::focus_icon(ui, "x", 1.25, false, tok::FG, tok::FG);
    let close = fill_nav(ui.bx(s().row().items_center().justify_center().size(2.5).mt(0.25).mr(-0.25)).radius(ui.r(0.25)).child(x), T_CLOSE, None, None, false);
    let hair = |ui: &Ui, top: bool| if top { top_line(ui) } else { bottom_line(ui) };
    let head = ui
        .bx(bw_side(s().row().items_start().justify_between().gap(1.0).px(2.0).pb(1.0).pt(1.5), 0.0, 1.0))
        .child(hair(ui, false))
        .child(ui.bx(s().row().items_center().gap(0.75).min0().shrink()).child(thumb).child(words))
        .child(close);

    let mut card = ui.bx(bw(s().col().wp(1.0).max_w(52.0), 1.0)).bg(tok::BG).radius(ui.r(1.25)).border(1.0, tok::BORDER_LIGHT).shadow(&Shadow::xl2(0.4)).clip().enter(Motion::FADE_IN).child(head);

    if let Phase::Done(digest) = &st.phase {
        let ic = kit::icon(ui, "check", 1.75, tok::FG);
        let mark = ui.bx(s().row().items_center().justify_center().size(3.5)).bg(tok::L3).radius(ui.r(1.75)).child(ic);
        let tx = baseline_row(ui, s().mt(0.75).gap(0.25), vec![(t(XS).c(tok::MUTED), "Transaction".into()), (t(XS).mono(), short_digest(digest))]);
        let txt = ui
            .bx(s().col().min0().shrink())
            .child(ui.text(format!("{} is yours for {total}", item.name), t(LG).w(600).wrap()))
            .child(ui.text_s(s().mt(0.25), "It's in your Rare Wallet. It can take a minute to show on rare.market.", t(SM).c(tok::MUTED).wrap()))
            .child(tx);
        card = card.child(ui.bx(s().row().items_center().gap(1.25).px(2.0).py(2.0)).child(mark).child(txt));
        let d = btn(ui, s().grow(), "Done", T_DONE, V::Primary, Sz::Lg, true, false);
        card = card.child(ui.bx(bw_top(s().row().gap(0.5).px(2.0).py(1.0), 1.0)).child(hair(ui, true)).child(d));
    } else {
        // ---- the price and its split
        let big = baseline_row(ui, s().mt(0.25).gap(0.375), vec![(t(XL4).w(700).tab(), mf::fmt_sui_num(total_n, SuiOpts { dp: 4, ..Default::default() })), (t(XL).w(600), "SUI".into())]);
        let usd = format!("({})", mf::fmt_usd(sui_usd.map(|u| total_n * u)));
        let mut split = ui.bx(bw(s().col().mt(1.0).gap(0.375).px(0.75).py(0.75), 1.0)).radius(ui.r(0.25)).border(1.0, BORDER).child(mono_line(ui, "List price", q.price_mist, false));
        if q.fee_mist > 0 {
            split = split.child(mono_line(ui, &format!("{} fee", mf::venue_name(Some(&q.venue))), q.fee_mist, false));
        }
        if q.royalty_mist > 0 {
            split = split.child(mono_line(ui, "Creator royalty", q.royalty_mist, true));
        }
        if q.commission_mist > 0 {
            split = split.child(mono_line(ui, "Rare fee", q.commission_mist, false));
        }
        let tv = format!("{} SUI", mf::fmt_sui_num(total_n, SuiOpts { dp: 4, ..Default::default() }));
        split = split.child(ui.bx(bw_top(s().row().justify_between().pt(0.375), 1.0)).child(hair(ui, true)).child(ui.text("Total", t(SM).w(600))).child(ui.text(tv, t(SM).w(600).mono())));
        let mut venue = ui.bx(s().row().items_center().gap(0.375));
        if let Some(v) = venue_mark(ui, &q.venue, 0.875) {
            venue = venue.child(v);
        }
        venue = venue.child(ui.text(mf::venue_name(Some(&q.venue)), t(SM)));
        split = split.child(ui.bx(s().row().justify_between()).child(ui.text("Listed on", t(SM).c(tok::MUTED))).child(venue));
        let left = ui
            .bx(s().col().min0())
            .child(ui.text("Total price", t(BASE)))
            .child(big)
            .child(ui.text(usd, t(BASE).c(tok::MUTED).tab()))
            .child(split)
            .child(ui.text_s(s().mt(0.5), "Network gas is extra.", t(XS).c(tok::MUTED)));
        let cols: Vec<Track> = if st.qr() { vec![Track::Fr(1.0), Track::Auto] } else { vec![Track::Fr(1.0)] };
        let mut grid = ui.bx(s().grid(&cols).gap(2.0).px(2.0).py(1.5)).child(left);
        if st.qr() {
            let svg = qr_svg(&url);
            let code = crate::kit::icon::svg_box(ui, &format!("qr-{}", item.id), svg, 11.0, 11.0, s());
            let qr = ui.bx(s().size(12.0).p(0.5)).bg(tok::WHITE).radius(ui.r(0.375)).clip().child(code);
            let mut right = ui
                .bx(s().col().items_center().w(14.0))
                .child(qr)
                .child(center_text(ui, s().wp(1.0).mt(0.75), if st.in_app { "Buy on your phone" } else { "Finish on your phone" }, t(SM).w(600)))
                .child(center_text(ui, s().wp(1.0).mt(0.25), "Scan to buy on rare.market.", t(XS).c(tok::MUTED)));
            if !st.in_app {
                let (line, label) = if st.signed_in { ("Add a passkey to buy.", "Add a passkey") } else { ("Sign in with a passkey to buy.", "Sign in") };
                let b = button(ui, Btn { hint: Some(label.into()), ..Btn::new(label, T_PASSKEY).primary() });
                right = right.child(ui.bx(s().col().items_start().gap(0.75).mt(0.75)).child(ui.text(line, T::new(Ty::Small).c(tok::MUTED))).child(b));
            }
            grid = grid.child(right);
        }
        card = card.child(grid);
        if st.in_app && st.phase != Phase::Idle {
            let (msg, bad) = match &st.phase {
                Phase::Building => ("Getting the purchase ready...".to_string(), false),
                Phase::Signing => ("Review and approve in Rare Wallet.".to_string(), false),
                Phase::Error(m) => (m.clone(), true),
                _ => (String::new(), false),
            };
            card = card.child(ui.bx(s().px(2.0).pb(1.0)).child(ui.text(msg, t(SM).c(if bad { tok::NEG } else { tok::MUTED }).wrap())));
        }
        let mut foot = ui.bx(bw_top(s().row().gap(0.5).px(2.0).py(1.0), 1.0)).child(hair(ui, true));
        if st.in_app {
            let label = match &st.phase {
                Phase::Building => "Getting ready...".to_string(),
                Phase::Signing => "Waiting for approval".into(),
                Phase::Error(_) => "Try again".into(),
                _ => format!("Buy for {total}"),
            };
            let main = btn(ui, s().grow(), &label, T_BUY, V::Primary, Sz::Lg, true, false);
            let second = btn(ui, s(), if st.qr() { "Open on rare.market" } else { "Buy on your phone" }, T_SECOND, V::Secondary, Sz::Lg, false, false);
            foot = foot.child(main).child(second);
        } else {
            let main = btn(ui, s().grow(), "Done", T_DONE, V::Primary, Sz::Lg, true, false);
            let second = btn(ui, s(), "Open on rare.market", T_SECOND, V::Secondary, Sz::Lg, false, false);
            foot = foot.child(main).child(second);
        }
        card = card.child(foot);
    }

    // In flow inside the layer (not absolute): an absolute root leaves the layer with a stale
    // out-of-flow entry when it closes (a swarm request to WS 1).
    ui.bx(s().full().row().items_center().justify_center().p(1.5).pb(4.5))
        .portal(layer::SHEET)
        .scope(SCOPE)
        .child(ui.bx(s().abs(Some(0.0), Some(0.0), Some(3.0), Some(0.0))).bg(tok::BG.alpha(0.8)))
        .child(card)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_and_qr() {
        assert_eq!(short_digest("abc"), "abc");
        assert_eq!(short_digest("0123456789abcdefghijKLMNOPQR"), "0123456789...KLMNOPQR");
        let svg = qr_svg("https://rare.market/item/0x1");
        assert!(svg.starts_with("<svg") && svg.contains("#100C18"));
    }

    #[test]
    fn qr_shows_when_asked_or_when_the_wallet_cannot_sign() {
        let st = BuyState { phase: Phase::Idle, qr_asked: false, in_app: true, signed_in: true };
        assert!(!st.qr());
        assert!(BuyState { qr_asked: true, ..st.clone() }.qr());
        assert!(BuyState { in_app: false, ..st }.qr());
    }
}
