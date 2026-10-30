use super::*;

fn tmp() -> PathBuf {
    let d = std::env::temp_dir().join(format!("rare-journey-{}-{}", std::process::id(), mono_ns()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn log_lines(d: &Path) -> Vec<String> {
    std::fs::read_to_string(d.join("rare-journey.log"))
        .unwrap_or_default()
        .lines()
        .map(|l| l.split_once(' ').expect("stamp").1.to_string())
        .collect()
}

const HOME: Screen = Screen { tab: "home", overlay: None, running: 0, launching: None };

#[test]
fn lines_match_journey_rs() {
    assert_eq!(line(42, "screen tab=home\nx"), "42 screen tab=home x\n");
    assert_eq!(line(1, &"a".repeat(1000)).len(), 2 + 400 + 1);
}

#[test]
fn screen_and_focus_lines_match_the_web() {
    assert_eq!(HOME.line(), "screen tab=home overlay=- running=0 launching=-");
    let g = Screen { tab: "library", overlay: Some(("game", Some("steam:1145350"))), running: 1, launching: Some("steam:1145350") };
    assert_eq!(g.line(), "screen tab=library overlay=game:steam:1145350 running=1 launching=steam:1145350");
    assert_eq!(Screen { overlay: Some(("raremenu", None)), ..HOME }.line(), "screen tab=home overlay=raremenu running=0 launching=-");
    let f = Focus { tag: "button", group: Some("home-strip"), text: "  Hades\n II  ", rect: (10.4, 20.6, 300.0, 400.2), nothing: false };
    assert_eq!(f.describe(), "button@home-strip \"Hades II\" at 10,21 300x400");
    assert_eq!(Focus::nothing().describe(), "(nothing)");
    // The runner's parser (jlib/analyze.py parse_screen) splits on spaces and '='.
    let gl = g.line();
    let kv: Vec<(&str, &str)> = gl[7..].split(' ').map(|kv| kv.split_once('=').unwrap()).collect();
    assert_eq!(kv[1], ("overlay", "game:steam:1145350"));
}

#[test]
fn commands_parse_like_journey_ts() {
    assert_eq!(Cmd::parse("tab:library"), Cmd::Tab("library".into()));
    assert_eq!(Cmd::parse("open:game:steam:1145350"), Cmd::OpenGame("steam:1145350".into()));
    assert_eq!(Cmd::parse("open:app:youtube"), Cmd::OpenApp("youtube".into()));
    assert_eq!(Cmd::parse("open:qam"), Cmd::Open("qam".into()));
    assert_eq!(Cmd::parse("open:nope"), Cmd::Unknown("open:nope".into()));
    assert_eq!(Cmd::parse("close"), Cmd::Close);
    assert_eq!(Cmd::parse("quit:all"), Cmd::Quit("all".into()));
    assert_eq!(Cmd::parse("state "), Cmd::State);
    assert_eq!(Cmd::parse("echo:hi there"), Cmd::Echo("hi there".into()));
}

#[test]
fn off_without_the_flag_and_no_files_touched() {
    let d = tmp();
    let mut p = Probe::with_dir(Some(d.clone()));
    let t = Instant::now();
    assert!(!p.pad("a", false, t));
    p.screen(&HOME);
    p.log("x");
    assert!(p.poll(t).is_empty());
    assert!(!d.join("rare-journey.log").exists());
    // A command file is left alone while off.
    std::fs::write(d.join("rare-journey.cmd"), "close\n").unwrap();
    assert!(p.poll(t + Duration::from_secs(5)).is_empty());
    assert!(d.join("rare-journey.cmd").exists());
    // No XDG_RUNTIME_DIR: never on.
    let mut none = Probe::with_dir(None);
    assert!(!none.pad("a", false, t));
}

#[test]
fn turns_on_at_a_press_checks_at_most_every_2_s_and_off_when_the_flag_goes() {
    let d = tmp();
    let mut p = Probe::with_dir(Some(d.clone()));
    let t0 = Instant::now();
    assert!(!p.pad("b", false, t0)); // checked: no flag
    std::fs::write(d.join("rare-journey"), "tools/journeys\n").unwrap();
    assert!(!p.pad("b", false, t0 + Duration::from_millis(500)), "within 2 s of the last check");
    assert!(p.pad("b", false, t0 + Duration::from_millis(2100)));
    assert!(p.on());
    p.state(&HOME, &Focus::nothing());
    p.pad("rb", false, t0 + Duration::from_millis(2200));
    p.pad("right", true, t0 + Duration::from_millis(2300));
    let lib = Screen { tab: "library", ..HOME };
    p.screen(&lib);
    p.screen(&lib); // unchanged: no line
    p.presented();
    p.presented(); // painted once per change
    p.toast("error", "Could not start");
    p.ingame(Some("panel"), Some("steam:1"));
    assert_eq!(
        log_lines(&d),
        vec![
            "probe on",
            "screen tab=home overlay=- running=0 launching=-",
            "focus (nothing)",
            "pad rb",
            "pad right repeat",
            "screen tab=library overlay=- running=0 launching=-",
            "painted",
            "toast error Could not start",
            "ingame mode=panel game=steam:1",
        ]
    );
    std::fs::remove_file(d.join("rare-journey")).unwrap();
    assert!(p.poll(t0 + Duration::from_millis(4500)).is_empty());
    assert!(!p.on());
}

#[test]
fn commands_are_read_once_in_order_and_answered() {
    let d = tmp();
    std::fs::write(d.join("rare-journey"), "x").unwrap();
    let mut p = Probe::with_dir(Some(d.clone()));
    let t0 = Instant::now();
    assert!(p.pad("b", false, t0));
    // The runner queues: a command written while another is unread follows it on the next line.
    std::fs::write(d.join("rare-journey.cmd"), "close\ntab:home\necho:ping\nbogus\n").unwrap();
    let c = p.poll(t0 + Duration::from_millis(300));
    assert_eq!(c, vec![Cmd::Close, Cmd::Tab("home".into()), Cmd::Echo("ping".into()), Cmd::Unknown("bogus".into())]);
    assert!(!d.join("rare-journey.cmd").exists());
    assert!(p.poll(t0 + Duration::from_millis(400)).is_empty(), "250 ms between polls");
    p.after_commands(&HOME, &Focus::nothing());
    // The runner's confirmed-Home check: a fresh screen line after "state", even when unchanged.
    std::fs::write(d.join("rare-journey.cmd"), "state\n").unwrap();
    assert_eq!(p.poll(t0 + Duration::from_millis(700)), vec![Cmd::State]);
    p.after_commands(&HOME, &Focus::nothing());
    let l = log_lines(&d);
    assert_eq!(l.iter().filter(|x| x.starts_with("screen tab=home")).count(), 2);
    assert!(l.contains(&"echo ping".to_string()) && l.contains(&"unknown command bogus".to_string()));
}

#[test]
fn stamps_are_monotonic_ns() {
    let d = tmp();
    std::fs::write(d.join("rare-journey"), "x").unwrap();
    let mut p = Probe::with_dir(Some(d.clone()));
    let before = mono_ns();
    p.pad("a", false, Instant::now());
    let t: u64 = std::fs::read_to_string(d.join("rare-journey.log")).unwrap().split(' ').next().unwrap().parse().unwrap();
    assert!(t >= before && t <= mono_ns());
}
