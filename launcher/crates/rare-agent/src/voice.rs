//! Push to talk, all on the device: PipeWire records the microphone while the button is held,
//! whisper.cpp (the small multilingual model, on the CPU) turns it into text. The model (190 MB) is
//! downloaded once on first use, checked against its SHA-256, and kept in Rare's data folder.
//! Nothing is sent anywhere for speech.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const MODEL_FILE: &str = "ggml-small-q5_1.bin";
pub const MODEL_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin";
pub const MODEL_SHA256: &str = "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb";
pub const MODEL_BYTES: u64 = 190_085_487;
/// Recordings stop on their own after this.
pub const MAX_RECORD: Duration = Duration::from_secs(30);
/// Shorter than this is a tap, not speech.
pub const MIN_SAMPLES: usize = 16_000 / 3;

pub fn model_path(dir: &Path) -> PathBuf {
    dir.join(MODEL_FILE)
}

/// The model is there (size checked; the hash was checked when it was downloaded).
pub fn have_model(dir: &Path) -> bool {
    std::fs::metadata(model_path(dir)).map(|m| m.len() == MODEL_BYTES).unwrap_or(false)
}

/// Downloads the model into `dir` (to a .part file, renamed once its hash matches). `progress(done,
/// total)` follows it. Blocks; run it on a worker.
pub fn download(dir: &Path, progress: &dyn Fn(u64, u64)) -> Result<PathBuf, String> {
    let to = model_path(dir);
    if have_model(dir) {
        return Ok(to);
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't make the folder for the speech model: {e}"))?;
    let part = to.with_extension("part");
    let resp = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout_read(Duration::from_secs(60)).build().get(MODEL_URL).call().map_err(|e| format!("Couldn't download the speech model: {e}"))?;
    let mut r = resp.into_reader();
    let mut f = std::fs::File::create(&part).map_err(|e| format!("Couldn't save the speech model: {e}"))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        let n = r.read(&mut buf).map_err(|e| format!("The download stopped: {e}"))?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n]).map_err(|e| format!("Couldn't save the speech model: {e}"))?;
        h.update(&buf[..n]);
        done += n as u64;
        progress(done, MODEL_BYTES);
    }
    drop(f);
    let got: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if got != MODEL_SHA256 {
        let _ = std::fs::remove_file(&part);
        return Err("The speech model didn't download correctly. Try again.".into());
    }
    std::fs::rename(&part, &to).map_err(|e| format!("Couldn't save the speech model: {e}"))?;
    Ok(to)
}

/// The microphone while it records (pw-record to a 16 kHz mono WAV).
pub struct Recorder {
    child: Child,
    path: PathBuf,
    started: Instant,
}

impl Recorder {
    pub fn start(dir: &Path) -> Result<Recorder, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let path = dir.join("ask.wav");
        let _ = std::fs::remove_file(&path);
        let child = Command::new("pw-record")
            .args(["--rate", "16000", "--channels", "1", "--format", "s16"])
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "The microphone isn't available.".to_string())?;
        Ok(Recorder { child, path, started: Instant::now() })
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Stops and returns the samples (16 kHz mono, -1..1).
    pub fn stop(mut self) -> Result<Vec<f32>, String> {
        // SIGINT lets pw-record finish the WAV header.
        let _ = Command::new("kill").args(["-INT", &self.child.id().to_string()]).status();
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(2) {
            if let Ok(Some(_)) = self.child.try_wait() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let bytes = std::fs::read(&self.path).map_err(|_| "Nothing was recorded.".to_string())?;
        let _ = std::fs::remove_file(&self.path);
        wav_samples(&bytes)
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// 16-bit PCM samples from a WAV (the data chunk; a header pw-record could not finish is read to
/// the end of the file).
pub fn wav_samples(b: &[u8]) -> Result<Vec<f32>, String> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("The recording is unreadable.".into());
    }
    let mut i = 12;
    let mut channels = 1u16;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let body = i + 8;
        if id == b"fmt " && body + 4 <= b.len() {
            channels = u16::from_le_bytes([b[body + 2], b[body + 3]]).max(1);
        }
        if id == b"data" {
            let end = if len == 0 || len == u32::MAX as usize || body + len > b.len() { b.len() } else { body + len };
            let pcm: Vec<f32> = b[body..end].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect();
            return Ok(if channels > 1 { pcm.chunks(channels as usize).map(|c| c.iter().sum::<f32>() / c.len() as f32).collect() } else { pcm });
        }
        i = body + len + (len & 1);
    }
    Err("The recording is empty.".into())
}

/// Text from 16 kHz mono samples, any language whisper knows. Blocks (a few seconds on the CPU).
#[cfg(target_os = "linux")]
pub fn transcribe(model: &Path, pcm: &[f32]) -> Result<String, String> {
    use std::sync::Mutex;
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
    static CTX: Mutex<Option<(PathBuf, WhisperContext)>> = Mutex::new(None);
    if pcm.len() < MIN_SAMPLES {
        return Ok(String::new());
    }
    let mut g = CTX.lock().map_err(|_| "Speech is busy.".to_string())?;
    if g.as_ref().map(|(p, _)| p.as_path()) != Some(model) {
        let ctx = WhisperContext::new_with_params(model, WhisperContextParameters::default()).map_err(|_| "The speech model couldn't load.".to_string())?;
        *g = Some((model.to_path_buf(), ctx));
    }
    let (_, ctx) = g.as_ref().unwrap();
    let mut state = ctx.create_state().map_err(|_| "The speech model couldn't start.".to_string())?;
    let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    p.set_language(Some("auto"));
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 6);
    p.set_n_threads(threads as i32);
    p.set_print_special(false);
    p.set_print_progress(false);
    p.set_print_realtime(false);
    p.set_print_timestamps(false);
    p.set_suppress_blank(true);
    p.set_no_context(true);
    p.set_no_timestamps(true);
    // A short question needs only part of whisper's 30 s window: encode just that (much faster).
    let ctx_len = ((pcm.len() as f32 / 16000.0) * 50.0).ceil() as i32 + 128;
    p.set_audio_ctx(ctx_len.clamp(256, 1500));
    state.full(p, pcm).map_err(|_| "Couldn't understand the recording.".to_string())?;
    let mut out = String::new();
    for seg in state.as_iter() {
        if let Ok(t) = seg.to_str_lossy() {
            out.push_str(&t);
        }
    }
    Ok(clean(&out))
}

#[cfg(not(target_os = "linux"))]
pub fn transcribe(_model: &Path, _pcm: &[f32]) -> Result<String, String> {
    Err("Speech works on the device.".into())
}

/// Whisper's own markers ("[BLANK_AUDIO]", "(music)") out, spaces tidied.
pub fn clean(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(samples: &[i16], data_len: Option<u32>) -> Vec<u8> {
        let mut b = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        b.extend(16u32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(16000u32.to_le_bytes());
        b.extend(32000u32.to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(16u16.to_le_bytes());
        b.extend(b"data");
        b.extend(data_len.unwrap_or(samples.len() as u32 * 2).to_le_bytes());
        for s in samples {
            b.extend(s.to_le_bytes());
        }
        b
    }

    #[test]
    fn reads_wav_data_even_with_an_unfinished_header() {
        assert_eq!(wav_samples(&wav(&[0, 16384, -32768], None)).unwrap(), vec![0.0, 0.5, -1.0]);
        assert_eq!(wav_samples(&wav(&[16384, 16384], Some(0))).unwrap().len(), 2);
        assert_eq!(wav_samples(&wav(&[16384, 16384], Some(u32::MAX))).unwrap().len(), 2);
        assert!(wav_samples(b"nope").is_err());
    }

    #[test]
    fn cleans_whisper_markers() {
        assert_eq!(clean(" [BLANK_AUDIO] "), "");
        assert_eq!(clean(" Start Hades (music)  please "), "Start Hades please");
    }

    /// RARE_WHISPER_MODEL=<ggml file> RARE_WHISPER_WAV=<16 kHz wav> cargo test -p rare-agent -- --ignored
    #[test]
    #[ignore]
    fn transcribes_a_sample() {
        let model = PathBuf::from(std::env::var("RARE_WHISPER_MODEL").unwrap());
        let pcm = wav_samples(&std::fs::read(std::env::var("RARE_WHISPER_WAV").unwrap()).unwrap()).unwrap();
        let t0 = Instant::now();
        let text = transcribe(&model, &pcm).unwrap();
        eprintln!("transcribed in {:?}: {text}", t0.elapsed());
        assert!(text.to_lowercase().contains("country"), "{text}");
    }
}
