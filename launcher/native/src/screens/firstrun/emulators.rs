//! First run: which emulators to set up. One line per emulator with the systems it plays, ticked
//! when the player already has games for it, its size; Set up (the ticked ones), All, or Later.
//! The same choice is in Library, Emulation.

use super::kit::step;
use crate::screens::wallet::markup::*;
use crate::screens::wallet::ui::{button_el, BSize, BVariant, ButtonSpec};
use rare_emu::setup::{size_text, Choice};

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Toggle(String),
    Now,
    All,
    Later,
    Back,
}

fn sub(c: &Choice) -> String {
    let mut s = c.name.clone();
    if c.installed {
        return s;
    }
    s.push_str(&format!(" \u{b7} {} MB", c.size_mb));
    match c.roms {
        0 => {}
        1 => s.push_str(" \u{b7} 1 game"),
        n => s.push_str(&format!(" \u{b7} {n} games")),
    }
    s
}

fn row(c: &Choice, on: bool, first: bool) -> El<Msg> {
    button(cn(&["flex w-full items-center gap-[1rem] rounded-card border px-[1.25rem] py-[0.85rem] text-left", if on { "border-line-2 bg-l2" } else { "border-hair bg-l1" }]), (!c.installed).then(|| Msg::Toggle(c.id.clone())))
        .def(first)
        .role("checkbox")
        .attr("aria-checked", (on || c.installed).to_string())
        .key(&c.id)
        .kid(span("min-w-0 flex-1").kid(span("block truncate text-body font-semibold").text(&c.plays)).kid(span("mt-[0.15rem] block truncate text-small text-muted").text(sub(c))))
        .kid(if c.installed { span("shrink-0 text-small text-muted").text("Installed") } else { check(on) })
}

/// A tick box: filled with a check when on.
fn check(on: bool) -> El<Msg> {
    span(cn(&["flex h-[1.5rem] w-[1.5rem] shrink-0 items-center justify-center rounded-[0.4rem] border", if on { "border-fg bg-fg text-bg" } else { "border-line-2" }]))
        .attr("aria-hidden", "true")
        .kid_if(on, || icon("check", "").attr("size", "1rem"))
}

/// `picked`: the ticked ids (installed ones are not in it).
pub fn view(choices: &[Choice], picked: &[String]) -> El<Msg> {
    let n = picked.len();
    // The first line to tick lands focus; with everything installed, Continue does.
    let first = choices.iter().position(|c| !c.installed).unwrap_or(usize::MAX);
    let rows: Vec<El<Msg>> = choices.iter().enumerate().map(|(i, c)| row(c, picked.contains(&c.id), i == first)).collect();
    let all_in = choices.iter().all(|c| c.installed);
    let size = size_text(choices, picked);
    let content = vec![div("flex flex-col gap-[0.5rem]").attr("data-nav-group", "emu-choice").attr("data-nav-remember", "").kids(rows)];
    let note = if all_in {
        "Every emulator is installed. Controls are set for the SuiPlay's buttons.".to_string()
    } else if n == 0 {
        "Nothing ticked. Tick the systems you play, or set up everything.".to_string()
    } else {
        size
    };
    let label = match n {
        0 => "Set up".to_string(),
        1 => "Set up 1 emulator".to_string(),
        n => format!("Set up {n} emulators"),
    };
    let actions = if all_in {
        vec![button_el(ButtonSpec::new("Continue").v(BVariant::Primary).size(BSize::Lg).def(true), Some(Msg::Later))]
    } else {
        vec![
            button_el(ButtonSpec::new(&label).v(BVariant::Primary).size(BSize::Lg).disabled(n == 0), (n > 0).then_some(Msg::Now)),
            button_el(ButtonSpec::new("All").v(BVariant::Ghost).size(BSize::Lg), Some(Msg::All)),
            button_el(ButtonSpec::new("Later").v(BVariant::Ghost).size(BSize::Lg), Some(Msg::Later)),
        ]
    };
    let mut actions = actions;
    actions.push(span("ml-[0.5rem] min-w-0 flex-1 truncate text-small text-muted").text(note));
    step(
        text("Emulators"),
        Some(text("Rare installs the emulators for the systems you pick and sets every button for the SuiPlay. Games you copy into Emulation/roms show up in Library. You can change this any time in Library, Emulation.")),
        content,
        actions,
        false,
    )
    .on_pad("b", Msg::Back)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, plays: &str, installed: bool, roms: usize) -> Choice {
        Choice { id: id.into(), name: id.into(), plays: plays.into(), size_mb: 60, installed, roms, picked: !installed && roms > 0 }
    }

    #[test]
    fn emulators() {
        let list = vec![c("dolphin", "GameCube and Wii", false, 3), c("pcsx2", "PlayStation 2", false, 0), c("xemu", "Xbox", true, 0)];
        let v = view(&list, &["dolphin".to_string()]);
        assert!(v.contains_text("GameCube and Wii") && v.contains_text("dolphin \u{b7} 60 MB \u{b7} 3 games") && v.contains_text("Installed") && v.contains_text("About 60 MB to download"));
        assert!(v.contains_text("Set up 1 emulator") && v.contains_text("All") && v.contains_text("Later"));
        let none = view(&list, &[]);
        assert!(none.contains_text("Nothing ticked. Tick the systems you play, or set up everything."));
        let done = view(&[c("xemu", "Xbox", true, 0)], &[]);
        assert_eq!(done.landing().as_deref(), Some("Continue"));
    }
}
