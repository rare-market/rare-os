//! Parts of a library game's page (launcher/src/games/GamePanels.tsx): the play row (Play, or Resume
//! and Quit game while it runs), ES-DE's ROM systems, a web game's controls with the browser
//! fallback, and a Steam game's Proton choice.

use crate::data::model::{fmt_ago, Game};
use crate::screens::wallet::markup::*;
use crate::screens::wallet::ui::{button_el, BSize, ButtonSpec, BVariant};

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    Launch,
    Resume,
    Quit,
    OpenInBrowser,
    /// Proton: use this tool for the game (None = Valve's stable)
    SetProton(Option<String>, String),
    InstallProton(String),
    RemoveProton(String, String),
}

/// `PlayButtons`.
pub fn play_buttons(g: &Game, running: bool, launching: bool) -> Vec<El<Msg>> {
    if running {
        return vec![
            button_el(ButtonSpec::new("Resume").v(BVariant::Primary).size(BSize::Lg).def(true).hint("Resume").icon("play", "").class("min-w-[10rem]"), Some(Msg::Resume)),
            button_el(ButtonSpec::new("Quit game").size(BSize::Lg).hint("Quit game"), Some(Msg::Quit)),
        ];
    }
    let label = if launching {
        "Starting..."
    } else if g.source == "apps" {
        "Open"
    } else {
        "Play"
    };
    vec![button_el(ButtonSpec::new(label).v(BVariant::Primary).size(BSize::Lg).def(true).icon("play", "").disabled(launching).class("min-w-[10rem]"), Some(Msg::Launch))]
}

/// ES-DE's view: systems with counts, the ROMs played last.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Emulation {
    /// (id, name, count)
    pub systems: Vec<(String, String, u32)>,
    pub recent: Vec<RecentRom>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecentRom {
    pub name: String,
    pub system: String,
    /// seconds
    pub playtime: Option<u64>,
    /// unix seconds
    pub last_played: Option<i64>,
}

fn fmt_hours(s: u64) -> String {
    if s < 3600 {
        format!("{} min", (s as f64 / 60.0).round().max(1.0))
    } else {
        let h = (s as f64 / 360.0).round() / 10.0;
        if h.fract() == 0.0 {
            format!("{} h", h as i64)
        } else {
            format!("{h} h")
        }
    }
}

/// The ROM's name without its region and revision tags (`/\s*[[(].*$/`).
fn rom_name(n: &str) -> String {
    match n.find(['[', '(']) {
        Some(i) if !n[..i].trim_end().is_empty() => n[..i].trim_end().to_string(),
        _ => n.to_string(),
    }
}

pub fn emulation_panel(e: &Emulation) -> El<Msg> {
    let total: u32 = e.systems.iter().map(|s| s.2).sum();
    let sys_name = |id: &str| e.systems.iter().find(|s| s.0 == id).map(|s| s.1.clone()).unwrap_or_else(|| id.to_uppercase());
    let mut d = div("flex max-w-[75rem] gap-[1rem]").kid(
        div("min-w-0 flex-[3] rounded-panel bg-l1 px-[1.25rem] py-[1rem]")
            .kid(div("mb-[0.6rem] flex items-baseline gap-[0.6rem]").kid(span("text-small font-semibold").text("Systems")).kid(span("tabular-nums text-meta text-muted").text(format!("{total} games"))))
            .kid(div("grid grid-cols-3 gap-x-[1.2rem] gap-y-[0.35rem]").kids(e.systems.iter().take(12).map(|(id, name, count)| {
                div("flex min-w-0 items-baseline justify-between gap-[0.5rem] text-small").key(id).kid(span("truncate").text(name)).kid(span("tabular-nums shrink-0 text-muted").text(count.to_string()))
            }))),
    );
    if !e.recent.is_empty() {
        d = d.kid(
            div("min-w-0 flex-[2] rounded-panel bg-l1 px-[1.25rem] py-[1rem]").kid(div("mb-[0.6rem] text-small font-semibold").text("Played last")).kid(div("flex flex-col gap-[0.35rem]").kids(e.recent.iter().take(4).map(|g| {
                div("flex min-w-0 items-baseline gap-[0.6rem] text-small")
                    .key(format!("{}:{}", g.system, g.name))
                    .kid(span("min-w-0 flex-1 truncate").text(rom_name(&g.name)))
                    .kid(span("shrink-0 text-meta text-muted").text(format!("{} · {}", sys_name(&g.system), match g.playtime.filter(|p| *p > 0) {
                        Some(p) => fmt_hours(p),
                        None => fmt_ago(g.last_played),
                    })))
            }))),
        );
    }
    d
}

/// The web game's profile, for its controls (catalog.ts `profileForUrl`): input mode and its words.
pub struct WebProfile {
    /// "gamepad" | "keys" | "focus"
    pub input: String,
    pub controls: Option<String>,
}

/// `WebGameInfo`: how the game takes the controller; "Open in <browser>" when there is one.
pub fn web_game_info(p: Option<&WebProfile>, url: Option<&str>, browser: Option<&str>) -> El<Msg> {
    let mode = p.map(|p| p.input.as_str()).unwrap_or("keys");
    let label = match mode {
        "gamepad" => "Controller",
        "focus" => "Controller picks buttons",
        _ => "Controller as keyboard",
    };
    let words = p.and_then(|p| p.controls.clone()).unwrap_or_else(|| {
        if mode == "keys" {
            "D-pad and left stick are the arrow keys, A Enter, B Escape, X Space. The right stick moves a pointer, RB clicks. View opens the keyboard.".into()
        } else {
            "Guide returns to Rare with the game still running.".into()
        }
    });
    let mut d = div("flex max-w-[60rem] items-end gap-[1.5rem]").kid(div("min-w-0 flex-1").kid(div("text-label font-medium text-secondary").text(label)).kid(div("mt-[0.25rem] text-small text-fg/80").text(words)));
    if let (Some(_), Some(b)) = (url, browser) {
        let l = format!("Open in {b}");
        d = d.kid(button_el(ButtonSpec::new(&l).size(BSize::Sm).v(BVariant::Ghost).hint(&l), Some(Msg::OpenInBrowser)));
    }
    d
}

/// A Proton build Steam knows (`ProtonStatus.tools`).
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub name: String,
    pub label: String,
    /// "valve" | "experimental" | "ge" | "cachyos" | "other"
    pub kind: String,
    pub stale: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProtonStatus {
    pub tools: Vec<Tool>,
    /// the game's own choice
    pub game: Option<String>,
    pub global: Option<String>,
    /// "ge" | "cachyos"
    pub installable: Vec<String>,
}

pub fn kind_label(k: &str) -> &'static str {
    match k {
        "ge" => "GE-Proton",
        _ => "Proton-CachyOS",
    }
}

/// `ProtonPanel`. `busy`: a change is running; `progress` is the install phase and percent.
pub fn proton_panel(st: &ProtonStatus, busy: bool, progress: Option<(&str, u64, u64)>) -> Option<El<Msg>> {
    if st.tools.is_empty() {
        return None;
    }
    let current = st.game.as_ref().or(st.global.as_ref());
    let stable = st.tools.iter().find(|t| t.kind == "valve");
    let pick = |kinds: &[&str]| st.tools.iter().filter(|t| kinds.contains(&t.kind.as_str()) && !t.stale).collect::<Vec<_>>();
    let mut entries: Vec<(String, String, Option<String>, bool)> = Vec::new();
    if let Some(s) = stable {
        entries.push(("stable".into(), s.label.clone(), None, current == Some(&s.name) && st.game.is_none()));
    }
    for t in pick(&["experimental"]) {
        entries.push((t.name.clone(), "Experimental".into(), Some(t.name.clone()), st.game.as_ref() == Some(&t.name)));
    }
    for t in pick(&["ge", "cachyos", "other"]) {
        entries.push((t.name.clone(), t.label.clone(), Some(t.name.clone()), st.game.as_ref() == Some(&t.name)));
    }
    let mut d = div("flex max-w-[75rem] flex-wrap items-center gap-[0.5rem]").kid(span("mr-[0.4rem] text-small font-semibold").text("Proton"));
    for (key, label, name, on) in entries {
        d = d.kid(button_el(ButtonSpec::new(&label).v(if on { BVariant::Primary } else { BVariant::Ghost }).disabled(busy), (!on).then(|| Msg::SetProton(name, label.clone()))).key(key));
    }
    for k in &st.installable {
        d = d.kid(button_el(ButtonSpec::new(&format!("Install {}", kind_label(k))).v(BVariant::Ghost).disabled(busy), Some(Msg::InstallProton(k.clone()))).key(k));
    }
    for t in st.tools.iter().filter(|t| t.stale) {
        d = d.kid(button_el(ButtonSpec::new(&format!("Remove old {}", t.label)).v(BVariant::Ghost).disabled(busy), Some(Msg::RemoveProton(t.name.clone(), t.label.clone()))).key(&t.name));
    }
    if busy {
        let txt = match progress {
            Some((phase, done, total)) => {
                let name = match phase {
                    "lookup" => "Looking up the release",
                    "download" => "Downloading",
                    "verify" => "Checking the checksum",
                    "unpack" => "Unpacking",
                    _ => "",
                };
                let pct = if phase == "download" && total > 0 { format!(" {}%", ((done as f64 / total as f64) * 100.0).round()) } else { String::new() };
                format!("{name}{pct}")
            }
            None => "Restarting Steam".into(),
        };
        d = d.kid(span("text-meta text-muted").text(txt));
    }
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(source: &str) -> Game {
        serde_json::from_value(serde_json::json!({ "id": "steam:1", "source": source, "title": "Balatro" })).unwrap()
    }

    #[test]
    fn play_row() {
        let b = play_buttons(&game("steam"), false, false);
        assert_eq!(b[0].texts(), ["Play"]);
        assert_eq!(play_buttons(&game("apps"), false, false)[0].texts(), ["Open"]);
        assert!(play_buttons(&game("steam"), false, true)[0].nav.as_ref().unwrap().disabled);
        let r = play_buttons(&game("steam"), true, false);
        assert_eq!(r.len(), 2);
        assert_eq!(r[1].texts(), ["Quit game"]);
    }

    #[test]
    fn emulation() {
        let e = Emulation {
            systems: vec![("snes".into(), "Super Nintendo".into(), 120), ("gba".into(), "Game Boy Advance".into(), 40)],
            recent: vec![RecentRom { name: "Chrono Trigger (USA) [!]".into(), system: "snes".into(), playtime: Some(7_200), last_played: None }],
        };
        let t = emulation_panel(&e).texts();
        assert_eq!(&t[..2], ["Systems", "160 games"]);
        assert!(t.contains(&"Chrono Trigger".to_string()));
        assert!(t.contains(&"Super Nintendo · 2 h".to_string()));
        assert_eq!(fmt_hours(5_400), "1.5 h");
        assert_eq!(fmt_hours(30), "1 min");
    }

    #[test]
    fn web_info_and_proton() {
        let v = web_game_info(None, Some("https://x"), Some("Chrome"));
        assert_eq!(v.texts()[0], "Controller as keyboard");
        assert!(v.find_nav("Open in Chrome").is_some());
        let st = ProtonStatus {
            tools: vec![
                Tool { name: "proton_9".into(), label: "Proton 9.0".into(), kind: "valve".into(), stale: false },
                Tool { name: "GE-Proton9-1".into(), label: "GE-Proton 9-1".into(), kind: "ge".into(), stale: true },
            ],
            game: None,
            global: Some("proton_9".into()),
            installable: vec!["cachyos".into()],
        };
        let p = proton_panel(&st, false, None).unwrap();
        assert_eq!(p.texts(), ["Proton", "Proton 9.0", "Install Proton-CachyOS", "Remove old GE-Proton 9-1"]);
        let p = proton_panel(&st, true, Some(("download", 50, 200))).unwrap();
        assert!(p.contains_text("Downloading 25%"));
    }
}
