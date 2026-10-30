//! Small UI sounds and controller rumble on move, select, back and the rest: the native port of
//! `launcher/src/nav/feedback.ts`.
//!
//! Sounds are synthesised here, sample for sample what the web launcher's WebAudio graph plays:
//! an oscillator (sine, or band-limited triangle and square as WebAudio builds them) with an
//! exponential pitch glide, a 4 ms exponential attack and an exponential decay, through a 0.32
//! master gain, at 48 kHz (the PipeWire graph's rate, no resampling). They go out through
//! PipeWire, loaded at run time (`libpipewire-0.3.so.0`): no build dependency, and no sound at
//! all where it is missing. The stream is inactive while nothing plays, so an idle launcher never
//! wakes the audio graph.
//!
//! Rumble reuses the three native effects (move, select, back) through rare-input's evdev
//! force-feedback player, which never re-uploads an idle effect.
//!
//! Both play only in the real app: never in golden captures (`RR_SHOT`), tests, `RR_SILENT=1`,
//! or anywhere but Linux (the dev Mac stays silent). Both can be turned off in Settings.

/// Every UI event the shell gives feedback for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Move,
    Select,
    Back,
    Tab,
    Open,
    Close,
    Launch,
    Toggle,
    Error,
    Notify,
    Boot,
}

impl Kind {
    pub const ALL: [Kind; 11] = [Kind::Move, Kind::Select, Kind::Back, Kind::Tab, Kind::Open, Kind::Close, Kind::Launch, Kind::Toggle, Kind::Error, Kind::Notify, Kind::Boot];

    /// Which native rumble a kind uses (None: no rumble).
    pub fn rumble(self) -> Option<&'static str> {
        Some(match self {
            Kind::Move | Kind::Tab | Kind::Toggle => "move",
            Kind::Select | Kind::Open | Kind::Launch => "select",
            Kind::Back | Kind::Close | Kind::Error => "back",
            Kind::Notify | Kind::Boot => return None,
        })
    }

    /// The tones (feedback.ts `TONES`).
    pub fn tones(self) -> &'static [Tone] {
        use Wave::*;
        const fn t(f: f32, to: f32, ms: f32, vol: f32, wave: Wave, at: f32) -> Tone {
            Tone { f, to, ms, vol, wave, at }
        }
        match self {
            Kind::Move => {
                const T: &[Tone] = &[t(1320.0, 1320.0, 28.0, 0.18, Sine, 0.0)];
                T
            }
            Kind::Select => {
                const T: &[Tone] = &[t(880.0, 1480.0, 70.0, 0.32, Triangle, 0.0)];
                T
            }
            Kind::Back => {
                const T: &[Tone] = &[t(740.0, 420.0, 70.0, 0.28, Triangle, 0.0)];
                T
            }
            // A tab change: a softer, lower tick than a move so LB/RB feel like a shelf turning.
            Kind::Tab => {
                const T: &[Tone] = &[t(990.0, 1180.0, 45.0, 0.2, Sine, 0.0)];
                T
            }
            // Overlays: rising when something opens over the page, falling when it closes.
            Kind::Open => {
                const T: &[Tone] = &[t(660.0, 990.0, 90.0, 0.24, Sine, 0.0)];
                T
            }
            Kind::Close => {
                const T: &[Tone] = &[t(880.0, 560.0, 80.0, 0.2, Sine, 0.0)];
                T
            }
            // Play: two rising notes, the one sound that says "something big is starting".
            Kind::Launch => {
                const T: &[Tone] = &[t(660.0, 660.0, 70.0, 0.26, Triangle, 0.0), t(990.0, 1320.0, 140.0, 0.3, Triangle, 0.07)];
                T
            }
            Kind::Toggle => {
                const T: &[Tone] = &[t(1100.0, 1250.0, 35.0, 0.2, Triangle, 0.0)];
                T
            }
            Kind::Error => {
                const T: &[Tone] = &[t(330.0, 300.0, 70.0, 0.26, Square, 0.0), t(300.0, 260.0, 90.0, 0.22, Square, 0.09)];
                T
            }
            // A sale or price alert arriving: one quiet bell, never on every poll.
            Kind::Notify => {
                const T: &[Tone] = &[t(1568.0, 1568.0, 110.0, 0.14, Sine, 0.0)];
                T
            }
            // Startup: a fifth that opens up, once per boot.
            Kind::Boot => {
                const T: &[Tone] = &[t(523.0, 523.0, 160.0, 0.2, Sine, 0.0), t(784.0, 784.0, 260.0, 0.2, Sine, 0.12)];
                T
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wave {
    Sine,
    Triangle,
    Square,
}

/// One oscillator note: frequency `f` gliding to `to` over `ms`, peak gain `vol`, starting `at`
/// seconds into the sound.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tone {
    pub f: f32,
    pub to: f32,
    pub ms: f32,
    pub vol: f32,
    pub wave: Wave,
    pub at: f32,
}

pub const RATE: u32 = 48_000;
/// The master gain node.
pub const MASTER: f32 = 0.32;
/// WebAudio's exponential ramps start and end at this (an exponential ramp cannot reach 0).
const FLOOR: f64 = 0.0001;
/// Oscillators stop this long after their decay ends.
const TAIL: f64 = 0.02;

/// WebAudio `exponentialRampToValueAtTime` from (t0, v0) to (t1, v1), evaluated at t.
fn exp_ramp(v0: f64, v1: f64, t0: f64, t1: f64, t: f64) -> f64 {
    if t <= t0 {
        return v0;
    }
    if t >= t1 {
        return v1;
    }
    v0 * (v1 / v0).powf((t - t0) / (t1 - t0))
}

/// The gain envelope at `t` seconds into a tone.
fn envelope(tone: &Tone, t: f64) -> f64 {
    let attack = 0.004;
    let end = tone.ms as f64 / 1000.0;
    if t < attack {
        exp_ramp(FLOOR, tone.vol as f64, 0.0, attack, t)
    } else {
        exp_ramp(tone.vol as f64, FLOOR, attack, end, t)
    }
}

/// The oscillator's frequency at `t` seconds into a tone.
fn frequency(tone: &Tone, t: f64) -> f64 {
    if tone.to == tone.f {
        tone.f as f64
    } else {
        exp_ramp(tone.f as f64, tone.to as f64, 0.0, tone.ms as f64 / 1000.0, t)
    }
}

/// One tone's samples (before the master gain): a phase-accumulating oscillator, band-limited
/// like WebAudio's built-in periodic waves (Fourier series up to Nyquist, normalised to a peak
/// of 1), times its envelope.
pub fn render_tone(tone: &Tone) -> Vec<f32> {
    let rate = RATE as f64;
    let len = ((tone.ms as f64 / 1000.0 + TAIL) * rate).ceil() as usize;
    let mut osc = Vec::with_capacity(len);
    let mut phase = 0.0f64; // cycles
    for i in 0..len {
        let t = i as f64 / rate;
        let f = frequency(tone, t);
        let x = std::f64::consts::TAU * phase;
        let v = match tone.wave {
            Wave::Sine => x.sin(),
            Wave::Triangle | Wave::Square => {
                let mut s = 0.0;
                let mut k = 1u32;
                while (k as f64) * f < rate / 2.0 {
                    let kf = k as f64;
                    s += match tone.wave {
                        // b_k = 8 sin(k pi / 2) / (k pi)^2
                        Wave::Triangle => 8.0 * (kf * std::f64::consts::FRAC_PI_2).sin() / (kf * std::f64::consts::PI).powi(2),
                        // b_k = 2 / (k pi) * (1 - (-1)^k)
                        _ => 4.0 / (kf * std::f64::consts::PI),
                    } * (kf * x).sin();
                    k += 2;
                }
                s
            }
        };
        osc.push(v);
        phase = (phase + f / rate).fract();
    }
    let peak = osc.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1e-9);
    let norm = if tone.wave == Wave::Sine { 1.0 } else { 1.0 / peak };
    osc.iter().enumerate().map(|(i, v)| (v * norm * envelope(tone, i as f64 / rate)) as f32).collect()
}

/// A whole sound, mono f32 at RATE, master gain applied.
pub fn render(kind: Kind) -> Vec<f32> {
    let mut out: Vec<f32> = Vec::new();
    for tone in kind.tones() {
        let start = (tone.at as f64 * RATE as f64).round() as usize;
        let s = render_tone(tone);
        if out.len() < start + s.len() {
            out.resize(start + s.len(), 0.0);
        }
        for (i, v) in s.into_iter().enumerate() {
            out[start + i] += v;
        }
    }
    for v in out.iter_mut() {
        *v = (*v * MASTER).clamp(-1.0, 1.0);
    }
    out
}

/// Sounds and rumble may play: Linux, not a golden capture, not `RR_SILENT=1`.
pub fn real_app() -> bool {
    cfg!(target_os = "linux") && !cfg!(test) && std::env::var_os("RR_SHOT").is_none() && std::env::var("RR_SILENT").as_deref() != Ok("1")
}

/// The feedback player for the UI thread.
pub struct Feedback {
    real: bool,
    sounds: bool,
    rumble: bool,
    /// Settings > Controls' rumble strength.
    pub strength: f32,
    last_move: Option<f64>,
    #[cfg(target_os = "linux")]
    out: Option<pw::Out>,
    #[cfg(target_os = "linux")]
    tried: bool,
    /// What was played, for tests and the journey probe: (kind, sound, rumble).
    pub log: Option<Vec<(Kind, bool, bool)>>,
}

impl Default for Feedback {
    fn default() -> Self {
        Feedback::new(real_app())
    }
}

impl Feedback {
    /// `real`: this is the real app (see `real_app`); otherwise nothing ever plays.
    pub fn new(real: bool) -> Feedback {
        Feedback {
            real,
            sounds: real,
            rumble: real,
            strength: 1.0,
            last_move: None,
            #[cfg(target_os = "linux")]
            out: None,
            #[cfg(target_os = "linux")]
            tried: false,
            log: None,
        }
    }

    /// Settings: sounds and rumble on or off (None leaves one as it is).
    pub fn set(&mut self, sounds: Option<bool>, rumble: Option<bool>) {
        if let Some(s) = sounds {
            self.sounds = self.real && s;
        }
        if let Some(r) = rumble {
            self.rumble = self.real && r;
        }
    }

    /// Plays `kind` (sound and rumble as set). Held directions repeat quickly: move ticks closer
    /// than 45 ms are dropped so they never turn into a buzz. Returns whether it played.
    pub fn play(&mut self, kind: Kind, now: f64) -> bool {
        if kind == Kind::Move {
            if self.last_move.is_some_and(|t| now - t < crate::input::MOVE_TICK) {
                return false;
            }
            self.last_move = Some(now);
        }
        let rumble = self.rumble.then(|| kind.rumble()).flatten();
        if let Some(log) = self.log.as_mut() {
            log.push((kind, self.sounds, rumble.is_some()));
        }
        if self.sounds {
            self.sound(kind);
        }
        if let Some(r) = rumble {
            let _ = rare_input::rumble::play(r, self.strength);
        }
        true
    }

    #[cfg(target_os = "linux")]
    fn sound(&mut self, kind: Kind) {
        if self.out.is_none() && !self.tried {
            self.tried = true;
            match pw::Out::open() {
                Ok(o) => self.out = Some(o),
                Err(e) => log::info!("ui sounds: {e}"),
            }
        }
        if let Some(o) = self.out.as_ref() {
            o.play(sound(kind));
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn sound(&mut self, _kind: Kind) {}
}

/// The rendered sound for a kind, as 16-bit samples (made once per process).
pub fn sound(kind: Kind) -> &'static [i16] {
    use std::sync::OnceLock;
    static SOUNDS: OnceLock<Vec<Vec<i16>>> = OnceLock::new();
    let all = SOUNDS.get_or_init(|| Kind::ALL.iter().map(|k| render(*k).into_iter().map(|v| (v * i16::MAX as f32) as i16).collect()).collect());
    &all[Kind::ALL.iter().position(|k| *k == kind).unwrap_or(0)]
}

/// The PipeWire stream's format: S16 mono at 48 kHz, as an `SPA_PARAM_EnumFormat` object pod
/// (what `spa_format_audio_raw_build` writes).
pub fn format_pod() -> Vec<u8> {
    const SPA_TYPE_ID: u32 = 3;
    const SPA_TYPE_INT: u32 = 4;
    const SPA_TYPE_ARRAY: u32 = 13;
    const SPA_TYPE_OBJECT: u32 = 15;
    const SPA_TYPE_OBJECT_FORMAT: u32 = 0x40003;
    const SPA_PARAM_ENUM_FORMAT: u32 = 3;
    const MEDIA_TYPE: u32 = 1;
    const MEDIA_SUBTYPE: u32 = 2;
    const AUDIO_FORMAT: u32 = 0x10001;
    const AUDIO_RATE: u32 = 0x10003;
    const AUDIO_CHANNELS: u32 = 0x10004;
    const AUDIO_POSITION: u32 = 0x10005;
    const MEDIA_AUDIO: u32 = 1;
    const SUBTYPE_RAW: u32 = 1;
    const FORMAT_S16_LE: u32 = 0x103;
    const CHANNEL_MONO: u32 = 2;
    let mut body: Vec<u8> = Vec::new();
    let w = |b: &mut Vec<u8>, v: u32| b.extend_from_slice(&v.to_ne_bytes());
    let pad8 = |b: &mut Vec<u8>| {
        while !b.len().is_multiple_of(8) {
            b.push(0)
        }
    };
    w(&mut body, SPA_TYPE_OBJECT_FORMAT);
    w(&mut body, SPA_PARAM_ENUM_FORMAT);
    for (key, ty, val) in [(MEDIA_TYPE, SPA_TYPE_ID, MEDIA_AUDIO), (MEDIA_SUBTYPE, SPA_TYPE_ID, SUBTYPE_RAW), (AUDIO_FORMAT, SPA_TYPE_ID, FORMAT_S16_LE), (AUDIO_RATE, SPA_TYPE_INT, RATE), (AUDIO_CHANNELS, SPA_TYPE_INT, 1)] {
        w(&mut body, key);
        w(&mut body, 0); // flags
        w(&mut body, 4); // value size
        w(&mut body, ty);
        w(&mut body, val);
        pad8(&mut body);
    }
    // position: an array of one Id (MONO).
    w(&mut body, AUDIO_POSITION);
    w(&mut body, 0);
    w(&mut body, 8 + 4); // array body: child pod header + one value
    w(&mut body, SPA_TYPE_ARRAY);
    w(&mut body, 4); // child size
    w(&mut body, SPA_TYPE_ID);
    w(&mut body, CHANNEL_MONO);
    pad8(&mut body);
    let mut pod = Vec::with_capacity(8 + body.len());
    w(&mut pod, body.len() as u32);
    w(&mut pod, SPA_TYPE_OBJECT);
    pod.extend_from_slice(&body);
    pod
}

/// A minimal PipeWire playback stream over dlopen'd libpipewire-0.3: a thread loop, one stream
/// connected inactive, a mixer of playing voices filled in the realtime process callback. When
/// the voices run out the stream drains and goes inactive again.
#[cfg(target_os = "linux")]
mod pw {
    use super::format_pod;
    use std::ffi::{c_char, c_int, c_void, CStr};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    #[repr(C)]
    struct SpaChunk {
        offset: u32,
        size: u32,
        stride: i32,
        flags: i32,
    }
    #[repr(C)]
    struct SpaData {
        ty: u32,
        flags: u32,
        fd: i64,
        mapoffset: u32,
        maxsize: u32,
        data: *mut c_void,
        chunk: *mut SpaChunk,
    }
    #[repr(C)]
    struct SpaBuffer {
        n_metas: u32,
        n_datas: u32,
        metas: *mut c_void,
        datas: *mut SpaData,
    }
    #[repr(C)]
    struct PwBuffer {
        buffer: *mut SpaBuffer,
        user_data: *mut c_void,
        size: u64,
        requested: u64,
    }
    type Cb = Option<unsafe extern "C" fn(*mut c_void)>;
    #[repr(C)]
    struct StreamEvents {
        version: u32,
        destroy: Cb,
        state_changed: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, *const c_char)>,
        control_info: Option<unsafe extern "C" fn(*mut c_void, u32, *const c_void)>,
        io_changed: Option<unsafe extern "C" fn(*mut c_void, u32, *mut c_void, u32)>,
        param_changed: Option<unsafe extern "C" fn(*mut c_void, u32, *const c_void)>,
        add_buffer: Option<unsafe extern "C" fn(*mut c_void, *mut PwBuffer)>,
        remove_buffer: Option<unsafe extern "C" fn(*mut c_void, *mut PwBuffer)>,
        process: Cb,
        drained: Cb,
        command: Option<unsafe extern "C" fn(*mut c_void, *const c_void)>,
        trigger_done: Cb,
    }

    struct Api {
        thread_loop_lock: unsafe extern "C" fn(*mut c_void),
        thread_loop_unlock: unsafe extern "C" fn(*mut c_void),
        dequeue: unsafe extern "C" fn(*mut c_void) -> *mut PwBuffer,
        queue: unsafe extern "C" fn(*mut c_void, *mut PwBuffer) -> c_int,
        set_active: unsafe extern "C" fn(*mut c_void, bool) -> c_int,
        flush: unsafe extern "C" fn(*mut c_void, bool) -> c_int,
    }

    /// What the callbacks share with the UI thread. Leaked: it lives as long as the process.
    struct Shared {
        api: Api,
        tl: *mut c_void,
        stream: *mut c_void,
        /// Voices playing: (samples, position).
        voices: Mutex<Vec<(&'static [i16], usize)>>,
        active: AtomicBool,
        draining: AtomicBool,
    }
    unsafe impl Send for Shared {}
    unsafe impl Sync for Shared {}

    pub struct Out {
        sh: &'static Shared,
    }

    const SPA_DIRECTION_OUTPUT: c_int = 1;
    const PW_ID_ANY: u32 = 0xffff_ffff;
    const FLAG_AUTOCONNECT: u32 = 1 << 0;
    const FLAG_INACTIVE: u32 = 1 << 1;
    const FLAG_MAP_BUFFERS: u32 = 1 << 2;
    const FLAG_RT_PROCESS: u32 = 1 << 4;

    unsafe fn sym<T: Copy>(lib: *mut c_void, name: &CStr) -> Result<T, String> {
        let p = libc::dlsym(lib, name.as_ptr());
        if p.is_null() {
            return Err(format!("libpipewire has no {}", name.to_string_lossy()));
        }
        Ok(std::mem::transmute_copy::<*mut c_void, T>(&p))
    }

    unsafe extern "C" fn on_process(data: *mut c_void) {
        let sh = &*(data as *const Shared);
        let b = (sh.api.dequeue)(sh.stream);
        if b.is_null() {
            return;
        }
        let buf = &mut *(*b).buffer;
        if buf.n_datas == 0 || buf.datas.is_null() {
            (sh.api.queue)(sh.stream, b);
            return;
        }
        let d = &mut *buf.datas;
        if d.data.is_null() {
            (sh.api.queue)(sh.stream, b);
            return;
        }
        let max = (d.maxsize as usize) / 2;
        let want = if (*b).requested > 0 { ((*b).requested as usize).min(max) } else { max.min(1024) };
        let out = std::slice::from_raw_parts_mut(d.data as *mut i16, want);
        out.fill(0);
        // Never block the realtime thread: a busy mixer means silence for this quantum.
        let done = match sh.voices.try_lock() {
            Ok(mut vs) => {
                for (s, pos) in vs.iter_mut() {
                    let n = (s.len() - *pos).min(want);
                    for i in 0..n {
                        out[i] = out[i].saturating_add(s[*pos + i]);
                    }
                    *pos += n;
                }
                vs.retain(|(s, pos)| *pos < s.len());
                vs.is_empty()
            }
            Err(_) => false,
        };
        let chunk = &mut *d.chunk;
        chunk.offset = 0;
        chunk.stride = 2;
        chunk.size = (want * 2) as u32;
        (sh.api.queue)(sh.stream, b);
        if done && !sh.draining.swap(true, Ordering::AcqRel) {
            // Out of voices: drain what is queued, then go inactive (on_drained).
            (sh.api.flush)(sh.stream, true);
        }
    }

    unsafe extern "C" fn on_drained(data: *mut c_void) {
        // Main loop thread, loop lock held.
        let sh = &*(data as *const Shared);
        (sh.api.set_active)(sh.stream, false);
        sh.active.store(false, Ordering::Release);
        sh.draining.store(false, Ordering::Release);
        let pending = sh.voices.lock().map(|v| !v.is_empty()).unwrap_or(false);
        if pending {
            (sh.api.set_active)(sh.stream, true);
            sh.active.store(true, Ordering::Release);
        }
    }

    static EVENTS: StreamEvents = StreamEvents {
        version: 2,
        destroy: None,
        state_changed: None,
        control_info: None,
        io_changed: None,
        param_changed: None,
        add_buffer: None,
        remove_buffer: None,
        process: Some(on_process),
        drained: Some(on_drained),
        command: None,
        trigger_done: None,
    };

    impl Out {
        pub fn open() -> Result<Out, String> {
            unsafe {
                let lib = libc::dlopen(c"libpipewire-0.3.so.0".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
                if lib.is_null() {
                    return Err("no libpipewire-0.3".into());
                }
                let init: unsafe extern "C" fn(*mut c_int, *mut *mut *mut c_char) = sym(lib, c"pw_init")?;
                let tl_new: unsafe extern "C" fn(*const c_char, *const c_void) -> *mut c_void = sym(lib, c"pw_thread_loop_new")?;
                let tl_start: unsafe extern "C" fn(*mut c_void) -> c_int = sym(lib, c"pw_thread_loop_start")?;
                let tl_loop: unsafe extern "C" fn(*mut c_void) -> *mut c_void = sym(lib, c"pw_thread_loop_get_loop")?;
                let props_new: unsafe extern "C" fn(*const c_char) -> *mut c_void = sym(lib, c"pw_properties_new_string")?;
                let stream_new: unsafe extern "C" fn(*mut c_void, *const c_char, *mut c_void, *const StreamEvents, *mut c_void) -> *mut c_void = sym(lib, c"pw_stream_new_simple")?;
                let connect: unsafe extern "C" fn(*mut c_void, c_int, u32, u32, *mut *const c_void, u32) -> c_int = sym(lib, c"pw_stream_connect")?;
                let api = Api {
                    thread_loop_lock: sym(lib, c"pw_thread_loop_lock")?,
                    thread_loop_unlock: sym(lib, c"pw_thread_loop_unlock")?,
                    dequeue: sym(lib, c"pw_stream_dequeue_buffer")?,
                    queue: sym(lib, c"pw_stream_queue_buffer")?,
                    set_active: sym(lib, c"pw_stream_set_active")?,
                    flush: sym(lib, c"pw_stream_flush")?,
                };
                init(std::ptr::null_mut(), std::ptr::null_mut());
                let tl = tl_new(c"rare-ui-sounds".as_ptr(), std::ptr::null());
                if tl.is_null() {
                    return Err("pw_thread_loop_new failed".into());
                }
                let sh: &'static mut Shared = Box::leak(Box::new(Shared { api, tl, stream: std::ptr::null_mut(), voices: Mutex::new(Vec::new()), active: AtomicBool::new(false), draining: AtomicBool::new(false) }));
                let props = props_new(c"media.type=Audio media.category=Playback media.role=Notification node.name=rare-ui-sounds application.name=Rare node.latency=256/48000".as_ptr());
                (sh.api.thread_loop_lock)(tl);
                let stream = stream_new(tl_loop(tl), c"Rare UI sounds".as_ptr(), props, &EVENTS, sh as *mut Shared as *mut c_void);
                if stream.is_null() {
                    (sh.api.thread_loop_unlock)(tl);
                    return Err("pw_stream_new_simple failed".into());
                }
                sh.stream = stream;
                let pod = Box::leak(format_pod().into_boxed_slice());
                let mut params: [*const c_void; 1] = [pod.as_ptr() as *const c_void];
                let r = connect(stream, SPA_DIRECTION_OUTPUT, PW_ID_ANY, FLAG_AUTOCONNECT | FLAG_INACTIVE | FLAG_MAP_BUFFERS | FLAG_RT_PROCESS, params.as_mut_ptr(), 1);
                (sh.api.thread_loop_unlock)(tl);
                if r < 0 {
                    return Err(format!("pw_stream_connect: {r}"));
                }
                if tl_start(tl) < 0 {
                    return Err("pw_thread_loop_start failed".into());
                }
                Ok(Out { sh })
            }
        }

        pub fn play(&self, s: &'static [i16]) {
            if let Ok(mut v) = self.sh.voices.lock() {
                v.push((s, 0));
            }
            if !self.sh.active.swap(true, Ordering::AcqRel) {
                unsafe {
                    (self.sh.api.thread_loop_lock)(self.sh.tl);
                    (self.sh.api.set_active)(self.sh.stream, true);
                    (self.sh.api.thread_loop_unlock)(self.sh.tl);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(v: &[f32]) -> f32 {
        v.iter().fold(0.0f32, |m, x| m.max(x.abs()))
    }

    #[test]
    fn every_kind_has_its_rumble() {
        assert_eq!(Kind::Move.rumble(), Some("move"));
        assert_eq!(Kind::Tab.rumble(), Some("move"));
        assert_eq!(Kind::Toggle.rumble(), Some("move"));
        assert_eq!(Kind::Open.rumble(), Some("select"));
        assert_eq!(Kind::Launch.rumble(), Some("select"));
        assert_eq!(Kind::Close.rumble(), Some("back"));
        assert_eq!(Kind::Error.rumble(), Some("back"));
        assert_eq!(Kind::Notify.rumble(), None);
        assert_eq!(Kind::Boot.rumble(), None);
        for k in Kind::ALL {
            if let Some(r) = k.rumble() {
                assert!(rare_input::rumble::Rumble::from_kind(r).is_some());
            }
        }
    }

    #[test]
    fn sounds_are_short_blips_with_the_web_envelope() {
        for k in Kind::ALL {
            let s = render(k);
            let ms = s.len() as f32 / RATE as f32 * 1000.0;
            // Every UI blip is under 120 ms plus its tail; launch, error and boot have two notes.
            let limit = match k {
                Kind::Launch => 231.0,
                Kind::Error => 201.0,
                Kind::Boot => 401.0,
                _ => 131.0,
            };
            assert!(ms <= limit, "{k:?} {ms} ms");
            let p = peak(&s);
            assert!(p > 0.02 && p <= MASTER * 0.6, "{k:?} peak {p}");
            // Starts and ends silent (no clicks).
            assert!(s[0].abs() < 1e-3 && s[s.len() - 1].abs() < 1e-3, "{k:?}");
        }
        // The move tick: 28 ms of 1320 Hz at 0.18, through the 0.32 master.
        let mv = render(Kind::Move);
        assert_eq!(mv.len(), ((0.028 + 0.02) * 48000.0f64).ceil() as usize);
        assert!((peak(&mv) - 0.18 * MASTER).abs() < 0.01, "{}", peak(&mv));
    }

    #[test]
    fn envelopes_and_glides_follow_webaudio_ramps() {
        let t = Kind::Select.tones()[0];
        assert!((envelope(&t, 0.0) - 0.0001).abs() < 1e-9);
        assert!((envelope(&t, 0.004) - 0.32).abs() < 1e-6);
        assert!((envelope(&t, 0.07) - 0.0001).abs() < 1e-9);
        // Exponential: halfway through the decay is the geometric mean.
        let mid = envelope(&t, 0.004 + 0.033);
        assert!((mid - (0.32f64 * 0.0001).sqrt()).abs() < 1e-6, "{mid}");
        assert!((frequency(&t, 0.0) - 880.0).abs() < 1e-6);
        assert!((frequency(&t, 0.07) - 1480.0).abs() < 1e-6);
        assert!((frequency(&t, 0.035) - (880.0f64 * 1480.0).sqrt()).abs() < 1e-3);
    }

    #[test]
    fn band_limited_waves_peak_at_one() {
        for wave in [Wave::Triangle, Wave::Square] {
            let t = Tone { f: 300.0, to: 300.0, ms: 1000.0, vol: 1.0, wave, at: 0.0 };
            let s = render_tone(&t);
            // Mid-tone the envelope is near its peak; the oscillator itself is normalised to 1.
            let p = peak(&s[200..2000]);
            assert!(p > 0.9 && p <= 1.0001, "{wave:?} {p}");
        }
        // Two-note sounds start their second note `at` seconds in.
        let l = render(Kind::Launch);
        let at = (0.07 * 48000.0) as usize;
        assert!(peak(&l[at + 400..at + 2000]) > 0.05);
    }

    #[test]
    fn moves_never_buzz_and_nothing_plays_outside_the_real_app() {
        let mut f = Feedback::new(false);
        f.log = Some(Vec::new());
        assert!(f.play(Kind::Move, 1.0));
        assert!(!f.play(Kind::Move, 1.02));
        assert!(f.play(Kind::Move, 1.05));
        assert!(f.play(Kind::Select, 1.051));
        // Not the real app: logged as silent and still.
        assert!(f.log.as_ref().unwrap().iter().all(|(_, s, r)| !s && !r));
        f.set(Some(true), Some(true));
        assert!(!f.sounds && !f.rumble, "settings cannot turn it on outside the real app");
        assert!(!real_app(), "tests are never the real app");
    }

    #[test]
    fn the_format_pod_is_a_raw_s16_mono_48k_enum_format() {
        let p = format_pod();
        let u = |i: usize| u32::from_ne_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]]);
        assert_eq!(u(0) as usize, p.len() - 8, "pod size is its body");
        assert_eq!(u(4), 15, "SPA_TYPE_Object");
        assert_eq!((u(8), u(12)), (0x40003, 3), "Format, EnumFormat");
        assert_eq!(p.len() % 8, 0);
        // The properties: key, flags, then an 8-byte pod header and its padded value.
        let mut props = Vec::new();
        let mut i = 16;
        while i < p.len() {
            let (key, size, ty) = (u(i), u(i + 8), u(i + 12));
            props.push((key, ty, u(i + 16)));
            i += 16 + (size as usize).div_ceil(8) * 8;
        }
        assert_eq!(props, vec![(1, 3, 1), (2, 3, 1), (0x10001, 3, 0x103), (0x10003, 4, 48000), (0x10004, 4, 1), (0x10005, 13, 4)]);
        assert_eq!(i, p.len());
    }
}
