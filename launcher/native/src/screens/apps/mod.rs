//! Apps screens (launcher/src/apps, docs/APPS.md): the tiles on Home's Apps shelf and in Library >
//! Apps, an app's page, and the running list. Workstream 13.
//!
//! `catalog` is catalog.ts over catalog.json, `state` the view-model with its `Host` trait for the
//! backend calls, `tiles` and `sheets` the screens. The Switcher and the Rare menu can import
//! `state::{RunningItem, running_items, resume_item, quit_item}` and `sheets::running_list`.

pub mod catalog;
pub mod sheets;
pub mod state;
pub mod tiles;

pub use catalog::{app_by_id, app_by_run_id, apps, run_id, AppEntry, AppStatus};
pub use sheets::{AppSheet, RunningSheet};
pub use state::{quit_item, resume_item, running_items, Apps, RunningItem};
pub use tiles::{app_icon, app_tile, install_bar, AppsTiles, Layout};

use crate::screens::overlay::common::{Cx, Screen};
use catalog::status_for;
use rare_render::Src;
use state::{GameRef, InstallState, MockHost, StreamRef};

static ACT_COVER: &[u8] = include_bytes!("../../../../render/assets/games/act/cover.jpg");

const OWNER: [&str; 2] = ["com.google.Chrome", "org.telegram.desktop"];

/// The model the fixtures run on: the web mock's installed list (the owner's SuiPlay has Chrome
/// and Telegram Desktop), a recording host, and no looping motion so the scene settles.
pub fn demo(installed: &[&str], running: &[&str]) -> state::Shared {
    let host = MockHost {
        games: vec![GameRef { id: "steam:730".into(), title: "Counter-Strike 2".into(), art: Some(Src::Static("fx/act-cover.jpg", ACT_COVER)) }],
        streams: vec![(
            "stream:pc:1".into(),
            StreamRef { host: "Desktop PC".into(), app: "Word Game".into(), art: Some(Src::Static("fx/act-cover.jpg", ACT_COVER)) },
            "Word Game".into(),
        )],
        ..Default::default()
    };
    let mut a = Apps::new(Box::new(host));
    a.live = false;
    a.set_status(apps().iter().map(|x| status_for(x, installed, true)).collect());
    a.running = running.iter().map(|s| s.to_string()).collect();
    a.shared()
}

fn installing(a: &state::Shared, id: &str, progress: Option<f32>, error: Option<&str>) {
    let mut m = a.borrow_mut();
    m.installing.insert(id.into(), InstallState { progress, error: error.map(String::from) });
    m.touch();
}

/// Fixture screens by name: "apps-<state>".
pub fn fixture(name: &str, cx: &mut Cx, now: f64) -> Option<Box<dyn Screen>> {
    let tiles = |cx: &mut Cx, a: state::Shared, l: Layout, f: Option<&str>| Some(Box::new(AppsTiles::build(cx, a, l, f, now)) as Box<dyn Screen>);
    let sheet = |cx: &mut Cx, a: state::Shared, id: &str| Some(Box::new(AppSheet::build(cx, a, id, now)?) as Box<dyn Screen>);
    let all = ["steam:730", "app:telegram", "stream:pc:1"];
    match name {
        // Home's Apps shelf: Telegram running, Discord installing, the rest ready.
        "apps-tiles" => {
            let a = demo(&OWNER, &["app:telegram"]);
            installing(&a, "discord", Some(0.42), None);
            tiles(cx, a, Layout::Rail, None)
        }
        "apps-tiles-focus" => {
            let a = demo(&OWNER, &["app:telegram"]);
            installing(&a, "discord", Some(0.42), None);
            tiles(cx, a, Layout::Rail, Some("telegram"))
        }
        // Nothing installed: Netflix and YouTube cannot open yet.
        "apps-tiles-bare" => tiles(cx, demo(&[], &[]), Layout::Rail, Some("netflix")),
        "apps-tiles-installing-wait" => {
            let a = demo(&[], &[]);
            installing(&a, "netflix", None, None);
            tiles(cx, a, Layout::Rail, Some("netflix"))
        }
        "apps-tiles-error" => {
            let a = demo(&[], &[]);
            installing(&a, "netflix", None, Some("Couldn't install Google Chrome. Check your Wi-Fi and try again."));
            tiles(cx, a, Layout::Rail, Some("netflix"))
        }
        "apps-grid" => tiles(cx, demo(&OWNER, &["app:telegram"]), Layout::Grid, None),
        "apps-grid-bare" => tiles(cx, demo(&[], &[]), Layout::Grid, None),
        // An app's page.
        "apps-sheet-details" => sheet(cx, demo(&OWNER, &[]), "telegram"),
        "apps-sheet-open" => sheet(cx, demo(&OWNER, &[]), "netflix"),
        "apps-sheet-running" => sheet(cx, demo(&OWNER, &["app:telegram"]), "telegram"),
        "apps-sheet-web" => sheet(cx, demo(&OWNER, &[]), "x"),
        "apps-sheet-gamepad" => sheet(cx, demo(&OWNER, &[]), "youtube"),
        "apps-sheet-steam" => sheet(cx, demo(&OWNER, &[]), "steam"),
        // Get: the best way is missing.
        "apps-sheet-get" => sheet(cx, demo(&OWNER, &[]), "discord"),
        "apps-sheet-needs" => sheet(cx, demo(&[], &[]), "netflix"),
        "apps-sheet-install" => {
            let a = demo(&[], &[]);
            installing(&a, "discord", Some(0.42), None);
            sheet(cx, a, "discord")
        }
        "apps-sheet-install-wait" => {
            let a = demo(&[], &[]);
            installing(&a, "netflix", None, None);
            sheet(cx, a, "netflix")
        }
        "apps-sheet-error" => {
            let a = demo(&[], &[]);
            installing(&a, "netflix", None, Some("Couldn't install Google Chrome. Check your Wi-Fi and try again."));
            sheet(cx, a, "netflix")
        }
        "apps-sheet-guest" => {
            let a = demo(&OWNER, &["app:telegram"]);
            a.borrow_mut().guest = true;
            sheet(cx, a, "telegram")
        }
        // The running list.
        "apps-running" | "apps-sheet-running-list" => Some(Box::new(RunningSheet::build(cx, demo(&OWNER, &all), None, None, now))),
        "apps-running-armed" => Some(Box::new(RunningSheet::build(cx, demo(&OWNER, &all), Some("steam:730"), None, now))),
        "apps-running-armed-app" => Some(Box::new(RunningSheet::build(cx, demo(&OWNER, &all), Some("app:telegram"), None, now))),
        "apps-running-quitting" => Some(Box::new(RunningSheet::build(cx, demo(&OWNER, &all), None, Some("steam:730"), now))),
        "apps-running-one" => Some(Box::new(RunningSheet::build(cx, demo(&OWNER, &["app:telegram"]), None, None, now))),
        "apps-running-empty" => Some(Box::new(RunningSheet::build(cx, demo(&OWNER, &[]), None, None, now))),
        "apps-running-guest" => {
            let a = demo(&OWNER, &all);
            a.borrow_mut().guest = true;
            Some(Box::new(RunningSheet::build(cx, a, None, None, now)))
        }
        _ => None,
    }
}
