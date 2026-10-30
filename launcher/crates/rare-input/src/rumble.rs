//! Short controller rumbles for UI feedback (focus moves, select, back, a saved capture).
//!
//! The Tauri shell plays them through gilrs (gamepad.rs). The native launcher plays them here,
//! straight through evdev force feedback: one FF_RUMBLE effect uploaded per pad, updated only when
//! the rumble differs from the last one, then a single EV_FF play write. Nothing is ever written
//! while idle and a zero effect is never uploaded: gilrs used to re-upload an idle zero rumble
//! every 50 ms for every pad, each an ioctl plus a write InputPlumber (root) logged and forwarded
//! to the physical pad, a constant ~5 % CPU (30b93f9). The kernel times the effect (replay
//! length), so no stop write is needed either.

/// One rumble: how long, and the light (weak) and heavy (strong) motor, 0..=1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rumble {
    pub ms: u32,
    pub weak: f32,
    pub strong: f32,
}

impl Rumble {
    /// The UI's rumbles (feedback.ts maps every sound kind onto move, select or back).
    pub fn from_kind(kind: &str) -> Option<Self> {
        Some(match kind {
            "move" => Rumble { ms: 20, weak: 0.12, strong: 0.0 },
            "select" => Rumble { ms: 35, weak: 0.3, strong: 0.0 },
            "back" => Rumble { ms: 30, weak: 0.22, strong: 0.0 },
            // A capture was saved while the game has the screen.
            "capture" => Rumble { ms: 150, weak: 0.45, strong: 0.3 },
            _ => return None,
        })
    }

    /// Settings > Controls' rumble strength.
    pub fn scaled(self, strength: f32) -> Self {
        let s = strength.max(0.0);
        Rumble { ms: self.ms, weak: self.weak * s, strong: self.strong * s }
    }

    /// (strong, weak, length ms) as the kernel takes them.
    pub fn effect(&self) -> (u16, u16, u16) {
        let mag = |v: f32| (v.clamp(0.0, 1.0) * u16::MAX as f32) as u16;
        (mag(self.strong), mag(self.weak), self.ms.min(u16::MAX as u32) as u16)
    }
}

/// What to write to one pad for a rumble.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Nothing: a zero rumble (silence needs no write).
    Skip,
    /// The uploaded effect is already this one: just play it.
    Play,
    /// Upload (update) the effect, then play it.
    UpdatePlay,
}

/// The per-pad upload state behind the no-spam rule.
#[derive(Clone, Copy, Debug, Default)]
pub struct Uploaded {
    last: Option<(u16, u16, u16)>,
}

impl Uploaded {
    pub fn plan(&mut self, r: &Rumble) -> Plan {
        let e = r.effect();
        if e.0 == 0 && e.1 == 0 || e.2 == 0 {
            return Plan::Skip;
        }
        if self.last == Some(e) {
            return Plan::Play;
        }
        self.last = Some(e);
        Plan::UpdatePlay
    }
}

#[cfg(target_os = "linux")]
mod player {
    use super::{Plan, Rumble, Uploaded};
    use evdev::{Device, FFEffect, FFEffectCode, FFEffectData, FFEffectKind, FFReplay, FFTrigger, KeyCode};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::mpsc::{channel, Sender};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    /// Pads are looked for again at most this often, and only when a rumble is asked for.
    const RESCAN: Duration = Duration::from_secs(5);

    struct Pad {
        dev: Device,
        effect: Option<FFEffect>,
        up: Uploaded,
    }

    fn data(r: &Rumble) -> FFEffectData {
        let (strong, weak, ms) = r.effect();
        FFEffectData {
            direction: 0,
            trigger: FFTrigger::default(),
            replay: FFReplay { length: ms, delay: 0 },
            kind: FFEffectKind::Rumble { strong_magnitude: strong, weak_magnitude: weak },
        }
    }

    fn scan(pads: &mut HashMap<PathBuf, Pad>) {
        for (path, dev) in evdev::enumerate() {
            if pads.contains_key(&path) {
                continue;
            }
            let pad = dev.supported_keys().is_some_and(|k| k.contains(KeyCode::BTN_SOUTH));
            let ff = dev.supported_ff().is_some_and(|f| f.contains(FFEffectCode::FF_RUMBLE));
            if pad && ff {
                crate::log(&format!("[rumble] {} ({})", path.display(), dev.name().unwrap_or("?")));
                pads.insert(path, Pad { dev, effect: None, up: Uploaded::default() });
            }
        }
    }

    fn play(p: &mut Pad, r: &Rumble) -> std::io::Result<()> {
        match p.up.plan(r) {
            Plan::Skip => return Ok(()),
            Plan::Play => {}
            Plan::UpdatePlay => match p.effect.as_mut() {
                Some(e) => e.update(data(r))?,
                None => p.effect = Some(p.dev.upload_ff_effect(data(r))?),
            },
        }
        match p.effect.as_mut() {
            Some(e) => e.play(1),
            None => Ok(()),
        }
    }

    static TX: OnceLock<Mutex<Sender<Rumble>>> = OnceLock::new();

    fn sender() -> &'static Mutex<Sender<Rumble>> {
        TX.get_or_init(|| {
            let (tx, rx) = channel::<Rumble>();
            std::thread::Builder::new()
                .name("rare-rumble".into())
                .spawn(move || {
                    let mut pads: HashMap<PathBuf, Pad> = HashMap::new();
                    let mut scanned: Option<Instant> = None;
                    while let Ok(mut r) = rx.recv() {
                        // Newest wins.
                        while let Ok(n) = rx.try_recv() {
                            r = n;
                        }
                        if pads.is_empty() || !scanned.is_some_and(|t| t.elapsed() < RESCAN) {
                            scanned = Some(Instant::now());
                            scan(&mut pads);
                        }
                        pads.retain(|path, p| match play(p, &r) {
                            Ok(()) => true,
                            Err(e) => {
                                crate::log(&format!("[rumble] {} dropped: {e}", path.display()));
                                false
                            }
                        });
                    }
                })
                .expect("rumble thread");
            Mutex::new(tx)
        })
    }

    pub fn queue(r: Rumble) {
        let _ = sender().lock().unwrap_or_else(|e| e.into_inner()).send(r);
    }
}

/// Plays a rumble kind on every pad that supports force feedback, through evdev (the native
/// launcher). Never blocks; a zero strength does nothing.
pub fn play(kind: &str, strength: f32) -> Result<(), String> {
    let r = Rumble::from_kind(kind).ok_or_else(|| format!("unknown rumble kind {kind}"))?.scaled(strength);
    if strength <= 0.0 {
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    player::queue(r);
    #[cfg(not(target_os = "linux"))]
    let _ = r;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_strength() {
        assert_eq!(Rumble::from_kind("move"), Some(Rumble { ms: 20, weak: 0.12, strong: 0.0 }));
        assert!(Rumble::from_kind("buzz").is_none());
        let r = Rumble::from_kind("select").unwrap().scaled(0.5);
        let (strong, weak, ms) = r.effect();
        assert_eq!((strong, ms), (0, 35));
        assert!((weak as i32 - 9830).abs() <= 1, "{weak}");
        assert_eq!(Rumble::from_kind("capture").unwrap().scaled(10.0).effect(), (65535, 65535, 150));
        assert!(play("move", 0.0).is_ok());
        assert!(play("nope", 1.0).is_err());
    }

    #[test]
    fn never_spams_the_pad() {
        let mut u = Uploaded::default();
        let mv = Rumble::from_kind("move").unwrap();
        let sel = Rumble::from_kind("select").unwrap();
        // Silence is never written, not even the first time.
        assert_eq!(u.plan(&mv.scaled(0.0)), Plan::Skip);
        assert_eq!(u.plan(&Rumble { ms: 0, weak: 1.0, strong: 1.0 }), Plan::Skip);
        // The first move uploads; a run of moves (a held direction) only plays.
        assert_eq!(u.plan(&mv), Plan::UpdatePlay);
        for _ in 0..50 {
            assert_eq!(u.plan(&mv), Plan::Play);
        }
        assert_eq!(u.plan(&sel), Plan::UpdatePlay);
        assert_eq!(u.plan(&mv), Plan::UpdatePlay);
        // A zero in between changes nothing on the pad.
        assert_eq!(u.plan(&mv.scaled(0.0)), Plan::Skip);
        assert_eq!(u.plan(&mv), Plan::Play);
    }
}
