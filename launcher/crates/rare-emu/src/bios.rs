//! BIOS check: which systems need which files, and whether the files the player put in
//! Emulation/bios (device or SD card) are there and the right ones (MD5 against the known dumps).
//! Rare never downloads BIOS files; it only looks at what is there.

use serde::Serialize;
use std::path::{Path, PathBuf};

pub struct File {
    pub name: &'static str,
    /// Known good dumps (lowercase MD5). Empty: any file with this name.
    pub md5: &'static [&'static str],
    pub note: &'static str,
}

pub struct Need {
    pub system: &'static str,
    /// Every group needs one of its files.
    pub groups: &'static [&'static [File]],
    /// Groups the system plays without (better with).
    pub optional: &'static [&'static [File]],
    /// Extra words for the player.
    pub hint: &'static str,
}

const fn f(name: &'static str, md5: &'static [&'static str], note: &'static str) -> File {
    File { name, md5, note }
}

pub static NEEDS: &[Need] = &[
    Need {
        system: "psx",
        groups: &[&[
            f("scph5501.bin", &["490f666e1afb15b7362b406ed1cea246"], "North America"),
            f("scph5500.bin", &["8dd7d5296a650fac7319bce665a6a53c"], "Japan"),
            f("scph5502.bin", &["32736f17079d0b2b7024407c39bd3050"], "Europe"),
            f("scph1001.bin", &["924e392ed05558ffdb115408c263dccf"], "North America"),
            f("scph7001.bin", &["1e68c231d0896b7eadcad1d7d8e76129"], "North America"),
            f("scph101.bin", &["6e3735ff4c7dc899ee98981385f6f3d0"], "PS one"),
        ]],
        optional: &[],
        hint: "One BIOS from your own console plays games from every region.",
    },
    Need { system: "ps2", groups: &[&[f("PS2 BIOS (.bin)", &[], "Any region, from your own console")]], optional: &[], hint: "Any PS2 BIOS dump works; PCSX2 lists every one it finds." },
    Need {
        system: "segacd",
        groups: &[&[
            f("bios_CD_U.bin", &["2efd74e3232ff260e371b99f84024f7f"], "North America"),
            f("bios_CD_E.bin", &["e66fa1dc5820d254611fdcdba0662372"], "Europe"),
            f("bios_CD_J.bin", &["278a9397d192149e84e820ac621a8edd"], "Japan"),
        ]],
        optional: &[],
        hint: "Each region's games want their own BIOS.",
    },
    Need {
        system: "saturn",
        groups: &[&[f("mpr-17933.bin", &["3240872c70984b6cbfda1586cab68dbe"], "North America and Europe"), f("sega_101.bin", &["85ec9ca47d8f6807718151cbcca8b964"], "Japan")]],
        optional: &[],
        hint: "",
    },
    Need { system: "dreamcast", groups: &[&[f("dc/dc_boot.bin", &["e10c53c2f8b90bab96ead2d368858623"], "Boot ROM")]], optional: &[&[f("dc/dc_flash.bin", &["0a93f7940c455905bea6e392dfde92a4"], "Flash ROM")]], hint: "They go in a dc folder inside the BIOS folder." },
    Need { system: "pcenginecd", groups: &[&[f("syscard3.pce", &["38179df8f4ac870017db21ebcbf53114"], "System Card 3.0")]], optional: &[], hint: "" },
    Need { system: "fds", groups: &[&[f("disksys.rom", &["ca30b50f880eb660a320674ed365ef7a"], "Disk System BIOS")]], optional: &[], hint: "" },
    Need { system: "atarilynx", groups: &[&[f("lynxboot.img", &["fcd403db69f54290b51035d82f835e7b"], "Boot ROM")]], optional: &[], hint: "" },
    Need { system: "3do", groups: &[&[f("panafz10.bin", &["51f2f43ae2f3508a14d9f56597e2d3ce"], "Panasonic FZ-10")]], optional: &[], hint: "" },
    Need { system: "neogeo", groups: &[&[f("neogeo.zip", &[], "Neo Geo BIOS set")]], optional: &[], hint: "neogeo.zip can also sit next to the games." },
    Need { system: "gba", groups: &[], optional: &[&[f("gba_bios.bin", &["a860e8c0b6d573d191e4ec7db1b1e4f6"], "Boot logo and better accuracy")]], hint: "Games play without it." },
    Need {
        system: "nds",
        groups: &[],
        optional: &[&[f("bios7.bin", &["df692a80a5b1bc90728bc3dfc76cd948"], "ARM7 BIOS")], &[f("bios9.bin", &["a392174eb3e572fed6447e956bde4b25"], "ARM9 BIOS")], &[f("firmware.bin", &[], "Firmware")]],
        hint: "melonDS has its own free BIOS; real ones help a few games.",
    },
    Need {
        system: "xbox",
        groups: &[&[f("mcpx_1.0.bin", &["d49c52a4102f6df7bcf8d0617ac475ed"], "MCPX boot ROM")], &[f("complex_4627.bin", &[], "Flash ROM")], &[f("xbox_hdd.qcow2", &[], "Hard disk image")]],
        optional: &[],
        hint: "",
    },
    Need { system: "switch", groups: &[&[f("prod.keys", &[], "Keys from your own console")]], optional: &[&[f("title.keys", &[], "Title keys")]], hint: "Firmware from your console is installed in Ryujinx itself." },
    Need { system: "ps3", groups: &[&[f("PS3UPDAT.PUP", &[], "PS3 system software")]], optional: &[], hint: "Install it in RPCS3 once: File, Install Firmware." },
];

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum State {
    Found,
    Missing,
    /// A file with that name whose MD5 is none of the known dumps.
    Wrong,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub note: String,
    pub state: State,
    pub required: bool,
    pub path: Option<String>,
    /// Other files that would do instead (the group's other names).
    pub or: Vec<String>,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SystemBios {
    pub system: String,
    pub name: String,
    /// Every required group has a good file.
    pub ready: bool,
    pub entries: Vec<Entry>,
    pub hint: String,
    /// Where to put them.
    pub folder: String,
    /// The player has games for this system.
    pub has_games: bool,
}

/// Every file in the BIOS folders (two levels deep), by lowercase relative path.
fn files_in(dirs: &[PathBuf]) -> Vec<(String, PathBuf)> {
    let mut v = Vec::new();
    for d in dirs {
        let mut stack = vec![(d.clone(), 0)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    if depth < 2 {
                        stack.push((p, depth + 1));
                    }
                } else if let Ok(rel) = p.strip_prefix(d) {
                    v.push((rel.to_string_lossy().to_ascii_lowercase(), p));
                }
            }
        }
    }
    v
}

fn md5_ok(p: &Path, want: &[&str]) -> bool {
    // BIOS files are small; anything over 64 MB is not one of these.
    want.is_empty() || (p.metadata().map(|m| m.len() < 64 << 20).unwrap_or(false) && crate::md5::file(p, 0, u64::MAX).map(|h| want.contains(&h.as_str())).unwrap_or(false))
}

/// A file of the group: exact relative path first, then the name anywhere.
fn find<'a>(files: &'a [(String, PathBuf)], name: &str) -> Option<&'a PathBuf> {
    let n = name.to_ascii_lowercase();
    files.iter().find(|(r, _)| *r == n).or_else(|| files.iter().find(|(r, _)| r.rsplit('/').next() == n.rsplit('/').next())).map(|(_, p)| p)
}

fn ps2_bios(files: &[(String, PathBuf)]) -> Option<&PathBuf> {
    files.iter().find(|(r, p)| {
        let name = r.rsplit('/').next().unwrap_or(r);
        name.ends_with(".bin") && (name.starts_with("scph") || name.contains("ps2")) && p.metadata().map(|m| m.len() == 4 << 20).unwrap_or(false)
    }).map(|(_, p)| p)
}

fn group_entries(files: &[(String, PathBuf)], group: &[File], required: bool, out: &mut Vec<Entry>) -> bool {
    let names: Vec<String> = group.iter().map(|f| f.name.to_string()).collect();
    let mut wrong: Option<Entry> = None;
    for fl in group {
        let hit = if fl.name.starts_with("PS2 BIOS") { ps2_bios(files) } else { find(files, fl.name) };
        let Some(p) = hit else { continue };
        let e = Entry { name: fl.name.into(), note: fl.note.into(), state: State::Found, required, path: Some(p.to_string_lossy().into_owned()), or: vec![] };
        if md5_ok(p, fl.md5) {
            out.push(e);
            return true;
        }
        wrong.get_or_insert(Entry { state: State::Wrong, ..e });
    }
    match wrong {
        Some(w) => out.push(w),
        None => out.push(Entry { name: group[0].name.into(), note: group[0].note.into(), state: State::Missing, required, path: None, or: names[1..].to_vec() }),
    }
    false
}

/// The check for every system that uses BIOS files. `dirs`: the BIOS folders, the first is where
/// new files go. `with_games`: system ids the player has games for (listed first).
pub fn check_in(dirs: &[PathBuf], with_games: &[&str]) -> Vec<SystemBios> {
    let files = files_in(dirs);
    let mut out: Vec<SystemBios> = NEEDS
        .iter()
        .map(|n| {
            let mut entries = Vec::new();
            let mut ready = true;
            for g in n.groups {
                ready &= group_entries(&files, g, true, &mut entries);
            }
            for g in n.optional {
                group_entries(&files, g, false, &mut entries);
            }
            let sys = crate::systems::by_id(n.system);
            SystemBios {
                system: n.system.into(),
                name: sys.map(|s| s.name).unwrap_or(n.system).into(),
                ready,
                entries,
                hint: n.hint.into(),
                folder: dirs.first().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
                has_games: with_games.contains(&n.system),
            }
        })
        .collect();
    out.sort_by_key(|s| (!s.has_games, s.ready));
    out
}

/// The BIOS folders on the device and the SD card.
pub fn dirs(home: &Path) -> Vec<PathBuf> {
    crate::roms::roots(home).into_iter().map(|r| r.base.join("bios")).collect()
}

pub fn check(with_games: &[&str]) -> Vec<SystemBios> {
    check_in(&dirs(&crate::roms::home()), with_games)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn found_missing_wrong() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("bios");
        std::fs::create_dir_all(d.join("dc")).unwrap();
        // A file with the right name and the wrong contents.
        std::fs::write(d.join("SCPH5501.BIN"), b"not a bios").unwrap();
        std::fs::write(d.join("dc/dc_boot.bin"), b"x").unwrap();
        std::fs::write(d.join("neogeo.zip"), b"zip").unwrap();
        let r = check_in(&[d.clone()], &["psx"]);
        assert_eq!(r[0].system, "psx", "games first");
        let psx = &r[0];
        assert!(!psx.ready);
        assert_eq!(psx.entries[0].state, State::Wrong);
        let neo = r.iter().find(|s| s.system == "neogeo").unwrap();
        assert!(neo.ready && neo.entries[0].state == State::Found);
        let sat = r.iter().find(|s| s.system == "saturn").unwrap();
        assert_eq!(sat.entries[0].state, State::Missing);
        assert_eq!(sat.entries[0].or, ["sega_101.bin"]);
        let gba = r.iter().find(|s| s.system == "gba").unwrap();
        assert!(gba.ready && !gba.entries[0].required);
        // Every system named here exists.
        for n in NEEDS {
            assert!(crate::systems::by_id(n.system).is_some(), "{}", n.system);
        }
    }
}
