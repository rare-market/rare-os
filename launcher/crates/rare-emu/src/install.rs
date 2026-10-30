//! Download, verify and atomically swap an emulator AppImage. The old file stays as `<name>.bak`.

use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// ELF magic, and the AppImage marker "AI" + type (1 or 2) at offset 8.
pub fn looks_like_appimage(head: &[u8]) -> bool {
    head.len() >= 11 && head[..4] == [0x7f, b'E', b'L', b'F'] && head[8] == b'A' && head[9] == b'I' && (head[10] == 1 || head[10] == 2)
}

/// What the check against the published checksum found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Verified {
    /// The SHA-256 matched the one the release publishes.
    Checksum,
    /// The release publishes no checksum; size and AppImage header were checked only.
    SizeOnly,
}

/// Checks a finished download. `expected_sha` comes from the release (digest or sidecar file).
pub fn verify_file(path: &Path, expected_sha: Option<&str>, expected_size: u64) -> Result<Verified, String> {
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    let mut head = Vec::new();
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if head.len() < 16 {
            head.extend_from_slice(&buf[..n.min(16)]);
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    if expected_size > 0 && total != expected_size {
        return Err(format!("The download is {total} bytes but the release says {expected_size}"));
    }
    if !looks_like_appimage(&head) {
        return Err("The download is not an AppImage".into());
    }
    match expected_sha {
        Some(want) => {
            let got: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
            if got.eq_ignore_ascii_case(want) {
                Ok(Verified::Checksum)
            } else {
                Err("The download does not match the checksum the release publishes".into())
            }
        }
        None => Ok(Verified::SizeOnly),
    }
}

/// Puts `new` in place of `target`: the old file is kept as `<target>.bak` (a hard link, so nothing is
/// copied and the path never stops existing), then `new` is renamed over `target` in one step.
pub fn swap_in(new: &Path, target: &Path) -> std::io::Result<Option<PathBuf>> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(new, fs::Permissions::from_mode(0o755))?;
    let mut bak = None;
    if target.exists() {
        let mut name = target.file_name().unwrap_or_default().to_os_string();
        name.push(".bak");
        let b = target.with_file_name(name);
        let _ = fs::remove_file(&b);
        if fs::hard_link(target, &b).is_err() {
            fs::copy(target, &b)?;
        }
        bak = Some(b);
    }
    fs::rename(new, target)?;
    Ok(bak)
}

/// Streams `url` to `dest`, calling `progress(done, total)`. https only.
pub fn download(url: &str, dest: &Path, mut progress: impl FnMut(u64, u64)) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("Only https downloads are allowed".into());
    }
    let agent = ureq::AgentBuilder::new().timeout_connect(std::time::Duration::from_secs(15)).timeout_read(std::time::Duration::from_secs(60)).build();
    let resp = agent
        .get(url)
        .set("User-Agent", "rare-launcher")
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;
    let total = resp.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut r = resp.into_reader();
    let mut f = File::create(dest).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    let mut last = 0u64;
    loop {
        let n = r.read(&mut buf).map_err(|e| format!("Download failed: {e}"))?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if done - last >= 2 << 20 {
            progress(done, total);
            last = done;
        }
    }
    f.flush().map_err(|e| e.to_string())?;
    progress(done, total);
    Ok(())
}

/// Pulls the first `*.AppImage` out of a .tar.gz into `dest`.
pub fn extract_appimage_tgz(archive: &Path, dest: &Path) -> Result<(), String> {
    let f = File::open(archive).map_err(|e| e.to_string())?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(f));
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut e = entry.map_err(|e| e.to_string())?;
        let is_img = e.path().map(|p| p.to_string_lossy().to_ascii_lowercase().ends_with(".appimage")).unwrap_or(false);
        if is_img {
            let mut out = File::create(dest).map_err(|e| e.to_string())?;
            std::io::copy(&mut e, &mut out).map_err(|e| e.to_string())?;
            return Ok(());
        }
    }
    Err("The archive has no AppImage in it".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(path: &Path, tail: &[u8]) {
        let mut b = vec![0x7f, b'E', b'L', b'F', 2, 1, 1, 0, b'A', b'I', 2, 0];
        b.extend_from_slice(tail);
        fs::write(path, b).unwrap();
    }

    #[test]
    fn verifies_checksum_size_and_header() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.AppImage");
        fake(&p, b"payload");
        let bytes = fs::read(&p).unwrap();
        let sha: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(verify_file(&p, Some(&sha), bytes.len() as u64), Ok(Verified::Checksum));
        assert_eq!(verify_file(&p, None, 0), Ok(Verified::SizeOnly));
        assert!(verify_file(&p, Some(&"0".repeat(64)), 0).unwrap_err().contains("checksum"));
        assert!(verify_file(&p, None, 3).unwrap_err().contains("bytes"));
        fs::write(&p, b"<html>not found</html>").unwrap();
        assert!(verify_file(&p, None, 0).unwrap_err().contains("not an AppImage"));
    }

    #[test]
    fn swap_keeps_the_old_one_as_bak() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("DuckStation.AppImage");
        fake(&target, b"old");
        let new = d.path().join(".DuckStation.AppImage.new");
        fake(&new, b"new");
        let bak = swap_in(&new, &target).unwrap().unwrap();
        assert_eq!(bak.file_name().unwrap(), "DuckStation.AppImage.bak");
        assert!(fs::read(&target).unwrap().ends_with(b"new"));
        assert!(fs::read(&bak).unwrap().ends_with(b"old"));
        assert!(!new.exists());
        // A second update replaces the .bak with the version just before it.
        let new2 = d.path().join(".n2");
        fake(&new2, b"newer");
        swap_in(&new2, &target).unwrap();
        assert!(fs::read(&bak).unwrap().ends_with(b"new"));
        // Fresh install: no old file, no bak.
        let fresh = d.path().join("Cemu.AppImage");
        let n3 = d.path().join(".n3");
        fake(&n3, b"x");
        assert!(swap_in(&n3, &fresh).unwrap().is_none());
    }

    #[test]
    fn extracts_appimage_from_tgz() {
        let d = tempfile::tempdir().unwrap();
        let img = d.path().join("azahar.AppImage");
        fake(&img, b"inside");
        let tgz = d.path().join("a.tar.gz");
        {
            let enc = flate2::write::GzEncoder::new(File::create(&tgz).unwrap(), flate2::Compression::fast());
            let mut b = tar::Builder::new(enc);
            b.append_path_with_name(&img, "azahar/azahar.AppImage").unwrap();
            b.into_inner().unwrap().finish().unwrap();
        }
        let out = d.path().join("out");
        extract_appimage_tgz(&tgz, &out).unwrap();
        assert_eq!(fs::read(&out).unwrap(), fs::read(&img).unwrap());
    }
}
