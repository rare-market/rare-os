//! The player's calls on copies of one game (tile Options): Same game as (merge two the matcher
//! missed) and Not the same game (split a wrong group). Saved in userlib.json; data/groups.rs
//! applies them.

use crate::act::Act;
use crate::data::groups::copy_label;
use crate::page::{Feedback, Fx};
use crate::state::{OptionItem, OptionsSpec, State, Tone};

/// The most games Same game as offers.
const MAX: usize = 6;

/// The games that may be this one, the likeliest first: one per game, never an app. With `q`,
/// only games whose name has every word typed (ranked the same way).
pub fn candidates<'a>(st: &'a State, id: &str, q: Option<&str>) -> Vec<&'a crate::data::model::Game> {
    let Some(me) = st.game(id) else { return vec![] };
    let mine = st.copy_ids(id);
    let Some(l) = st.library.as_ref() else { return vec![] };
    let want: Vec<String> = q.map(|q| q.to_lowercase().split_whitespace().map(|w| rare_library::dupes::norm(w)).filter(|w| !w.is_empty()).collect()).unwrap_or_default();
    let pool = l.games.iter().chain(l.owned.iter()).chain(l.cloud.iter()).filter(|g| !mine.contains(&g.id) && g.source != "apps" && !rare_android::is_store(&g.id));
    let mut v: Vec<(f32, &crate::data::model::Game)> = pool
        .filter_map(|g| {
            // The matcher's own rules say no (two Steam apps, ROMs of two systems): the player can
            // still say yes, so only the title counts here.
            let a = rare_library::dupes::Entry { id: "", source: "local", title: &me.title, key: None };
            let b = rare_library::dupes::Entry { id: "", source: "local:other", title: &g.title, key: None };
            let s = rare_library::dupes::score(&a, &b);
            if want.is_empty() {
                (s >= 0.1).then_some((s, g))
            } else {
                let t = rare_library::dupes::norm(&g.title);
                want.iter().all(|w| t.contains(w.as_str())).then_some((s, g))
            }
        })
        .collect();
    v.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.1.title.cmp(&b.1.title)));
    st.one_each(v.into_iter().map(|(_, g)| g)).into_iter().take(MAX).collect()
}

/// The Same game as sheet: Search by name first, then the games.
fn sheet(st: &mut State, fx: &mut Fx, id: &str, q: Option<&str>, now: f64) {
    let Some(title) = st.game(id).map(|g| st.shown(g).title.clone()) else { return };
    let mut items = vec![OptionItem { label: if q.is_some() { "Search again".into() } else { "Search by name".into() }, on: None, act: Act::SameGameFind(id.to_string(), String::new()) }];
    let found: Vec<OptionItem> = candidates(st, id, q).into_iter().map(|g| OptionItem { label: format!("{} \u{b7} {}", g.title, copy_label(g)), on: None, act: Act::MergeGames(id.to_string(), g.id.clone()) }).collect();
    if let (true, Some(q)) = (found.is_empty(), q) {
        st.toast(format!("No game in your library matches {q}"), Tone::Info, now);
    }
    items.extend(found);
    let sub = match q {
        Some(q) => format!("{title} \u{b7} names with {q}"),
        None => title,
    };
    fx.fb(Feedback::Open);
    st.options = Some(OptionsSpec { title: "Same game as".into(), sub: Some(sub), items });
}

/// Search by name: the keyboard, then the sheet with what matches.
fn ask(st: &mut State, id: &str) {
    st.options = None;
    st.osk = Some(crate::state::OskReq::new("Name of the other game", "", Some("Borderlands"), crate::state::OskFor::SameGame(id.to_string())));
}

pub fn run(st: &mut State, fx: &mut Fx, a: Act, now: f64) {
    match a {
        Act::SameGame(id) => sheet(st, fx, &id, None, now),
        // Empty: Search by name (the keyboard); else what the keyboard sent.
        Act::SameGameFind(id, q) if q.trim().is_empty() => ask(st, &id),
        Act::SameGameFind(id, q) => sheet(st, fx, &id, Some(&q), now),
        Act::MergeGames(a, b) => {
            let (ga, gb) = (st.copy_ids(&a), st.copy_ids(&b));
            st.user.merge(&ga, &gb);
            st.user_changed();
            let title = st.game(&a).map(|g| st.shown(g).title.clone()).unwrap_or_default();
            st.toast(format!("{title} shows as one game now. Pick a copy under Play on."), Tone::Info, now);
        }
        Act::NotSame(id) => {
            let ids = st.copy_ids(&id);
            if ids.len() > 2 {
                let items: Vec<OptionItem> = ids.iter().filter_map(|c| st.game(c)).map(|g| OptionItem { label: format!("{} \u{b7} {}", g.title, copy_label(g)), on: None, act: Act::SplitCopy(g.id.clone()) }).collect();
                fx.fb(Feedback::Open);
                st.options = Some(OptionsSpec { title: "Which one is a different game?".into(), sub: Some("It gets its own tile again".into()), items });
            } else if ids.len() == 2 {
                split(st, &id, now);
            }
        }
        Act::SplitCopy(id) => split(st, &id, now),
        _ => {}
    }
}

fn split(st: &mut State, id: &str, now: f64) {
    let ids = st.copy_ids(id);
    let names: Vec<String> = ids.iter().filter_map(|c| st.game(c)).map(|g| g.title.clone()).collect();
    st.user.split(id, &ids);
    st.user_changed();
    let text = match names.as_slice() {
        [a, b] if a != b => format!("{a} and {b} show apart now"),
        _ => format!("{} shows on its own now", st.run_name(id)),
    };
    st.toast(text, Tone::Info, now);
}
