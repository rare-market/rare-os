//! Clip playback for the capture page (ShareScreen's one `<video muted loop autoPlay>`) and the
//! clip poster (share/poster.ts): GStreamer on the device, `filesrc ! decodebin ! videoconvert !
//! videoscale ! video/x-raw,format=RGBA ! appsink` (decodebin picks the VA-API decoders), the
//! newest frame kept for the page to upload as a texture. Audio is never played: any non-video
//! pad of decodebin goes to a `fakesink`, like the web's muted video.
//!
//! Built only on Linux with the `share-video` feature (GStreamer is on the device image, not on
//! the Mac or the CI runner); everywhere else `Player::open` fails and the page keeps the poster.

use std::path::{Path, PathBuf};

/// One decoded frame, tightly packed RGBA.
#[derive(Clone, Debug)]
pub struct Frame {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// Where a clip's poster is cached (PNG; the native build has no JPEG encoder).
pub fn poster_cache(id: &str) -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache"))).unwrap_or_else(std::env::temp_dir);
    base.join("rare/share/posters").join(format!("{id}.png"))
}

/// Writes a poster frame as PNG.
pub fn save_png(f: &Frame, path: &Path) -> Result<(), String> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    image::save_buffer(path, &f.rgba, f.w, f.h, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())
}

/// A clip's poster as a cached PNG (poster.ts: a frame about 1 s in, at most 1280 px on the long
/// edge). Reuses the cache.
pub fn poster_thumb(clip: &Path, id: &str) -> Result<PathBuf, String> {
    let out = poster_cache(id);
    if out.exists() {
        return Ok(out);
    }
    let f = grab_poster(clip, 1280)?;
    save_png(&f, &out)?;
    Ok(out)
}

/// Long edge at most `max` (fitWithin), CPU resize.
pub fn fit_frame(f: Frame, max: u32) -> Frame {
    let (w, h) = super::format::fit_within(f.w, f.h, max);
    if (w, h) == (f.w, f.h) {
        return f;
    }
    let Some(img) = image::RgbaImage::from_raw(f.w, f.h, f.rgba) else { return Frame { w: 0, h: 0, rgba: Vec::new() } };
    let r = image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle);
    Frame { w, h, rgba: r.into_raw() }
}

#[cfg(all(target_os = "linux", feature = "share-video"))]
mod imp {
    use super::Frame;
    use gst::prelude::*;
    use gst_video::prelude::*;
    use gstreamer as gst;
    use gstreamer_app as gst_app;
    use gstreamer_video as gst_video;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    type Slot = Arc<Mutex<(u64, Option<Frame>)>>;

    pub struct Player {
        pipeline: gst::Pipeline,
        latest: Slot,
        seen: u64,
        error: Option<String>,
    }

    fn err(e: impl std::fmt::Display) -> String {
        e.to_string()
    }

    fn make(name: &str) -> Result<gst::Element, String> {
        gst::ElementFactory::make(name).build().map_err(|e| format!("{name}: {e}"))
    }

    /// Copies a mapped RGBA frame without its row padding.
    fn frame_of(sample: &gst::Sample) -> Option<Frame> {
        let buffer = sample.buffer()?;
        let info = gst_video::VideoInfo::from_caps(sample.caps()?).ok()?;
        let frame = gst_video::VideoFrameRef::from_buffer_ref_readable(buffer, &info).ok()?;
        let (w, h) = (info.width(), info.height());
        let stride = *frame.plane_stride().first()? as usize;
        let data = frame.plane_data(0).ok()?;
        let row = w as usize * 4;
        let mut rgba = Vec::with_capacity(row * h as usize);
        for y in 0..h as usize {
            rgba.extend_from_slice(data.get(y * stride..y * stride + row)?);
        }
        Some(Frame { w, h, rgba })
    }

    /// filesrc ! decodebin, video into `convert`, everything else (audio) into a fakesink.
    fn source(pipeline: &gst::Pipeline, path: &Path, convert: &gst::Element) -> Result<(), String> {
        let src = gst::ElementFactory::make("filesrc").property("location", path.to_string_lossy().to_string()).build().map_err(err)?;
        let decode = make("decodebin")?;
        pipeline.add_many([&src, &decode]).map_err(err)?;
        src.link(&decode).map_err(err)?;
        let pl = pipeline.downgrade();
        let conv = convert.downgrade();
        decode.connect_pad_added(move |_, pad| {
            let caps = pad.current_caps().unwrap_or_else(|| pad.query_caps(None));
            let video = caps.structure(0).is_some_and(|s| s.name().as_str().starts_with("video/"));
            if video {
                if let Some(sink) = conv.upgrade().and_then(|c| c.static_pad("sink")) {
                    if !sink.is_linked() {
                        let _ = pad.link(&sink);
                    }
                }
                return;
            }
            // Muted, always: audio is decoded into nothing, never to a device.
            let Some(pipeline) = pl.upgrade() else { return };
            if let Ok(fake) = gst::ElementFactory::make("fakesink").property("sync", false).build() {
                if pipeline.add(&fake).is_ok() {
                    let _ = fake.sync_state_with_parent();
                    if let Some(sp) = fake.static_pad("sink") {
                        let _ = pad.link(&sp);
                    }
                }
            }
        });
        Ok(())
    }

    /// videoconvert ! videoscale ! RGBA caps (scaled down to `max_w` wide, aspect kept) ! appsink.
    fn tail(pipeline: &gst::Pipeline, max_w: Option<u32>, sync: bool) -> Result<(gst::Element, gst_app::AppSink), String> {
        let convert = make("videoconvert")?;
        let scale = make("videoscale")?;
        let mut caps = gst_video::VideoCapsBuilder::new().format(gst_video::VideoFormat::Rgba).pixel_aspect_ratio(gst::Fraction::new(1, 1));
        if let Some(w) = max_w {
            caps = caps.width(w as i32);
        }
        let sink = gst_app::AppSink::builder().caps(&caps.build()).max_buffers(2).drop(true).sync(sync).build();
        pipeline.add_many([&convert, &scale, sink.upcast_ref()]).map_err(err)?;
        gst::Element::link_many([&convert, &scale, sink.upcast_ref()]).map_err(err)?;
        Ok((convert, sink))
    }

    impl Player {
        /// Starts playing `path` muted and looping, frames at most `max_w` px wide.
        pub fn open(path: &Path, max_w: Option<u32>) -> Result<Player, String> {
            gst::init().map_err(err)?;
            let pipeline = gst::Pipeline::new();
            let (convert, sink) = tail(&pipeline, max_w, true)?;
            source(&pipeline, path, &convert)?;
            Self::run(pipeline, sink)
        }

        /// A pipeline description ending in `appsink name=sink` (tests: videotestsrc, no audio
        /// device anywhere).
        pub fn launch(desc: &str) -> Result<Player, String> {
            gst::init().map_err(err)?;
            let pipeline = gst::parse::launch(desc).map_err(err)?.downcast::<gst::Pipeline>().map_err(|_| "not a pipeline".to_string())?;
            let sink = pipeline.by_name("sink").and_then(|s| s.downcast::<gst_app::AppSink>().ok()).ok_or("no appsink named sink")?;
            Self::run(pipeline, sink)
        }

        fn run(pipeline: gst::Pipeline, sink: gst_app::AppSink) -> Result<Player, String> {
            let latest: Slot = Arc::new(Mutex::new((0, None)));
            let slot = latest.clone();
            sink.set_callbacks(
                gst_app::AppSinkCallbacks::builder()
                    .new_sample(move |s| {
                        let sample = s.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                        if let Some(f) = frame_of(&sample) {
                            let mut g = slot.lock().unwrap();
                            g.0 += 1;
                            g.1 = Some(f);
                        }
                        Ok(gst::FlowSuccess::Ok)
                    })
                    .build(),
            );
            pipeline.set_state(gst::State::Playing).map_err(err)?;
            Ok(Player { pipeline, latest, seen: 0, error: None })
        }

        /// Bus messages: loops at the end (`loop`), keeps the first error.
        pub fn poll(&mut self) {
            let Some(bus) = self.pipeline.bus() else { return };
            while let Some(msg) = bus.pop() {
                match msg.view() {
                    gst::MessageView::Eos(..) => {
                        let _ = self.pipeline.seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT, gst::ClockTime::ZERO);
                    }
                    gst::MessageView::Error(e) => {
                        if self.error.is_none() {
                            self.error = Some(e.error().to_string());
                        }
                    }
                    _ => {}
                }
            }
        }

        /// The newest frame since the last call, if any.
        pub fn take_frame(&mut self) -> Option<Frame> {
            let mut g = self.latest.lock().unwrap();
            if g.0 == self.seen {
                return None;
            }
            self.seen = g.0;
            g.1.take()
        }

        pub fn error(&self) -> Option<&str> {
            self.error.as_deref()
        }
    }

    impl Drop for Player {
        fn drop(&mut self) {
            let _ = self.pipeline.set_state(gst::State::Null);
        }
    }

    /// poster.ts grabPoster: preroll paused, seek to posterTime, pull that frame, fit to `max_edge`.
    pub fn grab_poster(path: &Path, max_edge: u32) -> Result<Frame, String> {
        gst::init().map_err(err)?;
        let pipeline = gst::Pipeline::new();
        let (convert, sink) = tail(&pipeline, None, false)?;
        source(&pipeline, path, &convert)?;
        let done = |p: &gst::Pipeline| {
            let _ = p.set_state(gst::State::Null);
        };
        pipeline.set_state(gst::State::Paused).map_err(err)?;
        let (res, _, _) = pipeline.state(gst::ClockTime::from_seconds(10));
        if res.is_err() {
            done(&pipeline);
            return Err("This clip can't be read".into());
        }
        let secs = pipeline.query_duration::<gst::ClockTime>().map(|d| d.nseconds() as f64 / 1e9).unwrap_or(f64::NAN);
        let at = super::super::format::poster_time(secs);
        if at > 0.0 {
            let _ = pipeline.seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE, gst::ClockTime::from_nseconds((at * 1e9) as u64));
            let _ = pipeline.state(gst::ClockTime::from_seconds(10));
        }
        let sample = sink.try_pull_preroll(gst::ClockTime::from_seconds(10));
        done(&pipeline);
        let f = sample.as_ref().and_then(frame_of).ok_or("Timed out reading the clip")?;
        Ok(super::fit_frame(f, max_edge))
    }
}

#[cfg(not(all(target_os = "linux", feature = "share-video")))]
mod imp {
    use super::Frame;
    use std::path::Path;

    const OFF: &str = "Clip playback needs GStreamer (the share-video feature)";

    /// Without GStreamer: never opens, so the page shows the poster.
    pub struct Player;

    impl Player {
        pub fn open(_path: &Path, _max_w: Option<u32>) -> Result<Player, String> {
            Err(OFF.into())
        }
        pub fn launch(_desc: &str) -> Result<Player, String> {
            Err(OFF.into())
        }
        pub fn poll(&mut self) {}
        pub fn take_frame(&mut self) -> Option<Frame> {
            None
        }
        pub fn error(&self) -> Option<&str> {
            None
        }
    }

    pub fn grab_poster(_path: &Path, _max_edge: u32) -> Result<Frame, String> {
        Err(OFF.into())
    }
}

pub use imp::{grab_poster, Player};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_fit_the_poster_size() {
        let f = Frame { w: 1920, h: 1080, rgba: vec![255; 1920 * 1080 * 4] };
        let g = fit_frame(f, 1280);
        assert_eq!((g.w, g.h, g.rgba.len()), (1280, 720, 1280 * 720 * 4));
        let s = fit_frame(Frame { w: 64, h: 36, rgba: vec![0; 64 * 36 * 4] }, 1280);
        assert_eq!((s.w, s.h), (64, 36));
    }

    #[test]
    fn posters_cache_by_id() {
        assert!(poster_cache("clip-20261027-101500-000").ends_with("rare/share/posters/clip-20261027-101500-000.png"));
    }

    #[cfg(not(all(target_os = "linux", feature = "share-video")))]
    #[test]
    fn without_gstreamer_the_page_keeps_the_poster() {
        assert!(Player::open(Path::new("/nope.mp4"), None).is_err());
        assert!(grab_poster(Path::new("/nope.mp4"), 1280).is_err());
    }

    /// A test pattern through the same appsink tail; audio, if any, would go to a fakesink. No
    /// sound device is ever opened.
    #[cfg(all(target_os = "linux", feature = "share-video"))]
    #[test]
    fn frames_arrive_from_gstreamer() {
        let mut p = match Player::launch("videotestsrc num-buffers=20 ! video/x-raw,width=64,height=36,framerate=30/1 ! videoconvert ! video/x-raw,format=RGBA ! appsink name=sink sync=false max-buffers=2 drop=true audiotestsrc num-buffers=5 ! fakesink") {
            Ok(p) => p,
            Err(e) => panic!("{e}"),
        };
        let t = std::time::Instant::now();
        let mut got = None;
        while t.elapsed() < std::time::Duration::from_secs(10) {
            p.poll();
            if let Some(f) = p.take_frame() {
                got = Some(f);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let f = got.expect("a frame");
        assert_eq!((f.w, f.h, f.rgba.len()), (64, 36, 64 * 36 * 4));
        assert!(p.error().is_none());
    }
}
