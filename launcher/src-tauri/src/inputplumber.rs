//! The SuiPlay's four extra buttons and InputPlumber's intercept mode. Thin wrapper: the D-Bus listener, the press and hold state and the
//! intercept mode live in the rare-input crate (launcher/crates/rare-input/src/ip.rs); this module
//! sends what it decides into the launcher (pad presses, LC / RC, volume keys).

pub use rare_input::ip::{controls_support, surface, ControlSupport, Out};
use tauri::AppHandle;

fn apply(app: &AppHandle, o: Out) {
    match o {
        Out::Press(p, repeat) => crate::gamepad::press(app, p, repeat),
        Out::Release(p) => crate::gamepad::release_pad(app, p),
        Out::Side(k, down) => crate::sysbuttons::press(app, k, down),
        Out::Volume(k) => crate::syskeys::key(k),
    }
}

pub fn start(app: AppHandle) {
    let a = app.clone();
    rare_input::ip::start(move |o| apply(&a, o), crate::overlay::is_open);
}

#[tauri::command]
pub fn input_controls() -> Vec<ControlSupport> {
    controls_support()
}
