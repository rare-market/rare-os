//! Every vertical list and sheet keeps the focused row on screen: walk focus to the last item
//! with Down (and Right where a list runs sideways) and assert the focused box sits fully inside
//! every clipping ancestor and the window.

mod common;
use common::{one, report, Harness};
use rare_native::page::Btn;
use rare_native::state::Tab;

#[test]
fn settings_lists_follow_focus() {
    let _g = one();
    let mut errs = Vec::new();
    let mut h = Harness::new(Some(Tab::Settings), None);
    // The section list, top to bottom.
    let n = h.walk("settings sections", Btn::Down, 40, &mut errs);
    assert!(n >= 8, "the section list moved {n} times");
    report(errs);
}

#[test]
fn settings_panes_follow_focus() {
    let _g = one();
    let mut errs = Vec::new();
    let (mut total, mut sections) = (0, 0);
    let mut prev: Option<Vec<[f32; 4]>> = None;
    for i in 0..20 {
        let mut h = Harness::new(Some(Tab::Settings), None);
        // Focusing a section shows it; A moves into its first row.
        for _ in 0..i {
            h.press(Btn::Down);
        }
        h.settle();
        let list = h.focus_rects();
        if prev.as_ref() == Some(&list) {
            break; // past the last section
        }
        prev = Some(list.clone());
        let list_right = list.first().map(|r| r[0] + r[2]).unwrap_or(0.0);
        h.press(Btn::A);
        h.settle();
        if h.focus_rects().iter().all(|r| r[0] < list_right) {
            continue; // nothing focusable in this pane
        }
        sections += 1;
        total += h.walk_in(&format!("settings pane {i}"), Btn::Down, 60, &mut errs, |r| r[0] >= list_right);
    }
    eprintln!("settings panes: {sections} sections, {total} moves");
    assert!(sections >= 8 && total > 20, "the panes: {sections} sections, {total} moves");
    report(errs);
}

#[test]
fn quick_settings_follow_focus() {
    let _g = one();
    let mut errs = Vec::new();
    let mut h = Harness::new(Some(Tab::Home), None);
    h.press(Btn::Qam);
    h.settle();
    let n = h.walk("quick settings", Btn::Down, 60, &mut errs);
    assert!(n >= 2, "quick settings moved {n} times");
    report(errs);
}

#[test]
fn rare_menu_tabs_follow_focus() {
    let _g = one();
    let mut errs = Vec::new();
    let mut h = Harness::new(Some(Tab::Home), None);
    h.press(Btn::Guide);
    h.settle();
    let mut total = 0;
    for i in 0..6 {
        if i > 0 {
            h.press(Btn::Rt);
            h.settle();
        }
        total += h.walk(&format!("rare menu tab {i}"), Btn::Down, 60, &mut errs);
    }
    assert!(total > 3, "the Rare menu moved {total} times");
    report(errs);
}

#[test]
fn options_sheet_follows_focus() {
    let _g = one();
    let mut errs = Vec::new();
    let mut h = Harness::new(Some(Tab::Home), None);
    // Menu on a game: its Options sheet.
    h.press(Btn::Start);
    h.settle();
    let n = h.walk("options sheet", Btn::Down, 40, &mut errs);
    assert!(n > 1, "Options moved {n} times");
    report(errs);
}
