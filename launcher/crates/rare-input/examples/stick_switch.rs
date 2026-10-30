//! Repro for the stuck stick: a virtual source pad behind a real InputPlumber, the launcher's
//! input path (evdev readers + `pad::Machine`, InputPlumber's D-Bus actions + `ip::Keys`, the same
//! loop as rare-render's `input::linux::spawn`), and intercept mode switches in the middle of a
//! held direction.
//!
//! Needs root (uinput, the hidden source pad) and an InputPlumber running with this composite
//! device in /etc/inputplumber/devices.d (and its D-Bus policy in /etc/dbus-1/system.d):
//!
//!     version: 1
//!     kind: CompositeDevice
//!     name: SuiPlay test
//!     matches: []
//!     source_devices:
//!       - group: gamepad
//!         evdev: { name: Rare Stuck Source Pad, handler: "event*" }
//!     options: { auto_manage: true }
//!     target_devices: [xbox-elite]
//!
//! Prints every press the launcher would get and, per scenario, the presses that came after the
//! stick was let go (any is a stuck direction) and the target pad's axes as a game reads them.
//!
//!     cargo build --example stick_switch -p rare-input && sudo target/debug/examples/stick_switch

#[cfg(target_os = "linux")]
fn main() {
    imp::main()
}

#[cfg(not(target_os = "linux"))]
fn main() {}

#[cfg(target_os = "linux")]
mod imp {
    use evdev::uinput::VirtualDevice;
    use evdev::{AbsInfo, AbsoluteAxisCode, AttributeSet, BusType, EventType, InputEvent, InputId, KeyCode, UinputAbsSetup};
    use rare_input::evdev::{mono_ns, Raw};
    use rare_input::ip::Out as IpOut;
    use rare_input::pad::{Machine, Out};
    use rare_input::Pad;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{channel, RecvTimeoutError};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    pub const SOURCE: &str = "Rare Stuck Source Pad";
    const STICK: i32 = 32767;

    static SURFACE: AtomicBool = AtomicBool::new(false);
    static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

    #[derive(Clone, Debug)]
    struct Got {
        at: Instant,
        path: &'static str,
        pad: Pad,
        repeat: bool,
        release: bool,
    }

    /// On the Bluetooth bus: InputPlumber skips virtual pads unless they are Bluetooth ones.
    fn source() -> VirtualDevice {
        pad(SOURCE, BusType::BUS_BLUETOOTH)
    }

    /// A game's own virtual pad (Steam Input's), which InputPlumber leaves alone and the
    /// launcher reads too; it goes away when the game exits.
    fn game_pad() -> VirtualDevice {
        pad("Microsoft X-Box 360 pad (game)", BusType::BUS_USB)
    }

    fn pad(name: &str, bus: BusType) -> VirtualDevice {
        let mut keys = AttributeSet::<KeyCode>::new();
        for k in [0x130u16, 0x131, 0x133, 0x134, 0x136, 0x137, 0x13a, 0x13b, 0x13c, 0x13d, 0x13e] {
            keys.insert(KeyCode(k));
        }
        let stick = AbsInfo::new(0, -STICK - 1, STICK, 16, 128, 0);
        let trig = AbsInfo::new(0, 0, 255, 0, 0, 0);
        let hat = AbsInfo::new(0, -1, 1, 0, 0, 0);
        let mut b = VirtualDevice::builder().unwrap().name(name).input_id(InputId::new(bus, 0x045e, 0x028e, 0x0114)).with_keys(&keys).unwrap();
        for (code, info) in [(0u16, stick), (1, stick), (3, stick), (4, stick), (2, trig), (5, trig), (0x10, hat), (0x11, hat)] {
            b = b.with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode(code), info)).unwrap();
        }
        b.build().unwrap()
    }

    fn abs(dev: &mut VirtualDevice, code: u16, v: i32) {
        dev.emit(&[InputEvent::new(EventType::ABSOLUTE.0, code, v)]).unwrap();
    }

    /// Small wobble around a held value, as a real stick reports while held.
    fn hold(dev: &mut VirtualDevice, code: u16, v: i32, for_ms: u64) {
        let t = Instant::now();
        let mut i = 0;
        while t.elapsed() < Duration::from_millis(for_ms) {
            abs(dev, code, v + if i % 2 == 0 { 40 } else { -40 });
            i += 1;
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    fn target_path() -> Option<std::path::PathBuf> {
        evdev::enumerate().find(|(_, d)| d.name() == Some("Microsoft X-Box One Elite 2 pad")).map(|(p, _)| p)
    }

    fn target_abs() -> String {
        let Some(p) = target_path() else { return "no target".into() };
        let Ok(d) = evdev::Device::open(&p) else { return "target unreadable".into() };
        match d.get_abs_state() {
            Ok(a) => format!("target ABS_X {} ABS_Y {} HAT0X {}", a[0].value, a[1].value, a[0x10].value),
            Err(e) => format!("target abs: {e}"),
        }
    }

    fn mode(up: bool) {
        println!("   {} ms  mode -> {}", T0.get().unwrap().elapsed().as_millis(), if up { "Always (Rare surface up)" } else { "Pass" });
        SURFACE.store(up, Ordering::Relaxed);
        rare_input::ip::surface(up);
        // set_mode runs on its own thread; give InputPlumber the switch.
        std::thread::sleep(Duration::from_millis(150));
    }

    /// The launcher's input loop (rare-render input.rs linux::spawn), recording what it emits.
    fn pipeline(log: Arc<Mutex<Vec<Got>>>) {
        enum Msg {
            Pad(Raw),
            Ip(IpOut),
            Reset,
        }
        let (tx, rx) = channel::<Msg>();
        let (rtx, rrx) = channel::<Raw>();
        rare_input::evdev::spawn(rtx);
        {
            let tx = tx.clone();
            std::thread::spawn(move || {
                while let Ok(r) = rrx.recv() {
                    if tx.send(Msg::Pad(r)).is_err() {
                        return;
                    }
                }
            });
        }
        {
            let tx = Mutex::new(tx.clone());
            rare_input::reset::on_request(move || {
                let _ = tx.lock().unwrap().send(Msg::Reset);
            });
        }
        let tx = Mutex::new(tx);
        rare_input::ip::start(
            move |o| {
                let _ = tx.lock().unwrap().send(Msg::Ip(o));
            },
            || SURFACE.load(Ordering::Relaxed),
        );
        std::thread::spawn(move || {
            let mut m = Machine::new();
            m.debug = true;
            m.set_probe(rare_input::evdev::probe);
            let mut out = Vec::new();
            let push = |g: Got| {
                println!("{:>6} ms  {:<3} {:<5} {}{}", g.at.duration_since(*T0.get().unwrap()).as_millis(), g.path, g.pad.name(), if g.release { "release" } else { "press" }, if g.repeat { " (repeat)" } else { "" });
                log.lock().unwrap().push(g);
            };
            loop {
                let msg = match m.due() {
                    Some(d) => rx.recv_timeout(d.saturating_duration_since(Instant::now())),
                    None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                };
                let _t = mono_ns();
                match msg {
                    Ok(Msg::Pad(r)) => m.on(r.src, r.ev, r.swaps, Instant::now(), false, &mut out),
                    Ok(Msg::Ip(IpOut::Press(pad, repeat))) => push(Got { at: Instant::now(), path: "ip", pad, repeat, release: false }),
                    Ok(Msg::Ip(IpOut::Release(pad))) => push(Got { at: Instant::now(), path: "ip", pad, repeat: false, release: true }),
                    Ok(Msg::Ip(_)) => {}
                    Ok(Msg::Reset) => m.reset(Instant::now(), &mut out),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
                m.tick(Instant::now(), &mut out);
                for o in out.drain(..) {
                    match o {
                        Out::Press(pad, repeat) => push(Got { at: Instant::now(), path: "pad", pad, repeat, release: false }),
                        Out::Release(pad) => push(Got { at: Instant::now(), path: "pad", pad, repeat: false, release: true }),
                        _ => {}
                    }
                }
            }
        });
    }

    fn after(log: &Arc<Mutex<Vec<Got>>>, t: Instant) -> Vec<Got> {
        log.lock().unwrap().iter().filter(|g| g.at >= t && !g.release).cloned().collect()
    }

    fn scenario(name: &str, log: &Arc<Mutex<Vec<Got>>>, run: impl FnOnce() -> Instant) -> bool {
        println!("\n== {name}");
        let let_go = run();
        println!("   let go at {} ms", let_go.duration_since(*T0.get().unwrap()).as_millis());
        std::thread::sleep(Duration::from_millis(1500));
        let late = after(log, let_go + Duration::from_millis(60));
        let ok = late.is_empty();
        println!("-- {name}: {} presses after letting go ({}), {}", late.len(), if ok { "ok" } else { "STUCK" }, target_abs());
        ok
    }

    pub fn main() {
        T0.get_or_init(Instant::now);
        let mut src = source();
        // InputPlumber picks the source pad up and makes the composite device and its target.
        let t = Instant::now();
        while target_path().is_none() && t.elapsed() < Duration::from_secs(15) {
            std::thread::sleep(Duration::from_millis(200));
        }
        if target_path().is_none() {
            eprintln!("InputPlumber made no target pad for {SOURCE}");
            std::process::exit(2);
        }
        std::thread::sleep(Duration::from_millis(800));
        let log: Arc<Mutex<Vec<Got>>> = Arc::default();
        pipeline(log.clone());
        std::thread::sleep(Duration::from_millis(2500));
        mode(false);
        let mut ok = true;

        ok &= scenario("stick left held over D-Bus (Always), back to Pass mid-hold, then let go", &log, || {
            mode(true);
            hold(&mut src, 0, -STICK, 900);
            mode(false);
            hold(&mut src, 0, -STICK, 300);
            abs(&mut src, 0, 0);
            Instant::now()
        });
        ok &= scenario("stick left held over evdev (Pass), to Always mid-hold, then let go", &log, || {
            mode(false);
            hold(&mut src, 0, -STICK, 900);
            mode(true);
            hold(&mut src, 0, -STICK, 300);
            abs(&mut src, 0, 0);
            let at = Instant::now();
            mode(false);
            at
        });
        ok &= scenario("d-pad left held over D-Bus, back to Pass mid-hold, then let go", &log, || {
            mode(true);
            abs(&mut src, 0x10, -1);
            std::thread::sleep(Duration::from_millis(900));
            mode(false);
            std::thread::sleep(Duration::from_millis(300));
            abs(&mut src, 0x10, 0);
            Instant::now()
        });
        ok &= scenario("stick left over D-Bus, Pass while held, let go, then push right", &log, || {
            mode(true);
            hold(&mut src, 0, -STICK, 700);
            mode(false);
            abs(&mut src, 0, 0);
            let at = Instant::now();
            std::thread::sleep(Duration::from_millis(400));
            hold(&mut src, 0, STICK, 200);
            abs(&mut src, 0, 0);
            at + Duration::from_millis(1000)
        });
        ok &= scenario("game exit: Quit from the Rare menu, back to Rare, stick pushed while the mode is still Always", &log, || {
            mode(true);
            std::thread::sleep(Duration::from_millis(300));
            // The menu closes with the game (the surface flag only, as the native launcher did):
            // InputPlumber stays in Always until the 5 s reconcile, and the stick is used at once.
            SURFACE.store(false, Ordering::Relaxed);
            hold(&mut src, 0, -STICK, 6000);
            abs(&mut src, 0, 0);
            Instant::now()
        });
        ok &= scenario("game exit: the game's virtual pad goes while its stick was left", &log, || {
            mode(false);
            let mut g = game_pad();
            std::thread::sleep(Duration::from_millis(2500));
            hold(&mut g, 0, -STICK, 600);
            abs(&mut g, 0, -STICK);
            std::thread::sleep(Duration::from_millis(1600));
            drop(g);
            std::thread::sleep(Duration::from_millis(500));
            let at = Instant::now();
            // Back in Rare, the stick at rest wobbles on one axis only.
            for v in [300, -300, 200, 0] {
                abs(&mut src, 1, v);
                std::thread::sleep(Duration::from_millis(40));
            }
            at
        });
        mode(false);
        println!("\n{}", if ok { "PASS: nothing stuck" } else { "FAIL: a direction kept repeating after it was let go" });
        std::process::exit(if ok { 0 } else { 1 });
    }
}
