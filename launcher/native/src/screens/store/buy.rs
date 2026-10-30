//! Confirm sheet for a Store purchase (store/BuySheet.tsx), in the Marketplace buy sheet's shape:
//! the price, the coin to pay with (USDC, or SUI at the live Pyth price when the studio takes it),
//! then Rare Wallet's review sheet signs. A on a pay button hands `store:buy:<slug>:<coin>` to the
//! app, which reports back through `Phase`. B closes the sheet (not while waiting).

use super::logic::{price_now, sui_quote, usd_text};
use super::model::StoreGame;
use super::parts::{game_logo, trunc_text};
use crate::kit::format::fmt_sui;
use crate::kit::{self, tok, Btn};
use crate::screens::market::host::*;
use rare_render::scene::layer;
use rare_render::view::{El, Look, Motion};

pub const T_USDC: u32 = 9_200;
pub const T_SUI: u32 = 9_201;
pub const T_CLOSE: u32 = 9_202;
/// The focus scope the sheet traps.
pub const SCOPE: u32 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coin {
    Usdc,
    Sui,
}

impl Coin {
    pub fn id(self) -> &'static str {
        match self {
            Coin::Usdc => "usdc",
            Coin::Sui => "sui",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    /// Rare Wallet's review sheet is up for this coin.
    Working(Coin),
    Error(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Buy {
    pub phase: Phase,
    /// The SUI quote in MIST (None until the price arrives, or when SUI is not taken).
    pub sui_mist: Option<u128>,
}

impl Buy {
    /// A fresh sheet for `g`, quoting SUI at `price_e8` when the studio takes SUI.
    pub fn new(g: &StoreGame, price_e8: Option<u64>, now: f64) -> Buy {
        let price = price_now(g, now).unwrap_or(0);
        let accept = g.price.as_ref().is_some_and(|p| p.accept_sui);
        let sui_mist = if accept && price > 0 { price_e8.and_then(|e8| sui_quote(price, e8)).map(|q| q.0) } else { None };
        Buy { phase: Phase::Idle, sui_mist }
    }
}

/// The sheet.
pub fn buy_sheet(ui: &mut Ui, g: &StoreGame, b: &Buy, now: f64) -> El {
    let price = price_now(g, now).unwrap_or(0);
    let accept = g.price.as_ref().is_some_and(|p| p.accept_sui);
    let working = matches!(b.phase, Phase::Working(_));
    let sui = b.sui_mist.map(|m| fmt_sui(Some(m), 2, false, true));

    let names = ui
        .bx(s().col().min0().shrink())
        .child(ui.text(format!("Buy {}", g.name), T::new(Ty::Row).w(600)))
        .child(trunc_text(ui, s(), g.studio.name.clone(), T::new(Ty::Small).c(tok::MUTED)));
    let who = ui.bx(s().row().items_center().gap(0.75).min0().shrink()).child(game_logo(ui, g, 3.0)).child(names);
    let x = kit::focus_icon(ui, "x", 1.25, false, tok::FG, tok::FG);
    let close = ui.bx(s().row().items_center().justify_center().size(2.5).mt(0.25).mr(-0.25)).radius(ui.r(0.25)).child(x);
    let close = fill_nav(close, T_CLOSE, None, None, false);
    let head = ui
        .bx(s().row().items_start().justify_between().gap(1.0).px(2.0).pb(1.0 + 1.0 / ui.rem).pt(1.5))
        .child(who)
        .child(close)
        .child(ui.bx(s().abs(None, Some(0.0), Some(0.0), Some(0.0)).h_px(1.0)).bg(tok::HAIR));

    let mut body = ui
        .bx(s().col().px(2.0).py(1.5))
        .child(ui.text("Price", T::new(Ty::Small).c(tok::MUTED)))
        .child(ui.text_s(s().mt(0.25), usd_text(price), T::new(Ty::Display).w(600).tab()));
    if let (true, Some(sv)) = (accept, &sui) {
        body = body.child(ui.text(format!("or {sv} SUI at today's price"), T::new(Ty::Small).c(tok::MUTED).tab()));
    }
    body = body.child(ui.text_s(s().mt(1.0), "The licence goes to your wallet. You can refund it in full within 48 hours. Network gas is extra.", T::new(Ty::Small).c(tok::MUTED).wrap()));
    match &b.phase {
        Phase::Idle => {}
        Phase::Working(_) => body = body.child(ui.text_s(s().mt(0.75), "Review and approve in Rare Wallet.", T::new(Ty::Small).c(tok::fg(0.8)).wrap())),
        Phase::Error(m) => body = body.child(ui.text_s(s().mt(0.75), m.clone(), T::new(Ty::Small).c(tok::NEG).wrap())),
    }

    let usdc_label = if b.phase == Phase::Working(Coin::Usdc) { "Waiting for approval".to_string() } else { format!("Pay {} in USDC", usd_text(price)) };
    let usdc = kit::button(ui, Btn { full: true, ..Btn::new(usdc_label, T_USDC).primary().def(true).disabled(working) });
    let mut foot = ui
        .bx(s().row().gap(0.5).px(2.0).pb(1.0).pt(1.0 + 1.0 / ui.rem))
        .child(ui.bx(s().abs(Some(0.0), Some(0.0), None, Some(0.0)).h_px(1.0)).bg(tok::HAIR))
        .child(ui.bx(s().row().flex(1.0)).child(usdc));
    if accept {
        let label = if b.phase == Phase::Working(Coin::Sui) {
            "Waiting for approval".to_string()
        } else {
            match &sui {
                Some(v) => format!("Pay {v} SUI"),
                None => "Pay in SUI".into(),
            }
        };
        let sb = kit::button(ui, Btn { full: true, ..Btn::new(label, T_SUI).disabled(working || sui.is_none()) });
        foot = foot.child(ui.bx(s().row().flex(1.0)).child(sb));
    }

    let card = ui
        // border-box: the 1px border takes layout room.
        .bx(s().col().wp(1.0).max_w(40.0).p(1.0 / ui.rem))
        .look(Look::default().bg(tok::BG).radius(ui.r(1.25)).border(1.0, tok::BORDER_LIGHT))
        .clip()
        .enter(Motion::FADE_IN)
        .child(head)
        .child(body)
        .child(foot);
    ui.bx(s().abs(Some(0.0), Some(0.0), Some(3.0), Some(0.0)).row().items_center().justify_center().p(1.5))
        .portal(layer::SHEET)
        .scope(SCOPE)
        .child(ui.bx(s().fill()).bg(tok::BG.alpha(0.8)))
        .child(card)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::store::model::demo;

    #[test]
    fn quotes_sui_only_when_the_studio_takes_it() {
        let d = demo();
        let now = d.now;
        let hk = d.game("hollow-keep").unwrap();
        let b = Buy::new(hk, Some(d.sui_price.price_e8), now);
        // $6.99 at $0.906: 7.7152... SUI, rounded up in MIST.
        assert_eq!(b.sui_mist, Some(7_715_231_789));
        assert_eq!(fmt_sui(b.sui_mist, 2, false, true), "7.72");
        let free = d.game("pocket-orchard").unwrap();
        assert_eq!(Buy::new(free, Some(d.sui_price.price_e8), now).sui_mist, None);
    }
}
