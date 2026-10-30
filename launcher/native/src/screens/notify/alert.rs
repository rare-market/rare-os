//! "Price alert" on a coin's page in Trade (notify/AlertSheet.tsx): pick a USD target above or
//! below the price now (presets, minus and plus, LT/RT, or type one) and one notice is sent when it
//! is crossed. A saves, B cancels. Saving reports `Out::Act("toast:Price alert set: ...")`; the
//! shell shows the toast and closes the sheet. Typing a price reports `Out::Act("alert:type")`
//! and the shell answers with `AlertSheet::set_typed`.

use super::backend::{AlertsApi, AlertsState, NewAlert};
use super::data::types::AlertDir;
use super::ui::{amount_text, usd_price};
use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::input::Pad;
use rare_render::theme::tok;
use rare_render::{Id, Node};

pub const PRESETS: [i32; 5] = [-10, -5, 5, 10, 25];

/// Four significant digits: enough for any price, never a float tail.
pub fn round4(v: f64) -> f64 {
    if !(v > 0.0) {
        return 0.0;
    }
    let e = v.log10().floor() as i32;
    let f = 10f64.powi(3 - e);
    let r = (v * f).round() / f;
    // Away from float tails: parse what four digits print as.
    format!("{:.*}", (3 - e).max(0) as usize, r).parse().unwrap_or(r)
}

/// One step of the stepper: 1 % of the price now, never below 1 % of it.
pub fn step_target(price: f64, target: f64, d: i32) -> f64 {
    round4((price * 0.01).max(target + d as f64 * price * 0.01))
}

pub fn dir_of(price: f64, target: f64) -> AlertDir {
    if target >= price {
        AlertDir::Above
    } else {
        AlertDir::Below
    }
}

pub fn pct_from(price: f64, target: f64) -> f64 {
    (target - price) / price * 100.0
}

pub fn preset_target(price: f64, p: i32) -> f64 {
    round4(price * (1.0 + p as f64 / 100.0))
}

pub fn preset_label(p: i32) -> String {
    if p > 0 {
        format!("+{p}%")
    } else {
        format!("\u{2212}{}%", -p)
    }
}

pub fn pct_line(pct: f64) -> String {
    format!("{}{}% from now", if pct >= 0.0 { "+" } else { "\u{2212}" }, amount_text(pct.abs()))
}

/// The "Alert when" line.
pub fn when_line(symbol: &str, dir: AlertDir, target: f64) -> String {
    format!("{symbol} goes {} {}", dir.as_str(), usd_price(target))
}

/// Parses what a person types: "$1,05" style noise stripped; positive numbers only.
pub fn parse_price(v: &str) -> Option<f64> {
    let t: String = v.chars().filter(|c| !matches!(c, '$' | ',' | ' ' | '\t' | '\n')).collect();
    t.parse::<f64>().ok().filter(|n| n.is_finite() && *n > 0.0)
}

const T_SAVE: u32 = 1;
const T_TYPE: u32 = 2;
const T_CANCEL: u32 = 3;
const T_LESS: u32 = 10;
const T_MORE: u32 = 11;
const T_PRESET: u32 = 20;

pub struct AlertSheet {
    nav: Nav,
    pub symbol: String,
    pub coin_type: String,
    pub price: f64,
    pub target: f64,
    addr: Option<String>,
    pub busy: bool,
    pub error: Option<String>,
    price_text: Id,
    pct_text: Id,
    chips: Vec<(Id, Id)>,
    when_text: Id,
    error_text: Id,
    save: Id,
    api: Box<dyn AlertsApi>,
    pub state: AlertsState,
}

fn line(cx: &mut Cx, parent: Id, label: &str, value: &str, strong: bool, last: bool) -> Id {
    let r = cx.r();
    let row_ = cx.scene.add(parent, s().row().justify_between().items_center().gap(1.0 * r).wp(1.0).py(0.45 * r).done(), Node::default());
    text(cx, row_, ty::SMALL, if strong { 600 } else { 400 }, if strong { tok::FG } else { tok::MUTED }, label);
    let v = cx.scene.add_text(cx.t, row_, s().done(), value, ts(r, ty::SMALL.0, ty::SMALL.1, if strong { 600 } else { 400 }).tabular(), tok::FG);
    if !last {
        edge(cx, row_, 2, tok::HAIR);
    }
    v
}

impl AlertSheet {
    pub fn build(cx: &mut Cx, symbol: &str, coin_type: &str, price: f64, addr: Option<String>, api: Box<dyn AlertsApi>, now: f64) -> AlertSheet {
        let r = cx.r();
        let target = round4(price * 1.1);
        let root = cx.scene.root;
        let layer = cx.scene.layer(rare_render::scene::layer::SHEET);
        let _ = root;
        let parts = sheet(cx, layer, &format!("Price alert for {symbol}"), Some("One notice when the price crosses your target."), true, false, now);
        let mut nav = Nav::default();
        let body = cx.scene.add(parts.body, s().col().gap(1.0 * r).done(), Node::default());
        // Stepper.
        let st = cx.scene.add(body, s().row().items_center().gap(0.75 * r).done(), Node::default());
        let step_btn = |cx: &mut Cx, nav: &mut Nav, label: &str, tag: u32, hint: &str| {
            let b = cx.scene.add(st, s().wh(3.0 * r, 2.5 * r).items_center().justify_center().done(), Node::bg(tk::BTN2).radius(0.25 * r));
            cx.scene.add_text(cx.t, b, s().done(), label, ts(r, ty::ROW.0, ty::ROW.1, 600), tok::MUTED);
            nav.row_base.insert(b, tk::BTN2);
            nav.add(Focusable { id: b, kind: Kind::Row, group: 1, hint: hint.into(), tag, sid: String::new() });
            b
        };
        step_btn(cx, &mut nav, "\u{2212}", T_LESS, "Less price");
        let mid = cx.scene.add(st, s().col().items_center().grow().done(), Node::default());
        let price_text = cx.scene.add_text(cx.t, mid, s().done(), &usd_price(target), ts(r, 2.0, 2.25, 600).tracking(-0.02).tabular(), tok::FG);
        let pct_text = cx.scene.add_text(cx.t, mid, s().done(), &pct_line(pct_from(price, target)), ts(r, ty::SMALL.0, ty::SMALL.1, 400).tabular(), tok::MUTED);
        step_btn(cx, &mut nav, "+", T_MORE, "More price");
        // Presets.
        let pr = cx.scene.add(body, s().row().gap(0.4 * r).done(), Node::default());
        let mut chips = Vec::new();
        for (i, p) in PRESETS.iter().enumerate() {
            let on = preset_target(price, *p) == target;
            let base = if on { tk::BTN_ACTIVE } else { tk::BTN2 };
            let c = cx.scene.add(pr, s().row().items_center().justify_center().grow().h(2.5 * r).px(0.5 * r).done(), Node::bg(base).radius(0.25 * r));
            let t = cx.scene.add_text(cx.t, c, s().done(), &preset_label(*p), ts(r, ty::META.0, ty::META.1, 500).tabular(), if on { tok::FG } else { tok::MUTED });
            nav.row_base.insert(c, base);
            nav.add(Focusable { id: c, kind: Kind::Row, group: 2, hint: String::new(), tag: T_PRESET + i as u32, sid: String::new() });
            chips.push((c, t));
        }
        // Lines.
        let lines = cx.scene.add(body, s().col().done(), Node::default());
        line(cx, lines, "Price now", &usd_price(price), false, false);
        let when_text = line(cx, lines, "Alert when", &when_line(symbol, dir_of(price, target), target), true, true);
        let error_text = cx.scene.add_text(cx.t, body, s().done(), "Could not set the alert. Try again in a moment.", ts(r, ty::SMALL.0, ty::SMALL.1, 400), tok::NEG);
        cx.scene.n(error_text).hidden = true;
        // Footer.
        let f = parts.footer.unwrap();
        let mut save = button(cx, f, "Set alert", Variant::Primary, Sz::Lg, false).hint("Set alert").tag(T_SAVE).group(3);
        let mut st_ = cx.scene.style(save.id);
        st_.flex_grow = 1.0;
        cx.scene.set_style(save.id, st_);
        save.group = 3;
        let save_id = nav.add(save);
        nav.add(button(cx, f, "Type a price", Variant::Secondary, Sz::Lg, false).hint("Type a price").tag(T_TYPE).group(3));
        nav.add(button(cx, f, "Cancel", Variant::Secondary, Sz::Lg, false).tag(T_CANCEL).group(3));
        nav.focus(cx.scene, save_id, now, false);
        let state = AlertsState::default();
        AlertSheet { nav, symbol: symbol.into(), coin_type: coin_type.into(), price, target, addr, busy: false, error: None, price_text, pct_text, chips, when_text, error_text, save: save_id, api, state }
    }

    /// Hint-bar entries (useSheetPad): A Select, LT Lower, RT Higher, B Cancel.
    pub fn hints() -> [(&'static str, &'static str); 4] {
        [("a", "Select"), ("lt", "Lower"), ("rt", "Higher"), ("b", "Cancel")]
    }

    fn set_target(&mut self, cx: &mut Cx, t: f64, now: f64) {
        self.target = t;
        cx.scene.set_text(cx.t, self.price_text, &usd_price(t));
        cx.scene.set_text(cx.t, self.pct_text, &pct_line(pct_from(self.price, t)));
        cx.scene.set_text(cx.t, self.when_text, &when_line(&self.symbol, dir_of(self.price, t), t));
        for (i, (c, tx)) in self.chips.clone().into_iter().enumerate() {
            let on = preset_target(self.price, PRESETS[i]) == t;
            let base = if on { tk::BTN_ACTIVE } else { tk::BTN2 };
            self.nav.row_base.insert(c, base);
            if self.nav.cur != Some(c) {
                cx.scene.n(c).bg.set(base, now, 0.11);
            }
            cx.scene.n(tx).text.as_mut().unwrap().color.set(if on { tok::FG } else { tok::MUTED }, now, 0.11);
        }
    }

    /// The price the person typed (the shell asked with `alert:type`).
    pub fn set_typed(&mut self, cx: &mut Cx, v: &str, now: f64) {
        if let Some(n) = parse_price(v) {
            self.set_target(cx, round4(n), now);
        }
    }

    fn save(&mut self, cx: &mut Cx) -> Out {
        if self.busy || !(self.target > 0.0) {
            return Out::None;
        }
        self.busy = true;
        self.error = None;
        cx.scene.n(self.error_text).hidden = true;
        let a = NewAlert { coin_type: self.coin_type.clone(), symbol: self.symbol.clone(), dir: dir_of(self.price, self.target), target: self.target, addr: self.addr.clone() };
        let res = self.api.add(&a).map(|_| ());
        self.busy = false;
        match res {
            Ok(()) => Out::Act(format!("toast:Price alert set: {} {} {}", self.symbol, a.dir.as_str(), usd_price(self.target))),
            Err(_) => {
                self.error = Some("Could not set the alert. Try again in a moment.".into());
                cx.scene.n(self.error_text).hidden = false;
                Out::None
            }
        }
    }
}

impl Screen for AlertSheet {
    fn touch_focus(&mut self, cx: &mut Cx, x: f32, y: f32, now: f64) -> bool {
        self.nav.focus_at(cx.scene, x, y, now)
    }
    fn answer(&mut self, cx: &mut Cx, key: &str, value: Option<String>, now: f64) -> Out {
        if let ("alert:type", Some(v)) = (key, value) {
            self.set_typed(cx, &v, now);
        }
        Out::None
    }

    fn pad(&mut self, cx: &mut Cx, pad: Pad, now: f64) -> Out {
        match pad {
            Pad::B => Out::Close,
            // After the sheet's own handler, so LT/RT reach the target instead of being held.
            Pad::Lt | Pad::Rt => {
                let t = step_target(self.price, self.target, if pad == Pad::Rt { 1 } else { -1 });
                self.set_target(cx, t, now);
                Out::None
            }
            Pad::Up | Pad::Down | Pad::Left | Pad::Right => {
                if self.nav.step(cx.scene, pad, now) {
                    Out::Moved
                } else {
                    Out::None
                }
            }
            Pad::A => {
                let Some(f) = self.nav.current().cloned() else { return Out::None };
                match f.tag {
                    T_SAVE => self.save(cx),
                    T_TYPE => Out::Act("alert:type".into()),
                    T_CANCEL => Out::Close,
                    T_LESS | T_MORE => {
                        let t = step_target(self.price, self.target, if f.tag == T_MORE { 1 } else { -1 });
                        self.set_target(cx, t, now);
                        Out::None
                    }
                    t if t >= T_PRESET => {
                        let p = preset_target(self.price, PRESETS[(t - T_PRESET) as usize]);
                        self.set_target(cx, p, now);
                        Out::None
                    }
                    _ => Out::None,
                }
            }
            _ => Out::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_keeps_four_significant_digits() {
        assert_eq!(round4(1.1000000000000001), 1.1);
        assert_eq!(round4(1.23456), 1.235);
        assert_eq!(round4(0.0123456), 0.01235);
        assert_eq!(round4(1234.56), 1235.0);
        assert_eq!(round4(-1.0), 0.0);
        assert_eq!(round4(f64::NAN), 0.0);
    }

    #[test]
    fn default_target_is_ten_percent_up() {
        assert_eq!(round4(1.01 * 1.1), 1.111);
    }

    #[test]
    fn stepping_moves_one_percent_of_the_price_and_floors_at_one_percent() {
        assert_eq!(step_target(1.0, 1.1, 1), 1.11);
        assert_eq!(step_target(1.0, 1.1, -1), 1.09);
        assert_eq!(step_target(1.0, 0.01, -1), 0.01);
        assert_eq!(step_target(100.0, 100.0, -1), 99.0);
    }

    #[test]
    fn direction_follows_the_target() {
        assert_eq!(dir_of(1.0, 1.0), AlertDir::Above);
        assert_eq!(dir_of(1.0, 0.99), AlertDir::Below);
        assert_eq!(when_line("SUI", AlertDir::Above, 1.1), "SUI goes above $1.100");
    }

    #[test]
    fn labels() {
        assert_eq!(preset_label(10), "+10%");
        assert_eq!(preset_label(-5), "\u{2212}5%");
        assert_eq!(pct_line(10.0), "+10% from now");
        assert_eq!(pct_line(-5.0), "\u{2212}5% from now");
        assert_eq!(preset_target(2.0, 25), 2.5);
    }

    #[test]
    fn typed_prices_lose_their_dressing() {
        assert_eq!(parse_price("$1,200.5"), Some(1200.5));
        assert_eq!(parse_price(" 0.05 "), Some(0.05));
        assert_eq!(parse_price("abc"), None);
        assert_eq!(parse_price("0"), None);
        assert_eq!(parse_price("-3"), None);
    }
}
