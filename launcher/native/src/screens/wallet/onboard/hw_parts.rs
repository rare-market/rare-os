//! Pieces the Ledger and Keystone pages share (launcher/src/wallet/ui/onboard/HwParts.tsx): the
//! error box with the one-line USB permission fix, and the list of device accounts to pick from.

use crate::screens::wallet::kit::{addr, btn_full, field, panel, FieldSpec, Size, Variant};
use crate::screens::wallet::market::sui;
use crate::screens::wallet::markup::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum ErrMsg {
    CopyFix,
    Retry,
}

/// `HwError`: with a fix, the permission note and the command to copy; `retry` adds Try again.
pub fn hw_error(message: &str, fix: Option<&str>, copied: bool, retry: bool) -> El<ErrMsg> {
    let mut kids = vec![p("text-sm text-negative").text(if fix.is_some() { "Rare OS needs permission to use this device." } else { message })];
    if let Some(f) = fix {
        kids.push(p("text-sm text-secondary").text("Run this once in Konsole, then plug your device in again."));
        kids.push(code("select-text break-all rounded border border-border-primary bg-layer-02 px-3 py-2 font-mono text-xs leading-relaxed").text(f));
        kids.push(div("").kid(btn_full(if copied { "Copied" } else { "Copy command" }, Variant::Secondary, Size::Md, false, "", Some(ErrMsg::CopyFix))));
    }
    if retry {
        kids.push(div("").kid(btn_full("Try again", Variant::Secondary, Size::Md, false, "", Some(ErrMsg::Retry)).def(true)));
    }
    panel(None, None, "flex flex-col gap-3 px-5 py-4", kids)
}

#[derive(Clone, Debug, PartialEq)]
pub struct HwRow {
    pub index: u32,
    pub address: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verify {
    Busy,
    Ok,
    Bad,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pick {
    pub picked: Vec<String>,
    pub names: BTreeMap<String, String>,
    /// the first account not yet added, when the list first showed (focus lands there)
    pub first_free: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Toggle(String),
    /// A on the name field: keyboard "Name for account N"
    Name(String, u32),
    Named(String, String),
    Verify(String),
    More,
    Add,
}

pub struct Props<'a> {
    pub rows: &'a [HwRow],
    /// addresses already in the wallet
    pub existing: &'a [String],
    /// "Ledger 3": the default name for the nth (0-based) newly picked account
    pub default_name: &'a dyn Fn(usize) -> String,
    pub can_verify: bool,
    pub more: bool,
    pub adding: bool,
    pub error: Option<&'a str>,
    pub verified: &'a BTreeMap<String, Verify>,
    /// SUI per address (None while loading or unreadable)
    pub balances: &'a BTreeMap<String, i128>,
}

fn owned(pr: &Props, a: &str) -> bool {
    pr.existing.iter().any(|e| e.to_lowercase() == a.to_lowercase())
}

pub fn first_free(rows: &[HwRow], existing: &[String]) -> Option<String> {
    rows.iter().find(|r| !existing.iter().any(|e| e.to_lowercase() == r.address.to_lowercase())).map(|r| r.address.clone())
}

pub fn fresh(pr: &Props, st: &Pick) -> Vec<String> {
    st.picked.iter().filter(|a| !owned(pr, a)).cloned().collect()
}

pub fn view(pr: &Props, st: &Pick) -> El<Msg> {
    let rows = pr.rows.iter().map(|r| {
        let added = owned(pr, &r.address);
        let on = !added && st.picked.contains(&r.address);
        let nth = st.picked.iter().position(|a| *a == r.address).unwrap_or(0);
        let bal = if added {
            text("Added")
        } else {
            match pr.balances.get(&r.address) {
                Some(m) => sui(Some(*m), 3, false, true, ""),
                None => text(""),
            }
        };
        let mut line = div("flex items-center gap-2").kid(
            button(cn(&["flex min-w-0 flex-1 items-center gap-4 rounded px-4 py-3 text-left", when(added, "opacity-60")]), Some(Msg::Toggle(r.address.clone())))
                .def(st.first_free.as_deref() == Some(r.address.as_str()))
                .disabled(added)
                .role("checkbox")
                .attr("aria-checked", on.to_string())
                .kid(span(cn(&["flex size-6 shrink-0 items-center justify-center rounded border", if on { "border-transparent bg-primary text-background" } else { "border-border-light" }])).kid_if(on, || icon("check", "").attr("size", "1rem").attr("stroke-width", "3")))
                .kid(span("w-24 shrink-0 whitespace-nowrap text-sm text-secondary").text(format!("Account {}", r.index + 1)))
                .kid(addr(&r.address, false, "min-w-0 flex-1 truncate text-base"))
                .kid(span("shrink-0 text-sm tabular-nums text-secondary").kid(bal)),
        );
        if pr.can_verify && !added {
            let label = match pr.verified.get(&r.address) {
                Some(Verify::Busy) => "Check device",
                Some(Verify::Ok) => "Verified",
                Some(Verify::Bad) => "Does not match",
                None => "Verify on device",
            };
            line = line.kid(btn_full(label, Variant::Secondary, Size::Md, false, "w-40", Some(Msg::Verify(r.address.clone()))));
        }
        let mut cell = div("px-1 py-1").key(&r.address).kid(line);
        if on {
            let name = st.names.get(&r.address).map(String::as_str).unwrap_or("");
            let ph = (pr.default_name)(nth);
            cell = cell.kid(div("px-4 pb-2 pt-1 pl-[3.5rem]").kid(field(FieldSpec { value: name, placeholder: Some(&ph), ..Default::default() }, Msg::Name(r.address.clone(), r.index + 1))));
        }
        cell
    });
    let n = fresh(pr, st).len();
    let mut d = div("flex flex-col gap-4").kid(panel(None, None, "divide-y divide-border-primary p-1", rows.collect())).kid(btn(if pr.more { "Reading" } else { "Show more" }, Variant::Ghost, "w-full", Msg::More));
    if let Some(e) = pr.error {
        d = d.kid(p("text-sm text-negative").text(e));
    }
    d.kid(btn(
        &if pr.adding {
            "Adding".to_string()
        } else if n <= 1 {
            "Add account".to_string()
        } else {
            format!("Add {n} accounts")
        },
        Variant::Primary,
        &cn(&["w-full", when(n == 0 || pr.adding, "pointer-events-none opacity-40")]),
        Msg::Add,
    ))
}

fn btn(label: &str, v: Variant, class: &str, on: Msg) -> El<Msg> {
    crate::screens::wallet::kit::btn(label, v, class, Some(on))
}

/// The accounts to add: each fresh pick with its typed name or the default.
pub fn picks(pr: &Props, st: &Pick) -> Vec<(HwRow, String)> {
    fresh(pr, st)
        .iter()
        .enumerate()
        .filter_map(|(n, a)| {
            let row = pr.rows.iter().find(|r| r.address == *a)?.clone();
            let typed = st.names.get(a).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            Some((row, typed.unwrap_or_else(|| (pr.default_name)(n))))
        })
        .collect()
}

pub fn update(st: &mut Pick, msg: &Msg) {
    match msg {
        Msg::Toggle(a) => {
            if let Some(i) = st.picked.iter().position(|x| x == a) {
                st.picked.remove(i);
            } else {
                st.picked.push(a.clone());
            }
        }
        Msg::Named(a, v) => {
            st.names.insert(a.clone(), v.clone());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0x00000000000000000000000000000000000000000000000000000000000000a1";
    const B: &str = "0x00000000000000000000000000000000000000000000000000000000000000b2";

    #[test]
    fn pick_and_name() {
        let rows = vec![HwRow { index: 0, address: A.into() }, HwRow { index: 1, address: B.into() }];
        let existing = vec![A.to_string()];
        let verified = BTreeMap::new();
        let mut bal = BTreeMap::new();
        bal.insert(B.to_string(), 2_500_000_000);
        let name = |n: usize| format!("Ledger {}", n + 2);
        let pr = Props { rows: &rows, existing: &existing, default_name: &name, can_verify: true, more: false, adding: false, error: None, verified: &verified, balances: &bal };
        let mut st = Pick { first_free: first_free(&rows, &existing), ..Default::default() };
        let v = view(&pr, &st);
        assert_eq!(v.texts(), ["Account 1", "0x0000...00a1", "Added", "Account 2", "0x0000...00b2", "2.500", "SUI", "Verify on device", "Show more", "Add account"]);
        assert!(v.find_nav("Account 10x0000...00a1Added").unwrap().disabled);
        update(&mut st, &Msg::Toggle(B.into()));
        let v = view(&pr, &st);
        assert!(v.contains_text("Ledger 2"));
        assert_eq!(picks(&pr, &st), vec![(rows[1].clone(), "Ledger 2".to_string())]);
        update(&mut st, &Msg::Named(B.into(), " Savings ".into()));
        assert_eq!(picks(&pr, &st)[0].1, "Savings");
        let e = hw_error("x", Some("pkexec rare-udev"), false, true);
        assert_eq!(e.texts(), ["Rare OS needs permission to use this device.", "Run this once in Konsole, then plug your device in again.", "pkexec rare-udev", "Copy command", "Try again"]);
    }
}
