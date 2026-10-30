//! Glue between the launcher and the rare-system crate: the host hooks the crate calls back
//! into (launch environment, gamescope, front app, Bluetooth, web games), the event sink that
//! turns crate events into Tauri emits, and the Config impls for the crate's settings traits.

use crate::config::Config;
use rare_system::host::{self, Host};
use rare_system::network::NetworkCfg;
use rare_system::perfhud::HudLevel;
use rare_system::power::{GamePower, PowerCfg};
use rare_system::system::Profile;
use std::collections::BTreeMap;
use std::process::Command;

struct LauncherHost;

impl Host for LauncherHost {
    fn env(&self, cmd: &mut Command) {
        crate::launch::host_env(cmd);
    }
    fn log(&self, line: &str) {
        crate::gamepad::log(line);
    }
    fn gamescope_active(&self) -> bool {
        crate::overlay::gamescope::active()
    }
    fn gamescope_focused_app(&self) -> Option<u64> {
        crate::overlay::gamescope::focused_app()
    }
    fn gamescope_base_display(&self) -> String {
        crate::overlay::gamescope::base_display()
    }
    fn launcher_front(&self) -> bool {
        crate::apps::front::launcher_front() && !crate::webgame::visible()
    }
    fn bluetooth_state(&self) -> Option<bool> {
        crate::wallet::bluetooth::state_now().ok().filter(|b| b.present).map(|b| b.powered)
    }
    fn bluetooth_set(&self, on: bool) -> Result<(), String> {
        crate::wallet::bluetooth::set_powered(on).map(|_| ())
    }
    fn web_url_allowed(&self, u: &url::Url) -> bool {
        crate::webgame::url_allowed(u)
    }
}

/// Installs the hooks. First thing in `run`.
pub fn install_host() {
    host::set(LauncherHost);
}

impl PowerCfg for Config {
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

impl NetworkCfg for Config {
    fn network(&self) -> Option<&str> {
        self.network.as_deref()
    }
    fn testnet_api(&self) -> &Option<String> {
        &self.testnet_api
    }
    fn testnet_sponsor(&self) -> &Option<String> {
        &self.testnet_sponsor
    }
}
