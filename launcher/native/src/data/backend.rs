//! The native side the screens call: the library, launching, the device. Two implementations:
//! `Mock` is launcher/src/lib/mock.ts (the browser-mode fixtures the web screenshots use, so a
//! native shot and a web shot show the same games), `Device` reads the real backend's data and
//! grows into calls to the backend crates (rare-library, rare-launch, rare-system) as they land.
//! Calls that can block run on worker threads (app.rs `spawn`), never on the UI thread.

use super::model::{ProtonStatus, ProtonTool, UninstallInfo, Battery, Device, Game, Levels, Library, PcApp, PcShelf, Perf, Radios};
use crate::screens::apps::AppStatus;
use crate::state::Phase;
use crate::features::gamemgmt::art::{Choice as ArtChoice, Origin as ArtOrigin};
use crate::features::gamemgmt::GamePerf;
use std::path::PathBuf;
use std::sync::Mutex;

pub trait Backend: Send + Sync {
    /// The last scan from disk, at once (store.ts cachedLibrary).
    fn cached_library(&self) -> Option<Library>;
    /// A fresh scan (may take a while).
    fn scan(&self) -> Result<Library, String>;
    fn device(&self) -> Option<Device>;
    fn running(&self) -> Vec<String>;
    fn launch(&self, id: &str) -> Result<(), String>;
    /// A Sui game's web build without adding it (Play now): rare-launch's webgame_open.
    fn play_web(&self, _id: &str, _title: &str, _url: &str) -> Result<(), String> {
        Err(NOT_WIRED.into())
    }
    fn quit(&self, id: &str) -> Result<(), String>;
    fn launch_phase(&self, id: &str) -> Phase;
    /// Starts Steam quietly ahead of a Play press, unless Settings says to start it only on Play.
    fn steam_prewarm(&self) {}
    fn steam_big_picture(&self) -> Result<(), String>;
    fn steam_game_page(&self, _appid: u32) -> Result<(), String> {
        self.steam_big_picture()
    }
    /// A game's store page in Steam.
    fn steam_store(&self, _appid: u32) -> Result<(), String> {
        self.steam_big_picture()
    }
    /// Any steam:// link in Steam (a friend's lobby).
    fn steam_url(&self, _url: &str) -> Result<(), String> {
        self.steam_big_picture()
    }
    fn steam_add_library(&self, path: &str) -> Result<(), String>;
    fn perf(&self) -> Option<Perf>;
    fn set_perf(&self, profile: &str, tdp: Option<f32>, fps: Option<u32>) -> Result<(), String>;
    fn set_cpu_boost(&self, on: bool) -> Result<(), String>;
    fn set_fan(&self, mode: &str) -> Result<(), String>;
    fn levels(&self) -> Option<Levels>;
    fn set_levels(&self, brightness: Option<f32>, volume: Option<f32>) -> Result<(), String>;
    fn radios(&self) -> Option<Radios>;
    fn set_radio(&self, kind: &str, on: bool) -> Result<Radios, String>;
    fn hud(&self) -> String;
    fn set_hud(&self, level: &str) -> Result<(), String>;
    fn power(&self, action: &str) -> Result<(), String>;
    /// The running game whose settings Quick settings changes, and whether they are kept for it.
    fn game_power(&self) -> Option<(String, bool)>;
    fn save_game_power(&self, save: bool) -> Result<(), String>;
    /// Frame generation for the game in front (Quick settings) and what the device can do.
    fn game_fx(&self) -> Option<(rare_system::gamefx::FrameGen, rare_system::gamefx::Status)> {
        None
    }
    fn set_framegen(&self, _f: rare_system::gamefx::FrameGen) -> Result<(), String> {
        Err(NOT_WIRED.into())
    }
    /// The fan as rare-fand reports it (the curve editor); None without rare-fand.
    fn fan_status(&self) -> Option<rare_system::system::FanStatus> {
        None
    }
    fn set_fan_curve(&self, _points: &[(u32, u32)]) -> Result<(), String> {
        Err(NOT_WIRED.into())
    }
    /// Sound outputs and the mic (PipeWire). None on a fixture run: Quick settings shows no rows.
    fn audio(&self) -> Option<rare_system::audio::Audio> {
        None
    }
    /// Home's "On your PC" row: asks the paired PCs who answers (may take a couple of seconds).
    fn pc_shelf(&self) -> Option<PcShelf> {
        None
    }
    /// Streams a PC game (stream/state.ts play): resumes the stream when it runs.
    fn stream_play(&self, _uuid: &str, _app: u32) -> Result<(), String> {
        Err(NOT_WIRED.into())
    }
    /// Apps on this device (rare-launch statuses): the way each opens, what to install first.
    fn app_statuses(&self) -> Vec<AppStatus> {
        let owner = ["com.google.Chrome", "org.telegram.desktop"];
        crate::screens::apps::apps().iter().map(|a| crate::screens::apps::catalog::status_for(a, &owner, true)).collect()
    }
    /// Opens an app with way `way` (None: its default); resumes it when it runs.
    fn app_open(&self, _id: &str, _way: Option<usize>) -> Result<(), String> {
        Err(NOT_WIRED.into())
    }
    fn app_resume(&self, _id: &str) -> Result<(), String> {
        Err(NOT_WIRED.into())
    }
    /// Installs what the app's offer needs; progress arrives as `app-install` events.
    fn app_install(&self, _id: &str) -> Result<(), String> {
        Err(NOT_WIRED.into())
    }
    /// The calls behind a PC game's sheet (screens/stream), None when this backend has no streaming.
    fn stream_api(&self) -> Option<Box<dyn crate::screens::stream::state::StreamApi>> {
        None
    }
    /// The Chromium browser web games can open in (bridge.ts browser), if the device has one.
    fn browser_name(&self) -> Option<String> {
        None
    }
    /// Opens a web game in that browser; resolves to its name.
    fn webgame_open_browser(&self, _id: &str, _url: &str) -> Result<String, String> {
        Err(NOT_WIRED.into())
    }
    /// Proton builds, Steam's default and one game's choice (None: Steam is not installed).
    fn proton_status(&self, _appid: u64) -> Option<ProtonStatus> {
        None
    }
    /// Sets (None: clears) one game's Proton. Closes and restarts Steam around the edit; blocks.
    fn proton_set_game(&self, _appid: u64, _tool: Option<&str>) -> Result<(), String> {
        Err(NOT_WIRED.into())
    }
    /// Installs "ge" or "cachyos"; progress arrives as `proton-install` events. Blocks.
    fn proton_install(&self, _kind: &str) -> Result<String, String> {
        Err(NOT_WIRED.into())
    }
    fn proton_remove(&self, _name: &str) -> Result<(), String> {
        Err(NOT_WIRED.into())
    }
    /// Proton for a game Rare runs through umu-run (Epic, GOG): the same tools as Steam's, the
    /// default (Valve's newest) as `global`, the game's pick as `game` (rare-library umu).
    fn umu_proton_status(&self, _id: &str) -> Option<ProtonStatus> {
        None
    }
    /// Picks one for the game (None: back to the default). Takes effect on its next start.
    fn umu_proton_set(&self, _id: &str, _tool: Option<&str>) -> Result<(), String> {
        Err(NOT_WIRED.into())
    }
    /// The Rare menu's emulator rows for a running library game (rare-emu actions), empty when it
    /// is not an emulator game.
    fn emu_actions(&self, _id: &str) -> Vec<rare_emu::actions::EmuAction> {
        vec![]
    }
    /// Does one emulator row (save state, load state, the emulator's menu) for a running game.
    fn emu_do(&self, _action: &rare_emu::actions::EmuAction) -> Result<(), String> {
        Err(NOT_WIRED.into())
    }
    /// What uninstalling `id` (a library game or "rom:<system>/<path>") frees and asks. Blocks
    /// while it measures the folder. Err is the sentence the sheet shows when it can't be removed.
    fn uninstall_info(&self, _id: &str) -> Result<UninstallInfo, String> {
        Err(NOT_WIRED.into())
    }
    /// Removes it; `tick(done, total)` follows the bytes. Steam games wait for Steam (its own
    /// confirm) and give the screen back to Rare when Steam is done. Blocks.
    fn uninstall(&self, _id: &str, _keep_saves: bool, _tick: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
        Err(NOT_WIRED.into())
    }
    /// A Steam game's launch options as Steam has them (None: Steam is not installed).
    fn steam_launch_options(&self, _appid: u64) -> Option<String> {
        None
    }
    /// Sets a Steam game's launch options. Closes and restarts Steam around the edit; blocks.
    fn steam_set_launch_options(&self, _appid: u64, _opts: &str) -> Result<(), String> {
        Err(NOT_WIRED.into())
    }
    /// Steam Deck and ProtonDB ratings for a Steam game (may go to the network).
    fn compat(&self, _appid: u64) -> super::compat::Compat {
        Default::default()
    }
    // ---- game management (features/gamemgmt): Check files, a game's own performance
    // settings, adding a game, its name and art.
    /// A game's performance settings and what they fall back to.
    fn game_perf(&self, _id: &str) -> GamePerf {
        GamePerf { saved: None, global_upscale: Default::default(), watts: [10.0, 15.0, 25.0], fx: crate::features::gamemgmt::fixture_fx() }
    }
    /// Saves (None: forgets) a game's performance settings; a game in front gets them at once.
    fn set_game_perf(&self, _id: &str, _p: Option<rare_system::power::GamePower>) -> Result<(), String> {
        Err(NOT_WIRED.into())
    }
    /// Starts Check files (status: `gamemgmt::verify::status`). `keep` saves a game's files as
    /// they are now as the good copy (games outside the stores).
    fn verify_start(&self, _g: &Game, _keep: bool) -> Result<(), String> {
        Err(NOT_WIRED.into())
    }
    /// Adds a game from a picked .exe, AppImage, .sh or folder, with art found by its name.
    /// Returns its library id and name.
    fn add_game(&self, _path: &std::path::Path) -> Result<(String, String), String> {
        Err(NOT_WIRED.into())
    }
    fn game_custom(&self, _id: &str) -> rare_library::custom::Custom {
        Default::default()
    }
    /// Saves a game's name and art changes (blank: the original).
    fn set_game_custom(&self, _id: &str, _c: rare_library::custom::Custom) -> Result<(), String> {
        Err(NOT_WIRED.into())
    }
    fn art_choices(&self, title: &str, appid: Option<u32>, kind: rare_library::custom::ArtKind) -> Result<(ArtOrigin, Vec<ArtChoice>), String> {
        crate::features::gamemgmt::art::choices(title, appid, kind)
    }
    /// Steam's download queue (features/downloads.rs); None when it couldn't be read.
    fn steam_downloads(&self) -> Option<rare_launch::steamdl::Snapshot> {
        crate::features::downloads::mock::on().then(crate::features::downloads::mock::snapshot)
    }
    /// The library folders a Steam game can go to, with free space.
    fn steam_folders(&self) -> Vec<rare_launch::steamdl::Folder> {
        if crate::features::downloads::mock::on() {
            crate::features::downloads::mock::folders()
        } else {
            Vec::new()
        }
    }
    /// Installs a Steam game into a library folder through Steam. Blocks.
    fn steam_install(&self, appid: u32, _folder: &str) -> Result<rare_launch::steamdl::Started, String> {
        if crate::features::downloads::mock::on() {
            return Ok(crate::features::downloads::mock::install(appid));
        }
        Err(NOT_WIRED.into())
    }
    /// One of Steam's Downloads buttons; Ok(false) when Steam's own page was shown instead. Blocks.
    fn steam_control(&self, appid: u32, c: rare_launch::steamdl::Control) -> Result<bool, String> {
        if crate::features::downloads::mock::on() {
            crate::features::downloads::mock::control(appid, c);
            return Ok(true);
        }
        Err(NOT_WIRED.into())
    }
}

const DAY: i64 = 86400;

fn cdn(id: u64) -> String {
    format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{id}")
}

fn steam(appid: u64, title: &str, size_gb: f64, played_days_ago: Option<f64>, origin: &str, foreign: bool, now: i64) -> Game {
    Game {
        playtime_minutes: played_days_ago.map(|_| (600 + appid % 9000) as i64),
        id: format!("steam:{appid}"),
        source: "steam".into(),
        title: title.into(),
        installed: true,
        install_dir: Some(format!("/home/rare/.local/share/Steam/steamapps/common/{title}")),
        size_bytes: Some(size_gb * 1e9),
        last_played: played_days_ago.map(|d| now - (d * DAY as f64) as i64),
        art: super::model::Art {
            cover: vec![format!("{}/library_600x900_2x.jpg", cdn(appid))],
            hero: vec![format!("{}/library_hero.jpg", cdn(appid))],
            logo: vec![format!("{}/logo.png", cdn(appid))],
            header: vec![format!("{}/header.jpg", cdn(appid))],
            icon: vec![],
        },
        launch: None,
        origin: origin.into(),
        foreign,
    }
}

fn other(source: &str, id: &str, title: &str, origin: &str) -> Game {
    Game {
        id: format!("{source}:{id}"),
        source: source.into(),
        title: title.into(),
        installed: true,
        install_dir: None,
        size_bytes: None,
        last_played: None,
        playtime_minutes: None,
        art: Default::default(),
        launch: None,
        origin: origin.into(),
        foreign: false,
    }
}

/// Android apps for shots (RARE_ANDROID_FAKE=ready or many).
fn mock_android() -> Vec<Game> {
    let mode = rare_android::fake().unwrap_or_default();
    if mode != "ready" && mode != "many" {
        return Vec::new();
    }
    let mut v = vec![
        other("android", "org.fdroid.fdroid", "F-Droid", "Android app store"),
        other("android", "com.aurora.store", "Aurora Store", "Android app store"),
        other("android", "org.secuso.privacyfriendly2048", "2048", "Android"),
        other("android", "com.shatteredpixel.shatteredpixeldungeon", "Shattered Pixel Dungeon", "Android"),
    ];
    if mode == "many" {
        v.push(other("android", "org.example.long", "The Incredibly Long Named Android Puzzle Adventure Deluxe Edition", "Android"));
        for i in 0..28 {
            v.push(other("android", &format!("org.example.g{i}"), &format!("Android game {i}"), "Android"));
        }
    }
    for g in v.iter_mut() {
        g.size_bytes = Some(48_000_000.0);
    }
    v
}

/// mock.ts `library().emulation`.
fn mock_emulation(now: i64) -> super::model::Emulation {
    use super::model::{Emulation, RomGame, RomSystem};
    let sys = |id: &str, name: &str, count| RomSystem { id: id.into(), name: name.into(), count };
    Emulation {
        systems: vec![sys("switch", "Switch", 2), sys("xbox", "Xbox", 1), sys("gc", "GameCube", 12), sys("ps2", "PlayStation 2", 10), sys("wii", "Wii", 1)],
        recent: vec![
            RomGame { name: "Pokemon Violet".into(), system: "switch".into(), last_played: Some(now - 2 * DAY), playtime: 5400, path: Some("./Pokemon Violet.nsp".into()) },
            RomGame { name: "Jet Set Radio Future".into(), system: "xbox".into(), last_played: Some(now - 3 * DAY), playtime: 6540, path: Some("./Jet-Set-Radio.xiso.iso".into()) },
        ],
    }
}

/// mock.ts `library()`: the same games, the same art, the same last-played times.
pub fn mock_library(now: i64, nogames: bool) -> Library {
    let old = "Steam library on /var/home/playtron/.local/share/Steam";
    let mut games = if nogames {
        vec![]
    } else {
        vec![
            steam(1145350, "Hades II", 9.8, Some(0.2), "Steam", false, now),
            steam(2379780, "Balatro", 0.2, Some(1.0), "Steam", false, now),
            steam(1245620, "ELDEN RING", 48.0, Some(3.0), "Steam", false, now),
            steam(1868140, "DAVE THE DIVER", 9.0, Some(6.0), "Steam", false, now),
            steam(1794680, "Vampire Survivors", 0.6, Some(9.0), "Steam", false, now),
            steam(504230, "Celeste", 1.2, Some(14.0), "Steam", false, now),
            steam(367520, "Hollow Knight", 9.0, Some(30.0), "Steam", false, now),
            steam(413150, "Stardew Valley", 0.6, None, "Steam", false, now),
            steam(1086940, "Baldur's Gate 3", 122.0, None, "Steam", false, now),
            steam(1091500, "Cyberpunk 2077", 70.0, None, "Steam", false, now),
            steam(578080, "PUBG: BATTLEGROUNDS", 40.0, None, "Steam", false, now),
            steam(1244090, "Sea of Stars", 5.0, None, old, true, now),
            steam(1145360, "Hades", 15.0, None, old, true, now),
            other("epic", "Fortnite", "Alan Wake 2", "Epic Games"),
            other("gog", "1207658924", "The Witcher 3", "GOG"),
            other("itch", "a1", "A Short Hike", "itch.io"),
            other("local", "b2", "SuperTuxKart", "Games folder"),
            other("apps", "esde", "ES-DE", "EmuDeck"),
            other("apps", "retroarch", "RetroArch", "Desktop app"),
            other("apps", "dolphin", "Dolphin Emulator", "Desktop app"),
            other("apps", "ppsspp", "PPSSPP", "Desktop app"),
            other("apps", "xemu", "xemu", "Steam shortcut"),
            other("apps", "chrome", "Google Chrome", "Steam shortcut"),
        ]
    };
    games.extend(mock_android());
    // RR_MOCK_TITLES="A|B": extra art-less games in the Games folder (text cover shots).
    if !nogames {
        for (i, t) in std::env::var("RR_MOCK_TITLES").unwrap_or_default().split('|').filter(|t| !t.is_empty()).enumerate() {
            games.push(other("local", &format!("t{i}"), t, "Games folder"));
        }
    }
    for (id, title, url) in [("act", "ACT", "https://play.actuniverse.com/")] {
        games.push(Game {
            id: format!("web:{id}"),
            source: "web".into(),
            title: title.into(),
            installed: true,
            install_dir: None,
            size_bytes: None,
            last_played: if id == "act" { Some(now - 3600) } else { None },
            playtime_minutes: None,
            art: super::model::Art { cover: vec![format!("games/{id}/cover.jpg")], hero: vec![format!("games/{id}/hero.jpg")], logo: vec![format!("games/{id}/logo.svg")], header: vec![], icon: vec![] },
            launch: Some(super::model::LaunchSpec { kind: "web".into(), url: Some(url.into()), appid: None, ..Default::default() }),
            origin: "Web game".into(),
            foreign: false,
        });
    }
    if !nogames {
        games.extend(crate::features::emulation::mock_games());
    }
    let owned = if crate::features::downloads::mock::on() { crate::features::downloads::mock::owned(now) } else { vec![] };
    Library { games, sources: vec![], foreign_steam_libraries: vec!["/var/home/playtron/.local/share/Steam".into()], owned, scanned_at: now, emulation_app: Some("apps:esde".into()), emulation: Some(mock_emulation(now)), cloud: vec![], keys: Default::default() }
}

struct MockState {
    running: Vec<String>,
    started: std::collections::HashMap<String, std::time::Instant>,
    perf: Perf,
    levels: Levels,
    radios: Radios,
    hud: String,
    saved: bool,
    fg: rare_system::gamefx::FrameGen,
    fan: rare_system::system::FanStatus,
}

/// The fixtures' game in front: the one started, else the first of RR_RUNNING (shots).
fn mock_front(st: &MockState) -> Option<String> {
    st.running.first().cloned().or_else(|| std::env::var("RR_RUNNING").ok()?.split(',').find(|x| !x.is_empty()).map(String::from))
}

/// lib/mock.ts.
pub struct Mock {
    st: Mutex<MockState>,
    nogames: bool,
    gm: crate::features::gamemgmt::MockStore,
}

impl Mock {
    pub fn new(nogames: bool) -> Mock {
        Mock {
            st: Mutex::new(MockState {
                running: vec![],
                started: Default::default(),
                perf: Perf { profile: "balanced".into(), tdp: Some(15.0), tdp_min: 5.0, tdp_max: 28.0, fps_limit: None, cpu_boost: Some(false), fan: Some("default".into()), watts: [10.0, 15.0, 25.0], note: String::new() },
                levels: Levels { brightness: Some(70.0), volume: Some(55.0) },
                radios: Radios { wifi: Some(true), bluetooth: Some(true), airplane: false },
                hud: "off".into(),
                saved: false,
                fg: Default::default(),
                fan: rare_system::system::FanStatus { preset: "default".into(), curve: vec![], mode: "ec".into(), temp: Some(54.0), sensor: "edge".into(), duty: Some(41), rpm: Some(2898) },
            }),
            nogames,
            gm: Default::default(),
        }
    }
}

/// mock.ts's paired PC: Gaming-PC with its games (the web screenshots' "On your PC" row).
pub fn mock_pc() -> PcShelf {
    let cover = |id: u64| Some(format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{id}/library_600x900.jpg"));
    let apps = [
        (1091500, "Cyberpunk 2077", cover(1091500)),
        (1245620, "ELDEN RING", cover(1245620)),
        (1086940, "Baldur's Gate 3", cover(1086940)),
        (1551360, "Forza Horizon 5", cover(1551360)),
        (1174180, "Red Dead Redemption 2", cover(1174180)),
        (2358720, "Black Myth: Wukong", cover(2358720)),
        (881448767, "Desktop", None),
        (1093255277, "Steam Big Picture", None),
    ];
    PcShelf {
        uuid: crate::screens::stream::fixtures::PC.into(),
        name: "Gaming-PC".into(),
        online: true,
        can_wake: true,
        current_game: None,
        apps: apps.into_iter().map(|(id, name, art)| PcApp { id: id as u32, name: name.into(), art }).collect(),
    }
}

impl Backend for Mock {
    fn game_perf(&self, id: &str) -> GamePerf {
        self.gm.perf(id)
    }
    fn set_game_perf(&self, id: &str, p: Option<rare_system::power::GamePower>) -> Result<(), String> {
        self.gm.set_perf(id, p);
        Ok(())
    }
    fn verify_start(&self, g: &Game, _keep: bool) -> Result<(), String> {
        crate::features::gamemgmt::verify::mock_start(&g.id, g.source == "steam");
        Ok(())
    }
    fn add_game(&self, path: &std::path::Path) -> Result<(String, String), String> {
        rare_library::custom::launch_for(path)?;
        Ok(("local:b2".into(), rare_library::custom::title_from(path)))
    }
    fn game_custom(&self, id: &str) -> rare_library::custom::Custom {
        self.gm.custom(id)
    }
    fn set_game_custom(&self, id: &str, c: rare_library::custom::Custom) -> Result<(), String> {
        self.gm.set_custom(id, c);
        Ok(())
    }
    fn browser_name(&self) -> Option<String> {
        Some("Google Chrome".into())
    }
    fn webgame_open_browser(&self, _id: &str, _url: &str) -> Result<String, String> {
        Ok("Google Chrome".into())
    }
    fn proton_status(&self, appid: u64) -> Option<ProtonStatus> {
        let tool = |name: &str, label: &str, kind: &str, stale| ProtonTool { name: name.into(), label: label.into(), kind: kind.into(), stale };
        Some(ProtonStatus {
            tools: vec![tool("proton_9", "Proton 9.0-4", "valve", false), tool("proton_experimental", "Proton Experimental", "experimental", false), tool("GE-Proton9-27", "GE-Proton9-27", "ge", false)],
            global: Some("proton_9".into()),
            game: (appid % 2 == 0).then(|| "GE-Proton9-27".into()),
            installable: vec!["cachyos".into()],
        })
    }
    fn proton_set_game(&self, _appid: u64, _tool: Option<&str>) -> Result<(), String> {
        Ok(())
    }
    fn proton_install(&self, _kind: &str) -> Result<String, String> {
        Ok("mock".into())
    }
    fn proton_remove(&self, _name: &str) -> Result<(), String> {
        Ok(())
    }
    fn umu_proton_status(&self, id: &str) -> Option<ProtonStatus> {
        let mut st = self.proton_status(0)?;
        st.game = id.starts_with("gog:").then(|| "GE-Proton9-27".into());
        Some(st)
    }
    fn umu_proton_set(&self, _id: &str, _tool: Option<&str>) -> Result<(), String> {
        Ok(())
    }
    fn uninstall_info(&self, id: &str) -> Result<UninstallInfo, String> {
        std::thread::sleep(std::time::Duration::from_millis(150));
        let g = mock_library(super::model::now_secs(), false).games.into_iter().find(|g| g.id == id);
        let size = g.as_ref().and_then(|g| g.size_bytes).unwrap_or(3.4e9) as u64;
        let free = Some(41_300_000_000);
        Ok(match id.split(':').next().unwrap_or("") {
            "steam" => UninstallInfo { steam: true, size, saves: None, place: g.and_then(|g| g.install_dir), free },
            "web" => UninstallInfo { steam: false, size: 0, saves: Some((2_400_000, "web")), place: None, free },
            "app" => UninstallInfo { steam: false, size: 180_000_000, saves: Some((40_000_000, "app")), place: Some("Installed for you".into()), free },
            _ => UninstallInfo { steam: false, size, saves: Some((1_200_000_000, "prefix")), place: Some("~/Games/Hades".into()), free },
        })
    }
    fn uninstall(&self, id: &str, _keep_saves: bool, tick: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
        let total = 3_400_000_000u64;
        let steps = if id.starts_with("steam:") { 6 } else { 20 };
        for i in 1..=steps {
            std::thread::sleep(std::time::Duration::from_millis(100));
            tick(total * i / steps, total);
        }
        Ok(())
    }
    fn steam_launch_options(&self, appid: u64) -> Option<String> {
        Some(if appid % 2 == 0 { String::new() } else { "DXVK_HUD=fps %command%".into() })
    }
    fn steam_set_launch_options(&self, _appid: u64, _opts: &str) -> Result<(), String> {
        Ok(())
    }
    fn compat(&self, appid: u64) -> super::compat::Compat {
        super::compat::Compat { deck: Some(if appid % 3 == 0 { 2 } else { 3 }), protondb: Some(if appid % 2 == 0 { "gold" } else { "platinum" }.into()) }
    }
    fn pc_shelf(&self) -> Option<PcShelf> {
        Some(mock_pc())
    }
    fn stream_play(&self, _uuid: &str, _app: u32) -> Result<(), String> {
        Ok(())
    }
    fn app_open(&self, id: &str, _way: Option<usize>) -> Result<(), String> {
        let rid = crate::screens::apps::run_id(id);
        let mut st = self.st.lock().unwrap();
        if !st.running.contains(&rid) {
            st.running.push(rid);
        }
        Ok(())
    }
    fn app_resume(&self, _id: &str) -> Result<(), String> {
        Ok(())
    }
    fn app_install(&self, _id: &str) -> Result<(), String> {
        Ok(())
    }
    fn stream_api(&self) -> Option<Box<dyn crate::screens::stream::state::StreamApi>> {
        Some(Box::new(crate::screens::stream::fixtures::MockApi::new(crate::screens::stream::fixtures::Mode::Normal)))
    }
    fn cached_library(&self) -> Option<Library> {
        None
    }
    fn scan(&self) -> Result<Library, String> {
        std::thread::sleep(std::time::Duration::from_millis(250));
        // RR_LIBRARY: a device's real library over the mocks (UX journeys, shots).
        if let Some(l) = std::env::var_os("RR_LIBRARY").and_then(|p| DeviceBackend::read_file(std::path::Path::new(&p))) {
            return Ok(l);
        }
        Ok(mock_library(super::model::now_secs(), self.nogames))
    }
    fn device(&self) -> Option<Device> {
        Some(Device { name: "SuiPlay0X1".into(), battery: Some(Battery { percent: 82, charging: false, minutes: Some(190) }), wifi: Some("Home".into()), ssh: false })
    }
    fn running(&self) -> Vec<String> {
        self.st.lock().unwrap().running.clone()
    }
    fn launch(&self, id: &str) -> Result<(), String> {
        if id.starts_with("web:") {
            return Ok(());
        }
        let mut st = self.st.lock().unwrap();
        st.running = vec![id.to_string()];
        st.started.insert(id.to_string(), std::time::Instant::now());
        Ok(())
    }
    fn quit(&self, id: &str) -> Result<(), String> {
        self.st.lock().unwrap().running.retain(|r| r != id);
        Ok(())
    }
    fn launch_phase(&self, id: &str) -> Phase {
        // Steam games start Steam first (a cold start is a few seconds), then the game, then play.
        let st = self.st.lock().unwrap();
        let Some(t) = st.started.get(id) else { return Phase::Playing };
        let s = t.elapsed().as_secs_f32();
        if id.starts_with("steam:") && s < 2.0 {
            Phase::Steam
        } else if s < 4.0 {
            Phase::Game
        } else {
            Phase::Playing
        }
    }
    fn steam_big_picture(&self) -> Result<(), String> {
        Ok(())
    }
    fn steam_add_library(&self, _path: &str) -> Result<(), String> {
        Ok(())
    }
    fn perf(&self) -> Option<Perf> {
        Some(self.st.lock().unwrap().perf.clone())
    }
    fn set_perf(&self, profile: &str, tdp: Option<f32>, fps: Option<u32>) -> Result<(), String> {
        let mut st = self.st.lock().unwrap();
        let preset = match profile {
            "quiet" => Some((10.0, false)),
            "balanced" => Some((15.0, false)),
            "performance" => Some((25.0, true)),
            _ => None,
        };
        st.perf.profile = profile.into();
        st.perf.tdp = tdp.or(preset.map(|p| p.0)).or(st.perf.tdp);
        if let Some(p) = preset {
            st.perf.cpu_boost = Some(p.1);
        }
        if let Some(f) = fps {
            st.perf.fps_limit = (f > 0).then_some(f);
        }
        Ok(())
    }
    fn set_cpu_boost(&self, on: bool) -> Result<(), String> {
        self.st.lock().unwrap().perf.cpu_boost = Some(on);
        Ok(())
    }
    fn set_fan(&self, mode: &str) -> Result<(), String> {
        let mut st = self.st.lock().unwrap();
        st.perf.fan = Some(mode.into());
        st.fan.preset = mode.into();
        st.fan.curve = if mode == "default" { vec![] } else { rare_system::system::fan_preset_curve(mode) };
        Ok(())
    }
    fn fan_status(&self) -> Option<rare_system::system::FanStatus> {
        Some(self.st.lock().unwrap().fan.clone())
    }
    fn set_fan_curve(&self, points: &[(u32, u32)]) -> Result<(), String> {
        let mut st = self.st.lock().unwrap();
        st.perf.fan = Some("custom".into());
        st.fan.preset = "custom".into();
        st.fan.curve = points.to_vec();
        Ok(())
    }
    fn game_fx(&self) -> Option<(rare_system::gamefx::FrameGen, rare_system::gamefx::Status)> {
        let st = self.st.lock().unwrap();
        mock_front(&st).map(|_| (st.fg, crate::features::gamemgmt::fixture_fx()))
    }
    fn set_framegen(&self, f: rare_system::gamefx::FrameGen) -> Result<(), String> {
        self.st.lock().unwrap().fg = f;
        Ok(())
    }
    fn levels(&self) -> Option<Levels> {
        Some(self.st.lock().unwrap().levels)
    }
    fn set_levels(&self, b: Option<f32>, v: Option<f32>) -> Result<(), String> {
        let mut st = self.st.lock().unwrap();
        if b.is_some() {
            st.levels.brightness = b;
        }
        if v.is_some() {
            st.levels.volume = v;
        }
        Ok(())
    }
    fn radios(&self) -> Option<Radios> {
        Some(self.st.lock().unwrap().radios)
    }
    fn set_radio(&self, kind: &str, on: bool) -> Result<Radios, String> {
        let mut st = self.st.lock().unwrap();
        match kind {
            "airplane" => {
                st.radios.airplane = on;
                if on {
                    st.radios.wifi = Some(false);
                    st.radios.bluetooth = Some(false);
                } else {
                    st.radios.wifi = Some(true);
                }
            }
            "wifi" => st.radios.wifi = Some(on),
            "bluetooth" => st.radios.bluetooth = Some(on),
            _ => {}
        }
        Ok(st.radios)
    }
    fn hud(&self) -> String {
        self.st.lock().unwrap().hud.clone()
    }
    fn set_hud(&self, level: &str) -> Result<(), String> {
        self.st.lock().unwrap().hud = level.into();
        Ok(())
    }
    fn power(&self, _action: &str) -> Result<(), String> {
        Ok(())
    }
    fn game_power(&self) -> Option<(String, bool)> {
        let st = self.st.lock().unwrap();
        mock_front(&st).map(|id| (id, st.saved))
    }
    fn save_game_power(&self, save: bool) -> Result<(), String> {
        self.st.lock().unwrap().saved = save;
        Ok(())
    }
}

/// The device: the library (rare-library's cache, the same file the web launcher writes),
/// launching and tracking runs (rare-launch's Runner), the device controls (rare-system).
pub struct DeviceBackend {
    /// RR_LIBRARY: a library file to read instead of scanning (tests, captures).
    cache: Option<PathBuf>,
    /// The last scan in rare-library's model (launching reads it).
    last: Mutex<Option<rare_library::model::Library>>,
    runner: rare_launch::launch::Runner,
    ctx: rare_core::Ctx,
    /// The plan the uninstall sheet showed, by id, so Uninstall removes what it said.
    plans: Mutex<std::collections::HashMap<String, rare_library::uninstall::Plan>>,
}

impl DeviceBackend {
    pub fn new() -> DeviceBackend {
        let cache = std::env::var("RR_LIBRARY").ok().map(PathBuf::from);
        crate::features::webgame::install();
        rare_launch::launch::set_launch_extra(|id| {
            // The game's own arguments and environment, after Rare's frame generation and filter
            // environment (gamefx: the key its Vulkan layer manifests wait for).
            // SDL leaves InputPlumber's grabbed source pads out, so games and emulators see one pad.
            let mut fx = rare_emu::controls::launch_env();
            fx.extend(rare_system::power::fx_env(&power_load(), id));
            let own = crate::data::gameset::launch_extra(id);
            if fx.is_empty() {
                return own;
            }
            let (args, env) = own.unwrap_or_default();
            Some((args, fx.into_iter().chain(env).collect()))
        });
        DeviceBackend { cache, last: Mutex::new(None), runner: runner().clone(), ctx: crate::features::bus::ctx(), plans: Default::default() }
    }

    /// What uninstalling `id` removes: a library game, or a ROM ("rom:<system>/<path>").
    fn uninstall_plan(&self, id: &str) -> Result<rare_library::uninstall::Plan, String> {
        if let Some(rest) = id.strip_prefix("rom:") {
            let (system, rel) = rest.split_once('/').ok_or("That isn't a ROM.")?;
            let e = rare_library::esde::scan().filter(|e| !e.rom_dir.is_empty()).ok_or("ES-DE's ROM folder isn't set up.")?;
            let name = e.recent.iter().find(|r| r.system == system && r.path.as_deref().map(|p| p.trim_start_matches("./")) == Some(rel)).map(|r| r.name.clone()).unwrap_or_else(|| rel.to_string());
            return rare_library::uninstall::plan_rom(std::path::Path::new(&e.rom_dir), system, rel, &name);
        }
        // A ROM in the Emulation folders: its file, or every disc of it.
        if id.starts_with("emu:") {
            let rom = rare_emu::roms::find(id).ok_or("That game isn't in its ROM folder any more.")?;
            let (root, folder, rel) = rare_emu::roms::locate(id).ok_or("That isn't a ROM.")?;
            let mut p = rare_library::uninstall::plan_rom(&root.roms(), &folder, &rel, &rom.title)?;
            p.id = id.to_string();
            let sys_dir = std::fs::canonicalize(root.roms().join(&folder)).map_err(|e| e.to_string())?;
            for f in rare_emu::roms::files_of(&rom).into_iter().skip(1) {
                if let Ok(c) = std::fs::canonicalize(&f) {
                    if c.starts_with(&sys_dir) && !p.files.contains(&c) {
                        p.size += c.metadata().map(|m| m.len()).unwrap_or(0);
                        p.files.push(c);
                    }
                }
            }
            return Ok(p);
        }
        let g = self.lib_game(id).ok_or("It isn't in your library anymore.")?;
        let folders: Vec<String> = crate::features::config::get("gameFolders").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        rare_library::uninstall::plan(&g, &folders)
    }

    /// rare-library's model of a game in the cache.
    fn lib_game(&self, id: &str) -> Option<rare_library::model::Game> {
        let find = |l: &rare_library::model::Library| l.games.iter().find(|g| g.id == id).cloned();
        if let Some(g) = self.last.lock().unwrap().as_ref().and_then(find) {
            return Some(g);
        }
        rare_library::cache::load().as_ref().and_then(find).or_else(|| rare_store_games().into_iter().find(|g| g.id == id))
    }

    pub(crate) fn read_file(path: &std::path::Path) -> Option<Library> {
        #[derive(serde::Deserialize)]
        struct Stored {
            library: Library,
        }
        let b = std::fs::read(path).ok()?;
        serde_json::from_slice::<Stored>(&b).map(|s| s.library).or_else(|_| serde_json::from_slice::<Library>(&b)).ok()
    }
}

/// Settings > Storage > Start Steam (`steamStart` in launcher.json): "boot" (the default here),
/// "focus" or "demand".
pub fn steam_mode() -> String {
    match crate::features::config::get_str("steamStart").as_deref() {
        Some(m @ ("boot" | "focus" | "demand")) => m.to_string(),
        _ => "boot".into(),
    }
}

/// Session start on the device: Steam starts quietly a few seconds after the launcher ("boot"), so
/// it is up before the first Play press and a Play press never cold-starts it onto its home screen.
/// Not before a Steam account has signed in: `-silent` still opens Steam's sign-in window, which
/// covered first run on a new device (first-install VM walk); first run's Steam step opens it.
pub fn steam_at_start() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(6));
        if steam_mode() == "boot" && crate::features::firstrun::steam_account().is_some() {
            if let Err(e) = rare_launch::launch::steam_prewarm() {
                log::warn!("steam prewarm: {e}");
            }
        }
    });
}

/// The process's one runner: the app is rebuilt when the UI size changes and what runs stays
/// tracked; the overlay (features/ingame.rs) reads the same one.
pub fn runner() -> &'static rare_launch::launch::Runner {
    static R: std::sync::OnceLock<rare_launch::launch::Runner> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        let r = rare_launch::launch::Runner::default();
        // Tracks what runs (windows, Steam's reaper, child processes) and when a start fails.
        r.watch(crate::features::bus::ctx());
        r
    })
}

impl Default for DeviceBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// The launcher config's power fields (rare-system PowerSettings), read from and merged back into
/// ~/.config/rare/launcher.json so the web launcher sees the same choices.
pub(crate) fn power_load() -> rare_system::power::PowerSettings {
    std::fs::read(rare_core::paths::config_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub(crate) fn power_save(s: &rare_system::power::PowerSettings) {
    let path = rare_core::paths::config_file();
    let mut v: serde_json::Value = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| serde_json::json!({}));
    if let (Some(obj), Ok(serde_json::Value::Object(p))) = (v.as_object_mut(), serde_json::to_value(s)) {
        for (k, val) in p {
            obj.insert(k, val);
        }
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_vec_pretty(&v).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
    // Frame generation and filters: their Vulkan layer manifests and configs follow every change.
    rare_system::power::fx_sync(s);
}

/// An enum from its lowercase name (serde).
fn named<T: serde::de::DeserializeOwned>(s: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).ok()
}

/// What the Rare menu's Performance line says for a game that just started, before the first
/// reading: the settings saved for it, else Balanced (rare-system power `target`), with watts from
/// the last reading. Also whether they are saved for it.
pub(crate) fn planned_perf(rid: &str, last: Option<&Perf>) -> (Option<Perf>, Option<(String, bool)>) {
    use rare_system::power::{GamePower, PowerCfg};
    let c = power_load();
    let saved = c.game_power().get(rid).copied();
    let p = saved.unwrap_or(GamePower::DEFAULT);
    let i = match p.profile {
        rare_core::Profile::Quiet => Some(0),
        rare_core::Profile::Balanced => Some(1),
        rare_core::Profile::Performance => Some(2),
        rare_core::Profile::Custom => None,
    };
    let watts = last.map(|l| l.watts).unwrap_or([0.0; 3]);
    let tdp = p.tdp.map(|t| t as f32).or_else(|| i.map(|i| watts[i])).filter(|w| *w > 0.0);
    let perf = Perf {
        profile: name_of(&p.profile),
        tdp,
        tdp_min: last.map(|l| l.tdp_min).unwrap_or(0.0),
        tdp_max: last.map(|l| l.tdp_max).unwrap_or(0.0),
        fps_limit: p.fps.filter(|f| *f > 0),
        cpu_boost: p.cpu_boost,
        fan: last.and_then(|l| l.fan.clone()),
        watts,
        note: String::new(),
    };
    (Some(perf), Some((rid.to_string(), saved.is_some())))
}

fn name_of<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

pub const NOT_WIRED: &str = "not wired to the backend yet";

/// rare-library's model as the screens' (the same JSON the web launcher reads).
fn to_native(l: &rare_library::model::Library) -> Option<Library> {
    serde_json::to_value(l).ok().and_then(|v| serde_json::from_value(v).ok())
}

/// Identifies ROMs Rare hasn't seen (their art links come from that), one worker at a time; the
/// library scans again as they are done.
fn fetch_rom_art(ctx: rare_core::Ctx, home: std::path::PathBuf, roms: Vec<rare_emu::roms::Rom>) {
    static BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if roms.is_empty() || BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("rr-rom-ident".into())
        .spawn(move || {
            // The library picks pictures up as they arrive, at most every 4 s.
            let last = std::sync::Mutex::new(std::time::Instant::now());
            let pending = std::sync::atomic::AtomicBool::new(false);
            let arrived = || {
                let mut l = last.lock().unwrap_or_else(|e| e.into_inner());
                if l.elapsed().as_secs() >= 4 {
                    *l = std::time::Instant::now();
                    pending.store(false, std::sync::atomic::Ordering::SeqCst);
                    ctx.send(rare_core::Event::LibraryChanged);
                } else {
                    pending.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            };
            // Which game each new ROM is (its own ID, the dump catalogues): its picture links follow.
            let got = rare_emu::ident::identify_missing(&home, &roms, &arrived);
            BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
            if got > 0 {
                log::info!("roms identified: {got}");
                if pending.load(std::sync::atomic::Ordering::SeqCst) {
                    ctx.send(rare_core::Event::LibraryChanged);
                }
            }
        })
        .ok();
}

/// Installed Rare Store games as library games (src-tauri store.rs game_from).
fn rare_store_games() -> Vec<rare_library::model::Game> {
    use rare_library::model::{Art, Game, LaunchSpec, Source};
    rare_store::installed()
        .iter()
        .map(|i| {
            let r = &i.record;
            let run = rare_store::run_dir_of(i);
            let program = std::path::Path::new(&r.exec).components().fold(run.clone(), |p, c| p.join(c));
            let one = |v: &Option<String>| v.iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>();
            Game {
                id: i.id.clone(),
                source: Source::Rare,
                title: r.name.clone(),
                installed: true,
                install_dir: Some(i.dir.clone()),
                size_bytes: Some(r.size),
                last_played: None,
                playtime_minutes: None,
                art: Art { cover: one(&r.images.cover), hero: one(&r.images.banner), logo: one(&r.images.logo), header: one(&r.images.banner), icon: vec![] },
                launch: LaunchSpec::Exec { program: program.to_string_lossy().into_owned(), args: r.args.clone(), cwd: Some(run.to_string_lossy().into_owned()), windows: r.runtime == rare_store::unpack::Runtime::Proton },
                origin: "Rare Store".into(),
                foreign: false,
            }
        })
        .collect()
}

/// Free bytes on the drive holding `p` (its nearest folder that exists; the home folder for None).
pub fn free_space(p: Option<&std::path::Path>) -> Option<u64> {
    let mut p = p.map(|p| p.to_path_buf()).or_else(|| std::env::var_os("HOME").map(PathBuf::from))?;
    while !p.exists() {
        p = p.parent()?.to_path_buf();
    }
    rare_library::storage::space(&p).map(|(_, free)| free)
}

/// Steam removes its own games: steam://uninstall/<appid> through rare-launch's `steam_dialog`, so
/// only Steam's confirm shows and Rare takes the screen back once the game's app manifest is gone
/// (Steam always asks; it has no quiet uninstall). When the player comes back to Rare first (the
/// Rare button) and the manifest is still there, Steam didn't remove it.
fn steam_uninstall(ctx: &rare_core::Ctx, runner: &rare_launch::launch::Runner, appid: u32, manifest: &std::path::Path) -> Result<(), String> {
    use rare_launch::apps::front;
    use std::time::{Duration, Instant};
    if !manifest.exists() {
        return Ok(());
    }
    // Rare has asked already: Steam's own uninstall with its prompt off, when its API is up (it
    // opens on the first Steam start after Rare set it up; a restart here would close a game).
    let quiet = match rare_launch::steamdl::uninstall_quiet(appid) {
        None if runner.running().is_empty() => rare_launch::steamdl::ensure_api(runner).ok().and_then(|_| rare_launch::steamdl::uninstall_quiet(appid)),
        q => q,
    };
    if let Some(r) = quiet {
        r?;
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(300) {
            std::thread::sleep(Duration::from_millis(500));
            if !manifest.exists() {
                log::info!("uninstall: Steam removed {appid} (no prompt)");
                return Ok(());
            }
        }
        return Err("Steam didn't remove it. Try again.".into());
    }
    let m = manifest.to_path_buf();
    rare_launch::apps::steam_dialog(ctx, runner, &format!("steam://uninstall/{appid}"), move || !m.exists(), Duration::from_secs(1800)).map_err(|e| e.to_string())?;
    let t0 = Instant::now();
    let mut away = false;
    loop {
        std::thread::sleep(Duration::from_millis(500));
        if !manifest.exists() {
            log::info!("uninstall: Steam removed {appid}");
            return Ok(());
        }
        let ours = front::launcher_front();
        away |= !ours;
        if away && ours {
            // Back in Rare: Steam may still be deleting; give it a few seconds.
            for _ in 0..20 {
                std::thread::sleep(Duration::from_millis(500));
                if !manifest.exists() {
                    return Ok(());
                }
            }
            return Err("Steam didn't uninstall it.".into());
        }
        if (!away && t0.elapsed() > Duration::from_secs(180)) || t0.elapsed() > Duration::from_secs(1800) {
            return Err("Steam didn't answer. Open Steam and try again.".into());
        }
    }
}

/// A web game leaves launcher.json (lib/webgames.ts removeWebGame): a built-in one is marked
/// removed, one the player added is dropped.
fn remove_web_game(bare: &str) {
    use serde_json::Value;
    let mut removed: Vec<String> = crate::features::config::get("removedBuiltins").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
    if rare_library::builtin_web_games().iter().any(|b| b.id == bare) && !removed.iter().any(|r| r == bare) {
        removed.push(bare.to_string());
    }
    let mut web = crate::features::config::get("webGames").and_then(|v| v.as_array().cloned()).unwrap_or_default();
    web.retain(|w| w.get("id").and_then(Value::as_str) != Some(bare));
    crate::features::config::set(&[("removedBuiltins", serde_json::json!(removed)), ("webGames", Value::Array(web))]);
}

/// What rare-stream needs from the launcher (src-tauri/src/stream.rs Host), over rare-launch.
pub(crate) struct StreamHost<'a> {
    pub(crate) runner: &'a rare_launch::launch::Runner,
    pub(crate) ctx: &'a rare_core::Ctx,
}

impl rare_stream::StreamHost for StreamHost<'_> {
    fn flatpak_installed(&self, id: &str) -> bool {
        rare_launch::apps::flatpak_installed(id)
    }
    fn which(&self, program: &str) -> bool {
        rare_core::proc::which(program)
    }
    fn host_env(&self, cmd: &mut std::process::Command) {
        rare_launch::launch::host_env(cmd);
    }
    fn host_command_unit(&self, program: &str, args: &[String], run_id: &str) -> (std::process::Command, Option<String>) {
        rare_launch::launch::host_command_unit(program, args, run_id)
    }
    fn user_flathub(&self) -> bool {
        rare_launch::apps::env().user_flathub
    }
    fn install_command(&self, flatpak: &str, user_flathub: bool) -> String {
        rare_launch::apps::catalog::install_command(flatpak, user_flathub)
    }
    fn running(&self) -> Vec<String> {
        self.runner.running()
    }
    fn quit_run(&self, id: &str) {
        let _ = self.runner.quit(self.ctx, id);
    }
    fn track_child(&self, run_id: &str, child: std::process::Child, unit: Option<String>) {
        self.runner.track_child(run_id, child, unit);
    }
    fn to_front(&self, run_id: &str) {
        // While a stream is in front the pad belongs to Moonlight: never pointer mode.
        rare_launch::apps::front::set_pointer(run_id, None);
        rare_launch::apps::front::to_front(run_id);
    }
    fn step_aside(&self) {
        rare_launch::apps::step_aside(self.ctx);
    }
}

impl Backend for DeviceBackend {
    fn game_perf(&self, id: &str) -> GamePerf {
        crate::features::gamemgmt::device_perf(id)
    }
    fn set_game_perf(&self, id: &str, p: Option<rare_system::power::GamePower>) -> Result<(), String> {
        crate::features::gamemgmt::device_set_perf(id, p)
    }
    fn verify_start(&self, g: &Game, keep: bool) -> Result<(), String> {
        use crate::features::gamemgmt::verify;
        if self.runner.running().iter().any(|r| r == &g.id) {
            return Err("Close the game first.".into());
        }
        match g.id.strip_prefix("steam:").and_then(|a| a.parse::<u32>().ok()).filter(|_| g.source == "steam" && !g.foreign) {
            // Steam "checks" a game it no longer has and says all is well: say so first.
            Some(_) if g.install_dir.as_deref().is_some_and(|d| !std::path::Path::new(d).is_dir()) => Err("Steam doesn't have this game installed any more.".into()),
            Some(appid) => verify::steam_start(&self.ctx, &self.runner, &g.id, appid, g.size_bytes.unwrap_or(0.0) as u64),
            None => verify::local_start(&g.id, g.install_dir.as_deref().ok_or("This game has no folder to check.")?, keep),
        }
    }
    fn add_game(&self, path: &std::path::Path) -> Result<(String, String), String> {
        let (id, title) = rare_library::custom::add_game(path)?;
        let art = crate::features::gamemgmt::art::automatic(&title);
        if !art.is_empty() {
            rare_library::custom::update(&id, |c| {
                c.cover = c.cover.take().or(art.cover);
                c.hero = c.hero.take().or(art.hero);
                c.logo = c.logo.take().or(art.logo);
            })?;
        }
        Ok((id, title))
    }
    fn game_custom(&self, id: &str) -> rare_library::custom::Custom {
        rare_library::custom::load().remove(id).unwrap_or_default()
    }
    fn set_game_custom(&self, id: &str, c: rare_library::custom::Custom) -> Result<(), String> {
        rare_library::custom::update(id, |x| *x = c).map(|_| ())
    }
    fn browser_name(&self) -> Option<String> {
        rare_library::browser::find().map(|b| b.name)
    }
    fn webgame_open_browser(&self, id: &str, url: &str) -> Result<String, String> {
        rare_launch::api::webgame_open_browser(&self.runner, id, url).map_err(|e| e.to_string())
    }
    fn proton_status(&self, appid: u64) -> Option<ProtonStatus> {
        let s = rare_launch::api::proton_status(Some(appid)).ok()?;
        serde_json::to_value(&s).ok().and_then(|v| serde_json::from_value(v).ok())
    }
    fn proton_set_game(&self, appid: u64, tool: Option<&str>) -> Result<(), String> {
        // Steam starts again afterwards unless the player starts it on demand (config.rs steam_mode).
        let restart = crate::features::config::get("steamStart").and_then(|v| v.as_str().map(String::from)).as_deref() != Some("demand");
        rare_launch::api::proton_set_game(&self.runner, appid, tool, restart).map_err(|e| e.to_string())
    }
    fn proton_install(&self, kind: &str) -> Result<String, String> {
        rare_launch::api::proton_install(&self.ctx, kind).map_err(|e| e.to_string())
    }
    fn proton_remove(&self, name: &str) -> Result<(), String> {
        rare_launch::api::proton_remove(name).map_err(|e| e.to_string())
    }
    fn umu_proton_status(&self, id: &str) -> Option<ProtonStatus> {
        let mut s = rare_launch::api::proton_status(None).ok()?;
        s.global = rare_library::umu::default_tool();
        s.game = rare_library::umu::choice(id);
        serde_json::to_value(&s).ok().and_then(|v| serde_json::from_value(v).ok())
    }
    fn umu_proton_set(&self, id: &str, tool: Option<&str>) -> Result<(), String> {
        rare_library::umu::set_choice(id, tool)
    }
    fn emu_actions(&self, id: &str) -> Vec<rare_emu::actions::EmuAction> {
        let Some(g) = self.lib_game(id) else { return vec![] };
        let program = if let rare_library::model::LaunchSpec::Exec { program, .. } = &g.launch { Some(program.as_str()) } else { None };
        rare_emu::actions::emu_actions(id, program, &g.title)
    }
    fn emu_do(&self, action: &rare_emu::actions::EmuAction) -> Result<(), String> {
        if !self.runner.running().contains(&action.app_id) {
            return Err("That game isn't running.".into());
        }
        rare_emu::actions::emu_do(&self.ctx, action).map_err(|e| e.to_string())
    }
    fn uninstall_info(&self, id: &str) -> Result<UninstallInfo, String> {
        if rare_android::package_of(id).is_some() {
            return crate::features::android::uninstall_info(id);
        }
        let plan = self.uninstall_plan(id)?;
        let info = UninstallInfo {
            steam: matches!(plan.how, rare_library::uninstall::How::Steam { .. }),
            size: plan.size,
            saves: plan.saves.as_ref().filter(|s| s.found).map(|s| {
                use rare_library::uninstall::SavesKind;
                (s.size, match s.kind {
                    SavesKind::Prefix => "prefix",
                    SavesKind::Web => "web",
                    SavesKind::AppData => "app",
                })
            }),
            place: plan.place.clone(),
            free: free_space(plan.dirs.first().or(plan.files.first()).map(|p| p.as_path()).or_else(|| match &plan.how {
                rare_library::uninstall::How::Steam { manifest, .. } => manifest.parent(),
                _ => None,
            })),
        };
        self.plans.lock().unwrap().insert(id.to_string(), plan);
        Ok(info)
    }
    fn uninstall(&self, id: &str, keep_saves: bool, tick: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
        use rare_library::uninstall::{self as un, How};
        if self.runner.running().iter().any(|r| r == id) {
            return Err("Quit it first, then uninstall it.".into());
        }
        if rare_android::package_of(id).is_some() {
            return crate::features::android::uninstall(id);
        }
        let cached = self.plans.lock().unwrap().remove(id);
        let mut plan = match cached {
            Some(p) => p,
            None => self.uninstall_plan(id)?,
        };
        // Saves the sheet didn't ask about (none found) go with the game.
        let keep = keep_saves && plan.saves.as_ref().is_some_and(|s| s.found);
        match plan.how.clone() {
            How::Steam { appid, manifest } => {
                steam_uninstall(&self.ctx, &self.runner, appid, &manifest)?;
                tick(plan.size, plan.size);
                return Ok(());
            }
            How::Rare { slug } => rare_store::uninstall(&slug)?,
            How::Web { id: bare } => remove_web_game(&bare),
            How::Flatpak { app, user } => {
                un::flatpak_uninstall(&app, user, !keep)?;
                plan.saves = None;
            }
            How::Files => {}
        }
        un::remove(&plan, keep, tick)
    }
    fn steam_launch_options(&self, appid: u64) -> Option<String> {
        rare_launch::api::steam_launch_options(appid).ok()
    }
    fn steam_set_launch_options(&self, appid: u64, opts: &str) -> Result<(), String> {
        let restart = crate::features::config::get("steamStart").and_then(|v| v.as_str().map(String::from)).as_deref() != Some("demand");
        rare_launch::api::steam_set_launch_options(&self.runner, appid, opts, restart).map_err(|e| e.to_string())
    }
    fn compat(&self, appid: u64) -> super::compat::Compat {
        super::compat::lookup(appid)
    }
    fn pc_shelf(&self) -> Option<PcShelf> {
        let hosts = rare_stream::refresh();
        let host = hosts.into_iter().find(|h| h.paired && h.online)?;
        let apps = rare_stream::store::apps_of(&host.uuid, &rare_stream::store::load(), &rare_stream::conf::load());
        if apps.is_empty() {
            return None;
        }
        // Games with box art first (Sunshine lists Desktop and Steam Big Picture first).
        let (with, without): (Vec<_>, Vec<_>) = apps.into_iter().partition(|a| a.art.is_some());
        let apps = with.into_iter().chain(without).map(|a| PcApp { id: a.id, name: a.name, art: a.art }).collect();
        Some(PcShelf { uuid: host.uuid, name: host.name, online: host.online, can_wake: host.can_wake, current_game: host.current_game, apps })
    }
    fn app_statuses(&self) -> Vec<AppStatus> {
        use crate::screens::apps::catalog::{Controller as C, Offer};
        rare_launch::apps::statuses()
            .into_iter()
            .map(|s| AppStatus {
                id: s.id,
                way: s.way,
                opens_in: s.opens_in,
                other_ways: s.other_ways,
                offer: s.offer.map(|o| Offer { flatpak: o.flatpak, name: o.name, command: o.command }),
                controller: s.controller.map(|c| match c {
                    rare_launch::apps::catalog::Controller::Focus => C::Focus,
                    rare_launch::apps::catalog::Controller::Pointer => C::Pointer,
                    rare_launch::apps::catalog::Controller::Gamepad => C::Gamepad,
                }),
            })
            .collect()
    }
    fn app_open(&self, id: &str, way: Option<usize>) -> Result<(), String> {
        rare_launch::apps::open(&self.ctx, &self.runner, id, way).map(|_| ()).map_err(|e| e.to_string())
    }
    fn app_resume(&self, id: &str) -> Result<(), String> {
        rare_launch::apps::resume(&self.ctx, &self.runner, id).map_err(|e| e.to_string())
    }
    fn app_install(&self, id: &str) -> Result<(), String> {
        let st = rare_launch::apps::statuses().into_iter().find(|s| s.id == id).ok_or("Unknown app")?;
        let offer = st.offer.ok_or("Nothing to install")?;
        rare_launch::apps::install::start(self.ctx.clone(), id.to_string(), offer.flatpak, rare_launch::apps::env().user_flathub).map_err(|e| e.to_string())
    }
    fn stream_api(&self) -> Option<Box<dyn crate::screens::stream::state::StreamApi>> {
        Some(Box::new(crate::screens::stream::device::DeviceApi::new(self.runner.clone(), self.ctx.clone())))
    }
    fn stream_play(&self, uuid: &str, app: u32) -> Result<(), String> {
        rare_stream::launch(&StreamHost { runner: &self.runner, ctx: &self.ctx }, uuid, app, false)
    }
    fn cached_library(&self) -> Option<Library> {
        match &self.cache {
            Some(p) => Self::read_file(p),
            None => rare_library::cache::load().as_ref().and_then(to_native),
        }
    }
    fn scan(&self) -> Result<Library, String> {
        if let Some(p) = &self.cache {
            return Self::read_file(p).ok_or_else(|| format!("no library at {}", p.display()));
        }
        // rare-library's scan with the launcher.json folders and web games and the Rare Store's
        // installed games (src-tauri library_scan), kept in the cache both launchers read.
        let folders: Vec<String> = crate::features::config::get("gameFolders").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        let web: Vec<rare_core::WebGame> = crate::features::config::get("webGames").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        let removed: Vec<String> = crate::features::config::get("removedBuiltins").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        // Games Heroic installed become Rare's (legendary's and gogdl's lists) before the scan reads them.
        rare_stores::adopt::run();
        let t0 = std::time::Instant::now();
        let mut lib = rare_library::scan(&folders, &web, &removed, rare_store_games());
        // Cloud games from the lists kept on disk; a day-old or
        // missing list is fetched in the background and the library scans again after it.
        let cloud = rare_launch::apps::cloud::library_games(&lib.games);
        lib.sources.push(rare_library::model::SourceStatus { source: rare_library::model::Source::Cloud, available: true, count: cloud.len(), note: "Xbox Cloud Gaming and GeForce NOW".into() });
        let cloud_ids: std::collections::HashSet<&str> = cloud.iter().map(|g| g.id.as_str()).collect();
        for (id, appid) in rare_launch::apps::cloud::steam_keys() {
            if cloud_ids.contains(id.as_str()) {
                lib.keys.insert(id, format!("steam:{appid}"));
            }
        }
        drop(cloud_ids);
        lib.games.extend(cloud);
        let ctx = self.ctx.clone();
        rare_launch::apps::cloud::refresh_if_stale(move || ctx.send(rare_core::Event::LibraryChanged));
        // Android apps and games (Waydroid), once Android is set up.
        let android = rare_android::games();
        lib.sources.push(rare_library::model::SourceStatus { source: rare_library::model::Source::Android, available: rare_android::status() != rare_android::Status::Unavailable, count: android.len(), note: "Android".into() });
        lib.games.extend(android);
        // ROMs in the Emulation folders (device and SD card), and their box art in the background.
        let roms = rare_emu::roms::scan();
        let home = rare_emu::roms::home();
        lib.sources.push(rare_library::model::SourceStatus { source: rare_library::model::Source::Emu, available: true, count: roms.len(), note: "Emulation folders".into() });
        lib.games.extend(roms.iter().map(|r| rare_emu::roms::game(r, &home)));
        // Disc serials: one game as an ISO and as a folder, or in two formats (data/groups.rs).
        for r in &roms {
            if let Some(k) = rare_emu::serial::key(r) {
                lib.keys.insert(r.id.clone(), k);
            }
        }
        fetch_rom_art(self.ctx.clone(), home, roms);
        // The names and art the player changed (rare-library custom.rs).
        rare_library::custom::apply(&mut lib, &rare_library::custom::load());
        log::info!("library: {} games in {} ms", lib.games.len(), t0.elapsed().as_millis());
        rare_library::cache::save(&lib);
        let out = to_native(&lib).ok_or("the library didn't convert")?;
        *self.last.lock().unwrap() = Some(lib);
        Ok(out)
    }
    fn device(&self) -> Option<Device> {
        let battery = rare_system::system::battery().map(|b| Battery { percent: b.percent, charging: b.charging, minutes: b.minutes });
        // The top bar's "Offline": NetworkManager says there is no network (unknown counts as online).
        let wifi = match rare_system::netctl::linked() {
            Some(false) => None,
            _ => Some(String::new()),
        };
        Some(Device { name: String::new(), battery, wifi, ssh: rare_system::ssh::active() })
    }
    fn running(&self) -> Vec<String> {
        self.runner.running()
    }
    fn launch(&self, id: &str) -> Result<(), String> {
        if self.runner.running().iter().any(|r| r == id) {
            return rare_launch::api::game_resume(&self.ctx, &self.runner, id).map_err(|e| e.to_string());
        }
        let mut g = self.lib_game(id).ok_or_else(|| format!("{id} is not in the library"))?;
        // A ROM starts in its system's emulator as it is installed now.
        let rom = match id.starts_with("emu:") {
            true => Some(rare_emu::roms::find(id).ok_or("That game isn't in its ROM folder any more. Scan again from the Library.")?),
            false => None,
        };
        if rare_android::package_of(id).is_some() {
            // One Android at a time: a second app would end the first one's session.
            if let Some(other) = self.runner.running().into_iter().find(|r| rare_android::package_of(r).is_some()) {
                let name = self.lib_game(&other).map(|g| g.title.clone()).unwrap_or_else(|| "the Android app that's open".into());
                return Err(format!("Quit {name} first. Android runs one app at a time."));
            }
            if rare_android::jobs::current().is_some() {
                return Err("Android is busy with an install. Try again when it's done.".into());
            }
            rare_android::ensure_files().map_err(|e| e.to_string())?;
            rare_android::note_played(id);
        }
        if let Some(rom) = &rom {
            g.launch = rare_emu::roms::launch_spec(rom, &rare_emu::roms::home(), true)?;
        }
        // Epic and GOG: newer cloud saves first.
        crate::features::stores::before_launch(id, &g.title);
        // A web game opens in its own window process (features/webgame.rs).
        let launched = rare_launch::api::launch_game(&self.ctx, &self.runner, &g, |g| {
            // Emulators with no config yet get the tuned first-run settings, and their power
            // profile until the player saves one for the game (never overwrites).
            let program = if let rare_library::model::LaunchSpec::Exec { program, .. } = &g.launch { Some(program.as_str()) } else { None };
            let mut s = power_load();
            match &rom {
                // A ROM: Rare's RetroArch settings, the shader, achievements, the emulator's hotkeys.
                Some(rom) => {
                    if program != Some("flatpak") {
                        rare_emu::prepare_launch(program, &g.title);
                    }
                    rare_emu::roms::prepare(rom, &rare_emu::roms::home());
                    if rare_emu::seed_power(&mut s.game_power, &g.id, None, rare_emu::roms::emulator_of(rom.system).name) {
                        power_save(&s);
                    }
                }
                None => {
                    rare_emu::prepare_launch(program, &g.title);
                    if rare_emu::seed_power(&mut s.game_power, &g.id, program, &g.title) {
                        power_save(&s);
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;
        // Its power settings (saved for it, or Balanced) and the game-load boost window.
        crate::features::services::game_started(&g.id, launched.boost, launched.boost_target);
        rare_launch::apps::front::to_front(&g.id);
        Ok(())
    }
    fn play_web(&self, id: &str, title: &str, url: &str) -> Result<(), String> {
        rare_launch::api::webgame_open(&self.ctx, &self.runner, id, title, url).map(|_| ()).map_err(|e| e.to_string())
    }
    fn quit(&self, id: &str) -> Result<(), String> {
        match self.runner.quit(&self.ctx, id) {
            // Rare lost track of the run (its first process handed over and exited), but its
            // windows still carry its app id: close the processes behind them (TERM, KILL after 6 s).
            Err(e) if e.contains("not running") => {
                let pids = rare_overlay::appid::of(id).map(rare_overlay::appid::window_pids).unwrap_or_default();
                if pids.is_empty() {
                    return Err(e);
                }
                log::warn!("quit {id}: not tracked, closing its windows' processes {pids:?}");
                let list: Vec<String> = pids.iter().map(|p| p.to_string()).collect();
                let _ = std::process::Command::new("kill").arg("-TERM").args(&list).status();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(6));
                    let left: Vec<String> = list.into_iter().filter(|p| std::path::Path::new("/proc").join(p).exists()).collect();
                    if !left.is_empty() {
                        let _ = std::process::Command::new("kill").arg("-KILL").args(&left).status();
                    }
                });
                Ok(())
            }
            r => r,
        }
    }
    fn steam_prewarm(&self) {
        if steam_mode() != "demand" {
            let _ = rare_launch::launch::steam_prewarm();
        }
    }
    fn launch_phase(&self, id: &str) -> Phase {
        match self.runner.phase(id) {
            "steam" => Phase::Steam,
            "game" => Phase::Game,
            "answer" => Phase::Answer(self.runner.question(id).unwrap_or("Steam needs an answer before the game can start")),
            "playing" => Phase::Playing,
            _ => Phase::None,
        }
    }
    fn steam_big_picture(&self) -> Result<(), String> {
        rare_launch::api::steam_big_picture(&self.ctx, &self.runner).map_err(|e| e.to_string())
    }
    fn steam_game_page(&self, appid: u32) -> Result<(), String> {
        rare_launch::api::steam_game_page(&self.ctx, &self.runner, appid).map_err(|e| e.to_string())
    }
    fn steam_store(&self, appid: u32) -> Result<(), String> {
        rare_launch::api::steam_store(&self.ctx, &self.runner, appid).map_err(|e| e.to_string())
    }
    fn steam_url(&self, url: &str) -> Result<(), String> {
        if !url.starts_with("steam://") {
            return Err("Not a Steam link".into());
        }
        rare_launch::apps::steam(&self.ctx, &self.runner, url).map_err(|e| e.to_string())
    }
    fn steam_add_library(&self, path: &str) -> Result<(), String> {
        rare_launch::api::steam_add_library(&self.runner, path, steam_mode() != "demand").map_err(|e| e.to_string())?;
        // Steam lists the folder's games once it has read the file; the library picks them up.
        Ok(())
    }
    fn steam_downloads(&self) -> Option<rare_launch::steamdl::Snapshot> {
        Some(rare_launch::steamdl::snapshot())
    }
    fn steam_folders(&self) -> Vec<rare_launch::steamdl::Folder> {
        rare_launch::steamdl::folders()
    }
    fn steam_install(&self, appid: u32, folder: &str) -> Result<rare_launch::steamdl::Started, String> {
        rare_launch::steamdl::install(&self.ctx, &self.runner, appid, folder).map_err(|e| e.to_string())
    }
    fn steam_control(&self, appid: u32, c: rare_launch::steamdl::Control) -> Result<bool, String> {
        rare_launch::steamdl::control(&self.ctx, &self.runner, appid, c).map_err(|e| e.to_string())
    }
    fn perf(&self) -> Option<Perf> {
        let s = power_load();
        let p = rare_system::system::perf_get(&rare_system::power::wanted_now(&s));
        Some(Perf {
            profile: name_of(&p.profile),
            tdp: p.tdp.map(|t| t as f32),
            tdp_min: p.tdp_min as f32,
            tdp_max: p.tdp_max as f32,
            fps_limit: p.fps_limit,
            cpu_boost: p.cpu_boost,
            fan: p.fan.map(|f| name_of(&f)),
            watts: [p.profile_watts.quiet as f32, p.profile_watts.balanced as f32, p.profile_watts.performance as f32],
            note: p.note,
        })
    }
    fn set_perf(&self, profile: &str, tdp: Option<f32>, fps: Option<u32>) -> Result<(), String> {
        let profile: rare_core::Profile = named(profile).ok_or("Unknown profile")?;
        let mut s = power_load();
        let wanted = rare_system::power::perf_set_config(&mut s, profile, tdp.map(f64::from), fps);
        power_save(&s);
        rare_system::power::perf_set_apply(tdp.map(f64::from), wanted, fps)
    }

    fn set_cpu_boost(&self, on: bool) -> Result<(), String> {
        let mut s = power_load();
        rare_system::power::perf_boost_config(&mut s, on);
        power_save(&s);
        rare_system::system::set_cpu_boost(on)
    }
    fn set_fan(&self, mode: &str) -> Result<(), String> {
        rare_system::system::set_fan(named(mode).ok_or("Unknown fan mode")?)
    }
    fn fan_status(&self) -> Option<rare_system::system::FanStatus> {
        rare_system::system::fan_status()
    }
    fn set_fan_curve(&self, points: &[(u32, u32)]) -> Result<(), String> {
        rare_system::system::set_fan_curve(points)
    }
    fn game_fx(&self) -> Option<(rare_system::gamefx::FrameGen, rare_system::gamefx::Status)> {
        let id = rare_system::power::current_game()?;
        let fg = power_load().game_power.get(&id).and_then(|g| g.framegen).unwrap_or_default();
        Some((fg, rare_system::gamefx::status()))
    }
    fn set_framegen(&self, f: rare_system::gamefx::FrameGen) -> Result<(), String> {
        let mut s = power_load();
        rare_system::power::framegen_set_config(&mut s, f).ok_or("No game is running")?;
        power_save(&s);
        Ok(())
    }
    fn levels(&self) -> Option<Levels> {
        let l = rare_system::system::levels();
        Some(Levels { brightness: l.brightness.map(f32::from), volume: l.volume.map(f32::from) })
    }
    fn set_levels(&self, b: Option<f32>, v: Option<f32>) -> Result<(), String> {
        if let Some(b) = b {
            rare_system::system::set_brightness(b.round().clamp(0.0, 100.0) as u8)?;
        }
        if let Some(v) = v {
            rare_system::system::set_volume(v.round().clamp(0.0, 100.0) as u8)?;
        }
        Ok(())
    }
    fn audio(&self) -> Option<rare_system::audio::Audio> {
        Some(rare_system::audio::read())
    }
    fn radios(&self) -> Option<Radios> {
        let r = rare_system::radios::state();
        Some(Radios { wifi: r.wifi, bluetooth: r.bluetooth, airplane: r.airplane })
    }
    fn set_radio(&self, kind: &str, on: bool) -> Result<Radios, String> {
        let r = rare_system::radios::set(kind, on)?;
        Ok(Radios { wifi: r.wifi, bluetooth: r.bluetooth, airplane: r.airplane })
    }
    fn hud(&self) -> String {
        name_of(&rare_system::power::wanted_hud(&power_load()))
    }
    fn set_hud(&self, level: &str) -> Result<(), String> {
        let level: rare_system::perfhud::HudLevel = named(level).ok_or("Unknown overlay level")?;
        let mut s = power_load();
        rare_system::power::hud_set_config(&mut s, level);
        power_save(&s);
        rare_system::perfhud::apply(level);
        Ok(())
    }
    fn power(&self, action: &str) -> Result<(), String> {
        rare_system::system::power(action)
    }
    fn game_power(&self) -> Option<(String, bool)> {
        rare_system::power::view(&power_load()).map(|g| (g.id, g.saved))
    }
    fn save_game_power(&self, save: bool) -> Result<(), String> {
        let mut s = power_load();
        rare_system::power::save_for_game(&mut s, save);
        power_save(&s);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rare_library_scan_converts() {
        let lib = rare_library::scan(&[], &[], &[], vec![]);
        let n = super::to_native(&lib).expect("converts");
        assert_eq!(n.games.len(), lib.games.len());
        assert!(n.games.iter().any(|g| g.source == "web" && g.launch.as_ref().is_some_and(|l| l.url.is_some())));
    }
}
