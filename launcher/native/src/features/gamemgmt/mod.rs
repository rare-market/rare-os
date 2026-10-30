//! Game management from the game page and the Library: Check files (Steam's own check through
//! steam://validate, or a saved file list for games outside the stores), a game's own
//! performance settings (power, frame limit, upscaling, render size, refresh rate; applied by
//! rare-system's power session when it starts and undone when it ends), adding a game from a
//! file or folder with a controller file picker, and renaming a game or changing its art
//! (rare-library's custom.rs, kept per game).
//!
//! - `verify`: the checks, as jobs on threads with a status the page polls.
//! - `art`: art choices by name (SteamGridDB with the player's key, else Steam's store).
//! - `picker`: the file picker page (`Overlay::Other(PICK)`).
//! - `artpage`: the art page (`Overlay::Other(ART)`).
//! - The game page's section and sheet are in screens/game/manage.rs.

pub mod art;
pub mod artpage;
pub mod picker;
pub mod verify;

use crate::page::Page;
use rare_library::custom::{ArtKind, Custom};
use rare_system::gamefx::{FgStatus, Status};
use rare_system::power::GamePower;
use rare_system::upscale::Upscale;
use std::collections::HashMap;
use std::sync::Mutex;

pub const PICK: &str = "gm-pick";
pub const ART: &str = "gm-art";

pub fn overlay_page(kind: &str) -> Option<Box<dyn Page>> {
    match kind {
        PICK => Some(Box::new(picker::Picker::new(picker::take_mode()))),
        ART => Some(Box::new(artpage::ArtPage::new(artpage::target()))),
        _ => None,
    }
}

/// A game's performance settings as its page shows them.
#[derive(Clone, Debug, PartialEq)]
pub struct GamePerf {
    /// What is saved for the game; None: nothing (Balanced, no limit, Settings' upscaling).
    pub saved: Option<GamePower>,
    /// The upscaling Settings has, which a game without its own uses.
    pub global_upscale: Upscale,
    /// Watts of Quiet, Balanced and Performance on this device.
    pub watts: [f32; 3],
    /// What frame generation and the filters can do here (rare_system::gamefx).
    pub fx: Status,
}

/// The fixtures' frame generation and filter support: RR_FX=nodll, nolayer, old or nofilters
/// (anything else: all there), RR_FX_RESHADE=<n> ReShade shaders found.
pub fn fixture_fx() -> Status {
    let v = std::env::var("RR_FX").unwrap_or_default();
    let framegen = match v.as_str() {
        "nodll" => FgStatus::NoDll,
        "nolayer" => FgStatus::NoLayer,
        "old" => FgStatus::OldBranch,
        _ => FgStatus::Ready,
    };
    Status { framegen, filters: v != "nofilters", reshade: std::env::var("RR_FX_RESHADE").ok().and_then(|n| n.parse().ok()).unwrap_or(0) }
}

/// Fixtures for the browser-mode backend (screenshots, the Mac): changes live in memory.
#[derive(Default)]
pub struct MockStore {
    perf: Mutex<HashMap<String, GamePower>>,
    custom: Mutex<HashMap<String, Custom>>,
}

impl MockStore {
    pub fn perf(&self, id: &str) -> GamePerf {
        GamePerf { saved: self.perf.lock().unwrap().get(id).copied(), global_upscale: Upscale::default(), watts: [10.0, 15.0, 25.0], fx: fixture_fx() }
    }
    pub fn set_perf(&self, id: &str, p: Option<GamePower>) {
        let mut m = self.perf.lock().unwrap();
        match p {
            Some(p) => m.insert(id.to_string(), p),
            None => m.remove(id),
        };
    }
    pub fn custom(&self, id: &str) -> Custom {
        self.custom.lock().unwrap().get(id).cloned().unwrap_or_default()
    }
    pub fn set_custom(&self, id: &str, c: Custom) {
        self.custom.lock().unwrap().insert(id.to_string(), c);
    }
}

/// Device: what is saved in launcher.json's gamePower.
pub fn device_perf(id: &str) -> GamePerf {
    let s = crate::data::backend::power_load();
    let p = rare_system::system::perf_get(&rare_system::power::wanted_now(&s));
    GamePerf {
        saved: s.game_power.get(id).copied(),
        global_upscale: rare_system::upscale::saved(),
        watts: [p.profile_watts.quiet as f32, p.profile_watts.balanced as f32, p.profile_watts.performance as f32],
        fx: rare_system::gamefx::status(),
    }
}

/// Device: saves (None: forgets) a game's settings; a game that has the screen gets them at once.
pub fn device_set_perf(id: &str, p: Option<GamePower>) -> Result<(), String> {
    let mut s = crate::data::backend::power_load();
    match p {
        Some(p) => s.game_power.insert(id.to_string(), p),
        None => s.game_power.remove(id),
    };
    crate::data::backend::power_save(&s);
    if rare_system::power::current_game().as_deref() == Some(id) {
        rare_system::power::reapply(crate::data::backend::power_load);
    }
    Ok(())
}

/// Puts a change on the game in the library the screens show, until the next scan has it.
pub fn show_custom(st: &mut crate::state::State, id: &str, c: &Custom) {
    // A renamed copy names its group (data/groups.rs).
    if c.title.as_ref().is_some_and(|t| !t.trim().is_empty()) {
        st.renamed.insert(id.to_string());
    } else {
        st.renamed.remove(id);
    }
    let Some(lib) = st.library.as_mut() else { return };
    let Some(g) = lib.games.iter_mut().chain(lib.cloud.iter_mut()).chain(lib.owned.iter_mut()).find(|g| g.id == id) else { return };
    if let Some(t) = c.title.as_ref().filter(|t| !t.trim().is_empty()) {
        g.title = t.trim().to_string();
    }
    for (pick, list) in [(&c.cover, &mut g.art.cover), (&c.hero, &mut g.art.hero), (&c.logo, &mut g.art.logo)] {
        if let Some(p) = pick.as_ref().filter(|p| !p.is_empty()) {
            list.retain(|x| x != p);
            list.insert(0, p.clone());
        }
    }
    if let Some(h) = c.hero.as_ref().filter(|p| !p.is_empty()) {
        g.art.header.retain(|x| x != h);
        g.art.header.insert(0, h.clone());
    }
    st.regroup();
    st.ver.library += 1;
}

pub fn kind_name(k: ArtKind) -> &'static str {
    match k {
        ArtKind::Cover => "Cover",
        ArtKind::Hero => "Hero",
        ArtKind::Logo => "Logo",
    }
}
