//! Unpacking a build (`.tar.zst`) and reading its `rare.json`.
//!
//! Every entry is checked before anything is written: only plain relative paths, no `..`,
//! no writing through symlinks, symlinks that stay inside the build, no hardlinks.
//! Device and fifo entries are skipped.

use ruzstd::decoding::errors::{FrameDecoderError, ReadFrameHeaderError};
use ruzstd::decoding::{BlockDecodingStrategy, FrameDecoder};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf};

pub fn unsafe_path(p: &str) -> String {
    format!("The archive has an unsafe path: {p}")
}

/// A zstd stream reader for every frame in the input (the zstd CLI may write several),
/// skipping skippable frames.
pub struct ZstdReader<R: BufRead> {
    src: R,
    dec: FrameDecoder,
    active: bool,
}

impl<R: BufRead> ZstdReader<R> {
    pub fn new(src: R) -> Self {
        ZstdReader { src, dec: FrameDecoder::new(), active: false }
    }
}

impl<R: BufRead> Read for ZstdReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if self.active {
                while self.dec.can_collect() < buf.len() && !self.dec.is_finished() {
                    let want = buf.len() - self.dec.can_collect();
                    self.dec.decode_blocks(&mut self.src, BlockDecodingStrategy::UptoBytes(want)).map_err(io::Error::other)?;
                }
                let n = self.dec.read(buf)?;
                if n > 0 {
                    return Ok(n);
                }
                self.active = false;
            }
            if self.src.fill_buf()?.is_empty() {
                return Ok(0);
            }
            match self.dec.init(&mut self.src) {
                Ok(()) => self.active = true,
                Err(FrameDecoderError::ReadFrameHeaderError(ReadFrameHeaderError::SkipFrame { length, .. })) => {
                    let skipped = io::copy(&mut (&mut self.src).take(u64::from(length)), &mut io::sink())?;
                    if skipped != u64::from(length) {
                        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "truncated zstd frame"));
                    }
                }
                Err(e) => return Err(io::Error::other(e)),
            }
        }
    }
}

/// The plain components of a relative archive path. None for absolute paths, `..`,
/// Windows prefixes and drive letters, and backslashes.
fn plain_parts(p: &Path) -> Option<Vec<OsString>> {
    let mut out = Vec::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::Normal(s) => {
                let t = s.to_string_lossy();
                let drive = out.is_empty() && t.len() >= 2 && t.as_bytes()[1] == b':' && t.as_bytes()[0].is_ascii_alphabetic();
                if t.contains('\\') || drive {
                    return None;
                }
                out.push(s.to_os_string());
            }
            Component::RootDir | Component::Prefix(_) | Component::ParentDir => return None,
        }
    }
    Some(out)
}

/// A symlink at `parts` may point at `link` only when the target stays inside the root:
/// relative, `..` only at the start and never above the root, then plain names.
fn link_ok(parts: &[OsString], link: &Path) -> bool {
    if link.as_os_str().is_empty() {
        return false;
    }
    let mut depth = parts.len() as i64 - 1;
    let mut named = false;
    for c in link.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                depth -= 1;
                if named || depth < 0 {
                    return false;
                }
            }
            Component::Normal(s) => {
                if s.to_string_lossy().contains('\\') {
                    return false;
                }
                named = true;
            }
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    true
}

/// Creates `root/parts...` one real directory at a time. Refuses symlinks and files on the way.
fn ensure_dirs(root: &Path, parts: &[OsString], shown: &str) -> Result<PathBuf, String> {
    let mut p = root.to_path_buf();
    for part in parts {
        p.push(part);
        match fs::symlink_metadata(&p) {
            Ok(m) if m.file_type().is_symlink() || !m.is_dir() => return Err(unsafe_path(shown)),
            Ok(_) => {}
            Err(_) => fs::create_dir(&p).map_err(|e| format!("Couldn't create {}: {e}", p.display()))?,
        }
    }
    Ok(p)
}

/// Clears the way for a file or symlink at `p`: an old file or symlink goes, a directory refuses.
fn clear_leaf(p: &Path, shown: &str) -> Result<(), String> {
    match fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => Err(unsafe_path(shown)),
        Ok(_) => fs::remove_file(p).map_err(|e| e.to_string()),
        Err(_) => Ok(()),
    }
}

fn read_err(e: io::Error) -> String {
    format!("The archive can't be read: {e}")
}

/// Unpacks a tar stream into `dest`, checking every entry first.
pub fn unpack_tar<R: Read>(r: R, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| format!("Couldn't create {}: {e}", dest.display()))?;
    let mut ar = tar::Archive::new(r);
    for entry in ar.entries().map_err(read_err)? {
        let mut e = entry.map_err(read_err)?;
        let raw = String::from_utf8_lossy(&e.path_bytes()).into_owned();
        let path = match e.path() {
            Ok(p) => p.into_owned(),
            Err(_) => return Err(unsafe_path(&raw)),
        };
        let shown = path.to_string_lossy().into_owned();
        let parts = plain_parts(&path).ok_or_else(|| unsafe_path(&shown))?;
        let kind = e.header().entry_type();
        if parts.is_empty() {
            if kind.is_dir() {
                continue;
            }
            return Err(unsafe_path(&shown));
        }
        let (name, parents) = parts.split_last().unwrap();
        match kind {
            tar::EntryType::Directory => {
                ensure_dirs(dest, &parts, &shown)?;
            }
            tar::EntryType::Regular | tar::EntryType::Continuous => {
                let target = ensure_dirs(dest, parents, &shown)?.join(name);
                clear_leaf(&target, &shown)?;
                let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&target).map_err(|e| format!("Couldn't write {}: {e}", target.display()))?;
                io::copy(&mut e, &mut f).map_err(read_err)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = (e.header().mode().unwrap_or(0o644) & 0o777) | 0o600;
                    let _ = fs::set_permissions(&target, fs::Permissions::from_mode(mode));
                }
            }
            tar::EntryType::Symlink => {
                let link = e.link_name().map_err(|_| unsafe_path(&shown))?.ok_or_else(|| unsafe_path(&shown))?.into_owned();
                if !link_ok(&parts, &link) {
                    return Err(unsafe_path(&shown));
                }
                let target = ensure_dirs(dest, parents, &shown)?.join(name);
                clear_leaf(&target, &shown)?;
                #[cfg(unix)]
                std::os::unix::fs::symlink(&link, &target).map_err(|e| format!("Couldn't write {}: {e}", target.display()))?;
            }
            tar::EntryType::Link => return Err(unsafe_path(&shown)),
            // Devices, fifos, sparse files and anything else: not part of a game build.
            _ => {}
        }
    }
    Ok(())
}

/// Unpacks `archive` (`.tar.zst`) into `dest`. On any error `dest` is deleted.
pub fn unpack_tar_zst(archive: &Path, dest: &Path) -> Result<(), String> {
    let r = (|| {
        let f = File::open(archive).map_err(|e| format!("Couldn't open the download: {e}"))?;
        unpack_tar(ZstdReader::new(BufReader::with_capacity(1 << 20, f)), dest)
    })();
    if r.is_err() {
        let _ = fs::remove_dir_all(dest);
    }
    r
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    Native,
    Proton,
}

/// `rare.json` at the root of every build.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RareJson {
    pub name: String,
    pub exec: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub runtime: Runtime,
}

/// Reads and checks `<dir>/rare.json`. The program must be a relative path inside `dir`.
/// Native programs get their exec bit set. Returns the manifest and the program's path.
pub fn read_rare_json(dir: &Path) -> Result<(RareJson, PathBuf), String> {
    let text = fs::read_to_string(dir.join("rare.json")).map_err(|_| "The build has no rare.json.".to_string())?;
    let rj: RareJson = serde_json::from_str(&text).map_err(|e| format!("The build's rare.json can't be read: {e}"))?;
    let program = check_rare_json(dir, &rj)?;
    #[cfg(unix)]
    if rj.runtime == Runtime::Native {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = fs::metadata(&program) {
            let mut p = m.permissions();
            p.set_mode(p.mode() | 0o111);
            fs::set_permissions(&program, p).map_err(|e| format!("Couldn't make {} runnable: {e}", rj.exec))?;
        }
    }
    Ok((rj, program))
}

pub fn check_rare_json(dir: &Path, rj: &RareJson) -> Result<PathBuf, String> {
    if rj.name.trim().is_empty() {
        return Err("The build's rare.json has no name.".into());
    }
    let unsafe_exec = || format!("The build's rare.json names an unsafe program path: {}", rj.exec);
    let parts = plain_parts(Path::new(&rj.exec)).ok_or_else(unsafe_exec)?;
    if parts.is_empty() {
        return Err(unsafe_exec());
    }
    let program: PathBuf = parts.iter().fold(dir.to_path_buf(), |p, c| p.join(c));
    let missing = || format!("The build's program {} isn't in the build.", rj.exec);
    let real = fs::canonicalize(&program).map_err(|_| missing())?;
    let root = fs::canonicalize(dir).map_err(|e| e.to_string())?;
    if !real.starts_with(&root) {
        return Err(unsafe_exec());
    }
    if !real.is_file() {
        return Err(missing());
    }
    Ok(program)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use ruzstd::encoding::{compress_to_vec, CompressionLevel};
    use tar::{Builder, EntryType, Header};

    pub fn zst(data: &[u8]) -> Vec<u8> {
        compress_to_vec(data, CompressionLevel::Fastest)
    }

    fn file(b: &mut Builder<Vec<u8>>, path: &str, data: &[u8], mode: u32) {
        let mut h = Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(mode);
        h.set_entry_type(EntryType::Regular);
        b.append_data(&mut h, path, data).unwrap();
    }

    /// A header with raw name and link bytes (Builder refuses `..` and absolute paths).
    fn raw(b: &mut Builder<Vec<u8>>, name: &str, kind: EntryType, link: &str, data: &[u8]) {
        let mut h = Header::new_gnu();
        h.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
        h.as_old_mut().linkname[..link.len()].copy_from_slice(link.as_bytes());
        h.set_entry_type(kind);
        h.set_size(data.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        b.append(&h, data).unwrap();
    }

    /// A valid native build: rare.json, bin/game, data/levels/1.txt.
    pub fn good_tar() -> Vec<u8> {
        let mut b = Builder::new(Vec::new());
        file(&mut b, "rare.json", br#"{"name":"Test Game","exec":"bin/game","runtime":"native"}"#, 0o644);
        file(&mut b, "bin/game", b"#!/bin/sh\necho hi\n", 0o644);
        file(&mut b, "./data/levels/1.txt", b"level one", 0o644);
        b.into_inner().unwrap()
    }

    fn base(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rare-store-unpack-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn listing(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    #[test]
    fn unpacks_a_good_build() {
        let b = base("good");
        let archive = b.join("build.tar.zst");
        fs::write(&archive, zst(&good_tar())).unwrap();
        let dest = b.join("out");
        unpack_tar_zst(&archive, &dest).unwrap();
        assert_eq!(fs::read_to_string(dest.join("data/levels/1.txt")).unwrap(), "level one");
        let (rj, program) = read_rare_json(&dest).unwrap();
        assert_eq!((rj.name.as_str(), rj.exec.as_str(), rj.args.len(), rj.runtime), ("Test Game", "bin/game", 0, Runtime::Native));
        assert_eq!(program, dest.join("bin/game"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&program).unwrap().permissions().mode() & 0o111, 0o111);
        }
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn reads_every_frame_and_skips_skippable_ones() {
        let tar = good_tar();
        let (a, c) = tar.split_at(1000);
        let mut data = zst(a);
        data.extend_from_slice(&0x184D_2A50u32.to_le_bytes());
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(b"xyz");
        data.extend(zst(c));
        let mut out = Vec::new();
        ZstdReader::new(BufReader::new(&data[..])).read_to_end(&mut out).unwrap();
        assert_eq!(out, tar);
    }

    #[cfg(unix)]
    #[test]
    fn keeps_symlinks_that_stay_inside() {
        let b = base("oklink");
        let mut t = Builder::new(Vec::new());
        file(&mut t, "game.sh", b"x", 0o755);
        raw(&mut t, "bin/run", EntryType::Symlink, "../game.sh", b"");
        let dest = b.join("out");
        unpack_tar(&t.into_inner().unwrap()[..], &dest).unwrap();
        assert_eq!(fs::read_link(dest.join("bin/run")).unwrap(), Path::new("../game.sh"));
        fs::remove_dir_all(&b).unwrap();
    }

    fn rejects(tag: &str, build: impl FnOnce(&mut Builder<Vec<u8>>), shown: &str) {
        let b = base(tag);
        let mut t = Builder::new(Vec::new());
        file(&mut t, "rare.json", b"{}", 0o644);
        build(&mut t);
        let archive = b.join("build.tar.zst");
        fs::write(&archive, zst(&t.into_inner().unwrap())).unwrap();
        let dest = b.join("deep").join("out");
        let err = unpack_tar_zst(&archive, &dest).unwrap_err();
        assert_eq!(err, unsafe_path(shown), "{tag}");
        assert!(!dest.exists(), "{tag}: partial dir left behind");
        assert_eq!(listing(&b), vec!["build.tar.zst", "deep"], "{tag}");
        assert!(listing(&b.join("deep")).is_empty(), "{tag}");
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn rejects_unsafe_entries() {
        let abs = format!("/tmp/rare-store-abs-{}", std::process::id());
        rejects("dotdot", |t| raw(t, "../evil", EntryType::Regular, "", b"x"), "../evil");
        rejects("abs", |t| raw(t, &abs, EntryType::Regular, "", b"x"), &abs);
        assert!(!Path::new(&abs).exists());
        rejects("inner", |t| raw(t, "a/../../x", EntryType::Regular, "", b"x"), "a/../../x");
        rejects("linkup", |t| raw(t, "link", EntryType::Symlink, "../../outside", b""), "link");
        rejects("linkabs", |t| raw(t, "etc", EntryType::Symlink, "/etc", b""), "etc");
        rejects("linkmid", |t| raw(t, "a/link", EntryType::Symlink, "b/../../..", b""), "a/link");
        rejects("hardlink", |t| raw(t, "hard", EntryType::Link, "rare.json", b""), "hard");
        rejects("drive", |t| raw(t, "C:\\evil", EntryType::Regular, "", b"x"), "C:\\evil");
        #[cfg(unix)]
        rejects(
            "through",
            |t| {
                raw(t, "d", EntryType::Symlink, ".", b"");
                raw(t, "d/f", EntryType::Regular, "", b"x");
            },
            "d/f",
        );
    }

    #[test]
    fn skips_fifos() {
        let b = base("fifo");
        let mut t = Builder::new(Vec::new());
        raw(&mut t, "pipe", EntryType::Fifo, "", b"");
        file(&mut t, "rare.json", b"{}", 0o644);
        let dest = b.join("out");
        unpack_tar(&t.into_inner().unwrap()[..], &dest).unwrap();
        assert_eq!(listing(&dest), vec!["rare.json"]);
        fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn checks_rare_json() {
        let b = base("rj");
        fs::create_dir_all(b.join("bin")).unwrap();
        fs::write(b.join("bin/game.exe"), b"MZ").unwrap();
        let rj = |exec: &str| RareJson { name: "G".into(), exec: exec.into(), args: vec![], runtime: Runtime::Proton };
        assert!(check_rare_json(&b, &rj("bin/game.exe")).is_ok());
        assert!(check_rare_json(&b, &rj("./bin/game.exe")).is_ok());
        assert!(check_rare_json(&b, &rj("../bin/game.exe")).unwrap_err().contains("unsafe"));
        assert!(check_rare_json(&b, &rj("/bin/sh")).unwrap_err().contains("unsafe"));
        assert!(check_rare_json(&b, &rj("bin/../bin/game.exe")).unwrap_err().contains("unsafe"));
        assert!(check_rare_json(&b, &rj("")).unwrap_err().contains("unsafe"));
        assert!(check_rare_json(&b, &rj("bin")).unwrap_err().contains("isn't in the build"));
        assert!(check_rare_json(&b, &rj("bin/missing")).unwrap_err().contains("isn't in the build"));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/bin/sh", b.join("bin/out")).unwrap();
            assert!(check_rare_json(&b, &rj("bin/out")).unwrap_err().contains("unsafe"));
        }
        let mut no_name = rj("bin/game.exe");
        no_name.name = " ".into();
        assert!(check_rare_json(&b, &no_name).is_err());
        fs::write(b.join("rare.json"), r#"{"name":"G","exec":"bin/game.exe","runtime":"dos"}"#).unwrap();
        assert!(read_rare_json(&b).unwrap_err().contains("can't be read"));
        fs::write(b.join("rare.json"), r#"{"name":"G","exec":"bin/game.exe","args":["-x"],"runtime":"proton"}"#).unwrap();
        assert_eq!(read_rare_json(&b).unwrap().0.args, vec!["-x"]);
        fs::remove_dir_all(&b).unwrap();
    }
}
