//! The emulators the one-press setup installs, how each is found on the device (its Flatpak, or an
//! AppImage the player or EmuDeck put in ~/Applications) and how a game is started with it.

use std::path::{Path, PathBuf};

pub struct Emulator {
    pub id: &'static str,
    pub name: &'static str,
    /// What it plays, for the setup list.
    pub plays: &'static str,
    /// Its Flathub id (None: Flathub has no maintained build).
    pub flatpak: Option<&'static str>,
    /// Its id in `crate::EMULATORS` when Rare installs and updates it as an AppImage instead.
    pub appimage: Option<&'static str>,
    /// Arguments before the game's path (fullscreen, no UI).
    pub args: &'static [&'static str],
    /// Arguments after the path.
    pub tail: &'static [&'static str],
    /// About how much it takes on disk, in MB (measured on the build machine; RetroArch with its cores). The
    /// Flatpak runtimes they share come on top the first time (SHARED_MB).
    pub size_mb: u32,
}

/// The Flatpak runtimes the emulators share (KDE and Freedesktop platforms, their GL drivers), the first time.
pub const SHARED_MB: u32 = 2300;

pub static EMULATORS: &[Emulator] = &[
    Emulator { id: "retroarch", name: "RetroArch", plays: "NES, SNES, N64, Game Boy, Genesis, Saturn, Dreamcast, arcade and more", flatpak: Some("org.libretro.RetroArch"), appimage: None, args: &["-f"], tail: &[], size_mb: 950 },
    // Flathub's DuckStation is no longer maintained there: the official AppImage instead.
    Emulator { id: "duckstation", name: "DuckStation", plays: "PlayStation", flatpak: None, appimage: Some("duckstation"), args: &["-fullscreen", "-batch", "--"], tail: &[], size_mb: 95 },
    Emulator { id: "pcsx2", name: "PCSX2", plays: "PlayStation 2", flatpak: Some("net.pcsx2.PCSX2"), appimage: Some("pcsx2"), args: &["-fullscreen", "-batch", "--"], tail: &[], size_mb: 125 },
    Emulator { id: "ppsspp", name: "PPSSPP", plays: "PSP", flatpak: Some("org.ppsspp.PPSSPP"), appimage: None, args: &["--fullscreen"], tail: &[], size_mb: 80 },
    Emulator { id: "dolphin", name: "Dolphin", plays: "GameCube and Wii", flatpak: Some("org.DolphinEmu.dolphin-emu"), appimage: None, args: &["-b", "--config=Dolphin.Display.Fullscreen=True", "-e"], tail: &[], size_mb: 60 },
    Emulator { id: "melonds", name: "melonDS", plays: "Nintendo DS", flatpak: Some("net.kuribo64.melonDS"), appimage: None, args: &["-f"], tail: &[], size_mb: 10 },
    Emulator { id: "azahar", name: "Azahar", plays: "Nintendo 3DS", flatpak: Some("org.azahar_emu.Azahar"), appimage: Some("azahar"), args: &["-f"], tail: &[], size_mb: 60 },
    Emulator { id: "cemu", name: "Cemu", plays: "Wii U", flatpak: Some("info.cemu.Cemu"), appimage: Some("cemu"), args: &["-f", "-g"], tail: &[], size_mb: 65 },
    Emulator { id: "rpcs3", name: "RPCS3", plays: "PlayStation 3", flatpak: Some("net.rpcs3.RPCS3"), appimage: Some("rpcs3"), args: &["--no-gui"], tail: &[], size_mb: 155 },
    Emulator { id: "xemu", name: "xemu", plays: "Xbox", flatpak: Some("app.xemu.xemu"), appimage: None, args: &["-full-screen", "-dvd_path"], tail: &[], size_mb: 35 },
    Emulator { id: "ryujinx", name: "Ryujinx", plays: "Nintendo Switch", flatpak: Some("io.github.ryubing.Ryujinx"), appimage: None, args: &["--fullscreen"], tail: &[], size_mb: 100 },
];

/// Emulators Rare doesn't install but starts a system's games with when the player has one and
/// not the usual one: (the usual one's id, the alternative).
pub static ALTERNATES: &[(&str, Emulator)] = &[
    ("ryujinx", Emulator { id: "citron", name: "Citron", plays: "Nintendo Switch", flatpak: None, appimage: Some("citron"), args: &["-f", "-g"], tail: &[], size_mb: 165 }),
    ("azahar", Emulator { id: "lime3ds", name: "Lime3DS", plays: "Nintendo 3DS", flatpak: Some("io.github.lime3ds.Lime3DS"), appimage: None, args: &["-f"], tail: &[], size_mb: 60 }),
];

pub fn by_id(id: &str) -> Option<&'static Emulator> {
    EMULATORS.iter().find(|e| e.id == id)
}

/// The emulator a Flatpak id belongs to.
pub fn by_flatpak(app: &str) -> Option<&'static Emulator> {
    EMULATORS.iter().find(|e| e.flatpak == Some(app))
}

/// Where Flatpak keeps installed apps: the user installation first, then the system one.
pub fn flatpak_roots(home: &Path) -> Vec<PathBuf> {
    vec![home.join(".local/share/flatpak/app"), PathBuf::from("/var/lib/flatpak/app")]
}

pub fn flatpak_installed(home: &Path, app: &str) -> bool {
    flatpak_roots(home).iter().any(|r| r.join(app).join("current").exists() || r.join(app).join("x86_64").is_dir())
}

/// How a game is started with this emulator on this device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Install {
    Flatpak(&'static str),
    AppImage(PathBuf),
}

impl Install {
    /// The program and the arguments before the emulator's own.
    pub fn command(&self) -> (String, Vec<String>) {
        match self {
            Install::Flatpak(id) => ("flatpak".into(), vec!["run".into(), (*id).into()]),
            Install::AppImage(p) => (p.to_string_lossy().into_owned(), vec![]),
        }
    }
}

/// The emulator as it is on this device: its Flatpak when installed, else its AppImage.
pub fn installed(e: &Emulator, home: &Path) -> Option<Install> {
    if let Some(f) = e.flatpak.filter(|f| flatpak_installed(home, f)) {
        return Some(Install::Flatpak(f));
    }
    let apps = home.join("Applications");
    e.appimage.and_then(crate::by_id).and_then(|a| crate::find_installed(a, &apps)).map(Install::AppImage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_flatpak_then_appimage() {
        let t = tempfile::tempdir().unwrap();
        let h = t.path();
        let pcsx2 = by_id("pcsx2").unwrap();
        assert_eq!(installed(pcsx2, h), None);
        std::fs::create_dir_all(h.join("Applications")).unwrap();
        std::fs::write(h.join("Applications/pcsx2-Qt.AppImage"), b"x").unwrap();
        assert_eq!(installed(pcsx2, h), Some(Install::AppImage(h.join("Applications/pcsx2-Qt.AppImage"))));
        std::fs::create_dir_all(h.join(".local/share/flatpak/app/net.pcsx2.PCSX2/x86_64")).unwrap();
        assert_eq!(installed(pcsx2, h), Some(Install::Flatpak("net.pcsx2.PCSX2")));
        assert_eq!(Install::Flatpak("a.b.c").command().1, ["run", "a.b.c"]);
        assert_eq!(by_flatpak("org.libretro.RetroArch").unwrap().id, "retroarch");
        // Every system's emulator is in the catalog.
        for s in crate::systems::SYSTEMS {
            if let crate::systems::Runner::App(a) = s.runner {
                assert!(by_id(a).is_some(), "{a}");
            }
        }
    }
}
