//! The SD card: format it with SteamOS's own script and set it up for games in one step.
//!
//! Formatting goes through /usr/bin/steamos-polkit-helpers/steamos-format-sdcard, the polkit
//! helper Steam's own Format button uses (org.valve.steamos.policy allows it without a password).
//! It runs /usr/libexec/hwsupport/format-sdcard.sh: a test for fake cards (f3probe), a GPT with one
//! ext4 partition (casefold, as Proton needs), and the automount (steamos-automount.sh), which
//! mounts it under /run/media/<user>/.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const FORMAT_HELPER: &str = "/usr/bin/steamos-polkit-helpers/steamos-format-sdcard";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatStage {
    Testing,
    Formatting,
    Mounting,
}

/// What a line of the script's output says about where it is.
pub fn stage_of(line: &str) -> Option<FormatStage> {
    match line.trim() {
        "stage=testing" => Some(FormatStage::Testing),
        "stage=formatting" => Some(FormatStage::Formatting),
        l if l.starts_with("**** Mounted") => Some(FormatStage::Mounting),
        _ => None,
    }
}

/// The player-facing reason for the script's exit code.
pub fn format_error(code: i32) -> String {
    match code {
        14 => "This card isn't what it says it is: it reports more space than it has. It was set back to exFAT. Use another card for games.".into(),
        19 => "The SD card was taken out.".into(),
        32 => "The SD card is in use. Close what is using it and try again.".into(),
        53 => "The SD card is busy. Wait a moment and try again.".into(),
        5 => "The card was formatted but didn't mount. Take it out and put it back in.".into(),
        126 | 127 => "Formatting isn't available on this system.".into(),
        c => format!("Couldn't format the SD card (error {c})."),
    }
}

/// Erases the card in the slot and formats it for games. Blocks for the whole run (a minute or
/// more); `on` hears each stage. Everything on the card is lost.
pub fn format(on: &mut dyn FnMut(FormatStage)) -> Result<(), String> {
    let dev = crate::storage::sd_device();
    if dev.starts_with("/dev/loop") {
        return format_loop(&dev, on);
    }
    if !Path::new(FORMAT_HELPER).exists() {
        return Err(format_error(127));
    }
    let mut child = Command::new(FORMAT_HELPER).stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null()).spawn().map_err(|e| format!("Couldn't start formatting: {e}"))?;
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(s) = stage_of(&line) {
                on(s);
            }
        }
    }
    let st = child.wait().map_err(|e| format!("Formatting stopped: {e}"))?;
    match st.code() {
        Some(0) => Ok(()),
        Some(c) => Err(format_error(c)),
        None => Err("Formatting stopped.".into()),
    }
}

/// The test card (RARE_SD_DEVICE=/dev/loopN, an image file the player owns, attached with
/// `udisksctl loop-setup`): SteamOS's script only takes /dev/mmcblk* and /dev/sd*, and formatting a
/// device needs root, so this makes the same filesystem on the image file itself (format-device.sh's
/// mkfs.ext4 options: casefold, no reserved blocks, the player owns the root), attaches it again so
/// the loop device reads the new filesystem, and mounts it for the player through udisks, the way
/// steamos-automount.sh mounts a card.
fn format_loop(dev: &str, on: &mut dyn FnMut(FormatStage)) -> Result<(), String> {
    let name = dev.trim_start_matches("/dev/");
    if !name.starts_with("loop") || !name[4..].chars().all(|c| c.is_ascii_digit()) {
        return Err(format_error(19));
    }
    let run = |args: &[&str]| -> Result<String, String> {
        let o = Command::new(args[0]).args(&args[1..]).stdin(Stdio::null()).output().map_err(|e| format!("Couldn't run {}: {e}", args[0]))?;
        if o.status.success() {
            Ok(String::from_utf8_lossy(&o.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
        }
    };
    let file = std::fs::read_to_string(format!("/sys/block/{name}/loop/backing_file")).map_err(|_| "The test card has no image file.".to_string())?;
    let file = file.trim().to_string();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::metadata(&file).map_err(|e| format!("Couldn't read {file}: {e}"))?;
        if m.uid() != unsafe { libc::getuid() } || !m.is_file() {
            return Err(format!("{file} isn't an image file you own."));
        }
    }
    on(FormatStage::Formatting);
    let _ = run(&["udisksctl", "unmount", "-b", dev, "--no-user-interaction"]);
    run(&["udisksctl", "loop-delete", "-b", dev, "--no-user-interaction"]).map_err(|e| format!("Couldn't detach the test card: {e}"))?;
    #[cfg(unix)]
    let owner = unsafe { format!("root_owner={}:{},nodiscard", libc::getuid(), libc::getgid()) };
    #[cfg(not(unix))]
    let owner = "nodiscard".to_string();
    let made = run(&["mkfs.ext4", "-F", "-q", "-L", "RARE-SD", "-E", &owner, "-m", "0", "-O", "casefold", &file]);
    let again = run(&["udisksctl", "loop-setup", "-f", &file, "--no-user-interaction"]).map_err(|e| format!("Couldn't attach the test card again: {e}"))?;
    made.map_err(|e| format!("Couldn't format the test card: {e}"))?;
    if !again.contains(&format!("{dev}.")) {
        return Err(format!("The test card came back as another device: {}", again.trim()));
    }
    on(FormatStage::Mounting);
    std::thread::sleep(std::time::Duration::from_millis(500));
    run(&["udisksctl", "mount", "-b", dev, "--no-user-interaction"]).map_err(|e| format!("The card was formatted but didn't mount: {e}"))?;
    Ok(())
}

/// The folders a card gets for games: a Steam library and a Rare Games folder.
pub fn game_dirs(mount: &Path) -> (PathBuf, PathBuf) {
    (mount.join("SteamLibrary"), mount.join("Games"))
}

/// Makes the card's Games folder and Steam library and adds the library to Steam's list.
/// Steam must be closed (the caller closes it quietly and starts it again).
pub fn set_up_for_games(mount: &Path) -> Result<(PathBuf, PathBuf), String> {
    let (lib, games) = game_dirs(mount);
    std::fs::create_dir_all(&games).map_err(|e| format!("Couldn't make {}: {e}", games.display()))?;
    // A card that already has a Steam library (from a Steam Deck) keeps it.
    let lib = crate::steam::libraries_on_mounts().into_iter().map(|l| std::fs::canonicalize(&l).unwrap_or(l)).find(|l| l.starts_with(mount)).unwrap_or(lib);
    crate::movegame::make_steam_library(&lib)?;
    if crate::steam::find_roots().iter().any(|r| r.primary) {
        crate::steam::add_library(&lib.to_string_lossy())?;
    }
    Ok((lib, games))
}

/// True when the card's Steam library is in Steam's list and its Games folder exists.
pub fn is_set_up(mount: &Path) -> bool {
    let Some(root) = crate::steam::find_roots().into_iter().find(|r| r.primary) else { return mount.join("Games").is_dir() };
    let registered = crate::steam::registered_libraries(&root.path).iter().skip(1).any(|l| std::fs::canonicalize(l).unwrap_or(l.clone()).starts_with(mount));
    registered && mount.join("Games").is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_and_errors() {
        assert_eq!(stage_of("stage=testing"), Some(FormatStage::Testing));
        assert_eq!(stage_of("stage=formatting\n"), Some(FormatStage::Formatting));
        assert_eq!(stage_of("**** Mounted /dev/mmcblk0p1 at /run/media/deck/x ****"), Some(FormatStage::Mounting));
        assert_eq!(stage_of("Create partition table: 1"), None);
        assert!(format_error(14).contains("more space"));
        assert!(format_error(99).contains("99"));
    }
}
