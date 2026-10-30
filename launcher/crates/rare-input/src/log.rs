//! The input log. RARE_DEBUG_INPUT=1, or a file $XDG_RUNTIME_DIR/rare-debug-input (for a launcher
//! Steam started, where env can't be set; checked every few seconds), logs every pad event and
//! decision.

fn runtime_file(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|d| std::path::PathBuf::from(d).join(name))
}

pub fn debug_input() -> bool {
    std::env::var("RARE_DEBUG_INPUT").as_deref() == Ok("1") || runtime_file("rare-debug-input").is_some_and(|f| f.exists())
}

/// An input log line: the journal (stderr), and $XDG_RUNTIME_DIR/rare-launcher-debug.log while the
/// rare-debug-input file is there (Steam keeps a shortcut's stderr to itself).
pub fn log(msg: &str) {
    eprintln!("{msg}");
    if runtime_file("rare-debug-input").is_some_and(|f| f.exists()) {
        if let Some(f) = runtime_file("rare-launcher-debug.log") {
            use std::io::Write;
            if let Ok(mut h) = std::fs::OpenOptions::new().create(true).append(true).open(f) {
                let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
                let _ = writeln!(h, "{ms} {msg}");
            }
        }
    }
}
