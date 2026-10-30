//! Parser and scan tests against copies of the SuiPlay0X1 test device's real files
//! (tests/fixtures/suiplay, see its README), plus Heroic/legendary data in Heroic's formats.

use super::*;
use crate::model::{Game, LaunchSpec, Source};
use std::path::{Path, PathBuf};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/suiplay")
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let dst = to.join(e.file_name());
        if e.path().is_dir() {
            copy_tree(&e.path(), &dst);
        } else {
            std::fs::copy(e.path(), &dst).unwrap();
        }
    }
}

/// The fixture home with the ROM placeholders the repo cannot hold (.gitignore drops *.iso, *.chd,
/// *.rvz and friends): 11 GameCube games, 10 PS2 games and one Xbox game folder, as on the device.
fn home_with_roms() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    copy_tree(&fixture(), t.path());
    let roms = t.path().join("Emulation/roms");
    for i in 1..=11 {
        write(&roms.join(format!("gc/GameCube Game {i}.rvz")), "");
    }
    for i in 1..=10 {
        write(&roms.join(format!("ps2/PS2 Game {i}.chd")), "");
    }
    write(&roms.join("xbox/Jet Set Radio Future (USA)/default.xbe"), "");
    t
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(fixture().join(rel)).unwrap()
}

#[test]
fn device_manifests_and_library_folders() {
    let lf = vdf::parse(&read(".local/share/Steam/steamapps/libraryfolders.vdf"));
    let f0 = lf.get("libraryfolders").unwrap().get("0").unwrap();
    assert_eq!(f0.str("path"), Some("/var/home/bazzite/.local/share/Steam"));
    assert_eq!(f0.get("apps").unwrap().entries().len(), 12);

    let hades = vdf::parse(&read(".local/share/Steam/steamapps/appmanifest_1145350.acf"));
    let st = hades.get("AppState").unwrap();
    assert_eq!(st.str("name"), Some("Hades II"));
    assert_eq!(st.str("StateFlags"), Some("4"));
    let rt = vdf::parse(&read(".local/share/Steam/steamapps/appmanifest_1628350.acf"));
    assert_eq!(rt.get("AppState").unwrap().str("name"), Some("Steam Linux Runtime 3.0 (sniper)"));

    let lc = vdf::parse(&read(".local/share/Steam/userdata/1888050635/config/localconfig.vdf"));
    let apps = lc.get("UserLocalConfigStore").and_then(|x| x.get("Software")).and_then(|x| x.get("Valve")).and_then(|x| x.get("Steam")).and_then(|x| x.get("apps")).unwrap();
    assert!(apps.get("1145350").unwrap().str("LastPlayed").is_some());
}

#[test]
fn device_shortcuts() {
    let bytes = std::fs::read(fixture().join(".local/share/Steam/userdata/1888050635/config/shortcuts.vdf")).unwrap();
    let v = shortcuts::parse(&bytes);
    let list = v.get("shortcuts").unwrap().entries();
    let names: Vec<&str> = list.iter().filter_map(|(_, s)| s.str("AppName")).collect();
    assert_eq!(names, vec!["Google Chrome", "xemu", "EmulationStationDE", "Citron-0.12.25-anylinux-x86_64_v3.AppImage", "Rare"]);
    for (_, s) in list {
        assert!(s.int("appid").is_some(), "every shortcut has an appid");
    }
    // Rare's own shortcut is never listed; everything else on the device is an emulator or an app.
    assert!(shortcuts::is_launcher("Rare", "/home/bazzite/Applications/Rare-Launcher-x86_64.AppImage"));
    for (title, exe) in [("Google Chrome", "/usr/bin/flatpak"), ("xemu", "/usr/bin/flatpak"), ("ES-DE", "/home/bazzite/Emulation/tools/launchers/es-de/es-de.sh"), ("Citron", "/var/home/bazzite/Desktop/Citron-0.12.25-anylinux-x86_64_v3.AppImage")] {
        assert!(shortcuts::is_app(title, exe), "{title} is an app");
    }
    assert!(!shortcuts::is_app("EVE Frontier", "/home/b/Games/EVE Frontier/launcher.exe"));
    assert!(!shortcuts::is_app("Celeste", "/home/b/Games/Celeste/Celeste"));
}

#[test]
fn device_desktop_entries() {
    let dir = fixture().join(".local/share/applications");
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let kv = apps::parse_entry(&std::fs::read_to_string(e.path()).unwrap());
        assert!(kv.contains_key("Name"), "{:?} parses", e.path());
    }
}

#[test]
fn device_esde() {
    let gl = esde::parse_gamelist(&read("ES-DE/gamelists/xbox/gamelist.xml"), "xbox");
    assert_eq!(gl.len(), 3);
    assert_eq!(gl[0].name, "Jet Set Radio Future (USA)");
    assert_eq!(gl[0].playtime, 104);
    assert!(esde::parse_gamelist(&read("ES-DE/gamelists/gc/gamelist.xml"), "gc").is_empty());
    let custom = esde::custom_names(&read("ES-DE/custom_systems/es_systems.xml"));
    assert_eq!(custom.get("model2").map(String::as_str), Some("Sega Model 2"));

    let home = home_with_roms();
    let e = esde::scan_in(home.path()).expect("ES-DE is set up on the device");
    // The device's settings point at /home/bazzite/Emulation/roms; remapped into this home.
    assert!(e.rom_dir.ends_with("Emulation/roms"));
    let count = |id: &str| e.systems.iter().find(|s| s.id == id).map(|s| s.count);
    assert_eq!(count("gc"), Some(11));
    assert_eq!(count("ps2"), Some(10));
    assert_eq!(count("switch"), Some(1));
    assert_eq!(count("xbox"), Some(1), "a game folder counts once");
    assert_eq!(count("model2"), Some(1));
    assert_eq!(count("emulators"), None, "EmuDeck's launcher folder is not a system");
    assert_eq!(count("psx"), None, "empty systems are hidden");
    assert_eq!(e.systems.iter().find(|s| s.id == "gc").unwrap().name, "GameCube");
    // Played systems first.
    assert!(e.systems[0].last_played.is_some());
    let names: Vec<&str> = e.recent.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names[0], "Pokemon Violet [01008F6008C5E000][v0][US]");
    assert_eq!(names.iter().filter(|n| n.starts_with("Jet Set")).count(), 1, "copies of one game merge");
}

/// The whole scan over the device's home: right games, right art, nothing twice, no runtimes.
#[test]
fn device_full_scan() {
    let home = home_with_roms();
    std::env::set_var("HOME", home.path());
    let lib = scan(&[], &[], &[], vec![]);
    let ids: Vec<&str> = lib.games.iter().map(|g| g.id.as_str()).collect();
    let unique: std::collections::HashSet<&&str> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "no duplicate entries");

    let steam: Vec<&Game> = lib.games.iter().filter(|g| g.source == Source::Steam).collect();
    let mut steam_ids: Vec<&str> = steam.iter().map(|g| g.id.as_str()).collect();
    steam_ids.sort();
    assert_eq!(steam_ids, vec!["steam:10180", "steam:10190", "steam:1145350", "steam:1449850"], "games only, no Proton or runtimes");
    let hades = steam.iter().find(|g| g.id == "steam:1145350").unwrap();
    assert!(hades.art.cover[0].ends_with("2ba105370a00877459144db0dd68af5f2d338429/library_capsule.jpg"), "{:?}", hades.art.cover);
    assert!(hades.art.hero[0].ends_with("library_hero.jpg"));
    assert!(hades.art.logo[0].ends_with("4c811a41d2b3c87481cd5a265b45c7f347b6118e/logo.png"));
    assert!(hades.art.icon[0].ends_with("621d9f1cfa204c0bae07a981f41007d2cf03a56c.jpg"));
    assert!(hades.last_played.is_some() && hades.playtime_minutes.is_some(), "playtime from localconfig.vdf");
    assert!(matches!(hades.launch, LaunchSpec::Steam { appid: 1145350 }));
    let mw2 = steam.iter().find(|g| g.id == "steam:10180").unwrap();
    assert!(mw2.art.cover[0].ends_with("10180/library_600x900.jpg"));

    let apps: Vec<&Game> = lib.games.iter().filter(|g| g.source == Source::Apps).collect();
    let titles: Vec<&str> = apps.iter().map(|g| g.title.as_str()).collect();
    for want in ["ES-DE", "xemu", "Citron", "Google Chrome", "RPCS3", "Cemu", "DuckStation", "PCSX2"] {
        assert_eq!(titles.iter().filter(|t| **t == want).count(), 1, "{want} once in {titles:?}");
    }
    for never in ["Hades II", "Rare", "Game 424004941485572097"] {
        assert!(!titles.contains(&never), "{never} must not be an app");
    }
    // ES-DE's systems hang off its entry.
    let esde_id = lib.emulation_app.as_deref().expect("ES-DE entry");
    assert_eq!(apps.iter().find(|a| a.id == esde_id).unwrap().title, "ES-DE");
    assert!(lib.emulation.as_ref().unwrap().systems.len() >= 5);
    // The built-in web game.
    assert!(lib.games.iter().any(|g| g.id == "web:act"));
}

fn write(p: &Path, s: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}

#[test]
fn heroic_epic_gog_sideload() {
    let d = std::env::temp_dir().join(format!("rare-heroic-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let cfg = d.join(".config");
    let gog_dir = d.join("Games/Heroic/Gwent");
    write(
        &cfg.join("heroic/legendaryConfig/legendary/installed.json"),
        &format!(r#"{{"Fortnite":{{"app_name":"Fortnite","title":"Fortnite","install_path":"{0}/Games/Heroic/Fortnite","install_size":30000000000,"is_dlc":false,"platform":"Windows"}},"DLC1":{{"app_name":"DLC1","title":"Some DLC","is_dlc":true}}}}"#, d.display()),
    );
    write(
        &cfg.join("heroic/legendaryConfig/legendary/metadata/Fortnite.json"),
        r#"{"app_name":"Fortnite","metadata":{"keyImages":[{"type":"DieselGameBoxTall","url":"https://cdn/tall.jpg"},{"type":"DieselGameBox","url":"https://cdn/wide.jpg"}]}}"#,
    );
    write(&cfg.join("heroic/gog_store/installed.json"), &format!(r#"{{"installed":[{{"appName":"1207658924","install_path":"{}","platform":"windows","version":"1"}}]}}"#, gog_dir.display()));
    write(&cfg.join("heroic/store_cache/gog_library.json"), r#"{"games":[{"app_name":"1207658924","title":"GWENT","art_square":"https://gog/sq.jpg","art_cover":"https://gog/cover.jpg"}]}"#);
    write(&gog_dir.join("goggame-1207658924.info"), r#"{"playTasks":[{"isPrimary":true,"path":"bin\\Gwent.exe","workingDir":"bin","arguments":"-x 1"}]}"#);
    write(
        &cfg.join("heroic/sideload_apps/library.json"),
        r#"{"games":[{"app_name":"eve123","title":"EVE Frontier","is_installed":true,"install":{"executable":"/home/b/Games/EVE Frontier/launcher.exe","platform":"windows"},"art_square":"https://x/eve.jpg"}]}"#,
    );
    write(&cfg.join("heroic/store/timestamp.json"), r#"{"Fortnite":{"firstPlayed":"2026-01-01T10:00:00.000Z","lastPlayed":"2026-10-01T18:22:05.123Z","totalPlayed":95}}"#);

    let configs = vec![cfg.clone()];
    // With Heroic installed, Heroic's own installs launch through Heroic.
    let epic = heroic::scan_epic_in(&configs, false, true);
    assert_eq!(epic.games.len(), 1, "DLC is skipped");
    let fort = &epic.games[0];
    assert_eq!(fort.id, "epic:Fortnite");
    assert_eq!(fort.art.cover, vec!["https://cdn/tall.jpg"]);
    assert_eq!(fort.playtime_minutes, Some(95));
    assert_eq!(fort.last_played, esde::parse_time("20261001T182205"));
    assert!(matches!(&fort.launch, LaunchSpec::Heroic { runner, app_name, .. } if runner == "legendary" && app_name == "Fortnite"));
    // Without Heroic, legendary.
    assert!(matches!(heroic::scan_epic_in(&configs, true, false).games[0].launch, LaunchSpec::Legendary { .. }));

    let gog = heroic::scan_gog_in(&configs, false, true);
    let gwent = gog.games.iter().find(|g| g.source == Source::Gog).unwrap();
    assert_eq!(gwent.title, "GWENT");
    match &gwent.launch {
        LaunchSpec::Exec { program, args, cwd, windows } => {
            assert!(program.ends_with("Gwent/bin/Gwent.exe"));
            assert_eq!(args, &vec!["-x".to_string(), "1".to_string()]);
            assert!(cwd.as_deref().unwrap().ends_with("Gwent/bin"));
            assert!(*windows);
        }
        other => panic!("unexpected {other:?}"),
    }
    // Rare starts GOG games itself even with Heroic installed (umu-run in the game's prefix).
    assert!(matches!(heroic::scan_gog_in(&configs, true, true).games.iter().find(|g| g.source == Source::Gog).unwrap().launch, LaunchSpec::Exec { windows: true, .. }));
    let eve = gog.games.iter().find(|g| g.id == "heroic:eve123").unwrap();
    assert_eq!(eve.title, "EVE Frontier");
    assert!(matches!(&eve.launch, LaunchSpec::Exec { windows: true, .. }));
    let _ = std::fs::remove_dir_all(&d);
}
