//! Power per game. A game Rare starts gets its own power settings (profile or watts, CPU boost,
//! gamescope frame limit): the ones saved for it with "Apply to this game" in quick settings, or
//! Balanced (15 W, CPU boost off, no frame limit) for a game without any. When the last game
//! ends, the launcher's own profile comes back. Changes made in quick settings while a game runs
//! belong to that game: saved for it when it has saved settings, for this run otherwise. The
//! launcher's own profile is never changed from inside a game.
//!
//! Everything is applied through system::perf_apply and system::set_fps_limit (PowerStation
//! first, Handheld Daemon on stock Bazzite), on a thread of its own: D-Bus calls never hold up
//! a launch.

use crate::perfhud::HudLevel;
use crate::system::{self, Profile, Wanted};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// The launcher-config fields power needs. The launcher's Config implements it; `PowerSettings`
/// reads the same fields straight from the config file (the native UI).
pub trait PowerCfg {
    fn perf_profile(&self) -> Option<Profile>;
    fn perf_tdp(&self) -> Option<f64>;
    fn perf_cpu_boost(&self) -> Option<bool>;
    fn perf_hud(&self) -> Option<HudLevel>;
    fn game_power(&self) -> &BTreeMap<String, GamePower>;
    fn game_power_mut(&mut self) -> &mut BTreeMap<String, GamePower>;
    fn set_perf_profile(&mut self, v: Option<Profile>);
    fn set_perf_tdp(&mut self, v: Option<f64>);
    fn set_perf_cpu_boost(&mut self, v: Option<bool>);
    fn set_perf_hud(&mut self, v: Option<HudLevel>);
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct PowerSettings {
    pub perf_profile: Option<Profile>,
    pub perf_tdp: Option<f64>,
    pub perf_cpu_boost: Option<bool>,
    pub perf_hud: Option<HudLevel>,
    pub game_power: BTreeMap<String, GamePower>,
}

impl PowerCfg for PowerSettings {
    fn perf_profile(&self) -> Option<Profile> {
        self.perf_profile
    }
    fn perf_tdp(&self) -> Option<f64> {
        self.perf_tdp
    }
    fn perf_cpu_boost(&self) -> Option<bool> {
        self.perf_cpu_boost
    }
    fn perf_hud(&self) -> Option<HudLevel> {
        self.perf_hud
    }
    fn game_power(&self) -> &BTreeMap<String, GamePower> {
        &self.game_power
    }
    fn game_power_mut(&mut self) -> &mut BTreeMap<String, GamePower> {
        &mut self.game_power
    }
    fn set_perf_profile(&mut self, v: Option<Profile>) {
        self.perf_profile = v;
    }
    fn set_perf_tdp(&mut self, v: Option<f64>) {
        self.perf_tdp = v;
    }
    fn set_perf_cpu_boost(&mut self, v: Option<bool>) {
        self.perf_cpu_boost = v;
    }
    fn set_perf_hud(&mut self, v: Option<HudLevel>) {
        self.perf_hud = v;
    }
}

/// One game's power settings.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GamePower {
    pub profile: Profile,
    /// Watts with Custom.
    #[serde(default)]
    pub tdp: Option<f64>,
    /// CPU boost set by hand, over the profile's.
    #[serde(default)]
    pub cpu_boost: Option<bool>,
    /// gamescope frame limit; None is no limit.
    #[serde(default)]
    pub fps: Option<u32>,
    /// Performance overlay level (mangoapp), remembered with the rest.
    #[serde(default)]
    pub hud: HudLevel,
    /// gamescope upscaling and render size for this game; None keeps the one in Settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upscale: Option<crate::upscale::Upscale>,
    /// Refresh rate in Hz for this game; None is the screen's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh: Option<u32>,
    /// Frame generation (lsfg-vk, gamefx.rs); None: never set for this game.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framegen: Option<crate::gamefx::FrameGen>,
    /// Picture filters (vkBasalt, gamefx.rs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filters: Option<crate::gamefx::Filters>,
}

impl GamePower {
    /// A game nothing was saved for: Balanced, 15 W with CPU boost off, no frame limit.
    pub const DEFAULT: GamePower = GamePower { profile: Profile::Balanced, tdp: None, cpu_boost: None, fps: None, hud: HudLevel::Off, upscale: None, refresh: None, framegen: None, filters: None };

    pub fn wanted(&self) -> Wanted {
        Wanted { profile: self.profile, tdp: self.tdp, cpu_boost: self.cpu_boost }
    }

    pub fn with_hud(mut self, hud: HudLevel) -> GamePower {
        self.hud = hud;
        self
    }

    /// After a profile pick, a slider move or a frame limit pick (what `perf_set` gets).
    pub fn with_perf(mut self, profile: Profile, tdp: Option<f64>, fps: Option<u32>) -> GamePower {
        let fps_only = fps.is_some() && tdp.is_none() && profile == self.profile;
        if !fps_only {
            self.profile = profile;
            if tdp.is_some() {
                self.tdp = tdp;
            } else if profile != Profile::Custom {
                // Picking a profile resets the hand-set boost (as for the launcher's own).
                self.cpu_boost = None;
            }
        }
        if let Some(f) = fps {
            self.fps = (f > 0).then_some(f);
        }
        self
    }
}

/// What the running games are, as far as power goes.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct Session {
    /// Running ids in the order they started; the last one is the game whose power applies.
    order: Vec<String>,
    /// Quick settings changes for the current game this run (it has nothing saved).
    over: Option<GamePower>,
}

impl Session {
    /// The game whose power applies now.
    pub fn game(&self) -> Option<&str> {
        self.order.last().map(String::as_str)
    }

    /// A game was started. True when the game whose power applies changed.
    pub fn started(&mut self, id: &str) -> bool {
        if self.game() == Some(id) {
            return false;
        }
        self.order.retain(|x| x != id);
        self.order.push(id.to_string());
        self.over = None;
        true
    }

    /// The running set as the runner sees it. Ids that ended drop out; ids started some other
    /// way (an app, a stream, a web game) count as started now. True when the game changed.
    pub fn running(&mut self, now: &[String]) -> bool {
        let before = self.game().map(str::to_string);
        self.order.retain(|x| now.contains(x));
        for id in now {
            if !self.order.contains(id) {
                self.order.push(id.clone());
            }
        }
        let changed = self.game().map(str::to_string) != before;
        if changed {
            self.over = None;
        }
        changed
    }

    /// A quick settings change for the current game. The game remembers it: it is saved for
    /// the game ("Use for {game}" turns on by itself). Returns the new settings, or None with no game.
    pub fn change(&mut self, saved: &mut BTreeMap<String, GamePower>, f: impl FnOnce(GamePower) -> GamePower) -> Option<GamePower> {
        let id = self.game()?.to_string();
        let next = f(self.game_power(saved).unwrap_or(GamePower::DEFAULT));
        saved.insert(id, next);
        self.over = None;
        Some(next)
    }

    /// The current game's settings: this run's changes, else what is saved, else the default.
    pub fn game_power(&self, saved: &BTreeMap<String, GamePower>) -> Option<GamePower> {
        let id = self.game()?;
        Some(self.over.or_else(|| saved.get(id).copied()).unwrap_or(GamePower::DEFAULT))
    }
}

/// The launcher's own settings (Balanced unless picked otherwise; no frame limit).
pub fn launcher_power(c: &impl PowerCfg) -> GamePower {
    GamePower { profile: c.perf_profile().unwrap_or(Profile::Balanced), tdp: c.perf_tdp(), cpu_boost: c.perf_cpu_boost(), fps: None, hud: c.perf_hud().unwrap_or_default(), upscale: None, refresh: None, framegen: None, filters: None }
}

/// What should be applied now: the current game's settings, or the launcher's with no game.
pub fn target(c: &impl PowerCfg, s: &Session) -> GamePower {
    s.game_power(c.game_power()).unwrap_or_else(|| launcher_power(c))
}

#[cfg(test)]
fn reset_session_for_tests() {
    *session() = Session::default();
}

fn session() -> std::sync::MutexGuard<'static, Session> {
    static S: std::sync::OnceLock<Mutex<Session>> = std::sync::OnceLock::new();
    S.get_or_init(|| Mutex::new(Session::default())).lock().unwrap_or_else(|e| e.into_inner())
}

/// The settings `system::keep` re-applies after a resume or a charger change.
pub fn wanted_now(c: &impl PowerCfg) -> Wanted {
    target(c, &session()).wanted()
}

fn apply(p: GamePower, why: String, then: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        match system::perf_apply(&p.wanted()) {
            Ok(()) => log::info!("power: {why}: {:?}", p),
            Err(e) => log::warn!("power: {why}: {e}"),
        }
        if std::env::var_os("DISPLAY").is_some() {
            if let Err(e) = system::set_fps_limit(p.fps.unwrap_or(0)) {
                log::warn!("power: {why}: frame limit: {e}");
            }
        }
        if std::env::var_os("DISPLAY").is_some() {
            crate::perfhud::apply(p.hud);
            display_apply(&p, &why);
        }
        then();
    });
}

/// Upscaling and refresh rate: a game's own go on when it has them; the ones from Settings (and
/// the screen's own refresh rate) come back after a game that had its own. A game without any
/// leaves gamescope alone, so a docked screen keeps its size.
fn display_apply(p: &GamePower, why: &str) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static CUSTOM_SCALE: AtomicBool = AtomicBool::new(false);
    static CUSTOM_HZ: AtomicBool = AtomicBool::new(false);
    let r = match p.upscale {
        Some(u) => {
            CUSTOM_SCALE.store(true, Ordering::Relaxed);
            Some(crate::upscale::apply(&u))
        }
        None if CUSTOM_SCALE.swap(false, Ordering::Relaxed) => Some(crate::upscale::apply(&crate::upscale::saved())),
        None => None,
    };
    if let Some(Err(e)) = r {
        log::warn!("power: {why}: upscaling: {e}");
    }
    let r = match p.refresh {
        Some(hz) => {
            CUSTOM_HZ.store(true, Ordering::Relaxed);
            Some(crate::upscale::set_refresh(hz))
        }
        None if CUSTOM_HZ.swap(false, Ordering::Relaxed) => Some(crate::upscale::set_refresh(0)),
        None => None,
    };
    if let Some(Err(e)) = r {
        log::warn!("power: {why}: refresh rate: {e}");
    }
}

/// Applies what should be on now again (a game's settings were changed on its page while it runs).
pub fn reapply<C: PowerCfg>(config: impl FnOnce() -> C) {
    let c = config();
    let s = session().clone();
    apply(target(&c, &s), "settings changed".into(), || {});
}

/// The game whose settings apply now (the last one started that still runs).
pub fn current_game() -> Option<String> {
    session().game().map(str::to_string)
}

/// Rare started this game: its frame generation and filter files are put in place at once (the
/// game reads them as it creates its first Vulkan instance), its power settings go on, then `then`
/// runs (the game-load boost window, which reads the game's profile and must come after it).
pub fn started<C: PowerCfg>(config: impl FnOnce() -> C, id: &str, then: impl FnOnce() + Send + 'static) {
    let changed = session().started(id);
    let c = config();
    fx_sync(&c);
    crate::gamefx::starting(id, c.game_power().get(id).and_then(|g| g.filters));
    if changed {
        apply(target(&c, &session()), format!("{id} started"), then);
    } else {
        then();
    }
}

/// Every game's frame generation and filters as gamefx needs them.
pub fn fx_games(c: &impl PowerCfg) -> BTreeMap<String, crate::gamefx::Game> {
    c.game_power()
        .iter()
        .filter(|(_, g)| g.framegen.is_some() || g.filters.is_some())
        .map(|(id, g)| (id.clone(), crate::gamefx::Game { framegen: g.framegen, filters: g.filters }))
        .collect()
}

/// Writes the Vulkan layer manifests and configs for every game's saved frame generation and
/// filters (after a change, and when a game starts).
pub fn fx_sync(c: &impl PowerCfg) {
    crate::gamefx::sync(&fx_games(c));
}

/// The environment a game Rare starts itself needs for its frame generation and filters.
pub fn fx_env(c: &impl PowerCfg, id: &str) -> Vec<(String, String)> {
    let g = c.game_power().get(id).map(|g| crate::gamefx::Game { framegen: g.framegen, filters: g.filters }).unwrap_or_default();
    crate::gamefx::launch_env(id, &g)
}

/// Frame generation for the game in front (Quick settings), kept for the game. The caller saves
/// the config and calls `fx_sync`: lsfg-vk's config changes and the running game reloads it.
/// None with no game.
pub fn framegen_set_config(c: &mut impl PowerCfg, f: crate::gamefx::FrameGen) -> Option<GamePower> {
    change(c, |g| GamePower { framegen: Some(f), ..g })
}

/// The runner's running set changed: a game ended (the next one's settings, or the launcher's
/// own when none is left) or one started some other way.
pub fn running<C: PowerCfg>(config: impl FnOnce() -> C, now: &[String]) {
    let changed = session().running(now);
    if changed {
        let c = config();
        let s = session().clone();
        let why = s.game().map(|g| format!("{g} in front")).unwrap_or_else(|| "back to the launcher".into());
        apply(target(&c, &s), why, || {});
    }
}

/// While a game runs, quick settings change that game's settings, and the game keeps them
/// (power.rs saves them for it). Returns the new settings, or None with no game (the caller
/// changes the launcher's own then).
pub fn change(c: &mut impl PowerCfg, f: impl FnOnce(GamePower) -> GamePower) -> Option<GamePower> {
    session().change(c.game_power_mut(), f)
}

/// "Apply to this game": on saves the current settings for the game, off forgets them (they
/// stay on for this run). Returns false with no game.
pub fn save_for_game(c: &mut impl PowerCfg, save: bool) -> bool {
    let mut s = session();
    let Some(id) = s.game().map(str::to_string) else { return false };
    let now = s.game_power(c.game_power()).unwrap_or(GamePower::DEFAULT);
    if save {
        c.game_power_mut().insert(id, now);
        s.over = None;
    } else {
        c.game_power_mut().remove(&id);
        s.over = Some(now);
    }
    true
}

/// The overlay level that applies now.
pub fn wanted_hud(c: &impl PowerCfg) -> HudLevel {
    target(c, &session()).hud
}

/// For quick settings: the game whose power applies and whether it has saved settings.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GameView {
    pub id: String,
    pub saved: bool,
}

pub fn view(c: &impl PowerCfg) -> Option<GameView> {
    session().game().map(|id| GameView { id: id.to_string(), saved: c.game_power().contains_key(id) })
}

// ---------- quick settings: the config changes and the device calls behind the commands ----------

/// A profile (its TDP, CPU boost and power preferences), or the slider's watts with "custom".
/// A call that only carries `fps` for the current profile leaves the power settings alone.
/// While a game runs this changes that game's settings, never the launcher's own. Changes the
/// config (the caller saves it) and returns what to apply: `perf_set_apply(tdp, wanted, fps)`.
pub fn perf_set_config(c: &mut impl PowerCfg, profile: Profile, tdp: Option<f64>, fps: Option<u32>) -> Option<Wanted> {
    let fps_only = fps.is_some() && tdp.is_none() && wanted_now(c).profile == profile;
    if change(c, |g| g.with_perf(profile, tdp, fps)).is_none() {
        c.set_perf_profile(Some(profile));
        if tdp.is_some() {
            c.set_perf_tdp(tdp);
        } else if !fps_only && profile != Profile::Custom {
            // Picking a profile resets the hand-set boost.
            c.set_perf_cpu_boost(None);
        }
    }
    (!fps_only).then(|| wanted_now(c))
}

/// Applies what `perf_set_config` returned. Blocking (D-Bus, xprop).
pub fn perf_set_apply(tdp: Option<f64>, power: Option<Wanted>, fps: Option<u32>) -> Result<(), String> {
    let mut errs = Vec::new();
    match (tdp, power) {
        (Some(w), _) => system::set_tdp(w).unwrap_or_else(|e| errs.push(e)),
        (None, Some(w)) => system::perf_apply(&w).unwrap_or_else(|e| errs.push(e)),
        _ => {}
    }
    if let Some(f) = fps {
        system::set_fps_limit(f).unwrap_or_else(|e| errs.push(e));
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs.join("; "))
    }
}

/// CPU boost by hand: the running game's, or the launcher's own. The caller saves the config,
/// then calls `system::set_cpu_boost(on)`.
pub fn perf_boost_config(c: &mut impl PowerCfg, on: bool) {
    if change(c, |g| GamePower { cpu_boost: Some(on), ..g }).is_none() {
        c.set_perf_cpu_boost(Some(on));
    }
}

/// The performance overlay level: the running game's (remembered), or the launcher's own. The
/// caller saves the config, then calls `perfhud::apply(level)`.
pub fn hud_set_config(c: &mut impl PowerCfg, level: HudLevel) {
    if change(c, |g| g.with_hud(level)).is_none() {
        c.set_perf_hud(Some(level));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn cfg() -> PowerSettings {
        PowerSettings { perf_profile: Some(Profile::Quiet), ..Default::default() }
    }

    #[test]
    fn unknown_game_is_balanced_without_boost_or_limit() {
        let mut s = Session::default();
        let c = cfg();
        assert_eq!(target(&c, &s), GamePower { profile: Profile::Quiet, tdp: None, cpu_boost: None, fps: None, hud: HudLevel::Off, upscale: None, refresh: None, framegen: None, filters: None });
        assert!(s.started("steam:1"));
        let t = target(&c, &s);
        assert_eq!(t, GamePower::DEFAULT);
        let (tdp, boost, _) = system::resolve(&t.wanted(), 5.0, 28.0);
        assert_eq!((tdp, boost), (Some(15.0), Some(false)));
    }

    #[test]
    fn saved_game_settings_apply_and_the_launcher_comes_back() {
        let mut s = Session::default();
        let mut c = cfg();
        let perf = GamePower { profile: Profile::Performance, tdp: None, cpu_boost: None, fps: Some(40), hud: HudLevel::Off, upscale: None, refresh: None, framegen: None, filters: None };
        c.game_power.insert("steam:2".into(), perf);
        assert!(s.started("steam:2"));
        assert_eq!(target(&c, &s), perf);
        // Seen again by the runner: nothing changes.
        assert!(!s.running(&ids(&["steam:2"])));
        assert!(s.running(&[]));
        assert_eq!(s.game(), None);
        assert_eq!(target(&c, &s).profile, Profile::Quiet);
        assert_eq!(target(&c, &s).fps, None);
    }

    #[test]
    fn last_started_wins_and_the_one_under_it_takes_over() {
        let mut s = Session::default();
        let mut c = cfg();
        c.game_power.insert("a".into(), GamePower { profile: Profile::Custom, tdp: Some(10.0), cpu_boost: Some(true), fps: None, hud: HudLevel::Off, upscale: None, refresh: None, framegen: None, filters: None });
        s.started("a");
        assert!(s.started("b"));
        assert_eq!(target(&c, &s), GamePower::DEFAULT);
        assert!(!s.started("b"));
        // b ends: a's settings again.
        assert!(s.running(&ids(&["a"])));
        assert_eq!(target(&c, &s).tdp, Some(10.0));
        // Something started another way (an app) counts as started.
        assert!(s.running(&ids(&["a", "app:netflix"])));
        assert_eq!(s.game(), Some("app:netflix"));
    }

    #[test]
    fn a_change_in_a_game_is_remembered_for_that_game() {
        let mut s = Session::default();
        let mut c = cfg();
        s.started("x");
        assert!(c.game_power.is_empty());
        // Picking a profile saves the game's settings ("Use for" turns on by itself).
        let p = s.change(&mut c.game_power, |g| g.with_perf(Profile::Performance, None, None)).unwrap();
        assert_eq!(p.profile, Profile::Performance);
        assert_eq!(c.game_power["x"], p);
        assert_eq!(c.perf_profile, Some(Profile::Quiet));
        // The overlay level rides along, and the other settings stay.
        s.change(&mut c.game_power, |g| g.with_hud(HudLevel::Basic));
        assert_eq!(c.game_power["x"], GamePower { hud: HudLevel::Basic, ..p });
        // Next run of the same game: the same settings.
        s.running(&[]);
        s.started("x");
        assert_eq!(target(&c, &s), GamePower { hud: HudLevel::Basic, ..p });
        // Another game is not touched.
        s.started("y");
        assert_eq!(target(&c, &s), GamePower::DEFAULT);
        // No game: nothing to change here.
        s.running(&[]);
        assert_eq!(s.change(&mut c.game_power, |g| g), None);
    }

    #[test]
    fn perf_changes() {
        let base = GamePower { profile: Profile::Balanced, tdp: Some(12.0), cpu_boost: Some(true), fps: Some(30), hud: HudLevel::Off, upscale: None, refresh: None, framegen: None, filters: None };
        // A frame limit pick for the same profile leaves the power alone.
        assert_eq!(base.with_perf(Profile::Balanced, None, Some(60)), GamePower { fps: Some(60), ..base });
        // 0 removes the limit.
        assert_eq!(base.with_perf(Profile::Balanced, None, Some(0)).fps, None);
        // A profile pick drops the hand-set boost, keeps the limit.
        assert_eq!(base.with_perf(Profile::Quiet, None, None), GamePower { profile: Profile::Quiet, cpu_boost: None, ..base });
        // The slider: Custom with its watts.
        assert_eq!(base.with_perf(Profile::Custom, Some(9.0), None), GamePower { profile: Profile::Custom, tdp: Some(9.0), ..base });
    }

    #[test]
    fn stored_as_camel_case_and_old_configs_load() {
        let mut c = PowerSettings::default();
        c.game_power.insert("steam:3".into(), GamePower { profile: Profile::Custom, tdp: Some(11.0), cpu_boost: Some(false), fps: Some(40), hud: HudLevel::Off, upscale: None, refresh: None, framegen: None, filters: None });
        let j = serde_json::to_value(&c).unwrap();
        assert_eq!(j["gamePower"]["steam:3"], serde_json::json!({ "profile": "custom", "tdp": 11.0, "cpuBoost": false, "fps": 40, "hud": "off" }));
        let back: PowerSettings = serde_json::from_value(j).unwrap();
        assert_eq!(back.game_power, c.game_power);
        // A config from before per-game power, and a hand-written entry with only a profile.
        let old: PowerSettings = serde_json::from_str(r#"{"perfProfile":"balanced","onboarded":true}"#).unwrap();
        assert!(old.game_power.is_empty());
        let one: PowerSettings = serde_json::from_str(r#"{"gamePower":{"web:act":{"profile":"quiet"}}}"#).unwrap();
        assert_eq!(one.game_power["web:act"], GamePower { profile: Profile::Quiet, tdp: None, cpu_boost: None, fps: None, hud: HudLevel::Off, upscale: None, refresh: None, framegen: None, filters: None });
    }

    #[test]
    fn quick_settings_change_the_launcher_or_the_game() {
        // No game: the launcher's own settings.
        reset_session_for_tests();
        let mut c = PowerSettings::default();
        let w = perf_set_config(&mut c, Profile::Performance, None, None).unwrap();
        assert_eq!((c.perf_profile, w.profile), (Some(Profile::Performance), Profile::Performance));
        // Picking a profile drops the hand-set boost; Custom keeps it.
        perf_boost_config(&mut c, true);
        assert_eq!(c.perf_cpu_boost, Some(true));
        perf_set_config(&mut c, Profile::Quiet, None, None);
        assert_eq!(c.perf_cpu_boost, None);
        // A frame-limit-only call for the current profile applies nothing to power.
        assert!(perf_set_config(&mut c, Profile::Quiet, None, Some(40)).is_none());
        hud_set_config(&mut c, HudLevel::Basic);
        assert_eq!(c.perf_hud, Some(HudLevel::Basic));
    }
}
