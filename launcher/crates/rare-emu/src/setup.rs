//! One-press emulation setup (Library, Emulation): installs the emulators (Flatpaks from Flathub
//! into the user installation, DuckStation as its official AppImage), RetroArch's cores, the ROM
//! and BIOS folders on the device and the SD card, first-run settings for the handheld, and links
//! every emulator's saves under ~/Emulation/saves so Rare's cloud saves carry them.
//!
//! Nothing the player already has is replaced: installed emulators are skipped, existing config
//! files are left alone, existing save folders are never moved. Progress goes out as `emu-setup`
//! events: `{ step, id, name, progress, done, error }`.

use crate::catalog::{self, Emulator};
use crate::defaults::Env;
use crate::retroarch;
use crate::systems::{self, System};
use rare_core::{Ctx, Event};
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BUILDBOT: &str = "https://buildbot.libretro.com/nightly/linux/x86_64/latest";

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EmuStatus {
    pub id: String,
    pub name: String,
    pub plays: String,
    /// "flatpak", "appimage", or None when not installed.
    pub installed: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub label: String,
    pub roms: String,
    pub bios: String,
    pub exists: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub flatpak: bool,
    pub emulators: Vec<EmuStatus>,
    pub cores: usize,
    pub cores_missing: usize,
    pub folders: Vec<Folder>,
    /// Everything is installed and the folders exist.
    pub ready: bool,
}

fn which(p: &str) -> bool {
    rare_library::steam::which(p)
}

pub fn status_in(home: &Path, roots: &[crate::roms::Root], flatpak: bool) -> Status {
    let emulators: Vec<EmuStatus> = catalog::EMULATORS
        .iter()
        .map(|e| EmuStatus {
            id: e.id.into(),
            name: e.name.into(),
            plays: e.plays.into(),
            installed: catalog::installed(e, home).map(|i| match i {
                catalog::Install::Flatpak(_) => "flatpak".into(),
                catalog::Install::AppImage(_) => "appimage".into(),
            }),
        })
        .collect();
    let p = retroarch::paths(home);
    let cores = systems::cores();
    let cores_missing = cores.iter().filter(|c| !retroarch::core_path(&p, c).is_file()).count();
    let folders: Vec<Folder> = roots.iter().map(|r| Folder { label: r.label().into(), roms: r.roms().to_string_lossy().into_owned(), bios: r.base.join("bios").to_string_lossy().into_owned(), exists: r.roms().is_dir() }).collect();
    let ready = emulators.iter().all(|e| e.installed.is_some()) && cores_missing == 0 && folders.first().is_some_and(|f| f.exists);
    Status { flatpak, emulators, cores: cores.len(), cores_missing, folders, ready }
}

pub fn status() -> Status {
    let h = crate::roms::home();
    status_in(&h, &crate::roms::roots(&h), which("flatpak"))
}

/// One line of the "which emulators" choice (first run and Library, Emulation).
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub id: String,
    pub name: String,
    /// The systems it plays, the line's title.
    pub plays: String,
    pub size_mb: u32,
    pub installed: bool,
    /// The player's games it starts.
    pub roms: usize,
    /// Ticked to begin with: not installed, and the player has games for it.
    pub picked: bool,
}

/// The choice from what is installed and the games found.
pub fn choices_in(home: &Path, roms: &[crate::roms::Rom]) -> Vec<Choice> {
    catalog::EMULATORS
        .iter()
        .map(|e| {
            let n = roms.iter().filter(|r| crate::roms::emulator_of(r.system).id == e.id).count();
            let installed = catalog::installed(e, home).is_some();
            Choice { id: e.id.into(), name: e.name.into(), plays: e.plays.into(), size_mb: e.size_mb, installed, roms: n, picked: !installed && n > 0 }
        })
        .collect()
}

pub fn choices() -> Vec<Choice> {
    let h = crate::roms::home();
    choices_in(&h, &crate::roms::scan_in(&h, &crate::roms::roots(&h)))
}

/// "About 1.2 GB" for the picked emulators, with the shared runtimes when nothing is installed yet.
pub fn size_text(choices: &[Choice], picked: &[String]) -> String {
    let mb: u32 = choices.iter().filter(|c| !c.installed && picked.contains(&c.id)).map(|c| c.size_mb).sum();
    if mb == 0 {
        return String::new();
    }
    let first = !choices.iter().any(|c| c.installed && c.id != "retroarch");
    let total = mb + if first { catalog::SHARED_MB } else { 0 };
    let gb = |m: u32| if m >= 1000 { format!("{:.1} GB", m as f32 / 1000.0) } else { format!("{m} MB") };
    if first {
        format!("About {} to download, with {} of parts they share", gb(total), gb(catalog::SHARED_MB))
    } else {
        format!("About {} to download", gb(total))
    }
}

fn emit(ctx: &Ctx, step: &str, id: &str, name: &str, progress: Option<f32>, done: bool, error: Option<String>) {
    ctx.send(Event::Other { name: "emu-setup".into(), payload: serde_json::json!({ "step": step, "id": id, "name": name, "progress": progress, "done": done, "error": error }) });
}

fn flatpak_cmd(args: &[&str]) -> Command {
    let mut c = if which("script") {
        let mut c = Command::new("script");
        c.args(["-qfec", &format!("flatpak {}", args.join(" ")), "/dev/null"]);
        c
    } else {
        let mut c = Command::new("flatpak");
        c.args(args);
        c
    };
    rare_launch::launch::host_env(&mut c);
    c.stdin(Stdio::null());
    c
}

fn run_quiet(args: &[&str]) -> Result<(), String> {
    let mut c = Command::new("flatpak");
    rare_launch::launch::host_env(&mut c);
    let out = c.args(args).stdin(Stdio::null()).output().map_err(|e| format!("Could not run flatpak: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).lines().map(str::trim).filter(|l| !l.is_empty()).last().unwrap_or("flatpak failed").trim_start_matches("error: ").to_string())
    }
}

/// `flatpak install --user flathub <id>`, with its progress bar read as it draws.
fn install_flatpak(ctx: &Ctx, e: &Emulator, app: &str) -> Result<(), String> {
    let mut child = flatpak_cmd(&["install", "--user", "-y", "--noninteractive", "flathub", app]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|x| format!("Could not run flatpak: {x}"))?;
    let mut err = child.stderr.take();
    let et = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = err.as_mut() {
            let _ = e.read_to_string(&mut s);
        }
        s
    });
    let mut p = rare_launch::apps::install::Progress::default();
    let mut last = -1.0f32;
    if let Some(mut out) = child.stdout.take() {
        let mut buf = [0u8; 4096];
        while let Ok(n) = out.read(&mut buf) {
            if n == 0 {
                break;
            }
            if let Some(v) = p.feed(&String::from_utf8_lossy(&buf[..n])) {
                if (v * 100.0).floor() != (last * 100.0).floor() {
                    last = v;
                    emit(ctx, "install", e.id, e.name, Some(v), false, None);
                }
            }
        }
    }
    let st = child.wait().map_err(|x| x.to_string())?;
    let stderr = et.join().unwrap_or_default();
    if st.success() {
        Ok(())
    } else {
        Err(rare_launch::apps::install::strip_ansi(&stderr).lines().map(str::trim).filter(|l| !l.is_empty()).last().map(|l| l.trim_start_matches("error: ").to_string()).unwrap_or_else(|| format!("{} didn't install", e.name)))
    }
}

/// Downloads one RetroArch core from libretro's buildbot.
pub fn install_core(cores: &Path, core: &str) -> Result<(), String> {
    let url = format!("{BUILDBOT}/{core}_libretro.so.zip");
    let r = ureq::AgentBuilder::new().timeout_connect(std::time::Duration::from_secs(15)).timeout_read(std::time::Duration::from_secs(60)).build().get(&url).set("User-Agent", "rare-launcher").call().map_err(|e| format!("{core}: {e}"))?;
    let mut zip = Vec::new();
    r.into_reader().take(256 << 20).read_to_end(&mut zip).map_err(|e| e.to_string())?;
    let so = crate::unzip::entry(&zip, "_libretro.so")?;
    if so.len() < 4 || so[..4] != [0x7f, b'E', b'L', b'F'] {
        return Err(format!("{core}: not a Linux core"));
    }
    std::fs::create_dir_all(cores).map_err(|e| e.to_string())?;
    let dest = cores.join(format!("{core}_libretro.so"));
    let tmp = dest.with_extension("so.rare-new");
    std::fs::write(&tmp, so).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())
}

/// The ROM folders (one per system), BIOS and saves on a root.
pub fn make_folders(root: &crate::roms::Root, full: bool) -> std::io::Result<()> {
    for s in systems::SYSTEMS {
        // EmuDeck's alias folders count: a "megadrive" folder means "genesis" is not made too.
        let have = std::iter::once(s.id).chain(s.aliases.iter().copied()).any(|n| root.roms().join(n).is_dir());
        if !have {
            std::fs::create_dir_all(root.roms().join(s.id))?;
        }
    }
    std::fs::create_dir_all(root.base.join("bios"))?;
    if full {
        // RetroArch falls back to its own folders when these aren't there at its first start.
        for d in ["saves/retroarch/saves", "saves/retroarch/states", "storage/retroarch/screenshots"] {
            std::fs::create_dir_all(root.base.join(d))?;
        }
    }
    Ok(())
}

/// ~/Emulation/saves/<emulator>/<kind> pointing at each emulator's own save folder, so cloud
/// saves find them in one place. Only made where nothing is yet.
pub fn save_links(home: &Path) -> Vec<String> {
    let v = |a: &str| home.join(".var/app").join(a);
    let links: Vec<(&str, PathBuf)> = vec![
        ("duckstation/saves", home.join(".local/share/duckstation/memcards")),
        ("duckstation/states", home.join(".local/share/duckstation/savestates")),
        ("pcsx2/saves", v("net.pcsx2.PCSX2").join("config/PCSX2/memcards")),
        ("pcsx2/states", v("net.pcsx2.PCSX2").join("config/PCSX2/sstates")),
        ("ppsspp/saves", v("org.ppsspp.PPSSPP").join("config/ppsspp/PSP/SAVEDATA")),
        ("ppsspp/states", v("org.ppsspp.PPSSPP").join("config/ppsspp/PSP/PPSSPP_STATE")),
        ("dolphin/GC", v("org.DolphinEmu.dolphin-emu").join("data/dolphin-emu/GC")),
        ("dolphin/Wii", v("org.DolphinEmu.dolphin-emu").join("data/dolphin-emu/Wii")),
        ("azahar/saves", v("org.azahar_emu.Azahar").join("data/azahar-emu/sdmc")),
        ("Cemu/saves", v("info.cemu.Cemu").join("data/Cemu/mlc01/usr/save")),
        ("rpcs3/saves", v("net.rpcs3.RPCS3").join("config/rpcs3/dev_hdd0/home/00000001/savedata")),
        ("ryujinx/saves", v("io.github.ryubing.Ryujinx").join("config/Ryujinx/bis/user/save")),
        ("melonds/saves", v("net.kuribo64.melonDS").join("config/melonDS/saves")),
    ];
    let mut made = Vec::new();
    for (rel, target) in links {
        let link = home.join("Emulation/saves").join(rel);
        if link.symlink_metadata().is_ok() {
            continue;
        }
        // The emulator's folder: the folder the link points at must exist for the link to count.
        if std::fs::create_dir_all(&target).is_err() {
            continue;
        }
        let _ = std::fs::create_dir_all(link.parent().unwrap_or(Path::new("/")));
        #[cfg(unix)]
        if std::os::unix::fs::symlink(&target, &link).is_ok() {
            made.push(rel.to_string());
        }
    }
    made
}

/// The handheld LCD shader for a handheld system, when the player has none: a content folder
/// preset (`config/<Core>/<rom folder>.slangp`, so the Game Boy and Game Boy Color folders get
/// their own though they share a core) referencing the shader the RetroArch Flatpak ships.
pub fn shader_preset(home: &Path, sys: &System) {
    let (Some(shader), systems::Runner::Core(core)) = (sys.shader, sys.runner) else { return };
    let Some(name) = systems::core_display_name(core) else { return };
    let dir = retroarch::config_dir(home).join("config").join(name);
    // The player's own core preset wins.
    if ["slangp", "glslp", "cgp"].iter().any(|e| dir.join(format!("{name}.{e}")).exists()) {
        return;
    }
    for folder in std::iter::once(sys.id).chain(sys.aliases.iter().copied()) {
        let f = dir.join(format!("{folder}.slangp"));
        if f.exists() || dir.join(format!("{folder}.glslp")).exists() {
            continue;
        }
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&f, format!("#reference \"/app/share/libretro/shaders/shaders_slang/{shader}\"\n"));
    }
}

fn bios_into_fresh(home: &Path, r: &crate::defaults::Report, file_end: &str, section: &str, key: &str) {
    let bios = home.join("Emulation/bios").to_string_lossy().into_owned();
    for f in r.written.iter().filter(|f| f.ends_with(file_end)) {
        if let Ok(t) = std::fs::read_to_string(f) {
            let _ = std::fs::write(f, crate::actions::upsert(&t, section, key, &bios, false));
        }
    }
}

/// First-run settings for the emulators that have none: the tuned writers for the Flatpak's
/// folders, xemu's BIOS paths, Ryujinx's keys from the BIOS folder.
pub fn first_run(home: &Path) -> Vec<String> {
    let env = Env { home: home.to_path_buf(), ..Env::real() };
    let mut notes = Vec::new();
    for e in catalog::EMULATORS {
        let Some(inst) = catalog::installed(e, home) else { continue };
        let env = match inst {
            catalog::Install::Flatpak(app) => env.flatpak(app),
            catalog::Install::AppImage(_) => env.clone(),
        };
        let app_dir = match &inst {
            catalog::Install::AppImage(p) => p.parent().map(Path::to_path_buf),
            _ => None,
        };
        let r = match e.id {
            "duckstation" => crate::defaults::duckstation(&env, app_dir.as_deref()),
            "pcsx2" => crate::defaults::pcsx2(&env),
            "rpcs3" => crate::defaults::rpcs3(&env),
            "cemu" => crate::defaults::cemu(&env),
            "azahar" => crate::defaults::azahar(&env),
            "retroarch" => {
                let mut r = crate::defaults::Report::default();
                let f = retroarch::config_dir(home).join("retroarch.cfg");
                if !f.exists() {
                    let _ = std::fs::create_dir_all(f.parent().unwrap_or(Path::new("/")));
                    if std::fs::write(&f, retroarch::fresh_cfg(home)).is_ok() {
                        r.written.push(f.to_string_lossy().into_owned());
                    }
                }
                Ok(r)
            }
            "xemu" => {
                let mut r = crate::defaults::Report::default();
                let f = home.join(".var/app/app.xemu.xemu/data/xemu/xemu/xemu.toml");
                if !f.exists() {
                    let b = home.join("Emulation/bios");
                    let body = format!(
                        "[general]\nshow_welcome = false\n\n[display.window]\nfullscreen_on_startup = true\n\n[sys.files]\nbootrom_path = '{}'\nflashrom_path = '{}'\nhdd_path = '{}'\n",
                        b.join("mcpx_1.0.bin").display(),
                        b.join("complex_4627.bin").display(),
                        b.join("xbox_hdd.qcow2").display()
                    );
                    let _ = std::fs::create_dir_all(f.parent().unwrap_or(Path::new("/")));
                    if std::fs::write(&f, body).is_ok() {
                        r.written.push(f.to_string_lossy().into_owned());
                    }
                }
                Ok(r)
            }
            "ryujinx" => {
                let keys = home.join(".var/app/io.github.ryubing.Ryujinx/config/Ryujinx/system");
                for k in ["prod.keys", "title.keys"] {
                    let have = keys.join(k);
                    if have.exists() {
                        continue;
                    }
                    let found = crate::bios::dirs(home).into_iter().flat_map(|d| [d.join(k), d.join("switch").join(k), d.join("ryujinx").join(k), d.join("ryujinx/keys").join(k)]).find(|p| p.is_file());
                    if let Some(src) = found {
                        let _ = std::fs::create_dir_all(&keys);
                        let _ = std::fs::copy(src, &have);
                    }
                }
                Ok(Default::default())
            }
            _ => Ok(Default::default()),
        };
        match r {
            Ok(r) => {
                bios_into_fresh(home, &r, "duckstation/settings.ini", "BIOS", "SearchDirectory");
                bios_into_fresh(home, &r, "PCSX2/inis/PCSX2.ini", "Folders", "Bios");
                notes.extend(r.written);
            }
            Err(x) => notes.push(format!("{}: {x}", e.name)),
        }
    }
    // Controller profiles after the settings files exist (the parts that are the player's own are kept).
    for id in crate::controls::emulators() {
        notes.extend(crate::controls::ensure(id, home));
    }
    for s in systems::SYSTEMS {
        shader_preset(home, s);
    }
    notes
}

/// The whole setup, on a worker. `only`: emulator ids to install (None: all of them).
pub fn run(ctx: &Ctx, only: Option<&[&str]>) -> Result<(), String> {
    let home = crate::roms::home();
    let roots = crate::roms::roots(&home);
    let want = |e: &Emulator| only.map_or(true, |o| o.contains(&e.id));
    emit(ctx, "folders", "", "Folders", None, false, None);
    for (i, r) in roots.iter().enumerate() {
        make_folders(r, i == 0).map_err(|e| format!("Couldn't make the folders in {}: {e}", r.base.display()))?;
    }
    let mut failed: Vec<String> = Vec::new();
    let flatpak = which("flatpak");
    if flatpak {
        let _ = run_quiet(&["remote-add", "--user", "--if-not-exists", "flathub", "https://dl.flathub.org/repo/flathub.flatpakrepo"]);
    }
    for e in catalog::EMULATORS.iter().filter(|e| want(e)) {
        if catalog::installed(e, &home).is_some() {
            emit(ctx, "install", e.id, e.name, Some(1.0), true, None);
            continue;
        }
        emit(ctx, "install", e.id, e.name, None, false, None);
        let r = match (e.flatpak, e.appimage) {
            (Some(app), _) if flatpak => install_flatpak(ctx, e, app),
            (_, Some(img)) => crate::emu_update(ctx, img.to_string()).map(|_| ()).map_err(|x| x.to_string()),
            _ => Err("Flatpak isn't available on this system".into()),
        };
        if let (Ok(()), Some(app)) = (&r, e.flatpak.filter(|_| flatpak)) {
            // Games on the SD card and in any folder: the emulator may read the whole disk.
            let _ = run_quiet(&["override", "--user", "--filesystem=host", app]);
        }
        if let Err(x) = &r {
            failed.push(format!("{}: {x}", e.name));
        }
        emit(ctx, "install", e.id, e.name, r.is_ok().then_some(1.0), true, r.err());
    }
    if want(catalog::by_id("retroarch").expect("retroarch")) && catalog::installed(catalog::by_id("retroarch").expect("retroarch"), &home).is_some() {
        first_run(&home);
        let p = retroarch::paths(&home);
        let cores = systems::cores();
        let missing: Vec<&str> = cores.iter().copied().filter(|c| !retroarch::core_path(&p, c).is_file()).collect();
        for (i, c) in missing.iter().enumerate() {
            emit(ctx, "cores", c, "RetroArch cores", Some(i as f32 / missing.len() as f32), false, None);
            if let Err(x) = install_core(&p.cores, c) {
                failed.push(x);
            }
        }
        emit(ctx, "cores", "", "RetroArch cores", Some(1.0), true, None);
    }
    first_run(&home);
    save_links(&home);
    emit(ctx, "done", "", "", Some(1.0), true, (!failed.is_empty()).then(|| failed.join("; ")));
    if failed.is_empty() {
        Ok(())
    } else {
        Err(failed.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_pick_systems_with_games() {
        let t = tempfile::tempdir().unwrap();
        let h = t.path();
        let root = crate::roms::Root { tag: "", base: h.join("Emulation") };
        make_folders(&root, true).unwrap();
        std::fs::write(root.roms().join("gc/Battle for Bikini Bottom.rvz"), b"x").unwrap();
        std::fs::write(root.roms().join("snes/Mario.sfc"), b"x").unwrap();
        let roms = crate::roms::scan_in(h, &[root]);
        let c = choices_in(h, &roms);
        let get = |id: &str| c.iter().find(|x| x.id == id).unwrap();
        assert!(get("dolphin").picked && get("dolphin").roms == 1);
        assert!(get("retroarch").picked);
        assert!(!get("pcsx2").picked && get("pcsx2").roms == 0);
        let picked: Vec<String> = c.iter().filter(|x| x.picked).map(|x| x.id.clone()).collect();
        assert_eq!(size_text(&c, &picked), "About 3.3 GB to download, with 2.3 GB of parts they share");
        assert_eq!(size_text(&c, &[]), "");
    }

    #[test]
    fn folders_links_and_status() {
        let t = tempfile::tempdir().unwrap();
        let h = t.path();
        let root = crate::roms::Root { tag: "", base: h.join("Emulation") };
        std::fs::create_dir_all(root.roms().join("megadrive")).unwrap();
        make_folders(&root, true).unwrap();
        assert!(root.roms().join("snes").is_dir() && root.base.join("bios").is_dir());
        assert!(!root.roms().join("genesis").exists(), "the alias folder counts");
        let made = save_links(h);
        assert!(made.contains(&"pcsx2/saves".to_string()));
        assert!(h.join("Emulation/saves/pcsx2/saves").read_link().unwrap().ends_with("config/PCSX2/memcards"));
        assert!(save_links(h).is_empty(), "links are made once");
        let s = status_in(h, &[root], false);
        assert!(!s.ready && s.emulators.iter().all(|e| e.installed.is_none()) && s.cores_missing == s.cores);
        // The handheld shader preset, once.
        shader_preset(h, crate::systems::by_id("gb").unwrap());
        shader_preset(h, crate::systems::by_id("gbc").unwrap());
        let d = retroarch::config_dir(h).join("config/Gambatte");
        assert!(std::fs::read_to_string(d.join("gb.slangp")).unwrap().contains("handheld/gameboy-pocket.slangp"));
        assert!(std::fs::read_to_string(d.join("gbc.slangp")).unwrap().contains("lcd-grid-v2"));
        std::fs::write(d.join("gb.slangp"), "mine").unwrap();
        shader_preset(h, crate::systems::by_id("gb").unwrap());
        assert_eq!(std::fs::read_to_string(d.join("gb.slangp")).unwrap(), "mine");
    }
}
