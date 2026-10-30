//! The game systems Rare plays: their ROM folder (EmuDeck's names, so an existing ~/Emulation
//! works as it is), the file types that are games, which emulator runs them, where libretro's
//! box art lives and the RetroAchievements console.

/// What runs a system's games.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runner {
    /// A RetroArch core: the file stem in RetroArch's cores folder ("snes9x" for snes9x_libretro.so).
    Core(&'static str),
    /// A standalone emulator, by its id in `catalog::EMULATORS`.
    App(&'static str),
}

#[derive(Debug, PartialEq)]
pub struct System {
    /// The ROM folder name (EmuDeck and ES-DE use the same ones).
    pub id: &'static str,
    /// Other folder names that hold the same system's games.
    pub aliases: &'static [&'static str],
    pub name: &'static str,
    /// A short name for tabs and chips.
    pub short: &'static str,
    /// File types that are games, lowercase, without the dot.
    pub exts: &'static [&'static str],
    pub runner: Runner,
    /// The libretro-thumbnails repository (the box art's folder on thumbnails.libretro.com).
    pub thumbs: &'static str,
    /// RetroAchievements console id (0: none).
    pub ra: u32,
    /// Games here are folders (PS3 `.ps3` folders, Wii U `.wua` loadiine folders).
    pub folders: &'static [&'static str],
    /// Light enough to keep a rewind buffer at full speed on the handheld.
    pub rewind: bool,
    /// The handheld screen shader preset (RetroArch's shaders_slang), for handheld systems.
    pub shader: Option<&'static str>,
}

const CART: &[&str] = &["zip", "7z"];

macro_rules! sys {
    ($id:expr, $aliases:expr, $name:expr, $short:expr, $exts:expr, $runner:expr, $thumbs:expr, $ra:expr) => {
        System { id: $id, aliases: $aliases, name: $name, short: $short, exts: $exts, runner: $runner, thumbs: $thumbs, ra: $ra, folders: &[], rewind: false, shader: None }
    };
}

use Runner::{App, Core};

pub static SYSTEMS: &[System] = &[
    System { rewind: true, ..sys!("nes", &["famicom"], "Nintendo Entertainment System", "NES", &["nes", "unf", "unif", "zip", "7z"], Core("mesen"), "Nintendo - Nintendo Entertainment System", 7) },
    System { rewind: true, ..sys!("fds", &[], "Famicom Disk System", "FDS", &["fds", "zip"], Core("mesen"), "Nintendo - Family Computer Disk System", 81) },
    System { rewind: true, ..sys!("snes", &["sfc", "snesna"], "Super Nintendo", "SNES", &["sfc", "smc", "fig", "swc", "bs", "st", "zip", "7z"], Core("snes9x"), "Nintendo - Super Nintendo Entertainment System", 3) },
    sys!("n64", &[], "Nintendo 64", "N64", &["n64", "v64", "z64", "zip", "7z"], Core("mupen64plus_next"), "Nintendo - Nintendo 64", 2),
    System { rewind: true, shader: Some("handheld/gameboy-pocket.slangp"), ..sys!("gb", &[], "Game Boy", "GB", &["gb", "zip", "7z"], Core("gambatte"), "Nintendo - Game Boy", 4) },
    System { rewind: true, shader: Some("handheld/lcd-grid-v2.slangp"), ..sys!("gbc", &[], "Game Boy Color", "GBC", &["gbc", "zip", "7z"], Core("gambatte"), "Nintendo - Game Boy Color", 6) },
    System { rewind: true, shader: Some("handheld/lcd-grid-v2.slangp"), ..sys!("gba", &[], "Game Boy Advance", "GBA", &["gba", "zip", "7z"], Core("mgba"), "Nintendo - Game Boy Advance", 5) },
    sys!("nds", &[], "Nintendo DS", "DS", &["nds", "dsi", "zip"], App("melonds"), "Nintendo - Nintendo DS", 18),
    sys!("n3ds", &["3ds"], "Nintendo 3DS", "3DS", &["3ds", "cci", "cxi", "3dsx", "app", "zcci", "zcxi", "z3dsx"], App("azahar"), "Nintendo - Nintendo 3DS", 62),
    System { rewind: true, ..sys!("virtualboy", &[], "Virtual Boy", "VB", &["vb", "vboy", "zip"], Core("mednafen_vb"), "Nintendo - Virtual Boy", 28) },
    sys!("gc", &["gamecube"], "GameCube", "GC", &["iso", "gcm", "gcz", "rvz", "ciso", "m3u"], App("dolphin"), "Nintendo - GameCube", 16),
    sys!("wii", &[], "Wii", "Wii", &["iso", "wbfs", "rvz", "gcz", "wad", "wia", "ciso"], App("dolphin"), "Nintendo - Wii", 19),
    System { folders: &["wua"], ..sys!("wiiu", &[], "Wii U", "Wii U", &["wua", "wux", "wud", "rpx"], App("cemu"), "Nintendo - Wii U", 0) },
    sys!("switch", &[], "Nintendo Switch", "Switch", &["nsp", "xci", "nca"], App("ryujinx"), "Nintendo - Nintendo Switch", 0),
    sys!("psx", &["ps1", "playstation"], "PlayStation", "PS1", &["cue", "chd", "m3u", "pbp", "ccd", "iso", "img", "ecm", "mds"], App("duckstation"), "Sony - PlayStation", 12),
    sys!("ps2", &[], "PlayStation 2", "PS2", &["iso", "chd", "cso", "zso", "gz", "m3u", "elf"], App("pcsx2"), "Sony - PlayStation 2", 21),
    System { folders: &["ps3"], ..sys!("ps3", &[], "PlayStation 3", "PS3", &["iso"], App("rpcs3"), "Sony - PlayStation 3", 0) },
    sys!("psp", &[], "PlayStation Portable", "PSP", &["iso", "cso", "pbp", "chd"], App("ppsspp"), "Sony - PlayStation Portable", 41),
    System { rewind: true, ..sys!("genesis", &["megadrive", "genesiswide"], "Sega Genesis", "Genesis", &["md", "gen", "bin", "smd", "zip", "7z"], Core("genesis_plus_gx"), "Sega - Mega Drive - Genesis", 1) },
    System { rewind: true, ..sys!("mastersystem", &[], "Sega Master System", "SMS", &["sms", "zip", "7z"], Core("genesis_plus_gx"), "Sega - Master System - Mark III", 11) },
    System { rewind: true, ..sys!("gamegear", &[], "Sega Game Gear", "Game Gear", &["gg", "zip", "7z"], Core("genesis_plus_gx"), "Sega - Game Gear", 15) },
    sys!("segacd", &["megacd", "megacdjp"], "Sega CD", "Sega CD", &["cue", "chd", "iso", "m3u"], Core("genesis_plus_gx"), "Sega - Mega-CD - Sega CD", 9),
    System { rewind: true, ..sys!("sega32x", &["32x"], "Sega 32X", "32X", &["32x", "zip", "7z"], Core("picodrive"), "Sega - 32X", 10) },
    sys!("saturn", &[], "Sega Saturn", "Saturn", &["cue", "chd", "m3u", "ccd", "mds"], Core("mednafen_saturn"), "Sega - Saturn", 39),
    sys!("dreamcast", &[], "Dreamcast", "Dreamcast", &["chd", "cdi", "gdi", "m3u"], Core("flycast"), "Sega - Dreamcast", 40),
    System { rewind: true, ..sys!("pcengine", &["tg16"], "TurboGrafx-16", "TG-16", &["pce", "zip", "7z"], Core("mednafen_pce_fast"), "NEC - PC Engine - TurboGrafx 16", 8) },
    sys!("pcenginecd", &["tg-cd"], "TurboGrafx-CD", "TG-CD", &["cue", "chd", "ccd", "m3u"], Core("mednafen_pce_fast"), "NEC - PC Engine CD - TurboGrafx-CD", 76),
    System { rewind: true, ..sys!("atari2600", &[], "Atari 2600", "2600", &["a26", "bin", "zip"], Core("stella"), "Atari - 2600", 25) },
    System { rewind: true, shader: Some("handheld/lcd-grid-v2.slangp"), ..sys!("atarilynx", &["lynx"], "Atari Lynx", "Lynx", &["lnx", "zip"], Core("handy"), "Atari - Lynx", 13) },
    System { rewind: true, ..sys!("ngp", &[], "Neo Geo Pocket", "NGP", &["ngp", "zip"], Core("mednafen_ngp"), "SNK - Neo Geo Pocket", 14) },
    System { rewind: true, ..sys!("ngpc", &[], "Neo Geo Pocket Color", "NGPC", &["ngc", "zip"], Core("mednafen_ngp"), "SNK - Neo Geo Pocket Color", 14) },
    System { rewind: true, ..sys!("wonderswan", &[], "WonderSwan", "WS", &["ws", "zip"], Core("mednafen_wswan"), "Bandai - WonderSwan", 53) },
    System { rewind: true, ..sys!("wonderswancolor", &[], "WonderSwan Color", "WSC", &["wsc", "zip"], Core("mednafen_wswan"), "Bandai - WonderSwan Color", 53) },
    sys!("neogeo", &[], "Neo Geo", "Neo Geo", CART, Core("fbneo"), "SNK - Neo Geo", 27),
    sys!("arcade", &["fbneo", "fba"], "Arcade", "Arcade", CART, Core("fbneo"), "FBNeo - Arcade Games", 27),
    sys!("mame", &[], "MAME", "MAME", CART, Core("mame"), "MAME", 27),
    sys!("3do", &[], "3DO", "3DO", &["iso", "chd", "cue"], Core("opera"), "The 3DO Company - 3DO", 43),
    sys!("xbox", &[], "Xbox", "Xbox", &["iso", "xiso"], App("xemu"), "Microsoft - Xbox", 0),
    sys!("dos", &["pc"], "DOS", "DOS", &["zip", "dosz", "exe", "com", "bat"], Core("dosbox_pure"), "DOS", 0),
];

pub fn by_id(id: &str) -> Option<&'static System> {
    SYSTEMS.iter().find(|s| s.id == id)
}

/// The system a ROM folder name belongs to ("megadrive" is the Genesis).
pub fn by_folder(name: &str) -> Option<&'static System> {
    let n = name.to_ascii_lowercase();
    SYSTEMS.iter().find(|s| s.id == n || s.aliases.contains(&n.as_str()))
}

/// The cores Rare installs into RetroArch: every core a system above uses, once.
pub fn cores() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    for s in SYSTEMS {
        if let Runner::Core(c) = s.runner {
            if !v.contains(&c) {
                v.push(c);
            }
        }
    }
    v
}

/// RetroArch's display name for a core: the folder it keeps per-core options and shader
/// presets in (`config/<name>/<name>.slangp`).
pub fn core_display_name(core: &str) -> Option<&'static str> {
    Some(match core {
        "mesen" => "Mesen",
        "snes9x" => "Snes9x",
        "mupen64plus_next" => "Mupen64Plus-Next",
        "gambatte" => "Gambatte",
        "mgba" => "mGBA",
        "mednafen_vb" => "Beetle VB",
        "genesis_plus_gx" => "Genesis Plus GX",
        "picodrive" => "PicoDrive",
        "mednafen_saturn" => "Beetle Saturn",
        "flycast" => "Flycast",
        "mednafen_pce_fast" => "Beetle PCE Fast",
        "stella" => "Stella",
        "handy" => "Handy",
        "mednafen_ngp" => "Beetle NeoPop",
        "mednafen_wswan" => "Beetle WonderSwan",
        "fbneo" => "FinalBurn Neo",
        "mame" => "MAME",
        "opera" => "Opera",
        "dosbox_pure" => "DOSBox-pure",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_and_cores() {
        assert_eq!(by_folder("megadrive").unwrap().id, "genesis");
        assert_eq!(by_folder("GC").unwrap().id, "gc");
        assert_eq!(by_folder("gamecube").unwrap().id, "gc");
        assert!(by_folder("steam").is_none());
        let c = cores();
        assert!(c.contains(&"snes9x") && c.contains(&"gambatte"));
        assert_eq!(c.iter().filter(|x| **x == "gambatte").count(), 1);
        for core in c {
            assert!(core_display_name(core).is_some(), "{core}");
        }
        // Every id and alias names one system.
        let mut seen = std::collections::HashSet::new();
        for s in SYSTEMS {
            for n in std::iter::once(&s.id).chain(s.aliases.iter()) {
                assert!(seen.insert(*n), "{n} twice");
            }
            assert!(!s.exts.is_empty() || !s.folders.is_empty());
        }
    }
}
