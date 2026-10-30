//! Holds a finger on the virtual touchscreen for a few seconds (a check that Android sees it):
//!   touch_probe <package> <x> <y> [seconds]
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let pkg = a.first().cloned().unwrap_or_else(|| "probe".into());
    let x: f32 = a.get(1).and_then(|v| v.parse().ok()).unwrap_or(0.5);
    let y: f32 = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(0.5);
    let secs: u64 = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(3);
    let mut m = rare_android::touch::Map::new();
    m.insert("a".into(), [x, y]);
    rare_android::touch::set_map(&pkg, m).unwrap();
    assert!(rare_android::touch::begin(&pkg), "no virtual touchscreen");
    std::thread::sleep(std::time::Duration::from_secs(secs));
    rare_android::touch::press("a", true);
    std::thread::sleep(std::time::Duration::from_millis(150));
    rare_android::touch::press("a", false);
    std::thread::sleep(std::time::Duration::from_secs(secs));
    rare_android::touch::end();
    rare_android::touch::set_map(&pkg, Default::default()).unwrap();
    println!("touched {x},{y}");
}
