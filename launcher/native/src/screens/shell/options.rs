//! The two button sheets of the UX model (components/Options.tsx): Menu opens Options for the
//! focused game, View opens the screen's view. One list in the standard sheet. B, Menu or View
//! close it, and every other press is swallowed so nothing moves behind it.

use crate::act::Act;
use crate::kit::{self, s, Btn as B, BtnIcon, Ui};
use crate::page::{hash, Acts, Btn, Feedback, Fx, Page};
use crate::state::{OptionItem, OptionsSpec, State};
use rare_render::focus::hints::{hint, Hint, Key};
use rare_render::focus::Focusable;
use rare_render::focus::Group;
use rare_render::view::El;

/// The focus scope of the Options sheet (above the page and any overlay).
pub const SCOPE: u32 = 2;

#[derive(Default)]
pub struct Options;

impl Page for Options {
    fn stamp(&self, st: &State) -> u64 {
        hash(st.options.as_ref().map(|o| (o.title.clone(), o.items.iter().map(|i| (i.label.clone(), i.on)).collect::<Vec<_>>())))
    }

    fn view(&mut self, ui: &mut Ui, st: &State, acts: &mut Acts) -> El {
        let Some(spec) = &st.options else { return ui.bx(s()) };
        let any_on = spec.items.iter().any(|i| i.on == Some(true));
        let mut list = ui.bx(s().col().gap(0.5)).group(Group::new("options", [0.0; 4]));
        for (i, it) in spec.items.iter().enumerate() {
            let tag = acts.tag(Act::Local(i as u32));
            let mut b = B::new(it.label.clone(), tag).lg().full_start().gap(0.75).def(if any_on { it.on == Some(true) } else { i == 0 });
            if let Some(on) = it.on {
                b = b.icon(BtnIcon::Check(on));
            }
            list = list.child(kit::button(ui, b));
        }
        kit::sheet(ui, &spec.title, spec.sub.as_deref(), vec![list], vec![], false, SCOPE)
    }

    fn pad(&mut self, st: &mut State, fx: &mut Fx, b: Btn, _cur: Option<(&Focusable, Option<&Act>)>) -> bool {
        match b {
            Btn::B | Btn::Start | Btn::Select => {
                fx.fb(Feedback::Back);
                st.options = None;
                true
            }
            Btn::Lb | Btn::Rb | Btn::Lt | Btn::Rt | Btn::X | Btn::Y => true,
            _ => false,
        }
    }

    fn act(&mut self, st: &mut State, fx: &mut Fx, code: u32) {
        if let Some(it) = st.options.take().and_then(|o| o.items.get(code as usize).cloned()) {
            fx.run(it.act);
        }
    }

    fn hints(&self, _st: &State) -> Vec<Hint> {
        vec![hint(Key::B, "Close")]
    }
}

/// Menu: Options for a game or app (Options.tsx gameOptions), plus Quit while it runs and the
/// player's own library organisation.
pub fn game_options(st: &State, id: &str) -> Option<OptionsSpec> {
    let g = st.game(id)?;
    let running = st.is_running(id);
    let fav = st.is_fav(id);
    // An Epic or GOG game that is not installed: Install (the drive sheet) instead of Play.
    let install = !g.installed && !running && crate::screens::game::stores::managed(g);
    let mut items = vec![if install {
        OptionItem { label: "Install".into(), on: None, act: Act::Legacy(format!("stores:install:{id}")) }
    } else {
        OptionItem { label: if running { "Resume" } else if g.source == "apps" { "Open" } else { "Play" }.into(), on: None, act: Act::Launch(id.into()) }
    }];
    if running {
        items.push(OptionItem { label: if g.source == "apps" { "Close" } else { "Quit game" }.into(), on: None, act: Act::QuitGame(id.into()) });
    }
    // Uninstall right under Play (Menu, Down, A), as on the game page.
    if crate::features::uninstall::can_uninstall(g) && !running {
        let label = if g.source == "web" { "Remove".to_string() } else { crate::features::uninstall::label(g) };
        items.push(OptionItem { label, on: None, act: Act::Uninstall(id.into()) });
    }
    if g.installed && !running && matches!(g.source.as_str(), "steam" | "epic" | "gog" | "local" | "itch") {
        items.push(OptionItem { label: "Move".into(), on: None, act: Act::Legacy(format!("storage:move-game:{id}")) });
    }
    items.push(OptionItem { label: "Details".into(), on: None, act: Act::OpenGame(id.into()) });
    items.push(OptionItem { label: if fav { "Remove from favourites" } else { "Add to favourites" }.into(), on: None, act: Act::ToggleFav(id.into()) });
    items.push(OptionItem { label: "Add to collection".into(), on: None, act: Act::Collections(id.into()) });
    // Copies of one game: split a wrong group, merge one the matcher missed (features/copies.rs).
    if g.source != "apps" {
        if st.copy_ids(id).len() > 1 {
            items.push(OptionItem { label: "Not the same game".into(), on: None, act: Act::NotSame(id.into()) });
        }
        items.push(OptionItem { label: "Same game as another".into(), on: None, act: Act::SameGame(id.into()) });
    }
    if g.source == "steam" {
        let act = id.strip_prefix("steam:").and_then(|a| a.parse::<u32>().ok()).map(Act::SteamGamePage).unwrap_or(Act::SteamBigPicture);
        items.push(OptionItem { label: "Open in Steam".into(), on: None, act });
    }
    items.push(OptionItem { label: if st.is_hidden(id) { "Show in library" } else { "Hide from library" }.into(), on: None, act: Act::ToggleHidden(id.into()) });
    Some(OptionsSpec { title: st.shown(g).title.clone(), sub: Some("Options".into()), items })
}

/// Add to collection: every collection with a check when the game is in it, then New collection.
pub fn collection_options(st: &State, id: &str) -> Option<OptionsSpec> {
    let g = st.game(id)?;
    let mut items: Vec<OptionItem> = st.user.collections.iter().map(|c| OptionItem { label: c.name.clone(), on: Some(st.in_collection(&c.name, id)), act: Act::CollectionToggle(c.name.clone(), id.into()) }).collect();
    items.push(OptionItem { label: "New collection".into(), on: None, act: Act::NewCollection(Some(id.into())) });
    Some(OptionsSpec { title: "Add to collection".into(), sub: Some(g.title.clone()), items })
}

/// Quit game? (the Rare menu's armed second press, as a sheet).
pub fn quit_options(st: &State, id: &str) -> OptionsSpec {
    let name = st.run_name(id);
    OptionsSpec {
        title: format!("Quit {name}?"),
        sub: Some("Progress the game has not saved is lost.".into()),
        items: vec![OptionItem { label: "Quit game".into(), on: None, act: Act::QuitNow(id.into()) }, OptionItem { label: "Keep playing".into(), on: None, act: Act::None }],
    }
}
