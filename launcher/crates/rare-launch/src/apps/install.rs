//! Installing an app's Flatpak from Flathub into the user installation (no sudo), with
//! progress for the UI. flatpak only draws its progress bar on a terminal, so it runs under
//! `script` (util-linux) when that exists; otherwise the UI shows a plain loading bar.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use rare_core::events::InstallEvent;
use rare_core::{Ctx, Error, Event};

/// Strips terminal escapes (colours, cursor moves) from flatpak's output.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            if it.peek() == Some(&'[') {
                it.next();
                // Parameters, then one final letter.
                for n in it.by_ref() {
                    if n.is_ascii_alphabetic() || n == '~' {
                        break;
                    }
                }
            } else {
                it.next();
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Reads flatpak's progress lines ("Installing 2/3… ████▌ 45%  3.1 MB/s").
#[derive(Default, Debug)]
pub struct Progress {
    step: u32,
    steps: u32,
    pct: Option<f32>,
    pub last_line: String,
}

impl Progress {
    /// Feeds a chunk of output; returns the overall progress when it is known.
    pub fn feed(&mut self, chunk: &str) -> Option<f32> {
        for line in strip_ansi(chunk).split(['\r', '\n']) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            self.last_line = line.to_string();
            let words: Vec<&str> = line.split_whitespace().collect();
            for (i, w) in words.iter().enumerate() {
                if matches!(*w, "Installing" | "Updating" | "Installing:" | "Updating:") {
                    if let Some((a, b)) = words.get(i + 1).and_then(|n| n.trim_end_matches(['…', '.', ':']).split_once('/')) {
                        if let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) {
                            if b > 0 && a >= 1 && a <= b {
                                if (a, b) != (self.step, self.steps) {
                                    self.pct = None;
                                }
                                self.step = a;
                                self.steps = b;
                            }
                        }
                    }
                }
                if let Some(p) = w.strip_suffix('%').and_then(|p| p.parse::<f32>().ok()) {
                    if (0.0..=100.0).contains(&p) {
                        self.pct = Some(p);
                    }
                }
            }
        }
        self.overall()
    }

    pub fn overall(&self) -> Option<f32> {
        let pct = self.pct?;
        if self.steps == 0 {
            return Some(pct / 100.0);
        }
        Some((((self.step - 1) as f32) + pct / 100.0) / self.steps as f32)
    }
}

static BUSY: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn valid_flatpak_id(id: &str) -> bool {
    id.split('.').count() >= 3 && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Starts installing `flatpak` for catalog app `app_id`; progress arrives as `app-install` events.
pub fn start(ctx: Ctx, app_id: String, flatpak: String, user_flathub: bool) -> Result<(), Error> {
    if !valid_flatpak_id(&flatpak) {
        return Err("Not a Flatpak app id".into());
    }
    if !rare_library::steam::which("flatpak") {
        return Err("Flatpak isn't available on this system".into());
    }
    {
        let mut b = BUSY.lock().unwrap_or_else(|e| e.into_inner());
        if b.contains(&app_id) {
            return Ok(());
        }
        b.push(app_id.clone());
    }
    std::thread::spawn(move || {
        let result = run(&ctx, &app_id, &flatpak, user_flathub);
        BUSY.lock().unwrap_or_else(|e| e.into_inner()).retain(|x| x != &app_id);
        ctx.send(Event::AppInstall(InstallEvent { id: app_id.clone(), progress: result.is_ok().then_some(1.0), done: true, error: result.err() }));
    });
    Ok(())
}

fn run(ctx: &Ctx, app_id: &str, flatpak: &str, user_flathub: bool) -> Result<(), String> {
    let emit = |progress: Option<f32>| {
        ctx.send(Event::AppInstall(InstallEvent { id: app_id.to_string(), progress, done: false, error: None }));
    };
    emit(None);
    if !user_flathub {
        let out = crate::launch::host_env(&mut Command::new("flatpak"))
            .args(["remote-add", "--user", "--if-not-exists", "flathub", "https://dl.flathub.org/repo/flathub.flatpakrepo"])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("Could not run flatpak: {e}"))?;
        if !out.status.success() {
            return Err(last_line(&String::from_utf8_lossy(&out.stderr)).unwrap_or_else(|| "Could not add Flathub".into()));
        }
    }
    let install = ["install", "--user", "-y", "flathub", flatpak];
    let mut cmd = if rare_library::steam::which("script") {
        let mut c = Command::new("script");
        // -q quiet, -f flush, -e child's exit code, -c the command; the transcript goes nowhere.
        c.args(["-qfec", &format!("flatpak {}", install.join(" ")), "/dev/null"]);
        c
    } else {
        let mut c = Command::new("flatpak");
        c.args(install);
        c
    };
    crate::launch::host_env(&mut cmd);
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("Could not run flatpak: {e}"))?;
    let mut err_pipe = child.stderr.take();
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = err_pipe.as_mut() {
            let _ = e.read_to_string(&mut s);
        }
        s
    });
    let mut p = Progress::default();
    if let Some(mut out) = child.stdout.take() {
        let mut buf = [0u8; 4096];
        let mut last: Option<f32> = None;
        while let Ok(n) = out.read(&mut buf) {
            if n == 0 {
                break;
            }
            let now = p.feed(&String::from_utf8_lossy(&buf[..n]));
            // Only whole percent steps reach the UI.
            if now.map(|v| (v * 100.0).floor()) != last.map(|v| (v * 100.0).floor()) {
                emit(now);
                last = now;
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let stderr = err_thread.join().unwrap_or_default();
    if status.success() {
        Ok(())
    } else {
        Err(last_line(&stderr).or_else(|| Some(p.last_line.clone()).filter(|l| !l.is_empty())).unwrap_or_else(|| "The install didn't finish".into()))
    }
}

fn last_line(s: &str) -> Option<String> {
    strip_ansi(s).lines().map(str::trim).filter(|l| !l.is_empty()).last().map(|l| l.trim_start_matches("error: ").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_flatpak_progress() {
        let mut p = Progress::default();
        assert_eq!(p.feed("Looking for matches…\n"), None);
        let v = p.feed("\r\u{1b}[KInstalling 1/2… ████▌               23%  4.2 MB/s  00:12").unwrap();
        assert!((v - 0.115).abs() < 1e-6, "{v}");
        let v = p.feed("\r\u{1b}[KInstalling 1/2… ████████████████████ 100%  4.2 MB/s  00:00\r\u{1b}[KInstalling 2/2…                       0%").unwrap();
        assert!((v - 0.5).abs() < 1e-6, "{v}");
        let v = p.feed("\r\u{1b}[KInstalling 2/2… ██████████          50%  9 MB/s").unwrap();
        assert!((v - 0.75).abs() < 1e-6, "{v}");
        assert!(p.last_line.starts_with("Installing 2/2"));
        // A bare percentage without steps still counts.
        let mut q = Progress::default();
        assert!((q.feed("Downloading 40%").unwrap() - 0.4).abs() < 1e-6);
    }

    #[test]
    fn strips_escapes_and_checks_ids() {
        assert_eq!(strip_ansi("\u{1b}[1;32mok\u{1b}[0m \u{1b}[?25l"), "ok ");
        assert!(valid_flatpak_id("org.telegram.desktop"));
        assert!(!valid_flatpak_id("org.x; rm -rf ~"));
        assert!(!valid_flatpak_id("telegram"));
        assert_eq!(last_line("Installing…\nerror: No remote refs found for ‘flathub’\n").as_deref(), Some("No remote refs found for ‘flathub’"));
    }
}
