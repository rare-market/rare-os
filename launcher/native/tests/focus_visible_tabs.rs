//! Focus stays visible in the tab-side lists and sheets: Wallet's pages and sheets, Friends,
//! notifications, Market's filters and sort, the coin picker, the keyboard and Search's results.
//! Each case opens the surface as a user would, walks focus to its last item and asserts the
//! focused box is fully inside every clipping ancestor and the window after every press.

mod common;
use common::{report, Harness};
use rare_native::page::Btn;
use rare_native::state::Tab;
use std::sync::Mutex;

/// The app keeps process-wide state (the event bus, env-driven fixtures): one case at a time.
static ONE: Mutex<()> = Mutex::new(());

fn env(k: &str, v: &str) {
    std::env::set_var(k, v);
}

fn walked(name: &str, h: &mut Harness, dir: Btn, errs: &mut Vec<String>) -> usize {
    eprintln!("{name}: start at {}", h.focus_text());
    let n = h.walk(name, dir, 60, errs);
    eprintln!("{name}: {n} moves, ends at {}", h.focus_text());
    if n == 0 && errs.is_empty() {
        errs.push(format!("{name}: focus never moved (the case does not reach the list)"));
    }
    n
}

#[test]
fn wallet_pages_and_sheets() {
    let _g = ONE.lock().unwrap_or_else(|e| e.into_inner());
    env("RR_FIXTURE", "1");
    let mut errs = Vec::new();
    for (wv, name) in [("accounts", "wallet accounts"), ("settings", "wallet settings"), ("add", "wallet add account")] {
        env("RR_WALLET", wv);
        let mut h = Harness::new(Some(Tab::Wallet), None);
        let n = walked(name, &mut h, Btn::Down, &mut errs);
        if n == 0 && wv == "accounts" {
            // One fixture account: nothing to walk.
            errs.retain(|e| !e.starts_with("wallet accounts: focus never moved"));
        }
        // Settings opens part way down its list: back up to the top as well.
        if wv == "settings" {
            walked("wallet settings, up", &mut h, Btn::Up, &mut errs);
        }
    }
    std::env::remove_var("RR_WALLET");
    report(errs);
}

#[test]
fn friends_list_and_detail() {
    let _g = ONE.lock().unwrap_or_else(|e| e.into_inner());
    env("RR_FIXTURE", "1");
    env("RR_FRIENDS", "mock");
    let mut errs = Vec::new();
    let mut h = Harness::new(Some(Tab::Friends), None);
    walked("friends list", &mut h, Btn::Down, &mut errs);
    h.seq(&[Btn::Right]);
    walked("friends detail", &mut h, Btn::Down, &mut errs);
    report(errs);
}

#[test]
fn notifications() {
    let _g = ONE.lock().unwrap_or_else(|e| e.into_inner());
    env("RR_FIXTURE", "1");
    let mut errs = Vec::new();
    let mut h = Harness::new(Some(Tab::Home), None);
    // The Rare menu: Rare, Friends, Notifications; RB steps the tabs.
    h.seq(&[Btn::Guide]);
    eprintln!("rare menu: at {}", h.focus_text());
    // LT / RT step the menu's tabs: Rare, Friends, Notifications.
    h.seq(&[Btn::Rt]);
    walked("rare menu friends", &mut h, Btn::Down, &mut errs);
    h.seq(&[Btn::Rt]);
    walked("notifications", &mut h, Btn::Down, &mut errs);
    report(errs);
}

#[test]
fn market_filters_and_sort() {
    let _g = ONE.lock().unwrap_or_else(|e| e.into_inner());
    env("RR_FIXTURE", "1");
    let mut errs = Vec::new();
    let mut h = Harness::new(Some(Tab::Market), None);
    h.pump(1500);
    eprintln!("market: at {}", h.focus_text());
    // A collection, then X: the Filters panel (Sort, then the trait groups).
    h.seq(&[Btn::A]);
    h.pump(1500);
    h.seq(&[Btn::X]);
    h.pump(3000);
    walked("market filters", &mut h, Btn::Down, &mut errs);
    // The last trait group opened: its values run on below.
    h.seq(&[Btn::A]);
    if walked("market filters, a trait group open", &mut h, Btn::Down, &mut errs) == 0 {
        // No traits came back for the collection (offline): nothing more to walk.
        errs.retain(|e| !e.starts_with("market filters, a trait group open: focus never moved"));
        eprintln!("market filters: no trait groups loaded");
    }
    // The sort button opens the same panel.
    h.seq(&[Btn::B]);
    h.pump(2000);
    if h.seek("Sweep", 30) {
        h.seq(&[Btn::Left]);
        eprintln!("market sort: at {}", h.focus_text());
        h.seq(&[Btn::A]);
        // The panel opens on the current sort (the last one, picked above): walk back up.
        walked("market sort", &mut h, Btn::Up, &mut errs);
    } else {
        errs.push(format!("market sort: no sort button found (at {})", h.focus_text()));
    }
    report(errs);
}

/// The same walk on live rare.market answers (the app's fixture has no trait data for a
/// collection): run by hand with `--ignored` when the network is up.
#[test]
#[ignore]
fn market_filters_live() {
    let _g = ONE.lock().unwrap_or_else(|e| e.into_inner());
    std::env::remove_var("RR_FIXTURE");
    let mut errs = Vec::new();
    let mut h = Harness::new(Some(Tab::Market), None);
    h.pump(6000);
    h.seq(&[Btn::A]);
    h.pump(5000);
    h.seq(&[Btn::X]);
    h.pump(5000);
    walked("market filters live", &mut h, Btn::Down, &mut errs);
    h.seq(&[Btn::A]);
    h.pump(500);
    walked("market filters live, a trait group open", &mut h, Btn::Down, &mut errs);
    env("RR_FIXTURE", "1");
    report(errs);
}

#[test]
fn coin_picker() {
    let _g = ONE.lock().unwrap_or_else(|e| e.into_inner());
    env("RR_FIXTURE", "1");
    let mut errs = Vec::new();
    let mut h = Harness::new(Some(Tab::Market), None);
    h.pump(1500);
    // RT: Trade's home; its Swap button; the Pay with coin opens the picker.
    h.seq(&[Btn::Rt]);
    h.pump(1500);
    for _ in 0..8 {
        h.press(Btn::Up);
    }
    h.settle();
    if !h.seek("Swap", 30) {
        errs.push(format!("coin picker: no Swap button (at {})", h.focus_text()));
        report(errs);
        return;
    }
    h.seq(&[Btn::A]);
    h.pump(1500);
    eprintln!("swap: at {}", h.focus_text());
    // Review swap opens the review sheet (no account: Add a passkey).
    h.seq(&[Btn::A]);
    h.pump(800);
    walked("swap review sheet", &mut h, Btn::Down, &mut errs);
    errs.retain(|e| !e.starts_with("swap review sheet: focus never moved"));
    h.seq(&[Btn::B]);
    // The Pay with coin (SUI): the picker sheet.
    for _ in 0..6 {
        h.press(Btn::Up);
    }
    h.settle();
    if !h.seek("SUI", 20) {
        errs.push(format!("coin picker: no coin button (at {})", h.focus_text()));
        report(errs);
        return;
    }
    h.seq(&[Btn::A]);
    h.pump(1500);
    walked("coin picker", &mut h, Btn::Down, &mut errs);
    report(errs);
}

#[test]
fn keyboard_and_search_results() {
    let _g = ONE.lock().unwrap_or_else(|e| e.into_inner());
    env("RR_FIXTURE", "1");
    let mut errs = Vec::new();
    let mut h = Harness::new(Some(Tab::Home), Some("osk"));
    walked("keyboard", &mut h, Btn::Down, &mut errs);
    env("RR_SEARCH", "a");
    let mut h = Harness::new(Some(Tab::Home), Some("search"));
    walked("search results", &mut h, Btn::Down, &mut errs);
    std::env::remove_var("RR_SEARCH");
    report(errs);
}
