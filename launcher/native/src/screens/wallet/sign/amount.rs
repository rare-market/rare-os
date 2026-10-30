//! The amount pad (launcher/src/wallet/ui/sign/AmountPad.tsx, padKeys.ts): the kit PinPad's look
//! with a decimal point instead of OK. The d-pad moves over the keys, A presses, X deletes, Start
//! submits. Turn `active` off while a sheet is open so keys go to the sheet.

use crate::screens::wallet::markup::*;

pub const KEYS: [&str; 12] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", ".", "0", "back"];

/// `padPress`: one key ("0"-"9", ".", "back") on the amount text; `decimals` caps the fraction.
pub fn pad_press(value: &str, key: &str, decimals: u32) -> String {
    if key == "back" {
        let mut v = value.to_string();
        v.pop();
        return v;
    }
    if key == "." {
        if decimals == 0 || value.contains('.') {
            return value.to_string();
        }
        return if value.is_empty() { "0.".into() } else { format!("{value}.") };
    }
    if key.len() != 1 || !key.as_bytes()[0].is_ascii_digit() {
        return value.to_string();
    }
    if let Some((_, frac)) = value.split_once('.') {
        return if frac.len() >= decimals as usize { value.to_string() } else { format!("{value}{key}") };
    }
    if value.len() >= 15 {
        return value.to_string();
    }
    // no leading zeros: "0" then "5" is "5", and "0" then "0" stays "0"
    if value == "0" {
        return key.to_string();
    }
    format!("{value}{key}")
}

/// `cleanTyped`: text typed on a keyboard or pasted, cleaned to the same rules (the first comma is a point).
pub fn clean_typed(text: &str, decimals: u32) -> String {
    let t = text.replacen(',', ".", 1);
    let mut out = String::new();
    for ch in t.chars() {
        out = pad_press(&out, &ch.to_string(), decimals);
    }
    out
}

/// The pad. A sends `on(key)`; X sends `on("back")`; Start sends `submit` when there is one.
pub fn amount_pad<M: Clone>(active: bool, on: impl Fn(&'static str) -> M, submit: Option<M>) -> El<M> {
    let mut g = div("grid grid-cols-3 gap-2").role("group").label("Amount keys").kids(KEYS.iter().map(|k| {
        button(
            cn(&["btn flex h-14 items-center justify-center rounded bg-button-secondary text-2xl font-medium tabular-nums disabled:opacity-40", when(*k == "back", "text-base")]),
            Some(on(k)),
        )
        .def(*k == "5")
        .disabled(!active)
        .key(*k)
        .text(if *k == "back" { "Del" } else { k })
    }));
    if active {
        g = g.on_pad("x", on("back"));
        if let Some(s) = submit {
            g = g.on_pad("start", s);
        }
    }
    g
}

/// Leave a little SUI behind when sending most of it, so the transfer can still pay gas.
pub const SUI_GAS_RESERVE: i128 = 10_000_000;

/// tx/amount.ts `parseUnits`: "1.5" to base units; None for empty or invalid input.
pub fn parse_units(input: &str, decimals: u32) -> Option<i128> {
    let t = input.trim();
    if t.is_empty() || t == "." || !t.chars().all(|c| c.is_ascii_digit() || c == '.') || t.matches('.').count() > 1 {
        return None;
    }
    let (w, f) = t.split_once('.').unwrap_or((t, ""));
    let frac: String = f.chars().take(decimals as usize).collect();
    let frac = format!("{:0<width$}", frac, width = decimals as usize);
    format!("{}{}", if w.is_empty() { "0" } else { w }, frac).parse().ok()
}

/// tx/amount.ts `toInput`: base units as plain decimal text (no separators, full precision).
pub fn to_input(base: i128, decimals: u32) -> String {
    crate::screens::wallet::fmt::format_units(base, decimals, decimals as usize).replace(',', "")
}

/// tx/amount.ts `chipAmount`: 25% / 50% never eat into the SUI gas reserve.
pub fn chip_amount(available: i128, pct: i128, is_sui: bool) -> i128 {
    let cap = if is_sui { (available - SUI_GAS_RESERVE).max(0) } else { available };
    if pct == 100 {
        return cap;
    }
    (available * pct / 100).min(cap)
}

/// tx/amount.ts `usdToBase`: USD text to base units at `price`, rounded down.
pub fn usd_to_base(usd_text: &str, price: Option<f64>, decimals: u32) -> Option<i128> {
    let n: f64 = usd_text.parse().ok()?;
    let p = price.filter(|p| *p != 0.0)?;
    if usd_text.is_empty() || !n.is_finite() {
        return None;
    }
    parse_units(&crate::screens::wallet::fmt::to_fixed(n / p, decimals.min(20) as usize), decimals)
}

/// tx/amount.ts `baseToUsd`.
pub fn base_to_usd(base: i128, decimals: u32, price: Option<f64>) -> Option<f64> {
    let p = price.filter(|p| *p != 0.0)?;
    Some(to_input(base, decimals).parse::<f64>().unwrap_or(0.0) * p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ty(keys: &str, d: u32) -> String {
        keys.chars().fold(String::new(), |v, k| {
            let key = if k == '<' { "back".to_string() } else { k.to_string() };
            pad_press(&v, &key, d)
        })
    }

    // padKeys.test.ts
    #[test]
    fn amount_pad_rules() {
        assert_eq!(ty("12.5", 9), "12.5");
        assert_eq!(ty("1.2.3", 9), "1.23");
        assert_eq!(ty(".5", 9), "0.5");
        assert_eq!(ty("007", 9), "7");
        assert_eq!(ty("0.05", 9), "0.05");
        assert_eq!(ty("0.1234567", 4), "0.1234");
        assert_eq!(ty("1.5", 0), "15");
        assert_eq!(ty("1.5<<", 9), "1");
        assert_eq!(ty("<", 9), "");
        assert_eq!(clean_typed("1,5abc", 9), "1.5");
    }

    // tx.test.ts "amount"
    #[test]
    fn amounts() {
        const ONE: i128 = 1_000_000_000;
        assert_eq!(chip_amount(10 * ONE, 25, true), 2_500_000_000);
        assert_eq!(chip_amount(10 * ONE, 50, true), 5 * ONE);
        assert_eq!(chip_amount(10 * ONE, 100, true), 10 * ONE - SUI_GAS_RESERVE);
        assert_eq!(chip_amount(50_000_000, 100, false), 50_000_000);
        assert_eq!(chip_amount(50_000_000, 50, false), 25_000_000);
        assert_eq!(chip_amount(SUI_GAS_RESERVE, 100, true), 0);
        assert_eq!(chip_amount(12_000_000, 50, true), 2_000_000);
        assert_eq!(usd_to_base("10", Some(4.0), 9), Some(2_500_000_000));
        assert_eq!(usd_to_base("1", Some(3.0), 6), Some(333_333));
        assert_eq!(usd_to_base("", Some(4.0), 9), None);
        assert_eq!(usd_to_base("5", None, 9), None);
        assert_eq!(to_input(1_234_567_890_123, 9), "1234.567890123");
        assert_eq!(parse_units("1.5", 9), Some(1_500_000_000));
        assert_eq!(parse_units(".", 9), None);
        assert_eq!(parse_units("1.23456789012", 9), Some(1_234_567_890));
    }

    #[test]
    fn pad_keys() {
        let p: El<&str> = amount_pad(true, |k| k, Some("submit"));
        assert_eq!(p.texts(), ["1", "2", "3", "4", "5", "6", "7", "8", "9", ".", "0", "Del"]);
        assert_eq!(p.landing().as_deref(), Some("5"));
        assert_eq!(p.pad("x"), Some(&"back"));
        assert_eq!(p.pad("start"), Some(&"submit"));
        let off: El<&str> = amount_pad(false, |k| k, Some("submit"));
        assert!(off.pad("x").is_none());
        assert!(off.find_nav("5").unwrap().disabled);
    }
}
