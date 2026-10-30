//! A Chromium-based browser for web games that need features WebKitGTK lacks (WebGPU, some
//! codecs, Chrome-only APIs). Flatpaks first, since that is how Bazzite installs browsers.

use std::path::Path;

/// Flatpak ids in order of preference.
const FLATPAKS: &[(&str, &str)] = &[
    ("com.google.Chrome", "Google Chrome"),
    ("org.chromium.Chromium", "Chromium"),
    ("com.brave.Browser", "Brave"),
    ("io.github.ungoogled_software.ungoogled_chromium", "Chromium"),
    ("com.microsoft.Edge", "Microsoft Edge"),
    ("com.google.ChromeDev", "Google Chrome Dev"),
];

const BINARIES: &[(&str, &str)] = &[("google-chrome", "Google Chrome"), ("google-chrome-stable", "Google Chrome"), ("chromium", "Chromium"), ("chromium-browser", "Chromium"), ("brave-browser", "Brave")];

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Browser {
    pub name: String,
    pub program: String,
    /// Arguments before the browser's own flags (`run <flatpak id>`).
    pub prefix: Vec<String>,
}

/// Hardware video decode (VA-API on the 780M's VCN) for every Chromium-based browser Rare opens.
/// Chrome turns it on by itself only on Wayland; under gamescope it runs on X11 (Xwayland), where
/// YouTube's VP9/AV1 otherwise decodes on the CPU. The Flatpak gets the VA driver from the
/// org.freedesktop.Platform.GL.default runtime (Mesa radeonsi_drv_video.so).
pub const VIDEO_DECODE: &str =
    "--enable-features=AcceleratedVideoDecodeLinuxGL,AcceleratedVideoDecodeLinuxZeroCopyGL,AcceleratedVideoEncoder,VaapiIgnoreDriverChecks";

fn flatpak_installed(id: &str, roots: &[&Path]) -> bool {
    roots.iter().any(|r| r.join(id).join("current").exists() || r.join(id).is_dir())
}

/// The browser to use, if the device has one.
pub fn find() -> Option<Browser> {
    let user = dirs::data_dir().map(|d| d.join("flatpak/app"));
    let system = Path::new("/var/lib/flatpak/app");
    let mut roots: Vec<&Path> = vec![system];
    if let Some(u) = user.as_deref() {
        roots.insert(0, u);
    }
    if super::steam::which("flatpak") {
        if let Some((id, name)) = FLATPAKS.iter().find(|(id, _)| flatpak_installed(id, &roots)) {
            return Some(Browser { name: (*name).into(), program: "flatpak".into(), prefix: vec!["run".into(), (*id).into()] });
        }
    }
    BINARIES.iter().find(|(b, _)| super::steam::which(b)).map(|(b, name)| Browser { name: (*name).into(), program: (*b).into(), prefix: vec![] })
}

/// Arguments that open `url` as a fullscreen app window with its own profile per game, so a
/// game's sign-in stays with that game.
pub fn args(b: &Browser, url: &str, profile_dir: &Path) -> Vec<String> {
    let mut a = b.prefix.clone();
    a.push(format!("--app={url}"));
    a.push("--start-fullscreen".into());
    a.push("--no-first-run".into());
    a.push("--no-default-browser-check".into());
    a.push(VIDEO_DECODE.into());
    // Flatpak browsers can't see arbitrary paths; their own data dir always works.
    if b.program != "flatpak" {
        a.push(format!("--user-data-dir={}", profile_dir.display()));
    } else if let Some(id) = b.prefix.get(1) {
        let slug = profile_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(h) = dirs::home_dir() {
            a.push(format!("--user-data-dir={}", h.join(".var/app").join(id).join("data/rare-webgames").join(slug).display()));
        }
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_args() {
        let b = Browser { name: "Google Chrome".into(), program: "flatpak".into(), prefix: vec!["run".into(), "com.google.Chrome".into()] };
        let a = args(&b, "https://x.io/", Path::new("/h/.local/share/rare/chrome/act"));
        assert_eq!(&a[..3], &["run", "com.google.Chrome", "--app=https://x.io/"]);
        assert!(a.iter().any(|x| x.ends_with("com.google.Chrome/data/rare-webgames/act")));
        assert!(a.iter().any(|x| x.starts_with("--enable-features=") && x.contains("AcceleratedVideoDecodeLinuxGL")));
        let n = Browser { name: "Chromium".into(), program: "chromium".into(), prefix: vec![] };
        assert!(args(&n, "https://x.io/", Path::new("/p/act")).contains(&"--user-data-dir=/p/act".to_string()));
    }
}
