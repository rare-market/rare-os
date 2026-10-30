//! Thin wrapper: the code lives in the rare-system crate (launcher/crates/rare-system). Only the
//! Wayland main window's sizing (GTK, via Tauri) stays here.

pub use rare_system::render::*;

/// The Wayland main window: no frame, at the output's size (gamescope scales an xdg window's
/// geometry to fit the output, so anything else is letterboxed and resampled). The window was
/// created decorated (tauri.conf.json, for the desktop); GTK 3 then keeps subtracting the frame's
/// shadow (2 x 26 px) from the first resize after `set_decorated(false)`, so 1920x1200 came out
/// as 1868x1148 on the SuiPlay. A second resize lands exactly, so it resizes until GTK reports
/// the output's size. Returns that size.
#[cfg(target_os = "linux")]
pub fn fit_output(win: &tauri::WebviewWindow) -> Option<(u32, u32)> {
    let (w, h) = output_size()?;
    let win2 = win.clone();
    let _ = win.run_on_main_thread(move || {
        use gtk::prelude::*;
        let Ok(g) = win2.gtk_window() else { return };
        g.set_decorated(false);
        g.resize(w as i32, h as i32);
        let mut tries = 0;
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            let (cw, ch) = g.size();
            if (cw, ch) == (w as i32, h as i32) || tries >= 5 {
                eprintln!("[startup] Wayland window {cw}x{ch} after {tries} extra resize(s)");
                return gtk::glib::ControlFlow::Break;
            }
            tries += 1;
            g.resize(w as i32, h as i32);
            gtk::glib::ControlFlow::Continue
        });
    });
    Some((w, h))
}

#[cfg(not(target_os = "linux"))]
pub fn fit_output(_win: &tauri::WebviewWindow) -> Option<(u32, u32)> {
    None
}

