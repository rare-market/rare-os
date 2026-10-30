//! The Storage section (screens/settings/AccountsStorage.tsx StorageSection): the drives with their
//! free space and the SD card (format, set up for games), games and apps by size with Move and
//! Uninstall, sources, scan, game folders, when Steam starts, cover art key, Share.

use super::{config::Config, rows::*, SectionOut, SectionView};
use crate::screens::overlay::common::*;
use crate::screens::overlay::lay::s;
use rare_render::theme::tok;
use rare_render::{Id, Node};

/// lib/types.ts prettyPath: home folders (any user) become ~.
pub fn pretty_path(p: &str) -> String {
    let mut out = String::new();
    let mut rest = p;
    while let Some(i) = rest.find("/home/") {
        // "/var/home/<user>" or "/home/<user>"
        let start = if rest[..i].ends_with("/var") { i - 4 } else { i };
        out.push_str(&rest[..start]);
        let after = &rest[i + 6..];
        let end = after.find('/').unwrap_or(after.len());
        if end == 0 {
            out.push_str(&rest[start..i + 6]);
            rest = after;
            continue;
        }
        out.push('~');
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

pub const STEAM_START: [(&str, &str, &str); 3] = [
    ("boot", "After startup", "Steam starts quietly a little after the launcher. Steam games start fastest. Keeps Steam running, which costs some battery."),
    ("focus", "When I pick a game", "Steam starts quietly when you open a Steam game, so Play starts fast. Uses nothing until then."),
    ("demand", "On Play", "Steam starts only when you press Play. Uses nothing, and the first game takes a few seconds longer."),
];

fn steam_start_idx(c: &Config) -> usize {
    STEAM_START.iter().position(|(k, _, _)| Some(*k) == c.steam_start.as_deref()).unwrap_or(0)
}

/// "GOG" for gog, else the source capitalised.
pub fn source_label(source: &str) -> String {
    if source == "gog" {
        return "GOG".into();
    }
    let mut c = source.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Steam Cloud row text from Steam's local files; None off a real device run or without Steam data.
fn cloud_summary() -> Option<String> {
    super::live::get()?;
    let s = rare_library::steam_cloud::summary(&rare_library::steam::find_roots())?;
    Some(cloud_text(&s)).filter(|t| !t.is_empty())
}

fn cloud_text(s: &rare_library::steam_cloud::Summary) -> String {
    if s.off {
        return "Off".into();
    }
    let mut parts = Vec::new();
    if s.synced > 0 {
        parts.push(format!("{} {} synced", s.synced, if s.synced == 1 { "game" } else { "games" }));
    }
    if s.out_of_date > 0 {
        parts.push(format!("{} not synced yet", s.out_of_date));
    }
    if s.conflict > 0 {
        parts.push(format!("{} with a conflict", s.conflict));
    }
    parts.join(", ")
}

use crate::features::storage::{self as fst, Job, Snap};
use rare_library::movegame::Phase;
use rare_library::sdcard::FormatStage;
use rare_library::storage::{Drive, DriveKind, Item, ItemKind, SdCard};
use std::cell::Cell;
use std::path::PathBuf;

/// How the list is sorted: biggest first, by name, by drive.
const SORTS: [(&str, &str); 3] = [("Size", "Biggest first"), ("Name", "By name"), ("Drive", "By drive, biggest first")];
/// Rows shown before "Show all".
const FEW: usize = 12;

thread_local! {
    static SORT: Cell<usize> = const { Cell::new(0) };
    static ALL: Cell<bool> = const { Cell::new(false) };
    /// Which fixture a fixture run draws (`set_fixture`).
    static FIXTURE: Cell<&'static str> = const { Cell::new("") };
}

/// Fixture variants for shots: "" (card in), "nocard", "notready", "empty", "many" (300 rows, long
/// names, huge sizes, Show all), "moving", "format".
pub fn set_fixture(v: &'static str) {
    FIXTURE.with(|c| c.set(v));
    ALL.with(|c| c.set(v == "many"));
}

fn fixture_variant() -> Snap {
    let mut s = fixture_snap();
    match FIXTURE.with(|c| c.get()) {
        "nocard" => {
            s.drives.retain(|d| d.kind == DriveKind::Internal);
            s.sd = SdCard::None;
            if let Some(v) = s.items.as_mut() {
                v.retain(|i| !i.path.as_ref().is_some_and(|p| p.starts_with("/run/media")));
            }
        }
        "notready" => {
            s.drives.retain(|d| d.kind == DriveKind::Internal);
            s.sd = SdCard::NotReady { device: "/dev/mmcblk0".into(), size: 128_000_000_000 };
        }
        "empty" => s.items = Some(Vec::new()),
        "many" => {
            s.drives[0].total = 2_000_000_000_000_000;
            s.drives[0].free = 1_234_567_890_123;
            let mut v = s.items.take().unwrap_or_default();
            v.push(Item { key: "steam:9".into(), title: "The Elder Scrolls Anthology: Definitive Collection of Every Remaster Ed".into(), kind: ItemKind::Game, source: "Steam".into(), game_id: Some("steam:9".into()), path: Some("/var/home/rare/.local/share/Steam/steamapps/common/TES".into()), bytes: 1_450_000_000_000, no_move: None });
            for i in 0..290 {
                v.push(Item { key: format!("local:{i}"), title: format!("Game number {i}"), kind: ItemKind::Game, source: "Games folder".into(), game_id: Some(format!("local:{i}")), path: Some(format!("/var/home/rare/Games/G{i}").into()), bytes: 3_000_000 * (300 - i as u64), no_move: None });
            }
            v.sort_by(|a, b| b.bytes.cmp(&a.bytes));
            s.items = Some(v);
        }
        "moving" => s.job = Some(Job::Move { key: "steam:1145350".into(), to: "the SD card".into(), phase: Phase::Copying, done: 4_200_000_000, total: 12_100_000_000 }),
        "format" => s.job = Some(Job::Format(Some(FormatStage::Testing))),
        _ => {}
    }
    s
}

const T_FORMAT: u32 = 20;
const T_SETUP: u32 = 21;
const T_MEASURE: u32 = 22;
const T_SORT: u32 = 23;
const T_ALL: u32 = 24;
/// Item rows: ITEMS + 4 * index + (0: move, 1: uninstall). Above the search hits' 1000..9000.
const ITEMS: u32 = 10000;

/// "12.3 GB", "640 MB", "0 MB".
pub fn size_text(b: u64) -> String {
    let b = b as f64;
    if b >= 1e12 {
        format!("{:.1} TB", b / 1e12)
    } else if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else {
        format!("{} MB", (b / 1e6).round() as i64)
    }
}

/// "120 GB free of 256 GB".
pub fn free_line(d: &Drive) -> String {
    format!("{} free of {}", size_text(d.free), size_text(d.total))
}

struct RowRef {
    key: String,
    game_id: Option<String>,
    kind: ItemKind,
    /// Why Move does nothing (None: it moves).
    no_move: Option<String>,
    moving: bool,
}

pub struct StorageSection {
    folders: Vec<String>,
    steam_label: Id,
    steam_body: Id,
    key_btn_label: Id,
    key_body: Id,
    rows: Vec<RowRef>,
    busy: bool,
}

/// A drive's row: name, free space, a bar of the used part, buttons on the right.
fn drive_row(cx: &mut Cx, parent: Id, title: &str, body: &str, used: Option<f32>, first: bool) -> Id {
    let r = cx.r();
    let row = cx.scene.add(parent, s().row().items_center().gap(1.25 * r).min_h(4.25 * r).wp(1.0).px(1.4 * r).py(0.85 * r).done(), Node::default());
    if !first {
        edge(cx, row, 0, tok::HAIR);
    }
    let col = cx.scene.add(row, s().col().grow().gap(0.15 * r).done(), Node::default());
    cx.scene.add_text(cx.t, col, s().done(), title, ts(r, ty::BODY.0, ty::BODY.1, 600), tok::FG);
    cx.scene.add_text(cx.t, col, s().done(), body, ts(r, ty::SMALL.0, ty::SMALL.1, 400).wrap(), tok::MUTED);
    // The bar keeps its height when there is nothing to show, so the row never changes size.
    let track = cx.scene.add(col, s().wp(1.0).max_w(28.0 * r).h(0.375 * r).mt(0.4 * r).done(), Node::bg(if used.is_some() { tok::L3 } else { rare_render::Color::TRANSPARENT }).radius(0.1875 * r).clip());
    if let Some(u) = used {
        cx.scene.add(track, s().wp(u.clamp(0.0, 1.0)).h(0.375 * r).done(), Node::bg(tok::FG.alpha(0.7)).radius(0.1875 * r));
    }
    cx.scene.add(row, s().row().items_center().gap(0.5 * r).no_shrink_().done(), Node::default())
}

fn used_frac(d: &Drive) -> Option<f32> {
    (d.total > 0).then(|| d.used() as f32 / d.total as f32)
}

fn format_line(st: Option<FormatStage>) -> &'static str {
    match st {
        None | Some(FormatStage::Testing) => "Testing the card. This takes a minute or two.",
        Some(FormatStage::Formatting) => "Formatting",
        Some(FormatStage::Mounting) => "Almost done",
    }
}

/// The fixture drives and list (golden shots, the Mac preview).
fn fixture_snap() -> Snap {
    let g = |key: &str, title: &str, source: &str, gb: f64, path: &str, no_move: Option<&str>| Item {
        key: key.into(),
        title: title.into(),
        kind: ItemKind::Game,
        source: source.into(),
        game_id: Some(key.into()),
        path: Some(PathBuf::from(path)),
        bytes: (gb * 1e9) as u64,
        no_move: no_move.map(String::from),
    };
    let home = "/var/home/rare/.local/share/Steam/steamapps/common";
    let sd = "/run/media/rare/sd/SteamLibrary/steamapps/common";
    let mut items = vec![
        g("steam:1145350", "Hades II", "Steam", 12.1, &format!("{home}/Hades II"), None),
        g("steam:10180", "Call of Duty: Modern Warfare 2", "Steam", 12.1, &format!("{sd}/MW2"), None),
        g("steam:1449850", "Yu-Gi-Oh! Master Duel", "Steam", 4.4, &format!("{home}/Master Duel"), None),
        g("epic:fn", "Celeste", "Epic", 1.2, "/var/home/rare/Games/Heroic/Celeste", None),
        g("local:1", "Cave Story", "Games folder", 0.2, "/var/home/rare/Games/Cave Story", None),
        g("rare:act", "ACT", "Rare Store", 0.6, "/var/home/rare/Games/Rare/act", Some("Rare Store games stay in the Rare folder for now.")),
    ];
    items.push(Item { key: "flatpak:com.discordapp.Discord".into(), title: "Discord".into(), kind: ItemKind::App, source: "Flatpak".into(), game_id: Some("apps:discord".into()), path: Some("/var/lib/flatpak/app/com.discordapp.Discord".into()), bytes: 412_000_000, no_move: Some("Apps stay on this device.".into()) });
    items.push(Item { key: "roms:ps2".into(), title: "PlayStation 2 games".into(), kind: ItemKind::Roms, source: "Emulation".into(), game_id: None, path: Some("/var/home/rare/Emulation/roms/ps2".into()), bytes: 9_800_000_000, no_move: Some("ROMs stay in the emulation folder.".into()) });
    items.push(Item { key: "prefix:1".into(), title: "Hades II: saves and Windows files".into(), kind: ItemKind::Prefix, source: "Wine".into(), game_id: None, path: Some("/var/home/rare/.local/share/Steam/steamapps/compatdata/1145350".into()), bytes: 1_100_000_000, no_move: Some("Moves with its game.".into()) });
    items.push(Item { key: "orphan:/var/home/rare/.local/share/Steam/steamapps/compatdata/292030".into(), title: "Leftover saves, Steam game 292030".into(), kind: ItemKind::Prefix, source: "Left over".into(), game_id: None, path: Some("/var/home/rare/.local/share/Steam/steamapps/compatdata/292030".into()), bytes: 2_300_000_000, no_move: Some("It belongs to no game now. Delete it or leave it.".into()) });
    items.push(Item { key: "cache:img".into(), title: "Image cache".into(), kind: ItemKind::Cache, source: "Rare".into(), game_id: None, path: Some("/var/home/rare/.cache/rare/img".into()), bytes: 516_000_000, no_move: Some("The cache stays on this device.".into()) });
    items.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    Snap {
        drives: vec![
            Drive { kind: DriveKind::Internal, label: "This device".into(), mount: "/var/home".into(), device: "/dev/nvme0n1p3".into(), fs: "btrfs".into(), total: 511_000_000_000, free: 315_000_000_000 },
            Drive { kind: DriveKind::Sd, label: "SD card".into(), mount: "/run/media/rare/sd".into(), device: "/dev/mmcblk0p1".into(), fs: "ext4".into(), total: 255_000_000_000, free: 231_000_000_000 },
        ],
        sd: SdCard::Ready { mount: "/run/media/rare/sd".into(), device: "/dev/mmcblk0p1".into() },
        sd_set_up: true,
        items: Some(items),
        measuring: false,
        job: None,
        read: true,
    }
}

fn drive_name(item: &Item, drives: &[Drive]) -> String {
    item.path.as_deref().and_then(|p| rare_library::storage::drive_of(p, drives)).map(|d| d.label.clone()).unwrap_or_default()
}

impl StorageSection {
    pub fn build(cx: &mut Cx, nav: &mut Nav, parent: Id, cfg: &Config, now: f64) -> StorageSection {
        match super::live::get() {
            Some(s) => {
                let src: Vec<(&str, u32)> = s.sources.iter().map(|(k, n)| (k.as_str(), *n)).collect();
                fst::want_fresh();
                StorageSection::build_with(cx, nav, parent, cfg, &src, &fst::snapshot(), now)
            }
            None if super::live::enabled() => {
                fst::want_fresh();
                StorageSection::build_with(cx, nav, parent, cfg, &[], &fst::snapshot(), now)
            }
            None => {
                let sources = [("steam", 42u32), ("epic", 3), ("gog", 1), ("itch", 2), ("local", 5)];
                StorageSection::build_with(cx, nav, parent, cfg, &sources, &fixture_variant(), now)
            }
        }
    }

    pub fn build_with(cx: &mut Cx, nav: &mut Nav, parent: Id, cfg: &Config, sources: &[(&str, u32)], snap: &Snap, _now: f64) -> StorageSection {
        let r = cx.r();
        let job = snap.job.clone();
        let busy = job.is_some();
        // Drives.
        let p = panel(cx, parent);
        let mut first = true;
        if !snap.read {
            drive_row(cx, p, "This device", "Reading the drives", None, true);
            first = false;
        }
        for d in snap.drives.iter().filter(|d| d.kind == DriveKind::Internal) {
            drive_row(cx, p, &d.label, &free_line(d), used_frac(d), first);
            first = false;
        }
        if snap.read {
            let sd_drive = snap.drives.iter().find(|d| d.kind == DriveKind::Sd);
            let job_line = match &job {
                Some(Job::Format(st)) => Some(format_line(*st).to_string()),
                Some(Job::SetUp) => Some("Adding it to Steam and Rare".to_string()),
                _ => None,
            };
            match (&snap.sd, sd_drive) {
                (SdCard::Ready { .. }, Some(d)) => {
                    let body = job_line.unwrap_or_else(|| if snap.sd_set_up { free_line(d) } else { format!("{}. Not set up for games yet.", free_line(d)) });
                    let right = drive_row(cx, p, "SD card", &body, used_frac(d), first);
                    if !snap.sd_set_up {
                        nav.add(button(cx, right, "Use for games", if busy { Variant::Dimmed } else { Variant::Secondary }, Sz::Md, false).sid("sd-setup").tag(T_SETUP).hint("Add to Steam and Rare").group(10));
                    }
                    nav.add(button(cx, right, "Format", if busy { Variant::Dimmed } else { Variant::Secondary }, Sz::Md, false).sid("sd-format").tag(T_FORMAT).hint("Erase and format").group(10));
                }
                (SdCard::NotReady { .. }, _) | (SdCard::Ready { .. }, None) => {
                    let body = job_line.unwrap_or_else(|| "This card can't hold games yet. Format it to use it.".into());
                    let right = drive_row(cx, p, "SD card", &body, None, first);
                    nav.add(button(cx, right, "Format", if busy { Variant::Dimmed } else { Variant::Secondary }, Sz::Md, false).sid("sd-format").tag(T_FORMAT).hint("Erase and format").group(10));
                }
                (SdCard::None, _) => {
                    drive_row(cx, p, "SD card", "No card in the slot. Put one in to get more space for games.", None, first);
                }
            }
            first = false;
            for d in snap.drives.iter().filter(|d| d.kind == DriveKind::Usb) {
                drive_row(cx, p, &d.label, &free_line(d), used_frac(d), first);
            }
        }
        // Games and apps by size.
        let sort = SORT.with(|c| c.get()) % SORTS.len();
        let all = ALL.with(|c| c.get());
        let p = panel(cx, parent);
        let head_body = match (&snap.items, snap.measuring) {
            (None, _) => "Measuring".to_string(),
            (Some(_), true) => format!("{}. Measuring again", SORTS[sort].1),
            (Some(_), false) => SORTS[sort].1.to_string(),
        };
        let row_ = setting_row(cx, p, "Games and apps", Some(&head_body), true);
        nav.add(button(cx, row_.right, &format!("Sort: {}", SORTS[sort].0), Variant::Secondary, Sz::Md, false).sid("storage-sort").tag(T_SORT).hint("Sort").group(10));
        nav.add(button(cx, row_.right, "Measure again", if snap.measuring { Variant::Dimmed } else { Variant::Secondary }, Sz::Md, false).sid("storage-measure").tag(T_MEASURE).group(10));
        let mut items: Vec<Item> = snap.items.clone().unwrap_or_default();
        match sort {
            1 => items.sort_by_key(|i| i.title.to_lowercase()),
            2 => items.sort_by(|a, b| drive_name(a, &snap.drives).cmp(&drive_name(b, &snap.drives)).then(b.bytes.cmp(&a.bytes))),
            _ => items.sort_by(|a, b| b.bytes.cmp(&a.bytes)),
        }
        let moving_key = match &job {
            Some(Job::Move { key, .. }) => Some(key.clone()),
            Some(Job::ClosingSteam { key }) => key.clone(),
            _ => None,
        };
        let total = items.len();
        let shown = if all { total } else { total.min(FEW) };
        let mut rows = Vec::new();
        for (i, it) in items.iter().take(shown).enumerate() {
            let moving = moving_key.as_deref() == Some(it.key.as_str());
            let target = fst::move_target(it, &snap.drives);
            let here = drive_name(it, &snap.drives);
            let row = cx.scene.add(p, s().row().items_center().gap(1.0 * r).min_h(4.0 * r).wp(1.0).px(1.4 * r).py(0.7 * r).done(), Node::default());
            edge(cx, row, 0, tok::HAIR);
            let col = cx.scene.add(row, s().col().grow().done(), Node::default());
            cx.scene.add_text(cx.t, col, s().done(), &it.title, ts(r, ty::BODY.0, ty::BODY.1, 600).truncate(), tok::FG);
            let sub = if here.is_empty() { it.source.clone() } else { format!("{}, {}", it.source, here) };
            cx.scene.add_text(cx.t, col, s().done(), &sub, ts(r, ty::SMALL.0, ty::SMALL.1, 400).truncate(), tok::MUTED);
            let size_line = match (&job, moving) {
                (Some(Job::ClosingSteam { .. }), true) => "Closing Steam".to_string(),
                (Some(Job::Move { phase: Phase::Copying, done, total, .. }), true) => format!("Copying {}%", pct(*done, *total)),
                (Some(Job::Move { phase: Phase::Checking, done, total, .. }), true) => format!("Checking {}%", pct(*done, *total)),
                (Some(Job::Move { phase: Phase::Finishing, .. }), true) => "Finishing".to_string(),
                _ => size_text(it.bytes),
            };
            let cell = cx.scene.add(row, s().w(7.0 * r).no_shrink_().done(), Node::default());
            cx.scene.add_text(cx.t, cell, s().wp(1.0).done(), &size_line, ts(r, ty::SMALL.0, ty::SMALL.1, 500).align(rare_render::text::TextAlign::Right), if moving { tok::FG } else { tok::FG.alpha(0.85) });
            // Move.
            let (label, no_move) = if moving {
                ("Cancel".to_string(), None)
            } else if let Some(why) = &it.no_move {
                ("Move".to_string(), Some(why.clone()))
            } else if let Some(t) = target {
                (format!("Move to {}", if t.kind == DriveKind::Internal { "this device".to_string() } else { t.label.clone() }), None)
            } else {
                ("Move".to_string(), Some("Put an SD card in to move games to it.".to_string()))
            };
            let v = if moving { Variant::Secondary } else if no_move.is_some() || busy { Variant::Dimmed } else { Variant::Secondary };
            let bx = cx.scene.add(row, s().w(11.5 * r).no_shrink_().done(), Node::default());
            nav.add(button(cx, bx, &label, v, Sz::Sm, true).sid(&format!("{}:move", it.key)).tag(ITEMS + 4 * i as u32).hint(if moving { "Cancel the move" } else { "Move" }).group(10));
            // Uninstall (Clear for a cache).
            let (ulabel, can) = match it.kind {
                ItemKind::Cache => ("Clear", true),
                ItemKind::Prefix if it.key.starts_with("orphan:") => ("Delete", true),
                _ => ("Uninstall", it.game_id.is_some()),
            };
            let bx = cx.scene.add(row, s().w(7.0 * r).no_shrink_().done(), Node::default());
            nav.add(button(cx, bx, ulabel, if can && !moving { Variant::Secondary } else { Variant::Dimmed }, Sz::Sm, true).sid(&format!("{}:uninstall", it.key)).tag(ITEMS + 4 * i as u32 + 1).hint(ulabel).group(10));
            rows.push(RowRef { key: it.key.clone(), game_id: it.game_id.clone(), kind: it.kind, no_move, moving });
        }
        if total == 0 && snap.items.is_some() {
            setting_row(cx, p, "Nothing installed yet", Some("Games and apps you install show up here with their size."), false);
        }
        if total > FEW {
            let row_ = setting_row(cx, p, if all { "Showing everything" } else { "Showing the biggest" }, Some(&format!("{shown} of {total}")), false);
            nav.add(button(cx, row_.right, if all { "Show fewer" } else { "Show all" }, Variant::Secondary, Sz::Md, false).sid("storage-all").tag(T_ALL).group(10));
        }
        // Sources and the library settings.
        let p0 = cx.scene.add(parent, s().col().wp(1.0).px(1.4 * r).py(1.2 * r).done(), Node::bg(tok::L1).radius(1.0 * r));
        let h = cx.scene.add(p0, s().mb(0.9 * r).done(), Node::default());
        text(cx, h, ty::BODY, 600, tok::FG, "Sources");
        let labels: Vec<(String, String)> = sources.iter().map(|(k, n)| (source_label(k), n.to_string())).collect();
        let items: Vec<(&str, &str)> = labels.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        if !items.is_empty() {
            stat_line_sized(cx, p0, &items, true);
        }
        let p = panel(cx, parent);
        let row_ = setting_row(cx, p, "Scan for games", Some("Steam, Epic, GOG, itch.io and your Games folders."), true);
        nav.add(button(cx, row_.right, "Scan again", Variant::Secondary, Sz::Md, false).sid("scan").tag(1).group(10));
        let row_ = setting_row(cx, p, "Games folder", Some("A folder with one game per subfolder, on this device or an SD card."), false);
        nav.add(button(cx, row_.right, "Add folder", Variant::Secondary, Sz::Md, false).sid("games-folder").tag(2).group(10));
        for (i, f) in cfg.game_folders.iter().enumerate() {
            let row_ = setting_row(cx, p, &pretty_path(f), Some("Scanned for games."), false);
            nav.add(button(cx, row_.right, "Remove", Variant::Secondary, Sz::Md, false).tag(100 + i as u32).group(10));
        }
        let idx = steam_start_idx(cfg);
        let row_ = setting_row(cx, p, "When Steam starts", Some(STEAM_START[idx].2), false);
        let b = button(cx, row_.right, STEAM_START[idx].1, Variant::Secondary, Sz::Md, false).sid("steam-start").tag(3);
        let steam_label = match b.kind {
            Kind::Btn { text, .. } => text,
            _ => unreachable!(),
        };
        nav.add(b.group(10));
        let steam_body = row_.body.unwrap();
        if let Some(text) = cloud_summary() {
            setting_row(cx, p, "Steam Cloud", Some(&text), false);
        }
        let has_key = cfg.steamgriddb_key.as_deref().is_some_and(|k| !k.is_empty());
        let row_ = setting_row(cx, p, "SteamGridDB cover art", Some(if has_key { "API key added" } else { "Optional. A free API key adds covers for games outside Steam." }), false);
        let b = button(cx, row_.right, if has_key { "Change key" } else { "Add key" }, Variant::Secondary, Sz::Md, false).sid("steamgriddb").tag(4);
        let key_btn_label = match b.kind {
            Kind::Btn { text, .. } => text,
            _ => unreachable!(),
        };
        nav.add(b.group(10));
        let key_body = row_.body.unwrap();
        let row_ = setting_row(cx, p, "Screenshots and clips", Some("Take a screenshot or keep the last 30 seconds, and share them as a link on rare.market."), false);
        nav.add(button(cx, row_.right, "Open Share", Variant::Secondary, Sz::Md, false).sid("share").tag(5).group(10));
        StorageSection { folders: cfg.game_folders.clone(), steam_label, steam_body, key_btn_label, key_body, rows, busy }
    }
}

fn pct(done: u64, total: u64) -> u64 {
    if total == 0 {
        0
    } else {
        (done.saturating_mul(100) / total).min(100)
    }
}

impl SectionView for StorageSection {
    fn press(&mut self, cx: &mut Cx, nav: &mut Nav, cfg: &mut Config, _now: f64) -> SectionOut {
        match nav.current().map(|f| f.tag) {
            Some(1) => SectionOut::Act("storage:scan".into()),
            Some(2) => SectionOut::Act("storage:add-folder".into()),
            Some(T_FORMAT) if self.busy => SectionOut::Act("storage:say:Wait for the SD card to finish.".into()),
            Some(T_FORMAT) => SectionOut::Act("storage:format-ask".into()),
            Some(T_SETUP) if self.busy => SectionOut::Act("storage:say:Wait for the SD card to finish.".into()),
            Some(T_SETUP) => SectionOut::Act("storage:setup".into()),
            Some(T_MEASURE) => SectionOut::Act("storage:measure".into()),
            Some(T_SORT) => {
                SORT.with(|c| c.set((c.get() + 1) % SORTS.len()));
                SectionOut::Act("storage:redraw".into())
            }
            Some(T_ALL) => {
                ALL.with(|c| c.set(!c.get()));
                SectionOut::Act("storage:redraw".into())
            }
            Some(t) if t >= ITEMS => {
                let Some(row) = self.rows.get(((t - ITEMS) / 4) as usize) else { return SectionOut::None };
                if (t - ITEMS) % 4 == 0 {
                    if row.moving {
                        return SectionOut::Act("storage:cancel".into());
                    }
                    if let Some(why) = &row.no_move {
                        return SectionOut::Act(format!("storage:say:{why}"));
                    }
                    if self.busy {
                        return SectionOut::Act("storage:say:Wait for the current move to finish.".into());
                    }
                    SectionOut::Act(format!("storage:move:{}", row.key))
                } else {
                    if row.moving {
                        return SectionOut::Act("storage:say:Wait for the move to finish.".into());
                    }
                    if row.key == "android:system" {
                        return SectionOut::Act("storage:say:Android's system and data. Uninstall Android apps one by one to free space.".into());
                    }
                    if let Some(p) = row.key.strip_prefix("orphan:") {
                        return SectionOut::Act(format!("storage:delete-ask:{p}"));
                    }
                    match (row.kind, &row.game_id) {
                        (ItemKind::Cache, _) => SectionOut::Act(format!("storage:clear:{}", row.key.trim_start_matches("cache:"))),
                        (_, Some(id)) => SectionOut::Act(format!("storage:uninstall:{id}")),
                        (ItemKind::Prefix, None) => SectionOut::Act("storage:say:Saves live here. They go when you uninstall the game.".into()),
                        (ItemKind::Roms, None) => SectionOut::Act("storage:say:Delete ROMs from the game page of a recent game, or in Desktop mode.".into()),
                        _ => SectionOut::Act("storage:say:Uninstall this in Desktop mode.".into()),
                    }
                }
            }
            Some(t) if (100..1000).contains(&t) => {
                let f = self.folders.get((t - 100) as usize).cloned();
                match f {
                    Some(f) => {
                        cfg.game_folders.retain(|x| *x != f);
                        SectionOut::Changed
                    }
                    None => SectionOut::None,
                }
            }
            Some(3) => {
                let next = (steam_start_idx(cfg) + 1) % STEAM_START.len();
                cfg.steam_start = Some(STEAM_START[next].0.into());
                cx.scene.set_text(cx.t, self.steam_label, STEAM_START[next].1);
                cx.scene.set_text(cx.t, self.steam_body, STEAM_START[next].2);
                SectionOut::Changed
            }
            Some(4) => {
                let _ = (self.key_btn_label, self.key_body);
                SectionOut::Act("storage:grid-key".into())
            }
            Some(5) => SectionOut::Act("open:share".into()),
            _ => SectionOut::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_folders_become_tilde() {
        assert_eq!(pretty_path("/home/deck/Games"), "~/Games");
        assert_eq!(pretty_path("/var/home/rare/Games/x"), "~/Games/x");
        assert_eq!(pretty_path("/run/media/sd/Games"), "/run/media/sd/Games");
        assert_eq!(pretty_path("/home/a/b/home/c/d"), "~/b~/d");
    }

    #[test]
    fn cloud_row_text() {
        use rare_library::steam_cloud::Summary;
        assert_eq!(cloud_text(&Summary { synced: 4, out_of_date: 1, conflict: 2, off: false }), "4 games synced, 1 not synced yet, 2 with a conflict");
        assert_eq!(cloud_text(&Summary { synced: 1, ..Default::default() }), "1 game synced");
        assert_eq!(cloud_text(&Summary { off: true, synced: 3, ..Default::default() }), "Off");
    }

    #[test]
    fn source_labels() {
        assert_eq!(source_label("gog"), "GOG");
        assert_eq!(source_label("steam"), "Steam");
        assert_eq!(source_label("itch"), "Itch");
    }

    #[test]
    fn sizes_read_plainly() {
        assert_eq!(size_text(12_100_000_000), "12.1 GB");
        assert_eq!(size_text(640_000_000), "640 MB");
        assert_eq!(pct(5, 10), 50);
        assert_eq!(pct(5, 0), 0);
    }

    #[test]
    fn steam_start_cycles_in_order() {
        let c = Config::default();
        assert_eq!(steam_start_idx(&c), 0);
        let c = Config { steam_start: Some("demand".into()), ..Default::default() };
        assert_eq!(steam_start_idx(&c), 2);
    }
}
