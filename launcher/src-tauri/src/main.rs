// Prevents an extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `rare-launcher --scan-library` prints what the library scan finds, as JSON, and exits.
    if std::env::args().any(|a| a == "--scan-library") {
        rare_launcher_lib::print_scan();
        return;
    }
    // Under gamescope: how WebKitGTK draws (RARE_WEBKIT, GPU by default; render.rs). Before any
    // thread or GTK starts.
    rare_launcher_lib::render_prepare();
    rare_launcher_lib::run()
}
