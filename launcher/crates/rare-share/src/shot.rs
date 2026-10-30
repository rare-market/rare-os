//! Screenshots: grab a frame with the first backend that works, then save it as a lossy WebP.
//!
//! Rotation rule: the SuiPlay's panel is 1200x1920 portrait, mounted sideways, and gamescope
//! shows games landscape. Grabs taken from gamescope's composite are already landscape. A grab
//! that comes from the raw scanout (KMS capture in gpu-screen-recorder, older gamescope) is
//! portrait. So in Game Mode, a frame taller than it is wide is turned to landscape: with
//! gamescope's `--force-orientation right` (or none given, the SuiPlay default) it is turned
//! 90 degrees counter-clockwise, with `left` 90 degrees clockwise. Desktop Mode grabs are
//! never turned (KWin already hands out the logical, rotated image). RARE_SHARE_ROTATE=cw,
//! ccw or none overrides the rule.

use super::tools::{self, Probe, Session, ShotBackend};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

pub const MAX_EDGE: u32 = 1920;
pub const QUALITY: f32 = 82.0;
/// Above this the WebP is encoded again at a lower quality.
const SOFT_LIMIT: usize = 1_400_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotate {
    None,
    Cw,
    Ccw,
}

/// See the module comment.
pub fn rotation_for(session: Session, w: u32, h: u32, orientation: Option<&str>, override_: Option<&str>) -> Rotate {
    match override_.map(|s| s.trim().to_lowercase()).as_deref() {
        Some("cw") => return Rotate::Cw,
        Some("ccw") => return Rotate::Ccw,
        Some("none") => return Rotate::None,
        _ => {}
    }
    if session != Session::Gamescope || h <= w {
        return Rotate::None;
    }
    match orientation {
        Some("left") => Rotate::Cw,
        Some("normal") | Some("upsidedown") => Rotate::None,
        _ => Rotate::Ccw,
    }
}

pub struct Encoded {
    pub webp: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// PNG or JPEG bytes to a WebP: turned per `rot`, long edge capped at MAX_EDGE.
pub fn to_webp(data: &[u8], rot: Rotate) -> Result<Encoded, String> {
    let img = image::load_from_memory(data).map_err(|e| format!("decode: {e}"))?;
    let img = match rot {
        Rotate::None => img,
        Rotate::Cw => img.rotate90(),
        Rotate::Ccw => img.rotate270(),
    };
    let img = if img.width().max(img.height()) > MAX_EDGE { img.resize(MAX_EDGE, MAX_EDGE, image::imageops::FilterType::Triangle) } else { img };
    let rgb = img.to_rgb8();
    let (width, height) = rgb.dimensions();
    let enc = webp::Encoder::from_rgb(rgb.as_raw(), width, height);
    let mut out: Vec<u8> = Vec::new();
    for q in [QUALITY, 72.0, 62.0] {
        out = enc.encode_simple(false, q).map_err(|e| format!("webp: {e:?}"))?.to_vec();
        if out.len() <= SOFT_LIMIT {
            break;
        }
    }
    Ok(Encoded { webp: out, width, height })
}

/// Width and height of a WebP file's header.
pub fn webp_size(data: &[u8]) -> Option<(u32, u32)> {
    let f = webp::BitstreamFeatures::new(data)?;
    Some((f.width(), f.height()))
}

/// True once a PNG or JPEG file has been written out completely.
fn complete(b: &[u8]) -> bool {
    if b.starts_with(b"\x89PNG") {
        return b.len() > 12 && b[b.len() - 12..].windows(4).any(|w| w == b"IEND");
    }
    if b.starts_with(&[0xFF, 0xD8]) {
        return b.ends_with(&[0xFF, 0xD9]);
    }
    false
}

/// Waits for a complete image at `path`, written after `since`.
fn wait_file(path: &Path, since: SystemTime, timeout: Duration) -> Option<Vec<u8>> {
    let t0 = Instant::now();
    while t0.elapsed() < timeout {
        let fresh = std::fs::metadata(path).and_then(|m| m.modified()).map(|m| m + Duration::from_secs(1) >= since).unwrap_or(false);
        if fresh {
            if let Ok(b) = std::fs::read(path) {
                if complete(&b) {
                    return Some(b);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    None
}

/// /tmp/gamescope*.png files and their mtimes (where gamescope's property screenshot lands:
/// /tmp/gamescope.png now, /tmp/gamescope_<date>.png in older versions).
fn gamescope_tmp_pngs() -> Vec<(PathBuf, SystemTime)> {
    let Ok(rd) = std::fs::read_dir("/tmp") else { return vec![] };
    rd.flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with("gamescope") && n.ends_with(".png")
        })
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
        .collect()
}

/// Asks gamescope for a screenshot by setting GAMESCOPECTRL_REQUEST_SCREENSHOT on the root
/// window of its Xwayland. The value is the screenshot type; 0 is "all real layers" (the game
/// and its overlays as composited, without the mura correction layer).
#[cfg(target_os = "linux")]
fn gamescope_property(display: &str) -> Result<(), String> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, PropMode};
    use x11rb::wrapper::ConnectionExt as _;
    let (conn, screen) = x11rb::connect(Some(display)).map_err(|e| format!("x11 {display}: {e}"))?;
    let root = conn.setup().roots.get(screen).ok_or("no x11 screen")?.root;
    let atom = conn.intern_atom(false, b"GAMESCOPECTRL_REQUEST_SCREENSHOT").map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?.atom;
    conn.change_property32(PropMode::REPLACE, root, atom, AtomEnum::CARDINAL, &[0]).map_err(|e| e.to_string())?;
    // A round trip, so the request has reached the server before the connection closes.
    conn.get_input_focus().map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn gamescope_property(_display: &str) -> Result<(), String> {
    Err("gamescope runs on Linux only".into())
}

/// One grab with one backend. Returns PNG or JPEG bytes.
pub fn grab(b: ShotBackend, p: &Probe, tmp: &Path) -> Result<Vec<u8>, String> {
    let _ = std::fs::remove_file(tmp);
    let tmp_s = tmp.to_string_lossy().into_owned();
    let since = SystemTime::now();
    let need = |o: Option<&PathBuf>| o.cloned().ok_or_else(|| format!("{} is missing", b.name()));
    match b {
        ShotBackend::GamescopeCtl => {
            let ctl = need(p.gamescopectl.as_ref())?;
            let o = tools::run(&ctl, &["screenshot".into(), tmp_s, "0".into()], Duration::from_secs(3))?;
            if !o.ok() {
                return Err(format!("gamescopectl: {}", o.stderr.trim()));
            }
            wait_file(tmp, since, Duration::from_secs(4)).ok_or_else(|| "gamescope wrote no screenshot".into())
        }
        ShotBackend::GamescopeProp => {
            let before = gamescope_tmp_pngs();
            gamescope_property(p.x_display.as_deref().unwrap_or(":0"))?;
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_secs(4) {
                std::thread::sleep(Duration::from_millis(80));
                let fresh = gamescope_tmp_pngs().into_iter().find(|(path, m)| *m + Duration::from_secs(1) >= since && !before.iter().any(|(bp, bm)| bp == path && bm == m));
                if let Some((path, _)) = fresh {
                    if let Some(bytes) = std::fs::read(&path).ok().filter(|b| complete(b)) {
                        let _ = std::fs::remove_file(&path);
                        return Ok(bytes);
                    }
                }
            }
            Err("gamescope wrote no screenshot".into())
        }
        ShotBackend::Gsr => {
            let g = p.gsr.as_ref().ok_or("gpu-screen-recorder is missing")?;
            let o = tools::run(&g.path, &["-w".into(), "screen".into(), "-o".into(), tmp_s], Duration::from_secs(8))?;
            if !o.ok() {
                return Err(format!("gpu-screen-recorder: {}", o.stderr.trim()));
            }
            wait_file(tmp, since, Duration::from_secs(2)).ok_or_else(|| "gpu-screen-recorder wrote no screenshot".into())
        }
        ShotBackend::Gst => {
            let g = p.gst.as_ref().ok_or("gstreamer is missing")?;
            let node = tools::pw_gamescope_node().or_else(|| p.pw_node.clone()).ok_or("no gamescope video node")?;
            let o = tools::run(&g.launch, &super::clip::gst_snap_args(g, &node, tmp), Duration::from_secs(6))?;
            if !o.ok() {
                return Err(format!("gst-launch: {}", o.stderr.trim()));
            }
            wait_file(tmp, since, Duration::from_secs(1)).ok_or_else(|| "gstreamer wrote no screenshot".into())
        }
        ShotBackend::Spectacle => {
            let sp = need(p.spectacle.as_ref())?;
            let o = tools::run(&sp, &["-b".into(), "-n".into(), "-f".into(), "-o".into(), tmp_s], Duration::from_secs(10))?;
            if !o.ok() {
                return Err(format!("spectacle: {}", o.stderr.trim()));
            }
            wait_file(tmp, since, Duration::from_secs(3)).ok_or_else(|| "spectacle wrote no screenshot".into())
        }
        ShotBackend::Portal => {
            #[cfg(target_os = "linux")]
            {
                let path = super::portal::screenshot(Duration::from_secs(8))?;
                let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                // The portal saves a file of its own; remove it when it is a temporary one.
                let tmpdirs = [std::env::temp_dir(), PathBuf::from("/tmp"), std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_default()];
                if tmpdirs.iter().any(|d| !d.as_os_str().is_empty() && path.starts_with(d)) {
                    let _ = std::fs::remove_file(&path);
                }
                Ok(bytes)
            }
            #[cfg(not(target_os = "linux"))]
            Err("the screenshot portal is Linux only".into())
        }
        ShotBackend::ScreenCapture => {
            let sc = need(p.screencapture.as_ref())?;
            let o = tools::run(&sc, &["-x".into(), "-t".into(), "png".into(), tmp_s], Duration::from_secs(8))?;
            if !o.ok() {
                return Err(format!("screencapture: {}", o.stderr.trim()));
            }
            wait_file(tmp, since, Duration::from_secs(2)).ok_or_else(|| "screencapture wrote no file".into())
        }
    }
}

/// Grabs a frame with the first backend that works. Returns the bytes and the method used.
pub fn grab_any(p: &Probe, tmp: &Path) -> Result<(Vec<u8>, ShotBackend), String> {
    let list = tools::shot_backends(p);
    if list.is_empty() {
        return Err("Screenshots are not available here".into());
    }
    for b in list {
        let t0 = Instant::now();
        match grab(b, p, tmp) {
            Ok(bytes) => {
                eprintln!("[share] screenshot by {} in {} ms", b.name(), t0.elapsed().as_millis());
                let _ = std::fs::remove_file(tmp);
                return Ok((bytes, b));
            }
            Err(e) => eprintln!("[share] screenshot by {} failed: {e}", b.name()),
        }
    }
    let _ = std::fs::remove_file(tmp);
    Err("Could not take a screenshot".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_rule() {
        let g = Session::Gamescope;
        assert_eq!(rotation_for(g, 1200, 1920, Some("right"), None), Rotate::Ccw);
        assert_eq!(rotation_for(g, 1200, 1920, None, None), Rotate::Ccw);
        assert_eq!(rotation_for(g, 1200, 1920, Some("left"), None), Rotate::Cw);
        assert_eq!(rotation_for(g, 1920, 1200, Some("right"), None), Rotate::None);
        assert_eq!(rotation_for(Session::Kde, 1200, 1920, Some("right"), None), Rotate::None);
        assert_eq!(rotation_for(Session::Kde, 1920, 1200, None, Some("cw")), Rotate::Cw);
        assert_eq!(rotation_for(g, 1200, 1920, None, Some("none")), Rotate::None);
    }

    /// A portrait test frame: red block in the top-right corner, blue elsewhere.
    fn portrait_png() -> Vec<u8> {
        let img = image::RgbImage::from_fn(1200, 1920, |x, y| if x >= 1000 && y < 200 { image::Rgb([255, 0, 0]) } else { image::Rgb([0, 0, 255]) });
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn webp_turns_portrait_to_landscape() {
        let png = portrait_png();
        assert!(complete(&png));
        assert!(!complete(&png[..png.len() - 20]));
        let e = to_webp(&png, Rotate::Ccw).unwrap();
        assert_eq!((e.width, e.height), (1920, 1200));
        assert_eq!(webp_size(&e.webp), Some((1920, 1200)));
        assert!(e.webp.len() < 1_000_000);
        // Counter-clockwise: the portrait top-right corner is now the top-left one.
        let d = webp::Decoder::new(&e.webp).decode().unwrap();
        let px = |x: u32, y: u32| {
            let bpp = if d.is_alpha() { 4 } else { 3 };
            let i = ((y * d.width() + x) * bpp) as usize;
            (d[i], d[i + 1], d[i + 2])
        };
        let (r, _, b) = px(50, 50);
        assert!(r > 200 && b < 60, "top-left should be red, got {:?}", px(50, 50));
        let (r, _, b) = px(1900, 1180);
        assert!(r < 60 && b > 200);
    }

    #[test]
    fn webp_caps_the_long_edge() {
        let img = image::RgbImage::from_pixel(3024, 1964, image::Rgb([20, 120, 40]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
        let e = to_webp(&out.into_inner(), Rotate::None).unwrap();
        assert_eq!(e.width, 1920);
        assert!((1246..=1248).contains(&e.height), "{}", e.height);
        assert!(to_webp(b"nope", Rotate::None).is_err());
    }
}
